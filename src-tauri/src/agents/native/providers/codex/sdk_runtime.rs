//! Non-shipping managed-runtime adapter for the stable public Codex SDK.
//!
//! The adapter is complete enough for sealed-package integration and fake
//! conformance tests. Production registration remains fail-closed until every
//! release gate below is cleared in shared release engineering.

use super::sdk_protocol::{
    empty_params, login_id_params, login_start_params, CodexSdkAccount, CodexSdkInbound,
    CodexSdkLoginKind, CodexSdkLoginKindDto, CodexSdkLoginPrompt, CodexSdkLoginWait,
    CodexSdkLogout, CodexSdkMethod, CodexSdkProtocol, CodexSdkProtocolError, CodexSdkResponse,
    CodexSdkStreamEvent,
};
use super::{
    validate_codex_sdk_selection, CODEX_SDK_HOST_APPROVAL_BLOCKER, CODEX_SDK_RUNTIME_VERSION,
};
use crate::agent_accounts::models::{AgentProductId, CredentialCustodyMode, ManagedRuntimeId};
use crate::agent_accounts::resolver::NativeAgentCredential;
use crate::agent_accounts::runtime_profile::{
    RuntimeEnvironmentVariable, RuntimeProfile, RuntimeProfileBinding, RuntimeProfileLifecycle,
    RuntimeProfileRef, RuntimeProfileStore,
};
use crate::agents::managed_runtime::{
    ManagedRuntimeCancellation, ManagedRuntimeHandle, ManagedRuntimeLaunchSpec,
    ManagedRuntimeLifecycle, ManagedRuntimeSupervisor, RuntimeReadinessProbe, RuntimeShutdownHook,
    RuntimeStdoutPolicy,
};
use crate::agents::native::{
    CapabilityReportStatus, NativeErrorCode, NativeRuntimeError, NativeRuntimeRegistry,
    ResolvedNativeAccount,
};
use crate::agents::runtime_package::RuntimePackageSelection;
use crate::agents::AgentProvider;
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::io::{self, ErrorKind};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

pub const PUBLIC_CAPABILITY_AUDIT_BLOCKER: &str =
    "codex_python_sdk_public_capability_audit_blocked";
pub const KNOWN_CLIENT_ENTERPRISE_BLOCKER: &str =
    "codex_python_sdk_known_client_enterprise_clearance_missing";
pub const PACKAGED_SMOKE_BLOCKER: &str = "codex_python_sdk_packaged_smoke_missing";

const READY_LINE: &str =
    r#"{"experimentalApi":false,"protocolVersion":1,"sdkVersion":"0.147.0","type":"ready"}"#;
const SHUTDOWN_LINE: &str =
    r#"{"method":"shutdown","params":{},"protocolVersion":1,"requestId":"supervisor_shutdown"}"#;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSdkSurfaceStatus {
    Supported,
    Blocked,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSdkSurfaceEntry {
    pub capability: &'static str,
    pub status: CodexSdkSurfaceStatus,
    pub evidence: &'static str,
}

