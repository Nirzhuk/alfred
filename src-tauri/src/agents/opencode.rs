use super::models;
use super::process::{cwd_from_request, find_bin, prefer_stdout, run_cmd};
use super::{
    map_cmd_err, AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError,
    AgentProvider, AgentRequest, AgentResponse, AgentRunHooks,
};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

/// Adapter for the OpenCode CLI (`opencode`).
///
/// Headless: `opencode run --model <provider/model> <prompt>`
/// CLI flags reference: https://dev.opencode.ai/docs/cli/
pub struct OpencodeAdapter;

impl AgentAdapter for OpencodeAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Opencode
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let bin = find_bin("opencode").ok_or_else(|| {
            AgentError::Message(
                "opencode CLI not found. Install OpenCode and ensure `opencode` is on PATH.".into(),
            )
        })?;

        let prompt = request.effective_prompt();
        let model = request.effective_model(self.provider());
        let cwd = cwd_from_request(&request.working_directory);

        // Keep workflow-created sessions easy to identify in OpenCode history.
        let marker = format!("alfred-{}", Uuid::new_v4());

        let args = models::omit_cli_default_model(vec![
            "run".into(),
            "--model".into(),
            model.clone(),
            "--format".into(),
            "json".into(),
            "--agent".into(),
            "build".into(),
            "--auto".into(),
            "--title".into(),
            marker.clone(),
            prompt,
        ]);

        let stream_event = |line: &str| {
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
            Some(&stream_event),
        )
        .map_err(map_cmd_err)?;

        if !output.success {
            return Err(AgentError::Message(format!(
                "opencode failed:\n{}",
                prefer_stdout(&output)
            )));
        }

        let (text, run_stats) = parse_run_events(&output.stdout);
        if text.is_empty() {
            return Err(AgentError::Message(format!(
                "opencode completed without an assistant response{}",
                if output.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" (stderr: {})", output.stderr.trim())
                }
            )));
        }

        let mut metadata = serde_json::json!({
            "provider": "opencode",
            "model": model,
            "bin": bin.display().to_string(),
            "durationMs": output.duration_ms,
        });
        if let Some(stats) = run_stats {
            if let Value::Object(extra) = stats {
                if let Value::Object(meta) = &mut metadata {
                    meta.extend(extra);
                }
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
        Some("step_start") => vec![AgentActivity::new(
            event
                .pointer("/part/id")
                .and_then(Value::as_str)
                .unwrap_or("opencode:step"),
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "OpenCode session started",
            None,
        )],
        Some("text") => event
            .pointer("/part/text")
            .and_then(Value::as_str)
            .map(|text| {
                vec![AgentActivity::new(
                    event
                        .pointer("/part/id")
                        .and_then(Value::as_str)
                        .unwrap_or("opencode:text"),
                    AgentActivityKind::Assistant,
                    AgentActivityState::Completed,
                    "Agent response",
                    Some(text),
                )]
            })
            .unwrap_or_default(),
        Some("tool_use") | Some("tool") => {
            let part = event.get("part").unwrap_or(&event);
            let state = match part.pointer("/state/status").and_then(Value::as_str) {
                Some("completed") | Some("error") | Some("failed") => AgentActivityState::Completed,
                _ => AgentActivityState::Started,
            };
            let id = part
                .get("callID")
                .or_else(|| part.get("callId"))
                .or_else(|| part.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("opencode:tool");
            let label = part
                .get("tool")
                .or_else(|| part.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("Using tool");
            let detail = (state == AgentActivityState::Completed)
                .then(|| {
                    part.pointer("/state/output")
                        .or_else(|| part.pointer("/state/error"))
                        .and_then(Value::as_str)
                })
                .flatten();
            vec![AgentActivity::new(
                id,
                AgentActivityKind::Tool,
                state,
                label,
                detail,
            )]
        }
        Some("error") => vec![AgentActivity::new(
            "opencode:error",
            AgentActivityKind::Error,
            AgentActivityState::Completed,
            "OpenCode error",
            event
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| event.pointer("/error/message").and_then(Value::as_str)),
        )],
        _ => Vec::new(),
    }
}

fn parse_run_events(output: &str) -> (String, Option<Value>) {
    let mut text_parts = Vec::new();
    let mut total_cost = 0.0;
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut reasoning_tokens = 0_u64;
    let mut cache_read_tokens = 0_u64;
    let mut cache_creation_tokens = 0_u64;
    let mut has_stats = false;

    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = event.pointer("/part/text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        text_parts.push(text.trim().to_string());
                    }
                }
            }
            Some("step_finish") => {
                let Some(part) = event.get("part") else {
                    continue;
                };
                has_stats = true;
                total_cost += part.get("cost").and_then(Value::as_f64).unwrap_or(0.0);
                input_tokens += part
                    .pointer("/tokens/input")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                output_tokens += part
                    .pointer("/tokens/output")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                reasoning_tokens += part
                    .pointer("/tokens/reasoning")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                cache_read_tokens += part
                    .pointer("/tokens/cache/read")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                cache_creation_tokens += part
                    .pointer("/tokens/cache/write")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            }
            _ => {}
        }
    }

    let stats = has_stats.then(|| {
        serde_json::json!({
            "totalCostUsd": total_cost,
            "inputTokens": input_tokens,
            "outputTokens": output_tokens,
            "reasoningTokens": reasoning_tokens,
            "cacheReadTokens": cache_read_tokens,
            "cacheCreationTokens": cache_creation_tokens,
        })
    });
    (text_parts.join("\n\n"), stats)
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
    fn parses_text_and_aggregates_build_steps() {
        let output = [
            r#"{"type":"text","part":{"text":"First"}}"#,
            r#"{"type":"step_finish","part":{"cost":0.1,"tokens":{"input":2,"output":3,"reasoning":1,"cache":{"read":4,"write":5}}}}"#,
            r#"{"type":"text","part":{"text":"Done"}}"#,
            r#"{"type":"step_finish","part":{"cost":0.2,"tokens":{"input":7,"output":11,"reasoning":0,"cache":{"read":13,"write":17}}}}"#,
        ]
        .join("\n");

        let (text, stats) = parse_run_events(&output);
        let stats = stats.expect("stats");

        assert_eq!(text, "First\n\nDone");
        assert!((stats["totalCostUsd"].as_f64().unwrap() - 0.3).abs() < 0.000_001);
        assert_eq!(stats["inputTokens"], 9);
        assert_eq!(stats["outputTokens"], 14);
        assert_eq!(stats["reasoningTokens"], 1);
        assert_eq!(stats["cacheReadTokens"], 17);
        assert_eq!(stats["cacheCreationTokens"], 22);
    }

    #[test]
    fn emits_text_and_tool_lifecycle_without_tool_input() {
        let events = [
            r#"{"type":"step_start","part":{"id":"step-1"}}"#,
            r#"{"type":"tool_use","part":{"id":"part-1","callID":"call-1","tool":"bash","state":{"status":"running","input":{"command":"secret"}}}}"#,
            r#"{"type":"tool_use","part":{"id":"part-1","callID":"call-1","tool":"bash","state":{"status":"completed","output":"ok"}}}"#,
            r#"{"type":"text","part":{"id":"text-1","text":"Done"}}"#,
            r#"{"type":"future","payload":"ignored"}"#,
        ];
        let activities = events
            .iter()
            .flat_map(|event| activities_from_event(event))
            .collect::<Vec<_>>();

        // Lifecycle and ordering are unchanged; the unknown event is ignored.
        assert_eq!(activities.len(), 4);
        assert_eq!(activities[0].kind, AgentActivityKind::Status);
        assert_eq!(activities[0].state, AgentActivityState::Completed);
        assert_eq!(activities[0].label, "OpenCode session started");

        assert_eq!(activities[1].kind, AgentActivityKind::Tool);
        assert_eq!(activities[1].state, AgentActivityState::Started);
        assert_eq!(activities[1].label, "Using tool");

        assert_eq!(activities[2].kind, AgentActivityKind::Tool);
        assert_eq!(activities[2].state, AgentActivityState::Completed);
        assert_eq!(activities[2].label, "Tool completed");

        assert_eq!(activities[3].kind, AgentActivityKind::Assistant);
        assert_eq!(activities[3].label, "Agent response");

        // The tool call still correlates start to completion opaquely.
        assert_eq!(activities[1].id, activities[2].id);
        assert_ne!(activities[1].id, activities[3].id);
        assert!(activities.iter().all(|activity| is_opaque(&activity.id)));
        for raw_id in ["call-1", "part-1", "step-1", "text-1"] {
            assert!(
                activities.iter().all(|activity| activity.id != raw_id),
                "provider id {raw_id} reached a run event"
            );
        }

        assert!(activities.iter().all(|activity| activity.detail.is_none()));
        let rendered = format!("{activities:?}");
        for leaked in ["secret", "ignored", "Done", "bash", "call-1"] {
            assert!(!rendered.contains(leaked), "leaked {leaked} in {rendered}");
        }
    }
}
