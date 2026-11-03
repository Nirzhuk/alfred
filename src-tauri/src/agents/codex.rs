use super::models;
use super::process::{cwd_from_request, find_bin, prefer_stdout, run_cmd};
use super::{
    AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError, AgentProvider,
    AgentRequest, AgentResponse, AgentRunHooks,
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Adapter for the OpenAI Codex CLI (`codex`).
///
/// The installed `codex exec --help` is the local authority for its JSONL mode.
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let bin = find_bin("codex").ok_or_else(|| {
            AgentError::Message(
                "codex CLI not found. Install the Codex CLI and ensure `codex` is on PATH.".into(),
            )
        })?;

        let prompt = request.effective_prompt();
        let model = request.effective_model(self.provider());
        let cwd = cwd_from_request(&request.working_directory);
        let attempts: Vec<Vec<String>> = vec![
            vec![
                "exec".into(),
                "--json".into(),
                "--model".into(),
                model.clone(),
                "--full-auto".into(),
                prompt.clone(),
            ],
            vec![
                "exec".into(),
                "--model".into(),
                model.clone(),
                "--full-auto".into(),
                prompt.clone(),
            ],
            vec![
                "exec".into(),
                "--model".into(),
                model.clone(),
                prompt.clone(),
            ],
            vec!["--model".into(), model.clone(), "-q".into(), prompt],
        ];

        let mut last_err = String::new();
        for args in attempts.into_iter().map(models::omit_cli_default_model) {
            if hooks
                .control
                .map(|control| control.is_cancelled())
                .unwrap_or(false)
            {
                return Err(AgentError::Cancelled);
            }
            let structured = args.iter().any(|arg| arg == "--json");
            let line_index = AtomicUsize::new(0);
            let line_handler = |line: &str| {
                if structured {
                    for activity in activities_from_event(line) {
                        if let Some(callback) = hooks.on_activity {
                            callback(&activity);
                        }
                    }
                } else if !line.trim().is_empty() {
                    let index = line_index.fetch_add(1, Ordering::Relaxed);
                    if let Some(callback) = hooks.on_activity {
                        callback(&AgentActivity::new(
                            format!("codex:text:{index}"),
                            AgentActivityKind::Assistant,
                            AgentActivityState::Completed,
                            "Agent response",
                            Some(line),
                        ));
                    }
                }
            };

            match run_cmd(
                &bin,
                &args,
                cwd.as_deref(),
                Duration::from_secs(60 * 15),
                hooks.control,
                Some(&line_handler),
            ) {
                Ok(output) => {
                    let text = prefer_stdout(&output);
                    if text.trim().is_empty() {
                        last_err = format!("empty output (stderr: {})", output.stderr.trim());
                        continue;
                    }
                    if !output.success
                        && (text.to_lowercase().contains("unrecognized")
                            || text.to_lowercase().contains("unknown")
                            || text.to_lowercase().contains("usage:"))
                    {
                        last_err = text;
                        continue;
                    }
                    if !output.success {
                        return Err(AgentError::Message(format!("codex failed:\n{text}")));
                    }
                    if structured {
                        if let Some((message, mut metadata)) =
                            parse_json_output(&text, &model, &bin, output.duration_ms)
                        {
                            if message.is_empty() {
                                return Err(AgentError::Message(
                                    "codex completed without an assistant message".into(),
                                ));
                            }
                            if let Value::Object(map) = &mut metadata {
                                map.insert("provider".into(), json!("codex"));
                            }
                            return Ok(AgentResponse {
                                output: message,
                                metadata,
                            });
                        }
                    }
                    return Ok(AgentResponse {
                        output: text,
                        metadata: serde_json::json!({
                            "provider": "codex",
                            "model": model,
                            "bin": bin.display().to_string(),
                            "durationMs": output.duration_ms,
                        }),
                    });
                }
                Err(error) if error == "cancelled" => return Err(AgentError::Cancelled),
                Err(error) => last_err = error,
            }
        }

        Err(AgentError::Message(format!(
            "codex could not run.\n{last_err}"
        )))
    }
}

