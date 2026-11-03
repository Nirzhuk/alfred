//! Discover models from each local agent automatically, so a new model release
//! needs no Alfred update. Fallbacks hold only version-free ids (aliases or the
//! CLI's own default) because anything versioned here eventually goes stale.
//!
//! Sources:
//! - Claude Code: the CLI's `initialize` handshake, else its aliases
//! - OpenCode: `opencode models`
//! - Codex: `codex debug models`, else `~/.codex/models_cache.json`
//! - Cursor: Cursor IDE `availableDefaultModels2` from `state.vscdb`, else
//!   `cursor-agent models` / `--list-models`
//! - Grok: `grok models`
//! - pi: `pi --list-models` (authenticated providers only)
//! - OMP: `omp models --json`
//! - Gemini: the CLI's own aliases (no non-interactive catalog)
//! - GitHub Copilot: the CLI default plus custom ids (no documented
//!   non-interactive catalog)

use super::pi;
use super::{valid_model_id, AgentError, AgentHarness, AgentProvider, NATIVE_RUNTIME_UNAVAILABLE};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    /// Value passed to the CLI (`--model`).
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_variant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_fast_variant: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_fast_toggle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModels {
    pub provider: String,
    #[serde(flatten)]
    pub capabilities: HarnessModelCapabilities,
    pub default_model: String,
    pub models: Vec<ModelOption>,
    /// Allow typing a custom model id (useful for OpenCode provider/model).
    pub allow_custom: bool,
    /// `discovered`, `fallback`, or `unavailable` for an unregistered native runtime.
    pub source: String,
    /// Whether the agent binary/cache was found.
    pub available: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessModelCapabilities {
    pub harness: AgentHarness,
    pub requires_account: bool,
    pub supports_o_auth: bool,
    pub supports_api_key: bool,
    pub supports_usage: bool,
    pub account_connected: bool,
    pub native_runtime_available: bool,
}

fn cli_capabilities() -> HarnessModelCapabilities {
    HarnessModelCapabilities {
        harness: AgentHarness::Cli,
        requires_account: false,
        supports_o_auth: false,
        supports_api_key: false,
        supports_usage: false,
        account_connected: false,
        native_runtime_available: false,
    }
}

fn unavailable_native_capabilities(
    provider: AgentProvider,
    manifest: &super::capability_manifest::AgentCapabilityManifest,
) -> HarnessModelCapabilities {
    let entry = manifest.entry(provider, AgentHarness::Alfred);
    HarnessModelCapabilities {
        harness: AgentHarness::Alfred,
        requires_account: true,
        supports_o_auth: entry.is_some_and(|entry| {
            entry
                .auth_methods
                .iter()
                .any(|method| method.contains("oauth") || method.contains("device_code"))
        }),
        supports_api_key: entry.is_some_and(|entry| {
            entry
                .auth_methods
                .iter()
                .any(|method| method.contains("api_key") || method.contains("secret"))
        }),
        supports_usage: entry.is_some_and(|entry| entry.usage_source != "unavailable"),
        account_connected: false,
        native_runtime_available: entry
            .is_some_and(|entry| entry.permits_execution(manifest.platform, manifest.build_kind)),
    }
}

fn opt(
    id: impl Into<String>,
    label: impl Into<String>,
    description: impl Into<String>,
) -> ModelOption {
    ModelOption {
        id: id.into(),
        label: label.into(),
        description: description.into(),
        base_id: None,
        fast_variant_id: None,
        is_fast_variant: None,
        supports_fast_toggle: None,
    }
}

fn fast_base_id(id: &str) -> Option<&str> {
    id.strip_suffix("-fast").filter(|base| !base.is_empty())
}

fn is_fast_variant(model: &ModelOption) -> bool {
    model.is_fast_variant == Some(true) || fast_base_id(&model.id).is_some()
}

/// Collapse confident base/fast pairs into one picker row.
///
/// A pair is accepted only when one base maps to one fast variant and that
/// fast variant maps back to the same base. Ambiguous or incomplete matches
/// remain flat so discovery never silently changes a model id.
pub fn pair_fast_variants(models: Vec<ModelOption>) -> Vec<ModelOption> {
    if models.len() < 2 {
        return models;
    }

    let mut indices_by_id: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, model) in models.iter().enumerate() {
        indices_by_id
            .entry(model.id.clone())
            .or_default()
            .push(index);
    }

    let mut base_to_fast: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut fast_to_base: HashMap<usize, Vec<usize>> = HashMap::new();

    for (base_index, model) in models.iter().enumerate() {
        if is_fast_variant(model) {
            continue;
        }
        let Some(fast_id) = model.fast_variant_id.as_deref() else {
            continue;
        };
        let Some(fast_indices) = indices_by_id.get(fast_id) else {
            continue;
        };
        for &fast_index in fast_indices {
            if fast_index == base_index {
                continue;
            }
            base_to_fast.entry(base_index).or_default().push(fast_index);
            fast_to_base.entry(fast_index).or_default().push(base_index);
        }
    }

    for (fast_index, model) in models.iter().enumerate() {
        if !is_fast_variant(model) {
            continue;
        }
        let base_id = model.base_id.as_deref().or_else(|| fast_base_id(&model.id));
        let Some(base_id) = base_id else {
            continue;
        };
        let Some(base_indices) = indices_by_id.get(base_id) else {
            continue;
        };
        for &base_index in base_indices {
            if base_index == fast_index || is_fast_variant(&models[base_index]) {
                continue;
            }
            base_to_fast.entry(base_index).or_default().push(fast_index);
            fast_to_base.entry(fast_index).or_default().push(base_index);
        }
    }

    for indices in base_to_fast.values_mut() {
        indices.sort_unstable();
        indices.dedup();
    }
    for indices in fast_to_base.values_mut() {
        indices.sort_unstable();
        indices.dedup();
    }

    let mut fast_id_by_base = HashMap::new();
    let mut paired_fast = vec![false; models.len()];
    for (&base_index, fast_indices) in &base_to_fast {
        if fast_indices.len() != 1 {
            continue;
        }
        let fast_index = fast_indices[0];
        let Some(base_indices) = fast_to_base.get(&fast_index) else {
            continue;
        };
        if base_indices.len() != 1 || base_indices[0] != base_index {
            continue;
        }
        fast_id_by_base.insert(base_index, models[fast_index].id.clone());
        paired_fast[fast_index] = true;
    }

    if fast_id_by_base.is_empty() {
        return models;
    }

    let mut paired_models = Vec::with_capacity(models.len());
    for (index, mut model) in models.into_iter().enumerate() {
        if paired_fast[index] {
            continue;
        }
        if let Some(fast_id) = fast_id_by_base.get(&index) {
            model.base_id = Some(model.id.clone());
            model.fast_variant_id = Some(fast_id.clone());
            model.is_fast_variant = Some(false);
            model.supports_fast_toggle = Some(true);
        }
        paired_models.push(model);
    }
    paired_models
}

