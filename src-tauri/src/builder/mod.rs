//! Workflow Builder: a chat with a local agent CLI that can build workflows.
//!
//! The agent reaches Alfred through a loopback MCP server (`mcp.rs`) whose
//! tools call the same database and runner paths as the UI commands. The user
//! adds their own rules (appended to the system prompt) and MCP servers
//! (merged into the agent's `--mcp-config`); both live in `builder.json`.

mod agent;
pub mod chats;
pub mod mcp;

use crate::agents::active::RunControl;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

pub const STREAM_EVENT: &str = "builder://stream";
pub const WORKFLOW_CHANGED_EVENT: &str = "builder://workflow-changed";
pub const CONFIG_CHANGED_EVENT: &str = "builder://config-changed";

const MAX_RULES: usize = 50;
const MAX_RULE_BYTES: usize = 8 * 1024;
const MAX_MCP_SERVERS: usize = 20;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_IMAGES: usize = 6;
/// Claude rejects images over 5 MB of base64, which is ~3.75 MB decoded.
pub const MAX_IMAGE_BYTES: usize = 3_750_000;
const MAX_DROPPED_IMAGE_BYTES: u64 = 40 * 1024 * 1024;
pub const SENDABLE_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

pub struct BuilderState {
    pub mcp_token: String,
    pub mcp_port: Mutex<Option<u16>>,
    turns: Mutex<HashMap<String, RunControl>>,
    config_lock: Mutex<()>,
}