pub fn codex_sdk_surface() -> Vec<CodexSdkSurfaceEntry> {
    vec![
        CodexSdkSurfaceEntry {
            capability: "browser_login",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public Codex.login_chatgpt handle with wait and cancel",
        },
        CodexSdkSurfaceEntry {
            capability: "device_code_login",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public Codex.login_chatgpt_device_code handle with wait and cancel",
        },
        CodexSdkSurfaceEntry {
            capability: "account_logout_models",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public Codex.account, Codex.logout, and Codex.models",
        },
        CodexSdkSurfaceEntry {
            capability: "threads_and_streamed_turns",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public thread_start, thread_resume, turn, and TurnHandle.stream",
        },
        CodexSdkSurfaceEntry {
            capability: "turn_cancellation",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public TurnHandle.interrupt",
        },
        CodexSdkSurfaceEntry {
            capability: "host_approvals",
            status: CodexSdkSurfaceStatus::Blocked,
            evidence: CODEX_SDK_HOST_APPROVAL_BLOCKER,
        },
        CodexSdkSurfaceEntry {
            capability: "usage",
            status: CodexSdkSurfaceStatus::Unavailable,
            evidence: "not projected by the audited public stable SDK slice",
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSdkReleaseGate {
    pub gate: &'static str,
    pub status: CapabilityReportStatus,
    pub evidence: &'static str,
}

pub fn codex_sdk_release_gates() -> Vec<CodexSdkReleaseGate> {
    vec![
        CodexSdkReleaseGate {
            gate: "public_capability_audit",
            status: CapabilityReportStatus::Blocked,
            evidence: PUBLIC_CAPABILITY_AUDIT_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "host_approval",
            status: CapabilityReportStatus::Blocked,
            evidence: CODEX_SDK_HOST_APPROVAL_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "known_client_enterprise",
            status: CapabilityReportStatus::Blocked,
            evidence: KNOWN_CLIENT_ENTERPRISE_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "sealed_package",
            status: CapabilityReportStatus::Blocked,
            evidence: super::SEALED_PACKAGE_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "packaged_no_dependency_smoke",
            status: CapabilityReportStatus::Blocked,
            evidence: PACKAGED_SMOKE_BLOCKER,
        },
    ]
}

pub fn codex_sdk_native_ready() -> bool {
    codex_sdk_release_gates()
        .iter()
        .all(|gate| gate.status != CapabilityReportStatus::Blocked)
}

/// No shipping runtime is registered while the stable public SDK lacks host
/// approval control and the enterprise/package/smoke evidence is incomplete.
pub fn register(_registry: &NativeRuntimeRegistry) -> Result<(), NativeRuntimeError> {
    Err(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        format!(
            "{PUBLIC_CAPABILITY_AUDIT_BLOCKER}; {CODEX_SDK_HOST_APPROVAL_BLOCKER}; \
             {KNOWN_CLIENT_ENTERPRISE_BLOCKER}; {}; {PACKAGED_SMOKE_BLOCKER}",
            super::SEALED_PACKAGE_BLOCKER,
        ),
        false,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSdkRuntimeErrorCode {
    AccountMismatch,
    CredentialRejected,
    ProfileMismatch,
    PackageRejected,
    InvalidWorkingDirectory,
    LaunchRejected,
    ProtocolRejected,
    RuntimeCrashed,
    TimedOut,
    LogoutRejected,
    ProfilePurgeRejected,
}

impl CodexSdkRuntimeErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccountMismatch => "codex_python_sdk_account_mismatch",
            Self::CredentialRejected => "codex_python_sdk_credential_rejected",
            Self::ProfileMismatch => "codex_python_sdk_profile_mismatch",
            Self::PackageRejected => "codex_python_sdk_package_rejected",
            Self::InvalidWorkingDirectory => "codex_python_sdk_cwd_invalid",
            Self::LaunchRejected => "codex_python_sdk_launch_rejected",
            Self::ProtocolRejected => "codex_python_sdk_protocol_rejected",
            Self::RuntimeCrashed => "codex_python_sdk_runtime_crashed",
            Self::TimedOut => "codex_python_sdk_timed_out",
            Self::LogoutRejected => "codex_python_sdk_logout_rejected",
            Self::ProfilePurgeRejected => "codex_python_sdk_profile_purge_rejected",
        }
    }
}

pub struct CodexSdkRuntimeError(CodexSdkRuntimeErrorCode);

impl CodexSdkRuntimeError {
    pub fn code(&self) -> CodexSdkRuntimeErrorCode {
        self.0
    }
}

impl fmt::Debug for CodexSdkRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl fmt::Display for CodexSdkRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl std::error::Error for CodexSdkRuntimeError {}

pub type RuntimeResult<T> = Result<T, CodexSdkRuntimeError>;

fn runtime_error(code: CodexSdkRuntimeErrorCode) -> CodexSdkRuntimeError {
    CodexSdkRuntimeError(code)
}

pub fn validate_codex_sdk_account(
    account: &ResolvedNativeAccount,
    profile: &RuntimeProfile,
) -> RuntimeResult<()> {
    if account.provider != AgentProvider::Codex || account.product != AgentProductId::ChatgptCodex {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::AccountMismatch));
    }
    let credential = account
        .credential
        .downcast_ref::<NativeAgentCredential>()
        .ok_or_else(|| runtime_error(CodexSdkRuntimeErrorCode::CredentialRejected))?;
    if credential.custody_mode() != CredentialCustodyMode::RuntimeManaged
        || credential.managed_runtime_id() != Some(ManagedRuntimeId::CodexPythonSdk)
        || credential.managed_runtime_version() != Some(CODEX_SDK_RUNTIME_VERSION)
        || credential.runtime_profile_ref() != Some(profile.profile_ref().as_str())
        || credential.access_token().is_some()
        || credential.refresh_token().is_some()
        || credential.runtime_credential_ref().is_some()
        || credential.provider_field("api_key").is_some()
        || credential.provider_field("access_token").is_some()
        || credential.provider_field("refresh_token").is_some()
    {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::CredentialRejected));
    }
    validate_profile_binding(profile, &account.account_ref)
}

