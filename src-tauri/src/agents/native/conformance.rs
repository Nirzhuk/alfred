use super::fake::*;
use super::*;
use crate::agents::{
    AgentActivity, AgentActivityKind, AgentActivityState, AgentExecutionTarget, AgentHarness,
    AgentNativeRuntime, AgentProvider, AgentRequest, AgentRequestMetadata, AgentResponse,
    AgentRunHooks, OpaqueAgentAccountRef,
};
use crate::db::{
    agent_memory_lookup::{
        AgentMemoryLookupService, LookupSourceKind, LookupSourceState, LookupToolName,
        LookupTraceClassification,
    },
    lookup_trace::{
        insert_lookup_trace, lookup_trace_rows_conn, LookupTraceInput, LookupTraceSourceInput,
    },
    CreateMemoryInput, CreateWorkflowInput, Db, MemoryScopeType, MemoryStatus, MemoryType,
};
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn fixture() -> (
    Arc<NativeRuntimeRegistry>,
    Arc<FakeNativeRuntime>,
    Arc<FakeAccountResolver>,
    OpaqueAgentAccountRef,
) {
    let provider = AgentProvider::Codex;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let runtime = Arc::new(FakeNativeRuntime::new(provider));
    let registry = Arc::new(NativeRuntimeRegistry::default());
    registry.register(runtime.clone()).unwrap();
    let resolver = Arc::new(FakeAccountResolver::new(account_ref.clone(), provider));
    (registry, runtime, resolver, account_ref)
}

struct DbMemoryLookupTraceSink {
    db: Arc<Db>,
}

impl DbMemoryLookupTraceSink {
    fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

impl MemoryLookupTraceSink for DbMemoryLookupTraceSink {
    fn record(&self, trace: &NativeMemoryLookupTrace) -> Result<(), NativeRuntimeError> {
        let sources = trace
            .trace
            .sources
            .iter()
            .map(|source| LookupTraceSourceInput {
                reference: &source.reference,
                source_kind: match source.source {
                    LookupSourceKind::Memory => "memory",
                    LookupSourceKind::RunStep => "run_step",
                    LookupSourceKind::Evidence => "evidence",
                },
                source_id: &source.source_id,
                version: &source.version,
                state: match source.state {
                    LookupSourceState::Current => "current",
                    LookupSourceState::Historical => "historical",
                    LookupSourceState::Incomplete => "incomplete",
                    LookupSourceState::Unavailable => "unavailable",
                    LookupSourceState::Deleted => "deleted",
                    LookupSourceState::Expired => "expired",
                    LookupSourceState::Retracted => "retracted",
                    LookupSourceState::Replaced => "replaced",
                    LookupSourceState::Unlinked => "unlinked",
                },
                page: source.page,
            })
            .collect::<Vec<_>>();
        let created_at = Utc::now().to_rfc3339();
        let input = LookupTraceInput {
            turn_id: &trace.turn_id,
            run_id: &trace.run_id,
            node_id: &trace.node_id,
            request_id: &trace.request_id,
            tool: match trace.trace.tool {
                LookupToolName::MemorySearch => "memory.search",
                LookupToolName::MemoryGet => "memory.get",
                LookupToolName::HistorySearch => "history.search",
                LookupToolName::HistoryGet => "history.get",
            },
            retrieved_bytes: trace.retrieved_bytes,
            returned_bytes: trace.trace.returned_bytes,
            classification: match trace.trace.classification {
                LookupTraceClassification::RetrievedData => "retrieved_data",
            },
            delivery: match trace.delivery {
                NativeMemoryLookupDelivery::Delivered => "delivered",
                NativeMemoryLookupDelivery::RejectedNativeOutputLimit => {
                    "rejected_native_output_limit"
                }
                NativeMemoryLookupDelivery::RejectedContextBudget => "rejected_context_budget",
                NativeMemoryLookupDelivery::RejectedInvalidJson => "rejected_invalid_json",
                NativeMemoryLookupDelivery::RejectedCancelled => "rejected_cancelled",
                NativeMemoryLookupDelivery::RejectedTimedOut => "rejected_timeout",
            },
            created_at: &created_at,
            sources: &sources,
        };
        self.db
            .with_conn(|connection| insert_lookup_trace(connection, &input))
            .map(|_| ())
            .map_err(|error| {
                NativeRuntimeError::new(
                    NativeErrorCode::ProviderUnavailable,
                    format!("failed to persist memory lookup trace: {error}"),
                    true,
                )
            })
    }
}

struct ReservingToolExecutor {
    allowance: Arc<dyn MemoryLookupContextAllowance>,
    reserved_bytes: usize,
}

impl AlfredToolExecutor for ReservingToolExecutor {
    fn execute(
        &self,
        request: &AlfredToolRequest,
        _cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        Ok(AlfredToolResult {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: request.request_id.clone(),
            status: AlfredToolStatus::Completed,
            output: "non-lookup work".into(),
            exit_code: None,
            truncated: false,
            metadata: serde_json::Map::new(),
        })
    }

    fn execute_for_turn(
        &self,
        context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        if request.kind == AlfredToolKind::Shell {
            self.allowance
                .reserve_bytes(context, self.reserved_bytes)?;
        }
        self.execute(request, cancellation)
    }
}


struct CancelAfterReleaseAllowance {
    inner: Arc<FakeMemoryLookupContextAllowance>,
    cancellation: NativeCancellation,
    timed_out_after_release: bool,
}

impl MemoryLookupContextAllowance for CancelAfterReleaseAllowance {
    fn availability(&self) -> NativeLookupAvailability {
        self.inner.availability()
    }
    fn remaining_bytes(&self, context: &AlfredToolExecutionContext) -> Option<usize> {
        self.inner.remaining_bytes(context)
    }

    fn reserve_bytes(
        &self,
        context: &AlfredToolExecutionContext,
        requested_bytes: usize,
    ) -> Result<usize, NativeRuntimeError> {
        self.inner.reserve_bytes(context, requested_bytes)
    }

