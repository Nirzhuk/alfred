use super::{
    normalize_tool_result, AlfredToolExecutionContext, AlfredToolExecutor, AlfredToolKind,
    AlfredToolRequest, AlfredToolResult, AlfredToolStatus, NativeAgentRuntime,
    NativeCancellation, NativeErrorCode, NativeLookupAvailability, NativeRuntimeError,
    TOOL_CONTRACT_VERSION,
};
use crate::db::agent_memory_lookup::{
    AgentMemoryLookupService, AgentMemoryLookupSession, LookupAbort, LookupCallControl,
    LookupContractError, LookupServiceError, LookupToolName, LookupTrace, LOOKUP_LIMITS_V1,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeMemoryLookupTrace {
    pub turn_id: String,
    pub run_id: String,
    pub node_id: String,
    pub request_id: String,
    pub retrieved_bytes: usize,
    pub delivery: NativeMemoryLookupDelivery,
    pub trace: LookupTrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMemoryLookupDelivery {
    Delivered,
    RejectedNativeOutputLimit,
    RejectedContextBudget,
    RejectedInvalidJson,
    RejectedCancelled,
    RejectedTimedOut,
}

pub trait MemoryLookupTraceSink: Send + Sync {
    fn record(&self, trace: &NativeMemoryLookupTrace) -> Result<(), NativeRuntimeError>;
}

/// A host-owned, runtime-specific remaining-context accounting hook.
///
/// Implementations must return `None` when the runtime cannot prove its
/// remaining capacity. `reserve_bytes` is called before every lookup and may
/// reserve fewer bytes than requested when the verified allowance is smaller.
/// The executor releases any unused reservation after validation, including
/// on every error path.
pub trait MemoryLookupContextAllowance: Send + Sync {
    /// Pins the runtime ledger selected for this native turn.
    ///
    /// The default is suitable for an allowance that is already scoped to the
    /// executor. Registry-backed adapters override it to retain the exact
    /// runtime ledger across every reservation and release.
    fn bind_turn(
        &self,
        _context: &AlfredToolExecutionContext,
    ) -> Result<(), NativeRuntimeError> {
        Ok(())
    }
    fn bind_runtime(
        &self,
        _context: &AlfredToolExecutionContext,
        _runtime: Arc<dyn NativeAgentRuntime>,
    ) -> Result<(), NativeRuntimeError> {
        Ok(())
    }
    /// Reports whether this ledger can prove a usable remaining-context
    /// allowance without fabricating a model window or probing a provider.
    /// The default is fail-closed; runtime-owned ledgers must override it when
    /// they can provide this proof for the selected runtime.
    fn availability(&self) -> NativeLookupAvailability {
        NativeLookupAvailability::unavailable(
            "native runtime has no verified remaining-context allowance",
        )
    }

    fn remaining_bytes(&self, context: &AlfredToolExecutionContext) -> Option<usize>;

    fn reserve_bytes(
        &self,
        context: &AlfredToolExecutionContext,
        requested_bytes: usize,
    ) -> Result<usize, NativeRuntimeError>;

    fn release_bytes(&self, context: &AlfredToolExecutionContext, unused_bytes: usize);

    fn finish_turn(&self, _context: &AlfredToolExecutionContext) {}
}


struct BoundLookupTurn {
    provider: crate::agents::AgentProvider,
    model: String,
    run_id: String,
    node_id: String,
    working_directory: std::path::PathBuf,
    session: Arc<AgentMemoryLookupSession>,
    remaining_context_bytes: Mutex<usize>,
    call_gate: Mutex<()>,
    pending_traces: Mutex<HashMap<String, NativeMemoryLookupTrace>>,
    seen_request_ids: Mutex<HashSet<String>>,
}

pub struct MemoryLookupToolExecutor {
    service: Arc<AgentMemoryLookupService>,
    fallback: Arc<dyn AlfredToolExecutor>,
    trace_sink: Arc<dyn MemoryLookupTraceSink>,
    context_allowance: Arc<dyn MemoryLookupContextAllowance>,
    turns: Mutex<HashMap<String, Arc<BoundLookupTurn>>>,
}

impl MemoryLookupToolExecutor {
    pub fn new(
        service: Arc<AgentMemoryLookupService>,
        fallback: Arc<dyn AlfredToolExecutor>,
        trace_sink: Arc<dyn MemoryLookupTraceSink>,
        context_allowance: Arc<dyn MemoryLookupContextAllowance>,
    ) -> Self {
        Self {
            service,
            fallback,
            trace_sink,
            context_allowance,
            turns: Mutex::new(HashMap::new()),
        }
    }

    fn bound_for_context(
        &self,
        context: &AlfredToolExecutionContext,
    ) -> Result<Arc<BoundLookupTurn>, NativeRuntimeError> {
        let turns = self.turns.lock().map_err(|_| executor_unavailable())?;
        let bound = turns.get(&context.turn_id).ok_or_else(|| {
            native_error(
                NativeErrorCode::PermissionDenied,
                "memory lookup turn context is unavailable",
                false,
            )
        })?;
        if bound.provider != context.provider
            || bound.model != context.model
            || bound.run_id != context.run_id
            || bound.node_id != context.node_id
            || bound.working_directory != context.working_directory
        {
            return Err(native_error(
                NativeErrorCode::PermissionDenied,
                "memory lookup turn context does not match its trusted binding",
                false,
            ));
        }
        Ok(bound.clone())
    }

    fn record_trace(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        mut trace: LookupTrace,
        delivery: NativeMemoryLookupDelivery,
        delivered_bytes: usize,
    ) -> Result<(), NativeRuntimeError> {
        let retrieved_bytes = trace.returned_bytes;
        trace.returned_bytes = delivered_bytes;
        self.trace_sink.record(&NativeMemoryLookupTrace {
            turn_id: context.turn_id.clone(),
            run_id: context.run_id.clone(),
            node_id: context.node_id.clone(),
            request_id: request.request_id.clone(),
            retrieved_bytes,
            delivery,
            trace,
        })
    }

    fn stage_delivered_trace(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        mut trace: LookupTrace,
        delivered_bytes: usize,
        bound: &BoundLookupTurn,
    ) -> Result<(), NativeRuntimeError> {
        let retrieved_bytes = trace.returned_bytes;
        trace.returned_bytes = delivered_bytes;
        let native_trace = NativeMemoryLookupTrace {
            turn_id: context.turn_id.clone(),
            run_id: context.run_id.clone(),
            node_id: context.node_id.clone(),
            request_id: request.request_id.clone(),
            retrieved_bytes,
            delivery: NativeMemoryLookupDelivery::Delivered,
            trace,
        };
        let mut pending = bound
            .pending_traces
            .lock()
            .map_err(|_| executor_unavailable())?;
        if pending.contains_key(&request.request_id) {
            return Err(native_error(
                NativeErrorCode::InvalidRequest,
                "memory lookup request identity was reused in one native turn",
                false,
            ));
        }
        pending.insert(request.request_id.clone(), native_trace);
        Ok(())
    }

    fn reject_staged_trace(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        error: &NativeRuntimeError,
    ) -> Result<(), NativeRuntimeError> {
        let delivery = match error.code {
            NativeErrorCode::Cancelled => NativeMemoryLookupDelivery::RejectedCancelled,
            NativeErrorCode::TimedOut | NativeErrorCode::ToolTimeout => {
                NativeMemoryLookupDelivery::RejectedTimedOut
            }
            _ => return Ok(()),
        };
        let bound = self.bound_for_context(context)?;
        let Some(mut trace) = bound
            .pending_traces
            .lock()
            .map_err(|_| executor_unavailable())?
            .remove(&request.request_id)
        else {
            return Ok(());
        };
        bound
            .session
            .release_returned_bytes(trace.retrieved_bytes)
            .map_err(map_lookup_error)?;
        let returned_bytes = trace.trace.returned_bytes;
        if returned_bytes > 0 {
            let mut remaining = bound
                .remaining_context_bytes
                .lock()
                .map_err(|_| executor_unavailable())?;
            *remaining = (*remaining).saturating_add(returned_bytes);
            self.context_allowance
                .release_bytes(context, returned_bytes);
        }
        trace.trace.returned_bytes = 0;
        trace.delivery = delivery;
        self.trace_sink.record(&trace)
    }

    fn execute_lookup(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        if !context.memory_lookup_enabled {
            return Err(native_error(
                NativeErrorCode::PermissionDenied,
                "memory lookup is disabled for this native turn",
                false,
            ));
        }
        let bound = self.bound_for_context(context)?;
        let first_use = bound
            .seen_request_ids
            .lock()
            .map_err(|_| executor_unavailable())?
            .insert(request.request_id.clone());
        if !first_use {
            return Err(native_error(
                NativeErrorCode::InvalidRequest,
                "memory lookup request identity was reused in one native turn",
                false,
            ));
        }
        if request.path.is_some()
            || !request.arguments.is_empty()
            || request.timeout_ms == 0
            || request.timeout_ms > LOOKUP_LIMITS_V1.read_deadline_ms
        {
            return Err(native_error(
                NativeErrorCode::InvalidRequest,
                "memory lookup tool framing is invalid",
                false,
            ));
        }
        let tool = lookup_tool_name(&request.name)?;
        let encoded_request =
            serde_json::to_vec(&Value::Object(request.input.clone())).map_err(|_| {
                native_error(
                    NativeErrorCode::InvalidRequest,
                    "memory lookup input is invalid",
                    false,
                )
            })?;
        let control = NativeLookupCallControl::new(cancellation, request.timeout_ms);
        // Serialize lookups per turn so `available` cannot go stale while a
        // concurrent call spends the same remaining context allowance.
        let _call = lock_call_gate(&bound.call_gate, &control)?;
        let available = *bound
            .remaining_context_bytes
            .lock()
            .map_err(|_| executor_unavailable())?;
        let requested_reservation = available
            .min(LOOKUP_LIMITS_V1.get_response_bytes)
            .min(request.max_output_bytes);
        if requested_reservation == 0 {
            return Err(native_error(
                NativeErrorCode::ToolOutputLimitExceeded,
                "no verified native context capacity remains for memory lookup",
                false,
            ));
        }
        let reserved = self
            .context_allowance
            .reserve_bytes(context, requested_reservation)?;
        if reserved == 0 || reserved > requested_reservation {
            if reserved > 0 {
                self.context_allowance.release_bytes(context, reserved);
            }
            return Err(native_error(
                NativeErrorCode::ToolOutputLimitExceeded,
                "native context allowance could not reserve a bounded lookup result",
                false,
            ));
        }
        let (output, trace) = match bound
            .session
            .execute_json_with_trace(tool, &encoded_request, &control)
        {
            Ok(result) => result,
            Err(error) => {
                self.context_allowance.release_bytes(context, reserved);
                return Err(map_lookup_error(error));
            }
        };
        let result = AlfredToolResult {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: request.request_id.clone(),
            status: AlfredToolStatus::Completed,
            output,
            exit_code: None,
            truncated: false,
            metadata: serde_json::Map::new(),
        };
        let normalized = match normalize_tool_result(result, request) {
            Ok(result) => result,
            Err(error) => {
                self.context_allowance.release_bytes(context, reserved);
                bound
                    .session
                    .release_returned_bytes(trace.returned_bytes)
                    .map_err(map_lookup_error)?;
                return Err(error);
            }
        };
        if normalized.truncated {
            self.context_allowance.release_bytes(context, reserved);
            bound
                .session
                .release_returned_bytes(trace.returned_bytes)
                .map_err(map_lookup_error)?;
            self.record_trace(
                context,
                request,
                trace,
                NativeMemoryLookupDelivery::RejectedNativeOutputLimit,
                0,
            )?;
            return Err(native_error(
                NativeErrorCode::ToolOutputLimitExceeded,
                "memory lookup result exceeds the requested native tool limit",
                false,
            ));
        }
        if serde_json::from_str::<Value>(&normalized.output).is_err() {
            self.context_allowance.release_bytes(context, reserved);
            bound
                .session
                .release_returned_bytes(trace.returned_bytes)
                .map_err(map_lookup_error)?;
            self.record_trace(
                context,
                request,
                trace,
                NativeMemoryLookupDelivery::RejectedInvalidJson,
                0,
            )?;
            return Err(native_error(
                NativeErrorCode::InvalidRequest,
                "normalized memory lookup result is not valid JSON",
                false,
            ));
        }
        let delivered_bytes = normalized.output.len();
        if delivered_bytes > reserved || delivered_bytes > available {
            self.context_allowance.release_bytes(context, reserved);
            bound
                .session
                .release_returned_bytes(trace.returned_bytes)
                .map_err(map_lookup_error)?;
            self.record_trace(
                context,
                request,
                trace,
                NativeMemoryLookupDelivery::RejectedContextBudget,
                0,
            )?;
            return Err(native_error(
                NativeErrorCode::ToolOutputLimitExceeded,
                "memory lookup result exceeds the verified remaining context capacity",
                false,
            ));
        }
        if let Err(error) =
            self.stage_delivered_trace(context, request, trace, delivered_bytes, &bound)
        {
            self.context_allowance.release_bytes(context, reserved);
            return Err(error);
        }
        *bound
            .remaining_context_bytes
            .lock()
            .map_err(|_| executor_unavailable())? -= delivered_bytes;
        self.context_allowance
            .release_bytes(context, reserved.saturating_sub(delivered_bytes));
        Ok(normalized)
    }

    #[cfg(test)]
    pub(crate) fn active_turns(&self) -> usize {
        self.turns.lock().unwrap().len()
    }
}

impl AlfredToolExecutor for MemoryLookupToolExecutor {
    fn supports_memory_lookup(&self) -> bool {
        true
    }
    fn bind_runtime(
        &self,
        context: &AlfredToolExecutionContext,
        runtime: Arc<dyn NativeAgentRuntime>,
    ) -> Result<(), NativeRuntimeError> {
        self.fallback
            .bind_runtime(context, Arc::clone(&runtime))?;
        if !context.memory_lookup_enabled {
            return Ok(());
        }
        self.context_allowance.bind_runtime(context, runtime)
    }
    fn begin_turn(
        &self,
        context: &AlfredToolExecutionContext,
        cancellation: &NativeCancellation,
    ) -> Result<(), NativeRuntimeError> {
        // Reject a repeated turn before binding anything: the allowance and
        // fallback state for this identity belong to the live turn.
        if context.memory_lookup_enabled
            && self
                .turns
                .lock()
                .map_err(|_| executor_unavailable())?
                .contains_key(&context.turn_id)
        {
            return Err(duplicate_turn());
        }
        self.fallback.begin_turn(context, cancellation)?;
        if !context.memory_lookup_enabled {
            return Ok(());
        }
        if let Err(error) = self.context_allowance.bind_turn(context) {
            self.fallback.finish_turn(context);
            return Err(error);
        }
        if let Err(error) = cancellation.checkpoint() {
            self.context_allowance.finish_turn(context);
            self.fallback.finish_turn(context);
            return Err(error);
        }
        let Some(remaining_context_bytes) = self.context_allowance.remaining_bytes(context) else {
            self.context_allowance.finish_turn(context);
            self.fallback.finish_turn(context);
            return Err(native_error(
                NativeErrorCode::CapabilityUnsupported,
                "memory lookup has no verified native context allowance",
                false,
            ));
        };
        let remaining_context_bytes =
            remaining_context_bytes.min(LOOKUP_LIMITS_V1.returned_bytes_per_step);
        let session = match self.service.start_session(
            &context.run_id,
            &context.node_id,
            Some(&context.working_directory),
        ) {
            Ok(session) => session,
            Err(error) => {
                self.context_allowance.finish_turn(context);
                self.fallback.finish_turn(context);
                return Err(map_lookup_error(error));
            }
        };
        let mut turns = match self.turns.lock() {
            Ok(turns) => turns,
            Err(_) => {
                self.context_allowance.finish_turn(context);
                self.fallback.finish_turn(context);
                return Err(executor_unavailable());
            }
        };
        if turns.contains_key(&context.turn_id) {
            // A concurrent duplicate won the insert; its bindings stay live.
            return Err(duplicate_turn());
        }
        turns.insert(
            context.turn_id.clone(),
            Arc::new(BoundLookupTurn {
                provider: context.provider,
                model: context.model.clone(),
                run_id: context.run_id.clone(),
                node_id: context.node_id.clone(),
                working_directory: context.working_directory.clone(),
                session: Arc::new(session),
                remaining_context_bytes: Mutex::new(remaining_context_bytes),
                call_gate: Mutex::new(()),
                pending_traces: Mutex::new(HashMap::new()),
                seen_request_ids: Mutex::new(HashSet::new()),
            }),
        );
        Ok(())
    }

    fn execute(
        &self,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        if request.kind == AlfredToolKind::MemoryLookup {
            Err(native_error(
                NativeErrorCode::PermissionDenied,
                "memory lookup requires a trusted native turn context",
                false,
            ))
        } else {
            self.fallback.execute(request, cancellation)
        }
    }

    fn execute_for_turn(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        if request.kind == AlfredToolKind::MemoryLookup {
            self.execute_lookup(context, request, cancellation)
        } else {
            self.fallback
                .execute_for_turn(context, request, cancellation)
        }
    }

    fn finalize_for_turn(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        result: &AlfredToolResult,
    ) -> Result<(), NativeRuntimeError> {
        if request.kind != AlfredToolKind::MemoryLookup
            || result.status != AlfredToolStatus::Completed
        {
            return Ok(());
        }
        let bound = self.bound_for_context(context)?;
        let trace = bound
            .pending_traces
            .lock()
            .map_err(|_| executor_unavailable())?
            .remove(&request.request_id)
            .ok_or_else(|| {
                native_error(
                    NativeErrorCode::ProviderUnavailable,
                    "memory lookup trace was not staged for delivery",
                    true,
                )
            })?;
        self.trace_sink.record(&trace)
    }
    fn reject_for_turn(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        error: &NativeRuntimeError,
    ) -> Result<(), NativeRuntimeError> {
        if request.kind == AlfredToolKind::MemoryLookup {
            self.reject_staged_trace(context, request, error)?;
        }
        Ok(())
    }

    fn finish_turn(&self, context: &AlfredToolExecutionContext) {
        if let Ok(mut turns) = self.turns.lock() {
            turns.remove(&context.turn_id);
        }
        self.context_allowance.finish_turn(context);
        self.fallback.finish_turn(context);
    }
}

fn lock_call_gate<'a>(
    gate: &'a Mutex<()>,
    control: &dyn LookupCallControl,
) -> Result<MutexGuard<'a, ()>, NativeRuntimeError> {
    loop {
        control.checkpoint().map_err(|error| match error {
            LookupAbort::Cancelled => NativeRuntimeError::cancelled(),
            LookupAbort::TimedOut => NativeRuntimeError::timed_out(),
        })?;
        match gate.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => std::thread::yield_now(),
            Err(TryLockError::Poisoned(_)) => return Err(executor_unavailable()),
        }
    }
}