fn validate_profile_binding(
    profile: &RuntimeProfile,
    account_ref: &crate::agents::OpaqueAgentAccountRef,
) -> RuntimeResult<()> {
    let expected = RuntimeProfileBinding::new(
        account_ref,
        AgentProductId::ChatgptCodex,
        ManagedRuntimeId::CodexPythonSdk,
        CODEX_SDK_RUNTIME_VERSION,
    )
    .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProfileMismatch))?;
    let binding = profile.binding();
    let has_codex_home = profile
        .environment_roots()
        .get(RuntimeEnvironmentVariable::CodexHome)
        .is_some();
    if profile.lifecycle() != RuntimeProfileLifecycle::Active
        || binding != &expected
        || !has_codex_home
    {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::ProfileMismatch));
    }
    Ok(())
}

/// Candidate launch boundary used by provider-local integration tests. It is
/// crate-private so callers cannot bypass the blocked production `register()`.
pub(crate) fn launch_codex_sdk_candidate(
    supervisor: &ManagedRuntimeSupervisor,
    account: &ResolvedNativeAccount,
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
    working_directory: &Path,
    cancellation: ManagedRuntimeCancellation,
) -> RuntimeResult<CodexSdkConnection> {
    validate_codex_sdk_account(account, profile)?;
    launch_codex_sdk_process(
        supervisor,
        package,
        profile,
        working_directory,
        cancellation,
    )
}

/// First-login launch. ChatGPT OAuth has to start before any persisted
/// account exists, so this path authenticates the sealed package and an
/// isolated profile without requiring a resolved credential.
pub(crate) fn launch_codex_sdk_login(
    supervisor: &ManagedRuntimeSupervisor,
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
    working_directory: &Path,
    cancellation: ManagedRuntimeCancellation,
) -> RuntimeResult<CodexSdkConnection> {
    if profile.binding().product() != AgentProductId::ChatgptCodex
        || profile.lifecycle() != RuntimeProfileLifecycle::Active
        || package.expectation().product() != AgentProductId::ChatgptCodex
    {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::ProfileMismatch));
    }
    launch_codex_sdk_process(
        supervisor,
        package,
        profile,
        working_directory,
        cancellation,
    )
}

