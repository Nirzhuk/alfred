//! Runs one Workflow Builder turn on a local agent CLI.
//!
//! Every agent's output is translated into Claude Code's `stream-json` event
//! shape, so the page folds one event language no matter which CLI answered.

use super::{
    data_dir, load_config, mcp_config_json, system_prompt, write_private, BuilderConfig,
    BuilderSendInput, BuilderTurnResult, StreamPayload, STREAM_EVENT,
};
use crate::agents::active::RunControl;
use crate::agents::process::{find_bin, lock_claude_invocation, run_cmd_with_stdin_env};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const TURN_TIMEOUT: Duration = Duration::from_secs(60 * 30);
const TOKEN_ENV: &str = "ALFRED_MCP_TOKEN";
/// After its final result a CLI may linger shutting down MCP servers; the
/// chat should not stay locked on that.
const EXIT_GRACE: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderAgent {
    ClaudeCode,
    Codex,
}

impl BuilderAgent {
    pub const ALL: [BuilderAgent; 2] = [Self::ClaudeCode, Self::Codex];

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| agent.id() == id)
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    pub fn binary(self) -> Option<PathBuf> {
        find_bin(match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        })
    }
}

pub fn run(
    app: &AppHandle,
    agent: BuilderAgent,
    bin: &Path,
    port: u16,
    token: &str,
    control: &RunControl,
    input: &BuilderSendInput,
) -> Result<BuilderTurnResult, String> {
    let emit = |event: Value| {
        let _ = app.emit(
            STREAM_EVENT,
            StreamPayload {
                chat_id: &input.chat_id,
                event,
            },
        );
    };

    let config = load_config();
    let dir = data_dir()?;
    let workspace = dir.join("builder-workspace");
    std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
    let model = input.model.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let session = input.session_id.as_deref().filter(|s| !s.is_empty());

    let args = match agent {
        BuilderAgent::ClaudeCode => {
            let mcp_path = dir.join("builder-mcp.json");
            write_private(&mcp_path, &mcp_config_json(port, token, &config).to_string())?;
            claude_args(&input.message, &mcp_path, &system_prompt(&config), model, session)
        }
        BuilderAgent::Codex => codex_args(&input.message, port, &config, model, session),
    };

    // Claude processes share one Keychain item; wait for any workflow step
    // using Claude to finish rather than racing its credential refresh.
    let _invocation = if agent == BuilderAgent::ClaudeCode {
        emit(json!({ "type": "alfred", "subtype": "queued" }));
        match lock_claude_invocation(Some(control)) {
            Ok(guard) => Some(guard),
            Err(_) if control.is_cancelled() => {
                emit(json!({ "type": "alfred", "subtype": "finished", "cancelled": true }));
                return Ok(BuilderTurnResult {
                    session_id: input.session_id.clone(),
                    cancelled: true,
                    error: None,
                });
            }
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    emit(json!({ "type": "alfred", "subtype": "started" }));

    let session_id = Mutex::new(input.session_id.clone());
    let result_error = Mutex::new(None::<String>);
    let codex = Mutex::new(CodexTranslator::default());
    let result_seen = AtomicBool::new(false);
    let exited = Arc::new(AtomicBool::new(false));
    let on_line = |line: &str| {
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let events = match agent {
            BuilderAgent::ClaudeCode => vec![raw],
            BuilderAgent::Codex => codex.lock().unwrap_or_else(|e| e.into_inner()).translate(&raw),
        };
        for event in events {
            match event.get("type").and_then(Value::as_str) {
                Some("system") if event.get("subtype").and_then(Value::as_str) == Some("init") => {
                    if let Some(id) = event.get("session_id").and_then(Value::as_str) {
                        *session_id.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
                    }
                }
                // Hook chatter from the user's own agent settings is noise here.
                Some("system")
                    if event
                        .get("subtype")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.starts_with("hook_")) =>
                {
                    continue;
                }
                Some("result") => {
                    if event.get("is_error").and_then(Value::as_bool) == Some(true) {
                        let text = event
                            .get("result")
                            .and_then(Value::as_str)
                            .unwrap_or("The agent stopped with an error.");
                        *result_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.to_string());
                    }
                    if !result_seen.swap(true, Ordering::SeqCst) {
                        let (control, exited) = (control.clone(), Arc::clone(&exited));
                        std::thread::spawn(move || {
                            std::thread::sleep(EXIT_GRACE);
                            if !exited.load(Ordering::SeqCst) {
                                control.request_cancel();
                            }
                        });
                    }
                }
                _ => {}
            }
            emit(event);
        }
    };

    let envs = [(TOKEN_ENV, token.to_string())];
    let output = run_cmd_with_stdin_env(
        bin,
        &args,
        Some(&workspace),
        TURN_TIMEOUT,
        Some(control),
        Some(&on_line),
        "",
        if agent == BuilderAgent::Codex { &envs } else { &[] },
    );
    exited.store(true, Ordering::SeqCst);
    // A turn that already reported its result finished, even if its lingering
    // process had to be ended.
    let finished = result_seen.load(Ordering::SeqCst);
    let cancelled = control.is_cancelled() && !finished;
    let session_id = session_id.into_inner().unwrap_or_else(|e| e.into_inner());
    let mut error = result_error.into_inner().unwrap_or_else(|e| e.into_inner());
    match output {
        _ if finished => {}
        Ok(output) if !output.success && !cancelled && error.is_none() => {
            let stderr = output
                .stderr
                .lines()
                .filter(|line| !line.contains(" WARN ") && !line.contains("rmcp::"))
                .collect::<Vec<_>>()
                .join("\n");
            let stderr = stderr.trim();
            error = Some(if stderr.is_empty() {
                format!("{} exited with an error.", agent.label())
            } else {
                stderr.chars().take(600).collect()
            });
        }
        Err(message) if !cancelled => error = Some(message),
        _ => {}
    }
    emit(json!({ "type": "alfred", "subtype": "finished", "cancelled": cancelled, "error": error }));
    Ok(BuilderTurnResult {
        session_id,
        cancelled,
        error,
    })
}

