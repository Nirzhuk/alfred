use super::account::TestOpenCodeProfileCredential;
use super::fake_server::{
    FakeOpenCodeSidecar, FIXTURE_GO_KEY, FIXTURE_PASSWORD, FIXTURE_SESSION_ID,
};
use super::*;
use crate::agent_accounts::models::AgentProductId;
use crate::agent_accounts::runtime_profile::RuntimeProfileRef;
use crate::agents::native::{
    AlfredToolRequest, AlfredToolResult, NativeAgentRuntime, NativeApprovalPolicy,
    NativeCancellation, NativeContentClass, NativeContextBlock, NativeContextRole,
    NativeCredential, NativeErrorCode, NativeEvent, NativeEventKind, NativeEventLimits,
    NativePermissionProfile, NativeRuntimeError, NativeRuntimeRegistry, NativeSessionMode,
    NativeToolCapabilitySet, NativeToolExecutionOwner, NativeTurnHost, NativeTurnRequest,
    ResolvedNativeAccount, NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::{AgentHarness, AgentProvider, OpaqueAgentAccountRef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn catalog() -> Value {
    json!({
        "all": [
            {
                "id": "opencode-go",
                "name": "OpenCode Go",
                "models": {
                    "model-a": {
                        "id": "model-a",
                        "providerID": "opencode-go",
                        "name": "Go Model A"
                    },
                    "model-b": {
                        "id": "model-b",
                        "providerID": "opencode-go",
                        "name": "Go Model B"
                    }
                }
            },
            {
                "id": "zen",
                "name": "OpenCode Zen",
                "models": {
                    "zen-model": {
                        "id": "zen-model",
                        "providerID": "zen",
                        "name": "Must Never Escape"
                    }
                }
            },
            {
                "id": "anthropic",
                "name": "Upstream",
                "models": {
                    "claude": {
                        "id": "claude",
                        "providerID": "anthropic",
                        "name": "Must Never Escape"
                    }
                }
            }
        ],
        "connected": ["opencode-go", "zen", "anthropic"]
    })
}

fn event(event_type: &str, properties: Value) -> Value {
    json!({"type": event_type, "properties": properties})
}

fn streaming_events() -> Vec<Value> {
    vec![
        event("server.connected", json!({})),
        event(
            "message.part.updated",
            json!({
                "sessionID": FIXTURE_SESSION_ID,
                "part": {
                    "id": "part_text",
                    "sessionID": FIXTURE_SESSION_ID,
                    "messageID": "message_fixture",
                    "type": "text",
                    "text": "hello"
                }
            }),
        ),
        event(
            "message.part.updated",
            json!({
                "sessionID": FIXTURE_SESSION_ID,
                "part": {
                    "id": "part_text",
                    "sessionID": FIXTURE_SESSION_ID,
                    "messageID": "message_fixture",
                    "type": "text",
                    "text": "hello world"
                }
            }),
        ),
        event(
            "message.part.updated",
            json!({
                "sessionID": FIXTURE_SESSION_ID,
                "part": {
                    "id": "part_tool",
                    "sessionID": FIXTURE_SESSION_ID,
                    "messageID": "message_fixture",
                    "type": "tool",
                    "callID": "call_fixture",
                    "tool": "bash",
                    "state": {"status": "running"}
                }
            }),
        ),
        event(
            "message.part.updated",
            json!({
                "sessionID": FIXTURE_SESSION_ID,
                "part": {
                    "id": "part_tool",
                    "sessionID": FIXTURE_SESSION_ID,
                    "messageID": "message_fixture",
                    "type": "tool",
                    "callID": "call_fixture",
                    "tool": "bash",
                    "state": {"status": "completed", "output": "done"}
                }
            }),
        ),
        event("session.idle", json!({"sessionID": FIXTURE_SESSION_ID})),
    ]
}

struct FixtureSession {
    api: HttpOpenCodeApi,
    state: Arc<Mutex<OpenCodeServerState>>,
    stopped: Arc<AtomicBool>,
}

impl OpenCodeServerSession for FixtureSession {
    fn api(&self) -> &dyn OpenCodeApi {
        &self.api
    }

    fn state(&self) -> OpenCodeServerState {
        *self.state.lock().expect("state")
    }

    fn stop(&self) -> Result<(), NativeRuntimeError> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

struct FixtureServers {
    address: std::net::SocketAddr,
    profile_ref: RuntimeProfileRef,
    state: Arc<Mutex<OpenCodeServerState>>,
    stopped: Arc<AtomicBool>,
    purges: AtomicUsize,
}

impl FixtureServers {
    fn new(sidecar: &FakeOpenCodeSidecar, state: OpenCodeServerState) -> Self {
        Self {
            address: sidecar.address(),
            profile_ref: fixture_profile_ref(),
            state: Arc::new(Mutex::new(state)),
            stopped: Arc::new(AtomicBool::new(false)),
            purges: AtomicUsize::new(0),
        }
    }

    fn session(&self) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        Ok(Box::new(FixtureSession {
            api: HttpOpenCodeApi::new(
                self.address,
                OpenCodeServerPassword::new(FIXTURE_PASSWORD.into())?,
            )?,
            state: Arc::clone(&self.state),
            stopped: Arc::clone(&self.stopped),
        }))
    }
}

impl OpenCodeServerProvider for FixtureServers {
    fn create_and_launch(
        &self,
        _account_ref: &OpaqueAgentAccountRef,
        _repository: &Path,
        _cancellation: &NativeCancellation,
    ) -> Result<(RuntimeProfileRef, Box<dyn OpenCodeServerSession>), NativeRuntimeError> {
        Ok((self.profile_ref.clone(), self.session()?))
    }

    fn launch_existing(
        &self,
        _account_ref: &OpaqueAgentAccountRef,
        _profile_ref: &RuntimeProfileRef,
        _repository: &Path,
        _cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        self.session()
    }

    fn purge_profile(
        &self,
        _account_ref: &OpaqueAgentAccountRef,
        _profile_ref: &RuntimeProfileRef,
    ) -> Result<(), NativeRuntimeError> {
        self.purges.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct FixedPermission(OpenCodePermissionReply);

impl OpenCodePermissionBroker for FixedPermission {
    fn decide(
        &self,
        _request: &OpenCodePermissionRequest,
        _cancellation: &NativeCancellation,
    ) -> Result<OpenCodePermissionReply, NativeRuntimeError> {
        Ok(self.0)
    }
}

struct RecordingHost {
    cancellation: NativeCancellation,
    events: Vec<NativeEvent>,
}

impl NativeTurnHost for RecordingHost {
    fn emit(&mut self, event: NativeEvent) -> Result<(), NativeRuntimeError> {
        self.events.push(event);
        Ok(())
    }

    fn invoke_tool(
        &mut self,
        _request: AlfredToolRequest,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        Err(NativeRuntimeError::new(
            NativeErrorCode::CapabilityUnsupported,
            "OpenCode owns built-in tool execution",
            false,
        ))
    }

    fn cancellation(&self) -> &NativeCancellation {
        &self.cancellation
    }
}

fn fixture_profile_ref() -> RuntimeProfileRef {
    RuntimeProfileRef::parse("runtime_profile_0123456789abcdef0123456789abcdef")
        .expect("profile ref")
}

fn account() -> ResolvedNativeAccount {
    ResolvedNativeAccount {
        account_ref: OpaqueAgentAccountRef::parse("account_opencode-go-01").expect("account ref"),
        provider: AgentProvider::Opencode,
        product: AgentProductId::OpencodeGo,
        credential: NativeCredential::new(TestOpenCodeProfileCredential(fixture_profile_ref())),
    }
}

fn request(mode: NativeSessionMode, session_id: Option<&str>) -> NativeTurnRequest {
    let workspace = std::env::current_dir().expect("workspace");
    let cancellation =
        NativeCancellation::new("opencode_fixture", Duration::from_secs(5)).expect("cancellation");
    NativeTurnRequest {
        contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
        harness: AgentHarness::Alfred,
        harness_version: env!("CARGO_PKG_VERSION").into(),
        runtime_version: OPENCODE_RUNTIME_VERSION.into(),
        provider: AgentProvider::Opencode,
        account_ref: OpaqueAgentAccountRef::parse("account_opencode-go-01").expect("account ref"),
        run_id: "run_opencode_fixture".into(),
        node_id: "node_opencode_fixture".into(),
        model: "opencode-go/model-a".into(),
        prompt: "inspect the selected repository".into(),
        context: vec![
            NativeContextBlock {
                role: NativeContextRole::System,
                content: "Use built-in runtime tools only when approved.".into(),
                name: None,
            },
            NativeContextBlock {
                role: NativeContextRole::User,
                content: "inspect the selected repository".into(),
                name: None,
            },
        ],
        working_directory: workspace.clone(),
        allowed_workspace_roots: vec![workspace],
        permission_profile: NativePermissionProfile {
            filesystem: NativeApprovalPolicy::Ask,
            shell: NativeApprovalPolicy::Ask,
            mcp: NativeApprovalPolicy::Deny,
            subagents: NativeApprovalPolicy::Ask,
            memory_lookup: NativeApprovalPolicy::Deny,
        },
        tool_capabilities: NativeToolCapabilitySet {
            filesystem: true,
            shell: true,
            patch: true,
            mcp: false,
            subagents: true,
            memory_lookup: false,
        },
        session_mode: mode,
        session_id: session_id.map(str::to_owned),
        event_limits: NativeEventLimits::default(),
        timeout_ms: 5_000,
        cancellation: Some(cancellation),
    }
}

fn runtime(
    servers: Arc<FixtureServers>,
    reply: OpenCodePermissionReply,
    repository: PathBuf,
) -> OpenCodeNativeRuntime {
    OpenCodeNativeRuntime::new(servers, Arc::new(FixedPermission(reply)), repository)
        .expect("runtime")
}

#[test]
fn release_manifest_descriptor_and_registration_are_exact_and_fail_closed() {
    let gate = native_release_gate();
    assert_eq!(gate.runtime_version, "1.18.23");
    assert_eq!(gate.license, "MIT");
    assert_eq!(gate.platforms.len(), 6);
    assert_eq!(gate.blockers.len(), 2);
    assert!(!gate.ready);
    let blocker_codes = gate
        .blockers
        .iter()
        .map(|(code, _)| *code)
        .collect::<Vec<_>>();
    assert_eq!(
        blocker_codes,
        vec![COMMERCIAL_GATE_CODE, LIVE_SMOKE_GATE_CODE]
    );
    assert!(!blocker_codes.contains(&SUPERVISOR_HTTP_GATE_CODE));
    assert!(!blocker_codes.contains(&ACCOUNT_GATE_CODE));
    assert!(!blocker_codes.contains(&APPROVAL_GATE_CODE));

    let manifest = package_manifest();
    manifest.validate().expect("package manifest");
    assert_eq!(manifest.targets.len(), 6);
    assert!(manifest.targets.iter().all(|target| {
        target.publisher_verification.required
            && !target.rollback.automatic_fallback
            && target.rollback.retain_previous_verified
            && target.resources.len() == 2
    }));
    assert_eq!(OPENCODE_RELEASE_ARTIFACTS.len(), 6);
    assert!(OPENCODE_RELEASE_ARTIFACTS.iter().all(|artifact| {
        artifact.archive_sha256.len() == 64
            && artifact.executable_sha256.len() == 64
            && artifact.executable_bytes > 100_000_000
    }));
    assert!(!OPENCODE_LICENSE_BYTES.is_empty());
    assert!(!OPENCODE_NOTICE_BYTES.is_empty());

    let registry = NativeRuntimeRegistry::default();
    let error = register(&registry).expect_err("release must remain blocked");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    for code in [
        COMMERCIAL_GATE_CODE,
        PACKAGE_GATE_CODE,
        LIVE_SMOKE_GATE_CODE,
    ] {
        assert!(error.message.contains(code));
    }
    for code in [
        SUPERVISOR_HTTP_GATE_CODE,
        ACCOUNT_GATE_CODE,
        APPROVAL_GATE_CODE,
    ] {
        assert!(
            !error.message.contains(code),
            "{code} should not block registration once implemented"
        );
    }
}

#[test]
fn launch_is_random_loopback_isolated_and_has_no_external_cli_fallback() {
    let repository = std::env::current_dir().expect("workspace");
    let first =
        OpenCodeLaunchSpec::allocate(&repository, Duration::from_secs(60)).expect("first launch");
    let second =
        OpenCodeLaunchSpec::allocate(&repository, Duration::from_secs(60)).expect("second launch");
    assert!(first.address().ip().is_loopback());
    assert_ne!(first.address().port(), 0);
    assert!(second.address().ip().is_loopback());
    assert_ne!(second.address().port(), 0);
    assert_eq!(first.repository(), repository);
    assert_eq!(first.args()[0], "serve");
    assert_eq!(first.args()[1], "--hostname=127.0.0.1");
    assert!(first
        .args()
        .iter()
        .any(|argument| argument == "--mdns=false"));
    assert!(!first.environment().contains_key("PATH"));
    assert!(!first.environment().contains_key("HOME"));
    assert_eq!(first.environment()["OPENCODE_DISABLE_AUTOUPDATE"], "true");
    assert_eq!(
        first.environment()["OPENCODE_DISABLE_PROJECT_CONFIG"],
        "true"
    );
    let config: Value =
        serde_json::from_str(&first.environment()["OPENCODE_CONFIG_CONTENT"]).expect("config");
    assert_eq!(config["autoupdate"], json!(false));
    assert_eq!(config["share"], json!("disabled"));
    assert_eq!(config["server"]["cors"], json!([]));
    assert_eq!(config["server"]["mdns"], json!(false));
    assert!(!format!("{first:?}").contains(repository.to_string_lossy().as_ref()));
}

#[test]
fn auth_endpoint_is_exact_and_secrets_never_enter_records_or_debug_output() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        Vec::new(),
    );
    let api = HttpOpenCodeApi::new(
        sidecar.address(),
        OpenCodeServerPassword::new(FIXTURE_PASSWORD.into()).expect("password"),
    )
    .expect("api");
    let key = OpenCodeGoKey::parse(FIXTURE_GO_KEY.into()).expect("key");
    assert!(!format!("{key:?}").contains(FIXTURE_GO_KEY));
    assert!(!format!("{api:?}").contains(FIXTURE_PASSWORD));
    api.set_go_key(&key).expect("set key");
    drop(key);
    api.delete_go_key().expect("delete key");
    assert_eq!(sidecar.key_match_count(), 1);
    let records = sidecar.requests();
    assert_eq!(records[0].path, "/auth/opencode-go");
    assert_eq!(records[0].body["key"], json!("[REDACTED]"));
    assert_eq!(records[1].method, "DELETE");
    assert!(!format!("{records:?}").contains(FIXTURE_GO_KEY));
}

#[test]
fn wrong_listener_password_is_rejected_without_echoing_the_capability() {
    let non_loopback = "192.0.2.10:31337".parse().expect("address");
    let error = HttpOpenCodeApi::new(
        non_loopback,
        OpenCodeServerPassword::new(FIXTURE_PASSWORD.into()).expect("password"),
    )
    .expect_err("non-loopback listener");
    assert_eq!(error.code, NativeErrorCode::InvalidRequest);

    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        Vec::new(),
    );
    let wrong = "wrong-supervisor-password-000000001";
    let api = HttpOpenCodeApi::new(
        sidecar.address(),
        OpenCodeServerPassword::new(wrong.into()).expect("password"),
    )
    .expect("api");
    let error = api
        .list_providers(repository.to_string_lossy().as_ref())
        .expect_err("wrong password");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    assert!(!error.to_string().contains(wrong));
}

#[test]
fn model_discovery_and_routes_are_strictly_opencode_go() {
    let models = parse_go_models(&catalog()).expect("models");
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["opencode-go/model-a", "opencode-go/model-b"]
    );
    assert!(models.iter().all(|model| !model.id.contains("zen")));
    assert_eq!(
        OpenCodeRoute::parse("zen/zen-model")
            .expect_err("Zen refused")
            .code,
        NativeErrorCode::ModelUnavailable
    );
    assert_eq!(
        OpenCodeRoute::parse("anthropic/claude")
            .expect_err("upstream refused")
            .code,
        NativeErrorCode::ModelUnavailable
    );
}