fn launch_codex_sdk_process(
    supervisor: &ManagedRuntimeSupervisor,
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
    working_directory: &Path,
    cancellation: ManagedRuntimeCancellation,
) -> RuntimeResult<CodexSdkConnection> {
    validate_codex_sdk_selection(package)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::PackageRejected))?;
    if package.expectation().runtime_id() != profile.binding().runtime_id()
        || package.expectation().runtime_version() != profile.binding().runtime_version()
    {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::PackageRejected));
    }
    let working_directory = canonical_working_directory(working_directory)?;
    let readiness = RuntimeReadinessProbe::stdout_line_equals(READY_LINE)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::LaunchRejected))?;
    let shutdown = RuntimeShutdownHook::stdin_line(SHUTDOWN_LINE)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::LaunchRejected))?;
    let spec = ManagedRuntimeLaunchSpec::new(
        Vec::new(),
        readiness,
        shutdown,
        RuntimeStdoutPolicy::TypedFramesFailClosed,
    )
    .with_working_directory(working_directory)
    .with_startup_timeout(STARTUP_TIMEOUT)
    .with_shutdown_timeout(SHUTDOWN_TIMEOUT);
    let handle = supervisor
        .launch(package, profile, spec, cancellation)
        .map_err(|error| {
            // The public code stays opaque; the supervisor's reason is the only
            // way to tell a readiness timeout from a rejected environment.
            #[cfg(debug_assertions)]
            eprintln!("codex sidecar launch rejected: {:?}", error.code());
            #[cfg(not(debug_assertions))]
            let _ = &error;
            runtime_error(CodexSdkRuntimeErrorCode::LaunchRejected)
        })?;
    Ok(CodexSdkConnection {
        handle,
        protocol: Mutex::new(CodexSdkProtocol::default()),
        loopback_v6: Mutex::new(None),
    })
}

fn canonical_working_directory(path: &Path) -> RuntimeResult<PathBuf> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::InvalidWorkingDirectory))?;
    if !path.is_absolute() || !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(runtime_error(
            CodexSdkRuntimeErrorCode::InvalidWorkingDirectory,
        ));
    }
    path.canonicalize()
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::InvalidWorkingDirectory))
}

pub(crate) struct CodexSdkConnection {
    handle: ManagedRuntimeHandle,
    protocol: Mutex<CodexSdkProtocol>,
    loopback_v6: Mutex<Option<LoopbackV6Bridge>>,
}

impl fmt::Debug for CodexSdkConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexSdkConnection")
            .field("lifecycle", &self.handle.snapshot().lifecycle)
            .finish()
    }
}

