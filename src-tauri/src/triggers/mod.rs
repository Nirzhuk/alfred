//! Event triggers — workflows started by something other than a human or a clock.
//!
//! Two sources ship today:
//!   * `file`    — a path on disk changed (save, commit, build artifact).
//!   * `webhook` — an HTTP POST to the local listener (Slack, GitHub, curl, scripts).
//!
//! Both land in the same place: [`fire`] starts a normal run with
//! `RunTrigger::Event`, so nothing in the runner had to learn about triggers.
//! There is deliberately no `TriggerSource` trait — two implementations don't
//! justify one; extract it if a third arrives.

pub mod app;
pub mod file;
pub mod http;

use crate::db::{Db, Trigger};
use crate::runner::{start_run, RunTrigger};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// Live watcher handles + the bound HTTP port. Held in Tauri state so file
/// watchers can be rebuilt when triggers change.
#[derive(Default)]
pub struct TriggerRuntime {
    ready: AtomicBool,
    watchers: Mutex<Vec<notify::RecommendedWatcher>>,
    http_port: Mutex<Option<u16>>,
}

impl TriggerRuntime {
    pub(crate) fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub fn http_port(&self) -> Option<u16> {
        *self.http_port.lock().ok()?
    }

    pub(crate) fn set_http_port(&self, port: u16) {
        if let Ok(mut slot) = self.http_port.lock() {
            *slot = Some(port);
        }
    }

    pub(crate) fn replace_watchers(&self, next: Vec<notify::RecommendedWatcher>) {
        if let Ok(mut watchers) = self.watchers.lock() {
            // Dropping the old watchers unregisters them.
            *watchers = next;
        }
    }
}

/// Start a run for `trigger`, carrying the event data into the prompt.
pub fn fire(app: &AppHandle, db: &Db, trigger: &Trigger, payload: Value) -> Result<String, String> {
    let payload_json = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());

    let summary = start_run(
        app.clone(),
        db,
        &trigger.workflow_id,
        RunTrigger::Event(trigger.source.clone()),
        Some(&payload_json),
    )
    .map_err(|e| e.to_string())?;

    let _ = db.mark_trigger_fired(&trigger.id);
    let _ = app.emit(
        "trigger://fired",
        serde_json::json!({
            "triggerId": trigger.id,
            "workflowId": trigger.workflow_id,
            "source": trigger.source,
            "runId": summary.id,
        }),
    );

    Ok(summary.id)
}

/// (Re)bind file watchers to the current enabled triggers. The HTTP listener
/// needs no reload — it resolves triggers from the DB per request.
pub fn reload(app: &AppHandle) -> Result<usize, String> {
    // Trigger edits during startup are persisted, then picked up by the
    // initial reload after managed runtimes finish verification.
    if app
        .try_state::<TriggerRuntime>()
        .is_some_and(|runtime| !runtime.ready.load(Ordering::Acquire))
    {
        return Ok(0);
    }
    let watchers = file::reload_watchers(app)?;
    if let Some(runtime) = app.try_state::<app::AppEventRuntime>() {
        runtime.reload();
    }
    if let Some(integrations) = app.try_state::<crate::integrations::IntegrationsState>() {
        integrations.events.reset();
    }
    Ok(watchers)
}