#[test]
fn streaming_subscribes_before_prompt_and_maps_text_tools_and_usage_link() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        streaming_events(),
    );
    let servers = Arc::new(FixtureServers::new(&sidecar, OpenCodeServerState::Active));
    let runtime = runtime(
        Arc::clone(&servers),
        OpenCodePermissionReply::Once,
        repository.clone(),
    );
    let descriptor = runtime.descriptor();
    assert_eq!(descriptor.product, AgentProductId::OpencodeGo);
    assert_eq!(descriptor.runtime_id, "opencode_server");
    assert_eq!(descriptor.runtime_version, "1.18.23");
    assert_eq!(
        descriptor.tool_execution_owner,
        NativeToolExecutionOwner::RuntimeExecutedWithHostApproval
    );
    assert!(!descriptor.capabilities.supports_usage);
    assert!(!descriptor.capabilities.supports_mcp);
    let models = runtime
        .discover_models(&account())
        .expect("model discovery");
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["opencode-go/model-a", "opencode-go/model-b"]
    );

    let request = request(NativeSessionMode::Start, None);
    let mut host = RecordingHost {
        cancellation: request.cancellation().expect("cancellation").clone(),
        events: Vec::new(),
    };
    let outcome = runtime
        .run_turn(&account(), &request, &mut host)
        .expect("turn");
    assert_eq!(outcome.session_id.as_deref(), Some(FIXTURE_SESSION_ID));
    let text = host
        .events
        .iter()
        .filter(|event| event.kind == NativeEventKind::AssistantDelta)
        .filter_map(|event| event.text.as_deref())
        .collect::<String>();
    assert_eq!(text, "hello world");
    assert!(host.events.iter().any(|event| {
        event.kind == NativeEventKind::AssistantDelta
            && event.content_class == Some(NativeContentClass::Assistant)
    }));
    assert!(host
        .events
        .iter()
        .any(|event| event.kind == NativeEventKind::ToolStarted));
    let completed = host
        .events
        .iter()
        .find(|event| event.kind == NativeEventKind::TurnCompleted)
        .expect("completed");
    assert_eq!(
        completed.metadata["accountUsageState"],
        json!("unavailable")
    );
    assert_eq!(
        completed.metadata["usageDeepLinkOnly"],
        json!(OPENCODE_GO_USAGE_URL)
    );

    let records = sidecar.requests();
    let event_index = records
        .iter()
        .position(|request| request.path == "/event")
        .expect("event subscription");
    let prompt_index = records
        .iter()
        .position(|request| request.path.ends_with("/prompt_async"))
        .expect("prompt");
    assert!(event_index < prompt_index);
    let prompt = &records[prompt_index].body;
    assert_eq!(prompt["model"]["providerID"], json!("opencode-go"));
    assert_eq!(prompt["model"]["modelID"], json!("model-a"));
    assert!(prompt.get("tools").is_none());
    assert!(records
        .iter()
        .all(|request| !request.path.starts_with("/experimental/")
            && !request.path.starts_with("/api/")));
    assert!(servers.stopped.load(Ordering::SeqCst));
}

