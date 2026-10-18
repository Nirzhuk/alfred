//! Loopback MCP server (streamable HTTP, JSON responses only) exposing
//! Alfred's internal API to the Speak with Alfred agent.
//!
//! Bound to 127.0.0.1 on an ephemeral port and gated by a per-process bearer
//! token that only reaches the agent through an owner-only config file.

use super::{BuilderMcpServer, BuilderRule, BuilderState, WORKFLOW_CHANGED_EVENT};
use crate::agents;
use crate::db::{CreateWorkflowInput, Db, OnMissed, UpdateWorkflowInput};
use crate::integrations::IntegrationsState;
use crate::runner::{self, RunTrigger};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tiny_http::{Header, Request, Response, Server};

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_NODES: usize = 200;
const MAX_EDGES: usize = 400;
const MAX_WAIT_SECONDS: u64 = 300;
const STEP_OUTPUT_PREVIEW_CHARS: usize = 1500;

pub fn start(app: AppHandle) -> Result<u16, String> {
    let server = Server::http(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or("no port")?;
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let app = app.clone();
            // A tool call may wait on a run; never block the accept loop.
            std::thread::spawn(move || respond(&app, request));
        }
    });
    Ok(port)
}

fn respond(app: &AppHandle, mut request: Request) {
    let (status, body) = handle_http(app, &mut request);
    let response = Response::from_string(body).with_status_code(status);
    let response = if status == 200 {
        response.with_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                .expect("static header"),
        )
    } else {
        response
    };
    let _ = request.respond(response);
}

