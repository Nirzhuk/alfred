//! Conformance fixtures for the Alfred-native Claude (Anthropic API) runtime.
//!
//! Every fixture drives the real `NativeRuntimeRegistry` pipeline over a
//! scripted transport, so the assertions cover the harness event, approval,
//! redaction, and cancellation contracts rather than the provider module alone.

use super::wire::{
    classify_status, classify_stream_error, parse_model_catalog, tool_definitions, ClaudeFailure,
    SseDecoder,
};
use super::{
    register, transport::ClaudeByteStream, transport::ClaudeTransport,
    transport::HttpClaudeTransport, ClaudeNativeRuntime, ACCOUNT_INTAKE_BLOCKED_CODE,
    LIVE_SMOKE_BLOCKED_CODE, RUNTIME_VERSION,
};
use crate::agents::native::*;
use crate::agents::{AgentHarness, AgentProvider, OpaqueAgentAccountRef};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TEST_KEY: &str = "sk-ant-api03-fixture-key";
const TEST_MODEL: &str = "claude-opus-5";

/// Test-only credential. Production resolves a Plan 031
/// `NativeAgentCredential`, whose envelope has no public constructor.
pub(super) struct TestApiKey(pub String);

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
            || provider != AgentProvider::ClaudeCode
            || product != crate::agent_accounts::models::AgentProductId::ClaudeApi
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "fixture account does not match",
                false,
            ));
        }
        Ok(ResolvedNativeAccount {
            account_ref: account_ref.clone(),
            provider,
            product: crate::agent_accounts::models::AgentProductId::ClaudeApi,
            credential: NativeCredential::new(TestApiKey(self.key.clone())),
        })
    }
}

/// A scripted Anthropic transport: canned SSE chunks, an optional HTTP
/// failure, and optional per-chunk behaviour for cancellation/timeout fixtures.
#[derive(Default)]
struct ScriptedTransport {
    turns: Mutex<Vec<Vec<String>>>,
    fail_with: Option<(u16, String)>,
    catalog: Option<String>,
    chunk_delay: Duration,
    cancel_after: Option<(usize, NativeCancellation)>,
    bodies: Mutex<Vec<Value>>,
}

impl ScriptedTransport {
    fn new(turns: Vec<Vec<String>>) -> Self {
        Self {
            turns: Mutex::new(turns),
            ..Self::default()
        }
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().expect("bodies").clone()
    }
}

impl ClaudeTransport for ScriptedTransport {
    fn stream_messages(
        &self,
        api_key: &str,
        body: &Value,
        _cancellation: &NativeCancellation,
    ) -> Result<Box<dyn ClaudeByteStream>, NativeRuntimeError> {
        assert_eq!(
            api_key, TEST_KEY,
            "the runtime must send the stored API key"
        );
        self.bodies.lock().expect("bodies").push(body.clone());
        if let Some((status, payload)) = self.fail_with.as_ref() {
            return Err(classify_status(*status, payload).error());
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
            delay: self.chunk_delay,
            cancel_after: self.cancel_after.clone(),
        }))
    }

    fn list_models(&self, _api_key: &str) -> Result<String, NativeRuntimeError> {
        Ok(self.catalog.clone().unwrap_or_else(|| {
            json!({"data": [{"id": TEST_MODEL, "display_name": "Claude Opus 5"}]}).to_string()
        }))
    }
}

struct ScriptedStream {
    chunks: Vec<String>,
    index: usize,
    delay: Duration,
    cancel_after: Option<(usize, NativeCancellation)>,
}

impl ClaudeByteStream for ScriptedStream {
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
        Ok(Some(chunk.into_bytes()))
    }
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

fn sse(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {}\n\n",
                event["type"].as_str().unwrap(),
                event
            )
        })
        .collect()
}

fn text_turn(text: &str) -> Vec<Value> {
    vec![
        json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant"}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}),
        json!({"type": "message_stop"}),
    ]
}

