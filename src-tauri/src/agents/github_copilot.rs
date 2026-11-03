use super::models;
use super::process::{cwd_from_request, find_bin, prefer_stdout, run_cmd};
use super::{
    map_cmd_err, AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError,
    AgentProvider, AgentRequest, AgentResponse, AgentRunHooks,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Adapter for the GitHub Copilot CLI (`copilot`).
///
/// Copilot's `-p` mode is intentionally used instead of its interactive TUI so
/// Alfred can pass the workflow prompt, stream output, and terminate the
/// process when a run is cancelled.
pub struct GithubCopilotAdapter;

impl AgentAdapter for GithubCopilotAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::GithubCopilot
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let bin = find_bin("copilot").ok_or_else(|| {
            AgentError::Message(
                "GitHub Copilot CLI not found. Install `@github/copilot` and ensure `copilot` is on PATH."
                    .into(),
            )
        })?;

        let prompt = request.effective_prompt();
        let model = request.effective_model(self.provider());
        let cwd = cwd_from_request(&request.working_directory);
        let args = models::omit_cli_default_model(vec![
            "-p".into(),
            prompt,
            "-s".into(),
            "--model".into(),
            model.clone(),
            "--allow-all".into(),
            "--no-ask-user".into(),
        ]);

        let line_index = AtomicUsize::new(0);
        let on_line = |line: &str| {
            if line.trim().is_empty() {
                return;
            }
            let index = line_index.fetch_add(1, Ordering::Relaxed);
            if let Some(callback) = hooks.on_activity {
                callback(&AgentActivity::new(
                    format!("github_copilot:text:{index}"),
                    AgentActivityKind::Assistant,
                    AgentActivityState::Completed,
                    "Agent response",
                    Some(line),
                ));
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
        let text = prefer_stdout(&output);

        if text.trim().is_empty() {
            return Err(AgentError::Message(format!(
                "copilot returned empty output{}",
                if output.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" (stderr: {})", output.stderr.trim())
                }
            )));
        }
        if !output.success {
            return Err(AgentError::Message(format!("copilot failed:\n{text}")));
        }

        Ok(AgentResponse {
            output: text,
            metadata: serde_json::json!({
                "provider": "github_copilot",
                "model": model,
                "bin": bin.display().to_string(),
                "durationMs": output.duration_ms,
            }),
        })
    }
}