    fn release_bytes(&self, context: &AlfredToolExecutionContext, unused_bytes: usize) {
        self.inner.release_bytes(context, unused_bytes);
        if unused_bytes > 0 {
            if self.timed_out_after_release {
                std::thread::sleep(Duration::from_millis(120));
            } else {
                self.cancellation.cancel();
            }
        }
    }
}
fn fake_target(provider: AgentProvider) -> AgentExecutionTarget {
    AgentExecutionTarget {
        provider,
        harness: AgentHarness::Alfred,
        account_ref: Some(OpaqueAgentAccountRef::parse("account_fake-01").unwrap()),
        model: Some("fake-model".into()),
        working_directory: None,
        request_metadata: AgentRequestMetadata::new("run_1", "node_1").unwrap(),
    }
}

#[test]
fn fake_runtime_registers_validates_streams_reports_usage_and_unregisters() {
    let (registry, _runtime, resolver, account_ref) = fixture();
    registry
        .validate_account(AgentProvider::Codex, &account_ref, resolver.as_ref())
        .unwrap();
    assert_eq!(
        registry
            .discover_models(AgentProvider::Codex, &account_ref, resolver.as_ref())
            .unwrap()[0]
            .id,
        "fake-model"
    );
    let mut invalid_model = fake_request(AgentProvider::Codex, account_ref.clone());
    invalid_model.model = "unknown-model".into();
    assert_eq!(
        registry
            .execute_turn(
                &invalid_model,
                resolver.as_ref(),
                &FakeToolExecutor {
                    output: "ok".into()
                },
                &FakeApprovalHandler(AlfredApprovalDecision::Allow),
                &mut |_| {},
            )
            .unwrap_err()
            .code,
        NativeErrorCode::ModelUnavailable
    );
    let request = fake_request(AgentProvider::Codex, account_ref.clone());
    let mut streamed = Vec::new();
    let result = registry
        .execute_turn(
            &request,
            resolver.as_ref(),
            &FakeToolExecutor {
                output: "ok".into(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Allow),
            &mut |event| streamed.push(event.clone()),
        )
        .unwrap();
    assert_eq!(
        registry
            .memory_lookup_availability(AgentProvider::Codex)
            .unwrap(),
        NativeLookupAvailability::available()
    );
    assert_eq!(result.output, "fake response");
    assert_eq!(streamed.len(), 3);
    assert_eq!(
        registry
            .usage_snapshot(AgentProvider::Codex, &account_ref, resolver.as_ref())
            .unwrap()
            .state,
        NativeUsageState::Supported
    );
    let report = registry.capability_report(AgentProvider::Codex).unwrap();
    assert_eq!(
        report.product,
        crate::agent_accounts::models::AgentProductId::OpenaiApi
    );
    assert_eq!(
        report.tool_execution_owner,
        NativeToolExecutionOwner::AlfredExecuted
    );
    assert!(report.entries.iter().any(|entry| {
        entry.capability == "usage" && entry.status == CapabilityReportStatus::Supported
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.capability == "memory_lookup" && entry.status == CapabilityReportStatus::Supported
    }));
    assert!(registry.unregister(AgentProvider::Codex).unwrap().is_some());
    assert_eq!(
        registry.descriptor(AgentProvider::Codex).unwrap_err().code,
        NativeErrorCode::ProviderUnavailable
    );
}

#[test]
fn disconnected_account_fails_before_native_invocation() {
    let (registry, _runtime, resolver, account_ref) = fixture();
    resolver
        .available
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        registry
            .validate_account(AgentProvider::Codex, &account_ref, resolver.as_ref())
            .unwrap_err()
            .code,
        NativeErrorCode::AccountUnavailable
    );
}

#[test]
fn approval_allow_and_deny_are_explicit_and_bounded() {
    let (registry, runtime, resolver, account_ref) = fixture();
    *runtime.request_tool.lock().unwrap() = Some(shell_tool(vec!["pwd".into()]));
    let mut request = fake_request(AgentProvider::Codex, account_ref);
    request.permission_profile.shell = NativeApprovalPolicy::Ask;
    let mut events = Vec::new();
    let allowed = registry
        .execute_turn(
            &request,
            resolver.as_ref(),
            &FakeToolExecutor {
                output: "workspace".into(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Allow),
            &mut |event| events.push(event.clone()),
        )
        .unwrap();
    assert_eq!(allowed.output, "tool: workspace");
    assert!(events.iter().any(|event| {
        event.kind == NativeEventKind::ApprovalResolved && event.approved == Some(true)
    }));

    request.cancellation =
        Some(NativeCancellation::new("cancel_deny", DEFAULT_TURN_TIMEOUT).unwrap());
    let denied = registry
        .execute_turn(
            &request,
            resolver.as_ref(),
            &FakeToolExecutor {
                output: "must not leak".into(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Deny),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(denied.output, "tool denied");
}

#[test]
fn cancellation_timeout_and_session_gates_are_visible() {
    let (registry, runtime, resolver, account_ref) = fixture();
    let mut request = fake_request(AgentProvider::Codex, account_ref);
    request.cancellation.as_ref().unwrap().cancel();
    assert_eq!(
        registry
            .execute_turn(
                &request,
                resolver.as_ref(),
                &FakeToolExecutor { output: "".into() },
                &FakeApprovalHandler(AlfredApprovalDecision::Deny),
                &mut |_| {},
            )
            .unwrap_err()
            .code,
        NativeErrorCode::Cancelled
    );
    let timeout = NativeCancellation::new("timeout", Duration::from_millis(1)).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    request.cancellation = Some(timeout);
    assert_eq!(
        registry
            .execute_turn(
                &request,
                resolver.as_ref(),
                &FakeToolExecutor { output: "".into() },
                &FakeApprovalHandler(AlfredApprovalDecision::Deny),
                &mut |_| {},
            )
            .unwrap_err()
            .code,
        NativeErrorCode::TimedOut
    );
    request.cancellation = Some(NativeCancellation::new("session", DEFAULT_TURN_TIMEOUT).unwrap());
    request.session_mode = NativeSessionMode::Resume;
    request.session_id = Some("session_1".into());
    assert_eq!(
        registry
            .execute_turn(
                &request,
                resolver.as_ref(),
                &FakeToolExecutor { output: "".into() },
                &FakeApprovalHandler(AlfredApprovalDecision::Deny),
                &mut |_| {},
            )
            .unwrap_err()
            .code,
        NativeErrorCode::SessionUnavailable
    );
    let direct = NativeCancellation::new("direct_cancel", DEFAULT_TURN_TIMEOUT).unwrap();
    runtime.cancel(&direct).unwrap();
    assert!(direct.is_cancelled());
}

#[test]
fn registry_cancels_an_active_fake_turn() {
    let (registry, runtime, resolver, account_ref) = fixture();
    runtime
        .delay_ms
        .store(250, std::sync::atomic::Ordering::SeqCst);
    let mut request = fake_request(AgentProvider::Codex, account_ref);
    request.cancellation =
        Some(NativeCancellation::new("active_cancel", DEFAULT_TURN_TIMEOUT).unwrap());
    let worker_registry = registry.clone();
    let worker_resolver = resolver.clone();
    let worker = std::thread::spawn(move || {
        worker_registry.execute_turn(
            &request,
            worker_resolver.as_ref(),
            &FakeToolExecutor {
                output: String::new(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Deny),
            &mut |_| {},
        )
    });
    std::thread::sleep(Duration::from_millis(20));
    registry
        .cancel(AgentProvider::Codex, "active_cancel")
        .unwrap();
    assert_eq!(
        worker.join().unwrap().unwrap_err().code,
        NativeErrorCode::Cancelled
    );
    assert!(runtime.cancelled.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn malformed_oversized_reasoning_and_secret_events_are_safe() {
    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits::default()).unwrap();
    let malformed = json!({
        "contractVersion": 1,
        "sequence": 1,
        "kind": "assistant_delta",
        "contentClass": "assistant",
        "rawProviderPayload": {"anything": true}
    });
    assert_eq!(
        normalizer.normalize_untrusted(malformed).unwrap_err().code,
        NativeErrorCode::InvalidEvent
    );

    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits::default()).unwrap();
    let reasoning = json!({
        "contractVersion": 1,
        "sequence": 1,
        "kind": "assistant_delta",
        "contentClass": "reasoning",
        "metadata": {"reasoning": "private chain of thought"}
    });
    assert_eq!(
        normalizer.normalize_untrusted(reasoning).unwrap_err().code,
        NativeErrorCode::InvalidEvent
    );

    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits {
        max_text_bytes: 8,
        ..NativeEventLimits::default()
    })
    .unwrap();
    let oversized = json!({
        "contractVersion": 1,
        "sequence": 1,
        "kind": "assistant_delta",
        "contentClass": "assistant",
        "text": "too many bytes"
    });
    assert_eq!(
        normalizer.normalize_untrusted(oversized).unwrap_err().code,
        NativeErrorCode::EventLimitExceeded
    );

    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits::default()).unwrap();
    let secret = json!({
        "contractVersion": 1,
        "sequence": 1,
        "kind": "warning",
        "text": "Bearer access-secret",
        "metadata": {
            "authorization": "Bearer metadata-secret",
            "safe": "sk-provider-secret"
        }
    });
    let event = normalizer.normalize_untrusted(secret).unwrap();
    let serialized = serde_json::to_string(&event).unwrap();
    for secret in ["access-secret", "metadata-secret", "provider-secret"] {
        assert!(!serialized.contains(secret));
    }
    assert!(serialized.contains("[REDACTED]"));

    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits {
        max_events: 1,
        ..NativeEventLimits::default()
    })
    .unwrap();
    normalizer
        .normalize(NativeEvent::new(1, NativeEventKind::TurnStarted))
        .unwrap();
    assert_eq!(
        normalizer
            .normalize(NativeEvent::new(2, NativeEventKind::TurnCompleted))
            .unwrap_err()
            .code,
        NativeErrorCode::EventLimitExceeded
    );

    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits {
        max_metadata_depth: 2,
        ..NativeEventLimits::default()
    })
    .unwrap();
    let nested = json!({
        "contractVersion": 1,
        "sequence": 1,
        "kind": "tool_completed",
        "toolCallId": "tool_1",
        "toolName": "read",
        "toolOutput": "bounded",
        "metadata": {"one": {"two": {"three": "too deep"}}}
    });
    assert_eq!(
        normalizer.normalize_untrusted(nested).unwrap_err().code,
        NativeErrorCode::EventLimitExceeded
    );
}

#[test]
fn provider_errors_are_redacted_before_the_registry_returns_them() {
    let (registry, runtime, resolver, account_ref) = fixture();
    *runtime.fail_with.lock().unwrap() = Some(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        "provider failed with Bearer top-secret-token",
        true,
    ));
    let error = registry
        .execute_turn(
            &fake_request(AgentProvider::Codex, account_ref),
            resolver.as_ref(),
            &FakeToolExecutor {
                output: String::new(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Deny),
            &mut |_| {},
        )
        .unwrap_err();
    assert!(!error.message.contains("top-secret-token"));
    assert!(error.message.contains("[REDACTED]"));
}

#[test]
fn workspace_escape_output_command_timeout_and_cli_flags_are_bounded() {
    let workspace = std::env::current_dir().unwrap();
    let escape = validate_workspace_path(
        std::path::Path::new("../../outside"),
        &workspace,
        std::slice::from_ref(&workspace),
    );
    assert_eq!(escape.unwrap_err().code, NativeErrorCode::WorkspaceDenied);

    let mut tool = shell_tool(vec!["--full-auto".into()]);
    assert_eq!(
        validate_tool_request(
            &tool,
            &workspace,
            std::slice::from_ref(&workspace),
            &NativePermissionProfile::default(),
            &NativeToolCapabilitySet {
                shell: true,
                ..NativeToolCapabilitySet::default()
            },
        )
        .unwrap_err()
        .code,
        NativeErrorCode::PermissionDenied
    );
    tool.arguments = vec!["pwd".into()];
    tool.timeout_ms = DEFAULT_MAX_COMMAND_TIMEOUT_MS + 1;
    assert_eq!(
        validate_tool_request(
            &tool,
            &workspace,
            std::slice::from_ref(&workspace),
            &NativePermissionProfile::default(),
            &NativeToolCapabilitySet {
                shell: true,
                ..NativeToolCapabilitySet::default()
            },
        )
        .unwrap_err()
        .code,
        NativeErrorCode::ToolTimeout
    );
    tool.timeout_ms = 1_000;
    tool.max_output_bytes = 4;
    let normalized = normalize_tool_result(
        AlfredToolResult {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: tool.request_id.clone(),
            status: AlfredToolStatus::Completed,
            output: "abcdefgh".into(),
            exit_code: Some(0),
            truncated: false,
            metadata: Default::default(),
        },
        &tool,
    )
    .unwrap();
    assert_eq!(normalized.output, "abcd");
    assert!(normalized.truncated);
}

#[test]
fn alfred_resolves_skill_content_before_native_invocation() {
    let root = std::env::temp_dir().join(format!("alfred-native-skill-{}", uuid::Uuid::new_v4()));
    let skill_dir = root.join(".agents/skills/demo");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: fixture\n---\nUse the fixture instructions.",
    )
    .unwrap();
    let target = AgentExecutionTarget {
        provider: AgentProvider::Codex,
        harness: AgentHarness::Alfred,
        account_ref: Some(OpaqueAgentAccountRef::parse("account_fake-01").unwrap()),
        model: Some("fake-model".into()),
        working_directory: Some(root.to_string_lossy().into()),
        request_metadata: AgentRequestMetadata::new("run_1", "node_1").unwrap(),
    };
    let request = AgentRequest {
        prompt: "do work".into(),
        model: None,
        skill: None,
        skill_name: None,
        skill_names: vec!["demo".into()],
        working_directory: None,
        extra: serde_json::Value::Null,
    };
    let descriptor = FakeNativeRuntime::new(AgentProvider::Codex).descriptor();
    let native = prepare_native_request(
        &target,
        &request,
        &descriptor,
        NativePermissionProfile::default(),
        NativeToolCapabilitySet::default(),
        NativeEventLimits::default(),
        &NativeContextPolicy::default(),
    )
    .unwrap();
    assert_eq!(native.context[0].role, NativeContextRole::Skill);
    assert!(native.context[0].content.contains("fixture instructions"));
    assert_eq!(native.context[1].role, NativeContextRole::User);
    assert!(!native.context[1].content.starts_with("/demo"));