struct NativeLookupCallControl<'a> {
    cancellation: &'a NativeCancellation,
    deadline: Instant,
}

impl<'a> NativeLookupCallControl<'a> {
    fn new(cancellation: &'a NativeCancellation, timeout_ms: u64) -> Self {
        Self {
            cancellation,
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
        }
    }
}

impl LookupCallControl for NativeLookupCallControl<'_> {
    fn checkpoint(&self) -> Result<(), LookupAbort> {
        if self.cancellation.is_cancelled() {
            Err(LookupAbort::Cancelled)
        } else if self.cancellation.is_timed_out() || Instant::now() >= self.deadline {
            Err(LookupAbort::TimedOut)
        } else {
            Ok(())
        }
    }
}

fn lookup_tool_name(name: &str) -> Result<LookupToolName, NativeRuntimeError> {
    match name {
        "memory.search" => Ok(LookupToolName::MemorySearch),
        "memory.get" => Ok(LookupToolName::MemoryGet),
        "history.search" => Ok(LookupToolName::HistorySearch),
        "history.get" => Ok(LookupToolName::HistoryGet),
        _ => Err(native_error(
            NativeErrorCode::CapabilityUnsupported,
            "unknown memory lookup operation",
            false,
        )),
    }
}

fn map_lookup_error(error: LookupServiceError) -> NativeRuntimeError {
    match error {
        LookupServiceError::Cancelled => NativeRuntimeError::cancelled(),
        LookupServiceError::TimedOut => NativeRuntimeError::timed_out(),
        LookupServiceError::InvalidScope => native_error(
            NativeErrorCode::PermissionDenied,
            "memory lookup scope is no longer authorized",
            false,
        ),
        LookupServiceError::ArtifactUnavailable
        | LookupServiceError::Plan050IntegrationRequired
        | LookupServiceError::StorageUnavailable => executor_unavailable(),
        LookupServiceError::Contract(
            LookupContractError::ResponseTooLarge | LookupContractError::StepByteBudgetExhausted,
        ) => native_error(
            NativeErrorCode::ToolOutputLimitExceeded,
            "memory lookup output budget is exhausted",
            false,
        ),
        LookupServiceError::Contract(error) => native_error(
            NativeErrorCode::InvalidRequest,
            format!("memory lookup request rejected: {error}"),
            false,
        ),
    }
}

fn duplicate_turn() -> NativeRuntimeError {
    native_error(
        NativeErrorCode::InvalidRequest,
        "native tool turn identity is already active",
        false,
    )
}

fn executor_unavailable() -> NativeRuntimeError {
    native_error(
        NativeErrorCode::ProviderUnavailable,
        "memory lookup storage is unavailable",
        true,
    )
}

fn native_error(
    code: NativeErrorCode,
    message: impl Into<String>,
    retryable: bool,
) -> NativeRuntimeError {
    NativeRuntimeError::new(code, message, retryable)
}
