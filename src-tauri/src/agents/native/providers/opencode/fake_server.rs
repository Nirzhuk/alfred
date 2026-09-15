//! Deterministic HTTP/SSE fixture for the pinned OpenCode V1 surface.

use super::{
    HttpOpenCodeApi, OpenCodeApi, OpenCodeServerPassword, OpenCodeServerProvider,
    OpenCodeServerSession, OpenCodeServerState,
};
use crate::agent_accounts::runtime_profile::RuntimeProfileRef;
use crate::agents::native::{NativeCancellation, NativeRuntimeError};
use crate::agents::OpaqueAgentAccountRef;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use zeroize::Zeroize;

pub const FIXTURE_PASSWORD: &str = "fixture-supervisor-password-00000001";
pub const FIXTURE_GO_KEY: &str = "opencode-go-fixture-key-never-log";
pub const FIXTURE_SESSION_ID: &str = "session_fixture";

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub body: Value,
}

struct State {
    repository: String,
    catalog: Value,
    events: Vec<Value>,
    event_delay: Duration,
    requests: Mutex<Vec<RecordedRequest>>,
    prompt_seen: Mutex<bool>,
    prompt_ready: Condvar,
    key_match_count: Mutex<usize>,
    forced_status: Mutex<BTreeMap<String, u16>>,
}