#[test]
fn permission_replies_are_only_once_always_or_reject() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        Vec::new(),
    );
    let api = HttpOpenCodeApi::new(
        sidecar.address(),
        OpenCodeServerPassword::new(FIXTURE_PASSWORD.into()).expect("password"),
    )
    .expect("api");
    let cancellation =
        NativeCancellation::new("permissions", Duration::from_secs(2)).expect("cancellation");
    for (id, reply) in [
        ("permission_once", OpenCodePermissionReply::Once),
        ("permission_always", OpenCodePermissionReply::Always),
        ("permission_reject", OpenCodePermissionReply::Reject),
    ] {
        api.reply_permission(
            repository.to_string_lossy().as_ref(),
            id,
            reply,
            &cancellation,
        )
        .expect("permission reply");
    }
    let replies = sidecar
        .requests()
        .into_iter()
        .filter(|request| request.path.starts_with("/permission/"))
        .map(|request| request.body["reply"].as_str().expect("reply").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(replies, ["once", "always", "reject"]);
}

#[test]
fn runtime_permission_is_decided_by_the_host_bridge_and_emitted() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        vec![
            event(
                "permission.asked",
                json!({
                    "id": "permission_fixture",
                    "sessionID": FIXTURE_SESSION_ID,
                    "permission": "bash",
                    "patterns": ["git status"],
                    "metadata": {},
                    "always": ["git status"],
                    "tool": {"messageID": "message_fixture", "callID": "call_fixture"}
                }),
            ),
            event("session.idle", json!({"sessionID": FIXTURE_SESSION_ID})),
        ],
    );
    let servers = Arc::new(FixtureServers::new(&sidecar, OpenCodeServerState::Active));
    let runtime = runtime(servers, OpenCodePermissionReply::Always, repository);
    let request = request(NativeSessionMode::Start, None);
    let mut host = RecordingHost {
        cancellation: request.cancellation().expect("cancellation").clone(),
        events: Vec::new(),
    };
    runtime
        .run_turn(&account(), &request, &mut host)
        .expect("permission turn");
    assert!(host.events.iter().any(|event| {
        event.kind == NativeEventKind::ApprovalRequested
            && event.approval_id.as_deref() == Some("permission_fixture")
    }));
    assert!(host.events.iter().any(|event| {
        event.kind == NativeEventKind::ApprovalResolved && event.approved == Some(true)
    }));
    let reply = sidecar
        .requests()
        .into_iter()
        .find(|request| request.path == "/permission/permission_fixture/reply")
        .expect("reply");
    assert_eq!(reply.body["reply"], json!("always"));
}

