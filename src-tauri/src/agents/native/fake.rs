use super::*;
use crate::agent_accounts::models::AgentProductId;
use crate::agents::{AgentProvider, OpaqueAgentAccountRef};
use crate::db::agent_memory_lookup::LOOKUP_LIMITS_V1;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) struct FakeAccountResolver {
    pub account_ref: OpaqueAgentAccountRef,
    pub provider: AgentProvider,
    pub available: AtomicBool,
}

impl FakeAccountResolver {
    pub fn new(account_ref: OpaqueAgentAccountRef, provider: AgentProvider) -> Self {
        Self {
            account_ref,
            provider,
            available: AtomicBool::new(true),
        }
    }
}

impl NativeAccountResolver for FakeAccountResolver {
    fn resolve(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        provider: AgentProvider,
        product: AgentProductId,
    ) -> Result<ResolvedNativeAccount, NativeRuntimeError> {
        if !self.available.load(Ordering::SeqCst) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "fake account is disconnected",
                false,
            ));
        }
        if account_ref != &self.account_ref
            || provider != self.provider
            || product != fake_product(provider)
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "fake account does not match",
                false,
            ));
        }
        Ok(ResolvedNativeAccount {
            account_ref: account_ref.clone(),
            provider,
            product: fake_product(provider),
            credential: NativeCredential::new("fake-credential-held-in-memory".to_string()),
        })
    }
}

fn fake_product(provider: AgentProvider) -> AgentProductId {
    match provider {
        AgentProvider::ClaudeCode => AgentProductId::ClaudeApi,
        AgentProvider::Cursor => AgentProductId::CursorCloud,
        AgentProvider::Codex => AgentProductId::OpenaiApi,
        AgentProvider::Opencode => AgentProductId::OpencodeZen,
        AgentProvider::GithubCopilot => AgentProductId::GithubCopilotSubscription,
        AgentProvider::Gemini => AgentProductId::GeminiApi,
        AgentProvider::Grok => AgentProductId::GrokApi,
        AgentProvider::Pi | AgentProvider::Omp => {
            panic!("no native product is registered for a CLI-only provider")
        }
    }
}

pub(crate) struct FakeNativeRuntime {
    pub provider: AgentProvider,
    pub request_tool: Mutex<Option<AlfredToolRequest>>,
    pub request_tools: Mutex<Vec<AlfredToolRequest>>,
    pub fail_with: Mutex<Option<NativeRuntimeError>>,
    pub continue_after_tool_error: AtomicBool,
    pub cancelled: AtomicBool,
    pub delay_ms: AtomicU64,
    pub memory_lookup_available: AtomicBool,
    pub observed_memory_lookup: AtomicBool,
    pub memory_lookup_context_allowance: Arc<FakeMemoryLookupContextAllowance>,
}

impl FakeNativeRuntime {
    pub fn new(provider: AgentProvider) -> Self {
        Self {
            provider,
            request_tool: Mutex::new(None),
            request_tools: Mutex::new(Vec::new()),
            fail_with: Mutex::new(None),
            continue_after_tool_error: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            delay_ms: AtomicU64::new(0),
            memory_lookup_available: AtomicBool::new(true),
            observed_memory_lookup: AtomicBool::new(false),
            memory_lookup_context_allowance: Arc::new(FakeMemoryLookupContextAllowance::new(16_000)),
        }
    }

    fn capabilities(&self) -> NativeCapabilities {
        NativeCapabilities {
            supports_api_key: true,
            supports_model_list: true,
            supports_usage: true,
            supports_tool_calls: true,
            supports_approval_events: true,
            supports_native_filesystem: true,
            supports_native_shell: true,
            supports_patch: true,
            supports_memory_lookup: true,
            ..NativeCapabilities::default()
        }
    }
}