#[cfg(test)]
fn test_model(id: &str) -> ModelOption {
    opt(id, id, "")
}

pub fn default_model(provider: AgentProvider) -> &'static str {
    match provider {
        AgentProvider::ClaudeCode => "sonnet",
        AgentProvider::Gemini => "auto",
        // These reach many models, so the only id that stays valid on every
        // install and through every release is the one the user configured.
        AgentProvider::Cursor
        | AgentProvider::Codex
        | AgentProvider::Opencode
        | AgentProvider::GithubCopilot
        | AgentProvider::Grok
        | AgentProvider::Pi
        | AgentProvider::Omp => pi::CLI_DEFAULT_MODEL,
    }
}

/// Drop `--model default` so the CLI falls back to its configured model.
pub(super) fn omit_cli_default_model(mut args: Vec<String>) -> Vec<String> {
    if let Some(index) = args.iter().position(|arg| arg == "--model") {
        if args.get(index + 1).map(String::as_str) == Some(pi::CLI_DEFAULT_MODEL) {
            args.drain(index..=index + 1);
        }
    }
    args
}

/// Static fallback when discovery fails (CLI missing / not logged in).
pub fn fallback_models(provider: AgentProvider) -> ProviderModels {
    let models = match provider {
        AgentProvider::ClaudeCode => vec![
            opt("sonnet", "Sonnet", "Latest · Claude Code alias"),
            opt("opus", "Opus", "Latest · Claude Code alias"),
            opt("haiku", "Haiku", "Latest · Claude Code alias"),
            opt("fable", "Fable", "Latest · Claude Code alias"),
        ],
        AgentProvider::Gemini => vec![
            opt("auto", "Auto", "Gemini CLI model routing"),
            opt("pro", "Pro", "Latest · Gemini CLI alias"),
            opt("flash", "Flash", "Latest · Gemini CLI alias"),
            opt("flash-lite", "Flash Lite", "Latest · Gemini CLI alias"),
        ],
        AgentProvider::Cursor => vec![cursor_auto_model()],
        AgentProvider::Codex
        | AgentProvider::Opencode
        | AgentProvider::GithubCopilot
        | AgentProvider::Grok
        | AgentProvider::Pi
        | AgentProvider::Omp => vec![cli_default_model()],
    };

    ProviderModels {
        provider: provider.as_str().into(),
        capabilities: cli_capabilities(),
        default_model: default_model(provider).into(),
        models,
        allow_custom: true,
        source: "fallback".into(),
        available: false,
        error: None,
    }
}

/// Discover models from the installed agents (may take a few seconds).
pub fn discover_all() -> Vec<ProviderModels> {
    let providers = [
        AgentProvider::ClaudeCode,
        AgentProvider::Cursor,
        AgentProvider::Codex,
        AgentProvider::Opencode,
        AgentProvider::GithubCopilot,
        AgentProvider::Gemini,
        AgentProvider::Grok,
        AgentProvider::Pi,
        AgentProvider::Omp,
    ];
    std::thread::scope(|scope| {
        let handles = providers.map(|provider| scope.spawn(move || discover_for(provider)));
        providers
            .into_iter()
            .zip(handles)
            .map(|(provider, handle)| handle.join().unwrap_or_else(|_| fallback_models(provider)))
            .collect()
    })
}

pub fn list_provider_models(harness: Option<AgentHarness>) -> Vec<ProviderModels> {
    let manifest = super::capability_manifest::manifest();
    list_provider_models_with_manifest(harness, &manifest)
}

pub fn list_provider_models_with_manifest(
    harness: Option<AgentHarness>,
    manifest: &super::capability_manifest::AgentCapabilityManifest,
) -> Vec<ProviderModels> {
    match harness.unwrap_or_default() {
        AgentHarness::Cli => discover_all(),
        AgentHarness::Alfred => native_unavailable_models(manifest),
    }
}

/// Resolve and validate the model for one execution target. CLI keeps its
/// historical default; Alfred never borrows that default and may execute with
/// no model only so the native runtime can report its honest availability.
pub fn resolve_model_for_execution(
    provider: AgentProvider,
    harness: AgentHarness,
    requested: Option<&str>,
) -> Result<Option<String>, AgentError> {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    let candidate = match (harness, requested) {
        (AgentHarness::Cli, Some(model)) => Some(model),
        (AgentHarness::Cli, None) => Some(default_model(provider)),
        (AgentHarness::Alfred, Some(model)) => Some(model),
        (AgentHarness::Alfred, None) => return Ok(None),
    };
    let candidate = candidate.expect("model candidate is present");
    if !valid_model_id(candidate) {
        return Err(AgentError::InvalidModel);
    }
    // CLI catalogs always allow custom ids, so probing the CLI before every
    // step could not reject anything.
    if harness == AgentHarness::Alfred {
        validate_model_in_catalog(&native_unavailable_model(provider), candidate)?;
    }
    Ok(Some(candidate.to_owned()))
}

fn validate_model_in_catalog(catalog: &ProviderModels, model: &str) -> Result<(), AgentError> {
    let listed = catalog.models.iter().any(|option| {
        option.id == model
            || option.base_id.as_deref() == Some(model)
            || option.fast_variant_id.as_deref() == Some(model)
    });
    if listed || catalog.allow_custom {
        Ok(())
    } else {
        Err(AgentError::InvalidModel)
    }
}

