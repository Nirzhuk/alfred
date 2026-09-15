//! Local connected-app event runtime.
//!
//! The loop is intentionally separate from file/webhook triggers. It schedules
//! provider adapters, persists backoff/health, and drains the durable event
//! queue only when the workflow's single-run slot is free.

use crate::agents::active;
use crate::db::{Db, Trigger};
use crate::integrations::events::{
    next_retry_at, AppEventCancellation, AppEventDeliveryMode, AppEventErrorCode,
};
use crate::integrations::IntegrationsState;
use crate::runner;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

/// Forced re-scan cadence for the idle fast path, in ticks (so, seconds).
/// Cheap enough to be invisible on battery, short enough that a missed cache
/// invalidation is a one-minute blip instead of a permanently dead runtime.
const IDLE_RESCAN_TICKS: u32 = 60;

pub struct AppEventRuntime {
    revision: AtomicU64,
    stopped: AtomicBool,
    in_flight: Mutex<HashMap<String, AppEventCancellation>>,
    last_pruned_day: AtomicI64,
    recovered_interrupted_runs: AtomicBool,
    polling_concurrency: Arc<tokio::sync::Semaphore>,
    socket_concurrency: Arc<tokio::sync::Semaphore>,
    /// `revision + 1` of the scan that found no enabled `app` trigger *and* an
    /// empty queue; `0` means "unknown, go read SQLite". Both halves of the
    /// idle question collapse into this one word because the guard only ever
    /// cares about their conjunction. Comparing it against `revision` is what
    /// makes `reload()` an invalidation for free.
    idle_at_revision: AtomicU64,
    /// Ticks answered from the cache since the last real scan.
    skipped_ticks: AtomicU32,
}

impl Default for AppEventRuntime {
    fn default() -> Self {
        Self {
            revision: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            in_flight: Mutex::new(HashMap::new()),
            last_pruned_day: AtomicI64::new(0),
            recovered_interrupted_runs: AtomicBool::new(false),
            idle_at_revision: AtomicU64::new(0),
            skipped_ticks: AtomicU32::new(0),
            polling_concurrency: Arc::new(tokio::sync::Semaphore::new(4)),
            // Socket adapters spend most of their time waiting for a provider
            // frame. Keep their permits separate so several installations do
            // not prevent replayable polling adapters from making progress.
            socket_concurrency: Arc::new(tokio::sync::Semaphore::new(16)),
        }
    }
}