fn secret_matches(expected: &str, provided: &str) -> bool {
    expected.len() == provided.len()
        && expected
            .bytes()
            .zip(provided.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn handle_http(app: &AppHandle, request: &mut Request) -> (u16, String) {
    let path = request.url().split('?').next().unwrap_or("");
    if path != "/mcp" {
        return (404, String::new());
    }
    let token = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .and_then(|h| h.value.as_str().strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    let expected = app.state::<BuilderState>().mcp_token.clone();
    if !secret_matches(&expected, &token) {
        return (401, String::new());
    }
    // No server-initiated stream: GET/DELETE are optional in the transport.
    if request.method().as_str() != "POST" {
        return (405, String::new());
    }
    let mut body = String::new();
    if request
        .as_reader()
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_string(&mut body)
        .is_err()
        || body.len() > MAX_BODY_BYTES
    {
        return (413, String::new());
    }
    let Ok(message) = serde_json::from_str::<Value>(&body) else {
        return (
            200,
            rpc_error(Value::Null, -32700, "Parse error").to_string(),
        );
    };
    let reply = match message {
        Value::Array(batch) => {
            let replies: Vec<Value> = batch.into_iter().filter_map(|m| handle_rpc(app, m)).collect();
            (!replies.is_empty()).then(|| Value::Array(replies))
        }
        single => handle_rpc(app, single),
    };
    match reply {
        Some(reply) => (200, reply.to_string()),
        None => (202, String::new()),
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Notifications (no id) get no reply.
fn handle_rpc(app: &AppHandle, message: Value) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "alfred", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Alfred's internal API: create, edit, run and schedule workflows.",
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tool_definitions() }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (text, is_error) = match call_tool(app, name, &args) {
                Ok(value) => (
                    serde_json::to_string_pretty(&value).unwrap_or_default(),
                    false,
                ),
                Err(message) => (message, true),
            };
            json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
        }
        _ => return Some(rpc_error(id, -32601, "Method not found")),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

pub fn tool_definitions() -> Vec<Value> {
    let graph = json!({
        "type": "object",
        "description": "{ nodes: [{id, type, position?, data}], edges: [{id?, source, target, data?: {condition}}] }",
    });
    vec![
        tool("list_workflows", "List every workflow (id, name, description, step count).", json!({}), &[]),
        tool("get_workflow", "Get one workflow including its full graph.", json!({ "id": { "type": "string" } }), &["id"]),
        tool(
            "create_workflow",
            "Create a workflow from a graph. It appears in the user's sidebar immediately.",
            json!({
                "name": { "type": "string" },
                "description": { "type": "string" },
                "workingDirectory": { "type": "string", "description": "Absolute folder where agents/scripts run." },
                "parallelBranches": { "type": "boolean", "description": "Run fan-out branches concurrently (default true)." },
                "graph": graph,
            }),
            &["name", "graph"],
        ),
        tool(
            "update_workflow",
            "Update a workflow. `graph` replaces the whole graph, so send every node and edge.",
            json!({
                "id": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "workingDirectory": { "type": "string" },
                "parallelBranches": { "type": "boolean" },
                "graph": graph,
            }),
            &["id"],
        ),
        tool("run_workflow", "Start a manual run. Returns the run id.", json!({ "id": { "type": "string" } }), &["id"]),
        tool(
            "get_run",
            "Get a run's status and each step's output. waitSeconds (max 300) waits for it to finish.",
            json!({ "runId": { "type": "string" }, "waitSeconds": { "type": "number" } }),
            &["runId"],
        ),
        tool(
            "set_schedule",
            "Create or replace a workflow's cron schedule (5 fields, local time).",
            json!({
                "workflowId": { "type": "string" },
                "cron": { "type": "string" },
                "enabled": { "type": "boolean" },
                "onMissed": { "type": "string", "enum": ["run", "skip", "ask"] },
            }),
            &["workflowId", "cron"],
        ),
        tool("list_agents", "Agent CLIs for agent steps: provider id, whether installed, default model.", json!({}), &[]),
        tool(
            "list_skills",
            "SKILL.md packages usable in an agent step's skillNames.",
            json!({ "projectRoot": { "type": "string", "description": "Also scan this project folder." } }),
            &[],
        ),
        tool("list_connected_apps", "Connected app accounts and the actions appAction steps can call.", json!({}), &[]),
        tool("list_rules", "Standing rules the user set for you.", json!({}), &[]),
        tool(
            "add_rule",
            "Save a standing rule for yourself (applies from the next message). Only when the user asks.",
            json!({ "name": { "type": "string" }, "body": { "type": "string" } }),
            &["name", "body"],
        ),
        tool("list_mcp_servers", "MCP servers the user added for you (secrets redacted).", json!({}), &[]),
        tool(
            "add_mcp_server",
            "Add an MCP server for yourself (available from the next message). Only when the user asks.",
            json!({
                "name": { "type": "string", "description": "letters, digits, - and _" },
                "transport": { "type": "string", "enum": ["stdio", "http"] },
                "command": { "type": "string" },
                "args": { "type": "array", "items": { "type": "string" } },
                "env": { "type": "object", "additionalProperties": { "type": "string" } },
                "url": { "type": "string" },
                "headers": { "type": "object", "additionalProperties": { "type": "string" } },
            }),
            &["name", "transport"],
        ),
    ]
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("`{key}` is required."))
}

fn string_map(value: Option<&Value>) -> std::collections::BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn workflow_summary(workflow: &crate::db::Workflow) -> Value {
    let count = |key: &str| {
        workflow.graph[key]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default()
    };
    json!({
        "id": workflow.id,
        "name": workflow.name,
        "description": workflow.description,
        "workingDirectory": workflow.working_directory,
        "steps": count("nodes"),
        "connections": count("edges"),
        "updatedAt": workflow.updated_at,
    })
}

fn changed(app: &AppHandle, workflow: &crate::db::Workflow, action: &str) {
    let _ = app.emit(
        WORKFLOW_CHANGED_EVENT,
        json!({ "workflowId": workflow.id, "name": workflow.name, "action": action }),
    );
}

pub fn call_tool(app: &AppHandle, name: &str, args: &Value) -> Result<Value, String> {
    let db = app.state::<Db>();
    match name {
        "list_workflows" => {
            let workflows = db.list_workflows().map_err(|e| e.to_string())?;
            Ok(Value::Array(workflows.iter().map(workflow_summary).collect()))
        }
        "get_workflow" => {
            let id = required(args, "id")?;
            let workflow = db
                .get_workflow(id)
                .map_err(|e| e.to_string())?
                .ok_or("No workflow with that id. Call list_workflows.")?;
            let mut value = serde_json::to_value(&workflow).map_err(|e| e.to_string())?;
            // Canvas-only measurements just cost the agent tokens.
            if let Some(nodes) = value["graph"]["nodes"].as_array_mut() {
                for node in nodes {
                    if let Some(node) = node.as_object_mut() {
                        for key in ["measured", "selected", "dragging", "width", "height"] {
                            node.remove(key);
                        }
                    }
                }
            }
            Ok(value)
        }
        "create_workflow" => {
            let name = required(args, "name")?.to_string();
            let graph = normalize_graph(args.get("graph").cloned().unwrap_or(json!({})))?;
            let workflow = db
                .create_workflow(CreateWorkflowInput {
                    name,
                    description: str_arg(args, "description").unwrap_or_default().to_string(),
                    working_directory: str_arg(args, "workingDirectory")
                        .unwrap_or_default()
                        .to_string(),
                    folder_id: None,
                    graph,
                })
                .map_err(|e| e.to_string())?;
            let workflow = match args.get("parallelBranches").and_then(Value::as_bool) {
                Some(parallel) => db
                    .update_workflow(update_input(&workflow.id, Some(parallel), None))
                    .map_err(|e| e.to_string())?,
                None => workflow,
            };
            changed(app, &workflow, "created");
            Ok(workflow_summary(&workflow))
        }
        "update_workflow" => {
            let id = required(args, "id")?;
            db.get_workflow(id)
                .map_err(|e| e.to_string())?
                .ok_or("No workflow with that id. Call list_workflows.")?;
            let graph = match args.get("graph") {
                Some(graph) if !graph.is_null() => Some(normalize_graph(graph.clone())?),
                _ => None,
            };
            let mut input = update_input(
                id,
                args.get("parallelBranches").and_then(Value::as_bool),
                graph,
            );
            input.name = str_arg(args, "name")
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string);
            input.description = str_arg(args, "description").map(str::to_string);
            input.working_directory = str_arg(args, "workingDirectory").map(str::to_string);
            let workflow = db.update_workflow(input).map_err(|e| e.to_string())?;
            changed(app, &workflow, "updated");
            Ok(workflow_summary(&workflow))
        }
        "run_workflow" => {
            let id = required(args, "id")?;
            let summary = runner::start_run(app.clone(), db.inner(), id, RunTrigger::Manual, None)
                .map_err(|e| e.to_string())?;
            crate::tray::refresh(app);
            serde_json::to_value(summary).map_err(|e| e.to_string())
        }
        "get_run" => {
            let run_id = required(args, "runId")?;
            let wait = args
                .get("waitSeconds")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .clamp(0.0, MAX_WAIT_SECONDS as f64);
            let deadline = Instant::now() + Duration::from_secs_f64(wait);
            loop {
                let detail = db
                    .get_run_history(run_id)
                    .map_err(|e| e.to_string())?
                    .ok_or("No run with that id.")?;
                let finished = matches!(
                    detail.run.status.as_str(),
                    "completed" | "failed" | "cancelled"
                );
                if finished || Instant::now() >= deadline {
                    return Ok(run_report(&detail));
                }
                std::thread::sleep(Duration::from_millis(800));
            }
        }
        "set_schedule" => {
            let workflow_id = required(args, "workflowId")?.to_string();
            let cron = required(args, "cron")?.to_string();
            let on_missed = match str_arg(args, "onMissed") {
                Some("skip") => OnMissed::Skip,
                Some("ask") => OnMissed::Ask,
                _ => OnMissed::Run,
            };
            let enabled = args.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let schedule = crate::scheduler::upsert_schedule(
                db.inner(),
                workflow_id,
                cron,
                enabled,
                on_missed,
            )
            .map_err(|e| e.to_string())?;
            let _ = app.emit("schedules://changed", ());
            serde_json::to_value(schedule).map_err(|e| e.to_string())
        }
        "list_agents" => {
            // ponytail: PATH lookup only; probing each CLI's model catalog took ~30s.
            use agents::AgentProvider::*;
            let catalog = [
                (ClaudeCode, &["claude"][..]),
                (Codex, &["codex"][..]),
                (Cursor, &["cursor-agent", "agent"][..]),
                (Opencode, &["opencode"][..]),
                (GithubCopilot, &["copilot"][..]),
                (Gemini, &["gemini"][..]),
                (Grok, &["grok"][..]),
                (Pi, &["pi"][..]),
                (Omp, &["omp"][..]),
            ];
            Ok(json!({
                "agents": catalog
                    .iter()
                    .map(|(provider, bins)| {
                        json!({
                            "provider": provider.as_str(),
                            "label": provider.label(),
                            "installed": bins.iter().any(|bin| crate::agents::process::find_bin(bin).is_some()),
                            "defaultModel": agents::models::default_model(*provider),
                        })
                    })
                    .collect::<Vec<_>>(),
                "note": "Use model: null for the CLI's default model.",
            }))
        }
        "list_skills" => {
            let skills = crate::skills::list_skills(str_arg(args, "projectRoot"))
                .map_err(|e| e.to_string())?;
            Ok(Value::Array(
                skills
                    .iter()
                    .map(|s| json!({ "name": s.name, "description": s.description, "source": s.source }))
                    .collect(),
            ))
        }
        "list_connected_apps" => {
            let connections = db.list_app_connections().map_err(|e| e.to_string())?;
            let connections: Vec<Value> = connections
                .into_iter()
                .map(crate::integrations::models::AppConnectionDto::from)
                .map(|c| {
                    json!({
                        "connectionId": c.id,
                        "providerId": c.provider_id,
                        "displayName": c.display_name,
                        "status": c.status,
                    })
                })
                .collect();
            let connected: HashSet<String> = connections
                .iter()
                .filter_map(|c| c["providerId"].as_str().map(str::to_string))
                .collect();
            let actions: Vec<Value> = app
                .state::<IntegrationsState>()
                .action_descriptors(None)
                .into_iter()
                .filter(|a| connected.contains(&a.provider_id))
                .map(|a| serde_json::to_value(a).unwrap_or_default())
                .collect();
            Ok(json!({ "connections": connections, "actions": actions }))
        }
        "list_rules" => Ok(serde_json::to_value(super::load_config().rules).unwrap_or_default()),
        "add_rule" => {
            let rule = BuilderRule {
                id: uuid::Uuid::new_v4().to_string(),
                name: required(args, "name")?.to_string(),
                body: required(args, "body")?.to_string(),
                enabled: true,
            };
            super::update_config(app, |config| config.rules.push(rule))?;
            Ok(json!({ "ok": true, "note": "Applies from the user's next message." }))
        }
        "list_mcp_servers" => Ok(Value::Array(
            super::load_config()
                .mcp_servers
                .iter()
                .map(|s| {
                    json!({
                        "name": s.name,
                        "enabled": s.enabled,
                        "transport": s.transport,
                        "command": s.command,
                        "args": s.args,
                        "url": s.url,
                        "envKeys": s.env.keys().collect::<Vec<_>>(),
                        "headerKeys": s.headers.keys().collect::<Vec<_>>(),
                    })
                })
                .collect(),
        )),
        "add_mcp_server" => {
            let server = BuilderMcpServer {
                id: uuid::Uuid::new_v4().to_string(),
                name: required(args, "name")?.to_string(),
                enabled: true,
                transport: required(args, "transport")?.to_string(),
                command: str_arg(args, "command").unwrap_or_default().to_string(),
                args: args
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
                env: string_map(args.get("env")),
                url: str_arg(args, "url").unwrap_or_default().to_string(),
                headers: string_map(args.get("headers")),
            };
            super::update_config(app, |config| config.mcp_servers.push(server))?;
            Ok(json!({ "ok": true, "note": "Available from the user's next message." }))
        }
        _ => Err(format!("Unknown tool `{name}`.")),
    }
}

fn update_input(id: &str, parallel: Option<bool>, graph: Option<Value>) -> UpdateWorkflowInput {
    UpdateWorkflowInput {
        id: id.to_string(),
        name: None,
        description: None,
        working_directory: None,
        memory_retrieval_enabled: None,
        memory_lookup_enabled: None,
        parallel_branches: parallel,
        graph,
    }
}

fn run_report(detail: &crate::db::RunHistoryDetail) -> Value {
    let steps: Vec<Value> = detail
        .steps
        .iter()
        .map(|step| {
            let output = match &step.output {
                Value::String(text) => text.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            let mut preview: String = output.chars().take(STEP_OUTPUT_PREVIEW_CHARS).collect();
            if preview.len() < output.len() {
                preview.push('…');
            }
            json!({ "nodeId": step.node_id, "status": step.status, "error": step.error, "output": preview })
        })
        .collect();
    json!({
        "runId": detail.run.id,
        "workflow": detail.run.workflow_name,
        "status": detail.run.status,
        "error": detail.run.error,
        "finalOutput": detail.run.final_output_preview,
        "steps": steps,
    })
}

/// Canvas defaults for each node type, mirroring `src/features/workflow/types.ts`.
fn node_defaults(kind: &str) -> Option<Value> {
    Some(match kind {
        "input" | "prompt" => json!({ "label": "Input", "prompt": "", "blocked": false, "attachments": [] }),
        "agent" => json!({ "label": "Agent", "provider": "claude_code", "harness": "cli", "model": null, "skillNames": [] }),
        "identity" => json!({ "kind": "identity", "label": "Identity", "role": "", "personality": "", "instructions": "" }),
        "chooseOutput" => json!({ "label": "Output", "saveToMemory": true, "pinMemory": false, "memoryTitle": "", "asFinalResult": true, "htmlReport": false, "includeFilesChanged": true }),
        "memory" => json!({ "label": "Memories", "memoryIds": [] }),
        "template" => json!({ "kind": "template", "label": "Template", "template": "{{output}}", "mode": "append" }),
        "fileInject" => json!({ "kind": "fileInject", "label": "File inject", "paths": [] }),
        "gitStatus" => json!({ "kind": "gitStatus", "label": "Git status", "includeDiff": true }),
        "shell" => json!({ "kind": "shell", "label": "Shell", "command": "", "appendOutput": true }),
        "script" => json!({ "kind": "script", "label": "Script", "source": "inline", "path": "", "body": "", "interpreter": "bash", "appendOutput": true }),
        "http" => json!({ "kind": "http", "label": "HTTP", "method": "GET", "url": "", "body": "", "headers": "" }),
        "notify" => json!({ "kind": "notify", "label": "Notify", "title": "Alfred", "body": "{{output}}", "desktop": true, "webhookUrl": "" }),
        "writeFile" => json!({ "kind": "writeFile", "label": "Write file", "path": "", "content": "{{output}}" }),
        "gitHost" => json!({ "kind": "gitHost", "label": "Create PR", "action": "pr", "repo": "", "title": "", "body": "{{output}}", "base": "", "head": "", "draft": false }),
        "appAction" => json!({ "type": "appAction", "label": "App action", "providerId": "", "actionId": "", "connectionId": "", "input": {} }),
        "customAgent" => json!({ "kind": "customAgent", "label": "Custom agent", "command": "", "promptMode": "template" }),
        _ => return None,
    })
}

/// Make an agent-written graph safe for the canvas and runner: known node
/// types, defaulted data, unique ids, edges between real nodes, and a
/// left-to-right layout when positions are missing.
pub fn normalize_graph(graph: Value) -> Result<Value, String> {
    let empty = Vec::new();
    let raw_nodes = graph.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let raw_edges = graph.get("edges").and_then(Value::as_array).unwrap_or(&empty);
    if raw_nodes.len() > MAX_NODES || raw_edges.len() > MAX_EDGES {
        return Err(format!("A graph may have at most {MAX_NODES} steps and {MAX_EDGES} connections."));
    }

    let mut ids = HashSet::new();
    let mut nodes = Vec::with_capacity(raw_nodes.len());
    let mut needs_layout = false;
    for (index, raw) in raw_nodes.iter().enumerate() {
        let kind = raw.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(mut data) = node_defaults(kind) else {
            return Err(format!(
                "Step {index} has unknown type `{kind}`. Use input, agent, identity, chooseOutput, memory, template, fileInject, gitStatus, script, http, notify, writeFile, gitHost, appAction or customAgent."
            ));
        };
        if let Some(given) = raw.get("data").and_then(Value::as_object) {
            let target = data.as_object_mut().expect("defaults are objects");
            for (key, value) in given {
                target.insert(key.clone(), value.clone());
            }
            // The discriminators are not the agent's to change.
            if target.contains_key("kind") && kind != "chooseOutput" {
                target.insert("kind".into(), json!(kind));
            }
            if kind == "appAction" {
                target.insert("type".into(), json!("appAction"));
            }
        }
        if kind == "agent" {
            let provider = data["provider"].as_str().unwrap_or("");
            if agents::AgentProvider::from_str(provider).is_none() {
                return Err(format!("Agent step {index} has unknown provider `{provider}`. Call list_agents."));
            }
        }
        let id = raw
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{kind}-{}", index + 1));
        if !ids.insert(id.clone()) {
            return Err(format!("Two steps share the id `{id}`."));
        }
        let position = raw.get("position").filter(|p| {
            p.get("x").and_then(Value::as_f64).is_some() && p.get("y").and_then(Value::as_f64).is_some()
        });
        needs_layout |= position.is_none();
        nodes.push(json!({
            "id": id,
            "type": kind,
            "position": position.cloned().unwrap_or(json!({ "x": 0, "y": 0 })),
            "data": data,
        }));
    }

    let mut edges = Vec::with_capacity(raw_edges.len());
    let mut pairs = HashSet::new();
    for raw in raw_edges {
        let source = raw.get("source").and_then(Value::as_str).unwrap_or("");
        let target = raw.get("target").and_then(Value::as_str).unwrap_or("");
        if !ids.contains(source) || !ids.contains(target) {
            return Err(format!("Connection {source} → {target} points to a step that does not exist."));
        }
        if source == target || !pairs.insert((source.to_string(), target.to_string())) {
            continue;
        }
        let mut edge = Map::new();
        edge.insert(
            "id".into(),
            raw.get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(|id| json!(id))
                .unwrap_or_else(|| json!(format!("e-{source}-{target}"))),
        );
        edge.insert("source".into(), json!(source));
        edge.insert("target".into(), json!(target));
        edge.insert("type".into(), json!("default"));
        if let Some(data) = raw.get("data").filter(|d| d.is_object()) {
            edge.insert("data".into(), data.clone());
        }
        edges.push(Value::Object(edge));
    }

    if needs_layout {
        layout(&mut nodes, &edges);
    }
    let mut graph = json!({ "nodes": nodes, "edges": edges });
    agents::normalize_agent_nodes_in_graph(&mut graph).map_err(|e| e.to_string())?;
    Ok(graph)
}

/// Longest-path columns, one row per step in each column, centered.
fn layout(nodes: &mut [Value], edges: &[Value]) {
    let index: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n["id"].as_str().unwrap_or_default().to_string(), i))
        .collect();
    let mut indegree = vec![0usize; nodes.len()];
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for edge in edges {
        let (Some(&s), Some(&t)) = (
            index.get(edge["source"].as_str().unwrap_or_default()),
            index.get(edge["target"].as_str().unwrap_or_default()),
        ) else {
            continue;
        };
        next[s].push(t);
        indegree[t] += 1;
    }
    let mut column = vec![0usize; nodes.len()];
    let mut queue: VecDeque<usize> = (0..nodes.len()).filter(|&i| indegree[i] == 0).collect();
    while let Some(i) = queue.pop_front() {
        for &t in &next[i] {
            column[t] = column[t].max(column[i] + 1);
            indegree[t] -= 1;
            if indegree[t] == 0 {
                queue.push_back(t);
            }
        }
    }
    let mut rows: HashMap<usize, usize> = HashMap::new();
    let mut heights: HashMap<usize, usize> = HashMap::new();
    for &c in &column {
        *heights.entry(c).or_default() += 1;
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        let c = column[i];
        let row = rows.entry(c).or_default();
        let height = heights[&c] as f64;
        let y = (*row as f64 - (height - 1.0) / 2.0) * 240.0;
        node["position"] = json!({ "x": c as f64 * 320.0, "y": y });
        *row += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_minimal_agent_graph_with_defaults_and_layout() {
        let graph = normalize_graph(json!({
            "nodes": [
                { "id": "goal", "type": "input", "data": { "prompt": "Summarize" } },
                { "id": "a", "type": "agent", "data": { "provider": "codex", "label": "A" } },
                { "id": "b", "type": "agent", "data": { "provider": "claude_code" } },
                { "id": "out", "type": "chooseOutput" }
            ],
            "edges": [
                { "source": "goal", "target": "a" },
                { "source": "goal", "target": "b" },
                { "source": "a", "target": "out" },
                { "source": "b", "target": "out" },
                { "source": "b", "target": "out" }
            ]
        }))
        .unwrap();
        let nodes = graph["nodes"].as_array().unwrap();
        assert_eq!(nodes[0]["data"]["blocked"], false);
        assert_eq!(nodes[1]["data"]["harness"], "cli");
        assert_eq!(nodes[3]["data"]["asFinalResult"], true);
        // goal → (a, b) → out: three columns, a and b stacked.
        assert_eq!(nodes[0]["position"]["x"], 0.0);
        assert_eq!(nodes[1]["position"]["x"], 320.0);
        assert_eq!(nodes[3]["position"]["x"], 640.0);
        assert_ne!(nodes[1]["position"]["y"], nodes[2]["position"]["y"]);
        assert_eq!(graph["edges"].as_array().unwrap().len(), 4);
        assert_eq!(graph["edges"][0]["id"], "e-goal-a");
    }

    #[test]
    fn rejects_unknown_types_providers_dangling_edges_and_duplicate_ids() {
        let bad = |graph| normalize_graph(graph).unwrap_err();
        assert!(bad(json!({ "nodes": [{ "id": "x", "type": "teleport" }] })).contains("unknown type"));
        assert!(bad(json!({ "nodes": [{ "id": "x", "type": "agent", "data": { "provider": "hal" } }] }))
            .contains("unknown provider"));
        assert!(bad(json!({ "nodes": [{ "id": "x", "type": "input" }], "edges": [{ "source": "x", "target": "y" }] }))
            .contains("does not exist"));
        assert!(bad(json!({ "nodes": [{ "id": "x", "type": "input" }, { "id": "x", "type": "input" }] }))
            .contains("share the id"));
    }

    #[test]
    fn keeps_explicit_positions_and_utility_discriminators() {
        let graph = normalize_graph(json!({
            "nodes": [{ "id": "t", "type": "template", "position": { "x": 5, "y": 7 }, "data": { "kind": "evil", "template": "{{context}}" } }]
        }))
        .unwrap();
        assert_eq!(graph["nodes"][0]["position"]["x"], 5);
        assert_eq!(graph["nodes"][0]["data"]["kind"], "template");
        assert_eq!(graph["nodes"][0]["data"]["mode"], "append");
    }

    #[test]
    fn tool_definitions_have_unique_names_and_object_schemas() {
        let tools = tool_definitions();
        let names: HashSet<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names.len(), tools.len());
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    }
}
