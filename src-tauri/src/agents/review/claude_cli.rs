//! Claude Code's dedicated restricted review transport.
//!
//! The installed CLI is inspected as read-only. A runtime is considered
//! eligible only when its version is explicitly known and the local help text
//! contains every restriction used by this invocation. Live activation remains
//! a separate proof obligation; production therefore fails closed until that
//! check is recorded by the product.

use super::super::process::find_bin;
use super::{
    RestrictedReviewAgent, RestrictedReviewRequest, RestrictedReviewResponse, ReviewAvailability,
    ReviewError, ReviewExecutionIdentity, ReviewUsage, SupportedReviewCapability,
    UnavailableReviewCapability,
};
use crate::agents::review::REVIEW_CONTRACT_VERSION;
use crate::agents::AgentProvider;
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

pub const PRODUCT: &str = "claude_code";
pub const EXECUTION_MODE: &str = "restricted_print";

/// The current CLI's help output is the local source of truth for these
/// switches. The provider documentation is recorded in the support note.
const REQUIRED_FLAGS: &[&str] = &[
    "--safe-mode",
    "--restricted",
    "--tools",
    "--strict-mcp-config",
    "--disable-slash-commands",
    "--setting-sources",
    "--no-session-persistence",
    "--permission-prompts",
    "--max-turns",
    "--input-format",
    "--output-format",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeReviewCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
}

pub fn build_command(
    program: impl Into<PathBuf>,
    working_directory: impl Into<PathBuf>,
    model: Option<&str>,
) -> ClaudeReviewCommand {
    let mut args = vec![
        "-p".into(),
        "--input-format".into(),
        "text".into(),
        "--output-format".into(),
        "json".into(),
        "--max-turns".into(),
        "1".into(),
        "--no-session-persistence".into(),
        "--safe-mode".into(),
        "--restricted".into(),
        "--tools".into(),
        "".into(),
        "--strict-mcp-config".into(),
        "--disable-slash-commands".into(),
        "--setting-sources".into(),
        "".into(),
        "--permission-prompts".into(),
        "none".into(),
    ];
    if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
        args.extend(["--model".into(), model.to_owned()]);
    }
    ClaudeReviewCommand {
        program: program.into(),
        args,
        working_directory: working_directory.into(),
    }
}

/// Inspect a version/help pair without contacting the provider. The returned
/// supported row is a conformance fixture; production additionally requires
/// `live_activation_verified` before launching the process.
pub fn inspect_capability(
    version: &str,
    help: &str,
    live_activation_verified: bool,
) -> ReviewAvailability {
    let version = version.trim();
    // Only versions that have been inspected and recorded in the support
    // document may become candidates. Future 2.x releases fail closed.
    let known_version = version == "2.1.263";
    let all_flags = REQUIRED_FLAGS.iter().all(|flag| {
        help.split_whitespace().any(|token| {
            token == *flag
                || token
                    .strip_prefix(flag)
                    .is_some_and(|suffix| suffix.starts_with('='))
        })
    });
    let identity = ReviewExecutionIdentity {
        provider: AgentProvider::ClaudeCode,
        product: PRODUCT.into(),
        runtime_version: version.into(),
        execution_mode: EXECUTION_MODE.into(),
        restriction_contract_version: REVIEW_CONTRACT_VERSION.into(),
    };
    if !known_version {
        return unavailable(version, "unknown_runtime_version");
    }
    if !all_flags {
        return unavailable(version, "restriction_contract_not_proven");
    }
    if !live_activation_verified {
        return unavailable(version, "live_activation_not_verified");
    }
    ReviewAvailability::Supported(SupportedReviewCapability { identity })
}

fn unavailable(version: &str, reason: &str) -> ReviewAvailability {
    ReviewAvailability::Unavailable(UnavailableReviewCapability {
        provider: AgentProvider::ClaudeCode,
        product: PRODUCT.into(),
        runtime_version: (!version.is_empty()).then(|| version.into()),
        execution_mode: EXECUTION_MODE.into(),
        restriction_contract_version: REVIEW_CONTRACT_VERSION.into(),
        reason: reason.into(),
    })
}