impl AppEventRuntime {
    /// Bumping the revision is also what invalidates the idle cache: a cached
    /// answer is only trusted while it still matches the current revision.
    /// Every production write to an `app` trigger routes through
    /// `crate::triggers::reload`, which lands here.
    pub fn reload(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
        if let Ok(active) = self.in_flight.lock() {
            for cancellation in active.values() {
                cancellation.cancel();
            }
        }
    }

    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.revision.fetch_add(1, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn begin(&self, trigger_id: &str) -> Option<AppEventCancellation> {
        let mut active = self.in_flight.lock().ok()?;
        if active.contains_key(trigger_id) {
            return None;
        }
        let cancellation = AppEventCancellation::never();
        active.insert(trigger_id.to_owned(), cancellation.clone());
        Some(cancellation)
    }

    fn finish(&self, trigger_id: &str) {
        if let Ok(mut active) = self.in_flight.lock() {
            active.remove(trigger_id);
        }
    }

    /// True when an earlier scan proved there was nothing to do and nothing
    /// has invalidated that since. Two atomic loads instead of three SQLite
    /// round trips, every second, forever.
    ///
    /// Fails safe in both directions: an unset or stale cache returns `false`
    /// and the caller queries for real, and even a permanently-correct-looking
    /// cache is thrown away every `IDLE_RESCAN_TICKS`.
    fn skip_idle_scan(&self, revision: u64) -> bool {
        if self.idle_at_revision.load(Ordering::SeqCst) != revision.wrapping_add(1) {
            return false;
        }
        if self.skipped_ticks.fetch_add(1, Ordering::SeqCst) >= IDLE_RESCAN_TICKS {
            self.skipped_ticks.store(0, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Record what a real scan saw. `idle` means "no enabled `app` trigger and
    /// an empty queue".
    fn note_scan(&self, revision: u64, idle: bool) {
        self.skipped_ticks.store(0, Ordering::SeqCst);
        // A `reload()` that landed mid-scan makes this view stale; drop it
        // rather than cache an answer that was already wrong when it was read.
        let trustworthy = idle && self.revision.load(Ordering::SeqCst) == revision;
        self.idle_at_revision.store(
            if trustworthy {
                revision.wrapping_add(1)
            } else {
                0
            },
            Ordering::SeqCst,
        );
    }
}

impl Drop for AppEventRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub async fn tick(app: &AppHandle) -> Result<(), String> {
    let Some(runtime) = app.try_state::<AppEventRuntime>() else {
        return Ok(());
    };
    if runtime.is_stopped() {
        return Ok(());
    }
    let Some(db) = app.try_state::<Db>() else {
        return Ok(());
    };

    let db = db.inner();
    let triggers = scan(&runtime, db, |pending| {
        let _ = runner::start_pending_app_event_run(app.clone(), db, pending);
    })?;
    for trigger in triggers {
        if !is_due(db, app, &trigger) {
            continue;
        }
        let concurrency = if uses_socket_delivery(app, &trigger) {
            runtime.socket_concurrency.clone()
        } else {
            runtime.polling_concurrency.clone()
        };
        let Ok(permit) = concurrency.try_acquire_owned() else {
            continue;
        };
        let Some(cancellation) = runtime.begin(&trigger.id) else {
            continue;
        };
        let handle = app.clone();
        let trigger_id = trigger.id.clone();
        let revision = runtime.revision.load(Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let Some(runtime) = handle.try_state::<AppEventRuntime>() else {
                return;
            };
            if runtime.is_stopped() || runtime.revision.load(Ordering::SeqCst) != revision {
                runtime.finish(&trigger_id);
                return;
            }
            let Some(db) = handle.try_state::<Db>() else {
                runtime.finish(&trigger_id);
                return;
            };
            let Some(integrations) = handle.try_state::<IntegrationsState>() else {
                runtime.finish(&trigger_id);
                return;
            };
            match integrations
                .sync_app_trigger(db.inner(), &trigger, cancellation)
                .await
            {
                Ok(report) => {
                    let _ = handle.emit(
                        "app-trigger://status",
                        serde_json::json!({
                            "triggerId": trigger_id,
                            "accepted": report.accepted,
                            "duplicates": report.duplicates,
                            "rejected": report.rejected,
                            "droppedOverrun": report.dropped_overrun,
                            "backpressured": report.backpressured,
                        }),
                    );
                }
                Err(error) => {
                    if error.code == AppEventErrorCode::Cancelled {
                        runtime.finish(&trigger_id);
                        return;
                    }
                    let retry_count = db
                        .get_app_trigger_state(&trigger_id)
                        .ok()
                        .flatten()
                        .map(|state| state.retry_count)
                        .unwrap_or(0);
                    let retry_at = next_retry_at(&error, retry_count);
                    let _ = db.mark_app_trigger_error(
                        &trigger_id,
                        error.code.as_str(),
                        Some(&retry_at),
                    );
                    let _ = handle.emit(
                        "app-trigger://status",
                        serde_json::json!({
                            "triggerId": trigger_id,
                            "errorCode": error.code.as_str(),
                            "nextAttemptAt": retry_at,
                        }),
                    );
                }
            }
            runtime.finish(&trigger_id);
        });
    }
    Ok(())
}

/// The SQLite half of a tick, split out so it can be exercised without a Tauri
/// `AppHandle`. Returns the enabled `app` triggers to schedule — empty when the
/// idle guard answered from cache and never opened the database.
///
/// `start` launches a run promoted off the queue; it is a callback only because
/// that is the one step here that needs the `AppHandle`.
///
// ponytail: this caches one bit — "the last scan found nothing" — it is not a
// real change feed. Two ceilings. First, every write to an `app` trigger has to
// route through `AppEventRuntime::reload()`; anything that writes the `triggers`
// table behind its back is only caught by the `IDLE_RESCAN_TICKS` sweep. Second,
// an install with even one enabled app trigger still pays the full per-second
// scan — this only buys back the idle case. If either starts to hurt, replace
// the whole thing with a `tokio::sync::Notify` that the writers signal and the
// loop awaits, and drop the fixed cadence rather than growing this cache.
fn scan(
    runtime: &AppEventRuntime,
    db: &Db,
    start: impl FnMut(&crate::db::PromotedAppEventRun),
) -> Result<Vec<Trigger>, String> {
    // Both of these sit ABOVE the idle guard on purpose. Each is already gated
    // by its own atomic, so the idle path still costs no query, but neither can
    // be starved on a machine that never sees an app trigger: put the guard
    // first and crash recovery would never run.
    if !runtime
        .recovered_interrupted_runs
        .swap(true, Ordering::SeqCst)
    {
        let _ = db.fail_interrupted_app_event_runs();
    }
    let day = Utc::now().timestamp() / 86_400;
    if runtime.last_pruned_day.swap(day, Ordering::SeqCst) != day {
        let _ = db.prune_app_event_receipts();
    }

    let revision = runtime.revision.load(Ordering::SeqCst);
    if runtime.skip_idle_scan(revision) {
        return Ok(Vec::new());
    }

    let queue_empty = drain_queue(db, start)?;
    let triggers = db
        .list_enabled_triggers(Some("app"))
        .map_err(|error| error.to_string())?;
    runtime.note_scan(revision, queue_empty && triggers.is_empty());
    Ok(triggers)
}

fn uses_socket_delivery(app: &AppHandle, trigger: &Trigger) -> bool {
    let Ok(config) = serde_json::from_value::<crate::integrations::events::AppTriggerConfig>(
        trigger.config.clone(),
    ) else {
        return false;
    };
    app.try_state::<IntegrationsState>()
        .and_then(|integrations| {
            integrations
                .events
                .descriptor(&config.provider_id, &config.event_type)
        })
        .is_some_and(|descriptor| {
            !descriptor
                .delivery_modes
                .contains(&AppEventDeliveryMode::Polling)
        })
}

fn is_due(db: &Db, app: &AppHandle, trigger: &Trigger) -> bool {
    let Ok(config) = serde_json::from_value::<crate::integrations::events::AppTriggerConfig>(
        trigger.config.clone(),
    ) else {
        return true;
    };
    let Some(integrations) = app.try_state::<IntegrationsState>() else {
        return false;
    };
    let Some(descriptor) = integrations
        .events
        .descriptor(&config.provider_id, &config.event_type)
    else {
        return true;
    };
    let state = db.get_app_trigger_state(&trigger.id).ok().flatten();
    if state
        .as_ref()
        .and_then(|item| item.next_attempt_at.as_deref())
        .is_some_and(is_future)
    {
        return false;
    }
    let interval = if descriptor
        .delivery_modes
        .contains(&AppEventDeliveryMode::Polling)
    {
        descriptor.poll_interval_seconds.max(5)
    } else {
        1
    };
    !state
        .and_then(|item| item.last_polled_at)
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|last| {
            Utc::now()
                .signed_duration_since(last.with_timezone(&Utc))
                .num_seconds()
                < interval as i64
        })
}

fn is_future(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc) > Utc::now())
        .unwrap_or(false)
}

