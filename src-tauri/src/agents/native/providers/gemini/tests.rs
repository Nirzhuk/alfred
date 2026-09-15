//! Plan 038 fixtures. No network, Gemini CLI, API key, or Cloud project.

use super::credential::{GeminiCredential, TestGeminiApiKey};
use super::protocol::{error_for_status, parse_stream_chunk, GeminiChunkEvent, MAX_CHUNK_BYTES};
use super::runtime::GeminiNativeRuntime;
use super::surface::{blocked_surface_codes, GeminiAuthSurface, GeminiSurfaceStatus};
use super::transport::{GeminiByteStream, GeminiTransport};
use super::{
    native_gates, native_ready, register, GEMINI_AUTH_SURFACES, GEMINI_NATIVE_RUNTIME_VERSION,
    SELECTED_SURFACE,
};
use crate::agents::native::*;
use crate::agents::{AgentHarness, AgentProvider, OpaqueAgentAccountRef};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TEST_KEY: &str = "fixtureGeminiApiKey_0123456789abcdef";
const TEST_MODEL: &str = "gemini-3.7-flash";

struct TestResolver {
    account_ref: OpaqueAgentAccountRef,
    key: String,
}

impl NativeAccountResolver for TestResolver {
    fn resolve(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        provider: AgentProvider,
        product: crate::agent_accounts::models::AgentProductId,
    ) -> Result<ResolvedNativeAccount, NativeRuntimeError> {
        if account_ref != &self.account_ref
            || provider != AgentProvider::Gemini
            || product != crate::agent_accounts::models::AgentProductId::GeminiApi
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "gemini fixture account mismatch",
                false,
            ));
        }
        Ok(ResolvedNativeAccount {
            account_ref: account_ref.clone(),
            provider,
            product: crate::agent_accounts::models::AgentProductId::GeminiApi,
            credential: NativeCredential::new(TestGeminiApiKey(self.key.clone())),
        })
    }
}

#[derive(Default)]
struct ScriptedTransport {
    turns: Mutex<Vec<Vec<Vec<u8>>>>,
    fail_status: Option<u16>,
    catalog: Option<String>,
    delay: Duration,
    cancel_after: Option<(usize, NativeCancellation)>,
    bodies: Mutex<Vec<Value>>,
}

impl ScriptedTransport {
    fn new(turns: Vec<Vec<Vec<u8>>>) -> Self {
        Self {
            turns: Mutex::new(turns),
            ..Self::default()
        }
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().expect("bodies").clone()
    }
}

impl GeminiTransport for ScriptedTransport {
    fn stream_generate(
        &self,
        credential: &GeminiCredential,
        model: &str,
        body: &Value,
        _cancellation: &NativeCancellation,
    ) -> Result<Box<dyn GeminiByteStream>, NativeRuntimeError> {
        assert_eq!(credential.header_value(), TEST_KEY);
        assert_eq!(model, TEST_MODEL);
        self.bodies.lock().expect("bodies").push(body.clone());
        if let Some(status) = self.fail_status {
            return Err(error_for_status(status, "fixture"));
        }
        let mut turns = self.turns.lock().expect("turns");
        let chunks = if turns.is_empty() {
            Vec::new()
        } else {
            turns.remove(0)
        };
        Ok(Box::new(ScriptedStream {
            chunks,
            index: 0,
            delay: self.delay,
            cancel_after: self.cancel_after.clone(),
        }))
    }

    fn list_models(&self, credential: &GeminiCredential) -> Result<String, NativeRuntimeError> {
        assert_eq!(credential.header_value(), TEST_KEY);
        Ok(self.catalog.clone().unwrap_or_else(|| {
            json!({
                "models": [{
                    "name": format!("models/{TEST_MODEL}"),
                    "displayName": "Gemini 3.7 Flash",
                    "supportedGenerationMethods": ["generateContent"]
                }]
            })
            .to_string()
        }))
    }
}

struct ScriptedStream {
    chunks: Vec<Vec<u8>>,
    index: usize,
    delay: Duration,
    cancel_after: Option<(usize, NativeCancellation)>,
}