fn activities_from_event(line: &str) -> Vec<AgentActivity> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match event.get("type").and_then(Value::as_str) {
        Some("thread.started") => vec![AgentActivity::new(
            "codex:session",
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Codex session started",
            None,
        )],
        Some("turn.started") => vec![AgentActivity::new(
            "codex:turn",
            AgentActivityKind::Status,
            AgentActivityState::Started,
            "Working",
            None,
        )],
        Some("turn.completed") => vec![AgentActivity::new(
            "codex:turn",
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Work completed",
            None,
        )],
        Some("item.started") => event
            .get("item")
            .map(|item| item_activities(item, AgentActivityState::Started))
            .unwrap_or_default(),
        Some("item.completed") => event
            .get("item")
            .map(|item| item_activities(item, AgentActivityState::Completed))
            .unwrap_or_default(),
        Some("turn.failed") | Some("error") => vec![AgentActivity::new(
            "codex:error",
            AgentActivityKind::Error,
            AgentActivityState::Completed,
            "Codex error",
            event_error(&event),
        )],
        _ => Vec::new(),
    }
}

fn item_activities(item: &Value, state: AgentActivityState) -> Vec<AgentActivity> {
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("item");
    match item.get("type").and_then(Value::as_str) {
        Some("reasoning") => vec![AgentActivity::new(
            format!("codex:reasoning:{item_id}"),
            AgentActivityKind::Status,
            state,
            "Thinking",
            None,
        )],
        Some("agent_message") if state == AgentActivityState::Completed => item
            .get("text")
            .and_then(Value::as_str)
            .map(|text| {
                vec![AgentActivity::new(
                    item_id,
                    AgentActivityKind::Assistant,
                    AgentActivityState::Completed,
                    "Agent response",
                    Some(text),
                )]
            })
            .unwrap_or_default(),
        Some("command_execution") => {
            let command = item.get("command").and_then(Value::as_str);
            let result = (state == AgentActivityState::Completed)
                .then(|| item.get("aggregated_output").and_then(Value::as_str))
                .flatten();
            let detail = match (command, result) {
                (Some(command), Some(result)) if !result.trim().is_empty() => {
                    Some(format!("{command}\n{result}"))
                }
                (Some(command), _) => Some(command.to_string()),
                (_, Some(result)) => Some(result.to_string()),
                _ => None,
            };
            vec![AgentActivity::new(
                item_id,
                AgentActivityKind::Command,
                state,
                "Command",
                detail.as_deref(),
            )]
        }
        Some("file_change") if state == AgentActivityState::Completed => {
            let changes = item.get("changes").and_then(Value::as_array);
            changes
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(index, change)| {
                    let path = change.get("path").and_then(Value::as_str)?;
                    Some(AgentActivity::new(
                        format!("{item_id}:{index}"),
                        AgentActivityKind::File,
                        AgentActivityState::Completed,
                        format!("Changed {path}"),
                        None,
                    ))
                })
                .collect()
        }
        Some("mcp_tool_call") => {
            let label = [
                item.get("server").and_then(Value::as_str),
                item.get("tool").and_then(Value::as_str),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let detail = (state == AgentActivityState::Completed)
                .then(|| item.get("result").and_then(Value::as_str))
                .flatten();
            vec![AgentActivity::new(
                item_id,
                AgentActivityKind::Tool,
                state,
                if label.is_empty() { "MCP tool" } else { &label },
                detail,
            )]
        }
        Some("web_search") => vec![AgentActivity::new(
            item_id,
            AgentActivityKind::Tool,
            state,
            "Web search",
            item.get("query").and_then(Value::as_str),
        )],
        _ => Vec::new(),
    }
}

fn event_error(event: &Value) -> Option<&str> {
    event
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| event.pointer("/error/message").and_then(Value::as_str))
}

