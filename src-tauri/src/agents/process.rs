//! Shared helpers for spawning local agent CLIs.

use directories::BaseDirs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use super::active::RunControl;

const OUTPUT_CHANNEL_CAPACITY: usize = 256;
const MAX_STDOUT_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const MAX_STDERR_CAPTURE_BYTES: usize = 1024 * 1024;
const CAPTURE_TRUNCATED_MARKER: &str = "[... earlier process output omitted ...]\n";

struct BoundedCapture {
    text: String,
    max_bytes: usize,
    truncated: bool,
}

impl BoundedCapture {
    fn new(max_bytes: usize) -> Self {
        Self {
            text: String::new(),
            max_bytes,
            truncated: false,
        }
    }

    fn push_line(&mut self, line: &str) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(line);

        if self.text.len() <= self.max_bytes {
            return;
        }

        self.truncated = true;
        // Drop a chunk instead of a few bytes on every line once full. Keeping
        // the tail preserves terminal JSON/result records from streaming CLIs.
        let target_bytes = self.max_bytes.saturating_mul(3) / 4;
        let mut start = self.text.len().saturating_sub(target_bytes);
        while start < self.text.len() && !self.text.is_char_boundary(start) {
            start += 1;
        }
        if let Some(newline) = self.text[start..].find('\n') {
            start += newline + 1;
        }
        self.text.drain(..start);
    }

    fn finish(mut self) -> String {
        if !self.truncated {
            return self.text;
        }

        let available = self
            .max_bytes
            .saturating_sub(CAPTURE_TRUNCATED_MARKER.len());
        if self.text.len() > available {
            let mut start = self.text.len() - available;
            while start < self.text.len() && !self.text.is_char_boundary(start) {
                start += 1;
            }
            self.text.drain(..start);
        }
        self.text.insert_str(0, CAPTURE_TRUNCATED_MARKER);
        self.text
    }
}

pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    /// `None` when the process was killed by a signal or could not be waited on.
    pub code: Option<i32>,
    pub duration_ms: u128,
}

/// Claude Code stores subscription OAuth credentials in one shared macOS
/// Keychain item. Its headless processes can race while rotating that item,
/// invalidating every active session. Keep all Claude processes launched by
/// Alfred mutually exclusive until Claude Code makes Keychain refreshes
/// atomic upstream.
fn claude_invocation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn lock_claude_invocation(
    control: Option<&RunControl>,
) -> Result<MutexGuard<'static, ()>, String> {
    loop {
        match claude_invocation_lock().try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if control.is_some_and(RunControl::is_cancelled) {
                    return Err("cancelled".into());
                }
                thread::sleep(Duration::from_millis(40));
            }
        }
    }
}

pub fn find_bin(name: &str) -> Option<PathBuf> {
    if let Ok(output) = Command::new("which").arg(name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let p = PathBuf::from(path);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    let mut candidates = Vec::new();
    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        candidates.push(home.join(format!(".local/bin/{name}")));
        candidates.push(home.join(format!(".bun/bin/{name}")));
        candidates.push(home.join(format!(".opencode/bin/{name}")));
        candidates.push(home.join(format!(".codex/bin/{name}")));
        candidates.push(home.join(format!(".cargo/bin/{name}")));
        candidates.push(home.join(format!("bin/{name}")));
        if name == "codex" {
            candidates.push(home.join("Applications/ChatGPT.app/Contents/Resources/codex"));
        }
    }
    if name == "codex" {
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex",
        ));
    }
    candidates.push(PathBuf::from(format!("/opt/homebrew/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/local/bin/{name}")));
    candidates.push(PathBuf::from(format!("/usr/bin/{name}")));

    candidates.into_iter().find(|p| p.is_file())
}