fn tool_turn(id: &str, name: &str, input: Value) -> Vec<Value> {
    vec![
        json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant"}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": input.to_string()}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
        json!({"type": "message_stop"}),
    ]
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
        .register(Arc::new(ClaudeNativeRuntime::new(Box::new(
            SharedTransport(Arc::clone(&transport)),
        ))))
        .expect("register claude runtime");
    let account_ref = OpaqueAgentAccountRef::parse("account_claude-01").expect("account ref");
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

/// Lets a fixture keep a handle on the transport while the runtime owns one.
struct SharedTransport(Arc<ScriptedTransport>);

impl ClaudeTransport for SharedTransport {
    fn stream_messages(
        &self,
        api_key: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn ClaudeByteStream>, NativeRuntimeError> {
        self.0.stream_messages(api_key, body, cancellation)
    }

    fn list_models(&self, api_key: &str) -> Result<String, NativeRuntimeError> {
        self.0.list_models(api_key)
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
    let workspace = std::env::current_dir().expect("current directory");
    NativeTurnRequest {
        contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
        harness: AgentHarness::Alfred,
        harness_version: env!("CARGO_PKG_VERSION").into(),
        runtime_version: RUNTIME_VERSION.into(),
        provider: AgentProvider::ClaudeCode,
        account_ref,
        run_id: "run_claude".into(),
        node_id: "node_claude".into(),
        model: TEST_MODEL.into(),
        prompt: "summarize the workspace".into(),
        context: vec![NativeContextBlock {
            role: NativeContextRole::User,
            content: "summarize the workspace".into(),
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
        cancellation: Some(
            NativeCancellation::new("cancel_claude", timeout).expect("cancellation"),
        ),
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

fn deny_all() -> (DenyAllToolExecutor, DenyAllApprovalHandler) {
    (DenyAllToolExecutor, DenyAllApprovalHandler)
}

// ---------------------------------------------------------------- descriptor

#[test]
fn descriptor_declares_api_key_only_and_no_subscription_capability() {
    let runtime = ClaudeNativeRuntime::new(Box::new(ScriptedTransport::default()));
    let descriptor = runtime.descriptor();
    assert_eq!(descriptor.provider, AgentProvider::ClaudeCode);
    assert!(descriptor.capabilities.supports_api_key);
    // Claude.ai subscription login stays BLOCKED without Anthropic approval.
    assert!(!descriptor.capabilities.supports_oauth);
    // No documented account-usage endpoint for an API key.
    assert!(!descriptor.capabilities.supports_usage);
    assert!(!descriptor.capabilities.supports_sessions);
    assert!(!descriptor.capabilities.supports_mcp);
    assert!(!descriptor.capabilities.supports_subagents);
    assert!(!descriptor.runtime_id.contains("claude-code"));
}

#[test]
fn usage_is_reported_unavailable_rather_than_inferred() {
    let harness = harness(ScriptedTransport::default());
    let snapshot = harness
        .registry
        .usage_snapshot(
            AgentProvider::ClaudeCode,
            &harness.account_ref,
            &harness.resolver,
        )
        .expect("usage snapshot");
    assert_eq!(snapshot.state, NativeUsageState::Unavailable);
    assert!(snapshot.input_tokens.is_none());
}

#[test]
fn model_catalog_comes_from_the_documented_models_endpoint() {
    let harness = harness(ScriptedTransport::default());
    let models = harness
        .registry
        .discover_models(
            AgentProvider::ClaudeCode,
            &harness.account_ref,
            &harness.resolver,
        )
        .expect("models");
    assert!(models.iter().any(|model| model.id == TEST_MODEL));
}

#[test]
fn an_unknown_selected_model_fails_before_any_request() {
    let harness = harness(ScriptedTransport::new(vec![sse(&text_turn("hi"))]));
    let mut request = request(harness.account_ref.clone());
    request.model = "claude-not-a-model".into();
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("model rejected");
    assert_eq!(error.code, NativeErrorCode::ModelUnavailable);
    assert!(harness.transport.bodies().is_empty());
}

#[test]
fn a_non_api_key_credential_is_refused() {
    let runtime = ClaudeNativeRuntime::new(Box::new(ScriptedTransport::default()));
    let account = ResolvedNativeAccount {
        account_ref: OpaqueAgentAccountRef::parse("account_claude-01").expect("ref"),
        provider: AgentProvider::ClaudeCode,
        product: crate::agent_accounts::models::AgentProductId::ClaudeApi,
        credential: NativeCredential::new(TestApiKey("oauth-access-token".into())),
    };
    let error = runtime.validate_account(&account).expect_err("refused");
    assert_eq!(error.code, NativeErrorCode::AccountUnavailable);
}

#[test]
fn an_admin_api_key_is_not_accepted_as_a_messages_api_key() {
    let runtime = ClaudeNativeRuntime::new(Box::new(ScriptedTransport::default()));
    let account = ResolvedNativeAccount {
        account_ref: OpaqueAgentAccountRef::parse("account_claude-01").expect("ref"),
        provider: AgentProvider::ClaudeCode,
        product: crate::agent_accounts::models::AgentProductId::ClaudeApi,
        credential: NativeCredential::new(TestApiKey("sk-ant-admin01-fixture-key".into())),
    };
    let error = runtime
        .validate_account(&account)
        .expect_err("admin key refused");
    assert_eq!(error.code, NativeErrorCode::AccountUnavailable);
}

// ------------------------------------------------------------------ streaming

#[test]
fn text_stream_produces_bounded_assistant_events() {
    let harness = harness(ScriptedTransport::new(vec![sse(&text_turn("hello world"))]));
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let result = run(&harness, &request, &executor, &approver).expect("turn");
    assert_eq!(result.output, "hello world");
    let kinds: Vec<_> = result.events.iter().map(|event| event.kind).collect();
    assert_eq!(
        kinds,
        vec![
            NativeEventKind::TurnStarted,
            NativeEventKind::AssistantDelta,
            NativeEventKind::TurnCompleted
        ]
    );
    assert!(result
        .events
        .iter()
        .all(|event| event.content_class != Some(NativeContentClass::Reasoning)));
    // The request carries the documented streaming Messages API shape.
    let body = &harness.transport.bodies()[0];
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["model"], json!(TEST_MODEL));
}

#[test]
fn thinking_blocks_are_replayed_but_never_emitted() {
    let harness = harness(ScriptedTransport::new(vec![
        sse(&tool_turn(
            "toolu_1",
            "alfred_read_file",
            json!({"path": "."}),
        )),
        sse(&text_turn("done")),
    ]));
    let request = request(harness.account_ref.clone());
    let executor = RecordingExecutor {
        output: "file body".into(),
        seen: Mutex::new(Vec::new()),
    };
    let approver = FixedApprover(AlfredApprovalDecision::Allow);
    let result = run(&harness, &request, &executor, &approver).expect("turn");
    assert_eq!(result.output, "done");
    // No event carries reasoning text.
    assert!(result
        .events
        .iter()
        .all(|event| event.text.as_deref() != Some("sig")));
    // The follow-up request replays the assistant turn including its thinking
    // block, which the API requires to remain unmodified.
    let bodies = harness.transport.bodies();
    let assistant = &bodies[1]["messages"][1];
    assert_eq!(assistant["role"], json!("assistant"));
    let kinds: Vec<&str> = assistant["content"]
        .as_array()
        .expect("content")
        .iter()
        .map(|block| block["type"].as_str().unwrap_or_default())
        .collect();
    assert!(kinds.contains(&"thinking"));
    assert!(kinds.contains(&"tool_use"));
}

#[test]
fn assistant_text_is_redacted_before_it_leaves_the_runtime() {
    let harness = harness(ScriptedTransport::new(vec![sse(&text_turn(
        "your key is sk-ant-api03-LEAKED",
    ))]));
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let result = run(&harness, &request, &executor, &approver).expect("turn");
    assert!(!result.output.contains("sk-ant-api03-LEAKED"));
    assert!(result.output.contains("[REDACTED]"));
}

#[test]
fn oversized_assistant_output_fails_instead_of_streaming_unbounded() {
    let limits = NativeEventLimits {
        max_text_bytes: 32,
        ..NativeEventLimits::default()
    };
    let long = "x".repeat(512);
    let harness = harness(ScriptedTransport::new(vec![sse(&text_turn(&long))]));
    let request = request_with(harness.account_ref.clone(), limits, DEFAULT_TURN_TIMEOUT);
    let (executor, approver) = deny_all();
    let error =
        run(&harness, &request, &executor, &approver).expect_err("oversized output is rejected");
    assert_eq!(error.code, NativeErrorCode::EventLimitExceeded);
}

#[test]
fn a_truncated_stream_is_reported_as_provider_unavailable() {
    // message_stop never arrives.
    let harness = harness(ScriptedTransport::new(vec![sse(&text_turn("partial")
        [..3]
        .to_vec())]));
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("truncated stream");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    assert!(error.retryable);
}

// ---------------------------------------------------------------- tool calls

#[test]
fn a_tool_request_reaches_the_alfred_executor_and_its_result_is_replayed() {
    let harness = harness(ScriptedTransport::new(vec![
        sse(&tool_turn(
            "toolu_1",
            "alfred_run_command",
            json!({"path": ".", "command": ["ls", "-a"]}),
        )),
        sse(&text_turn("listed")),
    ]));
    let request = request(harness.account_ref.clone());
    let executor = RecordingExecutor {
        output: "a\nb".into(),
        seen: Mutex::new(Vec::new()),
    };
    let approver = FixedApprover(AlfredApprovalDecision::Allow);
    let result = run(&harness, &request, &executor, &approver).expect("turn");
    assert_eq!(result.output, "listed");

    let seen = executor.seen.lock().expect("seen");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].kind, AlfredToolKind::Shell);
    assert_eq!(seen[0].arguments, vec!["ls".to_string(), "-a".to_string()]);
    assert_eq!(seen[0].path, Some(PathBuf::from(".")));

    let kinds: Vec<_> = result.events.iter().map(|event| event.kind).collect();
    assert!(kinds.contains(&NativeEventKind::ApprovalRequested));
    assert!(kinds.contains(&NativeEventKind::ApprovalResolved));
    assert!(kinds.contains(&NativeEventKind::ToolStarted));
    assert!(kinds.contains(&NativeEventKind::ToolCompleted));

    let bodies = harness.transport.bodies();
    let tool_result = &bodies[1]["messages"][2]["content"][0];
    assert_eq!(tool_result["type"], json!("tool_result"));
    assert_eq!(tool_result["tool_use_id"], json!("toolu_1"));
    assert_eq!(tool_result["is_error"], json!(false));
}

#[test]
fn a_denied_approval_returns_a_denial_to_the_model_without_running_the_tool() {
    let harness = harness(ScriptedTransport::new(vec![
        sse(&tool_turn(
            "toolu_1",
            "alfred_run_command",
            json!({"path": ".", "command": ["rm", "-rf", "/"]}),
        )),
        sse(&text_turn("stopped")),
    ]));
    let request = request(harness.account_ref.clone());
    let executor = RecordingExecutor {
        output: "should not run".into(),
        seen: Mutex::new(Vec::new()),
    };
    let approver = FixedApprover(AlfredApprovalDecision::Deny);
    let result = run(&harness, &request, &executor, &approver).expect("turn");
    assert!(executor.seen.lock().expect("seen").is_empty());
    assert!(result.events.iter().any(|event| {
        event.kind == NativeEventKind::ApprovalResolved && event.approved == Some(false)
    }));
    let bodies = harness.transport.bodies();
    let tool_result = &bodies[1]["messages"][2]["content"][0];
    assert_eq!(tool_result["is_error"], json!(true));
}

#[test]
fn a_tool_outside_the_declared_capability_set_is_refused() {
    let harness = harness(ScriptedTransport::new(vec![sse(&tool_turn(
        "toolu_1",
        "alfred_run_command",
        json!({"path": ".", "command": ["ls"]}),
    ))]));
    let mut request = request(harness.account_ref.clone());
    request.tool_capabilities.shell = false;
    let executor = RecordingExecutor {
        output: String::new(),
        seen: Mutex::new(Vec::new()),
    };
    let approver = FixedApprover(AlfredApprovalDecision::Allow);
    let error = run(&harness, &request, &executor, &approver).expect_err("capability refused");
    assert_eq!(error.code, NativeErrorCode::CapabilityUnsupported);
    assert!(executor.seen.lock().expect("seen").is_empty());
}

#[test]
fn a_tool_path_outside_the_workspace_is_refused() {
    let harness = harness(ScriptedTransport::new(vec![sse(&tool_turn(
        "toolu_1",
        "alfred_read_file",
        json!({"path": "/etc/passwd"}),
    ))]));
    let request = request(harness.account_ref.clone());
    let executor = RecordingExecutor {
        output: String::new(),
        seen: Mutex::new(Vec::new()),
    };
    let approver = FixedApprover(AlfredApprovalDecision::Allow);
    let error = run(&harness, &request, &executor, &approver).expect_err("workspace denied");
    assert_eq!(error.code, NativeErrorCode::WorkspaceDenied);
    assert!(executor.seen.lock().expect("seen").is_empty());
}

#[test]
fn only_alfred_owned_tools_are_advertised() {
    let names: Vec<String> = tool_definitions(&NativeToolCapabilitySet {
        filesystem: true,
        shell: true,
        patch: true,
        mcp: true,
        subagents: true,
        memory_lookup: false,
    })
    .iter()
    .map(|tool| tool["name"].as_str().unwrap_or_default().to_string())
    .collect();
    assert!(names.iter().all(|name| name.starts_with("alfred_")));
    assert!(!names.iter().any(|name| name.contains("mcp")));
    assert!(!names.iter().any(|name| name.contains("subagent")));
    assert!(tool_definitions(&NativeToolCapabilitySet::default()).is_empty());
}

#[test]
fn a_looping_model_is_stopped_by_the_tool_iteration_bound() {
    let turns = (0..super::wire::MAX_TOOL_ITERATIONS + 2)
        .map(|iteration| {
            let request_id = format!("toolu_{iteration}");
            sse(&tool_turn(
                &request_id,
                "alfred_read_file",
                json!({"path": "."}),
            ))
        })
        .collect();
    let harness = harness(ScriptedTransport::new(turns));
    let request = request(harness.account_ref.clone());
    let executor = RecordingExecutor {
        output: "loop".into(),
        seen: Mutex::new(Vec::new()),
    };
    let approver = FixedApprover(AlfredApprovalDecision::Allow);
    let error = run(&harness, &request, &executor, &approver).expect_err("iteration bound");
    assert_eq!(error.code, NativeErrorCode::InvalidRequest);
    assert_eq!(
        harness.transport.bodies().len(),
        super::wire::MAX_TOOL_ITERATIONS
    );
}

// ------------------------------------------------- cancellation and deadlines

#[test]
fn cancellation_stops_the_turn_mid_stream() {
    let events = text_turn("hello");
    let mut transport = ScriptedTransport::new(vec![sse(&events)]);
    let cancellation =
        NativeCancellation::new("cancel_claude", DEFAULT_TURN_TIMEOUT).expect("cancellation");
    transport.cancel_after = Some((1, cancellation.clone()));
    let harness = harness(transport);
    let mut request = request(harness.account_ref.clone());
    request.cancellation = Some(cancellation);
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("cancelled");
    assert_eq!(error.code, NativeErrorCode::Cancelled);
}

#[test]
fn a_slow_stream_is_stopped_by_the_turn_deadline() {
    let events = text_turn("hello");
    let mut transport = ScriptedTransport::new(vec![sse(&events)]);
    transport.chunk_delay = Duration::from_millis(40);
    let harness = harness(transport);
    let request = request_with(
        harness.account_ref.clone(),
        NativeEventLimits::default(),
        Duration::from_millis(60),
    );
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("timed out");
    assert_eq!(error.code, NativeErrorCode::TimedOut);
    assert!(error.retryable);
}

// -------------------------------------------------------- error classification

#[test]
fn http_failures_map_to_stable_alfred_errors() {
    let cases = [
        (
            401,
            r#"{"error":{"type":"authentication_error"}}"#,
            ClaudeFailure::InvalidAuth,
            NativeErrorCode::AccountUnavailable,
            false,
        ),
        (
            402,
            r#"{"error":{"type":"billing_error"}}"#,
            ClaudeFailure::Billing,
            NativeErrorCode::AccountUnavailable,
            false,
        ),
        (
            403,
            r#"{"error":{"type":"permission_error"}}"#,
            ClaudeFailure::PermissionDenied,
            NativeErrorCode::PermissionDenied,
            false,
        ),
        (
            404,
            r#"{"error":{"type":"not_found_error"}}"#,
            ClaudeFailure::ModelUnavailable,
            NativeErrorCode::ModelUnavailable,
            false,
        ),
        (
            413,
            r#"{"error":{"type":"request_too_large"}}"#,
            ClaudeFailure::RequestTooLarge,
            NativeErrorCode::InvalidRequest,
            false,
        ),
        (
            429,
            r#"{"error":{"type":"rate_limit_error"}}"#,
            ClaudeFailure::RateLimited,
            NativeErrorCode::ProviderUnavailable,
            true,
        ),
        (
            500,
            r#"{"error":{"type":"api_error"}}"#,
            ClaudeFailure::ProviderUnavailable,
            NativeErrorCode::ProviderUnavailable,
            true,
        ),
        (
            529,
            r#"{"error":{"type":"overloaded_error"}}"#,
            ClaudeFailure::Overloaded,
            NativeErrorCode::ProviderUnavailable,
            true,
        ),
        (
            400,
            r#"{"error":{"type":"invalid_request_error","message":"prompt is too long: 1200000 tokens > 1000000 maximum"}}"#,
            ClaudeFailure::ContextLimit,
            NativeErrorCode::InvalidRequest,
            false,
        ),
        (
            400,
            r#"{"error":{"type":"invalid_request_error","message":"bad field"}}"#,
            ClaudeFailure::InvalidRequest,
            NativeErrorCode::InvalidRequest,
            false,
        ),
    ];
    for (status, body, expected, code, retryable) in cases {
        let failure = classify_status(status, body);
        assert_eq!(failure, expected, "status {status}");
        assert_eq!(failure.code(), code, "status {status}");
        assert_eq!(failure.retryable(), retryable, "status {status}");
    }
}

#[test]
fn an_invalid_api_key_surfaces_as_an_account_error_through_the_registry() {
    let mut transport = ScriptedTransport::default();
    transport.fail_with = Some((401, r#"{"error":{"type":"authentication_error"}}"#.into()));
    let harness = harness(transport);
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("invalid auth");
    assert_eq!(error.code, NativeErrorCode::AccountUnavailable);
    assert!(!error.retryable);
}

#[test]
fn an_overloaded_provider_surfaces_as_a_retryable_error() {
    let mut transport = ScriptedTransport::default();
    transport.fail_with = Some((529, r#"{"error":{"type":"overloaded_error"}}"#.into()));
    let harness = harness(transport);
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("overloaded");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    assert!(error.retryable);
}

#[test]
fn a_rate_limited_provider_surfaces_as_a_retryable_error() {
    let mut transport = ScriptedTransport::default();
    transport.fail_with = Some((429, r#"{"error":{"type":"rate_limit_error"}}"#.into()));
    let harness = harness(transport);
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("rate limited");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    assert!(error.retryable);
}

#[test]
fn a_context_limit_failure_surfaces_as_a_terminal_request_error() {
    let mut transport = ScriptedTransport::default();
    transport.fail_with = Some((
        400,
        r#"{"error":{"type":"invalid_request_error","message":"prompt is too long: context limit exceeded"}}"#.into(),
    ));
    let harness = harness(transport);
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("context limit");
    assert_eq!(error.code, NativeErrorCode::InvalidRequest);
    assert!(!error.retryable);
    assert_eq!(error.message, ClaudeFailure::ContextLimit.message());
}

#[test]
fn a_mid_stream_error_event_is_classified_not_streamed() {
    let mut events = text_turn("partial");
    events.truncate(3);
    events.push(
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "overloaded"}}),
    );
    let harness = harness(ScriptedTransport::new(vec![sse(&events)]));
    let request = request(harness.account_ref.clone());
    let (executor, approver) = deny_all();
    let error = run(&harness, &request, &executor, &approver).expect_err("stream error");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    assert_eq!(
        classify_stream_error(
            &json!({"error": {"type": "invalid_request_error", "message": "prompt is too long"}})
        ),
        ClaudeFailure::ContextLimit
    );
}

#[test]
fn provider_error_text_never_reaches_the_user() {
    let failure = classify_status(
        401,
        r#"{"error":{"type":"authentication_error","message":"invalid key sk-ant-api03-LEAKED"}}"#,
    );
    assert!(!failure.message().contains("LEAKED"));
}

// ------------------------------------------------------------------ decoding

#[test]
fn the_sse_decoder_reassembles_events_split_across_chunks() {
    let mut decoder = SseDecoder::default();
    assert!(decoder
        .push(b"event: message_stop\ndata: {\"ty")
        .expect("first")
        .is_empty());
    let events = decoder.push(b"pe\":\"message_stop\"}\n\n").expect("second");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], json!("message_stop"));
    // Keep-alive comments and the terminator carry no payload.
    assert!(decoder
        .push(b": ping\ndata: [DONE]\n\n")
        .expect("ping")
        .is_empty());
}

#[test]
fn a_malformed_stream_payload_is_rejected() {
    let mut decoder = SseDecoder::default();
    let error = decoder
        .push(b"data: {not json}\n\n")
        .expect_err("malformed");
    assert_eq!(error.code, NativeErrorCode::InvalidEvent);
}

#[test]
fn an_empty_model_catalog_is_an_error_not_a_silent_default() {
    let error = parse_model_catalog(r#"{"data":[]}"#).expect_err("empty catalog");
    assert_eq!(error.code, NativeErrorCode::ModelUnavailable);
}

#[test]
fn production_registration_is_blocked_with_exact_gate_codes() {
    let error = register(&NativeRuntimeRegistry::default()).expect_err("registration blocked");
    assert!(error.message.contains(ACCOUNT_INTAKE_BLOCKED_CODE));
    assert!(error.message.contains(LIVE_SMOKE_BLOCKED_CODE));
}

#[test]
fn model_catalog_enforces_entry_and_field_bounds() {
    let entries = (0..513)
        .map(|index| json!({"id": format!("claude-{index}"), "display_name": "Claude"}))
        .collect::<Vec<_>>();
    assert_eq!(
        parse_model_catalog(&json!({"data": entries}).to_string())
            .expect_err("entry bound")
            .code,
        NativeErrorCode::ModelUnavailable
    );
    for entry in [
        json!({"id": "x".repeat(257), "display_name": "Claude"}),
        json!({"id": "claude-safe", "display_name": "x".repeat(257)}),
    ] {
        assert_eq!(
            parse_model_catalog(&json!({"data": [entry]}).to_string())
                .expect_err("field bound")
                .code,
            NativeErrorCode::ModelUnavailable
        );
    }
}

#[test]
fn production_http_policy_refuses_redirects_before_replaying_x_api_key() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let redirect_sink = TcpListener::bind("127.0.0.1:0").expect("redirect sink");
    redirect_sink
        .set_nonblocking(true)
        .expect("nonblocking sink");
    let sink_url = format!("http://{}/stolen", redirect_sink.local_addr().unwrap());
    let source = TcpListener::bind("127.0.0.1:0").expect("source");
    let source_url = format!("http://{}/v1/messages", source.local_addr().unwrap());
    let redirected = Arc::new(AtomicBool::new(false));
    let redirected_thread = Arc::clone(&redirected);

    let sink_worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(750);
        while Instant::now() < deadline {
            match redirect_sink.accept() {
                Ok((_stream, _)) => {
                    redirected_thread.store(true, Ordering::SeqCst);
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("redirect sink failed: {error}"),
            }
        }
    });
    let source_worker = std::thread::spawn(move || {
        let (mut stream, _) = source.accept().expect("source request");
        let mut request = [0u8; 4096];
        let read = stream.read(&mut request).expect("read request");
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request
            .to_ascii_lowercase()
            .contains("x-api-key: sk-ant-api03-fixture-key"));
        write!(
            stream,
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {sink_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .expect("redirect response");
    });

    let transport = HttpClaudeTransport::fixture(&source_url, &source_url).expect("transport");
    let cancellation =
        NativeCancellation::new("claude-redirect", Duration::from_secs(5)).expect("cancellation");
    let result = transport.stream_messages(TEST_KEY, &json!({"messages": []}), &cancellation);
    if result.is_ok() {
        panic!("redirect response must not become a message stream");
    }

    source_worker.join().expect("source worker");
    sink_worker.join().expect("sink worker");
    assert!(!redirected.load(Ordering::SeqCst));
}

#[test]
fn production_model_transport_rejects_an_oversized_body() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let source = TcpListener::bind("127.0.0.1:0").expect("source");
    let source_url = format!("http://{}/v1/models", source.local_addr().unwrap());
    let source_worker = std::thread::spawn(move || {
        let (mut stream, _) = source.accept().expect("source request");
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request).expect("read request");
        let body = vec![b'x'; super::transport::MAX_MODEL_CATALOG_BYTES + 1];
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .expect("response headers");
        // The bounded reader is expected to close as soon as it crosses the
        // limit, so the fixture server may observe a broken pipe here.
        let _ = stream.write_all(&body);
    });

    let transport = HttpClaudeTransport::fixture(&source_url, &source_url).expect("transport");
    let error = transport
        .list_models(TEST_KEY)
        .expect_err("oversized model response");
    source_worker.join().expect("source worker");
    assert_eq!(error.code, NativeErrorCode::EventLimitExceeded);
}

#[test]
fn production_stream_read_observes_cooperative_cancellation_while_stalled() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let source = TcpListener::bind("127.0.0.1:0").expect("source");
    let source_url = format!("http://{}/v1/messages", source.local_addr().unwrap());
    let source_worker = std::thread::spawn(move || {
        let (mut stream, _) = source.accept().expect("source request");
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request).expect("read request");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .expect("response headers");
        stream.flush().expect("flush headers");
        std::thread::sleep(Duration::from_millis(250));
    });

    let transport = HttpClaudeTransport::fixture(&source_url, &source_url).expect("transport");
    let cancellation = NativeCancellation::new("claude-stalled-read", Duration::from_secs(5))
        .expect("cancellation");
    let mut stream = transport
        .stream_messages(TEST_KEY, &json!({"messages": []}), &cancellation)
        .expect("stream headers");
    let cancel_from_thread = cancellation.clone();
    let cancel_worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        cancel_from_thread.cancel();
    });

    let started = Instant::now();
    let error = stream
        .next_chunk(&cancellation)
        .expect_err("stalled read must observe cancellation");
    let cancellation_latency = started.elapsed();
    cancel_worker.join().expect("cancel worker");
    source_worker.join().expect("source worker");
    assert_eq!(error.code, NativeErrorCode::Cancelled);
    assert!(cancellation_latency < Duration::from_millis(200));
}
