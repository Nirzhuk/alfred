use super::models;
use super::process::{cwd_from_request, find_bin, prefer_stdout, run_cmd};
use super::{
    map_cmd_err, AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError,
    AgentProvider, AgentRequest, AgentResponse, AgentRunHooks,
};
use serde_json::Value;
use std::time::Duration;

/// Adapter for xAI's Grok Build CLI (`grok`).
pub struct GrokAdapter;

impl AgentAdapter for GrokAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Grok
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let bin = find_bin("grok").ok_or_else(|| {
            AgentError::Message(
                "Grok CLI not found. Install Grok Build and ensure `grok` is on PATH.".into(),
            )
        })?;

        let prompt = request.effective_prompt();
        let model = request.effective_model(self.provider());
        let cwd = cwd_from_request(&request.working_directory);
        let args = models::omit_cli_default_model(vec![
            "-p".into(),
            prompt,
            "--model".into(),
            model.clone(),
            "--output-format".into(),
            "streaming-json".into(),
            "--always-approve".into(),
        ]);

        let on_line = |line: &str| {
            for activity in activities_from_event(line) {
                if let Some(callback) = hooks.on_activity {
                    callback(&activity);
                }
            }
        };
        let output = run_cmd(
            &bin,
            &args,
            cwd.as_deref(),
            Duration::from_secs(60 * 15),
            hooks.control,
            Some(&on_line),
        )
        .map_err(map_cmd_err)?;

        let raw = prefer_stdout(&output);
        if raw.trim().is_empty() {
            return Err(AgentError::Message(format!(
                "grok returned empty output{}",
                if output.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" (stderr: {})", output.stderr.trim())
                }
            )));
        }
        if !output.success {
            return Err(AgentError::Message(format!("grok failed:\n{raw}")));
        }

        let (text, stats, error) = parse_stream_output(&output.stdout);
        let Some(text) = text.filter(|value| !value.trim().is_empty()) else {
            return Err(AgentError::Message(error.unwrap_or_else(|| {
                "grok completed without an assistant response".into()
            })));
        };

        let mut metadata = serde_json::json!({
            "provider": "grok",
            "model": model,
            "bin": bin.display().to_string(),
            "durationMs": output.duration_ms,
        });
        if let Some(Value::Object(extra)) = stats {
            if let Value::Object(map) = &mut metadata {
                map.insert("stats".into(), Value::Object(extra));
            }
        }

        Ok(AgentResponse {
            output: text,
            metadata,
        })
    }
}

fn activities_from_event(line: &str) -> Vec<AgentActivity> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };

    match event.get("type").and_then(Value::as_str) {
        Some("init") | Some("start") => vec![AgentActivity::new(
            "grok:session",
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Grok session started",
            event
                .get("model")
                .or_else(|| event.get("sessionId"))
                .and_then(Value::as_str),
        )],
        Some("text") | Some("message") => event_text(&event)
            .map(|text| {
                vec![AgentActivity::new(
                    "grok:assistant",
                    AgentActivityKind::Assistant,
                    AgentActivityState::Completed,
                    "Agent response",
                    Some(&text),
                )]
            })
            .unwrap_or_default(),
        Some("tool_use") | Some("tool_call") => vec![AgentActivity::new(
            event
                .get("tool_id")
                .or_else(|| event.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("grok:tool"),
            AgentActivityKind::Tool,
            AgentActivityState::Started,
            event
                .get("tool_name")
                .or_else(|| event.get("name"))
                .or_else(|| event.get("tool"))
                .and_then(Value::as_str)
                .unwrap_or("Using tool"),
            None,
        )],
        Some("tool_result") | Some("tool_completed") => vec![AgentActivity::new(
            event
                .get("tool_id")
                .or_else(|| event.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("grok:tool"),
            AgentActivityKind::Tool,
            AgentActivityState::Completed,
            "Tool completed",
            event.get("status").and_then(Value::as_str),
        )],
        Some("error") => vec![AgentActivity::new(
            "grok:error",
            AgentActivityKind::Error,
            AgentActivityState::Completed,
            "Grok error",
            event
                .get("message")
                .or_else(|| event.get("error"))
                .and_then(Value::as_str),
        )],
        Some("end") => {
            if event
                .get("stopReason")
                .or_else(|| event.get("status"))
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case("error"))
            {
                vec![AgentActivity::new(
                    "grok:error",
                    AgentActivityKind::Error,
                    AgentActivityState::Completed,
                    "Grok error",
                    event.get("error").and_then(Value::as_str),
                )]
            } else {
                vec![AgentActivity::new(
                    "grok:turn",
                    AgentActivityKind::Status,
                    AgentActivityState::Completed,
                    "Work completed",
                    None,
                )]
            }
        }
        _ => Vec::new(),
    }
}

fn event_text(event: &Value) -> Option<String> {
    event
        .get("data")
        .or_else(|| event.get("text"))
        .or_else(|| event.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn parse_stream_output(raw: &str) -> (Option<String>, Option<Value>, Option<String>) {
    let mut text = Vec::new();
    let mut stats = None;
    let mut error = None;

    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("text") | Some("message") => {
                if let Some(chunk) = event_text(&event) {
                    text.push(chunk);
                }
            }
            Some("end") => {
                stats = event.get("stats").cloned();
                if event
                    .get("stopReason")
                    .or_else(|| event.get("status"))
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case("error"))
                {
                    error = event
                        .get("error")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            Some("error") => {
                error = event
                    .get("message")
                    .or_else(|| event.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    }

    ((!text.is_empty()).then(|| text.join("")), stats, error)
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
    fn parses_grok_streaming_json() {
        let raw = [
            r#"{"type":"text","data":"Hello"}"#,
            r#"{"type":"thought","data":"hidden"}"#,
            r#"{"type":"text","data":" world"}"#,
            r#"{"type":"end","stopReason":"EndTurn","sessionId":"s1"}"#,
        ]
        .join("\n");

        let (text, stats, error) = parse_stream_output(&raw);
        assert_eq!(text.as_deref(), Some("Hello world"));
        assert_eq!(stats, None);
        assert_eq!(error, None);

        let activities = raw
            .lines()
            .flat_map(activities_from_event)
            .collect::<Vec<_>>();
        // The hidden `thought` event still produces no activity at all.
        assert_eq!(activities.len(), 3);
        assert_eq!(activities[0].kind, AgentActivityKind::Assistant);
        assert_eq!(activities[0].label, "Agent response");
        assert_eq!(activities[1].kind, AgentActivityKind::Assistant);
        assert_eq!(activities[1].label, "Agent response");
        assert_eq!(activities[2].kind, AgentActivityKind::Status);
        assert_eq!(activities[2].state, AgentActivityState::Completed);
        assert_eq!(activities[2].label, "Work completed");

        assert!(activities.iter().all(|activity| is_opaque(&activity.id)));
        assert!(activities.iter().all(|activity| activity.detail.is_none()));
        let rendered = format!("{activities:?}");
        for leaked in ["Hello", "world", "hidden", "EndTurn"] {
            assert!(!rendered.contains(leaked), "leaked {leaked} in {rendered}");
        }
    }
}
