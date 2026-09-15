use super::redaction::{
    contains_cli_permission_flag, contains_secret_marker, is_secret_key, redact_text,
};
use super::{
    NativeAgentRuntime, NativeApprovalPolicy, NativeCancellation, NativeErrorCode,
    NativePermissionProfile, NativeRuntimeError, NativeToolCapabilitySet, NativeTurnRequest,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const TOOL_CONTRACT_VERSION: u16 = 2;
pub const DEFAULT_MAX_TOOL_OUTPUT_BYTES: usize = 128 * 1024;
pub const DEFAULT_MAX_COMMAND_TIMEOUT_MS: u64 = 120_000;
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlfredToolKind {
    FileRead,
    FileWrite,
    FileEdit,
    DirectoryList,
    Shell,
    Process,
    ApplyPatch,
    Mcp,
    Subagent,
    MemoryLookup,
}

impl AlfredToolKind {
    fn permission(self, profile: &NativePermissionProfile) -> NativeApprovalPolicy {
        match self {
            Self::FileRead
            | Self::FileWrite
            | Self::FileEdit
            | Self::DirectoryList
            | Self::ApplyPatch => profile.filesystem,
            Self::Shell | Self::Process => profile.shell,
            Self::Mcp => profile.mcp,
            Self::Subagent => profile.subagents,
            Self::MemoryLookup => profile.memory_lookup,
        }
    }

    /// Tools that touch or execute inside the workspace must name their target
    /// (or, for a command, its working directory) so it can be confined.
    fn requires_workspace_path(self) -> bool {
        matches!(
            self,
            Self::FileRead
                | Self::FileWrite
                | Self::FileEdit
                | Self::DirectoryList
                | Self::ApplyPatch
                | Self::Shell
                | Self::Process
        )
    }

    fn supported(self, capabilities: &NativeToolCapabilitySet) -> bool {
        match self {
            Self::FileRead | Self::FileWrite | Self::FileEdit | Self::DirectoryList => {
                capabilities.filesystem
            }
            Self::Shell | Self::Process => capabilities.shell,
            Self::ApplyPatch => capabilities.patch,
            Self::Mcp => capabilities.mcp,
            Self::Subagent => capabilities.subagents,
            Self::MemoryLookup => capabilities.memory_lookup,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlfredToolExecutionContext {
    pub turn_id: String,
    pub provider: crate::agents::AgentProvider,
    pub model: String,
    pub run_id: String,
    pub node_id: String,
    pub working_directory: PathBuf,
    pub memory_lookup_enabled: bool,
}

impl AlfredToolExecutionContext {
    pub(crate) fn from_turn(request: &NativeTurnRequest) -> Result<Self, NativeRuntimeError> {
        let cancellation = request.cancellation()?;
        Ok(Self {
            turn_id: cancellation.id().to_owned(),
            provider: request.provider,
            model: request.model.clone(),
            run_id: request.run_id.clone(),
            node_id: request.node_id.clone(),
            working_directory: request.working_directory.clone(),
            memory_lookup_enabled: request.permission_profile.memory_lookup
                == NativeApprovalPolicy::Allow
                && request.tool_capabilities.memory_lookup,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlfredToolRequest {
    pub contract_version: u16,
    pub request_id: String,
    pub kind: AlfredToolKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub input: Map<String, Value>,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

impl AlfredToolRequest {
    pub fn new(
        request_id: impl Into<String>,
        kind: AlfredToolKind,
        name: impl Into<String>,
    ) -> Self {
        Self {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: request_id.into(),
            kind,
            name: name.into(),
            path: None,
            arguments: Vec::new(),
            input: Map::new(),
            timeout_ms: DEFAULT_MAX_COMMAND_TIMEOUT_MS,
            max_output_bytes: DEFAULT_MAX_TOOL_OUTPUT_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlfredToolStatus {
    Completed,
    Denied,
    Cancelled,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlfredToolResult {
    pub contract_version: u16,
    pub request_id: String,
    pub status: AlfredToolStatus,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

impl AlfredToolResult {
    pub fn denied(request_id: &str) -> Self {
        Self {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: request_id.into(),
            status: AlfredToolStatus::Denied,
            output: "Tool request denied by Alfred permission policy.".into(),
            exit_code: None,
            truncated: false,
            metadata: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlfredApprovalRequest {
    pub contract_version: u16,
    pub approval_id: String,
    pub tool_request_id: String,
    pub tool_name: String,
    pub kind: AlfredToolKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlfredApprovalDecision {
    Allow,
    Deny,
}

pub trait AlfredApprovalHandler: Send + Sync {
    fn decide(
        &self,
        request: &AlfredApprovalRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredApprovalDecision, NativeRuntimeError>;
}

pub trait AlfredToolExecutor: Send + Sync {
    /// Whether this installed host executor can serve Alfred-owned memory
    /// lookup requests. Deny-all and legacy executors stay unavailable.
    fn supports_memory_lookup(&self) -> bool {
        false
    }
    /// Binds this executor to the selected runtime for one native turn.
    /// Registry-backed lookup executors use this to pin reservations and
    /// releases to the exact runtime selected for execution.
    fn bind_runtime(
        &self,
        _context: &AlfredToolExecutionContext,
        _runtime: Arc<dyn NativeAgentRuntime>,
    ) -> Result<(), NativeRuntimeError> {
        Ok(())
    }

    fn begin_turn(
        &self,
        _context: &AlfredToolExecutionContext,
        _cancellation: &NativeCancellation,
    ) -> Result<(), NativeRuntimeError> {
        Ok(())
    }

    fn execute(
        &self,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError>;

    fn execute_for_turn(
        &self,
        _context: &AlfredToolExecutionContext,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        self.execute(request, cancellation)
    }

    /// Gives a scoped executor a commit point after the registry has accepted
    /// the normalized result. Lookup traces use this hook so a result that
    /// fails host output or context validation is never marked Delivered.
    fn finalize_for_turn(
        &self,
        _context: &AlfredToolExecutionContext,
        _request: &AlfredToolRequest,
        _result: &AlfredToolResult,
    ) -> Result<(), NativeRuntimeError> {
        Ok(())
    }

    /// Rejects a result that was produced by a scoped executor but cancelled
    /// before the host delivered it. The default is a no-op for executors that
    /// do not stage durable result metadata.
    fn reject_for_turn(
        &self,
        _context: &AlfredToolExecutionContext,
        _request: &AlfredToolRequest,
        _error: &NativeRuntimeError,
    ) -> Result<(), NativeRuntimeError> {
        Ok(())
    }

    fn finish_turn(&self, _context: &AlfredToolExecutionContext) {}

}

pub struct DenyAllToolExecutor;

impl AlfredToolExecutor for DenyAllToolExecutor {
    fn execute(
        &self,
        _request: &AlfredToolRequest,
        _cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "no Alfred tool executor is registered",
            false,
        ))
    }
}

pub struct DenyAllApprovalHandler;

impl AlfredApprovalHandler for DenyAllApprovalHandler {
    fn decide(
        &self,
        _request: &AlfredApprovalRequest,
        _cancellation: &NativeCancellation,
    ) -> Result<AlfredApprovalDecision, NativeRuntimeError> {
        Ok(AlfredApprovalDecision::Deny)
    }
}

pub(crate) fn validate_tool_request(
    request: &AlfredToolRequest,
    working_directory: &Path,
    roots: &[PathBuf],
    profile: &NativePermissionProfile,
    capabilities: &NativeToolCapabilitySet,
) -> Result<NativeApprovalPolicy, NativeRuntimeError> {
    if request.contract_version != TOOL_CONTRACT_VERSION {
        return Err(invalid_tool("unsupported Alfred tool contract version"));
    }
    if request.request_id.is_empty()
        || request.request_id.len() > 128
        || request.name.is_empty()
        || request.name.len() > 128
    {
        return Err(invalid_tool("Alfred tool identity is invalid"));
    }
    if !request.kind.supported(capabilities) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::CapabilityUnsupported,
            "the native request did not grant this tool capability",
            false,
        ));
    }
    if request.timeout_ms == 0 || request.timeout_ms > DEFAULT_MAX_COMMAND_TIMEOUT_MS {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ToolTimeout,
            "tool command timeout exceeds the Alfred limit",
            false,
        ));
    }
    if request.max_output_bytes == 0 || request.max_output_bytes > DEFAULT_MAX_TOOL_OUTPUT_BYTES {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ToolOutputLimitExceeded,
            "tool output limit exceeds the Alfred maximum",
            false,
        ));
    }
    let argument_bytes = request.arguments.iter().map(String::len).sum::<usize>()
        + serde_json::to_vec(&request.input)
            .map_err(|_| invalid_tool("tool input could not be encoded"))?
            .len();
    if argument_bytes > MAX_TOOL_ARGUMENT_BYTES {
        return Err(invalid_tool("tool arguments exceed the Alfred limit"));
    }
    if contains_secret_material(&Value::Object(request.input.clone()))
        || request
            .arguments
            .iter()
            .any(|argument| contains_secret_marker(argument))
        || contains_secret_marker(&request.name)
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "secret-bearing fields are prohibited in Alfred tool requests",
            false,
        ));
    }
    reject_cli_permission_inheritance(request)?;
    match request.path.as_deref() {
        Some(path) => {
            validate_workspace_path(path, working_directory, roots)?;
        }
        // A command tool without an explicit cwd would run wherever the
        // executor happens to be; the Alfred boundary refuses to guess.
        None if request.kind.requires_workspace_path() => {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::WorkspaceDenied,
                "this Alfred tool requires an explicit workspace-confined path",
                false,
            ));
        }
        None => {}
    }
    Ok(request.kind.permission(profile))
}

pub(crate) fn normalize_tool_result(
    mut result: AlfredToolResult,
    request: &AlfredToolRequest,
) -> Result<AlfredToolResult, NativeRuntimeError> {
    if result.contract_version != TOOL_CONTRACT_VERSION || result.request_id != request.request_id {
        return Err(invalid_tool("tool result does not match its request"));
    }
    if result.output.len() > request.max_output_bytes {
        result.output = truncate_utf8(&result.output, request.max_output_bytes);
        result.truncated = true;
    }
    result.output = redact_text(&result.output);
    if serde_json::to_vec(&result.metadata)
        .map_err(|_| invalid_tool("tool result metadata could not be encoded"))?
        .len()
        > 16 * 1024
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ToolOutputLimitExceeded,
            "tool result metadata exceeded the Alfred limit",
            false,
        ));
    }
    for (key, value) in result.metadata.iter_mut() {
        if is_secret_key(key) {
            *value = Value::String("[REDACTED]".into());
        } else if let Value::String(text) = value {
            *text = redact_text(text);
        }
    }
    Ok(result)
}

fn reject_cli_permission_inheritance(
    request: &AlfredToolRequest,
) -> Result<(), NativeRuntimeError> {
    let serialized = serde_json::to_string(request)
        .map_err(|_| invalid_tool("tool request could not be encoded"))?;
    if contains_cli_permission_flag(&serialized) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "CLI permission flags cannot be inherited by the Alfred harness",
            false,
        ));
    }
    Ok(())
}