impl CodexSdkConnection {
    pub(crate) fn send(
        &self,
        request_id: &str,
        method: CodexSdkMethod,
        params: Value,
    ) -> RuntimeResult<()> {
        let frame = self
            .protocol
            .lock()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
            .encode_request(request_id, method, params)
            .map_err(map_protocol_error)?;
        self.handle
            .write_stdin_frame(&frame)
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))
    }

    pub(crate) fn track_login_operation(&self, login_id: &str) -> RuntimeResult<()> {
        self.protocol
            .lock()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
            .track_login_operation(login_id)
            .map_err(map_protocol_error)
    }

    pub(crate) fn track_turn_operation(
        &self,
        operation_id: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> RuntimeResult<()> {
        self.protocol
            .lock()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
            .track_turn_operation(operation_id, thread_id, turn_id)
            .map_err(map_protocol_error)
    }

    pub(crate) fn receive(&self, timeout: Duration) -> RuntimeResult<Option<CodexSdkInbound>> {
        let frame = self
            .handle
            .read_stdout_frame(timeout)
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))?;
        match frame {
            Some(frame) => self
                .protocol
                .lock()
                .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
                .ingest(&frame)
                .map(Some)
                .map_err(map_protocol_error),
            None if matches!(
                self.handle.snapshot().lifecycle,
                ManagedRuntimeLifecycle::Exited | ManagedRuntimeLifecycle::Failed
            ) =>
            {
                if let Ok(mut protocol) = self.protocol.lock() {
                    protocol.process_exited();
                }
                Err(runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))
            }
            None => Ok(None),
        }
    }

    /// Starts public ChatGPT browser login and returns only the allow-listed
    /// authorization URL plus opaque login id. Tokens never cross this boundary.
    pub(crate) fn start_chatgpt_login(
        &self,
        timeout: Duration,
    ) -> RuntimeResult<CodexSdkLoginPrompt> {
        self.send(
            "login_start_1",
            CodexSdkMethod::LoginStart,
            login_start_params(CodexSdkLoginKind::Browser),
        )?;
        let response =
            self.wait_for_response("login_start_1", CodexSdkMethod::LoginStart, timeout)?;
        let prompt: CodexSdkLoginPrompt =
            super::sdk_protocol::parse_result(&response).map_err(map_protocol_error)?;
        prompt
            .validate()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?;
        if prompt.kind != CodexSdkLoginKindDto::Browser {
            return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
        }
        self.track_login_operation(&prompt.login_id)?;
        self.send(
            "login_wait_1",
            CodexSdkMethod::LoginWait,
            login_id_params(&prompt.login_id).map_err(map_protocol_error)?,
        )?;
        let wait_response = self.wait_for_response(
            "login_wait_1",
            CodexSdkMethod::LoginWait,
            Duration::from_secs(5),
        )?;
        let wait: CodexSdkLoginWait =
            super::sdk_protocol::parse_result(&wait_response).map_err(map_protocol_error)?;
        wait.validate()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?;
        if wait.login_id != prompt.login_id {
            return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
        }
        self.bridge_chatgpt_localhost_ipv6(&prompt.authorization_url);
        Ok(prompt)
    }

    /// Drains login completion and, once ChatGPT auth is acknowledged, reads
    /// the public account document. Returns `Ok(None)` while the browser
    /// ceremony is still outstanding.
    pub(crate) fn poll_chatgpt_account(
        &self,
        login_id: &str,
        timeout: Duration,
    ) -> RuntimeResult<Option<CodexSdkAccount>> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| runtime_error(CodexSdkRuntimeErrorCode::TimedOut))?;
        let mut completed = false;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.receive(remaining.min(Duration::from_millis(50)))? {
                Some(CodexSdkInbound::Response(response))
                    if response.method == CodexSdkMethod::LoginWait.as_str() =>
                {
                    let wait: CodexSdkLoginWait =
                        super::sdk_protocol::parse_result(&response).map_err(map_protocol_error)?;
                    wait.validate()
                        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?;
                    if wait.login_id != login_id {
                        return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
                    }
                }
                Some(CodexSdkInbound::Event {
                    event:
                        CodexSdkStreamEvent::LoginCompleted {
                            login_id: completed_id,
                            success,
                        },
                    ..
                }) => {
                    if success && completed_id == login_id {
                        completed = true;
                    } else if completed_id == login_id {
                        return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
                    }
                }
                Some(CodexSdkInbound::Error(_)) => {
                    return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
                }
                Some(CodexSdkInbound::Response(response))
                    if response.method == CodexSdkMethod::Account.as_str() =>
                {
                    let account: CodexSdkAccount =
                        super::sdk_protocol::parse_result(&response).map_err(map_protocol_error)?;
                    account
                        .validate()
                        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?;
                    if account.authenticated {
                        return Ok(Some(account));
                    }
                    return Ok(None);
                }
                Some(_) | None => {}
            }
            if completed {
                break;
            }
        }
        if !completed {
            return Ok(None);
        }
        self.send("account_1", CodexSdkMethod::Account, empty_params())?;
        let response =
            self.wait_for_response("account_1", CodexSdkMethod::Account, Duration::from_secs(5))?;
        let account: CodexSdkAccount =
            super::sdk_protocol::parse_result(&response).map_err(map_protocol_error)?;
        account
            .validate()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?;
        if account.authenticated {
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }

    fn wait_for_response(
        &self,
        request_id: &str,
        method: CodexSdkMethod,
        timeout: Duration,
    ) -> RuntimeResult<super::CodexSdkResponse> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| runtime_error(CodexSdkRuntimeErrorCode::TimedOut))?;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(runtime_error(CodexSdkRuntimeErrorCode::TimedOut));
            }
            match self.receive(deadline.saturating_duration_since(now))? {
                Some(CodexSdkInbound::Response(response))
                    if response.request_id == request_id && response.method == method.as_str() =>
                {
                    return Ok(response);
                }
                Some(CodexSdkInbound::Error(error))
                    if error.request_id.as_deref() == Some(request_id) =>
                {
                    return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
                }
                Some(CodexSdkInbound::Event { .. }) | Some(CodexSdkInbound::Ready(_)) => continue,
                Some(_) => return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected)),
                None => continue,
            }
        }
    }

    /// Logs the SDK out, waits for the token-free acknowledgement, stops the
    /// supervised process, and only then returns a receipt that authorizes a
    /// matching profile purge.
    pub(crate) fn logout_and_stop(
        self,
        request_id: &str,
        profile: &RuntimeProfile,
        timeout: Duration,
    ) -> RuntimeResult<CodexSdkLogoutReceipt> {
        self.send(
            request_id,
            CodexSdkMethod::Logout,
            super::sdk_protocol::empty_params(),
        )?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| runtime_error(CodexSdkRuntimeErrorCode::TimedOut))?;
        let logout = loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(runtime_error(CodexSdkRuntimeErrorCode::TimedOut));
            }
            let remaining = deadline.saturating_duration_since(now);
            match self.receive(remaining)? {
                Some(CodexSdkInbound::Response(response))
                    if response.request_id == request_id
                        && response.method == CodexSdkMethod::Logout.as_str() =>
                {
                    break super::sdk_protocol::parse_result::<CodexSdkLogout>(&response)
                        .map_err(map_protocol_error)?;
                }
                Some(CodexSdkInbound::Error(error))
                    if error.request_id.as_deref() == Some(request_id) =>
                {
                    return Err(runtime_error(CodexSdkRuntimeErrorCode::LogoutRejected));
                }
                Some(_) => {
                    return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
                }
                None => continue,
            }
        };
        logout
            .validate()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::LogoutRejected))?;
        self.handle
            .stop()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))?;
        Ok(CodexSdkLogoutReceipt {
            profile_ref: profile.profile_ref().clone(),
            binding: profile.binding().clone(),
        })
    }
}