fn native_unavailable_models(
    manifest: &super::capability_manifest::AgentCapabilityManifest,
) -> Vec<ProviderModels> {
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
    .map(|provider| native_unavailable_model_from_manifest(provider, manifest))
    .collect()
}

fn native_unavailable_model(provider: AgentProvider) -> ProviderModels {
    let manifest = super::capability_manifest::manifest();
    native_unavailable_model_from_manifest(provider, &manifest)
}

fn native_unavailable_model_from_manifest(
    provider: AgentProvider,
    manifest: &super::capability_manifest::AgentCapabilityManifest,
) -> ProviderModels {
    let entry = manifest.entry(provider, AgentHarness::Alfred);
    let available =
        entry.is_some_and(|entry| entry.permits_execution(manifest.platform, manifest.build_kind));
    ProviderModels {
        provider: provider.as_str().into(),
        capabilities: unavailable_native_capabilities(provider, manifest),
        default_model: String::new(),
        models: vec![],
        allow_custom: false,
        source: entry
            .map(|entry| entry.status.as_str().to_owned())
            .unwrap_or_else(|| "disabled".into()),
        available,
        error: (!available).then(|| {
            entry
                .and_then(|entry| entry.block_reason.clone())
                .unwrap_or_else(|| NATIVE_RUNTIME_UNAVAILABLE.into())
        }),
    }
}

fn discover_for(provider: AgentProvider) -> ProviderModels {
    let result = match provider {
        AgentProvider::ClaudeCode => discover_claude(),
        AgentProvider::Cursor => discover_cursor(),
        AgentProvider::Codex => discover_codex(),
        AgentProvider::Opencode => discover_opencode(),
        AgentProvider::GithubCopilot => {
            discover_static_cli(AgentProvider::GithubCopilot, "copilot")
        }
        AgentProvider::Gemini => discover_static_cli(AgentProvider::Gemini, "gemini"),
        AgentProvider::Grok => discover_grok(),
        AgentProvider::Pi => discover_pi(),
        AgentProvider::Omp => discover_omp(),
    };

    match result {
        Ok(mut catalog) => {
            catalog.models = pair_fast_variants(catalog.models);
            catalog.provider = provider.as_str().into();
            catalog.allow_custom = true;
            if catalog.source.is_empty() {
                catalog.source = "discovered".into();
            }
            catalog.available = true;
            if catalog.default_model.is_empty() {
                catalog.default_model = default_model(provider).into();
            }
            if catalog.models.is_empty() {
                let mut fb = fallback_models(provider);
                fb.error = Some("Agent returned no models".into());
                fb
            } else {
                catalog
            }
        }
        Err(err) => {
            let mut fb = fallback_models(provider);
            fb.error = Some(err);
            fb
        }
    }
}

fn discover_static_cli(provider: AgentProvider, command: &str) -> Result<ProviderModels, String> {
    find_bin(command).ok_or_else(|| format!("{command} CLI not found on PATH"))?;
    let fallback = fallback_models(provider);
    Ok(ProviderModels {
        provider: String::new(),
        capabilities: cli_capabilities(),
        default_model: fallback.default_model,
        models: fallback.models,
        allow_custom: true,
        source: "fallback".into(),
        available: true,
        error: None,
    })
}

fn discover_claude() -> Result<ProviderModels, String> {
    let bin = find_bin("claude").ok_or_else(|| "claude CLI not found on PATH".to_string())?;

    let models = match claude_models_from_handshake(&bin) {
        Ok(models) if !models.is_empty() => models,
        _ => return discover_static_cli(AgentProvider::ClaudeCode, "claude"),
    };
    let default = default_model(AgentProvider::ClaudeCode);
    let default = if models.iter().any(|model| model.id == default) {
        default.to_owned()
    } else {
        models[0].id.clone()
    };

    Ok(ProviderModels {
        provider: String::new(),
        capabilities: cli_capabilities(),
        default_model: default,
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    })
}

/// Read the live model list from the `initialize` handshake the Agent SDK uses.
/// No prompt is sent. Hooks, MCP servers and non-essential traffic are off, so
/// the probe makes no API call and stays out of the macOS OAuth refresh race a
/// real headless session can join.
fn claude_models_from_handshake(bin: &Path) -> Result<Vec<ModelOption>, String> {
    let mut child = Command::new(bin)
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--settings",
            r#"{"disableAllHooks":true}"#,
        ])
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("ENABLE_CLAUDEAI_MCP_SERVERS", "false")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

    let mut stdin = child.stdin.take().ok_or("claude stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("claude stdout unavailable")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(models) = parse_claude_handshake_line(&line) {
                let _ = sender.send(models);
                return;
            }
        }
    });

    let request = r#"{"type":"control_request","request_id":"alfred-models","request":{"subtype":"initialize"}}"#;
    let result = writeln!(stdin, "{request}")
        .map_err(|e| e.to_string())
        .and_then(|()| {
            receiver
                .recv_timeout(Duration::from_secs(10))
                .map_err(|e| format!("claude handshake failed: {e}"))
        });

    // Closing stdin ends the CLI on its own; only force it if that does not work.
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(2);
    while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn parse_claude_handshake_line(line: &str) -> Option<Vec<ModelOption>> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "control_response" {
        return None;
    }
    let list = value.pointer("/response/response/models")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|item| {
                let id = item.get("value")?.as_str()?.trim();
                // `sonnet` / `opus` cover it, and Alfred already has its own default.
                if id == "default" || !super::valid_model_id(id) {
                    return None;
                }
                let name = item
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id);
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // Some CLI versions write "Opus" + "Opus 5.5 · Best for…"; show the version.
                Some(match description.split_once(" · ") {
                    Some((versioned, rest)) if versioned.starts_with(name) => {
                        opt(id, versioned, rest)
                    }
                    _ => opt(id, name, description),
                })
            })
            .collect(),
    )
}

