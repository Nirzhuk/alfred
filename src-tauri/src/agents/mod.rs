pub mod active;
pub mod activity;
pub mod auth;
pub mod capability_manifest;
pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod gemini;
pub mod github_copilot;
pub mod grok;
pub mod managed_runtime;
pub mod models;
#[allow(dead_code, unused_imports)] // Provider plans consume this frozen registration contract.
pub mod native;
pub mod omp;
pub mod opencode;
pub mod pi;
pub(crate) mod process;
pub mod publisher_trust;
pub mod review;
pub mod runtime_package;
pub mod usage;

use crate::skills::{compose_prompt_with_skills, SkillRef};
use active::RunControl;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use activity::{AgentActivity, AgentActivityKind, AgentActivityState};
pub use auth::{auth_required, AgentAuthRequired};
pub use models::ProviderModels;
pub use usage::{list_provider_usage, AgentUsageSnapshot};

pub const INVALID_AGENT_HARNESS: &str = "invalid_agent_harness";
pub const INVALID_AGENT_ACCOUNT_REF: &str = "invalid_agent_account_ref";
pub const INVALID_AGENT_MODEL: &str = "invalid_agent_model";
pub const NATIVE_RUNTIME_UNAVAILABLE: &str = "native_runtime_unavailable";

const MAX_ACCOUNT_REF_CHARS: usize = 128;
const MAX_REQUEST_ID_CHARS: usize = 128;
const MAX_MODEL_ID_CHARS: usize = 256;
const MAX_FILES_CHANGED: usize = 128;
const MAX_FILE_PATH_CHARS: usize = 512;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHarness {
    #[default]
    Cli,
    Alfred,
}

impl AgentHarness {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Alfred => "alfred",
        }
    }

    /// Parse the persisted wire value. Missing values preserve old graphs by
    /// selecting the CLI harness; unknown values are never reinterpreted.
    pub fn parse_persisted(value: Option<&Value>) -> Result<Self, AgentError> {
        match value {
            None => Ok(Self::Cli),
            Some(Value::String(value)) if value == "cli" => Ok(Self::Cli),
            Some(Value::String(value)) if value == "alfred" => Ok(Self::Alfred),
            _ => Err(AgentError::InvalidHarness),
        }
    }
}

