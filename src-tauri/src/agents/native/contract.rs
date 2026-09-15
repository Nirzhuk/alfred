use super::{MemoryLookupContextAllowance, NativeEvent, NativeEventLimits};
use crate::agent_accounts::models::AgentProductId;
use crate::agents::{AgentHarness, AgentProvider, OpaqueAgentAccountRef};
use serde::Serialize;
use std::any::Any;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(300);
pub const MAX_TURN_TIMEOUT: Duration = Duration::from_secs(3_600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCapability {
    OAuth,
    ApiKey,
    Sessions,
    Resume,
    ModelList,
    Usage,
    ToolCalls,
    ApprovalEvents,
    NativeFilesystem,
    NativeShell,
    Patch,
    Mcp,
    Subagents,
    MemoryLookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeToolExecutionOwner {
    AlfredExecuted,
    RuntimeExecutedWithHostApproval,
    NoTools,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCapabilities {
    pub contract_version: u16,
    pub supports_oauth: bool,
    pub supports_api_key: bool,
    pub supports_sessions: bool,
    pub supports_resume: bool,
    pub supports_model_list: bool,
    pub supports_usage: bool,
    pub supports_tool_calls: bool,
    pub supports_approval_events: bool,
    pub supports_native_filesystem: bool,
    pub supports_native_shell: bool,
    pub supports_patch: bool,
    pub supports_mcp: bool,
    pub supports_subagents: bool,
    pub supports_memory_lookup: bool,
}


impl Default for NativeCapabilities {
    fn default() -> Self {
        Self {
            contract_version: super::NATIVE_CAPABILITY_CONTRACT_VERSION,
            supports_oauth: false,
            supports_api_key: false,
            supports_sessions: false,
            supports_resume: false,
            supports_model_list: false,
            supports_usage: false,
            supports_tool_calls: false,
            supports_approval_events: false,
            supports_native_filesystem: false,
            supports_native_shell: false,
            supports_patch: false,
            supports_mcp: false,
            supports_subagents: false,
            supports_memory_lookup: false,
        }
    }
}

impl NativeCapabilities {
    pub fn supports(&self, capability: NativeCapability) -> bool {
        match capability {
            NativeCapability::OAuth => self.supports_oauth,
            NativeCapability::ApiKey => self.supports_api_key,
            NativeCapability::Sessions => self.supports_sessions,
            NativeCapability::Resume => self.supports_resume,
            NativeCapability::ModelList => self.supports_model_list,
            NativeCapability::Usage => self.supports_usage,
            NativeCapability::ToolCalls => self.supports_tool_calls,
            NativeCapability::ApprovalEvents => self.supports_approval_events,
            NativeCapability::NativeFilesystem => self.supports_native_filesystem,
            NativeCapability::NativeShell => self.supports_native_shell,
            NativeCapability::Patch => self.supports_patch,
            NativeCapability::Mcp => self.supports_mcp,
            NativeCapability::Subagents => self.supports_subagents,
            NativeCapability::MemoryLookup => self.supports_memory_lookup,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLookupAvailability {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl NativeLookupAvailability {
    pub fn available() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRuntimeDescriptor {
    pub runtime_id: String,
    pub runtime_version: String,
    pub request_contract_version: u16,
    pub event_contract_version: u16,
    pub provider: AgentProvider,
    pub product: AgentProductId,
    pub tool_execution_owner: NativeToolExecutionOwner,
    pub capabilities: NativeCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeContextRole {
    System,
    Skill,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeContextBlock {
    pub role: NativeContextRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSessionMode {
    Ephemeral,
    Start,
    Resume,
    Fork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeApprovalPolicy {
    Deny,
    Ask,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePermissionProfile {
    pub filesystem: NativeApprovalPolicy,
    pub shell: NativeApprovalPolicy,
    pub mcp: NativeApprovalPolicy,
    pub subagents: NativeApprovalPolicy,
    pub memory_lookup: NativeApprovalPolicy,
}

impl Default for NativePermissionProfile {
    fn default() -> Self {
        Self {
            filesystem: NativeApprovalPolicy::Ask,
            shell: NativeApprovalPolicy::Ask,
            mcp: NativeApprovalPolicy::Deny,
            subagents: NativeApprovalPolicy::Deny,
            memory_lookup: NativeApprovalPolicy::Deny,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeToolCapabilitySet {
    pub filesystem: bool,
    pub shell: bool,
    pub patch: bool,
    pub mcp: bool,
    pub subagents: bool,
    pub memory_lookup: bool,
}

impl Default for NativeToolCapabilitySet {
    fn default() -> Self {
        Self {
            filesystem: false,
            shell: false,
            patch: false,
            mcp: false,
            subagents: false,
            memory_lookup: false,
        }
    }
}

#[derive(Clone)]
pub struct NativeCancellation {
    id: String,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl fmt::Debug for NativeCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeCancellation")
            .field("id", &self.id)
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl NativeCancellation {
    pub fn new(id: impl Into<String>, timeout: Duration) -> Result<Self, NativeRuntimeError> {
        if timeout.is_zero() || timeout > MAX_TURN_TIMEOUT {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "native turn timeout is outside the supported range",
                false,
            ));
        }
        Ok(Self {
            id: id.into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + timeout,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn is_timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }

    pub fn checkpoint(&self) -> Result<(), NativeRuntimeError> {
        if self.is_cancelled() {
            Err(NativeRuntimeError::cancelled())
        } else if self.is_timed_out() {
            Err(NativeRuntimeError::timed_out())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTurnRequest {
    pub contract_version: u16,
    pub harness: AgentHarness,
    pub harness_version: String,
    pub runtime_version: String,
    pub provider: AgentProvider,
    pub account_ref: OpaqueAgentAccountRef,
    pub run_id: String,
    pub node_id: String,
    pub model: String,
    pub prompt: String,
    pub context: Vec<NativeContextBlock>,
    pub working_directory: PathBuf,
    pub allowed_workspace_roots: Vec<PathBuf>,
    pub permission_profile: NativePermissionProfile,
    pub tool_capabilities: NativeToolCapabilitySet,
    pub session_mode: NativeSessionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub event_limits: NativeEventLimits,
    pub timeout_ms: u64,
    #[serde(skip)]
    pub cancellation: Option<NativeCancellation>,
}

impl NativeTurnRequest {
    pub fn cancellation(&self) -> Result<&NativeCancellation, NativeRuntimeError> {
        self.cancellation.as_ref().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "native request is missing its cancellation handle",
                false,
            )
        })
    }
}

pub struct NativeCredential(Box<dyn Any + Send + Sync>);

impl fmt::Debug for NativeCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeCredential([REDACTED])")
    }
}

impl NativeCredential {
    pub fn new<T: Any + Send + Sync>(credential: T) -> Self {
        Self(Box::new(credential))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

#[derive(Debug)]
pub struct ResolvedNativeAccount {
    pub account_ref: OpaqueAgentAccountRef,
    pub provider: AgentProvider,
    pub product: AgentProductId,
    pub credential: NativeCredential,
}

pub trait NativeAccountResolver: Send + Sync {
    fn resolve(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        provider: AgentProvider,
        product: AgentProductId,
    ) -> Result<ResolvedNativeAccount, NativeRuntimeError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeUsageState {
    Supported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeUsageSnapshot {
    pub state: NativeUsageState,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub window_resets_at: Option<String>,
}

impl NativeUsageSnapshot {
    pub fn unavailable() -> Self {
        Self {
            state: NativeUsageState::Unavailable,
            input_tokens: None,
            output_tokens: None,
            window_resets_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTurnOutcome {
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeErrorCode {
    AccountUnavailable,
    AccountMismatch,
    ModelUnavailable,
    CapabilityUnsupported,
    SessionUnavailable,
    InvalidRequest,
    InvalidEvent,
    EventLimitExceeded,
    PermissionDenied,
    WorkspaceDenied,
    ToolOutputLimitExceeded,
    ToolTimeout,
    Cancelled,
    TimedOut,
    ProviderUnavailable,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{code:?}: {message}")]
pub struct NativeRuntimeError {
    pub code: NativeErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl NativeRuntimeError {
    pub fn new(code: NativeErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn cancelled() -> Self {
        Self::new(NativeErrorCode::Cancelled, "native turn cancelled", false)
    }

    pub fn timed_out() -> Self {
        Self::new(NativeErrorCode::TimedOut, "native turn timed out", true)
    }
}

pub trait NativeTurnHost {
    fn emit(&mut self, event: NativeEvent) -> Result<(), NativeRuntimeError>;
    fn invoke_tool(
        &mut self,
        request: super::AlfredToolRequest,
    ) -> Result<super::AlfredToolResult, NativeRuntimeError>;
    fn cancellation(&self) -> &NativeCancellation;
}

pub trait NativeAgentRuntime: Send + Sync {
    fn descriptor(&self) -> NativeRuntimeDescriptor;
    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError>;
    fn discover_models(
        &self,
        account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError>;
    fn run_turn(
        &self,
        account: &ResolvedNativeAccount,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError>;

    /// Reports whether this concrete registered runtime can carry Alfred's
    /// bounded memory lookup callback and result path. A descriptor flag only
    /// declares the protocol shape. This hook is the runtime-specific,
    /// fail-closed availability check.
    /// Implementations must also return unavailable when the selected model's
    /// context bound is unknown; the host cannot synthesize a run, model
    /// window, or provider probe merely to answer readiness.
    fn memory_lookup_availability(&self) -> NativeLookupAvailability {
        NativeLookupAvailability::unavailable(
            "native runtime has no verified Alfred memory lookup transport",
        )
    }

    /// Returns the runtime-owned, shared reservation ledger for this process.
    /// `None` is an explicit claim that remaining model context is unknown or
    /// cannot be reserved safely; callers must deny lookup reads in that case.
    /// Implementations must return the same ledger object for the lifetime of
    /// the registered runtime so reservations cover all traffic, not only
    /// memory lookup calls.
    fn memory_lookup_context_allowance(&self) -> Option<Arc<dyn MemoryLookupContextAllowance>> {
        None
    }

    fn cancel(&self, cancellation: &NativeCancellation) -> Result<(), NativeRuntimeError> {
        cancellation.cancel();
        Ok(())
    }
    fn usage_snapshot(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        Ok(NativeUsageSnapshot::unavailable())
    }
}