    let mut missing = request.clone();
    missing.skill_names = vec!["missing-skill".into()];
    assert_eq!(
        prepare_native_request(
            &target,
            &missing,
            &descriptor,
            NativePermissionProfile::default(),
            NativeToolCapabilitySet::default(),
            NativeEventLimits::default(),
            &NativeContextPolicy::default(),
        )
        .unwrap_err()
        .code,
        NativeErrorCode::InvalidRequest
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn request_and_event_contracts_never_serialize_credentials() {
    let request = fake_request(
        AgentProvider::Codex,
        OpaqueAgentAccountRef::parse("account_fake-01").unwrap(),
    );
    let serialized = serde_json::to_string(&request).unwrap();
    assert!(serialized.contains("account_fake-01"));
    for forbidden in [
        "accessToken",
        "refreshToken",
        "apiKey",
        "bypassPermissions",
        "--full-auto",
        "--allow-all",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

// ---------------------------------------------------------------------------
// Gate 2 remediation coverage (Plans 031-032).
// ---------------------------------------------------------------------------

#[derive(Default)]
struct BlockingCancelState {
    started: bool,
    cancel_called: bool,
    cancel_called_before_settle: bool,
    cancellation_id: Option<String>,
    settled: bool,
}

struct BlockingCancelRuntime {
    provider: AgentProvider,
    state: std::sync::Mutex<BlockingCancelState>,
    wake: std::sync::Condvar,
}

impl BlockingCancelRuntime {
    fn new(provider: AgentProvider) -> Self {
        Self {
            provider,
            state: std::sync::Mutex::new(BlockingCancelState::default()),
            wake: std::sync::Condvar::new(),
        }
    }

    fn wait_for(&self, predicate: impl Fn(&BlockingCancelState) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut state = self.state.lock().expect("blocking runtime state");
        while !predicate(&state) {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("blocking runtime timed out");
            let (next, timeout) = self
                .wake
                .wait_timeout(state, remaining)
                .expect("blocking runtime wait");
            state = next;
            assert!(!timeout.timed_out(), "blocking runtime timed out");
        }
    }
}

impl NativeAgentRuntime for BlockingCancelRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: "blocking-cancel-runtime".into(),
            runtime_version: "1.0.0".into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: self.provider,
            product: crate::agent_accounts::models::AgentProductId::OpenaiApi,
            tool_execution_owner: NativeToolExecutionOwner::NoTools,
            capabilities: NativeCapabilities {
                supports_model_list: true,
                ..NativeCapabilities::default()
            },
        }
    }

    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        if account.provider == self.provider {
            Ok(())
        } else {
            Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "blocking runtime account mismatch",
                false,
            ))
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
        _request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        let mut state = self.state.lock().expect("blocking runtime state");
        state.started = true;
        self.wake.notify_all();
        while !state.cancel_called {
            state = self.wake.wait(state).expect("blocking runtime wait");
        }
        drop(state);

        let result = host.cancellation().checkpoint();
        let mut state = self.state.lock().expect("blocking runtime state");
        state.settled = true;
        self.wake.notify_all();
        drop(state);
        result?;
        Ok(NativeTurnOutcome { session_id: None })
    }

    fn cancel(&self, cancellation: &NativeCancellation) -> Result<(), NativeRuntimeError> {
        let mut state = self.state.lock().expect("blocking runtime state");
        state.cancel_called = true;
        state.cancel_called_before_settle = !state.settled;
        state.cancellation_id = Some(cancellation.id().to_owned());
        cancellation.cancel();
        self.wake.notify_all();
        Ok(())
    }
}

/// H1: a runtime that publishes no model catalog still runs a turn, provided
/// the explicitly selected model is bounded and secret-free.
#[test]
fn a_runtime_without_a_model_catalog_runs_with_an_explicit_bounded_model() {
    let provider = AgentProvider::Grok;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let registry = Arc::new(NativeRuntimeRegistry::default());
    registry
        .register(Arc::new(FakeCatalogFreeRuntime { provider }))
        .unwrap();
    let resolver = FakeAccountResolver::new(account_ref.clone(), provider);

    let mut request = fake_request(provider, account_ref.clone());
    request.model = "grok-4-fast".into();
    request.tool_capabilities = NativeToolCapabilitySet::default();
    let result = registry
        .execute_turn(
            &request,
            &resolver,
            &FakeToolExecutor {
                output: "ok".into(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Deny),
            &mut |_| {},
        )
        .expect("catalog-free runtime executes");
    assert_eq!(result.output, "catalog-free response");

    // Model discovery itself stays honestly unsupported.
    assert_eq!(
        registry
            .discover_models(provider, &account_ref, &resolver)
            .unwrap_err()
            .code,
        NativeErrorCode::CapabilityUnsupported
    );

    // An unbounded or secret-bearing model is still refused.
    for bad in ["", "  spaced", &"m".repeat(257), "Bearer sk-live-1"] {
        let mut invalid = fake_request(provider, account_ref.clone());
        invalid.tool_capabilities = NativeToolCapabilitySet::default();
        invalid.model = bad.into();
        assert!(
            registry
                .execute_turn(
                    &invalid,
                    &resolver,
                    &FakeToolExecutor {
                        output: "ok".into()
                    },
                    &FakeApprovalHandler(AlfredApprovalDecision::Deny),
                    &mut |_| {},
                )
                .is_err(),
            "model {bad:?} must be refused"
        );
    }
}

/// M2: a denied tool always terminates its own event pair.
#[test]
fn denied_tools_emit_a_terminal_tool_completed_event() {
    for (policy, decision) in [
        (NativeApprovalPolicy::Deny, AlfredApprovalDecision::Allow),
        (NativeApprovalPolicy::Ask, AlfredApprovalDecision::Deny),
    ] {
        let (registry, runtime, resolver, account_ref) = fixture();
        *runtime.request_tool.lock().unwrap() = Some(shell_tool(vec!["pwd".into()]));
        let mut request = fake_request(AgentProvider::Codex, account_ref);
        request.permission_profile.shell = policy;
        let mut events = Vec::new();
        let result = registry
            .execute_turn(
                &request,
                resolver.as_ref(),
                &FakeToolExecutor {
                    output: "workspace".into(),
                },
                &FakeApprovalHandler(decision),
                &mut |event| events.push(event.clone()),
            )
            .expect("turn completes");
        assert_eq!(result.output, "tool denied");

        let started = events
            .iter()
            .filter(|event| event.kind == NativeEventKind::ToolStarted)
            .count();
        let completed = events
            .iter()
            .filter(|event| event.kind == NativeEventKind::ToolCompleted)
            .count();
        assert_eq!(started, 1, "policy {policy:?} started count");
        assert_eq!(
            completed, started,
            "policy {policy:?} left a dangling tool_started"
        );
        assert!(events
            .iter()
            .any(|event| event.kind == NativeEventKind::ToolCompleted
                && event
                    .tool_output
                    .as_deref()
                    .is_some_and(|output| output.contains("denied"))));
    }
}

/// M3: patch application needs an explicit declared capability.
#[test]
fn apply_patch_requires_a_declared_patch_capability() {
    let provider = AgentProvider::Grok;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let registry = Arc::new(NativeRuntimeRegistry::default());
    // The catalog-free runtime declares neither patch nor filesystem support.
    registry
        .register(Arc::new(FakeCatalogFreeRuntime { provider }))
        .unwrap();
    let resolver = FakeAccountResolver::new(account_ref.clone(), provider);

    let mut request = fake_request(provider, account_ref);
    request.model = "grok-4-fast".into();
    request.tool_capabilities = NativeToolCapabilitySet {
        patch: true,
        ..NativeToolCapabilitySet::default()
    };
    assert_eq!(
        registry
            .execute_turn(
                &request,
                &resolver,
                &FakeToolExecutor {
                    output: "ok".into()
                },
                &FakeApprovalHandler(AlfredApprovalDecision::Deny),
                &mut |_| {},
            )
            .unwrap_err()
            .code,
        NativeErrorCode::CapabilityUnsupported
    );

    // Filesystem support is not an implicit patch grant.
    let mut filesystem_only = NativeCapabilities {
        supports_tool_calls: true,
        supports_native_filesystem: true,
        ..NativeCapabilities::default()
    };
    let mut patch_request = fake_request(
        AgentProvider::Codex,
        OpaqueAgentAccountRef::parse("account_fake-01").unwrap(),
    );
    patch_request.tool_capabilities = NativeToolCapabilitySet {
        patch: true,
        ..NativeToolCapabilitySet::default()
    };
    assert_eq!(
        super::registry::validate_request_for_capabilities(&patch_request, &filesystem_only,)
            .unwrap_err()
            .code,
        NativeErrorCode::CapabilityUnsupported
    );

    filesystem_only.supports_patch = true;
    super::registry::validate_request_for_capabilities(&patch_request, &filesystem_only)
        .expect("an explicit patch capability authorizes the request");

    // The capability is reported explicitly rather than assumed.
    let (registry, _runtime, _resolver, _account) = fixture();
    let report = registry.capability_report(AgentProvider::Codex).unwrap();
    assert!(report
        .entries
        .iter()
        .any(|entry| entry.capability == "patch"
            && entry.status == CapabilityReportStatus::Supported));
    assert_eq!(
        NativeCapabilities::default().contract_version,
        NATIVE_CAPABILITY_CONTRACT_VERSION
    );
}

/// M4: a shell or process tool must name a workspace-confined cwd.
#[test]
fn shell_and_process_tools_require_a_confined_working_directory() {
    let workspace = std::env::current_dir().unwrap();
    let roots = std::slice::from_ref(&workspace);
    let capabilities = NativeToolCapabilitySet {
        shell: true,
        filesystem: true,
        ..NativeToolCapabilitySet::default()
    };

    for kind in [AlfredToolKind::Shell, AlfredToolKind::Process] {
        let mut missing = shell_tool(vec!["pwd".into()]);
        missing.kind = kind;
        missing.path = None;
        assert_eq!(
            validate_tool_request(
                &missing,
                &workspace,
                roots,
                &NativePermissionProfile::default(),
                &capabilities,
            )
            .unwrap_err()
            .code,
            NativeErrorCode::WorkspaceDenied,
            "{kind:?} without a cwd must be refused"
        );

        let mut escaping = shell_tool(vec!["pwd".into()]);
        escaping.kind = kind;
        escaping.path = Some(PathBuf::from("../../outside"));
        assert_eq!(
            validate_tool_request(
                &escaping,
                &workspace,
                roots,
                &NativePermissionProfile::default(),
                &capabilities,
            )
            .unwrap_err()
            .code,
            NativeErrorCode::WorkspaceDenied,
            "{kind:?} outside the roots must be refused"
        );

        let mut confined = shell_tool(vec!["pwd".into()]);
        confined.kind = kind;
        confined.path = Some(PathBuf::from("."));
        validate_tool_request(
            &confined,
            &workspace,
            roots,
            &NativePermissionProfile::default(),
            &capabilities,
        )
        .unwrap_or_else(|error| panic!("{kind:?} inside the workspace must pass: {error}"));
    }
}

/// M5: concurrent turns for the same run/node do not share a cancellation key,
/// and a handle from one provider cannot be cancelled through another.
#[test]
fn cancellation_handles_are_unique_per_turn_and_scoped_by_provider() {
    let target = fake_target(AgentProvider::Codex);
    let runtime = FakeNativeRuntime::new(AgentProvider::Codex);
    let descriptor = runtime.descriptor();
    let request = AgentRequest {
        prompt: "hello".into(),
        model: Some("fake-model".into()),
        skill: None,
        skill_name: None,
        skill_names: Vec::new(),
        working_directory: None,
        extra: serde_json::Value::Null,
    };
    let prepare = || {
        prepare_native_request(
            &target,
            &request,
            &descriptor,
            NativePermissionProfile::default(),
            NativeToolCapabilitySet::default(),
            NativeEventLimits::default(),
            &NativeContextPolicy::default(),
        )
        .expect("prepare")
    };
    let first = prepare();
    let second = prepare();
    assert_ne!(
        first.cancellation.as_ref().unwrap().id(),
        second.cancellation.as_ref().unwrap().id(),
        "two turns of the same run/node must not share a cancellation id"
    );

    // A live handle is not reachable through a different provider.
    let (registry, _runtime, resolver, account_ref) = fixture();
    let request = fake_request(AgentProvider::Codex, account_ref);
    let handle_id = request.cancellation.as_ref().unwrap().id().to_string();
    registry
        .execute_turn(
            &request,
            resolver.as_ref(),
            &FakeToolExecutor {
                output: "ok".into(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Deny),
            &mut |_| {},
        )
        .expect("turn");
    // The finished handle is evicted from the active map.
    assert_eq!(
        registry
            .cancel(AgentProvider::Codex, &handle_id)
            .unwrap_err()
            .code,
        NativeErrorCode::InvalidRequest
    );
}

/// Low: metadata nesting is bounded at exactly the declared depth.
#[test]
fn metadata_depth_bound_is_exact() {
    let limits = NativeEventLimits::default();
    let nest = |depth: usize| {
        let mut value = serde_json::json!("leaf");
        for _ in 0..depth {
            value = serde_json::json!({ "child": value });
        }
        value
    };
    // The metadata object itself is level one.
    for depth in 0..limits.max_metadata_depth {
        let mut normalizer = NativeEventNormalizer::new(limits.clone()).unwrap();
        let mut event = NativeEvent::new(1, NativeEventKind::TurnStarted);
        event.metadata = nest(depth).as_object().cloned().unwrap_or_default();
        assert!(
            normalizer.normalize(event).is_ok(),
            "depth {depth} must be accepted"
        );
    }
    let mut normalizer = NativeEventNormalizer::new(limits.clone()).unwrap();
    let mut event = NativeEvent::new(1, NativeEventKind::TurnStarted);
    event.metadata = nest(limits.max_metadata_depth)
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        normalizer.normalize(event).unwrap_err().code,
        NativeErrorCode::EventLimitExceeded
    );
}

/// H2: lowercase credential markers are detected and redacted everywhere the
/// native surfaces screen text.
#[test]
fn lowercase_credentials_are_redacted_across_every_native_surface() {
    let secret = "authorization: bearer eyJhbGciOiJsecret";
    assert!(contains_secret_marker(secret));

    // Event text, tool output, error text, and metadata.
    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits::default()).unwrap();
    let mut event = NativeEvent::new(1, NativeEventKind::Warning);
    event.text = Some(secret.into());
    event.error = Some("failed: cookie: session=abc".into());
    event.metadata = serde_json::json!({
        "detail": "set-cookie: s=1",
        "authorization": "bearer eyJhbGciOiJsecret",
    })
    .as_object()
    .cloned()
    .unwrap();
    let normalized = normalizer.normalize(event).expect("normalize");
    let serialized = serde_json::to_string(&normalized).expect("serialize");
    for leak in ["eyJhbGciOiJsecret", "session=abc", "s=1"] {
        assert!(!serialized.contains(leak), "leaked {leak} in {serialized}");
    }
    assert!(serialized.contains("[REDACTED]"));

    // Tool results.
    let mut tool = shell_tool(vec!["pwd".into()]);
    tool.max_output_bytes = 4096;
    let normalized = normalize_tool_result(
        AlfredToolResult {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: tool.request_id.clone(),
            status: AlfredToolStatus::Completed,
            output: format!("curl -H '{secret}'"),
            exit_code: Some(0),
            truncated: false,
            metadata: serde_json::json!({ "apikey": "sk-live-1" })
                .as_object()
                .cloned()
                .unwrap(),
        },
        &tool,
    )
    .unwrap();
    assert!(!normalized.output.contains("eyJhbGciOiJsecret"));
    assert!(!serde_json::to_string(&normalized.metadata)
        .unwrap()
        .contains("sk-live-1"));

    // Secret-bearing tool arguments are refused outright.
    let mut leaking = shell_tool(vec![format!("--header={secret}")]);
    leaking.path = Some(PathBuf::from("."));
    let workspace = std::env::current_dir().unwrap();
    assert_eq!(
        validate_tool_request(
            &leaking,
            &workspace,
            std::slice::from_ref(&workspace),
            &NativePermissionProfile::default(),
            &NativeToolCapabilitySet {
                shell: true,
                ..NativeToolCapabilitySet::default()
            },
        )
        .unwrap_err()
        .code,
        NativeErrorCode::PermissionDenied
    );
}