impl Default for BuilderState {
    fn default() -> Self {
        use rand::Rng;
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(40)
            .map(char::from)
            .collect();
        Self {
            mcp_token: token,
            mcp_port: Mutex::new(None),
            turns: Mutex::new(HashMap::new()),
            config_lock: Mutex::new(()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuilderRule {
    pub id: String,
    pub name: String,
    pub body: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuilderMcpServer {
    pub id: String,
    pub name: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    /// `stdio` or `http`.
    pub transport: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuilderConfig {
    #[serde(default)]
    pub rules: Vec<BuilderRule>,
    #[serde(default)]
    pub mcp_servers: Vec<BuilderMcpServer>,
}

fn enabled_default() -> bool {
    true
}

fn data_dir() -> Result<PathBuf, String> {
    crate::db::app_data_dir().map_err(|e| e.to_string())
}

fn config_path() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("builder.json"))
}

/// Owner-only: MCP env vars and headers often carry tokens.
fn write_private(path: &PathBuf, contents: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn load_config() -> BuilderConfig {
    config_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn valid_server_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn validate_config(mut config: BuilderConfig) -> Result<BuilderConfig, String> {
    if config.rules.len() > MAX_RULES {
        return Err(format!("At most {MAX_RULES} rules."));
    }
    if config.mcp_servers.len() > MAX_MCP_SERVERS {
        return Err(format!("At most {MAX_MCP_SERVERS} MCP servers."));
    }
    for rule in &mut config.rules {
        rule.name = rule.name.trim().to_string();
        if rule.id.trim().is_empty() {
            rule.id = uuid::Uuid::new_v4().to_string();
        }
        if rule.name.is_empty() {
            return Err("Every rule needs a name.".into());
        }
        if rule.body.len() > MAX_RULE_BYTES {
            return Err(format!("Rule \"{}\" is longer than 8 KB.", rule.name));
        }
    }
    let mut names = std::collections::HashSet::new();
    for server in &mut config.mcp_servers {
        server.name = server.name.trim().to_string();
        if server.id.trim().is_empty() {
            server.id = uuid::Uuid::new_v4().to_string();
        }
        if !valid_server_name(&server.name) {
            return Err(format!(
                "MCP server name \"{}\" may only use letters, digits, - and _.",
                server.name
            ));
        }
        if server.name == "alfred" {
            return Err("\"alfred\" is reserved for Alfred's built-in API.".into());
        }
        if !names.insert(server.name.clone()) {
            return Err(format!("Two MCP servers are named \"{}\".", server.name));
        }
        match server.transport.as_str() {
            "stdio" if server.command.trim().is_empty() => {
                return Err(format!("MCP server \"{}\" needs a command.", server.name));
            }
            "http" if !(server.url.starts_with("http://") || server.url.starts_with("https://")) => {
                return Err(format!(
                    "MCP server \"{}\" needs an http(s) URL.",
                    server.name
                ));
            }
            "stdio" | "http" => {}
            _ => return Err("MCP transport must be stdio or http.".into()),
        }
    }
    Ok(config)
}

pub fn save_config(app: &AppHandle, config: BuilderConfig) -> Result<BuilderConfig, String> {
    let config = validate_config(config)?;
    let state = app.state::<BuilderState>();
    let _guard = state.config_lock.lock().unwrap_or_else(|e| e.into_inner());
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    write_private(&config_path()?, &text)?;
    let _ = app.emit(CONFIG_CHANGED_EVENT, &config);
    Ok(config)
}

/// Read-modify-write under the config lock, for MCP tools that append.
pub fn update_config(
    app: &AppHandle,
    change: impl FnOnce(&mut BuilderConfig),
) -> Result<BuilderConfig, String> {
    let mut config = {
        let state = app.state::<BuilderState>();
        let _guard = state.config_lock.lock().unwrap_or_else(|e| e.into_inner());
        load_config()
    };
    change(&mut config);
    save_config(app, config)
}

/// `--mcp-config` document: Alfred's API plus every enabled user server.
pub fn mcp_config_json(port: u16, token: &str, config: &BuilderConfig) -> Value {
    let mut servers = serde_json::Map::new();
    servers.insert(
        "alfred".into(),
        json!({
            "type": "http",
            "url": format!("http://127.0.0.1:{port}/mcp"),
            "headers": { "Authorization": format!("Bearer {token}") },
        }),
    );
    for server in config.mcp_servers.iter().filter(|s| s.enabled) {
        let entry = if server.transport == "http" {
            json!({ "type": "http", "url": server.url, "headers": server.headers })
        } else {
            json!({
                "type": "stdio",
                "command": server.command,
                "args": server.args,
                "env": server.env,
            })
        };
        servers.insert(server.name.clone(), entry);
    }
    json!({ "mcpServers": servers })
}

pub fn system_prompt(config: &BuilderConfig) -> String {
    let mut prompt = String::from(BASE_PROMPT);
    let rules: Vec<_> = config
        .rules
        .iter()
        .filter(|r| r.enabled && !r.body.trim().is_empty())
        .collect();
    if !rules.is_empty() {
        prompt.push_str("\n\n# User rules (always follow these)\n");
        for rule in rules {
            prompt.push_str(&format!("\n## {}\n{}\n", rule.name, rule.body.trim()));
        }
    }
    prompt
}

const BASE_PROMPT: &str = r#"You are Alfred's Workflow Builder, a chat agent inside the Alfred desktop app. Alfred composes multi-agent automations ("workflows") that run local AI CLIs. You design, create, edit, test and schedule workflows for the user by calling the tools of the `alfred` MCP server. Those tools ARE Alfred's internal API: whatever you create appears in the user's sidebar immediately.

# How to work
- Act. When the request is clear enough, build it right away. Ask one short question only when something critical is missing (a repo path, a Slack channel).
- Check what exists before you use it: list_agents for installed CLIs, list_skills for skills, list_connected_apps for app actions and connection ids. Never invent ids, skill names, or connections.
- To change an existing workflow: list_workflows → get_workflow → update_workflow with the COMPLETE new graph (it replaces the old one).
- After creating or changing a workflow, describe it in 2-4 short bullets (the steps in order). Offer to run it; call run_workflow only when the user asks to run or test it, then get_run with waitSeconds to report the outcome.
- The user can also add rules and MCP servers for you; use add_rule / add_mcp_server only when the user asks.
- Be brief. Use markdown.

# Workflow graph reference
A graph is `{ "nodes": [...], "edges": [...] }`. Steps run in dependency order along edges; each step sees the accumulated context and the previous step's output.

Node: `{ "id": "short-kebab-id", "type": <type>, "position": {"x": n, "y": n}, "data": {...} }`. Omit position to auto-layout (left→right). Missing data fields get defaults.
Edge: `{ "id": "e-a-b", "source": "a", "target": "b" }`. Optional gate: `"data": {"condition": {"source": "output"|"context"|"trigger", "operator": "contains"|"notContains"|"equals"|"notEquals"|"matches"|"isEmpty"|"isNotEmpty", "value": "text", "caseSensitive": false}}` — the target runs only when the condition holds; otherwise it is skipped.

Node types and their data:
- `input` — instruction/context for downstream steps: `{label, prompt}`. Most workflows start with one.
- `agent` — runs a local AI CLI on the context: `{label, provider, model?, skillNames?: []}`. provider ∈ claude_code | codex | cursor | opencode | github_copilot | gemini | grok | pi | omp (use list_agents; prefer installed ones). model null = provider default. skillNames from list_skills.
- `identity` — persona for the next agents: `{label, role, personality, instructions}`.
- `chooseOutput` — final result sink: `{label, saveToMemory: true, pinMemory: false, asFinalResult: true, htmlReport: false}`. End most workflows with one.
- `memory` — inject saved memories: `{label, memoryIds: []}`.
- `template` — rewrite/append context: `{label, template, mode: "append"|"replace"}` with placeholders {{context}}, {{output}}, {{cwd}}.
- `script` — run a script, stdout flows downstream: `{label, source: "inline", body, interpreter: "bash"|"python3"|"node", appendOutput: true}`.
- `http` — HTTP request, response appended: `{label, method, url, body, headers: "Name: value\n..."}`.
- `notify` — desktop notification / webhook: `{label, title, body: "{{output}}", desktop: true, webhookUrl: ""}`.
- `writeFile` — write a file: `{label, path, content: "{{output}}"}`.
- `fileInject` — add file/folder contents to context: `{label, paths: []}`.
- `gitStatus` — git status (+diff) of the working directory: `{label, includeDiff: true}`.
- `gitHost` — open a GitHub PR or issue via gh: `{label, action: "pr"|"issue", repo: "owner/name" or "", title, body: "{{output}}", base: "", draft: false}`.
- `appAction` — connected-app action: `{label, providerId, actionId, connectionId, input: {...}}` (ids from list_connected_apps).
- `customAgent` — any CLI: `{label, command: "my-agent \"{{prompt}}\"", promptMode: "template"|"stdin"}`.

Parallel branches: fan out from one node to several, then join them into one node (e.g. two agents debating → a judge agent). A workflow's `workingDirectory` is where agents and scripts run; set it when the work touches a repo or folder.
Schedules use 5-field cron (`0 9 * * 1-5` = weekdays 9:00) via set_schedule.
"#;

#[tauri::command]
pub fn builder_get_config() -> BuilderConfig {
    load_config()
}

#[tauri::command]
pub fn builder_save_config(app: AppHandle, config: BuilderConfig) -> Result<BuilderConfig, String> {
    save_config(&app, config)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderInfo {
    pub agents: Vec<Value>,
    pub mcp_ready: bool,
    pub tools: Vec<Value>,
}

#[tauri::command]
pub fn builder_info(state: State<'_, BuilderState>) -> BuilderInfo {
    BuilderInfo {
        agents: agent::BuilderAgent::ALL
            .iter()
            .map(|agent| {
                json!({
                    "id": agent.id(),
                    "label": agent.label(),
                    "installed": agent.binary().is_some(),
                })
            })
            .collect(),
        mcp_ready: state.mcp_port.lock().map(|p| p.is_some()).unwrap_or(false),
        tools: mcp::tool_definitions()
            .into_iter()
            .map(|tool| json!({ "name": tool["name"], "description": tool["description"] }))
            .collect(),
    }
}

/// One image for the agent, already sized by the page.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderImage {
    pub media_type: String,
    /// Base64, no data-URL prefix.
    pub data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderSendInput {
    pub chat_id: String,
    pub message: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub images: Vec<BuilderImage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderTurnResult {
    pub session_id: Option<String>,
    pub cancelled: bool,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamPayload<'a> {
    chat_id: &'a str,
    event: Value,
}

/// Session ids are passed to `--resume` / `exec resume`; accept only uuids.
fn valid_session_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 128
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '[' | ']' | '/'))
}

/// One chat turn: run the chosen agent CLI with Alfred's MCP API, stream
/// every event to the page in Claude's `stream-json` shape, and return the
/// session id for the next turn's resume.
#[tauri::command]
pub async fn builder_send(
    app: AppHandle,
    input: BuilderSendInput,
) -> Result<BuilderTurnResult, String> {
    if input.message.trim().is_empty() {
        return Err("Message is empty.".into());
    }
    if input.message.len() > MAX_MESSAGE_BYTES {
        return Err("Message is too long.".into());
    }
    validate_images(&input.images)?;
    tauri::async_runtime::spawn_blocking(move || run_turn(&app, input))
        .await
        .map_err(|e| e.to_string())?
}

fn run_turn(app: &AppHandle, input: BuilderSendInput) -> Result<BuilderTurnResult, String> {
    let agent = agent::BuilderAgent::from_id(input.agent.as_deref().unwrap_or("claude_code"))
        .ok_or("Unsupported agent.")?;
    let bin = agent.binary().ok_or_else(|| {
        format!(
            "{} CLI not found. Install it and make sure it is on PATH.",
            agent.label()
        )
    })?;
    if let Some(model) = input.model.as_deref().filter(|m| !m.trim().is_empty()) {
        if !valid_model(model.trim()) {
            return Err("Unsupported model id.".into());
        }
    }
    if let Some(session) = input.session_id.as_deref().filter(|s| !s.is_empty()) {
        if !valid_session_id(session) {
            return Err("Invalid session id.".into());
        }
    }
    let state = app.state::<BuilderState>();
    let port = state
        .mcp_port
        .lock()
        .ok()
        .and_then(|p| *p)
        .ok_or("Alfred's agent API is not running. Restart Alfred.")?;

    let control = RunControl::new();
    {
        let mut turns = state.turns.lock().unwrap_or_else(|e| e.into_inner());
        if turns.contains_key(&input.chat_id) {
            return Err("This chat is already working. Stop it first.".into());
        }
        turns.insert(input.chat_id.clone(), control.clone());
    }
    let result = agent::run(app, agent, &bin, port, &state.mcp_token, &control, &input);
    state
        .turns
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&input.chat_id);
    result
}

pub fn validate_images(images: &[BuilderImage]) -> Result<(), String> {
    use base64::Engine;
    if images.len() > MAX_IMAGES {
        return Err(format!("Attach at most {MAX_IMAGES} images per message."));
    }
    for image in images {
        if !SENDABLE_IMAGE_TYPES.contains(&image.media_type.as_str()) {
            return Err("Images must be PNG, JPEG, GIF or WebP.".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&image.data)
            .map_err(|_| "An attached image could not be read.".to_string())?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("An attached image is larger than 3.75 MB.".into());
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedImage {
    pub name: String,
    pub media_type: String,
    pub data: String,
}

/// Read an image the user dropped or picked, so the page can size it. Only
/// paths the user chose reach this, and only image extensions are read.
#[tauri::command]
pub async fn builder_read_image(path: String) -> Result<DroppedImage, String> {
    use base64::Engine;
    let path = PathBuf::from(path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let media_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heic",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => return Err("Only images can be attached.".into()),
    };
    let size = std::fs::metadata(&path).map_err(|_| "That file could not be read.".to_string())?.len();
    if size > MAX_DROPPED_IMAGE_BYTES {
        return Err("That image is larger than 40 MB.".into());
    }
    let bytes = tauri::async_runtime::spawn_blocking(move || std::fs::read(&path).map(|b| (b, path)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "That file could not be read.".to_string())?;
    let (bytes, path) = bytes;
    Ok(DroppedImage {
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image")
            .to_string(),
        media_type: media_type.into(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[tauri::command]
pub fn builder_cancel(state: State<'_, BuilderState>, chat_id: String) -> bool {
    let control = state
        .turns
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&chat_id)
        .cloned();
    match control {
        Some(control) => {
            control.request_cancel();
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_config_includes_alfred_and_enabled_servers_only() {
        let config = BuilderConfig {
            rules: vec![],
            mcp_servers: vec![
                BuilderMcpServer {
                    id: "1".into(),
                    name: "fs".into(),
                    enabled: true,
                    transport: "stdio".into(),
                    command: "npx".into(),
                    args: vec!["-y".into(), "server-fs".into()],
                    ..Default::default()
                },
                BuilderMcpServer {
                    id: "2".into(),
                    name: "off".into(),
                    enabled: false,
                    transport: "http".into(),
                    url: "https://x".into(),
                    ..Default::default()
                },
            ],
        };
        let doc = mcp_config_json(4242, "tok", &config);
        let servers = doc["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers["alfred"]["url"], "http://127.0.0.1:4242/mcp");
        assert_eq!(servers["alfred"]["headers"]["Authorization"], "Bearer tok");
        assert_eq!(servers["fs"]["command"], "npx");
    }

    #[test]
    fn validation_rejects_reserved_duplicate_and_incomplete_servers() {
        let server = |name: &str, transport: &str| BuilderMcpServer {
            name: name.into(),
            transport: transport.into(),
            command: "x".into(),
            url: "https://ok".into(),
            ..Default::default()
        };
        let with = |servers| BuilderConfig {
            rules: vec![],
            mcp_servers: servers,
        };
        assert!(validate_config(with(vec![server("alfred", "stdio")])).is_err());
        assert!(validate_config(with(vec![server("a", "stdio"), server("a", "http")])).is_err());
        assert!(validate_config(with(vec![server("bad name", "stdio")])).is_err());
        assert!(validate_config(with(vec![server("a", "ftp")])).is_err());
        let ok = validate_config(with(vec![server("a", "stdio")])).unwrap();
        assert!(!ok.mcp_servers[0].id.is_empty());
    }

    #[test]
    fn image_validation_bounds_count_type_and_size() {
        use base64::Engine;
        let image = |media_type: &str, len: usize| BuilderImage {
            media_type: media_type.into(),
            data: base64::engine::general_purpose::STANDARD.encode(vec![0u8; len]),
        };
        assert!(validate_images(&[image("image/png", 10)]).is_ok());
        assert!(validate_images(&[image("image/svg+xml", 10)]).is_err());
        assert!(validate_images(&[image("image/png", MAX_IMAGE_BYTES + 1)]).is_err());
        let many: Vec<_> = (0..=MAX_IMAGES).map(|_| image("image/png", 1)).collect();
        assert!(validate_images(&many).is_err());
        let bad = BuilderImage { media_type: "image/png".into(), data: "%%%".into() };
        assert!(validate_images(&[bad]).is_err());
    }

    #[test]
    fn system_prompt_appends_enabled_rules() {
        let config = BuilderConfig {
            rules: vec![
                BuilderRule {
                    id: "1".into(),
                    name: "Tone".into(),
                    body: "Always use codex".into(),
                    enabled: true,
                },
                BuilderRule {
                    id: "2".into(),
                    name: "Off".into(),
                    body: "never shown".into(),
                    enabled: false,
                },
            ],
            mcp_servers: vec![],
        };
        let prompt = system_prompt(&config);
        assert!(prompt.contains("## Tone\nAlways use codex"));
        assert!(!prompt.contains("never shown"));
    }
}