fn discover_opencode() -> Result<ProviderModels, String> {
    let bin = find_bin("opencode").ok_or_else(|| "opencode CLI not found on PATH".to_string())?;
    let output = run_cmd(&bin, &["models"], Duration::from_secs(30))?;
    let mut models = Vec::new();

    for line in output.stdout.lines() {
        let id = line.trim();
        if id.is_empty() || id.starts_with('{') || id.starts_with('[') {
            continue;
        }
        // `opencode models` prints one `provider/model` per line.
        if !id.contains('/') {
            continue;
        }
        models.push(opt(id, id, "Discovered via `opencode models`"));
    }

    if models.is_empty() {
        return Err(format!(
            "no models from opencode (stderr: {})",
            output.stderr.trim()
        ));
    }

    Ok(cli_default_catalog(models))
}

/// `grok models` is documented without an output format, so accept only rows
/// that are clearly model ids. Anything else leaves the CLI default in charge.
fn discover_grok() -> Result<ProviderModels, String> {
    let bin = find_bin("grok").ok_or_else(|| "grok CLI not found on PATH".to_string())?;
    let models = run_cmd(&bin, &["models"], Duration::from_secs(20))
        .map(|output| parse_grok_models(&output.stdout))
        .unwrap_or_default();
    if models.is_empty() {
        return discover_static_cli(AgentProvider::Grok, "grok");
    }
    Ok(cli_default_catalog(models))
}

fn parse_grok_models(stdout: &str) -> Vec<ModelOption> {
    parse_cursor_cli_models(stdout)
        .into_iter()
        .filter(|model| model.id.starts_with("grok-"))
        .map(|model| opt(&model.id, &model.id, "Discovered via `grok models`"))
        .collect()
}

/// `pi --list-models` prints a padded table whose first row is the header and
/// whose first two columns are `provider` and `model`. pi accepts the pair as
/// `provider/id`, which stays unambiguous when two providers share a model.
fn discover_pi() -> Result<ProviderModels, String> {
    let bin = find_bin("pi").ok_or_else(|| "pi CLI not found on PATH".to_string())?;
    let output = run_cmd(&bin, &["--list-models"], Duration::from_secs(30))?;
    let models = parse_pi_model_table(&output.stdout);

    if models.is_empty() {
        return Err(format!(
            "no models from pi (stderr: {})",
            output.stderr.trim()
        ));
    }

    Ok(cli_default_catalog(models))
}

fn parse_pi_model_table(stdout: &str) -> Vec<ModelOption> {
    let mut models = Vec::new();
    for line in stdout.lines() {
        let mut columns = line.split_whitespace();
        let (Some(provider), Some(id)) = (columns.next(), columns.next()) else {
            continue;
        };
        if provider == "provider" || id == "model" {
            continue;
        }
        let selector = format!("{provider}/{id}");
        models.push(opt(
            &selector,
            &selector,
            "Discovered via `pi --list-models`",
        ));
    }
    models
}

/// `omp models --json` returns `{ "models": [{ selector, name, ... }] }`.
fn discover_omp() -> Result<ProviderModels, String> {
    let bin = find_bin("omp").ok_or_else(|| "omp CLI not found on PATH".to_string())?;
    let output = run_cmd(&bin, &["models", "--json"], Duration::from_secs(30))?;
    let models = parse_omp_model_json(&output.stdout)?;

    if models.is_empty() {
        return Err(format!(
            "no models from omp (stderr: {})",
            output.stderr.trim()
        ));
    }

    Ok(cli_default_catalog(models))
}

fn parse_omp_model_json(stdout: &str) -> Result<Vec<ModelOption>, String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("invalid `omp models --json`: {e}"))?;
    let list = value
        .get("models")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "`omp models --json` missing models[]".to_string())?;

    let mut models = Vec::new();
    for item in list {
        let Some(selector) = item.get("selector").and_then(|v| v.as_str()) else {
            continue;
        };
        let label = item
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(selector);
        models.push(opt(
            selector,
            format!("{label} ({selector})"),
            "Discovered via `omp models`",
        ));
    }
    Ok(models)
}

fn cli_default_model() -> ModelOption {
    opt(
        pi::CLI_DEFAULT_MODEL,
        "CLI default",
        "Model configured in the CLI",
    )
}

/// Keep the "CLI default" entry first so a workflow can defer to whatever the
/// user configured instead of pinning a model that may later be retired.
fn cli_default_catalog(discovered: Vec<ModelOption>) -> ProviderModels {
    let mut models = vec![cli_default_model()];
    models.extend(discovered);

    ProviderModels {
        provider: String::new(),
        capabilities: cli_capabilities(),
        default_model: pi::CLI_DEFAULT_MODEL.into(),
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    }
}

fn discover_codex() -> Result<ProviderModels, String> {
    if let Some(models) = find_bin("codex")
        .and_then(|bin| run_cmd(&bin, &["debug", "models"], Duration::from_secs(15)).ok())
        .and_then(|output| parse_codex_models(&output.stdout).ok())
        .filter(|models| !models.is_empty())
    {
        return Ok(cli_default_catalog(models));
    }

    let home = BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .ok_or_else(|| "could not resolve home directory".to_string())?;
    let cache = home.join(".codex/models_cache.json");
    if !cache.is_file() {
        return Err(format!(
            "Codex model cache not found at {}",
            cache.display()
        ));
    }

    let raw = std::fs::read_to_string(&cache).map_err(|e| e.to_string())?;
    let models = parse_codex_models(&raw)?;
    if models.is_empty() {
        return Err("Codex models_cache.json contained no models".into());
    }
    Ok(cli_default_catalog(models))
}

