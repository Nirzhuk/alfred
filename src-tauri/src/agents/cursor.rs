use super::models;
use super::process::{cwd_from_request, find_bin, prefer_stdout, run_cmd};
use super::{
    AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError, AgentProvider,
    AgentRequest, AgentResponse, AgentRunHooks,
};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Adapter for the Cursor agent CLI (`cursor-agent` / `agent`).
///
/// Stream schema reference: https://docs.cursor.com/en/cli/reference/output-format
pub struct CursorAdapter;

impl AgentAdapter for CursorAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Cursor
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let bin = find_bin("cursor-agent")
            .or_else(|| find_bin("agent"))
            .ok_or_else(|| {
                AgentError::Message(
                    "cursor-agent CLI not found. Install/update Cursor Agent (`cursor-agent update`)."
                        .into(),
                )
            })?;

        let prompt = request.effective_prompt();
        let model = request.effective_model(self.provider());
        let cwd = cwd_from_request(&request.working_directory);
        let attempts: Vec<(Vec<String>, bool)> = vec![
            (
                vec![
                    "-p".into(),
                    "--force".into(),
                    "--model".into(),
                    model.clone(),
                    "--output-format".into(),
                    "stream-json".into(),
                    prompt.clone(),
                ],
                true,
            ),
            (
                vec!["-p".into(), "--model".into(), model.clone(), prompt.clone()],
                false,
            ),
            (
                vec![
                    "agent".into(),
                    "-p".into(),
                    "--model".into(),
                    model.clone(),
                    prompt,
                ],
                false,
            ),
        ];

        let mut last_err = String::new();
        for (args, structured) in attempts {
            let args = models::omit_cli_default_model(args);
            if hooks
                .control
                .map(|control| control.is_cancelled())
                .unwrap_or(false)
            {
                return Err(AgentError::Cancelled);
            }
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
                            format!("cursor:text:{index}"),
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
                    if text.contains("Press any key to sign in")
                        || text.contains("version is too old")
                        || text.contains("cursor-agent update")
                    {
                        last_err = text;
                        continue;
                    }
                    if text.trim().is_empty() {
                        last_err = format!("empty output (stderr: {})", output.stderr.trim());
                        continue;
                    }
                    if !output.success
                        && (text.to_lowercase().contains("unknown option")
                            || text
                                .to_lowercase()
                                .contains("not in the list of known options"))
                    {
                        last_err = text;
                        continue;
                    }
                    if !output.success {
                        return Err(AgentError::Message(format!("cursor-agent failed:\n{text}")));
                    }
                    let final_text = if structured {
                        parse_stream_output(&output.stdout).ok_or_else(|| {
                            AgentError::Message(
                                "cursor-agent completed without an assistant response".into(),
                            )
                        })?
                    } else {
                        text
                    };
                    return Ok(AgentResponse {
                        output: final_text,
                        metadata: serde_json::json!({
                            "provider": "cursor",
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
            "cursor-agent could not run. Update with `cursor-agent update`.\n{last_err}"
        )))
    }
}

fn activities_from_event(line: &str) -> Vec<AgentActivity> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match event.get("type").and_then(Value::as_str) {
        Some("system") if event.get("subtype").and_then(Value::as_str) == Some("init") => {
            vec![AgentActivity::new(
                "cursor:session",
                AgentActivityKind::Status,
                AgentActivityState::Completed,
                "Cursor session started",
                None,
            )]
        }
        Some("tool_call") => {
            let state = match event.get("subtype").and_then(Value::as_str) {
                Some("completed") | Some("success") | Some("error") => {
                    AgentActivityState::Completed
                }
                _ => AgentActivityState::Started,
            };
            let id = event
                .get("call_id")
                .or_else(|| event.get("id"))
                .or_else(|| event.pointer("/tool_call/id"))
                .and_then(Value::as_str)
                .unwrap_or("cursor:tool");
            let name = event
                .get("tool_name")
                .or_else(|| event.get("name"))
                .or_else(|| event.pointer("/tool_call/name"))
                .and_then(Value::as_str)
                .unwrap_or("Using tool");
            let detail = (state == AgentActivityState::Completed)
                .then(|| {
                    event
                        .get("result")
                        .or_else(|| event.get("output"))
                        .or_else(|| event.pointer("/tool_call/result"))
                        .and_then(Value::as_str)
                })
                .flatten();
            vec![AgentActivity::new(
                id,
                AgentActivityKind::Tool,
                state,
                name,
                detail,
            )]
        }
        Some("assistant") => event_text(&event)
            .map(|text| {
                vec![AgentActivity::new(
                    event
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("cursor:assistant"),
                    AgentActivityKind::Assistant,
                    AgentActivityState::Completed,
                    "Agent response",
                    Some(&text),
                )]
            })
            .unwrap_or_default(),
        Some("result") if event.get("subtype").and_then(Value::as_str) == Some("error") => {
            vec![AgentActivity::new(
                "cursor:error",
                AgentActivityKind::Error,
                AgentActivityState::Completed,
                "Cursor error",
                event.get("result").and_then(Value::as_str),
            )]
        }
        _ => Vec::new(),
    }
}