impl NativeAgentRuntime for FakeNativeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: "fake-native-runtime".into(),
            runtime_version: "1.0.0".into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: self.provider,
            product: fake_product(self.provider),
            tool_execution_owner: NativeToolExecutionOwner::AlfredExecuted,
            capabilities: self.capabilities(),
        }
    }

    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        if account.provider != self.provider
            || account
                .credential
                .downcast_ref::<String>()
                .map(String::as_str)
                != Some("fake-credential-held-in-memory")
        {
            Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "fake runtime rejected account",
                false,
            ))
        } else {
            Ok(())
        }
    }

    fn discover_models(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        Ok(vec![NativeModel {
            id: "fake-model".into(),
            label: "Fake Model".into(),
        }])
    }
    fn run_turn(
        &self,
        _account: &ResolvedNativeAccount,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        self.observed_memory_lookup.store(
            request.tool_capabilities.memory_lookup,
            Ordering::SeqCst,
        );
        host.cancellation().checkpoint()?;
        let delay_ms = self.delay_ms.load(Ordering::SeqCst);
        for _ in 0..delay_ms {
            std::thread::sleep(std::time::Duration::from_millis(1));
            host.cancellation().checkpoint()?;
        }
        if let Some(error) = self.fail_with.lock().expect("fake failure lock").clone() {
            return Err(error);
        }
        host.emit(NativeEvent::new(0, NativeEventKind::TurnStarted))?;
        let mut tools = self
            .request_tools
            .lock()
            .expect("fake tools lock")
            .clone();
        if tools.is_empty() {
            if let Some(tool) = self.request_tool.lock().expect("fake tool lock").clone() {
                tools.push(tool);
            }
        }
        tools.retain(|tool| {
            request.tool_capabilities.memory_lookup || tool.kind != AlfredToolKind::MemoryLookup
        });
        if tools.is_empty() {
            let mut delta = NativeEvent::new(0, NativeEventKind::AssistantDelta);
            delta.content_class = Some(NativeContentClass::Assistant);
            delta.text = Some("fake response".into());
            host.emit(delta)?;
        } else {
            let mut previous_reference: Option<String> = None;
            let mut previous_cursor: Option<String> = None;
            for mut tool in tools {
                for value in tool.input.values_mut() {
                    if let Value::String(value) = value {
                        match value.as_str() {
                            "$previous_reference" => {
                                if let Some(reference) = previous_reference.as_ref() {
                                    *value = reference.clone();
                                }
                            }
                            "$previous_cursor" => {
                                if let Some(cursor) = previous_cursor.as_ref() {
                                    *value = cursor.clone();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let result = match host.invoke_tool(tool) {
                    Ok(result) => result,
                    Err(_error)
                        if self
                            .continue_after_tool_error
                            .swap(false, Ordering::SeqCst) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                update_lookup_chain_state(
                    &result.output,
                    &mut previous_reference,
                    &mut previous_cursor,
                );
                let text = match result.status {
                    AlfredToolStatus::Denied => "tool denied".to_string(),
                    AlfredToolStatus::Cancelled => "tool cancelled".to_string(),
                    AlfredToolStatus::TimedOut => "tool timed out".to_string(),
                    AlfredToolStatus::Failed => "tool failed".to_string(),
                    AlfredToolStatus::Completed => format!("tool: {}", result.output),
                };
                let mut delta = NativeEvent::new(0, NativeEventKind::AssistantDelta);
                delta.content_class = Some(NativeContentClass::Assistant);
                delta.text = Some(text);
                host.emit(delta)?;
            }
        }
        host.emit(NativeEvent::new(0, NativeEventKind::TurnCompleted))?;
        Ok(NativeTurnOutcome { session_id: None })
    }

    fn memory_lookup_availability(&self) -> NativeLookupAvailability {
        if self
            .memory_lookup_context_allowance
            .unknown
            .load(Ordering::SeqCst)
        {
            NativeLookupAvailability::unavailable(
                "fake runtime cannot verify remaining context",
            )
        } else if self.memory_lookup_available.load(Ordering::SeqCst) {
            NativeLookupAvailability::available()
        } else {
            NativeLookupAvailability::unavailable(
                "fake runtime has been configured without a lookup result transport",
            )
        }
    }
    fn memory_lookup_context_allowance(
        &self,
    ) -> Option<Arc<dyn MemoryLookupContextAllowance>> {
        Some(Arc::clone(&self.memory_lookup_context_allowance)
            as Arc<dyn MemoryLookupContextAllowance>)
    }

    fn cancel(&self, cancellation: &NativeCancellation) -> Result<(), NativeRuntimeError> {
        self.cancelled.store(true, Ordering::SeqCst);
        cancellation.cancel();
        Ok(())
    }

    fn usage_snapshot(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        Ok(NativeUsageSnapshot {
            state: NativeUsageState::Supported,
            input_tokens: Some(7),
            output_tokens: Some(3),
            window_resets_at: None,
        })
    }
}
fn update_lookup_chain_state(
    output: &str,
    previous_reference: &mut Option<String>,
    previous_cursor: &mut Option<String>,
) {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return;
    };
    if let Some(reference) = value
        .pointer("/results/0/reference")
        .and_then(Value::as_str)
        .or_else(|| value.get("reference").and_then(Value::as_str))
    {
        *previous_reference = Some(reference.to_owned());
    }
    *previous_cursor = value
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::to_owned);
}


/// A runtime that declares no model catalog. It still executes turns with an
/// explicitly selected, bounded model.
pub(crate) struct FakeCatalogFreeRuntime {
    pub provider: AgentProvider,
}

impl NativeAgentRuntime for FakeCatalogFreeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: "fake-catalog-free-runtime".into(),
            runtime_version: "1.0.0".into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: self.provider,
            product: fake_product(self.provider),
            tool_execution_owner: NativeToolExecutionOwner::NoTools,
            capabilities: NativeCapabilities {
                supports_api_key: true,
                ..NativeCapabilities::default()
            },
        }
    }

    fn validate_account(&self, _account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        Ok(())
    }

    fn discover_models(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        Err(NativeRuntimeError::new(
            NativeErrorCode::CapabilityUnsupported,
            "this runtime publishes no model catalog",
            false,
        ))
    }

    fn run_turn(
        &self,
        _account: &ResolvedNativeAccount,
        _request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        let mut delta = NativeEvent::new(0, NativeEventKind::AssistantDelta);
        delta.content_class = Some(NativeContentClass::Assistant);
        delta.text = Some("catalog-free response".into());
        host.emit(delta)?;
        Ok(NativeTurnOutcome { session_id: None })
    }
}