fn map_protocol_error(_error: CodexSdkProtocolError) -> CodexSdkRuntimeError {
    runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected)
}

struct LoopbackV6Bridge {
    stop: Arc<AtomicBool>,
}

impl Drop for LoopbackV6Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl CodexSdkConnection {
    fn bridge_chatgpt_localhost_ipv6(&self, authorization_url: &str) {
        let Some(port) = chatgpt_localhost_redirect_port(authorization_url) else {
            return;
        };
        let Ok(mut slot) = self.loopback_v6.lock() else {
            return;
        };
        *slot = spawn_ipv6_loopback_bridge(port);
    }
}

pub(crate) fn open_chatgpt_sign_in_url(authorization_url: &str) {
    if !is_chatgpt_sign_in_url(authorization_url) {
        return;
    }
    let _ = tauri_plugin_opener::open_url(authorization_url, None::<&str>);
}

pub(crate) fn is_chatgpt_sign_in_url(authorization_url: &str) -> bool {
    let Ok(parsed) = Url::parse(authorization_url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.port_or_known_default() == Some(443)
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && matches!(parsed.host_str(), Some("chatgpt.com" | "auth.openai.com"))
}

fn chatgpt_localhost_redirect_port(authorization_url: &str) -> Option<u16> {
    let parsed = Url::parse(authorization_url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?;
    if host != "chatgpt.com" && host != "auth.openai.com" {
        return None;
    }
    let redirect = parsed
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value.into_owned())?;
    let redirect_url = Url::parse(&redirect).ok()?;
    if redirect_url.scheme() != "http" || redirect_url.host_str() != Some("localhost") {
        return None;
    }
    redirect_url.port()
}

fn spawn_ipv6_loopback_bridge(port: u16) -> Option<LoopbackV6Bridge> {
    if port == 0 {
        return None;
    }
    let listener = TcpListener::bind(("::1", port)).ok()?;
    listener.set_nonblocking(true).ok()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    thread::Builder::new()
        .name("codex-login-v6-bridge".into())
        .spawn(move || loop {
            if stop_thread.load(Ordering::Relaxed) {
                break;
            }
            match listener.accept() {
                Ok((incoming, _)) => match TcpStream::connect(("127.0.0.1", port)) {
                    Ok(outgoing) => proxy_tcp(incoming, outgoing),
                    Err(_) => {}
                },
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(_) => break,
            }
        })
        .ok()?;
    Some(LoopbackV6Bridge { stop })
}

fn proxy_tcp(left: TcpStream, right: TcpStream) {
    let Ok(mut left_reader) = left.try_clone() else {
        return;
    };
    let Ok(mut right_reader) = right.try_clone() else {
        return;
    };
    let mut left_writer = left;
    let mut right_writer = right;
    thread::spawn(move || {
        let _ = io::copy(&mut left_reader, &mut right_writer);
        let _ = right_writer.shutdown(Shutdown::Write);
    });
    thread::spawn(move || {
        let _ = io::copy(&mut right_reader, &mut left_writer);
        let _ = left_writer.shutdown(Shutdown::Write);
    });
}

pub struct CodexSdkLogoutReceipt {
    profile_ref: RuntimeProfileRef,
    binding: RuntimeProfileBinding,
}

impl fmt::Debug for CodexSdkLogoutReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CodexSdkLogoutReceipt([REDACTED PROFILE])")
    }
}