#[test]
fn cancellation_aborts_the_exact_session() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = Arc::new(FakeOpenCodeSidecar::start_delayed(
        repository.to_string_lossy().into_owned(),
        catalog(),
        streaming_events(),
        Duration::from_millis(300),
    ));
    let servers = Arc::new(FixtureServers::new(&sidecar, OpenCodeServerState::Active));
    let runtime = runtime(
        Arc::clone(&servers),
        OpenCodePermissionReply::Once,
        repository,
    );
    let request = request(NativeSessionMode::Start, None);
    let cancellation = request.cancellation().expect("cancellation").clone();
    let cancel_from_thread = cancellation.clone();
    let cancel_sidecar = Arc::clone(&sidecar);
    let cancel_thread = std::thread::spawn(move || {
        cancel_sidecar.wait_for_prompt();
        cancel_from_thread.cancel();
    });
    let mut host = RecordingHost {
        cancellation,
        events: Vec::new(),
    };
    let error = runtime
        .run_turn(&account(), &request, &mut host)
        .expect_err("cancelled turn");
    cancel_thread.join().expect("cancel thread");
    assert_eq!(error.code, NativeErrorCode::Cancelled);
    assert!(sidecar.requests().iter().any(|request| {
        request.method == "POST" && request.path == format!("/session/{FIXTURE_SESSION_ID}/abort")
    }));
}