/// Returns `true` when the queue held nothing at all — neither a pending run
/// nor a queued event — which is half of what the idle guard needs to know.
fn drain_queue(
    db: &Db,
    mut start: impl FnMut(&crate::db::PromotedAppEventRun),
) -> Result<bool, String> {
    let mut empty = true;
    // Recover a run promoted just before a crash before promoting anything new.
    for pending in db
        .list_pending_app_event_runs()
        .map_err(|error| error.to_string())?
    {
        empty = false;
        if !active::has_workflow(&pending.workflow_id) {
            start(&pending);
        }
    }
    for workflow_id in db
        .app_event_queue_workflow_ids()
        .map_err(|error| error.to_string())?
    {
        empty = false;
        if active::has_workflow(&workflow_id) {
            continue;
        }
        if let Some(pending) = db
            .promote_next_app_event(&workflow_id)
            .map_err(|error| error.to_string())?
        {
            start(&pending);
        }
    }
    Ok(empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, UpsertTriggerInput};
    use serde_json::json;

    #[test]
    fn retry_deadline_blocks_a_trigger_until_due() {
        assert!(is_future(
            &(Utc::now() + chrono::Duration::minutes(1)).to_rfc3339()
        ));
        assert!(!is_future(
            &(Utc::now() - chrono::Duration::minutes(1)).to_rfc3339()
        ));
    }

    /// Workflow + enabled `app` trigger, the shape every idle-guard test needs.
    fn seed(db: &Db) -> (String, String) {
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Queue".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .expect("workflow");
        let trigger = db
            .upsert_trigger(UpsertTriggerInput {
                id: None,
                workflow_id: workflow.id.clone(),
                source: "app".into(),
                label: String::new(),
                config: json!({}),
                enabled: true,
            })
            .expect("trigger");
        (workflow.id, trigger.id)
    }

    fn set_enabled(db: &Db, workflow_id: &str, trigger_id: &str, enabled: bool) {
        db.upsert_trigger(UpsertTriggerInput {
            id: Some(trigger_id.to_owned()),
            workflow_id: workflow_id.to_owned(),
            source: "app".into(),
            label: String::new(),
            config: json!({}),
            enabled,
        })
        .expect("trigger");
    }

    fn run_status(db: &Db, run_id: &str) -> String {
        db.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT status FROM runs WHERE id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )?)
        })
        .expect("status")
    }

    #[test]
    fn idle_ticks_skip_the_database_once_a_scan_finds_nothing() {
        let db = Db::open_in_memory().expect("database");
        let (workflow_id, trigger_id) = seed(&db);

        // A run left `running` by a crash. Recovery has to fail it even though
        // this install ends up with no enabled app trigger at all.
        db.record_app_event(&trigger_id, "evt", "{}", true, 10)
            .expect("queue");
        let promoted = db
            .promote_next_app_event(&workflow_id)
            .expect("promote")
            .expect("pending");
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE runs SET status = 'running' WHERE id = ?1",
                rusqlite::params![promoted.run_id],
            )?;
            Ok(())
        })
        .expect("simulate crash");
        set_enabled(&db, &workflow_id, &trigger_id, false);

        let runtime = AppEventRuntime::default();
        let mut started = 0usize;
        let triggers = scan(&runtime, &db, |_| started += 1).expect("first scan");
        assert!(triggers.is_empty());
        assert_eq!(started, 0);
        // The one-shot recovery ran on the same tick that armed the guard.
        assert_eq!(run_status(&db, &promoted.run_id), "failed");

        // Enable the trigger straight in SQLite, i.e. exactly what a missed
        // invalidation looks like. The next scan must not notice — that is the
        // proof it never opened the database.
        set_enabled(&db, &workflow_id, &trigger_id, true);
        let triggers = scan(&runtime, &db, |_| started += 1).expect("cached scan");
        assert!(triggers.is_empty());
        assert_eq!(started, 0);
    }

    #[test]
    fn a_reloaded_trigger_fires_on_the_very_next_tick() {
        let db = Db::open_in_memory().expect("database");
        let runtime = AppEventRuntime::default();

        let mut started = Vec::new();
        assert!(scan(&runtime, &db, |run| started.push(run.run_id.clone()))
            .expect("idle scan")
            .is_empty());

        let (workflow_id, trigger_id) = seed(&db);
        db.record_app_event(&trigger_id, "evt", "{}", true, 10)
            .expect("queue");
        // What `crate::triggers::reload` does after every trigger write.
        runtime.reload();

        let triggers =
            scan(&runtime, &db, |run| started.push(run.run_id.clone())).expect("scan after reload");
        assert_eq!(
            triggers.iter().map(|item| &item.id).collect::<Vec<_>>(),
            vec![&trigger_id]
        );
        assert_eq!(
            started.len(),
            1,
            "the queued event was promoted and started"
        );
        assert_eq!(run_status(&db, &started[0]), "pending");
        assert_eq!(
            db.list_pending_app_event_runs().expect("pending").len(),
            1,
            "workflow_id {workflow_id} keeps its promoted run"
        );
    }

    #[test]
    fn the_idle_cache_revalidates_itself_after_a_missed_invalidation() {
        let db = Db::open_in_memory().expect("database");
        let runtime = AppEventRuntime::default();
        assert!(scan(&runtime, &db, |_| {}).expect("idle scan").is_empty());

        // No `reload()` — the cache is now wrong and nothing told it so.
        let (_, trigger_id) = seed(&db);
        for _ in 0..IDLE_RESCAN_TICKS {
            assert!(scan(&runtime, &db, |_| {}).expect("cached scan").is_empty());
        }
        let triggers = scan(&runtime, &db, |_| {}).expect("forced scan");
        assert_eq!(
            triggers.iter().map(|item| &item.id).collect::<Vec<_>>(),
            vec![&trigger_id],
            "the guard re-reads SQLite at least once a minute, so a missed \
             invalidation costs latency instead of wedging the runtime"
        );
    }

    #[test]
    fn pending_queue_survives_without_starting_a_second_run() {
        let db = Db::open_in_memory().expect("database");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Queue".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .expect("workflow");
        let trigger = db
            .upsert_trigger(UpsertTriggerInput {
                id: None,
                workflow_id: workflow.id.clone(),
                source: "app".into(),
                label: String::new(),
                config: json!({}),
                enabled: true,
            })
            .expect("trigger");
        db.record_app_event(&trigger.id, "evt", "{}", true, 10)
            .expect("queue");
        let first = db
            .promote_next_app_event(&workflow.id)
            .expect("promote")
            .expect("pending");
        assert!(db
            .promote_next_app_event(&workflow.id)
            .expect("second promote")
            .is_none());
        assert_eq!(
            db.list_pending_app_event_runs().expect("pending"),
            vec![first]
        );
    }
}