/// Low: the CLI-permission denylist is one shared helper, so every native
/// surface refuses the same flags.
#[test]
fn the_cli_permission_denylist_is_shared_by_every_native_surface() {
    let workspace = std::env::current_dir().unwrap();
    for flag in [
        "--full-auto",
        "--ALLOW-ALL",
        "bypassPermissions",
        "bypass_permissions",
        "--dangerously-skip-permissions",
        "--yolo",
    ] {
        assert!(contains_cli_permission_flag(flag), "helper missed {flag}");

        // Context preparation.
        let target = fake_target(AgentProvider::Codex);
        let runtime = FakeNativeRuntime::new(AgentProvider::Codex);
        let prepared = prepare_native_request(
            &target,
            &AgentRequest {
                prompt: format!("please run {flag}"),
                model: Some("fake-model".into()),
                skill: None,
                skill_name: None,
                skill_names: Vec::new(),
                working_directory: None,
                extra: serde_json::Value::Null,
            },
            &runtime.descriptor(),
            NativePermissionProfile::default(),
            NativeToolCapabilitySet::default(),
            NativeEventLimits::default(),
            &NativeContextPolicy::default(),
        );
        assert_eq!(
            prepared.unwrap_err().code,
            NativeErrorCode::PermissionDenied,
            "context accepted {flag}"
        );

        // The Alfred tool boundary.
        let mut tool = shell_tool(vec![flag.into()]);
        tool.path = Some(PathBuf::from("."));
        assert_eq!(
            validate_tool_request(
                &tool,
                &workspace,
                std::slice::from_ref(&workspace),
                &NativePermissionProfile::default(),
                &NativeToolCapabilitySet {
                    shell: true,
                    ..NativeToolCapabilitySet::default()
                },
            )
            .unwrap_err()
            .code,
            NativeErrorCode::PermissionDenied,
            "tool boundary accepted {flag}"
        );
    }
}

/// B1/B2: the router is the seam a provider plan consumes. It resolves the
/// account, honours Stop, and streams bounded activity — with no runner edit.
#[test]
fn the_execution_router_resolves_streams_activity_and_honours_stop() {
    use crate::agents::active::RunControl;
    use crate::agents::{
        AgentActivity, AgentActivityKind, AgentActivityState, AgentError, AgentNativeRuntime,
        AgentRunHooks,
    };
    use std::sync::Mutex as StdMutex;

    let provider = AgentProvider::Codex;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let registry = Arc::new(NativeRuntimeRegistry::default());
    let runtime = Arc::new(FakeNativeRuntime::new(provider));
    registry.register(runtime.clone()).unwrap();
    let router = NativeExecutionRouter::new(
        Arc::clone(&registry),
        Arc::new(FakeAccountResolver::new(account_ref, provider)),
        Arc::new(DenyAllToolExecutor),
        Arc::new(DenyAllApprovalHandler),
    );

    let target = fake_target(provider);
    let request = || AgentRequest {
        prompt: "hello".into(),
        model: Some("fake-model".into()),
        skill: None,
        skill_name: None,
        skill_names: Vec::new(),
        working_directory: None,
        extra: serde_json::Value::Null,
    };

    // Activity streams through the hook and stays inside the closed vocabulary.
    let seen: StdMutex<Vec<AgentActivity>> = StdMutex::new(Vec::new());
    let on_activity = |activity: &AgentActivity| {
        seen.lock().unwrap().push(activity.clone());
    };
    let control = RunControl::new();
    let response = router
        .run(
            &target,
            request(),
            AgentRunHooks {
                control: Some(&control),
                on_activity: Some(&on_activity),
            },
        )
        .expect("router runs the turn");
    assert_eq!(response.output, "fake response");

    let activities = seen.lock().unwrap().clone();
    assert!(!activities.is_empty(), "no activity reached the run hook");
    assert!(activities
        .iter()
        .any(|activity| activity.kind == AgentActivityKind::Assistant));
    assert!(activities
        .iter()
        .any(|activity| activity.state == AgentActivityState::Completed));
    // No provider text or detail escapes through activity.
    for activity in &activities {
        assert!(activity.detail.is_none());
        assert!(!activity.label.contains("fake response"));
    }

    // Stop cancels the live turn instead of being ignored.
    runtime
        .delay_ms
        .store(2_000, std::sync::atomic::Ordering::SeqCst);
    let control = RunControl::new();
    control.request_cancel();
    let error = router
        .run(
            &target,
            request(),
            AgentRunHooks {
                control: Some(&control),
                on_activity: None,
            },
        )
        .unwrap_err();
    assert!(
        matches!(error, AgentError::Cancelled),
        "Stop must cancel the native turn, got {error:?}"
    );
}