pub struct FakeOpenCodeSidecar {
    address: SocketAddr,
    state: Arc<State>,
    shutdown: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl FakeOpenCodeSidecar {
    pub fn start(repository: String, catalog: Value, events: Vec<Value>) -> Self {
        Self::start_delayed(repository, catalog, events, Duration::ZERO)
    }

    pub fn start_delayed(
        repository: String,
        catalog: Value,
        events: Vec<Value>,
        event_delay: Duration,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        listener
            .set_nonblocking(true)
            .expect("fixture nonblocking listener");
        let address = listener.local_addr().expect("fixture address");
        let state = Arc::new(State {
            repository,
            catalog,
            events,
            event_delay,
            requests: Mutex::new(Vec::new()),
            prompt_seen: Mutex::new(false),
            prompt_ready: Condvar::new(),
            key_match_count: Mutex::new(0),
            forced_status: Mutex::new(BTreeMap::new()),
        });
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_shutdown = Arc::clone(&shutdown);
        let accept_thread = thread::spawn(move || {
            while !thread_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // BSD/macOS sockets inherit the listener's nonblocking
                        // mode; handlers rely on blocking reads with timeouts.
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        let state = Arc::clone(&thread_state);
                        thread::spawn(move || handle(stream, state));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            state,
            shutdown,
            accept_thread: Some(accept_thread),
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.requests.lock().expect("requests").clone()
    }

    pub fn key_match_count(&self) -> usize {
        *self.state.key_match_count.lock().expect("key matches")
    }

    pub fn force_status(&self, path: &str, status: u16) {
        self.state
            .forced_status
            .lock()
            .expect("statuses")
            .insert(path.into(), status);
    }

    pub fn wait_for_prompt(&self) {
        let prompt = self.state.prompt_seen.lock().expect("prompt");
        let _guard = self
            .state
            .prompt_ready
            .wait_timeout_while(prompt, Duration::from_secs(3), |seen| !*seen)
            .expect("prompt wait")
            .0;
    }
}

impl Drop for FakeOpenCodeSidecar {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
    }
}

fn handle(mut stream: TcpStream, state: Arc<State>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let Some(mut request) = read_request(&mut stream) else {
        return;
    };
    let expected = format!(
        "Basic {}",
        STANDARD.encode(format!("opencode:{FIXTURE_PASSWORD}"))
    );
    if request.authorization.as_deref() != Some(expected.as_str()) {
        let _ = write_json(&mut stream, 401, &json!({"error": "unauthorized"}));
        return;
    }

    let path = request.target.split('?').next().unwrap_or(&request.target);
    let mut parsed = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
    if request.method == "PUT" && path == "/auth/opencode-go" {
        if parsed.get("type").and_then(Value::as_str) == Some("api")
            && parsed.get("key").and_then(Value::as_str) == Some(FIXTURE_GO_KEY)
        {
            *state.key_match_count.lock().expect("key matches") += 1;
        }
        redact_auth_body(&mut parsed);
        request.body.zeroize();
    }
    state
        .requests
        .lock()
        .expect("requests")
        .push(RecordedRequest {
            method: request.method.clone(),
            path: path.into(),
            body: parsed,
        });

    if let Some(status) = state
        .forced_status
        .lock()
        .expect("statuses")
        .get(path)
        .copied()
    {
        let _ = write_json(&mut stream, status, &json!({"error": "fixture"}));
        return;
    }

    match (request.method.as_str(), path) {
        ("PUT", "/auth/opencode-go") | ("DELETE", "/auth/opencode-go") => {
            let _ = write_json(&mut stream, 200, &Value::Bool(true));
        }
        ("GET", "/provider") => {
            let _ = write_json(&mut stream, 200, &state.catalog);
        }
        ("POST", "/session") => {
            let _ = write_json(
                &mut stream,
                200,
                &json!({"id": FIXTURE_SESSION_ID, "directory": state.repository.as_str()}),
            );
        }
        ("GET", path) if path == format!("/session/{FIXTURE_SESSION_ID}") => {
            let _ = write_json(
                &mut stream,
                200,
                &json!({"id": FIXTURE_SESSION_ID, "directory": state.repository.as_str()}),
            );
        }
        ("DELETE", path) if path == format!("/session/{FIXTURE_SESSION_ID}") => {
            let _ = write_json(&mut stream, 200, &Value::Bool(true));
        }
        ("POST", path) if path == format!("/session/{FIXTURE_SESSION_ID}/prompt_async") => {
            *state.prompt_seen.lock().expect("prompt") = true;
            state.prompt_ready.notify_all();
            let _ = write_empty(&mut stream, 204);
        }
        ("POST", path) if path == format!("/session/{FIXTURE_SESSION_ID}/abort") => {
            let _ = write_json(&mut stream, 200, &Value::Bool(true));
        }
        ("POST", path) if path.starts_with("/permission/") && path.ends_with("/reply") => {
            let _ = write_json(&mut stream, 200, &Value::Bool(true));
        }
        ("GET", "/event") => write_events(&mut stream, &state),
        _ => {
            let _ = write_json(&mut stream, 404, &json!({"error": "not found"}));
        }
    }
}

struct Request {
    method: String,
    target: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() > 64 * 1024 {
            return None;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let mut lines = header.split("\r\n");
    let mut first = lines.next()?.split_ascii_whitespace();
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let mut authorization = None;
    let mut content_length = 0_usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().into());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().ok()?;
        }
    }
    while bytes.len().saturating_sub(header_end) < content_length {
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Some(Request {
        method,
        target,
        authorization,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn redact_auth_body(body: &mut Value) {
    if let Some(object) = body.as_object_mut() {
        if let Some(Value::String(key)) = object.get_mut("key") {
            key.zeroize();
            *key = "[REDACTED]".into();
        }
    }
}

fn write_events(stream: &mut TcpStream, state: &State) {
    if stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .is_err()
    {
        return;
    }
    let _ = stream.flush();
    let prompt = state.prompt_seen.lock().expect("prompt");
    let (prompt, _) = state
        .prompt_ready
        .wait_timeout_while(prompt, Duration::from_secs(3), |seen| !*seen)
        .expect("prompt wait");
    if !*prompt {
        return;
    }
    drop(prompt);
    if !state.event_delay.is_zero() {
        thread::sleep(state.event_delay);
    }
    for event in &state.events {
        let Ok(value) = serde_json::to_vec(event) else {
            return;
        };
        if stream.write_all(b"data: ").is_err()
            || stream.write_all(&value).is_err()
            || stream.write_all(b"\n\n").is_err()
            || stream.flush().is_err()
        {
            return;
        }
    }
}

fn write_json(stream: &mut TcpStream, status: u16, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value).expect("fixture JSON");
    write_response(stream, status, "application/json", &body)
}

fn write_empty(stream: &mut TcpStream, status: u16) -> std::io::Result<()> {
    write_response(stream, status, "application/json", &[])
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        401 => "Unauthorized",
        402 => "Payment Required",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Fixture",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

pub fn fixture_go_catalog() -> Value {
    json!({
        "all": [
            {
                "id": "opencode-go",
                "name": "OpenCode Go",
                "models": {
                    "model-a": {
                        "id": "model-a",
                        "providerID": "opencode-go",
                        "name": "Go Model A"
                    }
                }
            },
            {
                "id": "zen",
                "name": "OpenCode Zen",
                "models": {
                    "zen-model": {
                        "id": "zen-model",
                        "providerID": "zen",
                        "name": "Must Never Escape"
                    }
                }
            }
        ],
        "connected": ["opencode-go", "zen"]
    })
}

struct FixtureSession {
    api: HttpOpenCodeApi,
    state: Arc<Mutex<OpenCodeServerState>>,
    stopped: Arc<AtomicBool>,
}

impl OpenCodeServerSession for FixtureSession {
    fn api(&self) -> &dyn OpenCodeApi {
        &self.api
    }

    fn state(&self) -> OpenCodeServerState {
        *self.state.lock().expect("state")
    }

    fn stop(&self) -> Result<(), NativeRuntimeError> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

pub struct FixtureOpenCodeProvider {
    address: SocketAddr,
    profile_ref: RuntimeProfileRef,
    state: Arc<Mutex<OpenCodeServerState>>,
    stopped: Arc<AtomicBool>,
    purges: AtomicUsize,
}

impl FixtureOpenCodeProvider {
    pub fn new(sidecar: &FakeOpenCodeSidecar) -> Self {
        Self {
            address: sidecar.address(),
            profile_ref: RuntimeProfileRef::parse(
                "runtime_profile_0123456789abcdef0123456789abcdef",
            )
            .expect("fixture profile"),
            state: Arc::new(Mutex::new(OpenCodeServerState::Active)),
            stopped: Arc::new(AtomicBool::new(false)),
            purges: AtomicUsize::new(0),
        }
    }

    pub fn purges(&self) -> usize {
        self.purges.load(Ordering::SeqCst)
    }

    fn session(&self) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        Ok(Box::new(FixtureSession {
            api: HttpOpenCodeApi::new(
                self.address,
                OpenCodeServerPassword::new(FIXTURE_PASSWORD.into())?,
            )?,
            state: Arc::clone(&self.state),
            stopped: Arc::clone(&self.stopped),
        }))
    }
}

impl OpenCodeServerProvider for FixtureOpenCodeProvider {
    fn create_and_launch(
        &self,
        _account_ref: &OpaqueAgentAccountRef,
        _repository: &Path,
        _cancellation: &NativeCancellation,
    ) -> Result<(RuntimeProfileRef, Box<dyn OpenCodeServerSession>), NativeRuntimeError> {
        Ok((self.profile_ref.clone(), self.session()?))
    }

    fn launch_existing(
        &self,
        _account_ref: &OpaqueAgentAccountRef,
        _profile_ref: &RuntimeProfileRef,
        _repository: &Path,
        _cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        self.session()
    }

    fn purge_profile(
        &self,
        _account_ref: &OpaqueAgentAccountRef,
        _profile_ref: &RuntimeProfileRef,
    ) -> Result<(), NativeRuntimeError> {
        self.purges.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