impl GeminiByteStream for ScriptedStream {
    fn next_chunk(
        &mut self,
        _cancellation: &NativeCancellation,
    ) -> Result<Option<Vec<u8>>, NativeRuntimeError> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        if let Some((after, cancellation)) = self.cancel_after.as_ref() {
            if self.index >= *after {
                cancellation.cancel();
            }
        }
        let chunk = self.chunks[self.index].clone();
        self.index += 1;
        Ok(Some(chunk))
    }
}

struct Harness {
    registry: NativeRuntimeRegistry,
    resolver: TestResolver,
    transport: Arc<ScriptedTransport>,
    account_ref: OpaqueAgentAccountRef,
}

fn harness(transport: ScriptedTransport) -> Harness {
    let transport = Arc::new(transport);
    let registry = NativeRuntimeRegistry::default();
    registry
        .register(Arc::new(GeminiNativeRuntime::new(transport.clone())))
        .expect("register fixture runtime");
    let account_ref = OpaqueAgentAccountRef::parse("account_gemini-01").expect("account");
    Harness {
        registry,
        resolver: TestResolver {
            account_ref: account_ref.clone(),
            key: TEST_KEY.into(),
        },
        transport,
        account_ref,
    }
}

fn request(account_ref: OpaqueAgentAccountRef) -> NativeTurnRequest {
    request_with(
        account_ref,
        NativeEventLimits::default(),
        DEFAULT_TURN_TIMEOUT,
    )
}

fn request_with(
    account_ref: OpaqueAgentAccountRef,
    event_limits: NativeEventLimits,
    timeout: Duration,
) -> NativeTurnRequest {
    let workspace = std::env::current_dir().expect("workspace");
    NativeTurnRequest {
        contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
        harness: AgentHarness::Alfred,
        harness_version: env!("CARGO_PKG_VERSION").into(),
        runtime_version: GEMINI_NATIVE_RUNTIME_VERSION.into(),
        provider: AgentProvider::Gemini,
        account_ref,
        run_id: "run_gemini".into(),
        node_id: "node_gemini".into(),
        model: TEST_MODEL.into(),
        prompt: "inspect the workspace".into(),
        context: vec![NativeContextBlock {
            role: NativeContextRole::User,
            content: "inspect the workspace".into(),
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
        event_limits,
        timeout_ms: timeout.as_millis() as u64,
        cancellation: Some(NativeCancellation::new("cancel_gemini", timeout).expect("cancel")),
    }
}

fn run(
    harness: &Harness,
    request: &NativeTurnRequest,
    executor: &dyn AlfredToolExecutor,
    approver: &dyn AlfredApprovalHandler,
) -> Result<NativeExecutionResult, NativeRuntimeError> {
    let mut sink = |_: &NativeEvent| {};
    harness
        .registry
        .execute_turn(request, &harness.resolver, executor, approver, &mut sink)
}

fn sse(value: Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}

fn text_turn(text: &str) -> Vec<Vec<u8>> {
    vec![sse(json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 9, "candidatesTokenCount": 4}
    }))]
}

fn tool_turn(name: &str, args: Value) -> Vec<Vec<u8>> {
    vec![sse(json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{
                "thoughtSignature": "opaque-signature",
                "functionCall": {"id": "call_1", "name": name, "args": args}
            }]},
            "finishReason": "STOP"
        }]
    }))]
}

struct RecordingExecutor {
    output: String,
    seen: Mutex<Vec<AlfredToolRequest>>,
}