#[test]
fn live_stop_invokes_the_provider_cancel_hook_before_the_turn_settles() {
    use crate::agents::active::RunControl;
    use crate::agents::{AgentError, AgentNativeRuntime, AgentRunHooks};

    let provider = AgentProvider::Codex;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let registry = Arc::new(NativeRuntimeRegistry::default());
    let runtime = Arc::new(BlockingCancelRuntime::new(provider));
    registry.register(runtime.clone()).unwrap();
    let router = Arc::new(NativeExecutionRouter::new(
        Arc::clone(&registry),
        Arc::new(FakeAccountResolver::new(account_ref, provider)),
        Arc::new(DenyAllToolExecutor),
        Arc::new(DenyAllApprovalHandler),
    ));
    let control = RunControl::new();
    let worker_control = control.clone();
    let worker = std::thread::spawn(move || {
        router.run(
            &fake_target(provider),
            AgentRequest {
                prompt: "hello".into(),
                model: Some("fake-model".into()),
                skill: None,
                skill_name: None,
                skill_names: Vec::new(),
                working_directory: None,
                extra: serde_json::Value::Null,
            },
            AgentRunHooks {
                control: Some(&worker_control),
                on_activity: None,
            },
        )
    });

    runtime.wait_for(|state| state.started);
    control.request_cancel();
    runtime.wait_for(|state| state.cancel_called);
    assert!(
        runtime
            .state
            .lock()
            .expect("blocking runtime state")
            .cancel_called_before_settle,
        "provider cancel hook ran only after the turn settled"
    );
    let error = worker.join().expect("blocking turn thread").unwrap_err();
    assert!(matches!(error, AgentError::Cancelled));
    runtime.wait_for(|state| state.settled);
    let cancellation_id = runtime
        .state
        .lock()
        .expect("blocking runtime state")
        .cancellation_id
        .clone()
        .expect("cancel handle");
    assert_eq!(
        registry
            .cancel(provider, &cancellation_id)
            .unwrap_err()
            .code,
        NativeErrorCode::InvalidRequest,
        "completed turn left its cancellation handle active"
    );
}

/// B1: an unregistered provider surfaces `provider_unavailable` and never
/// silently falls back to the CLI harness.
#[test]
fn the_router_reports_an_unregistered_provider_without_a_cli_fallback() {
    use crate::agents::{AgentError, AgentNativeRuntime, AgentRunHooks};

    let provider = AgentProvider::Gemini;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let router = NativeExecutionRouter::new(
        Arc::new(NativeRuntimeRegistry::default()),
        Arc::new(FakeAccountResolver::new(account_ref, provider)),
        Arc::new(DenyAllToolExecutor),
        Arc::new(DenyAllApprovalHandler),
    );
    let error = router
        .run(
            &fake_target(provider),
            AgentRequest {
                prompt: "hello".into(),
                model: Some("fake-model".into()),
                skill: None,
                skill_name: None,
                skill_names: Vec::new(),
                working_directory: None,
                extra: serde_json::Value::Null,
            },
            AgentRunHooks {
                control: None,
                on_activity: None,
            },
        )
        .unwrap_err();
    match error {
        AgentError::Message(message) => {
            assert!(
                message.contains("providerunavailable"),
                "unexpected message {message}"
            );
        }
        other => panic!("expected a provider-unavailable message, got {other:?}"),
    }
}

/// B1: the default tool and approval handlers deny, so a provider that ships
/// before an Alfred executor exists cannot execute anything.
#[test]
fn the_default_tool_and_approval_handlers_deny() {
    let cancellation = NativeCancellation::new("cancel_fake", DEFAULT_TURN_TIMEOUT).unwrap();
    let request = shell_tool(vec!["pwd".into()]);
    assert_eq!(
        DenyAllToolExecutor
            .execute(&request, &cancellation)
            .unwrap_err()
            .code,
        NativeErrorCode::PermissionDenied
    );
    assert_eq!(
        DenyAllApprovalHandler
            .decide(
                &AlfredApprovalRequest {
                    contract_version: TOOL_CONTRACT_VERSION,
                    approval_id: "approval_fake".into(),
                    tool_request_id: request.request_id.clone(),
                    tool_name: request.name.clone(),
                    kind: request.kind,
                },
                &cancellation,
            )
            .unwrap(),
        AlfredApprovalDecision::Deny
    );
}

#[test]
fn memory_lookup_native_boundary_chains_scoped_sqlite_results() {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let workspace = std::env::current_dir().unwrap();
    let workflow = db
        .create_workflow(CreateWorkflowInput {
            name: "Native lookup fixture".into(),
            description: String::new(),
            working_directory: String::new(),
            folder_id: None,
            graph: json!({"nodes": [], "edges": []}),
        })
        .unwrap();
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE workflows SET memory_lookup_enabled = 1 WHERE id = ?1",
            [&workflow.id],
        )?;
        conn.execute(
            "INSERT INTO runs (id, workflow_id, created_at) VALUES (?1, ?2, ?3)",
            params!["native_lookup_run", workflow.id, "2025-01-01T00:00:00Z"],
        )?;
        Ok(())
    })
    .unwrap();

    let current = db
        .create_memory(CreateMemoryInput {
            workflow_id: workflow.id.clone(),
            title: "Current lookup note".into(),
            body: format!("needle current {}", "bounded-page ".repeat(330)),
            run_id: None,
            node_id: None,
            kind: Some("text".into()),
            scope_type: Some(MemoryScopeType::Workflow),
            memory_type: Some(MemoryType::Fact),
            source: Some("manual".into()),
            pinned: None,
            confidence: None,
            salience: None,
            status: Some(MemoryStatus::Active),
            supersedes_id: None,
            fact_key: None,
            observed_at: None,
            last_confirmed_at: None,
            valid_from: None,
            expires_at: None,
            id: Some("native_lookup_current".into()),
        })
        .unwrap();
    assert_eq!(current.id, "native_lookup_current");
    db.create_memory(CreateMemoryInput {
        workflow_id: workflow.id.clone(),
        title: "Historical lookup note".into(),
        body: "needle historical record".into(),
        run_id: None,
        node_id: None,
        kind: Some("text".into()),
        scope_type: Some(MemoryScopeType::Workflow),
        memory_type: Some(MemoryType::Fact),
        source: Some("manual".into()),
        pinned: None,
        confidence: None,
        salience: None,
        status: Some(MemoryStatus::Superseded),
        supersedes_id: None,
        fact_key: None,
        observed_at: None,
        last_confirmed_at: None,
        valid_from: None,
        expires_at: None,
        id: Some("native_lookup_history".into()),
    })
    .unwrap();

    let service = Arc::new(AgentMemoryLookupService::new(
        Arc::clone(&db),
        std::env::temp_dir().join(format!("alfred-native-lookup-{}", uuid::Uuid::new_v4())),
    ));
    let provider = AgentProvider::Codex;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let runtime = Arc::new(FakeNativeRuntime::new(provider));
    let allowance = Arc::clone(&runtime.memory_lookup_context_allowance);
    let registry = Arc::new(NativeRuntimeRegistry::default());
    let mut shared_lane = AlfredToolRequest::new("shared_lane", AlfredToolKind::Shell, "shell");
    shared_lane.path = Some(workspace.clone());
    registry.register(runtime.clone()).unwrap();
    let shared_allowance = Arc::clone(&allowance) as Arc<dyn MemoryLookupContextAllowance>;
    let executor = Arc::new(MemoryLookupToolExecutor::new(
        service,
        Arc::new(ReservingToolExecutor {
            allowance: shared_allowance,
            reserved_bytes: 1_024,
        }),
        Arc::new(DbMemoryLookupTraceSink::new(Arc::clone(&db))),
        Arc::new(NativeRuntimeMemoryLookupContextAllowance::new(Arc::clone(
            &registry,
        ))) as Arc<dyn MemoryLookupContextAllowance>,
    ));
    *runtime.request_tools.lock().unwrap() = vec![
        shared_lane,
        memory_lookup_tool(
            "lookup_search",
            "memory.search",
            json!({"query": "needle", "limit": 1})
                .as_object()
                .unwrap()
                .clone(),
        ),
        memory_lookup_tool(
            "lookup_get_page_0",
            "memory.get",
            json!({"reference": "$previous_reference"})
                .as_object()
                .unwrap()
                .clone(),
        ),
        memory_lookup_tool(
            "lookup_get_page_1",
            "memory.get",
            json!({
                "reference": "$previous_reference",
                "cursor": "$previous_cursor"
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
        memory_lookup_tool(
            "lookup_history_search",
            "history.search",
            json!({"query": "needle", "limit": 1})
                .as_object()
                .unwrap()
                .clone(),
        ),
        memory_lookup_tool(
            "lookup_history_get",
            "history.get",
            json!({"reference": "$previous_reference"})
                .as_object()
                .unwrap()
                .clone(),
        ),
    ];
    let resolver = Arc::new(FakeAccountResolver::new(account_ref.clone(), provider));
    let permission_db = Arc::clone(&db);
    let router = NativeExecutionRouter::new(
        Arc::clone(&registry),
        resolver,
        executor.clone(),
        Arc::new(FakeApprovalHandler(AlfredApprovalDecision::Allow)),
    )
    .with_policy(
        NativePermissionProfile {
            shell: NativeApprovalPolicy::Allow,
            ..NativePermissionProfile::default()
        },
        NativeToolCapabilitySet {
            shell: true,
            ..NativeToolCapabilitySet::default()
        },
    )
    .with_memory_lookup_permission_resolver(Arc::new(move |run_id, node_id| {
        !node_id.trim().is_empty()
            && permission_db
                .with_conn(|conn| {
                    Ok(conn.query_row(
                        "SELECT EXISTS(
                           SELECT 1 FROM runs r
                           JOIN workflows w ON w.id = r.workflow_id
                          WHERE r.id = ?1 AND w.memory_lookup_enabled = 1
                         )",
                        [run_id],
                        |row| row.get::<_, bool>(0),
                    )?)
                })
                .unwrap_or(false)
    }))
    .with_release_binding_validator(Arc::new(move |descriptor| {
        descriptor.provider == provider
            && descriptor.runtime_id == "fake-native-runtime"
            && descriptor.runtime_version == "1.0.0"
    }));
    let target = AgentExecutionTarget {
        provider,
        harness: AgentHarness::Alfred,
        account_ref: Some(account_ref),
        model: Some("fake-model".into()),
        working_directory: Some(workspace.to_string_lossy().into_owned()),
        request_metadata: AgentRequestMetadata::new(
            "native_lookup_run",
            "native_lookup_node",
        )
        .unwrap(),
    };
    let result = router
        .run(
            &target,
            AgentRequest {
                prompt: "Use the scoped memory tools.".into(),
                model: Some("fake-model".into()),
                skill: None,
                skill_name: None,
                skill_names: Vec::new(),
                working_directory: None,
                extra: serde_json::Value::Null,
            },
            AgentRunHooks {
                control: None,
                on_activity: None,
            },
        )
        .unwrap();
    assert!(result.output.contains("tool:"));
    assert!(runtime
        .observed_memory_lookup
        .load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(executor.active_turns(), 0);

    let rows = db
        .with_conn(|conn| lookup_trace_rows_conn(conn, "native_lookup_run"))
        .unwrap();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| {
        row.delivery == "delivered"
            && row.retrieved_bytes > 0
            && row.returned_bytes > 0
            && row.step_id.is_none()
    }));
    let source_count = db
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*)
                   FROM memory_lookup_trace_sources s
                   JOIN memory_lookup_traces t ON t.id = s.trace_id
                  WHERE t.run_id = ?1",
                ["native_lookup_run"],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .unwrap();
    assert!(source_count >= 5);
    let page_one_count = db
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*)
                   FROM memory_lookup_trace_sources s
                   JOIN memory_lookup_traces t ON t.id = s.trace_id
                  WHERE t.run_id = ?1 AND s.page = 1",
                ["native_lookup_run"],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .unwrap();
    assert!(page_one_count > 0);
    assert!(rows.iter().any(|row| row.tool == "memory.search"));
    assert!(rows.iter().any(|row| row.tool == "memory.get"));
    assert!(rows.iter().any(|row| row.tool == "history.search"));
    assert!(rows.iter().any(|row| row.tool == "history.get"));

    let forged_reference = db
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT s.reference
                   FROM memory_lookup_trace_sources s
                   JOIN memory_lookup_traces t ON t.id = s.trace_id
                  WHERE t.run_id = ?1 AND t.tool = 'memory.search'
                  ORDER BY t.created_at, t.id, s.ordinal
                  LIMIT 1",
                ["native_lookup_run"],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap();
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO runs (id, workflow_id, created_at) VALUES (?1, ?2, ?3)",
            params!["forged_lookup_run", workflow.id, "2025-01-01T00:00:01Z"],
        )?;
        Ok(())
    })
    .unwrap();
    *runtime.request_tools.lock().unwrap() = vec![memory_lookup_tool(
        "forged_cross_run_get",
        "memory.get",
        json!({"reference": forged_reference})
            .as_object()
            .unwrap()
            .clone(),
    )];
    runtime
        .continue_after_tool_error
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let forged_target = AgentExecutionTarget {
        request_metadata: AgentRequestMetadata::new(
            "forged_lookup_run",
            "native_lookup_node",
        )
        .unwrap(),
        ..target.clone()
    };
    let forged_activities = std::sync::Mutex::new(Vec::<AgentActivity>::new());
    let on_forged_activity = |activity: &AgentActivity| {
        forged_activities.lock().unwrap().push(activity.clone());
    };
    let forged_result = router.run(
        &forged_target,
        AgentRequest {
            prompt: "Reject a cross-run memory reference.".into(),
            model: Some("fake-model".into()),
            skill: None,
            skill_name: None,
            skill_names: Vec::new(),
            working_directory: None,
            extra: serde_json::Value::Null,
        },
        AgentRunHooks {
            control: None,
            on_activity: Some(&on_forged_activity),
        },
    );
    assert!(forged_result.is_ok());
    assert_eq!(executor.active_turns(), 0);
    let forged_rows = db
        .with_conn(|conn| lookup_trace_rows_conn(conn, "forged_lookup_run"))
        .unwrap();
    assert!(forged_rows.is_empty());
    let forged_activities = forged_activities.lock().unwrap();
    let forged_tool_activities = forged_activities
        .iter()
        .filter(|activity| activity.kind == AgentActivityKind::Tool)
        .collect::<Vec<_>>();
    assert_eq!(forged_tool_activities.len(), 2);
    assert_eq!(forged_tool_activities[0].state, AgentActivityState::Started);
    assert_eq!(
        forged_tool_activities[1].state,
        AgentActivityState::Completed
    );
}