fn claude_args(
    message: &str,
    mcp_path: &Path,
    system: &str,
    model: Option<&str>,
    session: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-p".into(),
        message.into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--permission-mode".into(),
        "bypassPermissions".into(),
        "--max-turns".into(),
        "60".into(),
        "--mcp-config".into(),
        mcp_path.to_string_lossy().into_owned(),
        // Exactly the servers the Setup rail shows; global ones slow startup.
        "--strict-mcp-config".into(),
        "--append-system-prompt".into(),
        system.into(),
    ];
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(session) = session {
        args.extend(["--resume".into(), session.into()]);
    }
    args
}

/// A TOML basic string. JSON string escapes are a subset of TOML's.
fn toml_str(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn toml_table<'a>(pairs: impl Iterator<Item = (&'a String, &'a String)>) -> String {
    let body: Vec<String> = pairs
        .map(|(k, v)| format!("{} = {}", toml_str(k), toml_str(v)))
        .collect();
    format!("{{ {} }}", body.join(", "))
}

/// Codex takes the whole MCP table as one `-c` override, which also replaces
/// the user's global servers: the agent gets exactly what Setup shows.
pub fn codex_mcp_servers(port: u16, config: &BuilderConfig) -> String {
    let mut servers = vec![format!(
        "alfred = {{ url = {}, bearer_token_env_var = {} }}",
        toml_str(&format!("http://127.0.0.1:{port}/mcp")),
        toml_str(TOKEN_ENV)
    )];
    for server in config.mcp_servers.iter().filter(|s| s.enabled) {
        let entry = if server.transport == "http" {
            format!(
                "{{ url = {}, http_headers = {} }}",
                toml_str(&server.url),
                toml_table(server.headers.iter())
            )
        } else {
            let args: Vec<String> = server.args.iter().map(|a| toml_str(a)).collect();
            format!(
                "{{ command = {}, args = [{}], env = {} }}",
                toml_str(&server.command),
                args.join(", "),
                toml_table(server.env.iter())
            )
        };
        servers.push(format!("{} = {}", toml_str(&server.name), entry));
    }
    format!("mcp_servers={{ {} }}", servers.join(", "))
}