#[test]
fn resume_is_exact_and_a_mismatched_session_is_rejected() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        vec![event(
            "session.idle",
            json!({"sessionID": FIXTURE_SESSION_ID}),
        )],
    );
    let servers = Arc::new(FixtureServers::new(&sidecar, OpenCodeServerState::Active));
    let runtime = runtime(
        Arc::clone(&servers),
        OpenCodePermissionReply::Once,
        repository,
    );
    let exact = request(NativeSessionMode::Resume, Some(FIXTURE_SESSION_ID));
    let mut host = RecordingHost {
        cancellation: exact.cancellation().expect("cancellation").clone(),
        events: Vec::new(),
    };
    let outcome = runtime
        .run_turn(&account(), &exact, &mut host)
        .expect("exact resume");
    assert_eq!(outcome.session_id.as_deref(), Some(FIXTURE_SESSION_ID));

    let wrong = request(NativeSessionMode::Resume, Some("session_other"));
    let mut wrong_host = RecordingHost {
        cancellation: wrong.cancellation().expect("cancellation").clone(),
        events: Vec::new(),
    };
    let error = runtime
        .run_turn(&account(), &wrong, &mut wrong_host)
        .expect_err("wrong session");
    assert_eq!(error.code, NativeErrorCode::SessionUnavailable);
}