fn enrich_path(command: &mut Command) {
    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        let extras = [
            home.join(".local/bin"),
            home.join(".bun/bin"),
            home.join(".opencode/bin"),
            home.join(".cargo/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ];
        let mut path = std::env::var("PATH").unwrap_or_default();
        for extra in extras {
            let s = extra.to_string_lossy();
            if !path.split(':').any(|p| p == s) {
                path = format!("{s}:{path}");
            }
        }
        command.env("PATH", path);
    }
}

fn configure_agent_environment(command: &mut Command, bin: &Path) {
    if bin.file_name().and_then(|name| name.to_str()) == Some("claude") {
        // A workflow should never replace the CLI executable underneath
        // itself. Updates still happen from normal interactive Claude use or
        // an explicit `claude update`.
        command.env("DISABLE_AUTOUPDATER", "1");
    }
}

/// Spawn a CLI, stream stdout/stderr line-by-line, honor cancel/timeout.
pub fn run_cmd(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
) -> Result<CmdOutput, String> {
    run_cmd_inner(bin, args, cwd, timeout, control, on_line, None, &[])
}

/// Variant used by custom agents that accept their prompt over stdin.
pub fn run_cmd_with_stdin(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
    stdin_payload: &str,
) -> Result<CmdOutput, String> {
    run_cmd_inner(
        bin,
        args,
        cwd,
        timeout,
        control,
        on_line,
        Some(stdin_payload),
        &[],
    )
}

/// Variant used by Script steps: a stdin payload plus explicit environment
/// variables. Both carry upstream output without shell interpolation.
pub fn run_cmd_with_stdin_env(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
    stdin_payload: &str,
    envs: &[(&str, String)],
) -> Result<CmdOutput, String> {
    run_cmd_inner(
        bin,
        args,
        cwd,
        timeout,
        control,
        on_line,
        Some(stdin_payload),
        envs,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_cmd_inner(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    control: Option<&RunControl>,
    on_line: Option<&dyn Fn(&str)>,
    stdin_payload: Option<&str>,
    envs: &[(&str, String)],
) -> Result<CmdOutput, String> {
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin_payload.is_some() {
        command.stdin(Stdio::piped());
    }

    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    enrich_path(&mut command);
    configure_agent_environment(&mut command, bin);
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("missing stdout for {}", bin.display()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("missing stderr for {}", bin.display()))?;

    // Own the child either in RunControl (cancelable) or a local slot.
    let local_slot: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    if let Some(ctrl) = control {
        ctrl.set_child(child);
    } else if let Ok(mut slot) = local_slot.lock() {
        *slot = Some(child);
    }

    let cancel = control
        .map(|c| Arc::clone(&c.cancel))
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

    let stdout_buf = Arc::new(Mutex::new(BoundedCapture::new(MAX_STDOUT_CAPTURE_BYTES)));
    let stderr_buf = Arc::new(Mutex::new(BoundedCapture::new(MAX_STDERR_CAPTURE_BYTES)));
    let stdout_acc = Arc::clone(&stdout_buf);
    let stderr_acc = Arc::clone(&stderr_buf);

    // Backpressure prevents a noisy child from duplicating an arbitrary amount
    // of unread output in Alfred's heap.
    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(OUTPUT_CHANNEL_CAPACITY);
    let tx_out = tx.clone();
    let tx_err = tx.clone();
    drop(tx);

    let t_out = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().flatten() {
            if let Ok(mut buf) = stdout_acc.lock() {
                buf.push_line(&line);
            }
            let _ = tx_out.send(line);
        }
    });

    let t_err = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().flatten() {
            if let Ok(mut buf) = stderr_acc.lock() {
                buf.push_line(&line);
            }
            let _ = tx_err.send(line);
        }
    });

    let start = Instant::now();
    // Keep this thread consuming the bounded output channel and checking the
    // deadline even when the child stops reading its stdin pipe.
    let (mut t_stdin, mut stdin_error) = match (stdin_payload, stdin) {
        (Some(payload), Some(mut stdin)) => {
            let payload = payload.as_bytes().to_vec();
            (Some(thread::spawn(move || stdin.write_all(&payload))), None)
        }
        (Some(_), None) => (None, Some(format!("missing stdin for {}", bin.display()))),
        (None, _) => (None, None),
    };

    let mut cancelled = false;
    let mut timed_out = false;

    loop {
        for _ in 0..OUTPUT_CHANNEL_CAPACITY {
            let Ok(line) = rx.try_recv() else {
                break;
            };
            if let Some(cb) = on_line {
                cb(&line);
            }
        }

        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            kill_child(control, &local_slot);
            break;
        }

        if start.elapsed() > timeout {
            timed_out = true;
            kill_child(control, &local_slot);
            break;
        }

        if let Some(writer) = t_stdin.take_if(|writer| writer.is_finished()) {
            stdin_error = finish_stdin_write(writer);
        }
        if stdin_error.is_some() {
            kill_child(control, &local_slot);
            break;
        }

        let child_done = child_exited(control, &local_slot);
        if child_done && t_stdin.is_none() && t_out.is_finished() && t_err.is_finished() {
            break;
        }

        thread::sleep(Duration::from_millis(40));
    }

    // A bounded sender may be waiting for capacity. Continue consuming while
    // the pipe readers finish so joining them cannot deadlock.
    while !t_out.is_finished() || !t_err.is_finished() {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(40)) {
            if let Some(cb) = on_line {
                cb(&line);
            }
        }
    }
    while let Ok(line) = rx.try_recv() {
        if let Some(cb) = on_line {
            cb(&line);
        }
    }

    let _ = t_out.join();
    let _ = t_err.join();
    if let Some(writer) = t_stdin {
        stdin_error = finish_stdin_write(writer);
    }

    let (status_ok, exit_code) = wait_child(control, &local_slot);

    if cancelled {
        return Err("cancelled".into());
    }
    if timed_out {
        return Err(format!(
            "{} timed out after {}s",
            bin.display(),
            timeout.as_secs()
        ));
    }
    if let Some(error) = stdin_error {
        return Err(error);
    }

    let stdout = take_capture(stdout_buf);
    let stderr = take_capture(stderr_buf);

    Ok(CmdOutput {
        stdout,
        stderr,
        success: status_ok,
        code: exit_code,
        duration_ms: start.elapsed().as_millis(),
    })
}