fn codex_args(
    message: &str,
    port: u16,
    config: &BuilderConfig,
    model: Option<&str>,
    session: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--dangerously-bypass-approvals-and-sandbox".into(),
        "-c".into(),
        codex_mcp_servers(port, config),
        // Replaces the user's developer_instructions for this turn only.
        "-c".into(),
        format!("developer_instructions={}", toml_str(&system_prompt(config))),
    ];
    if let Some(model) = model {
        args.extend(["-m".into(), model.into()]);
    }
    if let Some(session) = session {
        args.extend(["resume".into(), session.into()]);
    }
    args.push(message.into());
    args
}

/// Codex `exec --json` items → Claude `stream-json` events.
#[derive(Default)]
pub struct CodexTranslator {
    started: HashSet<String>,
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

impl CodexTranslator {
    fn tool_use(item: &Value) -> Option<Value> {
        let id = str_of(item, "id");
        let (name, input) = match str_of(item, "type") {
            "mcp_tool_call" => (
                format!("mcp__{}__{}", str_of(item, "server"), str_of(item, "tool")),
                item.get("arguments").cloned().unwrap_or_else(|| json!({})),
            ),
            "command_execution" => ("Bash".into(), json!({ "command": str_of(item, "command") })),
            "web_search" => ("WebSearch".into(), json!({ "query": str_of(item, "query") })),
            "file_change" => (
                "Edit".into(),
                json!({ "file_path": item.pointer("/changes/0/path").and_then(Value::as_str).unwrap_or("") }),
            ),
            "todo_list" => ("TodoWrite".into(), json!({})),
            _ => return None,
        };
        // Arguments may arrive as a JSON-encoded string.
        let input = match input {
            Value::String(text) => serde_json::from_str(&text).unwrap_or(json!({})),
            other => other,
        };
        Some(json!({
            "type": "assistant",
            "message": { "id": format!("codex-{id}"), "content": [
                { "type": "tool_use", "id": id, "name": name, "input": input }
            ] }
        }))
    }