#[test]
fn http_and_event_rate_limits_are_defensively_classified() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        Vec::new(),
    );
    sidecar.force_status("/provider", 429);
    let api = HttpOpenCodeApi::new(
        sidecar.address(),
        OpenCodeServerPassword::new(FIXTURE_PASSWORD.into()).expect("password"),
    )
    .expect("api");
    let http_error = api
        .list_providers(repository.to_string_lossy().as_ref())
        .expect_err("rate limit");
    assert_eq!(http_error.code, NativeErrorCode::ProviderUnavailable);
    assert!(http_error.retryable);
    assert!(http_error.message.contains("usage limit"));

    let mut mapper = OpenCodeEventMapper::new(FIXTURE_SESSION_ID.into()).expect("mapper");
    let mapped = mapper
        .map(event(
            "session.error",
            json!({
                "sessionID": FIXTURE_SESSION_ID,
                "error": {
                    "name": "APIError",
                    "data": {
                        "statusCode": 429,
                        "message": "provider-specific opaque failure",
                        "isRetryable": true
                    }
                }
            }),
        ))
        .expect("mapped");
    let OpenCodeProtocolEvent::SessionError(failure) = mapped else {
        panic!("expected session failure");
    };
    assert_eq!(failure, OpenCodeGoFailure::RateLimited);
}