/// Same shape from `codex debug models` and `models_cache.json`. `visibility`
/// is what Codex's own picker uses to hide internal models.
fn parse_codex_models(raw: &str) -> Result<Vec<ModelOption>, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid Codex model catalog: {e}"))?;
    let list = value
        .get("models")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Codex model catalog missing models[]".to_string())?;

    let mut models = Vec::new();
    for item in list {
        if item.get("visibility").and_then(|v| v.as_str()) == Some("hide") {
            continue;
        }
        let id = item
            .get("slug")
            .or_else(|| item.get("id"))
            .or_else(|| item.get("model"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if id.is_empty() {
            continue;
        }
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("From the Codex model catalog")
            .to_string();
        let label = item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();
        models.push(opt(id, label, description));
    }
    Ok(models)
}

fn cursor_auto_model() -> ModelOption {
    opt(pi::CLI_DEFAULT_MODEL, "Auto", "Cursor Auto")
}

fn discover_cursor() -> Result<ProviderModels, String> {
    let mut errors = Vec::new();

    // Cursor IDE carries curated labels and parameterized variants; use the
    // CLI only when the IDE catalog is unavailable.
    match discover_cursor_from_ide_state() {
        Ok(catalog) if !catalog.models.is_empty() => return Ok(catalog),
        Ok(_) => errors.push("Cursor IDE state had no agent models".into()),
        Err(e) => errors.push(e),
    }

    match discover_cursor_from_cli() {
        Ok(catalog) if !catalog.models.is_empty() => return Ok(catalog),
        Ok(_) => errors.push("CLI returned no models".into()),
        Err(e) => errors.push(e),
    }

    Err(format!(
        "could not discover Cursor models ({})",
        errors.join("; ")
    ))
}

fn discover_cursor_from_cli() -> Result<ProviderModels, String> {
    let bin = find_bin("cursor-agent")
        .or_else(|| find_bin("agent"))
        .ok_or_else(|| "cursor-agent / agent CLI not found on PATH".to_string())?;

    let attempts: &[&[&str]] = &[&["models"], &["--list-models"], &["model", "list"]];
    let mut last_err = String::new();

    for args in attempts {
        match run_cmd(&bin, args, Duration::from_secs(20)) {
            Ok(output) => {
                let text = format!("{}\n{}", output.stdout, output.stderr);
                // Old CLIs open an interactive login UI instead of listing models.
                if text.contains("Press any key to sign in")
                    || text.contains("not in the list of known options")
                    || text.contains("version is too old")
                {
                    last_err = format!(
                        "`{} {}` unsupported on this install",
                        bin.display(),
                        args.join(" ")
                    );
                    continue;
                }
                let models = parse_cursor_cli_models(&text);
                if models.is_empty() {
                    last_err = format!(
                        "`{} {}` produced no model ids",
                        bin.display(),
                        args.join(" ")
                    );
                    continue;
                }
                return Ok(cursor_catalog_from_models(
                    models,
                    "Discovered via cursor-agent CLI",
                ));
            }
            Err(e) => last_err = e,
        }
    }

    Err(last_err)
}

fn parse_cursor_cli_models(text: &str) -> Vec<ModelOption> {
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Common formats:
        // - `grok-4.5`
        // - `grok-4.5  Cursor Grok 4.5`
        // - `- grok-4.5 (Cursor Grok 4.5)`
        let cleaned = line.trim_start_matches(['-', '*', '•', '·']).trim();
        let mut parts = cleaned.split_whitespace();
        let Some(raw_id) = parts.next() else {
            continue;
        };
        let id = raw_id
            .trim_matches(|c: char| {
                c == '`' || c == ',' || c == '(' || c == ')' || c == '"' || c == '\''
            })
            .to_string();
        if !looks_like_cursor_model_id(&id) || !seen.insert(id.clone()) {
            continue;
        }
        let rest = parts.collect::<Vec<_>>().join(" ");
        let label = strip_html_tags(
            rest.trim_matches(|c: char| c == '(' || c == ')' || c == '-' || c == ':')
                .trim(),
        );
        let label = if label.is_empty() { id.clone() } else { label };
        models.push(opt(id, label, "From cursor-agent models"));
    }

    models
}

fn cursor_variant_is_fast(variant: &serde_json::Value, id: &str) -> bool {
    if fast_base_id(id).is_some() {
        return true;
    }
    variant
        .get("parameterValues")
        .and_then(|values| values.as_array())
        .is_some_and(|values| {
            values.iter().any(|parameter| {
                parameter.get("id").and_then(|value| value.as_str()) == Some("fast")
                    && (parameter.get("value").and_then(|value| value.as_bool()) == Some(true)
                        || parameter.get("value").and_then(|value| value.as_str()) == Some("true"))
            })
        })
}

fn cursor_variant_id<'a>(variant: &'a serde_json::Value, fallback: &'a str) -> &'a str {
    variant
        .get("legacySlug")
        .and_then(|value| value.as_str())
        .or_else(|| {
            variant
                .get("variantStringRepresentation")
                .and_then(|value| value.as_str())
        })
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(fallback)
}

fn cursor_variant_pair_key(variant: &serde_json::Value) -> Option<String> {
    let values = variant.get("parameterValues")?.as_array()?;
    let mut parameters = values
        .iter()
        .filter_map(|parameter| {
            let id = parameter.get("id").and_then(|value| value.as_str())?;
            if id == "fast" {
                return None;
            }
            let value = parameter.get("value")?;
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            Some(format!("{id}={value}"))
        })
        .collect::<Vec<_>>();
    parameters.sort_unstable();
    Some(parameters.join(","))
}

fn cursor_variant_counterpart_id(
    variant: &serde_json::Value,
    variant_id: &str,
    variants: &[serde_json::Value],
) -> Option<String> {
    let key = cursor_variant_pair_key(variant)?;
    let mut counterpart_id = None;
    for candidate in variants {
        let candidate_id = cursor_variant_id(candidate, "");
        if candidate_id.is_empty()
            || candidate_id == variant_id
            || cursor_variant_is_fast(candidate, candidate_id)
        {
            continue;
        }
        let Some(candidate_key) = cursor_variant_pair_key(candidate) else {
            continue;
        };
        if candidate_key != key {
            continue;
        }
        if counterpart_id.is_some() {
            return None;
        }
        counterpart_id = Some(candidate_id.to_string());
    }
    counterpart_id
}