#[test]
fn memory_lookup_requires_opt_in_verified_availability_and_context_capacity() {
    let (registry, runtime, resolver, account_ref) = fixture();
    let mut request = fake_request(AgentProvider::Codex, account_ref.clone());
    request.tool_capabilities.memory_lookup = true;
    *runtime.request_tool.lock().unwrap() = Some(memory_lookup_tool(
        "lookup_permission",
        "memory.search",
        json!({"query": "needle"})
            .as_object()
            .unwrap()
            .clone(),
    ));

    request.permission_profile.memory_lookup = NativeApprovalPolicy::Deny;
    let mut denied_events = Vec::new();
    let denied_result = registry
        .execute_turn(
            &request,
            resolver.as_ref(),
            &FakeToolExecutor {
                output: "must not run".into(),
            },
            &FakeApprovalHandler(AlfredApprovalDecision::Allow),
            &mut |event| denied_events.push(event.clone()),
        )
        .unwrap();
    assert!(!denied_result.output.contains("must not run"));
    assert_eq!(
        denied_events
            .iter()
            .filter(|event| event.kind == NativeEventKind::ToolStarted)
            .count(),
        1
    );
    let denied_completed = denied_events
        .iter()
        .filter(|event| event.kind == NativeEventKind::ToolCompleted)
        .collect::<Vec<_>>();
    assert_eq!(denied_completed.len(), 1);
    assert_eq!(
        denied_completed[0].metadata.get("status"),
        Some(&json!("denied"))
    );
    assert_eq!(
        denied_completed[0].tool_call_id.as_deref(),
        Some("lookup_permission")
    );

    request.permission_profile.memory_lookup = NativeApprovalPolicy::Allow;
    runtime
        .memory_lookup_available
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        registry
            .execute_turn(
                &request,
                resolver.as_ref(),
                &FakeToolExecutor {
                    output: "must not run".into(),
                },
                &FakeApprovalHandler(AlfredApprovalDecision::Allow),
                &mut |_| {},
            )
            .unwrap_err()
            .code,
        NativeErrorCode::CapabilityUnsupported
    );

    runtime
        .memory_lookup_available
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let allowance = Arc::new(FakeMemoryLookupContextAllowance::new(1_024));
    allowance.unknown.store(true, std::sync::atomic::Ordering::SeqCst);
    let executor = MemoryLookupToolExecutor::new(
        Arc::new(AgentMemoryLookupService::new(
            Arc::new(Db::open_in_memory().unwrap()),
            std::env::temp_dir(),
        )),
        Arc::new(DenyAllToolExecutor),
        Arc::new(FakeMemoryLookupTraceSink::new()),
        allowance,
    );
    *runtime.request_tool.lock().unwrap() = Some(memory_lookup_tool(
        "lookup_unknown_capacity",
        "memory.search",
        json!({"query": "needle"})
            .as_object()
            .unwrap()
            .clone(),
    ));
    assert_eq!(
        registry
            .execute_turn(
                &request,
                resolver.as_ref(),
                &executor,
                &FakeApprovalHandler(AlfredApprovalDecision::Allow),
                &mut |_| {},
            )
            .unwrap_err()
            .code,
        NativeErrorCode::CapabilityUnsupported
    );
}

#[test]
fn router_disables_lookup_for_unknown_capacity_without_aborting_turn() {
    let provider = AgentProvider::Codex;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let registry = Arc::new(NativeRuntimeRegistry::default());
    let runtime = Arc::new(FakeNativeRuntime::new(provider));
    runtime
        .memory_lookup_context_allowance
        .unknown
        .store(true, std::sync::atomic::Ordering::SeqCst);
    *runtime.request_tool.lock().unwrap() = Some(memory_lookup_tool(
        "unknown_capacity",
        "memory.search",
        json!({"query": "ignored"})
            .as_object()
            .unwrap()
            .clone(),
    ));
    registry.register(runtime.clone()).unwrap();
    let resolver = Arc::new(FakeAccountResolver::new(account_ref.clone(), provider));
    let db = Arc::new(Db::open_in_memory().unwrap());
    let executor = Arc::new(MemoryLookupToolExecutor::new(
        Arc::new(AgentMemoryLookupService::new(
            Arc::clone(&db),
            std::env::temp_dir(),
        )),
        Arc::new(DenyAllToolExecutor),
        Arc::new(FakeMemoryLookupTraceSink::new()),
        Arc::new(NativeRuntimeMemoryLookupContextAllowance::new(Arc::clone(
            &registry,
        ))) as Arc<dyn MemoryLookupContextAllowance>,
    ));
    let router = NativeExecutionRouter::new(
        Arc::clone(&registry),
        resolver,
        executor,
        Arc::new(DenyAllApprovalHandler),
    )
    .with_memory_lookup_permission_resolver(Arc::new(|_, _| true))
    .with_release_binding_validator(Arc::new(move |descriptor| {
        descriptor.provider == provider
            && descriptor.runtime_id == "fake-native-runtime"
            && descriptor.runtime_version == "1.0.0"
    }));
    assert!(!router
        .memory_lookup_availability(provider)
        .unwrap()
        .available);
    router
        .run(
            &AgentExecutionTarget {
                provider,
                harness: AgentHarness::Alfred,
                account_ref: Some(account_ref),
                model: Some("fake-model".into()),
                working_directory: None,
                request_metadata: AgentRequestMetadata::new("unknown_run", "unknown_node")
                    .unwrap(),
            },
            AgentRequest {
                prompt: "continue normally".into(),
                model: Some("fake-model".into()),
                skill: None,
                skill_name: None,
                skill_names: Vec::new(),
                working_directory: None,
                extra: serde_json::Value::Null,
            },
            AgentRunHooks {
                control: None,
                on_activity: None,
            },
        )
        .unwrap();
    assert!(!runtime
        .observed_memory_lookup
        .load(std::sync::atomic::Ordering::SeqCst));
}