pub(crate) struct FakeToolExecutor {
    pub output: String,
}

impl AlfredToolExecutor for FakeToolExecutor {
    fn supports_memory_lookup(&self) -> bool {
        true
    }
    fn execute(
        &self,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        cancellation.checkpoint()?;
        Ok(AlfredToolResult {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: request.request_id.clone(),
            status: AlfredToolStatus::Completed,
            output: self.output.clone(),
            exit_code: Some(0),
            truncated: false,
            metadata: Map::new(),
        })
    }
}

pub(crate) struct FakeApprovalHandler(pub AlfredApprovalDecision);

impl AlfredApprovalHandler for FakeApprovalHandler {
    fn decide(
        &self,
        _request: &AlfredApprovalRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredApprovalDecision, NativeRuntimeError> {
        cancellation.checkpoint()?;
        Ok(self.0)
    }
}

pub(crate) fn fake_request(
    provider: AgentProvider,
    account_ref: OpaqueAgentAccountRef,
) -> NativeTurnRequest {
    let workspace = std::env::current_dir().expect("current directory");
    NativeTurnRequest {
        contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
        harness: crate::agents::AgentHarness::Alfred,
        harness_version: env!("CARGO_PKG_VERSION").into(),
        runtime_version: "1.0.0".into(),
        provider,
        account_ref,
        run_id: "run_fake".into(),
        node_id: "node_fake".into(),
        model: "fake-model".into(),
        prompt: "hello".into(),
        context: vec![NativeContextBlock {
            role: NativeContextRole::User,
            content: "hello".into(),
            name: None,
        }],
        working_directory: workspace.clone(),
        allowed_workspace_roots: vec![workspace],
        permission_profile: NativePermissionProfile::default(),
        tool_capabilities: NativeToolCapabilitySet {
            filesystem: true,
            shell: true,
            patch: true,
            mcp: false,
            subagents: false,
            memory_lookup: false,
        },
        session_mode: NativeSessionMode::Ephemeral,
        session_id: None,
        event_limits: NativeEventLimits::default(),
        timeout_ms: DEFAULT_TURN_TIMEOUT.as_millis() as u64,
        cancellation: Some(
            NativeCancellation::new("cancel_fake", DEFAULT_TURN_TIMEOUT)
                .expect("fake cancellation"),
        ),
    }
}

pub(crate) fn shell_tool(arguments: Vec<String>) -> AlfredToolRequest {
    AlfredToolRequest {
        contract_version: TOOL_CONTRACT_VERSION,
        request_id: "tool_fake".into(),
        kind: AlfredToolKind::Shell,
        name: "shell".into(),
        path: Some(PathBuf::from(".")),
        arguments,
        input: Map::new(),
        timeout_ms: 1_000,
        max_output_bytes: 32,
    }
}
pub(crate) struct FakeMemoryLookupTraceSink {
    pub traces: Mutex<Vec<NativeMemoryLookupTrace>>,
    pub fail: AtomicBool,
}

impl FakeMemoryLookupTraceSink {
    pub fn new() -> Self {
        Self {
            traces: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
        }
    }
}

impl MemoryLookupTraceSink for FakeMemoryLookupTraceSink {
    fn record(&self, trace: &NativeMemoryLookupTrace) -> Result<(), NativeRuntimeError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "fake lookup trace sink rejected the trace",
                true,
            ));
        }
        self.traces
            .lock()
            .map_err(|_| NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "fake lookup trace sink is unavailable",
                true,
            ))?
            .push(trace.clone());
        Ok(())
    }
}