/// Read-only capability discovery for the installed CLI. The product does not
/// infer live support from this result; callers must retain the unavailable
/// result until an authorized synthetic activation check is recorded.
pub fn installed_capability() -> ReviewAvailability {
    #[cfg(not(unix))]
    {
        return unavailable("", "restricted_transport_unsupported_platform");
    }
    #[cfg(unix)]
    installed_capability_uncached()
}

fn installed_capability_uncached() -> ReviewAvailability {
    let Some(bin) = find_bin("claude") else {
        return unavailable("", "provider_not_installed");
    };
    let version = probe_output(&bin, "--version")
        .map(|output| {
            String::from_utf8_lossy(&output)
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .unwrap_or_default();
    let help = probe_output(&bin, "--help")
        .map(|output| String::from_utf8_lossy(&output).into_owned())
        .unwrap_or_default();
    inspect_capability(&version, &help, false)
}

fn probe_output(bin: &PathBuf, arg: &str) -> Option<Vec<u8>> {
    let mut builder = Command::new(bin);
    builder
        .arg(arg)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        builder.process_group(0);
    }
    let mut child = builder.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stdout.take(64 * 1024).read_to_end(&mut output);
        output
    });
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A probe can itself fork. Kill its process group before
                // joining the pipe reader, otherwise a descendant holding
                // stdout open can defeat the probe deadline.
                kill_process_tree(&mut child);
                break status;
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                kill_process_tree(&mut child);
                let _ = child.wait();
                let _ = reader.join();
                return None;
            }
        }
    };
    let output = reader.join().ok()?;
    status.success().then_some(output)
}

/// Restricted Claude transport. It is intentionally only constructible by
/// the review module; ordinary workflow adapters cannot reach it accidentally.
pub struct ClaudeCliReviewAgent {
    availability: ReviewAvailability,
    program: Option<PathBuf>,
    capability_probe: Arc<dyn Fn() -> ReviewAvailability + Send + Sync>,
    executable_probe: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>,
}

impl ClaudeCliReviewAgent {
    pub fn installed() -> Self {
        #[cfg(not(unix))]
        {
            return Self {
                availability: unavailable("", "restricted_transport_unsupported_platform"),
                program: None,
                capability_probe: Arc::new(|| {
                    unavailable("", "restricted_transport_unsupported_platform")
                }),
                executable_probe: Arc::new(|| None),
            };
        }
        #[cfg(unix)]
        Self::from_capability(installed_capability())
    }

    pub(crate) fn from_capability(availability: ReviewAvailability) -> Self {
        Self {
            availability,
            program: find_bin("claude"),
            capability_probe: Arc::new(installed_capability),
            executable_probe: Arc::new(|| find_bin("claude")),
        }
    }