fn finish_stdin_write(writer: thread::JoinHandle<std::io::Result<()>>) -> Option<String> {
    match writer.join() {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(format!("failed to write prompt to stdin: {error}")),
        Err(_) => Some("stdin writer thread panicked".into()),
    }
}

fn take_capture(capture: Arc<Mutex<BoundedCapture>>) -> String {
    match Arc::try_unwrap(capture) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(capture) => capture.finish(),
            Err(poisoned) => poisoned.into_inner().finish(),
        },
        Err(capture) => capture
            .lock()
            .map(|value| value.text.clone())
            .unwrap_or_default(),
    }
}

fn kill_child(control: Option<&RunControl>, local_slot: &Arc<Mutex<Option<std::process::Child>>>) {
    if let Some(ctrl) = control {
        ctrl.request_cancel();
        return;
    }
    if let Ok(mut slot) = local_slot.lock() {
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn child_exited(
    control: Option<&RunControl>,
    local_slot: &Arc<Mutex<Option<std::process::Child>>>,
) -> bool {
    if let Some(ctrl) = control {
        if let Ok(mut slot) = ctrl.child.lock() {
            return match slot.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
                None => true,
            };
        }
        return false;
    }
    if let Ok(mut slot) = local_slot.lock() {
        return match slot.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
            None => true,
        };
    }
    false
}

/// Returns `(succeeded, exit_code)`. The code is `None` when the child was
/// signalled or could not be waited on.
fn wait_child(
    control: Option<&RunControl>,
    local_slot: &Arc<Mutex<Option<std::process::Child>>>,
) -> (bool, Option<i32>) {
    fn finish(child: Option<std::process::Child>) -> (bool, Option<i32>) {
        match child {
            Some(mut child) => match child.wait() {
                Ok(status) => (status.success(), status.code()),
                Err(_) => (false, None),
            },
            None => (false, None),
        }
    }

    if let Some(ctrl) = control {
        return finish(ctrl.take_child());
    }
    if let Ok(mut slot) = local_slot.lock() {
        return finish(slot.take());
    }
    (false, None)
}