/// Profile deletion is impossible without a receipt created after the public
/// SDK's logout acknowledgement and supervisor shutdown.
pub fn purge_logged_out_codex_profile(
    store: &RuntimeProfileStore,
    receipt: CodexSdkLogoutReceipt,
) -> RuntimeResult<()> {
    store
        .purge(&receipt.profile_ref, &receipt.binding)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProfilePurgeRejected))
}

#[cfg(test)]
mod tests {
    use super::{
        chatgpt_localhost_redirect_port, is_chatgpt_sign_in_url, spawn_ipv6_loopback_bridge,
    };
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn chatgpt_localhost_redirect_port_reads_the_callback_port() {
        let url = "https://auth.openai.com/oauth/authorize?client_id=codex&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&response_type=code";
        assert_eq!(chatgpt_localhost_redirect_port(url), Some(1455));
        assert_eq!(
            chatgpt_localhost_redirect_port("https://chatgpt.com/auth/codex"),
            None
        );
        assert_eq!(
            chatgpt_localhost_redirect_port(
                "https://auth.openai.com/oauth/authorize?redirect_uri=http://127.0.0.1:1455/auth/callback"
            ),
            None
        );
    }

    #[test]
    fn chatgpt_sign_in_url_accepts_only_provider_https_hosts() {
        assert!(is_chatgpt_sign_in_url(
            "https://auth.openai.com/oauth/authorize?client_id=example"
        ));
        assert!(is_chatgpt_sign_in_url("https://chatgpt.com/auth/codex"));
        assert!(!is_chatgpt_sign_in_url(
            "http://auth.openai.com/oauth/authorize"
        ));
        assert!(!is_chatgpt_sign_in_url("https://evil.example/oauth"));
        assert!(!is_chatgpt_sign_in_url(
            "https://auth.openai.com.evil/oauth"
        ));
    }

    #[test]
    fn ipv6_localhost_bridge_forwards_to_ipv4_loopback() {
        let server = TcpListener::bind(("127.0.0.1", 0)).expect("ipv4 listener");
        let port = server.local_addr().expect("server addr").port();
        let accepted = thread::spawn(move || {
            let (mut stream, _) = server.accept().expect("accept ipv4");
            let mut buffer = [0_u8; 4];
            stream.read_exact(&mut buffer).expect("read probe");
            stream.write_all(b"pong").expect("write probe");
            buffer
        });
        let bridge = spawn_ipv6_loopback_bridge(port).expect("listen on ::1");
        thread::sleep(Duration::from_millis(50));
        let mut client = TcpStream::connect(("::1", port)).expect("connect via localhost ipv6");
        client.write_all(b"ping").expect("write via bridge");
        let mut reply = [0_u8; 4];
        client.read_exact(&mut reply).expect("read via bridge");
        assert_eq!(&reply, b"pong");
        assert_eq!(&accepted.join().expect("server thread"), b"ping");
        drop(bridge);
    }
}