pub fn validate_workspace_path(
    path: &Path,
    working_directory: &Path,
    roots: &[PathBuf],
) -> Result<PathBuf, NativeRuntimeError> {
    if roots.is_empty() {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::WorkspaceDenied,
            "native request has no allowed workspace roots",
            false,
        ));
    }
    let absolute = if path.is_absolute() {
        normalize_lexically(path)
    } else {
        normalize_lexically(&working_directory.join(path))
    }?;
    let absolute = canonicalize_with_missing(&absolute)?;
    let allowed = roots
        .iter()
        .filter_map(|root| canonicalize_with_missing(root).ok())
        .any(|root| absolute == root || absolute.starts_with(&root));
    if allowed {
        Ok(absolute)
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::WorkspaceDenied,
            "tool path escapes the allowed workspace roots",
            false,
        ))
    }
}

fn canonicalize_with_missing(path: &Path) -> Result<PathBuf, NativeRuntimeError> {
    let normalized = normalize_lexically(path)?;
    let mut existing = normalized.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::WorkspaceDenied,
                "workspace path has no existing ancestor",
                false,
            )
        })?;
        missing.push(name.to_os_string());
        existing = existing.parent().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::WorkspaceDenied,
                "workspace path has no existing ancestor",
                false,
            )
        })?;
    }
    let mut canonical = existing.canonicalize().map_err(|_| {
        NativeRuntimeError::new(
            NativeErrorCode::WorkspaceDenied,
            "workspace path could not be canonicalized",
            false,
        )
    })?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn normalize_lexically(path: &Path) -> Result<PathBuf, NativeRuntimeError> {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            Component::RootDir => output.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::Normal(value) => output.push(value),
            Component::ParentDir => {
                if !output.pop() {
                    return Err(NativeRuntimeError::new(
                        NativeErrorCode::WorkspaceDenied,
                        "tool path cannot traverse above its root",
                        false,
                    ));
                }
            }
        }
    }
    if output.is_absolute() {
        Ok(output)
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::WorkspaceDenied,
            "tool path did not resolve to an absolute workspace path",
            false,
        ))
    }
}

fn truncate_utf8(value: &str, maximum: usize) -> String {
    let mut end = maximum.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn contains_secret_material(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, child)| is_secret_key(key) || contains_secret_material(child)),
        Value::Array(values) => values.iter().any(contains_secret_material),
        Value::String(value) => contains_secret_marker(value),
        _ => false,
    }
}

fn invalid_tool(message: impl Into<String>) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidRequest, message, false)
}