impl AlfredToolExecutor for RecordingExecutor {
    fn execute(
        &self,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        cancellation.checkpoint()?;
        self.seen.lock().expect("seen").push(request.clone());
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

struct FixedApprover(AlfredApprovalDecision);

impl AlfredApprovalHandler for FixedApprover {
    fn decide(
        &self,
        _request: &AlfredApprovalRequest,
        _cancellation: &NativeCancellation,
    ) -> Result<AlfredApprovalDecision, NativeRuntimeError> {
        Ok(self.0)
    }
}

#[test]
fn surface_evidence_keeps_all_four_auth_and_billing_boundaries_distinct() {
    assert_eq!(SELECTED_SURFACE, GeminiAuthSurface::ApiKey);
    assert_eq!(GEMINI_AUTH_SURFACES.len(), 4);
    assert_eq!(
        GEMINI_AUTH_SURFACES[0].status,
        GeminiSurfaceStatus::Selected
    );
    assert!(GEMINI_AUTH_SURFACES[0].billing_owner.contains("project"));
    assert!(GEMINI_AUTH_SURFACES[2]
        .project_region
        .contains("Standard Vertex"));
    assert!(GEMINI_AUTH_SURFACES[3].billing_owner.contains("consumer"));
    assert_eq!(
        blocked_surface_codes(),
        vec![
            "gemini_oauth_client_packaging_unresolved",
            "gemini_vertex_project_binding_unresolved",
            "gemini_consumer_subscription_prohibited"
        ]
    );
}

#[test]
fn runtime_is_api_key_only_and_production_readiness_is_fail_closed() {
    let runtime = GeminiNativeRuntime::new(Arc::new(ScriptedTransport::default()));
    let descriptor = runtime.descriptor();
    assert_eq!(descriptor.provider, AgentProvider::Gemini);
    assert!(descriptor.capabilities.supports_api_key);
    assert!(!descriptor.capabilities.supports_oauth);
    assert!(!descriptor.capabilities.supports_usage);
    assert!(!descriptor.capabilities.supports_sessions);
    assert!(!native_ready());
    assert_eq!(
        native_gates()
            .iter()
            .filter(|gate| gate.status == CapabilityReportStatus::Blocked)
            .count(),
        2
    );
    let error = register(&NativeRuntimeRegistry::default()).expect_err("registration blocked");
    assert!(error
        .message
        .contains("gemini_api_key_account_intake_unavailable"));
    assert!(error.message.contains("gemini_live_api_key_smoke_missing"));
}

#[test]
fn model_catalog_and_usage_unavailable_are_explicit() {
    let harness = harness(ScriptedTransport::default());
    let models = harness
        .registry
        .discover_models(
            AgentProvider::Gemini,
            &harness.account_ref,
            &harness.resolver,
        )
        .expect("models");
    assert_eq!(models[0].id, TEST_MODEL);
    let usage = harness
        .registry
        .usage_snapshot(
            AgentProvider::Gemini,
            &harness.account_ref,
            &harness.resolver,
        )
        .expect("usage");
    assert_eq!(usage.state, NativeUsageState::Unavailable);
    assert!(usage.input_tokens.is_none());
}

#[test]
fn streamed_text_maps_to_bounded_native_events_and_turn_token_metadata() {
    let harness = harness(ScriptedTransport::new(vec![text_turn("hello")]));
    let request = request(harness.account_ref.clone());
    let result = run(
        &harness,
        &request,
        &DenyAllToolExecutor,
        &DenyAllApprovalHandler,
    )
    .expect("turn");
    assert_eq!(result.output, "hello");
    assert_eq!(
        result
            .events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![
            NativeEventKind::TurnStarted,
            NativeEventKind::AssistantDelta,
            NativeEventKind::TurnCompleted
        ]
    );
    let completed = result.events.last().expect("completed");
    assert_eq!(
        completed.metadata["accountUsageState"],
        json!("unavailable")
    );
    assert_eq!(completed.metadata["providerInputUnitCount"], json!(9));
}

#[test]
fn function_call_runs_only_through_alfred_and_replays_id_signature_and_result() {
    let harness = harness(ScriptedTransport::new(vec![
        tool_turn(
            "alfred_run_shell",
            json!({"path": ".", "command": ["rg", "--files"]}),
        ),
        text_turn("done"),
    ]));
    let request = request(harness.account_ref.clone());
    let executor = RecordingExecutor {
        output: "a.rs".into(),
        seen: Mutex::new(Vec::new()),
    };
    let result = run(
        &harness,
        &request,
        &executor,
        &FixedApprover(AlfredApprovalDecision::Allow),
    )
    .expect("tool turn");
    assert_eq!(result.output, "done");
    let seen = executor.seen.lock().expect("seen");
    assert_eq!(seen[0].request_id, "call_1");
    assert_eq!(seen[0].kind, AlfredToolKind::Shell);
    assert_eq!(seen[0].path, Some(PathBuf::from(".")));
    assert_eq!(seen[0].arguments, vec!["rg", "--files"]);
    drop(seen);
    let bodies = harness.transport.bodies();
    assert_eq!(
        bodies[1]["contents"][1]["parts"][0]["thoughtSignature"],
        json!("opaque-signature")
    );
    assert_eq!(
        bodies[1]["contents"][2]["parts"][0]["functionResponse"]["id"],
        json!("call_1")
    );
}

#[test]
fn approval_denial_is_returned_to_gemini_without_executing_the_tool() {
    let harness = harness(ScriptedTransport::new(vec![
        tool_turn(
            "alfred_write_file",
            json!({"path": "denied.txt", "content": "no"}),
        ),
        text_turn("not written"),
    ]));
    let request = request(harness.account_ref.clone());
    let executor = RecordingExecutor {
        output: "must not run".into(),
        seen: Mutex::new(Vec::new()),
    };
    let result = run(
        &harness,
        &request,
        &executor,
        &FixedApprover(AlfredApprovalDecision::Deny),
    )
    .expect("denied turn");
    assert!(executor.seen.lock().expect("seen").is_empty());
    assert!(result.events.iter().any(|event| {
        event.kind == NativeEventKind::ApprovalResolved && event.approved == Some(false)
    }));
    let bodies = harness.transport.bodies();
    assert_eq!(
        bodies[1]["contents"][2]["parts"][0]["functionResponse"]["response"]["status"],
        json!("denied")
    );
}

#[test]
fn content_blocks_are_failures_not_empty_successes() {
    let harness = harness(ScriptedTransport::new(vec![vec![sse(json!({
        "promptFeedback": {"blockReason": "PROHIBITED_CONTENT"}
    }))]]));
    let request = request(harness.account_ref.clone());
    let error = run(
        &harness,
        &request,
        &DenyAllToolExecutor,
        &DenyAllApprovalHandler,
    )
    .expect_err("blocked");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    assert!(error.message.contains("blocked"));

    let events = parse_stream_chunk(
        &json!({
            "candidates": [{
                "content": {"parts": [{"text": "withheld"}]},
                "finishReason": "SAFETY"
            }]
        })
        .to_string(),
        &NativeEventLimits::default(),
    )
    .expect("mapped block");
    assert!(matches!(
        events.as_slice(),
        [GeminiChunkEvent::Blocked { .. }]
    ));
}

#[test]
fn auth_revocation_and_rate_limit_have_stable_states() {
    for (status, code, retryable) in [
        (401, NativeErrorCode::AccountUnavailable, false),
        (403, NativeErrorCode::AccountUnavailable, false),
        (429, NativeErrorCode::ProviderUnavailable, true),
    ] {
        let mut transport = ScriptedTransport::default();
        transport.fail_status = Some(status);
        let harness = harness(transport);
        let request = request(harness.account_ref.clone());
        let error = run(
            &harness,
            &request,
            &DenyAllToolExecutor,
            &DenyAllApprovalHandler,
        )
        .expect_err("provider failure");
        assert_eq!(error.code, code, "status {status}");
        assert_eq!(error.retryable, retryable, "status {status}");
    }
}

#[test]
fn malformed_partial_and_oversized_streams_fail_closed() {
    let cases = [
        (
            vec![b"data: {not json}\n\n".to_vec()],
            NativeErrorCode::InvalidEvent,
        ),
        (
            vec![b"data: {\"candidates\":".to_vec()],
            NativeErrorCode::InvalidEvent,
        ),
        (
            vec![vec![b'x'; MAX_CHUNK_BYTES + 8 * 1024]],
            NativeErrorCode::EventLimitExceeded,
        ),
    ];
    for (chunks, expected) in cases {
        let harness = harness(ScriptedTransport::new(vec![chunks]));
        let request = request(harness.account_ref.clone());
        let error = run(
            &harness,
            &request,
            &DenyAllToolExecutor,
            &DenyAllApprovalHandler,
        )
        .expect_err("invalid stream");
        assert_eq!(error.code, expected);
    }
}

#[test]
fn oversized_output_is_rejected_before_completion() {
    let harness = harness(ScriptedTransport::new(vec![text_turn(&"x".repeat(512))]));
    let limits = NativeEventLimits {
        max_text_bytes: 32,
        ..NativeEventLimits::default()
    };
    let request = request_with(harness.account_ref.clone(), limits, DEFAULT_TURN_TIMEOUT);
    let error = run(
        &harness,
        &request,
        &DenyAllToolExecutor,
        &DenyAllApprovalHandler,
    )
    .expect_err("oversized");
    assert_eq!(error.code, NativeErrorCode::EventLimitExceeded);
}

#[test]
fn cancellation_and_timeout_checkpoint_between_stream_reads() {
    let cancellation =
        NativeCancellation::new("cancel_gemini", DEFAULT_TURN_TIMEOUT).expect("cancel");
    let mut transport = ScriptedTransport::new(vec![text_turn("hello")]);
    transport.cancel_after = Some((0, cancellation.clone()));
    let cancelled_harness = harness(transport);
    let mut request = request(cancelled_harness.account_ref.clone());
    request.cancellation = Some(cancellation);
    let error = run(
        &cancelled_harness,
        &request,
        &DenyAllToolExecutor,
        &DenyAllApprovalHandler,
    )
    .expect_err("cancelled");
    assert_eq!(error.code, NativeErrorCode::Cancelled);

    let mut transport = ScriptedTransport::new(vec![text_turn("hello")]);
    transport.delay = Duration::from_millis(40);
    let harness = harness(transport);
    let request = request_with(
        harness.account_ref.clone(),
        NativeEventLimits::default(),
        Duration::from_millis(20),
    );
    let error = run(
        &harness,
        &request,
        &DenyAllToolExecutor,
        &DenyAllApprovalHandler,
    )
    .expect_err("timed out");
    assert_eq!(error.code, NativeErrorCode::TimedOut);
}

#[test]
fn account_key_is_redacted_from_output_and_rejected_in_tool_arguments() {
    let redaction_harness = harness(ScriptedTransport::new(vec![text_turn(&format!(
        "leak {TEST_KEY}"
    ))]));
    let redaction_request = request(redaction_harness.account_ref.clone());
    let result = run(
        &redaction_harness,
        &redaction_request,
        &DenyAllToolExecutor,
        &DenyAllApprovalHandler,
    )
    .expect("redacted");
    assert!(!result.output.contains(TEST_KEY));
    assert!(result.output.contains("[REDACTED]"));

    let harness = harness(ScriptedTransport::new(vec![tool_turn(
        "alfred_write_file",
        json!({"path": "leak.txt", "content": TEST_KEY}),
    )]));
    let request = request(harness.account_ref.clone());
    let error = run(
        &harness,
        &request,
        &DenyAllToolExecutor,
        &FixedApprover(AlfredApprovalDecision::Allow),
    )
    .expect_err("secret tool argument");
    assert_eq!(error.code, NativeErrorCode::PermissionDenied);
}

#[test]
fn split_sse_frames_are_reassembled_without_reasoning_events() {
    let full = text_turn("split").remove(0);
    let midpoint = full.len() / 2;
    let harness = harness(ScriptedTransport::new(vec![vec![
        full[..midpoint].to_vec(),
        full[midpoint..].to_vec(),
    ]]));
    let request = request(harness.account_ref.clone());
    let result = run(
        &harness,
        &request,
        &DenyAllToolExecutor,
        &DenyAllApprovalHandler,
    )
    .expect("split stream");
    assert_eq!(result.output, "split");
    assert!(result
        .events
        .iter()
        .all(|event| event.content_class != Some(NativeContentClass::Reasoning)));
}