/// Validate and bound agent-node graph data at the workflow DTO boundary.
/// This both migrates missing harnesses and ensures credentials cannot hitch a
/// ride in an otherwise untyped graph object.
pub fn normalize_agent_nodes_in_graph(graph: &mut Value) -> Result<(), AgentError> {
    let Some(nodes) = graph.get_mut("nodes").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for node in nodes {
        if node.get("type").and_then(Value::as_str) != Some("agent") {
            continue;
        }
        let Some(data) = node.get_mut("data").and_then(Value::as_object_mut) else {
            continue;
        };
        let harness = AgentHarness::parse_persisted(data.get("harness"))?;
        data.insert("harness".into(), Value::String(harness.as_str().into()));
        data.retain(|key, _| {
            matches!(
                key.as_str(),
                "label"
                    | "provider"
                    | "harness"
                    | "accountRef"
                    | "model"
                    | "skillNames"
                    | "skillName"
            )
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    ClaudeCode,
    Cursor,
    Codex,
    Opencode,
    GithubCopilot,
    Gemini,
    Grok,
    Pi,
    Omp,
}

impl AgentProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::GithubCopilot => "github_copilot",
            Self::Gemini => "gemini",
            Self::Grok => "grok",
            Self::Pi => "pi",
            Self::Omp => "omp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
            Self::Opencode => "OpenCode",
            Self::GithubCopilot => "GitHub Copilot",
            Self::Gemini => "Gemini",
            Self::Grok => "Grok",
            Self::Pi => "Pi",
            Self::Omp => "OMP",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(Self::ClaudeCode),
            "cursor" => Some(Self::Cursor),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            "github_copilot" => Some(Self::GithubCopilot),
            "gemini" => Some(Self::Gemini),
            "grok" => Some(Self::Grok),
            "pi" => Some(Self::Pi),
            "omp" => Some(Self::Omp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequest {
    pub prompt: String,
    /// CLI model id / alias (`--model`).
    #[serde(default)]
    pub model: Option<String>,
    /// Optional skill to invoke for this step (e.g. `tdd` → `/tdd …`).
    #[serde(default)]
    pub skill: Option<SkillRef>,
    /// Legacy single skill name.
    #[serde(default)]
    pub skill_name: Option<String>,
    /// Preferred: multiple skills → `/a /b …`.
    #[serde(default)]
    pub skill_names: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct OpaqueAgentAccountRef(String);

impl OpaqueAgentAccountRef {
    pub fn parse(value: &str) -> Result<Self, AgentError> {
        let value = value.trim();
        let prefixed = ["account_", "acct_"]
            .into_iter()
            .find_map(|prefix| value.strip_prefix(prefix))
            .is_some_and(|suffix| suffix.len() >= 4);
        let valid = (8..=MAX_ACCOUNT_REF_CHARS).contains(&value.chars().count())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            && (prefixed || uuid::Uuid::parse_str(value).is_ok())
            && !looks_like_secret_value(value);
        valid
            .then(|| Self(value.to_owned()))
            .ok_or(AgentError::InvalidAccountRef)
    }

    /// The opaque identifier itself. It is an account row id, never a secret.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequestMetadata {
    run_id: String,
    node_id: String,
}

impl AgentRequestMetadata {
    pub fn new(run_id: &str, node_id: &str) -> Result<Self, AgentError> {
        if !safe_request_id(run_id) || !safe_request_id(node_id) {
            return Err(AgentError::InvalidRequestMetadata);
        }
        Ok(Self {
            run_id: run_id.to_owned(),
            node_id: node_id.to_owned(),
        })
    }
    pub(crate) fn native_ids(&self) -> (&str, &str) {
        (&self.run_id, &self.node_id)
    }
}

/// Bounded, allowlisted metadata safe for run history and UI events. Adapter
/// JSON is treated as untrusted input and only numeric usage fields survive.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeAgentFileChange {
    path: String,
    status: SafeAgentFileChangeStatus,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SafeAgentFileChangeStatus {
    Created,
    Modified,
    Deleted,
    Renamed,
}

impl SafeAgentFileChangeStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "created" => Some(Self::Created),
            "modified" => Some(Self::Modified),
            "deleted" => Some(Self::Deleted),
            "renamed" => Some(Self::Renamed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeAgentRunMetadata {
    pub provider: AgentProvider,
    pub harness: AgentHarness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_turns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_changed: Vec<SafeAgentFileChange>,
}

impl SafeAgentRunMetadata {
    pub fn identity(provider: AgentProvider, harness: AgentHarness, model: Option<&str>) -> Self {
        Self {
            provider,
            harness,
            model: model.and_then(safe_model_id),
            duration_ms: None,
            num_turns: None,
            total_cost_usd: None,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            files_changed: Vec::new(),
        }
    }

    pub fn from_untrusted(
        provider: AgentProvider,
        harness: AgentHarness,
        model: Option<&str>,
        value: &Value,
    ) -> Self {
        let mut metadata = Self::identity(provider, harness, model);
        metadata.duration_ms = safe_u64(value, &["durationMs", "duration_ms"], 86_400_000);
        metadata.num_turns = safe_u64(value, &["numTurns", "num_turns"], 1_000_000);
        metadata.total_cost_usd = safe_f64(value, &["totalCostUsd", "total_cost_usd"], 1_000_000.0);
        metadata.input_tokens = safe_u64(
            value,
            &["inputTokens", "input_tokens", "input"],
            1_000_000_000_000,
        );
        metadata.output_tokens = safe_u64(
            value,
            &["outputTokens", "output_tokens", "output"],
            1_000_000_000_000,
        );
        metadata.reasoning_tokens = safe_u64(
            value,
            &["reasoningTokens", "reasoning_tokens", "reasoning"],
            1_000_000_000_000,
        );
        metadata.cache_read_tokens = safe_u64(
            value,
            &["cacheReadTokens", "cache_read_tokens", "read"],
            1_000_000_000_000,
        );
        metadata.cache_creation_tokens = safe_u64(
            value,
            &["cacheCreationTokens", "cache_creation_tokens", "write"],
            1_000_000_000_000,
        );
        metadata
    }

    pub fn add_file_changed(&mut self, path: &str, status: &str) {
        if self.files_changed.len() >= MAX_FILES_CHANGED {
            return;
        }
        let Some(path) = safe_bounded_text(path, MAX_FILE_PATH_CHARS) else {
            return;
        };
        let Some(status) = SafeAgentFileChangeStatus::parse(status) else {
            return;
        };
        self.files_changed
            .push(SafeAgentFileChange { path, status });
    }
}

fn safe_u64(value: &Value, keys: &[&str], max: u64) -> Option<u64> {
    metadata_objects(value).find_map(|map| {
        keys.iter()
            .find_map(|key| map.get(*key).and_then(Value::as_u64))
            .filter(|number| *number <= max)
    })
}

fn safe_f64(value: &Value, keys: &[&str], max: f64) -> Option<f64> {
    metadata_objects(value).find_map(|map| {
        keys.iter()
            .find_map(|key| map.get(*key).and_then(Value::as_f64))
            .filter(|number| number.is_finite() && *number >= 0.0 && *number <= max)
    })
}

fn metadata_objects(value: &Value) -> impl Iterator<Item = &serde_json::Map<String, Value>> {
    let root = value.as_object();
    let nested = ["usage", "stats"]
        .into_iter()
        .filter_map(move |key| root.and_then(|map| map.get(key)).and_then(Value::as_object));
    root.into_iter().chain(nested)
}

fn safe_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_REQUEST_ID_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
        && !looks_like_secret_value(value)
}

fn safe_model_id(value: &str) -> Option<String> {
    safe_bounded_text(value.trim(), MAX_MODEL_ID_CHARS)
}

pub(crate) fn valid_model_id(value: &str) -> bool {
    safe_model_id(value).is_some()
}

fn safe_bounded_text(value: &str, max_chars: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
        && !looks_like_secret_value(value))
    .then(|| value.to_owned())
}

fn looks_like_secret_value(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    let secret_segment = lower
        .split(|character: char| {
            character.is_whitespace() || matches!(character, '/' | ':' | '=' | ',' | ';')
        })
        .any(|segment| {
            segment.starts_with("sk-")
                || segment.starts_with("ghp_")
                || segment.starts_with("github_pat_")
                || segment.starts_with("xox")
        });
    lower.contains("bearer ")
        || lower.contains("basic ")
        || secret_segment
        || (lower.starts_with("eyj") && lower.matches('.').count() == 2)
}

/// Safe, persisted execution identity. Account references are validated opaque
/// ids and request metadata is a closed typed shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionTarget {
    pub provider: AgentProvider,
    pub harness: AgentHarness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_ref: Option<OpaqueAgentAccountRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    pub request_metadata: AgentRequestMetadata,
}

impl AgentRequest {
    fn resolved_skill_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .skill_names
            .iter()
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .collect();
        if names.is_empty() {
            if let Some(ref skill) = self.skill {
                if !skill.name.trim().is_empty() {
                    names.push(skill.name.as_str());
                }
            }
        }
        if names.is_empty() {
            if let Some(ref name) = self.skill_name {
                if !name.trim().is_empty() {
                    names.push(name.as_str());
                }
            }
        }
        names
    }

    /// Final prompt sent to the CLI, with skill slash-commands applied when set.
    pub fn effective_prompt(&self) -> String {
        let names = self.resolved_skill_names();
        if names.is_empty() {
            self.prompt.clone()
        } else {
            compose_prompt_with_skills(&names, &self.prompt)
        }
    }

    pub fn effective_model(&self, provider: AgentProvider) -> String {
        self.model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| models::default_model(provider).to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    pub output: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("cancelled")]
    Cancelled,
    #[error("{INVALID_AGENT_HARNESS}")]
    InvalidHarness,
    #[error("{INVALID_AGENT_ACCOUNT_REF}")]
    InvalidAccountRef,
    #[error("invalid_agent_request_metadata")]
    InvalidRequestMetadata,
    #[error("{INVALID_AGENT_MODEL}")]
    InvalidModel,
    #[error("{NATIVE_RUNTIME_UNAVAILABLE}")]
    NativeRuntimeUnavailable,
    #[error("{0}")]
    Message(String),
}

/// Live-run hooks shared by CLI adapters (cancel + safe activity streaming).
pub struct AgentRunHooks<'a> {
    pub control: Option<&'a RunControl>,
    pub on_activity: Option<&'a dyn Fn(&AgentActivity)>,
}

pub trait AgentAdapter: Send + Sync {
    fn provider(&self) -> AgentProvider;
    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError>;
}

/// Registration seam for provider-native execution. Plan 030 intentionally
/// registers no runtime; callers must surface `native_runtime_unavailable`.
pub trait AgentNativeRuntime: Send + Sync {
    fn run(
        &self,
        target: &AgentExecutionTarget,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError>;
}

pub fn adapter_for(provider: AgentProvider) -> Box<dyn AgentAdapter> {
    match provider {
        AgentProvider::ClaudeCode => Box::new(claude_code::ClaudeCodeAdapter),
        AgentProvider::Cursor => Box::new(cursor::CursorAdapter),
        AgentProvider::Codex => Box::new(codex::CodexAdapter),
        AgentProvider::Opencode => Box::new(opencode::OpencodeAdapter),
        AgentProvider::GithubCopilot => Box::new(github_copilot::GithubCopilotAdapter),
        AgentProvider::Gemini => Box::new(gemini::GeminiAdapter),
        AgentProvider::Grok => Box::new(grok::GrokAdapter),
        AgentProvider::Pi => Box::new(pi::PiAdapter),
        AgentProvider::Omp => Box::new(omp::OmpAdapter),
    }
}

fn execute_target_with_adapter_factory<F>(
    target: &AgentExecutionTarget,
    request: AgentRequest,
    hooks: AgentRunHooks<'_>,
    native_runtime: Option<&dyn AgentNativeRuntime>,
    adapter_factory: F,
) -> Result<AgentResponse, AgentError>
where
    F: FnOnce(AgentProvider) -> Box<dyn AgentAdapter>,
{
    match target.harness {
        AgentHarness::Cli => adapter_factory(target.provider).run(request, hooks),
        AgentHarness::Alfred => native_runtime
            .ok_or(AgentError::NativeRuntimeUnavailable)?
            .run(target, request, hooks),
    }
}

pub fn execute_target(
    target: &AgentExecutionTarget,
    request: AgentRequest,
    hooks: AgentRunHooks<'_>,
    native_runtime: Option<&dyn AgentNativeRuntime>,
) -> Result<AgentResponse, AgentError> {
    execute_target_with_adapter_factory(target, request, hooks, native_runtime, adapter_for)
}

pub fn list_providers(
    manifest: &capability_manifest::AgentCapabilityManifest,
    harness: Option<AgentHarness>,
) -> Vec<serde_json::Value> {
    [
        AgentProvider::ClaudeCode,
        AgentProvider::Cursor,
        AgentProvider::Codex,
        AgentProvider::Opencode,
        AgentProvider::GithubCopilot,
        AgentProvider::Gemini,
        AgentProvider::Grok,
        AgentProvider::Pi,
        AgentProvider::Omp,
    ]
    .into_iter()
    .map(|p| {
        let selected = match harness {
            Some(value) => vec![value],
            None => vec![AgentHarness::Cli, AgentHarness::Alfred],
        };
        let harnesses = selected
            .into_iter()
            .map(|value| {
                let entry = manifest.entry(p, value);
                let available = entry.is_some_and(|entry| {
                    entry.permits_execution(manifest.platform, manifest.build_kind)
                });
                serde_json::json!({
                    "harness": value,
                    "available": available,
                    "requiresAccount": value == AgentHarness::Alfred,
                    "supportsOAuth": entry.is_some_and(|entry| entry.auth_methods.iter().any(|method| method.contains("oauth") || method.contains("device_code"))),
                    "supportsApiKey": entry.is_some_and(|entry| entry.auth_methods.iter().any(|method| method.contains("api_key") || method.contains("secret"))),
                    "supportsUsage": entry.is_some_and(|entry| entry.usage_source != "unavailable"),
                    "connected": false,
                    "accounts": [],
                    "status": entry.map(|entry| entry.status),
                    "runtimeVersion": entry.and_then(|entry| entry.runtime_version.as_deref()),
                    "error": (!available).then(|| entry.and_then(|entry| entry.block_reason.as_deref()).unwrap_or(NATIVE_RUNTIME_UNAVAILABLE)),
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "id": p.as_str(),
            "label": p.label(),
            "defaultModel": models::default_model(p),
            "harnesses": harnesses,
        })
    })
    .collect()
}

#[cfg(test)]
mod harness_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn harness_round_trips_and_missing_defaults_to_cli() {
        assert_eq!(serde_json::to_value(AgentHarness::Cli).unwrap(), "cli");
        assert_eq!(
            serde_json::to_value(AgentHarness::Alfred).unwrap(),
            "alfred"
        );
        assert_eq!(
            AgentHarness::parse_persisted(None).unwrap(),
            AgentHarness::Cli
        );
        assert_eq!(
            AgentHarness::parse_persisted(Some(&serde_json::json!("alfred"))).unwrap(),
            AgentHarness::Alfred
        );
    }

    #[test]
    fn unknown_harness_has_stable_validation_error() {
        for value in [serde_json::json!("vendor"), Value::Null] {
            let error = AgentHarness::parse_persisted(Some(&value)).unwrap_err();
            assert_eq!(error.to_string(), INVALID_AGENT_HARNESS);
        }
    }

    #[test]
    fn workflow_graph_migration_defaults_and_redacts_agent_data() {
        let mut graph = serde_json::json!({
            "nodes": [{
                "id": "agent",
                "type": "agent",
                "data": {
                    "label": "Agent",
                    "provider": "codex",
                    "model": "gpt",
                    "accessToken": "secret",
                    "credentials": { "refreshToken": "secret" }
                }
            }],
            "edges": []
        });
        normalize_agent_nodes_in_graph(&mut graph).unwrap();
        assert_eq!(graph["nodes"][0]["data"]["harness"], "cli");
        let serialized = serde_json::to_string(&graph).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("accessToken"));
        assert!(!serialized.contains("credentials"));
    }

    #[test]
    fn alfred_without_runtime_never_builds_a_cli_adapter() {
        let target = AgentExecutionTarget {
            provider: AgentProvider::Codex,
            harness: AgentHarness::Alfred,
            account_ref: Some(OpaqueAgentAccountRef::parse("account_opaque").unwrap()),
            model: Some("model".into()),
            working_directory: None,
            request_metadata: AgentRequestMetadata::new("run_1", "node_1").unwrap(),
        };
        let cli_called = AtomicBool::new(false);
        let result = execute_target_with_adapter_factory(
            &target,
            AgentRequest {
                prompt: "hello".into(),
                model: None,
                skill: None,
                skill_name: None,
                skill_names: vec![],
                working_directory: None,
                extra: Value::Null,
            },
            AgentRunHooks {
                control: None,
                on_activity: None,
            },
            None,
            |_| {
                cli_called.store(true, Ordering::SeqCst);
                panic!("native execution must not construct a CLI adapter")
            },
        );
        assert_eq!(result.unwrap_err().to_string(), NATIVE_RUNTIME_UNAVAILABLE);
        assert!(!cli_called.load(Ordering::SeqCst));
    }

    #[test]
    fn every_provider_keeps_its_cli_adapter_identity() {
        for provider in [
            AgentProvider::ClaudeCode,
            AgentProvider::Cursor,
            AgentProvider::Codex,
            AgentProvider::Opencode,
            AgentProvider::GithubCopilot,
            AgentProvider::Gemini,
            AgentProvider::Grok,
            AgentProvider::Pi,
            AgentProvider::Omp,
        ] {
            assert_eq!(adapter_for(provider).provider(), provider);
        }
    }

    #[test]
    fn execution_target_serialization_has_no_credential_fields() {
        let target = AgentExecutionTarget {
            provider: AgentProvider::Codex,
            harness: AgentHarness::Alfred,
            account_ref: Some(OpaqueAgentAccountRef::parse("account_opaque").unwrap()),
            model: Some("gpt".into()),
            working_directory: Some("/tmp/project".into()),
            request_metadata: AgentRequestMetadata::new("run_1", "node_1").unwrap(),
        };
        let serialized = serde_json::to_string(&target).unwrap();
        assert!(serialized.contains("account_opaque"));
        for forbidden in ["accessToken", "refreshToken", "apiKey", "credential"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn opaque_account_references_reject_credentials_and_unbounded_values() {
        assert!(OpaqueAgentAccountRef::parse("account_opaque-01").is_ok());
        for value in [
            "sk-live-secret",
            "Bearer raw-token",
            "too short",
            "abcdefghijklmnop1234567890",
            &"a".repeat(MAX_ACCOUNT_REF_CHARS + 1),
        ] {
            assert!(matches!(
                OpaqueAgentAccountRef::parse(value),
                Err(AgentError::InvalidAccountRef)
            ));
        }
    }

    #[test]
    fn safe_metadata_is_typed_bounded_and_drops_adversarial_fields_and_values() {
        let raw = serde_json::json!({
            "inputTokens": 12,
            "durationMs": 42,
            "accessToken": "obvious-secret",
            "innocent": "sk-live-secret",
            "usage": {
                "output_tokens": 8,
                "password": "another-secret"
            },
            "stats": { "reasoning_tokens": 3 },
            "oversized": "x".repeat(10_000)
        });
        let mut metadata = SafeAgentRunMetadata::from_untrusted(
            AgentProvider::Codex,
            AgentHarness::Cli,
            Some("gpt-5.6-luna"),
            &raw,
        );
        for index in 0..MAX_FILES_CHANGED + 20 {
            metadata.add_file_changed(&format!("src/file-{index}.rs"), "modified");
        }
        let serialized = serde_json::to_string(&metadata).unwrap();
        assert!(serialized.contains(r#""inputTokens":12"#));
        assert!(serialized.contains(r#""outputTokens":8"#));
        assert!(serialized.contains(r#""reasoningTokens":3"#));
        assert_eq!(metadata.files_changed.len(), MAX_FILES_CHANGED);
        for forbidden in [
            "obvious-secret",
            "another-secret",
            "sk-live-secret",
            "accessToken",
            "password",
            "innocent",
            "oversized",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn provider_capabilities_keep_native_unavailable_and_disconnected() {
        let manifest = capability_manifest::manifest();
        let providers = list_providers(&manifest, None);
        assert_eq!(providers.len(), 9);
        let codex = providers
            .iter()
            .find(|provider| provider["id"] == "codex")
            .unwrap();
        assert_eq!(codex["harnesses"][0]["harness"], "cli");
        assert_eq!(codex["harnesses"][1]["harness"], "alfred");
        assert_eq!(codex["harnesses"][1]["available"], false);
        assert_eq!(codex["harnesses"][1]["connected"], false);
        assert_eq!(
            codex["harnesses"][1]["error"],
            "codex_cross_platform_signing_and_packaged_smoke_missing"
        );
    }
}

pub(crate) fn map_cmd_err(err: String) -> AgentError {
    if err == "cancelled" {
        AgentError::Cancelled
    } else {
        AgentError::Message(err)
    }
}