struct ErrorToolExecutor(NativeRuntimeError);

impl AlfredToolExecutor for ErrorToolExecutor {
    fn execute(
        &self,
        _request: &AlfredToolRequest,
        _cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        Err(self.0.clone())
    }
}

#[test]
fn duplicate_native_request_ids_are_rejected_after_success_or_failure() {
    for first_call_fails in [false, true] {
        let (registry, runtime, resolver, account_ref) = fixture();
        let first = shell_tool(vec!["pwd".into()]);
        *runtime.request_tools.lock().unwrap() = vec![first.clone(), first];
        runtime
            .continue_after_tool_error
            .store(first_call_fails, std::sync::atomic::Ordering::SeqCst);
        let success_executor = FakeToolExecutor {
            output: "successful tool output".into(),
        };
        let failed_executor = ErrorToolExecutor(NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "first tool failed",
            false,
        ));
        let executor: &dyn AlfredToolExecutor = if first_call_fails {
            &failed_executor
        } else {
            &success_executor
        };
        let mut request = fake_request(AgentProvider::Codex, account_ref);
        request.permission_profile.shell = NativeApprovalPolicy::Allow;
        let mut events = Vec::new();
        let error = registry
            .execute_turn(
                &request,
                resolver.as_ref(),
                executor,
                &FakeApprovalHandler(AlfredApprovalDecision::Allow),
                &mut |event| events.push(event.clone()),
            )
            .unwrap_err();
        assert_eq!(error.code, NativeErrorCode::InvalidRequest);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == NativeEventKind::ToolStarted)
                .count(),
            1
        );
        let completed = events
            .iter()
            .filter(|event| event.kind == NativeEventKind::ToolCompleted)
            .collect::<Vec<_>>();
        assert_eq!(completed.len(), 1);
        let expected_status = if first_call_fails {
            "failed"
        } else {
            "completed"
        };
        assert_eq!(
            completed[0].metadata.get("status"),
            Some(&json!(expected_status))
        );
    }
}

#[test]
fn cancelled_after_lookup_result_rejects_trace_and_restores_capacity() {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let workflow = db
        .create_workflow(CreateWorkflowInput {
            name: "Native cancellation fixture".into(),
            description: String::new(),
            working_directory: String::new(),
            folder_id: None,
            graph: json!({"nodes": [], "edges": []}),
        })
        .unwrap();
    let workflow_id = workflow.id.clone();
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE workflows SET memory_lookup_enabled = 1 WHERE id = ?1",
            [&workflow_id],
        )?;
        conn.execute(
            "INSERT INTO runs (id, workflow_id, created_at) VALUES (?1, ?2, ?3)",
            params!["cancel_run", workflow_id.clone(), "2025-01-01T00:00:00Z"],
        )?;
        Ok(())
    })
    .unwrap();
    db.create_memory(CreateMemoryInput {
        workflow_id: workflow_id.clone(),
        title: "Cancellation lookup note".into(),
        body: "cancellation body".into(),
        run_id: None,
        node_id: None,
        kind: Some("text".into()),
        scope_type: Some(MemoryScopeType::Workflow),
        memory_type: Some(MemoryType::Fact),
        source: Some("manual".into()),
        pinned: None,
        confidence: None,
        salience: None,
        status: Some(MemoryStatus::Active),
        supersedes_id: None,
        fact_key: None,
        observed_at: None,
        last_confirmed_at: None,
        valid_from: None,
        expires_at: None,
        id: Some("cancel_memory".into()),
    })
    .unwrap();

    let (registry, runtime, resolver, account_ref) = fixture();
    *runtime.request_tool.lock().unwrap() = Some(memory_lookup_tool(
        "cancel_lookup",
        "memory.search",
        json!({"query": "cancellation"})
            .as_object()
            .unwrap()
            .clone(),
    ));
    let cancellation = NativeCancellation::new("cancel_turn", DEFAULT_TURN_TIMEOUT).unwrap();
    let allowance = Arc::clone(&runtime.memory_lookup_context_allowance);
    let executor = MemoryLookupToolExecutor::new(
        Arc::new(AgentMemoryLookupService::new(
            Arc::clone(&db),
            std::env::temp_dir().join(format!(
                "alfred-native-cancel-lookup-{}",
                uuid::Uuid::new_v4()
            )),
        )),
        Arc::new(DenyAllToolExecutor),
        Arc::new(DbMemoryLookupTraceSink::new(Arc::clone(&db))),
        Arc::new(CancelAfterReleaseAllowance {
            inner: Arc::clone(&allowance),
            cancellation: cancellation.clone(),
            timed_out_after_release: false,
        }),
    );
    let mut request = fake_request(AgentProvider::Codex, account_ref);
    request.run_id = "cancel_run".into();
    request.node_id = "cancel_node".into();
    request.working_directory = std::env::current_dir().unwrap();
    request.permission_profile.memory_lookup = NativeApprovalPolicy::Allow;
    request.tool_capabilities.memory_lookup = true;
    request.cancellation = Some(cancellation);
    let mut events = Vec::new();
    let error = registry
        .execute_turn(
            &request,
            resolver.as_ref(),
            &executor,
            &FakeApprovalHandler(AlfredApprovalDecision::Allow),
            &mut |event| events.push(event.clone()),
        )
        .unwrap_err();
    assert_eq!(error.code, NativeErrorCode::Cancelled);
    assert_eq!(executor.active_turns(), 0);
    assert_eq!(allowance.remaining_for("cancel_turn"), Some(16_000));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == NativeEventKind::ToolStarted)
            .count(),
        1
    );
    let completed = events
        .iter()
        .filter(|event| event.kind == NativeEventKind::ToolCompleted)
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0].metadata.get("status"),
        Some(&json!("cancelled"))
    );
    let rows = db
        .with_conn(|conn| lookup_trace_rows_conn(conn, "cancel_run"))
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].delivery, "rejected_cancelled");
    assert_eq!(rows[0].returned_bytes, 0);
    assert!(rows[0].retrieved_bytes > 0);
    let source_count = db
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*)
                   FROM memory_lookup_trace_sources s
                   JOIN memory_lookup_traces t ON t.id = s.trace_id
                  WHERE t.run_id = ?1",
                ["cancel_run"],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .unwrap();
    assert!(source_count > 0);
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO runs (id, workflow_id, created_at) VALUES (?1, ?2, ?3)",
            params!["timeout_run", workflow_id.clone(), "2025-01-01T00:00:00Z"],
        )?;
        Ok(())
    })
    .unwrap();
    *runtime.request_tool.lock().unwrap() = Some(memory_lookup_tool(
        "timeout_lookup",
        "memory.search",
        json!({"query": "cancellation"})
            .as_object()
            .unwrap()
            .clone(),
    ));
    let timeout_cancellation =
        NativeCancellation::new("timeout_turn", Duration::from_millis(100)).unwrap();
    let timeout_executor = MemoryLookupToolExecutor::new(
        Arc::new(AgentMemoryLookupService::new(
            Arc::clone(&db),
            std::env::temp_dir().join(format!(
                "alfred-native-timeout-lookup-{}",
                uuid::Uuid::new_v4()
            )),
        )),
        Arc::new(DenyAllToolExecutor),
        Arc::new(DbMemoryLookupTraceSink::new(Arc::clone(&db))),
        Arc::new(CancelAfterReleaseAllowance {
            inner: Arc::clone(&allowance),
            cancellation: timeout_cancellation.clone(),
            timed_out_after_release: true,
        }),
    );
    let timeout_account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let mut timeout_request = fake_request(AgentProvider::Codex, timeout_account_ref);
    timeout_request.run_id = "timeout_run".into();
    timeout_request.node_id = "timeout_node".into();
    timeout_request.working_directory = std::env::current_dir().unwrap();
    timeout_request.permission_profile.memory_lookup = NativeApprovalPolicy::Allow;
    timeout_request.tool_capabilities.memory_lookup = true;
    timeout_request.timeout_ms = 100;
    timeout_request.cancellation = Some(timeout_cancellation);
    let mut timeout_events = Vec::new();
    let timeout_error = registry
        .execute_turn(
            &timeout_request,
            resolver.as_ref(),
            &timeout_executor,
            &FakeApprovalHandler(AlfredApprovalDecision::Allow),
            &mut |event| timeout_events.push(event.clone()),
        )
        .unwrap_err();
    assert_eq!(timeout_error.code, NativeErrorCode::TimedOut);
    assert_eq!(timeout_executor.active_turns(), 0);
    assert_eq!(allowance.remaining_for("timeout_turn"), Some(16_000));
    let timeout_completed = timeout_events
        .iter()
        .filter(|event| event.kind == NativeEventKind::ToolCompleted)
        .collect::<Vec<_>>();
    assert_eq!(timeout_completed.len(), 1);
    assert_eq!(
        timeout_completed[0].metadata.get("status"),
        Some(&json!("timed_out"))
    );
    let timeout_rows = db
        .with_conn(|conn| lookup_trace_rows_conn(conn, "timeout_run"))
        .unwrap();
    assert_eq!(timeout_rows.len(), 1);
    assert_eq!(timeout_rows[0].delivery, "rejected_timeout");
    assert_eq!(timeout_rows[0].returned_bytes, 0);
    assert!(timeout_rows[0].retrieved_bytes > 0);
}