pub(crate) struct FakeMemoryLookupContextAllowance {
    initial_bytes: usize,
    remaining: Mutex<HashMap<String, usize>>,
    pub unknown: AtomicBool,
    pub fail: AtomicBool,
}

impl FakeMemoryLookupContextAllowance {
    pub fn new(initial_bytes: usize) -> Self {
        Self {
            initial_bytes,
            remaining: Mutex::new(HashMap::new()),
            unknown: AtomicBool::new(false),
            fail: AtomicBool::new(false),
        }
    }

    pub fn set_remaining(&self, turn_id: &str, bytes: usize) {
        self.remaining
            .lock()
            .expect("fake allowance lock")
            .insert(turn_id.into(), bytes);
    }
    pub(crate) fn remaining_for(&self, turn_id: &str) -> Option<usize> {
        self.remaining
            .lock()
            .expect("fake allowance lock")
            .get(turn_id)
            .copied()
    }
}

impl MemoryLookupContextAllowance for FakeMemoryLookupContextAllowance {
    fn availability(&self) -> NativeLookupAvailability {
        if self.unknown.load(Ordering::SeqCst) {
            NativeLookupAvailability::unavailable(
                "fake runtime cannot verify remaining context",
            )
        } else {
            NativeLookupAvailability::available()
        }
    }
    fn remaining_bytes(&self, context: &AlfredToolExecutionContext) -> Option<usize> {
        if self.unknown.load(Ordering::SeqCst) {
            return None;
        }
        Some(
            self.remaining
                .lock()
                .expect("fake allowance lock")
                .get(&context.turn_id)
                .copied()
                .unwrap_or(self.initial_bytes),
        )
    }

    fn reserve_bytes(
        &self,
        context: &AlfredToolExecutionContext,
        requested_bytes: usize,
    ) -> Result<usize, NativeRuntimeError> {
        if self.unknown.load(Ordering::SeqCst) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::CapabilityUnsupported,
                "fake runtime cannot verify remaining context",
                false,
            ));
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "fake context allowance failed",
                true,
            ));
        }
        let mut remaining = self.remaining.lock().expect("fake allowance lock");
        let available = remaining
            .entry(context.turn_id.clone())
            .or_insert(self.initial_bytes);
        if *available < requested_bytes {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::ToolOutputLimitExceeded,
                "fake runtime has insufficient remaining context",
                false,
            ));
        }
        *available -= requested_bytes;
        Ok(requested_bytes)
    }

    fn release_bytes(&self, context: &AlfredToolExecutionContext, unused_bytes: usize) {
        if unused_bytes == 0 {
            return;
        }
        let mut remaining = self.remaining.lock().expect("fake allowance lock");
        let available = remaining
            .entry(context.turn_id.clone())
            .or_insert(self.initial_bytes);
        *available = available.saturating_add(unused_bytes);
    }
}

pub(crate) fn memory_lookup_tool(
    request_id: impl Into<String>,
    name: &str,
    input: Map<String, serde_json::Value>,
) -> AlfredToolRequest {
    AlfredToolRequest {
        contract_version: TOOL_CONTRACT_VERSION,
        request_id: request_id.into(),
        kind: AlfredToolKind::MemoryLookup,
        name: name.into(),
        path: None,
        arguments: Vec::new(),
        input,
        timeout_ms: LOOKUP_LIMITS_V1.read_deadline_ms,
        max_output_bytes: LOOKUP_LIMITS_V1.get_response_bytes,
    }
}