fn looks_like_cursor_model_id(id: &str) -> bool {
    if id.len() < 3 || id.len() > 80 || id.contains('/') || id.contains(' ') {
        return false;
    }
    let lower = id.to_ascii_lowercase();
    // Reject help noise.
    if matches!(
        lower.as_str(),
        "model" | "models" | "available" | "name" | "id" | "usage" | "error" | "warning"
    ) {
        return false;
    }
    lower.contains('-')
        || lower.starts_with("gpt")
        || lower.starts_with("claude")
        || lower.starts_with("gemini")
        || lower.starts_with("composer")
        || lower.starts_with("grok")
        || lower.starts_with("kimi")
        || lower.starts_with("glm")
        || lower == "default"
        || lower == "auto"
}

fn discover_cursor_from_ide_state() -> Result<ProviderModels, String> {
    let db_path = cursor_state_db_path()
        .ok_or_else(|| "Cursor IDE state.vscdb not found (is Cursor installed?)".to_string())?;

    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("failed to open {}: {e}", db_path.display()))?;

    let key = "src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser";
    let raw: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .map_err(|e| format!("Cursor model catalog missing in state.vscdb: {e}"))?;

    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid Cursor state JSON: {e}"))?;

    let list = value
        .get("availableDefaultModels2")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "availableDefaultModels2 missing in Cursor state".to_string())?;

    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for item in list {
        // Prefer models that work in Agent mode.
        if let Some(supports) = item.get("supportsAgent") {
            if supports.as_bool() == Some(false) {
                continue;
            }
        }

        let base_id = item
            .get("name")
            .or_else(|| item.get("serverModelName"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if base_id.is_empty() {
            continue;
        }

        let base_label = item
            .get("clientDisplayName")
            .or_else(|| item.get("inputboxShortModelName"))
            .and_then(|v| v.as_str())
            .map(strip_html_tags)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| base_id.to_string());

        let vendor = item
            .get("vendor")
            .and_then(|v| v.get("displayName"))
            .or_else(|| item.get("vendorName"))
            .and_then(|v| v.as_str())
            .unwrap_or("Cursor IDE");

        let default_on = item
            .get("defaultOn")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let description = if default_on {
            format!("{vendor} · shown by default")
        } else {
            format!("{vendor} · from Cursor Settings → Models")
        };

        let mut pushed_variant = false;
        if let Some(variants) = item.get("variants").and_then(|v| v.as_array()) {
            for variant in variants {
                let variant_id = cursor_variant_id(variant, base_id);
                let is_fast = cursor_variant_is_fast(variant, variant_id);
                if !seen.insert(variant_id.to_string()) {
                    continue;
                }
                let variant_label = variant
                    .get("displayNameOutsidePicker")
                    .or_else(|| variant.get("displayName"))
                    .and_then(|v| v.as_str())
                    .map(strip_html_tags)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| base_label.clone());
                let mut option = opt(variant_id, variant_label, description.clone());
                if is_fast {
                    option.is_fast_variant = Some(true);
                    option.base_id = fast_base_id(variant_id)
                        .map(str::to_owned)
                        .or_else(|| cursor_variant_counterpart_id(variant, variant_id, variants));
                }
                models.push(option);
                pushed_variant = true;
            }
        }
        if !pushed_variant && seen.insert(base_id.to_string()) {
            models.push(opt(base_id, base_label, description));
        }
    }

    if models.is_empty() {
        return Err("no agent-capable models in Cursor IDE catalog".into());
    }

    // Keep IDE order (already curated), but float defaultOn models first.
    models.sort_by_key(|m| {
        let default_first = !m.description.contains("shown by default");
        (default_first, m.label.to_ascii_lowercase())
    });

    let default = value
        .pointer("/aiSettings/composerModel")
        .and_then(|v| v.as_str())
        .and_then(|selected| {
            models
                .iter()
                .find(|m| m.id == selected)
                .or_else(|| {
                    // IDE stores base ids like `grok-4.5`, while the picker can
                    // expose variant slugs like `cursor-grok-4.5-high-fast`.
                    models
                        .iter()
                        .find(|m| m.id.contains(selected) && !m.id.ends_with("-fast"))
                })
                .or_else(|| {
                    models
                        .iter()
                        .find(|m| m.id.contains(selected) && m.id.ends_with("-fast"))
                })
                .map(|m| m.id.clone())
        })
        .or_else(|| {
            models
                .iter()
                .find(|m| m.id == pi::CLI_DEFAULT_MODEL || m.id == "auto")
                .map(|m| m.id.clone())
        })
        .unwrap_or_else(|| models[0].id.clone());

    Ok(ProviderModels {
        provider: String::new(),
        capabilities: cli_capabilities(),
        default_model: default,
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    })
}

fn cursor_catalog_from_models(mut models: Vec<ModelOption>, _source: &str) -> ProviderModels {
    let default = match models
        .iter()
        .find(|m| m.id == pi::CLI_DEFAULT_MODEL || m.id == "auto")
    {
        Some(model) => model.id.clone(),
        None => {
            models.insert(0, cursor_auto_model());
            pi::CLI_DEFAULT_MODEL.to_owned()
        }
    };

    ProviderModels {
        provider: String::new(),
        capabilities: cli_capabilities(),
        default_model: default,
        models,
        allow_custom: true,
        source: String::new(),
        available: true,
        error: None,
    }
}