#[test]
fn failed_tool_calls_emit_terminal_statuses_without_dangling_starts() {
    for error in [NativeRuntimeError::cancelled(), NativeRuntimeError::timed_out()] {
        let (registry, runtime, resolver, account_ref) = fixture();
        *runtime.request_tool.lock().unwrap() = Some(shell_tool(vec!["pwd".into()]));
        let mut request = fake_request(AgentProvider::Codex, account_ref);
        request.permission_profile.shell = NativeApprovalPolicy::Allow;
        let mut events = Vec::new();
        let error_code = error.code;
        assert_eq!(
            registry
                .execute_turn(
                    &request,
                    resolver.as_ref(),
                    &ErrorToolExecutor(error),
                    &FakeApprovalHandler(AlfredApprovalDecision::Allow),
                    &mut |event| events.push(event.clone()),
                )
                .unwrap_err()
                .code,
            error_code
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == NativeEventKind::ToolStarted)
                .count(),
            1
        );
        let completed = events
            .iter()
            .filter(|event| event.kind == NativeEventKind::ToolCompleted)
            .collect::<Vec<_>>();
        assert_eq!(completed.len(), 1);
        assert!(
            completed[0]
                .tool_output
                .as_deref()
                .is_some_and(|output| output.contains(match error_code {
                    NativeErrorCode::Cancelled => "cancelled",
                    NativeErrorCode::TimedOut => "timed out",
                    _ => "failed",
                }))
        );
    }
}

#[test]
fn router_gates_lookup_advertisement_on_trusted_workflow_permission() {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let workflow = db
        .create_workflow(CreateWorkflowInput {
            name: "Router lookup permission fixture".into(),
            description: String::new(),
            working_directory: String::new(),
            folder_id: None,
            graph: json!({"nodes": [], "edges": []}),
        })
        .unwrap();
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE workflows SET memory_lookup_enabled = 1 WHERE id = ?1",
            [&workflow.id],
        )?;
        conn.execute(
            "INSERT INTO runs (id, workflow_id, created_at) VALUES (?1, ?2, ?3)",
            params!["run_enabled", workflow.id, "2025-01-01T00:00:00Z"],
        )?;
        Ok(())
    })
    .unwrap();
    db.create_memory(CreateMemoryInput {
        workflow_id: workflow.id.clone(),
        title: "Router permission note".into(),
        body: "router permission body".into(),
        run_id: None,
        node_id: None,
        kind: Some("text".into()),
        scope_type: Some(MemoryScopeType::Workflow),
        memory_type: Some(MemoryType::Fact),
        source: Some("manual".into()),
        pinned: None,
        confidence: None,
        salience: None,
        status: Some(MemoryStatus::Active),
        supersedes_id: None,
        fact_key: None,
        observed_at: None,
        last_confirmed_at: None,
        valid_from: None,
        expires_at: None,
        id: Some("router_permission_memory".into()),
    })
    .unwrap();

    let provider = AgentProvider::Codex;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let registry = Arc::new(NativeRuntimeRegistry::default());
    let runtime = Arc::new(FakeNativeRuntime::new(provider));
    registry.register(runtime.clone()).unwrap();
    *runtime.request_tools.lock().unwrap() = vec![
        memory_lookup_tool(
            "router_lookup_enabled",
            "memory.search",
            json!({"query": "router"})
                .as_object()
                .unwrap()
                .clone(),
        ),
        memory_lookup_tool(
            "router_lookup_revoked",
            "memory.search",
            json!({"query": "router"})
                .as_object()
                .unwrap()
                .clone(),
        ),
    ];
    let executor = Arc::new(MemoryLookupToolExecutor::new(
        Arc::new(AgentMemoryLookupService::new(
            Arc::clone(&db),
            std::env::temp_dir().join(format!(
                "alfred-native-router-lookup-{}",
                uuid::Uuid::new_v4()
            )),
        )),
        Arc::new(DenyAllToolExecutor),
        Arc::new(DbMemoryLookupTraceSink::new(Arc::clone(&db))),
        Arc::new(NativeRuntimeMemoryLookupContextAllowance::new(Arc::clone(
            &registry,
        ))) as Arc<dyn MemoryLookupContextAllowance>,
    ));
    let resolver = Arc::new(FakeAccountResolver::new(account_ref.clone(), provider));
    let permission_db = Arc::clone(&db);
    let activities = Arc::new(std::sync::Mutex::new(Vec::<AgentActivity>::new()));
    let activity_log = Arc::clone(&activities);
    let revoke_once = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let revoke_flag = Arc::clone(&revoke_once);
    let revoke_db = Arc::clone(&db);
    let revoke_workflow_id = workflow.id.clone();
    let on_activity = move |activity: &AgentActivity| {
        activity_log.lock().unwrap().push(activity.clone());
        if activity.kind == AgentActivityKind::Tool
            && activity.state == AgentActivityState::Completed
            && !revoke_flag.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            revoke_db
                .with_conn(|conn| {
                    conn.execute(
                        "UPDATE workflows SET memory_lookup_enabled = 0 WHERE id = ?1",
                        [&revoke_workflow_id],
                    )?;
                    Ok(())
                })
                .unwrap();
        }
    };
    let router = NativeExecutionRouter::new(
        Arc::clone(&registry),
        resolver,
        executor.clone(),
        Arc::new(FakeApprovalHandler(AlfredApprovalDecision::Allow)),
    )
    .with_memory_lookup_permission_resolver(Arc::new(move |run_id, node_id| {
        !node_id.trim().is_empty()
            && permission_db
                .with_conn(|conn| {
                    Ok(conn.query_row(
                        "SELECT EXISTS(
                           SELECT 1 FROM runs r
                           JOIN workflows w ON w.id = r.workflow_id
                          WHERE r.id = ?1 AND w.memory_lookup_enabled = 1
                         )",
                        [run_id],
                        |row| row.get::<_, bool>(0),
                    )?)
                })
                .unwrap_or(false)
    }))
    .with_release_binding_validator(Arc::new(move |descriptor| {
        descriptor.provider == provider
            && descriptor.runtime_id == "fake-native-runtime"
            && descriptor.runtime_version == "1.0.0"
    }));
    let request = || AgentRequest {
        prompt: "Use memory if allowed.".into(),
        model: Some("fake-model".into()),
        skill: None,
        skill_name: None,
        skill_names: Vec::new(),
        working_directory: None,
        extra: serde_json::Value::Null,
    };
    let target = |run_id: &str| AgentExecutionTarget {
        provider,
        harness: AgentHarness::Alfred,
        account_ref: Some(account_ref.clone()),
        model: Some("fake-model".into()),
        working_directory: Some(std::env::current_dir().unwrap().to_string_lossy().into_owned()),
        request_metadata: AgentRequestMetadata::new(run_id, "router_node").unwrap(),
    };
    runtime
        .continue_after_tool_error
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let result = router
        .run(
            &target("run_enabled"),
            request(),
            AgentRunHooks {
                control: None,
                on_activity: Some(&on_activity),
            },
        )
        .unwrap();
    assert!(result.output.contains("tool:"));
    assert!(runtime
        .observed_memory_lookup
        .load(std::sync::atomic::Ordering::SeqCst));
    assert!(revoke_once.load(std::sync::atomic::Ordering::SeqCst));
    let activities = activities.lock().unwrap().clone();
    let tool_activities = activities
        .iter()
        .filter(|activity| activity.kind == AgentActivityKind::Tool)
        .collect::<Vec<_>>();
    assert_eq!(tool_activities.len(), 4);
    assert_eq!(tool_activities[0].state, AgentActivityState::Started);
    assert_eq!(tool_activities[1].state, AgentActivityState::Completed);
    assert_eq!(tool_activities[2].state, AgentActivityState::Started);
    assert_eq!(tool_activities[3].state, AgentActivityState::Completed);
    let enabled_rows = db
        .with_conn(|conn| lookup_trace_rows_conn(conn, "run_enabled"))
        .unwrap();
    assert_eq!(enabled_rows.len(), 1);
    assert_eq!(enabled_rows[0].delivery, "delivered");
    assert_eq!(executor.active_turns(), 0);
    assert!(db
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT memory_lookup_enabled FROM workflows WHERE id = ?1",
                [&workflow.id],
                |row| row.get::<_, bool>(0),
            )?)
        })
        .unwrap()
        == false);
}

#[test]
fn shared_registry_routers_report_only_their_installed_lookup_executor() {
    let provider = AgentProvider::Codex;
    let account_ref = OpaqueAgentAccountRef::parse("account_fake-01").unwrap();
    let registry = Arc::new(NativeRuntimeRegistry::default());
    let runtime = Arc::new(FakeNativeRuntime::new(provider));
    registry.register(runtime).unwrap();
    let resolver = Arc::new(FakeAccountResolver::new(account_ref, provider));
    let db = Arc::new(Db::open_in_memory().unwrap());
    let lookup_executor = Arc::new(MemoryLookupToolExecutor::new(
        Arc::new(AgentMemoryLookupService::new(
            Arc::clone(&db),
            std::env::temp_dir(),
        )),
        Arc::new(DenyAllToolExecutor),
        Arc::new(DbMemoryLookupTraceSink::new(Arc::clone(&db))),
        Arc::new(NativeRuntimeMemoryLookupContextAllowance::new(Arc::clone(
            &registry,
        ))) as Arc<dyn MemoryLookupContextAllowance>,
    ));
    let lookup_router = NativeExecutionRouter::new(
        Arc::clone(&registry),
        Arc::clone(&resolver) as Arc<dyn NativeAccountResolver>,
        lookup_executor,
        Arc::new(DenyAllApprovalHandler),
    )
    .with_release_binding_validator(Arc::new(move |descriptor| {
        descriptor.provider == provider
            && descriptor.runtime_id == "fake-native-runtime"
            && descriptor.runtime_version == "1.0.0"
    }));
    let deny_router = NativeExecutionRouter::new(
        Arc::clone(&registry),
        resolver,
        Arc::new(DenyAllToolExecutor),
        Arc::new(DenyAllApprovalHandler),
    );
    assert!(lookup_router
        .memory_lookup_availability(provider)
        .unwrap()
        .available);
    assert!(!deny_router
        .memory_lookup_availability(provider)
        .unwrap()
        .available);
    assert!(registry
        .memory_lookup_availability(provider)
        .unwrap()
        .available);
}