    fn tool_result(item: &Value) -> Value {
        let failed = matches!(str_of(item, "status"), "failed" | "declined")
            || item.get("error").is_some_and(|e| !e.is_null());
        let content = match str_of(item, "type") {
            "mcp_tool_call" => item
                .pointer("/result/content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|text| !text.is_empty())
                .or_else(|| {
                    item.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default(),
            "command_execution" => str_of(item, "aggregated_output").to_string(),
            _ => String::new(),
        };
        json!({
            "type": "user",
            "message": { "content": [
                { "type": "tool_result", "tool_use_id": str_of(item, "id"), "content": content, "is_error": failed }
            ] }
        })
    }

    pub fn translate(&mut self, event: &Value) -> Vec<Value> {
        let item = event.get("item").unwrap_or(&Value::Null);
        let id = str_of(item, "id").to_string();
        match str_of(event, "type") {
            "thread.started" => vec![json!({
                "type": "system",
                "subtype": "init",
                "session_id": str_of(event, "thread_id"),
                "mcp_servers": [],
            })],
            "item.started" => match Self::tool_use(item) {
                Some(tool) if self.started.insert(id) => vec![tool],
                _ => vec![],
            },
            "item.completed" => match str_of(item, "type") {
                "agent_message" => vec![json!({
                    "type": "assistant",
                    "message": { "id": format!("codex-{id}"), "content": [{ "type": "text", "text": str_of(item, "text") }] }
                })],
                "reasoning" => vec![json!({
                    "type": "assistant",
                    "message": { "id": format!("codex-{id}"), "content": [{ "type": "thinking", "thinking": str_of(item, "text") }] }
                })],
                _ => match Self::tool_use(item) {
                    Some(tool) => {
                        let mut events = Vec::new();
                        if self.started.insert(id) {
                            events.push(tool);
                        }
                        events.push(Self::tool_result(item));
                        events
                    }
                    None => vec![],
                },
            },
            "turn.completed" => vec![json!({ "type": "result", "is_error": false })],
            "turn.failed" => vec![json!({
                "type": "result",
                "is_error": true,
                "result": event.pointer("/error/message").and_then(Value::as_str).unwrap_or("Codex stopped with an error."),
            })],
            "error" => vec![json!({
                "type": "result",
                "is_error": true,
                "result": str_of(event, "message"),
            })],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::BuilderMcpServer;

    #[test]
    fn translates_a_codex_turn_into_stream_json() {
        let mut t = CodexTranslator::default();
        let lines = [
            r#"{"type":"thread.started","thread_id":"01a142c3-b5f8-7820-8198-a91b40df32b6"}"#,
            r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Skill descriptions were shortened"}}"#,
            r#"{"type":"item.started","item":{"id":"item_1","type":"mcp_tool_call","server":"alfred","tool":"create_workflow","arguments":{"name":"Digest"},"status":"in_progress"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_1","type":"mcp_tool_call","server":"alfred","tool":"create_workflow","arguments":{"name":"Digest"},"result":{"content":[{"type":"text","text":"{\"id\":\"wf\"}"}]},"status":"completed"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_2","type":"command_execution","command":"ls","aggregated_output":"a\n","exit_code":0,"status":"completed"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"Done."}}"#,
            r#"{"type":"turn.completed","usage":{}}"#,
        ];
        let events: Vec<Value> = lines
            .iter()
            .flat_map(|l| t.translate(&serde_json::from_str(l).unwrap()))
            .collect();
        let kinds: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["system", "assistant", "user", "assistant", "user", "assistant", "result"]);
        assert_eq!(events[0]["session_id"], "01a142c3-b5f8-7820-8198-a91b40df32b6");
        assert_eq!(events[1]["message"]["content"][0]["name"], "mcp__alfred__create_workflow");
        assert_eq!(events[2]["message"]["content"][0]["content"], "{\"id\":\"wf\"}");
        assert_eq!(events[3]["message"]["content"][0]["name"], "Bash");
        assert_eq!(events[4]["message"]["content"][0]["content"], "a\n");
        assert_eq!(events[5]["message"]["content"][0]["text"], "Done.");
    }

    #[test]
    fn failed_turns_become_error_results() {
        let mut t = CodexTranslator::default();
        let event = t.translate(&json!({ "type": "turn.failed", "error": { "message": "usage limit" } }));
        assert_eq!(event[0]["is_error"], true);
        assert_eq!(event[0]["result"], "usage limit");
    }

    #[test]
    fn codex_mcp_table_is_strict_and_escaped() {
        let config = BuilderConfig {
            rules: vec![],
            mcp_servers: vec![BuilderMcpServer {
                id: "1".into(),
                name: "fs".into(),
                enabled: true,
                transport: "stdio".into(),
                command: "npx".into(),
                args: vec!["-y".into(), "a \"b\"".into()],
                env: [("TOKEN".to_string(), "x".to_string())].into(),
                ..Default::default()
            }],
        };
        let table = codex_mcp_servers(4242, &config);
        assert!(table.starts_with("mcp_servers={ alfred = { url = \"http://127.0.0.1:4242/mcp\""));
        assert!(table.contains("bearer_token_env_var = \"ALFRED_MCP_TOKEN\""));
        assert!(table.contains("\"fs\" = { command = \"npx\", args = [\"-y\", \"a \\\"b\\\"\"], env = { \"TOKEN\" = \"x\" } }"));
    }

    #[test]
    fn codex_resume_comes_after_exec_options() {
        let args = codex_args("hi", 1, &BuilderConfig::default(), Some("gpt-5"), Some("abc"));
        let resume = args.iter().position(|a| a == "resume").unwrap();
        assert!(args.iter().position(|a| a == "--json").unwrap() < resume);
        assert_eq!(args[resume + 1], "abc");
        assert_eq!(args.last().unwrap(), "hi");
    }
}