    #[cfg(test)]
    pub fn with_availability(availability: ReviewAvailability) -> Self {
        Self {
            capability_probe: Arc::new({
                let expected = availability.clone();
                move || expected.clone()
            }),
            availability,
            program: None,
            executable_probe: Arc::new(|| None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_program_and_probe(
        availability: ReviewAvailability,
        program: PathBuf,
        capability_probe: impl Fn() -> ReviewAvailability + Send + Sync + 'static,
    ) -> Self {
        let expected_program = program.clone();
        Self::with_program_and_probes(availability, program, capability_probe, move || {
            Some(expected_program.clone())
        })
    }

    #[cfg(test)]
    pub(crate) fn with_program_and_probes(
        availability: ReviewAvailability,
        program: PathBuf,
        capability_probe: impl Fn() -> ReviewAvailability + Send + Sync + 'static,
        executable_probe: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
    ) -> Self {
        Self {
            availability,
            program: Some(program),
            capability_probe: Arc::new(capability_probe),
            executable_probe: Arc::new(executable_probe),
        }
    }
}

impl RestrictedReviewAgent for ClaudeCliReviewAgent {
    fn run_review(
        &self,
        request: RestrictedReviewRequest,
    ) -> Result<RestrictedReviewResponse, ReviewError> {
        let capability = match &self.availability {
            ReviewAvailability::Supported(value) => value,
            ReviewAvailability::Unavailable(value) => {
                return Err(ReviewError::Unavailable(value.clone()))
            }
        };
        if request.identity != capability.identity {
            return Err(ReviewError::InvalidRequest);
        }
        if request.cancellation.is_cancelled() {
            return Err(ReviewError::Cancelled);
        }
        if self.program.is_some() && (self.capability_probe)() != self.availability {
            return Err(ReviewError::InvalidRequest);
        }
        // Test fixtures may inject capability metadata, but cannot use that
        // metadata to reach an arbitrary installed executable. Production
        // stores the exact path discovered by `installed()` and rechecks it.
        let bin = self.program.clone().ok_or(ReviewError::Process)?;
        if self
            .program
            .as_ref()
            .is_some_and(|expected| (self.executable_probe)().as_ref() != Some(expected))
        {
            return Err(ReviewError::InvalidRequest);
        }
        let temp =
            std::env::temp_dir().join(format!("alfred-memory-review-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&temp).map_err(|_| ReviewError::Process)?;
        let command = build_command(&bin, &temp, request.model.as_deref());
        let result = invoke(&command, &request);
        let _ = fs::remove_dir_all(&temp);
        result
    }
}

fn invoke(
    command: &ClaudeReviewCommand,
    request: &RestrictedReviewRequest,
) -> Result<RestrictedReviewResponse, ReviewError> {
    request.validate()?;
    let start = Instant::now();
    let mut builder = Command::new(&command.program);
    builder
        .args(&command.args)
        .current_dir(&command.working_directory)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMPDIR",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "AWS_PROFILE",
        "AWS_REGION",
        "AWS_DEFAULT_REGION",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ] {
        if let Some(value) = std::env::var_os(key) {
            builder.env(key, value);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        builder.process_group(0);
    }
    // Keep PATH/HOME so the selected CLI can use its own installation and
    // authentication. Extensions are disabled by the fixed argv above.
    let mut child = builder.spawn().map_err(|_| ReviewError::Process)?;
    let stdin = child.stdin.take().ok_or(ReviewError::Process)?;
    let prompt = request.prompt.clone();
    let stdin_thread = thread::spawn(move || {
        let mut stdin = stdin;
        stdin.write_all(prompt.as_bytes()).map_err(|_| ())
    });
    let stdout = child.stdout.take().ok_or(ReviewError::Process)?;
    let stderr = child.stderr.take().ok_or(ReviewError::Process)?;
    let channel_capacity = (request.limits.response_max_bytes / 4096).clamp(1, 8);
    let (output_tx, output_rx) = mpsc::sync_channel::<Vec<u8>>(channel_capacity);
    let stdout_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut buffer = [0u8; 4096];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) if output_tx.send(buffer[..read].to_vec()).is_err() => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    let stderr_thread = thread::spawn(move || {
        let mut stderr = stderr;
        let mut buffer = [0u8; 4096];
        while let Ok(read) = stderr.read(&mut buffer) {
            if read == 0 {
                break;
            }
        }
    });
    let mut output = Vec::with_capacity(request.limits.response_max_bytes.min(4096));
    let mut output_closed = false;
    let mut child_finished = false;
    let status = loop {
        if request.cancellation.is_cancelled() {
            kill_process_tree(&mut child);
            let _ = child.wait();
            drop(output_rx);
            let _ = stdin_thread.join();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(ReviewError::Cancelled);
        }
        if start.elapsed() > request.limits.timeout {
            kill_process_tree(&mut child);
            let _ = child.wait();
            drop(output_rx);
            let _ = stdin_thread.join();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(ReviewError::Timeout);
        }
        loop {
            match output_rx.try_recv() {
                Ok(chunk) => {
                    if request.cancellation.is_cancelled() {
                        kill_process_tree(&mut child);
                        let _ = child.wait();
                        drop(output_rx);
                        let _ = stdin_thread.join();
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(ReviewError::Cancelled);
                    }
                    if output.len().saturating_add(chunk.len()) > request.limits.response_max_bytes
                    {
                        kill_process_tree(&mut child);
                        let _ = child.wait();
                        drop(output_rx);
                        let _ = stdin_thread.join();
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(ReviewError::ResponseTooLarge {
                            limit: request.limits.response_max_bytes,
                        });
                    }
                    output.extend_from_slice(&chunk);
                    if stream_contains_tool_event(&output) {
                        kill_process_tree(&mut child);
                        let _ = child.wait();
                        drop(output_rx);
                        let _ = stdin_thread.join();
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(ReviewError::ToolRequest);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    output_closed = true;
                    break;
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !child_finished {
                    child_finished = true;
                    // The direct child may have exited while a descendant
                    // still owns stdout/stderr. Close the whole process group
                    // before waiting for the reader threads.
                    kill_process_tree(&mut child);
                }
                if output_closed {
                    break status;
                }
            }
            Ok(None) => {}
            Err(_) => {
                kill_process_tree(&mut child);
                let _ = child.wait();
                drop(output_rx);
                let _ = stdin_thread.join();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(ReviewError::Process);
            }
        }
        thread::sleep(std::time::Duration::from_millis(10));
    };
    let _ = stdin_thread.join();
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    if !status.success() {
        return Err(ReviewError::Process);
    }
    let output = String::from_utf8(output).map_err(|_| ReviewError::Process)?;
    let text = decode_output(&output, request.limits.response_max_bytes)?;
    Ok(RestrictedReviewResponse {
        usage: ReviewUsage {
            response_bytes: text.len(),
            duration_ms: start.elapsed().as_millis(),
        },
        output: text,
    })
}

fn decode_output(output: &str, max_bytes: usize) -> Result<String, ReviewError> {
    if output.len() > max_bytes {
        return Err(ReviewError::ResponseTooLarge { limit: max_bytes });
    }
    if stream_contains_tool_event(output.as_bytes()) {
        return Err(ReviewError::ToolRequest);
    }
    let text = serde_json::from_str::<Value>(&output)
        .ok()
        .and_then(|value| {
            value
                .get("result")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| output.to_owned());
    if text.len() > max_bytes {
        return Err(ReviewError::ResponseTooLarge { limit: max_bytes });
    }
    Ok(text)
}

fn stream_contains_tool_event(output: &[u8]) -> bool {
    let whole = serde_json::from_slice::<Value>(output)
        .ok()
        .is_some_and(|value| value_has_tool_event(&value));
    whole
        || output
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
            .any(|value| value_has_tool_event(&value))
}

fn value_has_tool_event(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "tool_use" || kind == "tool_call")
                || map.iter().any(|(key, child)| {
                    key == "tool_use" || key == "tool_call" || value_has_tool_event(child)
                })
        }
        Value::Array(items) => items.iter().any(value_has_tool_event),
        _ => false,
    }
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(test)]
mod capability {
    use super::*;

    fn help() -> String {
        REQUIRED_FLAGS.join(" ")
    }

    #[test]
    fn known_fixture_is_supported_only_after_live_activation() {
        assert!(!inspect_capability("2.1.263", &help(), false).is_supported());
        assert!(inspect_capability("2.1.263", &help(), true).is_supported());
    }

    #[test]
    fn unknown_version_fails_closed() {
        let result = inspect_capability("3.0.0", &help(), true);
        assert_eq!(result.reason(), Some("unknown_runtime_version"));
    }

    #[test]
    fn changed_restriction_contract_fails_closed() {
        let result = inspect_capability("2.1.263", "--safe-mode", true);
        assert_eq!(result.reason(), Some("restriction_contract_not_proven"));
    }

    #[test]
    fn lookalike_flag_does_not_satisfy_the_contract() {
        let help = help().replace("--restricted", "--restricted-mode");
        let result = inspect_capability("2.1.263", &help, true);
        assert_eq!(result.reason(), Some("restriction_contract_not_proven"));
    }

    #[cfg(unix)]
    #[test]
    fn injected_supported_capability_runs_only_the_injected_fixture() {
        use std::os::unix::fs::PermissionsExt;
        let path =
            std::env::temp_dir().join(format!("alfred-review-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            b"#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"result\":\"fixture\"}'\n",
        )
        .expect("fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture executable");
        let capability = inspect_capability("2.1.263", &help(), true);
        let expected = capability.clone();
        let agent =
            ClaudeCliReviewAgent::with_program_and_probe(capability, path.clone(), move || {
                expected.clone()
            });
        let request = RestrictedReviewRequest::new(
            ReviewExecutionIdentity::claude_cli("2.1.263"),
            None,
            "fixture input".into(),
        )
        .expect("request");
        let result = agent.run_review(request).expect("fixture response");
        assert_eq!(result.output, "fixture");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn capability_drift_is_rejected_before_injected_executable_launch() {
        use std::os::unix::fs::PermissionsExt;
        let marker =
            std::env::temp_dir().join(format!("alfred-review-marker-{}", uuid::Uuid::new_v4()));
        let path =
            std::env::temp_dir().join(format!("alfred-review-drift-{}", uuid::Uuid::new_v4()));
        let script = format!("#!/bin/sh\ntouch '{}'\nprintf '{{}}'\n", marker.display());
        std::fs::write(&path, script).expect("fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture executable");
        let supported = inspect_capability("2.1.263", &help(), true);
        let drift = inspect_capability("2.1.264", &help(), true);
        let agent =
            ClaudeCliReviewAgent::with_program_and_probe(supported, path.clone(), move || {
                drift.clone()
            });
        let request = RestrictedReviewRequest::new(
            ReviewExecutionIdentity::claude_cli("2.1.263"),
            None,
            "fixture input".into(),
        )
        .expect("request");
        assert_eq!(agent.run_review(request), Err(ReviewError::InvalidRequest));
        assert!(!marker.exists(), "drift must prevent launch");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(marker);
    }

    #[cfg(unix)]
    #[test]
    fn executable_drift_is_rejected_before_launch() {
        use std::os::unix::fs::PermissionsExt;
        let marker = std::env::temp_dir().join(format!(
            "alfred-review-executable-marker-{}",
            uuid::Uuid::new_v4()
        ));
        let path = std::env::temp_dir().join(format!(
            "alfred-review-executable-drift-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!("#!/bin/sh\ntouch '{}'\nprintf '{{}}'\n", marker.display());
        std::fs::write(&path, script).expect("fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture executable");
        let supported = inspect_capability("2.1.263", &help(), true);
        let expected = supported.clone();
        let agent = ClaudeCliReviewAgent::with_program_and_probes(
            supported,
            path.clone(),
            move || expected.clone(),
            || None,
        );
        let request = RestrictedReviewRequest::new(
            ReviewExecutionIdentity::claude_cli("2.1.263"),
            None,
            "fixture input".into(),
        )
        .expect("request");
        assert_eq!(agent.run_review(request), Err(ReviewError::InvalidRequest));
        assert!(!marker.exists(), "executable drift must prevent launch");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(marker);
    }

    #[test]
    fn fixed_command_never_places_prompt_in_argv() {
        let command = build_command("claude", "/tmp/review", Some("sonnet"));
        assert!(command
            .args
            .iter()
            .all(|arg| !arg.contains("private digest")));
        assert!(command.args.contains(&"--tools".into()));
        assert!(command.args.contains(&"".into()));
    }

    #[test]
    fn oversized_unterminated_output_is_rejected_before_parse() {
        let error =
            decode_output("x".repeat(33 * 1024).as_str(), 32 * 1024).expect_err("output limit");
        assert_eq!(error, ReviewError::ResponseTooLarge { limit: 32 * 1024 });
    }

    #[test]
    fn tool_events_are_rejected_without_a_dispatcher() {
        let error = decode_output(r#"{"type":"tool_use","name":"Bash"}"#, 32 * 1024)
            .expect_err("tool event");
        assert_eq!(error, ReviewError::ToolRequest);
    }

    #[test]
    fn silent_fixture_obeys_timeout_without_blocking_on_stdout() {
        let temp = std::env::temp_dir();
        let command = ClaudeReviewCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "sleep 1".into()],
            working_directory: temp,
        };
        let request = RestrictedReviewRequest::with_limits(
            ReviewExecutionIdentity::claude_cli("2.1.263"),
            None,
            "bounded review".into(),
            crate::agents::review::ReviewLimits {
                input_max_bytes: 64 * 1024,
                response_max_bytes: 32 * 1024,
                timeout: std::time::Duration::from_millis(50),
            },
        )
        .expect("request");
        let started = Instant::now();
        assert_eq!(invoke(&command, &request), Err(ReviewError::Timeout));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
}

#[cfg(all(test, unix))]
mod transport {
    use super::*;
    use crate::agents::review::{ReviewCancellation, ReviewLimits};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    static ENV_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn request(timeout: Duration, response_max_bytes: usize) -> RestrictedReviewRequest {
        RestrictedReviewRequest::with_limits(
            ReviewExecutionIdentity::claude_cli("2.1.263"),
            None,
            "stdin review payload".into(),
            ReviewLimits {
                input_max_bytes: 64 * 1024,
                response_max_bytes,
                timeout,
            },
        )
        .expect("fixture request")
    }

    fn shell(script: &str) -> ClaudeReviewCommand {
        ClaudeReviewCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            working_directory: std::env::temp_dir(),
        }
    }

    #[test]
    fn oversized_unterminated_stdout_is_stopped_while_streaming() {
        let started = Instant::now();
        let error = invoke(
            &shell("head -c 40000 /dev/zero"),
            &request(Duration::from_secs(2), 1024),
        )
        .expect_err("response cap");
        assert_eq!(error, ReviewError::ResponseTooLarge { limit: 1024 });
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn huge_stderr_is_drained_without_deadlock() {
        let response = invoke(
            &shell("head -c 100000 /dev/zero >&2; printf '{}'"),
            &request(Duration::from_secs(2), 1024),
        )
        .expect("stderr is bounded separately");
        assert_eq!(response.output, "{}");
    }

    #[test]
    fn descendant_holding_pipes_is_killed_after_parent_exit() {
        let started = Instant::now();
        let response = invoke(
            &shell("printf '{}'; sleep 5 & exit 0"),
            &request(Duration::from_secs(2), 1024),
        )
        .expect("parent result");
        assert_eq!(response.output, "{}");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn stdin_is_used_and_unlisted_environment_is_removed() {
        let _guard = ENV_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("environment test lock");
        struct EnvRestore(Option<std::ffi::OsString>);
        impl Drop for EnvRestore {
            fn drop(&mut self) {
                if let Some(value) = self.0.take() {
                    std::env::set_var("ALFRED_REVIEW_FORBIDDEN", value);
                } else {
                    std::env::remove_var("ALFRED_REVIEW_FORBIDDEN");
                }
            }
        }
        let _restore = EnvRestore(std::env::var_os("ALFRED_REVIEW_FORBIDDEN"));
        std::env::set_var("ALFRED_REVIEW_FORBIDDEN", "must-not-reach-review");
        let response = invoke(
            &shell("read value; test -z \"$ALFRED_REVIEW_FORBIDDEN\" && printf '%s' \"$value\""),
            &request(Duration::from_secs(2), 1024),
        )
        .expect("stdin fixture");
        assert_eq!(response.output, "stdin review payload");
    }

    #[test]
    fn cancellation_stops_a_full_output_channel() {
        let (cancellation, flag) = ReviewCancellation::new();
        let request = request(Duration::from_secs(2), 32 * 1024).with_cancellation(cancellation);
        let setter = thread::spawn(move || {
            thread::sleep(Duration::from_millis(1));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let error = invoke(&shell("yes x"), &request).expect_err("cancelled fixture");
        setter.join().expect("setter");
        assert_eq!(error, ReviewError::Cancelled);
    }

    #[test]
    fn nested_tool_event_terminates_before_silence_timeout() {
        let started = Instant::now();
        let error = invoke(
            &shell("printf '%s' '{\"events\":[{\"type\":\"tool_use\"}]}'; sleep 5"),
            &request(Duration::from_secs(2), 1024),
        )
        .expect_err("nested tool event");
        assert_eq!(error, ReviewError::ToolRequest);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