pub fn cwd_from_request(working_directory: &Option<String>) -> Option<PathBuf> {
    working_directory
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

pub fn prefer_stdout(output: &CmdOutput) -> String {
    let stdout = output.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }
    output.stderr.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::Read;
    use std::sync::mpsc;

    const STDIN_FIXTURE_ENV: &str = "ALFRED_PROCESS_STDIN_TEST_MODE";
    const STDIN_FIXTURE_BYTES: usize = 2 * 1024 * 1024;

    fn run_stdin_fixture(
        mode: &str,
        timeout: Duration,
        control: Option<&RunControl>,
        on_line: Option<&dyn Fn(&str)>,
    ) -> Result<CmdOutput, String> {
        run_cmd_with_stdin_env(
            &std::env::current_exe().expect("test executable"),
            &[
                "--exact".into(),
                "agents::process::tests::stdin_process_fixture_child".into(),
                "--nocapture".into(),
            ],
            None,
            timeout,
            control,
            on_line,
            &"x".repeat(STDIN_FIXTURE_BYTES),
            &[(STDIN_FIXTURE_ENV, mode.into())],
        )
    }

    #[test]
    fn stdin_process_fixture_child() {
        let Ok(mode) = std::env::var(STDIN_FIXTURE_ENV) else {
            return;
        };

        // This process has no descendants. Even a regression that blocks all
        // pipe I/O must end; dropping the sender cancels and joins the watchdog.
        thread::scope(|scope| {
            let (_finished, finish_rx) = mpsc::channel::<()>();
            scope.spawn(move || {
                if matches!(
                    finish_rx.recv_timeout(Duration::from_secs(8)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    std::process::exit(91);
                }
            });

            let mut stdout = std::io::stdout().lock();
            writeln!(stdout).expect("fixture output boundary");
            match mode.as_str() {
                "output-before-input" => {
                    let line = format!("fixture-output-{}", "y".repeat(4096));
                    for _ in 0..1024 {
                        writeln!(stdout, "{line}").expect("fixture output");
                    }
                    stdout.flush().expect("flush fixture output");

                    let mut input = Vec::new();
                    std::io::stdin()
                        .read_to_end(&mut input)
                        .expect("fixture input");
                    assert_eq!(input.len(), STDIN_FIXTURE_BYTES);
                    assert!(input.iter().all(|byte| *byte == b'x'));
                    writeln!(stdout, "fixture-input-complete").expect("fixture result");
                }
                "wait-before-input" => {
                    writeln!(stdout, "fixture-ready").expect("fixture readiness");
                    stdout.flush().expect("flush fixture readiness");
                    thread::sleep(Duration::from_secs(10));
                }
                "exit-before-input" => {}
                _ => panic!("unknown stdin fixture mode"),
            }
        });
    }

    #[test]
    fn drains_output_while_writing_large_stdin() {
        let output_lines = Cell::new(0);
        let supervisor = thread::current().id();
        let on_line = |line: &str| {
            assert_eq!(thread::current().id(), supervisor);
            if line.starts_with("fixture-output-") {
                output_lines.set(output_lines.get() + 1);
            }
        };

        let output = run_stdin_fixture(
            "output-before-input",
            Duration::from_secs(4),
            None,
            Some(&on_line),
        )
        .expect("large input and output should both drain");

        assert!(output.success);
        assert_eq!(output_lines.get(), 1024);
        assert!(output.stdout.contains("fixture-input-complete"));
    }

    #[test]
    fn timeout_interrupts_a_blocked_stdin_write() {
        let start = Instant::now();
        let result = run_stdin_fixture("wait-before-input", Duration::from_millis(200), None, None);

        assert!(matches!(result, Err(ref error) if error.contains("timed out")));
        assert!(start.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn cancellation_interrupts_a_blocked_stdin_write() {
        let control = RunControl::new();
        let on_line = |line: &str| {
            if line == "fixture-ready" {
                control.request_cancel();
            }
        };
        let start = Instant::now();
        let result = run_stdin_fixture(
            "wait-before-input",
            Duration::from_secs(4),
            Some(&control),
            Some(&on_line),
        );

        assert!(matches!(result, Err(ref error) if error == "cancelled"));
        assert!(start.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn reports_a_stdin_write_failure_without_cancellation() {
        let result = run_stdin_fixture(
            "exit-before-input",
            Duration::from_secs(4),
            Some(&RunControl::new()),
            None,
        );

        assert!(
            matches!(result, Err(ref error) if error.starts_with("failed to write prompt to stdin:"))
        );
    }

    #[test]
    fn reports_a_stdin_writer_panic() {
        let writer = thread::spawn(|| panic!("fixture stdin writer panic"));

        assert_eq!(
            finish_stdin_write(writer).as_deref(),
            Some("stdin writer thread panicked")
        );
    }

    #[test]
    fn serializes_claude_invocations() {
        let first = lock_claude_invocation(None).expect("first lock");
        let (sender, receiver) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let _second = lock_claude_invocation(None).expect("second lock");
            sender.send(()).expect("notify lock acquired");
        });

        assert!(receiver.recv_timeout(Duration::from_millis(80)).is_err());
        drop(first);
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("waiter should acquire released lock");
        waiter.join().expect("waiter thread");
    }

    #[test]
    fn bounded_capture_keeps_recent_output_within_its_budget() {
        let mut capture = BoundedCapture::new(96);
        for index in 0..20 {
            capture.push_line(&format!("event-{index:02}-abcdefghij"));
        }

        let output = capture.finish();
        assert!(output.len() <= 96);
        assert!(output.starts_with(CAPTURE_TRUNCATED_MARKER));
        assert!(output.contains("event-19"));
        assert!(!output.contains("event-00"));
    }

    #[cfg(unix)]
    #[test]
    fn shared_process_path_writes_stdin_and_captures_output() {
        let output = run_cmd_with_stdin(
            Path::new("/bin/sh"),
            &["-c".into(), "cat".into()],
            None,
            Duration::from_secs(2),
            None,
            None,
            "hello from stdin\n",
        )
        .expect("stdin command should run");

        assert!(output.success);
        assert_eq!(output.stdout.trim(), "hello from stdin");
    }
}