#[test]
fn disconnect_purges_profile_even_after_endpoint_use() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        Vec::new(),
    );
    let servers = Arc::new(FixtureServers::new(&sidecar, OpenCodeServerState::Active));
    let manager = OpenCodeAccountManager::new(servers.clone());
    let account_ref = OpaqueAgentAccountRef::parse("account_opencode-go-01").expect("account ref");
    let cancellation =
        NativeCancellation::new("account", Duration::from_secs(3)).expect("cancellation");
    let profile_ref = manager
        .connect(
            &account_ref,
            &repository,
            OpenCodeGoKey::parse(FIXTURE_GO_KEY.into()).expect("key"),
            &cancellation,
        )
        .expect("connect");
    manager
        .disconnect(&account_ref, &profile_ref, &repository, &cancellation)
        .expect("disconnect");
    assert_eq!(servers.purges.load(Ordering::SeqCst), 1);
    let records = sidecar.requests();
    assert!(records
        .iter()
        .any(|request| { request.method == "PUT" && request.path == "/auth/opencode-go" }));
    assert!(records
        .iter()
        .any(|request| { request.method == "DELETE" && request.path == "/auth/opencode-go" }));
    assert!(!format!("{records:?}").contains(FIXTURE_GO_KEY));
}

#[test]
fn managed_runtime_crash_aborts_and_never_falls_back_to_a_cli() {
    let repository = std::env::current_dir().expect("workspace");
    let sidecar = FakeOpenCodeSidecar::start(
        repository.to_string_lossy().into_owned(),
        catalog(),
        vec![event("server.connected", json!({}))],
    );
    let servers = Arc::new(FixtureServers::new(&sidecar, OpenCodeServerState::Failed));
    let runtime = runtime(
        Arc::clone(&servers),
        OpenCodePermissionReply::Once,
        repository,
    );
    let request = request(NativeSessionMode::Start, None);
    let mut host = RecordingHost {
        cancellation: request.cancellation().expect("cancellation").clone(),
        events: Vec::new(),
    };
    let error = runtime
        .run_turn(&account(), &request, &mut host)
        .expect_err("crash");
    assert_eq!(error.code, NativeErrorCode::ProviderUnavailable);
    assert!(error.message.contains("managed runtime crashed"));
    assert!(sidecar
        .requests()
        .iter()
        .any(|request| { request.path == format!("/session/{FIXTURE_SESSION_ID}/abort") }));
}