/// Parse Codex's `exec --json` stream for final response and usage. Activity
/// parsing is intentionally separate so console text cannot alter final output.
fn parse_json_output(
    raw: &str,
    model: &str,
    bin: &Path,
    duration_ms: u128,
) -> Option<(String, Value)> {
    let mut messages = Vec::new();
    let mut usage: Option<Value> = None;

    for line in raw.lines() {
        let event: Value = serde_json::from_str(line).ok()?;
        match event.get("type").and_then(Value::as_str) {
            Some("item.completed") => {
                let item = event.get("item")?;
                if item.get("type").and_then(Value::as_str) == Some("agent_message") {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        messages.push(text.to_string());
                    }
                }
            }
            Some("turn.completed") => usage = event.get("usage").cloned(),
            Some("turn.failed") | Some("error") => return None,
            _ => {}
        }
    }

    if messages.is_empty() && usage.is_none() {
        return None;
    }

    let usage = usage.unwrap_or_else(|| json!({}));
    let metadata = json!({
        "provider": "codex",
        "model": model,
        "bin": bin.display().to_string(),
        "durationMs": duration_ms,
        "inputTokens": usage.get("input_tokens").cloned().unwrap_or(Value::Null),
        "outputTokens": usage.get("output_tokens").cloned().unwrap_or(Value::Null),
        "reasoningTokens": usage.get("reasoning_output_tokens").cloned().unwrap_or(Value::Null),
        "cacheReadTokens": usage.get("cached_input_tokens").cloned().unwrap_or(Value::Null),
    });
    Some((messages.join("\n"), metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Activity ids are hashed at the boundary, so a provider id never
    /// reaches a run event verbatim.
    fn is_opaque(id: &str) -> bool {
        id.strip_prefix("agent_activity_").is_some_and(|suffix| {
            suffix.len() == 24 && suffix.bytes().all(|b| b.is_ascii_hexdigit())
        })
    }

    #[test]
    fn maps_supported_items_and_hides_reasoning_text() {
        let events = [
            r#"{"type":"thread.started","thread_id":"abc"}"#,
            r#"{"type":"item.started","item":{"id":"r1","type":"reasoning","text":"private chain"}}"#,
            r#"{"type":"item.started","item":{"id":"c1","type":"command_execution","command":"pwd"}}"#,
            r#"{"type":"item.completed","item":{"id":"c1","type":"command_execution","command":"pwd","aggregated_output":"/tmp"}}"#,
            r#"{"type":"item.completed","item":{"id":"f1","type":"file_change","changes":[{"path":"src/app.rs","kind":"update"}]}}"#,
            r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"Done"}}"#,
            r#"{"type":"future.event","raw":{"secret":true}}"#,
        ];
        let activities = events
            .iter()
            .flat_map(|event| activities_from_event(event))
            .collect::<Vec<_>>();

        // Every supported item still produces its activity in order; the
        // unknown `future.event` is still ignored.
        assert_eq!(activities.len(), 6);
        assert_eq!(activities[0].kind, AgentActivityKind::Status);
        assert_eq!(activities[0].state, AgentActivityState::Completed);
        assert_eq!(activities[0].label, "Codex session started");

        assert_eq!(activities[1].kind, AgentActivityKind::Status);
        assert_eq!(activities[1].state, AgentActivityState::Started);
        assert_eq!(activities[1].label, "Thinking");

        assert_eq!(activities[2].kind, AgentActivityKind::Command);
        assert_eq!(activities[2].state, AgentActivityState::Started);
        assert_eq!(activities[2].label, "Running command");

        assert_eq!(activities[3].kind, AgentActivityKind::Command);
        assert_eq!(activities[3].state, AgentActivityState::Completed);
        assert_eq!(activities[3].label, "Command completed");

        // The changed path is a category now, not the path itself.
        assert_eq!(activities[4].kind, AgentActivityKind::File);
        assert_eq!(activities[4].state, AgentActivityState::Completed);
        assert_eq!(activities[4].label, "File changed");

        assert_eq!(activities[5].kind, AgentActivityKind::Assistant);
        assert_eq!(activities[5].state, AgentActivityState::Completed);
        assert_eq!(activities[5].label, "Agent response");

        // The command still correlates start to completion via an opaque id.
        assert_eq!(activities[2].id, activities[3].id);
        assert_ne!(activities[2].id, activities[4].id);
        assert!(activities.iter().all(|activity| is_opaque(&activity.id)));
        // Provider item ids are short and hex-like, so compare them exactly
        // rather than scanning the rendered digest for a substring.
        for raw_id in ["abc", "r1", "c1", "f1", "m1"] {
            assert!(
                activities.iter().all(|activity| activity.id != raw_id),
                "provider id {raw_id} reached a run event"
            );
        }

        assert!(activities.iter().all(|activity| activity.detail.is_none()));
        let rendered = format!("{activities:?}");
        for leaked in [
            "private chain",
            "secret",
            "src/app.rs",
            "pwd",
            "/tmp",
            "Done",
        ] {
            assert!(!rendered.contains(leaked), "leaked {leaked} in {rendered}");
        }
    }

    #[test]
    fn preserves_final_message_and_usage_parser() {
        let raw = [
            r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"Done"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":2,"output_tokens":3,"reasoning_output_tokens":1,"cached_input_tokens":4}}"#,
        ]
        .join("\n");
        let (message, metadata) =
            parse_json_output(&raw, "model", Path::new("codex"), 10).expect("output");
        assert_eq!(message, "Done");
        assert_eq!(metadata["inputTokens"], 2);
        assert_eq!(metadata["reasoningTokens"], 1);
    }
}