fn event_text(event: &Value) -> Option<String> {
    if let Some(text) = event.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    match event.pointer("/message/content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn parse_stream_output(raw: &str) -> Option<String> {
    let mut assistant = Vec::new();
    let mut result = None;
    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(text) = event_text(&event) {
                    assistant.push(text);
                }
            }
            Some("result") => {
                if let Some(text) = event.get("result").and_then(Value::as_str) {
                    result = Some(text.to_string());
                }
            }
            _ => {}
        }
    }
    result.or_else(|| (!assistant.is_empty()).then(|| assistant.join("\n")))
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
    fn parses_cursor_session_tool_and_assistant_events() {
        let events = [
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"tool_call","subtype":"started","call_id":"call-1","tool_name":"Read","input":{"path":"private"}}"#,
            r#"{"type":"tool_call","subtype":"completed","call_id":"call-1","tool_name":"Read","result":"ok"}"#,
            r#"{"type":"assistant","id":"msg-1","message":{"content":[{"type":"text","text":"Done"}]}}"#,
            r#"{"type":"unknown","payload":"ignored"}"#,
        ];
        let activities = events
            .iter()
            .flat_map(|event| activities_from_event(event))
            .collect::<Vec<_>>();

        // Ordering and lifecycle are unchanged; the unknown event is ignored.
        assert_eq!(activities.len(), 4);
        assert_eq!(activities[0].kind, AgentActivityKind::Status);
        assert_eq!(activities[0].state, AgentActivityState::Completed);
        assert_eq!(activities[0].label, "Cursor session started");

        assert_eq!(activities[1].kind, AgentActivityKind::Tool);
        assert_eq!(activities[1].state, AgentActivityState::Started);
        assert_eq!(activities[1].label, "Using tool");

        assert_eq!(activities[2].kind, AgentActivityKind::Tool);
        assert_eq!(activities[2].state, AgentActivityState::Completed);
        assert_eq!(activities[2].label, "Tool completed");

        assert_eq!(activities[3].kind, AgentActivityKind::Assistant);
        assert_eq!(activities[3].state, AgentActivityState::Completed);
        assert_eq!(activities[3].label, "Agent response");

        // The tool call still correlates start to completion opaquely.
        assert_eq!(activities[1].id, activities[2].id);
        assert_ne!(activities[1].id, activities[3].id);
        assert!(activities.iter().all(|activity| is_opaque(&activity.id)));

        assert!(activities.iter().all(|activity| activity.detail.is_none()));
        let rendered = format!("{activities:?}");
        for leaked in ["private", "ignored", "Done", "call-1", "msg-1", "Read"] {
            assert!(!rendered.contains(leaked), "leaked {leaked} in {rendered}");
        }
    }

    #[test]
    fn terminal_result_wins_over_assistant_records() {
        let raw = [
            r#"{"type":"assistant","message":{"content":"Draft"}}"#,
            r#"{"type":"result","subtype":"success","result":"Final"}"#,
        ]
        .join("\n");
        assert_eq!(parse_stream_output(&raw).as_deref(), Some("Final"));
    }
}
