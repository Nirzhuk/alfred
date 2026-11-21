//! `Alfred mcp <endpoint.json>`: stdio MCP server for outside agents (Claude
//! Code, Codex, Cursor...). Handshake and tool list are answered here, so the
//! agent connects even while the app is closed; tool calls are forwarded to the
//! running app's loopback server. The endpoint file (port + per-process token,
//! owner-only) is re-read per call, so restarting Alfred never breaks a client.

use super::mcp::{self, rpc_error, tool_text, GUIDE_TOOL};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const NOT_RUNNING: &str =
    "Alfred isn't running. Ask the user to open the Alfred app, then try again.";

pub fn run(endpoint: PathBuf) {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        // run_workflow + get_run may wait up to 300 s.
        .timeout(Duration::from_secs(330))
        .build()
        .expect("http client");
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let (client, endpoint, stdout) = (client.clone(), endpoint.clone(), stdout.clone());
        // A call can wait on a run; keep reading so pings and other calls flow.
        std::thread::spawn(move || {
            if let Some(reply) = reply(&line, &endpoint, &client) {
                let mut out = stdout.lock().unwrap_or_else(|e| e.into_inner());
                let _ = writeln!(out, "{reply}");
                let _ = out.flush();
            }
        });
    }
}

fn reply(line: &str, endpoint: &Path, client: &reqwest::blocking::Client) -> Option<String> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(rpc_error(Value::Null, -32700, "Parse error").to_string());
    };
    // Notifications (no id) need no answer.
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let ok = |result: Value| {
        Some(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
    };
    if let Some(result) = mcp::stateless_result(method, &params, true) {
        return ok(result);
    }
    if method != "tools/call" {
        return Some(rpc_error(id, -32601, "Method not found").to_string());
    }
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    if name == GUIDE_TOOL {
        return ok(tool_text(
            format!("{}\n{}", mcp::EXTERNAL_GUIDE, super::GRAPH_REFERENCE),
            false,
        ));
    }
    if mcp::BUILDER_ONLY_TOOLS.contains(&name) {
        return ok(tool_text(format!("unknown tool: {name}"), true));
    }
    match forward(line, endpoint, client) {
        Some(body) => Some(body),
        None => ok(tool_text(NOT_RUNNING.into(), true)),
    }
}

/// None when the app is closed, was never started, or its token rotated mid-call.
fn forward(line: &str, endpoint: &Path, client: &reqwest::blocking::Client) -> Option<String> {
    let file: Value = serde_json::from_slice(&std::fs::read(endpoint).ok()?).ok()?;
    let port = file.get("port")?.as_u64()?;
    let token = file.get("token")?.as_str()?;
    let response = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(line.to_string())
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.text().ok().filter(|body| !body.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(line: &str) -> Value {
        let client = reqwest::blocking::Client::new();
        let missing = std::env::temp_dir().join("alfred-no-such-endpoint.json");
        serde_json::from_str(&reply(line, &missing, &client).expect("reply")).unwrap()
    }

    #[test]
    fn outside_agents_get_setup_context_but_not_builder_writes() {
        let tools = call(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&GUIDE_TOOL) && names.contains(&"create_workflow"));
        assert!(names.contains(&"list_rules"));
        assert!(names.contains(&"list_mcp_servers"));
        assert!(names.contains(&"get_skill"));
        assert!(!names.contains(&"add_rule"));
        assert!(!names.contains(&"add_mcp_server"));

        let guide = call(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_workflow_guide"}}"#,
        );
        assert!(guide["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Node types"));

        let hidden =
            call(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add_rule"}}"#);
        assert_eq!(hidden["result"]["isError"], true);
    }

    #[test]
    fn closed_app_is_a_readable_tool_error_and_notifications_stay_silent() {
        let init = call(
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");

        let run = call(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_workflows"}}"#,
        );
        assert_eq!(run["id"], 4);
        assert!(run["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("isn't running"));

        let client = reqwest::blocking::Client::new();
        let note = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert!(reply(note, Path::new("/nonexistent"), &client).is_none());
    }
}