fn cursor_state_db_path() -> Option<PathBuf> {
    let home = BaseDirs::new()?.home_dir().to_path_buf();
    let candidates = [
        home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb"),
        home.join(".config/Cursor/User/globalStorage/state.vscdb"),
        home.join("AppData/Roaming/Cursor/User/globalStorage/state.vscdb"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '\u{200b}' => {}
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct CmdOutput {
    stdout: String,
    stderr: String,
}

fn run_cmd(bin: &Path, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

    // Drain while waiting: a catalog larger than the pipe buffer (`codex debug
    // models` prints ~460 KB) would otherwise block the CLI until the timeout.
    let stdout = drain_in_background(child.stdout.take());
    let stderr = drain_in_background(child.stderr.take());

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} timed out after {}s",
                        bin.display(),
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait failed for {}: {e}", bin.display())),
        }
    }

    Ok(CmdOutput {
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn drain_in_background<R: Read + Send + 'static>(
    pipe: Option<R>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn find_bin(name: &str) -> Option<PathBuf> {
    // 1) PATH
    if let Ok(output) = Command::new("which").arg(name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let p = PathBuf::from(path);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    // 2) Common install locations (Tauri apps often lack shell PATH).
    let mut candidates = Vec::new();
    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        candidates.push(home.join(format!(".local/bin/{name}")));
        candidates.push(home.join(format!(".opencode/bin/{name}")));
        candidates.push(home.join(format!(".cargo/bin/{name}")));
        candidates.push(home.join(format!("bin/{name}")));
    }
    candidates.push(PathBuf::from(format!("/opt/homebrew/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/local/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/bin/{name}")));

    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod pi_family_tests {
    use super::*;

    #[test]
    fn omitted_harness_returns_one_cli_catalog_per_provider() {
        let catalogs = list_provider_models(None);
        assert_eq!(catalogs.len(), 9);
        assert!(catalogs
            .iter()
            .all(|catalog| catalog.capabilities.harness == AgentHarness::Cli));
        let providers = catalogs
            .iter()
            .map(|catalog| catalog.provider.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(providers.len(), catalogs.len());
    }

    #[test]
    fn model_validation_is_harness_scoped_and_alfred_has_no_cli_default() {
        assert_eq!(
            resolve_model_for_execution(AgentProvider::Codex, AgentHarness::Alfred, None).unwrap(),
            None
        );
        assert!(matches!(
            resolve_model_for_execution(
                AgentProvider::Codex,
                AgentHarness::Alfred,
                Some("gpt-5.6-luna")
            ),
            Err(AgentError::InvalidModel)
        ));

        let mut strict = fallback_models(AgentProvider::Codex);
        strict.allow_custom = false;
        assert!(validate_model_in_catalog(&strict, pi::CLI_DEFAULT_MODEL).is_ok());
        assert!(matches!(
            validate_model_in_catalog(&strict, "sonnet"),
            Err(AgentError::InvalidModel)
        ));
    }

    #[test]
    fn native_catalogs_are_explicit_unavailable_and_secret_free() {
        let manifest = crate::agents::capability_manifest::manifest();
        let catalogs = native_unavailable_models(&manifest);
        assert_eq!(catalogs.len(), 9);
        assert!(catalogs.iter().all(|catalog| {
            let provider = AgentProvider::from_str(&catalog.provider).expect("known provider");
            let expected_error = manifest
                .entry(provider, AgentHarness::Alfred)
                .and_then(|entry| entry.block_reason.as_deref())
                .unwrap_or(NATIVE_RUNTIME_UNAVAILABLE);
            catalog.capabilities.harness == AgentHarness::Alfred
                && !catalog.available
                && !catalog.capabilities.account_connected
                && catalog.capabilities.requires_account
                && catalog.error.as_deref() == Some(expected_error)
        }));
        let json = serde_json::to_string(&catalogs).unwrap();
        assert!(json.contains(r#""harness":"alfred""#));
        assert!(json.contains(r#""supportsOAuth":false"#));
        for forbidden in ["accessToken", "refreshToken", "apiKey", "credential"] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn claude_handshake_reads_real_rows_and_skips_noise() {
        assert!(parse_claude_handshake_line(r#"{"type":"system"}"#).is_none());
        assert!(parse_claude_handshake_line("not json").is_none());

        let line = serde_json::json!({
            "type": "control_response",
            "response": {"subtype": "success", "response": {"models": [
                {"value": "default", "displayName": "Default (recommended)", "description": "Opus 5.5 · Best"},
                {"value": "opus", "displayName": "Opus", "description": "Opus 5.5 · Best for everyday"},
                {"value": "sonnet", "displayName": "Sonnet 5.5", "description": "Most efficient"},
                {"value": "claude-fable-5-1[1m]", "displayName": "Fable", "description": "Fable 5.1 · Hardest tasks"},
                {"value": "", "displayName": "Empty"},
                {"displayName": "No value"}
            ]}}
        })
        .to_string();
        let models = parse_claude_handshake_line(&line).expect("control response");
        let rows: Vec<_> = models
            .iter()
            .map(|m| (m.id.as_str(), m.label.as_str(), m.description.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("opus", "Opus 5.5", "Best for everyday"),
                ("sonnet", "Sonnet 5.5", "Most efficient"),
                ("claude-fable-5-1[1m]", "Fable 5.1", "Hardest tasks"),
            ]
        );
    }

    #[test]
    fn codex_catalog_hides_internal_models() {
        let models = parse_codex_models(
            r#"{"models":[
                {"slug":"gpt-new","display_name":"GPT New","visibility":"list"},
                {"slug":"internal","display_name":"Internal","visibility":"hide"},
                {"slug":"no-visibility"}
            ]}"#,
        )
        .expect("catalog");
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["gpt-new", "no-visibility"]);
        assert!(parse_codex_models("nope").is_err());
    }

    #[test]
    fn grok_parser_keeps_only_model_ids() {
        let stdout = "Available models:\n* grok-build  (default)\n- grok-4.5\ngrok-code-fast-1  2M\nError: please log in\ngrok models needs auth\n";
        let ids: Vec<_> = parse_grok_models(stdout)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec!["grok-build", "grok-4.5", "grok-code-fast-1"]);
    }

    #[test]
    fn cli_default_drops_the_model_flag_only() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(
            omit_cli_default_model(args(&["exec", "--model", "default", "go"])),
            args(&["exec", "go"])
        );
        assert_eq!(
            omit_cli_default_model(args(&["exec", "--model", "gpt-x", "default"])),
            args(&["exec", "--model", "gpt-x", "default"])
        );
    }

    #[test]
    fn fallbacks_never_pin_a_versioned_model() {
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
            let catalog = fallback_models(provider);
            assert!(catalog.models.iter().any(|m| m.id == catalog.default_model));
            for model in catalog.models {
                assert!(
                    !model.id.chars().any(|c| c.is_ascii_digit()),
                    "{} pins {}",
                    provider.as_str(),
                    model.id
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn run_cmd_survives_output_larger_than_the_pipe_buffer() {
        let output = run_cmd(
            Path::new("/bin/sh"),
            &["-c", "head -c 500000 /dev/zero | tr '\\0' 'x'"],
            Duration::from_secs(10),
        )
        .expect("large output must not time out");
        assert_eq!(output.stdout.len(), 500_000);
    }

    #[test]
    fn cli_runs_do_not_need_a_catalog_probe() {
        assert_eq!(
            resolve_model_for_execution(AgentProvider::Codex, AgentHarness::Cli, None).unwrap(),
            Some(pi::CLI_DEFAULT_MODEL.to_owned())
        );
    }

    #[test]
    fn parses_the_pi_model_table_and_skips_its_header() {
        let stdout = [
            "provider   model              context  max-out  thinking  images",
            "anthropic  claude-sonnet-5    1M       128K     yes       yes",
            "openai     gpt-5.6-luna       272K     128K     yes       yes",
        ]
        .join("\n");

        let models = parse_pi_model_table(&stdout);
        let ids = models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["anthropic/claude-sonnet-5", "openai/gpt-5.6-luna"]
        );
    }

    #[test]
    fn parses_omp_model_json_by_selector() {
        let stdout = r#"{"models":[
            {"provider":"anthropic","id":"claude-opus-5","selector":"anthropic/claude-opus-5","name":"Claude Opus 5"},
            {"provider":"openai","id":"gpt-5.2","selector":"openai/gpt-5.2"},
            {"provider":"broken","id":"no-selector"}
        ]}"#;

        let models = parse_omp_model_json(stdout).expect("valid json");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "anthropic/claude-opus-5");
        assert_eq!(models[0].label, "Claude Opus 5 (anthropic/claude-opus-5)");
        // Missing `name` falls back to the selector on both sides.
        assert_eq!(models[1].label, "openai/gpt-5.2 (openai/gpt-5.2)");
        assert!(parse_omp_model_json("not json").is_err());
    }

    #[test]
    fn catalogs_offer_the_cli_default_first() {
        let catalog = cli_default_catalog(vec![opt("anthropic/x", "X", "")]);
        assert_eq!(catalog.default_model, pi::CLI_DEFAULT_MODEL);
        assert_eq!(catalog.models[0].id, pi::CLI_DEFAULT_MODEL);
        assert!(catalog.allow_custom);
    }
    #[test]
    fn pairs_one_suffix_variant_and_removes_the_duplicate_row() {
        let models = pair_fast_variants(vec![
            test_model("gpt-5.3-codex-high"),
            test_model("gpt-5.3-codex-high-fast"),
            test_model("gpt-5.2"),
        ]);

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.3-codex-high", "gpt-5.2"]
        );
        assert_eq!(models[0].base_id.as_deref(), Some("gpt-5.3-codex-high"));
        assert_eq!(
            models[0].fast_variant_id.as_deref(),
            Some("gpt-5.3-codex-high-fast")
        );
        assert_eq!(models[0].supports_fast_toggle, Some(true));
    }

    #[test]
    fn keeps_ambiguous_pairs_flat() {
        let mut fast_low = test_model("cursor-grok-low-priority");
        fast_low.is_fast_variant = Some(true);
        fast_low.base_id = Some("cursor-grok".into());
        let mut fast_high = test_model("cursor-grok-high-priority");
        fast_high.is_fast_variant = Some(true);
        fast_high.base_id = Some("cursor-grok".into());

        let models = pair_fast_variants(vec![test_model("cursor-grok"), fast_low, fast_high]);

        assert_eq!(models.len(), 3);
        assert!(models
            .iter()
            .all(|model| model.supports_fast_toggle != Some(true)));
    }

    #[test]
    fn keeps_missing_half_pairs_flat() {
        let models = pair_fast_variants(vec![
            test_model("gpt-5.3-codex-fast"),
            test_model("gpt-5.2"),
        ]);

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.3-codex-fast", "gpt-5.2"]
        );
        assert!(models
            .iter()
            .all(|model| model.supports_fast_toggle != Some(true)));
    }

    #[test]
    fn pairs_explicit_mapper_metadata_without_a_fast_suffix() {
        let mut base = test_model("cursor-grok");
        base.fast_variant_id = Some("cursor-grok-priority".into());
        let mut fast = test_model("cursor-grok-priority");
        fast.is_fast_variant = Some(true);
        fast.base_id = Some("cursor-grok".into());

        let models = pair_fast_variants(vec![base, fast]);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "cursor-grok");
        assert_eq!(
            models[0].fast_variant_id.as_deref(),
            Some("cursor-grok-priority")
        );
        assert_eq!(models[0].supports_fast_toggle, Some(true));
    }
    #[test]
    fn detects_cursor_fast_parameter_values() {
        let fast = serde_json::json!({
            "parameterValues": [{"id": "fast", "value": "true"}]
        });
        let base = serde_json::json!({
            "parameterValues": [{"id": "fast", "value": "false"}]
        });

        assert!(cursor_variant_is_fast(&fast, "cursor-grok-high"));
        assert!(!cursor_variant_is_fast(&base, "cursor-grok-high"));
    }
    #[test]
    fn matches_parameterized_cursor_fast_variant_to_its_counterpart() {
        let base = serde_json::json!({
            "variantStringRepresentation": "grok-4.5[effort=high,fast=false]",
            "parameterValues": [
                {"id": "effort", "value": "high"},
                {"id": "fast", "value": "false"}
            ]
        });
        let fast = serde_json::json!({
            "variantStringRepresentation": "grok-4.5[effort=high,fast=true]",
            "parameterValues": [
                {"id": "effort", "value": "high"},
                {"id": "fast", "value": "true"}
            ]
        });
        let variants = vec![base.clone(), fast.clone()];

        assert_eq!(
            cursor_variant_counterpart_id(&fast, "grok-4.5[effort=high,fast=true]", &variants,)
                .as_deref(),
            Some("grok-4.5[effort=high,fast=false]")
        );
    }
}
