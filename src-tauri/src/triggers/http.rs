//! Local webhook listener — the "custom trigger" escape hatch.
//!
//! `POST http://127.0.0.1:<port>/hooks/<triggerId>` with the trigger's token
//! starts a run and hands the request body to the agent as the trigger payload.
//! Anything that can make an HTTP request (curl, a git hook, Zapier via a
//! tunnel, another script) becomes a trigger source without new Rust code.
//!
//! Bound to loopback on purpose: this process holds the user's agent
//! credentials, so it is not something to expose on 0.0.0.0. Reaching it from
//! outside is a deliberate act (ssh tunnel / ngrok), not the default.

use crate::db::Db;
use serde_json::Value;
use std::io::Read;
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Request, Response, Server};

/// Bodies above this are rejected rather than buffered.
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub fn configured_port() -> u16 {
    std::env::var("ALFRED_HTTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| if crate::app_identity::is_development() { 8788 } else { 8787 })
}

/// Bind the listener and serve it on a background thread. Returns the bound port.
pub fn start(app: AppHandle) -> Result<u16, String> {
    let requested = configured_port();
    let server = Server::http(("127.0.0.1", requested))
        .map_err(|e| format!("could not bind 127.0.0.1:{requested}: {e}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .unwrap_or(requested);

    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let (status, body) = handle(&app, &mut request);
            let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                .expect("static header");
            let _ = request.respond(
                Response::from_string(body)
                    .with_status_code(status)
                    .with_header(header),
            );
        }
    });

    Ok(port)
}

fn json_error(message: &str) -> String {
    serde_json::json!({ "ok": false, "error": message }).to_string()
}

/// Length-first, difference-accumulating compare so a wrong token can't be
/// discovered byte by byte from response timing.
fn secret_matches(expected: &str, provided: &str) -> bool {
    if expected.len() != provided.len() {
        return false;
    }
    expected
        .bytes()
        .zip(provided.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn split_target(url: &str) -> (&str, Option<&str>) {
    match url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (url, None),
    }
}

fn token_from(request: &Request, query: Option<&str>) -> Option<String> {
    for header in request.headers() {
        if header.field.equiv("X-Alfred-Token") {
            return Some(header.value.as_str().to_string());
        }
        if header.field.equiv("Authorization") {
            if let Some(rest) = header.value.as_str().strip_prefix("Bearer ") {
                return Some(rest.trim().to_string());
            }
        }
    }

    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "token")
        .map(|(_, value)| value.to_string())
}

fn read_body(request: &mut Request) -> Result<String, &'static str> {
    if request
        .body_length()
        .is_some_and(|len| len > MAX_BODY_BYTES)
    {
        return Err("payload too large");
    }

    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_string(&mut body)
        .map_err(|_| "body is not valid UTF-8")?;

    if body.len() > MAX_BODY_BYTES {
        return Err("payload too large");
    }
    Ok(body)
}

fn handle(app: &AppHandle, request: &mut Request) -> (u16, String) {
    let (path, query) = split_target(request.url());
    let query = query.map(str::to_string);
    let method = request.method().as_str().to_uppercase();

    if path == "/health" {
        return (200, serde_json::json!({ "ok": true }).to_string());
    }

    let Some(trigger_id) = path.strip_prefix("/hooks/").filter(|id| !id.is_empty()) else {
        return (
            404,
            json_error("unknown path — use POST /hooks/<triggerId>"),
        );
    };
    let trigger_id = trigger_id.to_string();

    if method != "POST" {
        return (405, json_error("use POST"));
    }

    let Some(db) = app.try_state::<Db>() else {
        return (503, json_error("database unavailable"));
    };

    let trigger = match db.get_trigger(&trigger_id) {
        Ok(Some(trigger)) if trigger.source == "webhook" => trigger,
        // Same answer for missing and wrong-source so probing learns nothing.
        Ok(_) => return (404, json_error("no such webhook trigger")),
        Err(e) => return (500, json_error(&e.to_string())),
    };

    let provided = token_from(request, query.as_deref()).unwrap_or_default();
    let expected = trigger.secret.clone().unwrap_or_default();
    if expected.is_empty() || !secret_matches(&expected, &provided) {
        return (401, json_error("invalid or missing token"));
    }

    if !trigger.enabled {
        return (409, json_error("trigger is disabled"));
    }

    let body = match read_body(request) {
        Ok(body) => body,
        Err(message) => return (413, json_error(message)),
    };

    // Structured bodies stay structured; anything else rides as raw text.
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::String(body));
    let payload = serde_json::json!({
        "source": "webhook",
        "triggerId": trigger.id,
        "body": parsed,
    });

    match super::fire(app, db.inner(), &trigger, payload) {
        Ok(run_id) => (
            202,
            serde_json::json!({ "ok": true, "runId": run_id }).to_string(),
        ),
        Err(e) => (500, json_error(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_compare_rejects_mismatches() {
        assert!(secret_matches("abc123", "abc123"));
        assert!(!secret_matches("abc123", "abc124"));
        assert!(!secret_matches("abc123", "abc12"));
        assert!(!secret_matches("", "a"));
    }

    #[test]
    fn target_splitting() {
        assert_eq!(split_target("/hooks/a"), ("/hooks/a", None));
        assert_eq!(
            split_target("/hooks/a?token=x&y=1"),
            ("/hooks/a", Some("token=x&y=1"))
        );
    }
}
