//! Schedule triggers for workflows.
//!
//! Workflows are always runnable manually. Schedules enqueue the same runner
//! with `trigger_kind = schedule` when due. The ticker only runs while the app
//! is open, so a time that passed while Alfred was closed is a *miss*: the
//! schedule's own `on_missed` choice catches it up, drops it, or waits for the
//! user to answer from the missed list.

use crate::db::{parse_rfc3339, Db, OnMissed, ScheduleEnabledGuard, UpsertScheduleInput};
use crate::runner::{start_run, RunTrigger};
use chrono::{DateTime, Utc};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tauri::{AppHandle, Manager};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("{0}")]
    Message(String),
}

/// When Alfred opened. Any scheduled time before this passed while the app was
/// closed, so it counts as missed rather than merely due.
pub struct SchedulerRuntime {
    opened_at: DateTime<Utc>,
}

impl SchedulerRuntime {
    pub fn new() -> Self {
        Self {
            opened_at: Utc::now(),
        }
    }

    pub fn opened_at(&self) -> DateTime<Utc> {
        self.opened_at
    }
}

impl Default for SchedulerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// A missed scheduled run that waits for the user's decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissedSchedule {
    pub schedule_id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub cron: String,
    /// The scheduled time that passed while Alfred was closed.
    pub missed_at: String,
}

/// The user's answer for one missed schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MissedDecision {
    Run,
    Skip,
}

/// Validate a cron expression and compute the next fire time from now.
///
/// Expects a 6-field cron: `sec min hour day month dow`
/// Example: `0 0 9 * * 1-5` = weekdays at 09:00.
pub fn next_run_from_cron(
    cron: &str,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, SchedulerError> {
    let schedule = Schedule::from_str(cron.trim())
        .map_err(|e| SchedulerError::Message(format!("invalid cron `{cron}`: {e}")))?;

    schedule
        .after(&after)
        .next()
        .ok_or_else(|| SchedulerError::Message(format!("cron `{cron}` has no upcoming time")))
}

pub fn upsert_schedule(
    db: &Db,
    workflow_id: String,
    cron: String,
    enabled: bool,
    on_missed: OnMissed,
) -> Result<crate::db::Schedule, SchedulerError> {
    let next_run_at = if enabled {
        Some(next_run_from_cron(&cron, Utc::now())?.to_rfc3339())
    } else {
        None
    };

    db.upsert_schedule(
        UpsertScheduleInput {
            workflow_id,
            cron,
            enabled,
            on_missed,
        },
        next_run_at,
    )
    .map_err(|e| SchedulerError::Message(e.to_string()))
}

/// Turn a schedule on or off without touching its recurrence.
///
/// An enabled schedule computes its next occurrence from now; a disabled one
/// clears it. Setting the value the schedule already has is a no-op, so a
/// switch that was already applied elsewhere never re-encodes cron. The guard
/// rejects a write that lost the race to a recurrence edit or another toggle.
pub fn set_schedule_enabled(
    db: &Db,
    schedule_id: &str,
    enabled: bool,
    guard: &ScheduleEnabledGuard,
) -> Result<crate::db::Schedule, SchedulerError> {
    let schedule = db
        .get_schedule(schedule_id)
        .map_err(|e| SchedulerError::Message(e.to_string()))?
        .ok_or_else(|| SchedulerError::Message(format!("schedule not found: {schedule_id}")))?;

    if schedule.enabled == enabled {
        return Ok(schedule);
    }

    let next_run_at = if enabled {
        Some(next_run_from_cron(&schedule.cron, Utc::now())?.to_rfc3339())
    } else {
        None
    };

    db.set_schedule_enabled(schedule_id, enabled, next_run_at, guard)
        .map_err(|e| SchedulerError::Message(e.to_string()))
}

/// Check due schedules and start runs. Returns how many runs were started.
pub fn tick(app: &AppHandle, db: &Db) -> Result<usize, SchedulerError> {
    let opened_at = app.state::<SchedulerRuntime>().opened_at();
    tick_with_start(db, Utc::now(), opened_at, |workflow_id| {
        start_run(app.clone(), db, workflow_id, RunTrigger::Schedule, None)
            .map(|_| ())
            .map_err(|e| SchedulerError::Message(e.to_string()))
    })
}

fn tick_with_start(
    db: &Db,
    now: DateTime<Utc>,
    opened_at: DateTime<Utc>,
    mut start: impl FnMut(&str) -> Result<(), SchedulerError>,
) -> Result<usize, SchedulerError> {
    let schedules = db
        .list_enabled_schedules()
        .map_err(|e| SchedulerError::Message(e.to_string()))?;

    let mut fired = 0usize;

    for schedule in schedules {
        match tick_schedule(db, &schedule, now, opened_at, &mut start) {
            Ok(true) => fired += 1,
            Ok(false) => {}
            // Leave failed starts due for retry, without blocking other workflows.
            Err(error) => eprintln!("scheduler tick error: {error}"),
        }
    }

    Ok(fired)
}

fn tick_schedule(
    db: &Db,
    schedule: &crate::db::Schedule,
    now: DateTime<Utc>,
    opened_at: DateTime<Utc>,
    start: &mut impl FnMut(&str) -> Result<(), SchedulerError>,
) -> Result<bool, SchedulerError> {
    let Some(next) = schedule.next_run_at.as_deref().and_then(parse_rfc3339) else {
        // Never armed, or unreadable: set the next time without running.
        return advance_schedule(db, schedule, now).map(|_| false);
    };

    if next > now {
        return Ok(false);
    }

    if next < opened_at {
        match schedule.on_missed {
            OnMissed::Run => {}
            OnMissed::Skip => {
                advance_schedule(db, schedule, now)?;
                return Ok(false);
            }
            // Deliberately left due: the user answers it from the missed list.
            OnMissed::Ask => return Ok(false),
        }
    }

    start(&schedule.workflow_id)?;
    advance_schedule(db, schedule, now)?;
    Ok(true)
}

fn advance_schedule(
    db: &Db,
    schedule: &crate::db::Schedule,
    now: DateTime<Utc>,
) -> Result<(), SchedulerError> {
    let next = next_run_from_cron(&schedule.cron, now)?;
    db.set_schedule_next_run(&schedule.id, Some(next.to_rfc3339()))
        .map_err(|e| SchedulerError::Message(e.to_string()))
}

/// Missed runs waiting on the user: enabled schedules set to `ask` whose
/// stored time passed before Alfred opened.
pub fn missed_schedules(
    db: &Db,
    opened_at: DateTime<Utc>,
) -> Result<Vec<MissedSchedule>, SchedulerError> {
    let items = db
        .list_enabled_schedule_items()
        .map_err(|e| SchedulerError::Message(e.to_string()))?;

    Ok(items
        .into_iter()
        .filter(|item| item.on_missed == OnMissed::Ask)
        .filter_map(|item| {
            let missed_at = item.next_run_at?;
            let missed = parse_rfc3339(&missed_at)?;
            (missed < opened_at).then(|| MissedSchedule {
                schedule_id: item.id,
                workflow_id: item.workflow_id,
                workflow_name: item.workflow_name,
                cron: item.cron,
                missed_at,
            })
        })
        .collect())
}

/// Answer one missed schedule. Returns whether a run was started.
pub fn resolve_missed(
    app: &AppHandle,
    db: &Db,
    schedule_id: &str,
    decision: MissedDecision,
) -> Result<bool, SchedulerError> {
    resolve_missed_with(db, schedule_id, decision, Utc::now(), &mut |workflow_id| {
        start_run(app.clone(), db, workflow_id, RunTrigger::Schedule, None)
            .map(|_| ())
            .map_err(|e| SchedulerError::Message(e.to_string()))
    })
}

fn resolve_missed_with(
    db: &Db,
    schedule_id: &str,
    decision: MissedDecision,
    now: DateTime<Utc>,
    start: &mut impl FnMut(&str) -> Result<(), SchedulerError>,
) -> Result<bool, SchedulerError> {
    let schedule = db
        .get_schedule(schedule_id)
        .map_err(|e| SchedulerError::Message(e.to_string()))?
        .ok_or_else(|| SchedulerError::Message("schedule not found".into()))?;

    if schedule.on_missed != OnMissed::Ask {
        return Err(SchedulerError::Message(
            "schedule does not ask before running a missed time".into(),
        ));
    }
    if !schedule.enabled {
        return Err(SchedulerError::Message("schedule is disabled".into()));
    }

    let started = match decision {
        MissedDecision::Run => {
            start(&schedule.workflow_id)?;
            true
        }
        MissedDecision::Skip => false,
    };

    // Either answer clears the missed time, so the same occurrence can neither
    // fire twice nor be asked again.
    advance_schedule(db, &schedule, now)?;
    Ok(started)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, Schedule};

    fn schedule_with(
        db: &Db,
        name: &str,
        cron: &str,
        next: Option<&str>,
        on_missed: OnMissed,
    ) -> Schedule {
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: name.into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: serde_json::json!({ "nodes": [], "edges": [] }),
            })
            .unwrap();
        db.upsert_schedule(
            UpsertScheduleInput {
                workflow_id: workflow.id,
                cron: cron.into(),
                enabled: true,
                on_missed,
            },
            next.map(str::to_string),
        )
        .unwrap()
    }

    fn schedule(db: &Db, name: &str, cron: &str, next: Option<&str>) -> Schedule {
        schedule_with(db, name, cron, next, OnMissed::Run)
    }

    fn at(value: &str) -> DateTime<Utc> {
        parse_rfc3339(value).unwrap()
    }

    fn next_run(db: &Db, schedule: &Schedule) -> Option<DateTime<Utc>> {
        db.get_schedule_for_workflow(&schedule.workflow_id)
            .unwrap()
            .unwrap()
            .next_run_at
            .as_deref()
            .and_then(parse_rfc3339)
    }

    #[test]
    fn a_failed_start_does_not_block_other_due_workflows_and_remains_retryable() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let busy = schedule(&db, "Busy", "0 * * * * *", Some("2026-09-08T11:00:00Z"));
        let ready = schedule(&db, "Ready", "0 * * * * *", Some("2026-09-08T11:01:00Z"));
        let mut attempted = Vec::new();

        let fired = tick_with_start(&db, now, now, |id| {
            attempted.push(id.to_string());
            if id == busy.workflow_id {
                Err(SchedulerError::Message(
                    "This workflow is already running".into(),
                ))
            } else {
                Ok(())
            }
        })
        .unwrap();

        assert_eq!(fired, 1);
        assert_eq!(
            attempted,
            [busy.workflow_id.clone(), ready.workflow_id.clone()]
        );
        assert_eq!(
            db.get_schedule_for_workflow(&busy.workflow_id)
                .unwrap()
                .unwrap()
                .next_run_at,
            busy.next_run_at
        );
        assert!(next_run(&db, &ready).unwrap() > now);

        attempted.clear();
        assert_eq!(
            tick_with_start(&db, now, now, |id| {
                attempted.push(id.to_string());
                Ok(())
            })
            .unwrap(),
            1
        );
        assert_eq!(attempted, [busy.workflow_id]);
    }

    #[test]
    fn invalid_schedule_does_not_block_valid_schedules() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        schedule(&db, "Invalid", "invalid cron", None);
        let ready = schedule(&db, "Ready", "0 * * * * *", Some("2026-09-08T11:00:00Z"));
        let mut attempted = Vec::new();
        assert_eq!(
            tick_with_start(&db, now, now, |id| {
                attempted.push(id.to_string());
                Ok(())
            })
            .unwrap(),
            1
        );
        assert_eq!(attempted, [ready.workflow_id]);
    }

    #[test]
    fn missing_next_run_is_initialized_without_starting_early() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let missing = schedule(&db, "Missing", "0 * * * * *", None);
        schedule(&db, "Future", "0 * * * * *", Some("2026-09-08T13:00:00Z"));
        assert_eq!(
            tick_with_start(&db, now, now, |_| panic!("not due")).unwrap(),
            0
        );
        assert_eq!(
            next_run(&db, &missing).unwrap(),
            now + chrono::Duration::minutes(1)
        );
    }

    #[test]
    fn on_missed_run_catches_up_on_open() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let opened_at = at("2026-09-08T11:30:00Z");
        let missed = schedule(&db, "Catch up", "0 * * * * *", Some("2026-09-08T11:00:00Z"));
        let mut attempted = Vec::new();

        assert_eq!(
            tick_with_start(&db, now, opened_at, |id| {
                attempted.push(id.to_string());
                Ok(())
            })
            .unwrap(),
            1
        );
        assert_eq!(attempted, [missed.workflow_id]);
        assert!(missed_schedules(&db, opened_at).unwrap().is_empty());
    }

    #[test]
    fn on_missed_skip_drops_the_closed_window_run_and_keeps_the_cadence() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let opened_at = at("2026-09-08T11:30:00Z");
        let skipped = schedule_with(
            &db,
            "Skip",
            "0 * * * * *",
            Some("2026-09-08T11:00:00Z"),
            OnMissed::Skip,
        );

        assert_eq!(
            tick_with_start(&db, now, opened_at, |_| panic!("not asked to run")).unwrap(),
            0
        );
        assert_eq!(
            next_run(&db, &skipped).unwrap(),
            now + chrono::Duration::minutes(1)
        );
        assert!(missed_schedules(&db, opened_at).unwrap().is_empty());
    }

    #[test]
    fn on_missed_ask_holds_the_run_for_the_user() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let opened_at = at("2026-09-08T11:30:00Z");
        let asked = schedule_with(
            &db,
            "Ask me",
            "0 0 9 * * *",
            Some("2026-09-08T09:00:00Z"),
            OnMissed::Ask,
        );

        for _ in 0..2 {
            assert_eq!(
                tick_with_start(&db, now, opened_at, |_| panic!("not asked to run")).unwrap(),
                0
            );
        }

        // Still due on its original missed time, waiting on the answer.
        assert_eq!(
            next_run(&db, &asked).unwrap(),
            at("2026-09-08T09:00:00Z")
        );
        let missed = missed_schedules(&db, opened_at).unwrap();
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].schedule_id, asked.id);
        assert_eq!(missed[0].workflow_id, asked.workflow_id);
        assert_eq!(missed[0].workflow_name, "Ask me");
        assert_eq!(missed[0].cron, "0 0 9 * * *");
        assert_eq!(missed[0].missed_at, "2026-09-08T09:00:00Z");
    }

    #[test]
    fn a_time_that_comes_due_while_alfred_is_open_runs_for_every_policy() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let opened_at = at("2026-09-08T10:00:00Z");
        let mut attempted = Vec::new();

        for (name, policy) in [
            ("Catch up", OnMissed::Run),
            ("Skip", OnMissed::Skip),
            ("Ask me", OnMissed::Ask),
        ] {
            let due = schedule_with(
                &db,
                name,
                "0 * * * * *",
                Some("2026-09-08T11:59:00Z"),
                policy,
            );
            assert_eq!(
                tick_with_start(&db, now, opened_at, |id| {
                    attempted.push(id.to_string());
                    Ok(())
                })
                .unwrap(),
                1
            );
            assert_eq!(attempted.last().unwrap(), &due.workflow_id);
            assert!(next_run(&db, &due).unwrap() > now);
        }

        assert!(missed_schedules(&db, opened_at).unwrap().is_empty());
    }

    #[test]
    fn answering_a_missed_schedule_runs_or_drops_it_exactly_once() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let opened_at = at("2026-09-08T11:30:00Z");
        let run_now = schedule_with(
            &db,
            "Run now",
            "0 0 9 * * *",
            Some("2026-09-08T09:00:00Z"),
            OnMissed::Ask,
        );
        let drop = schedule_with(
            &db,
            "Drop",
            "0 0 9 * * *",
            Some("2026-09-08T09:00:00Z"),
            OnMissed::Ask,
        );
        let automatic = schedule(&db, "Automatic", "0 0 9 * * *", Some("2026-09-08T09:00:00Z"));

        assert_eq!(missed_schedules(&db, opened_at).unwrap().len(), 2);

        let mut started = Vec::new();
        let mut start = |id: &str| {
            started.push(id.to_string());
            Ok(())
        };

        let ran = resolve_missed_with(&db, &run_now.id, MissedDecision::Run, now, &mut start).unwrap();
        let skipped =
            resolve_missed_with(&db, &drop.id, MissedDecision::Skip, now, &mut start).unwrap();
        let automatic =
            resolve_missed_with(&db, &automatic.id, MissedDecision::Run, now, &mut start);
        let missing = resolve_missed_with(&db, "missing-id", MissedDecision::Run, now, &mut start);

        assert!(ran);
        assert!(!skipped);
        assert!(automatic.is_err());
        assert!(missing.is_err());
        assert_eq!(started, [run_now.workflow_id.clone()]);

        // Both answers clear the missed time; nothing is asked a second time.
        assert!(missed_schedules(&db, opened_at).unwrap().is_empty());
        assert!(next_run(&db, &run_now).unwrap() > now);
        assert!(next_run(&db, &drop).unwrap() > now);
    }

    #[test]
    fn a_failed_catch_up_start_keeps_the_missed_answer_pending() {
        let db = Db::open_in_memory().unwrap();
        let now = at("2026-09-08T12:00:00Z");
        let opened_at = at("2026-09-08T11:30:00Z");
        let busy = schedule_with(
            &db,
            "Busy",
            "0 0 9 * * *",
            Some("2026-09-08T09:00:00Z"),
            OnMissed::Ask,
        );

        let mut start = |_: &str| Err(SchedulerError::Message("already running".into()));
        assert!(resolve_missed_with(&db, &busy.id, MissedDecision::Run, now, &mut start).is_err());
        assert_eq!(missed_schedules(&db, opened_at).unwrap().len(), 1);
        assert_eq!(next_run(&db, &busy).unwrap(), at("2026-09-08T09:00:00Z"));
    }

    /// The guard a caller sends when it toggles a row exactly as rendered.
    fn guard(schedule: &Schedule) -> ScheduleEnabledGuard {
        ScheduleEnabledGuard {
            cron: schedule.cron.clone(),
            enabled: schedule.enabled,
        }
    }

    fn disabled_schedule(db: &Db, name: &str, cron: &str, on_missed: OnMissed) -> Schedule {
        let schedule = schedule_with(db, name, cron, None, on_missed);
        set_schedule_enabled(db, &schedule.id, false, &guard(&schedule)).unwrap()
    }

    #[test]
    fn enabling_arms_the_next_run_and_disabling_clears_it() {
        let db = Db::open_in_memory().unwrap();
        let paused = disabled_schedule(&db, "Paused", "0 0 9 * * 1-5", OnMissed::Skip);

        let enabled = set_schedule_enabled(&db, &paused.id, true, &guard(&paused)).unwrap();
        assert!(enabled.enabled);
        assert_eq!(enabled.cron, paused.cron);
        assert_eq!(enabled.on_missed, OnMissed::Skip);
        assert!(next_run(&db, &enabled).unwrap() > Utc::now());

        let disabled = set_schedule_enabled(&db, &enabled.id, false, &guard(&enabled)).unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.cron, paused.cron);
        assert_eq!(disabled.on_missed, OnMissed::Skip);
        assert_eq!(disabled.next_run_at, None);
    }

    #[test]
    fn setting_the_value_a_schedule_already_has_changes_nothing() {
        let db = Db::open_in_memory().unwrap();
        let due = schedule(&db, "Already on", "0 0 9 * * *", Some("2026-09-08T09:00:00Z"));

        let again = set_schedule_enabled(&db, &due.id, true, &guard(&due)).unwrap();
        assert_eq!(again.enabled, due.enabled);
        assert_eq!(again.next_run_at, due.next_run_at);
        assert_eq!(again.updated_at, due.updated_at);

        let paused = disabled_schedule(&db, "Already off", "0 0 9 * * *", OnMissed::Run);
        let again = set_schedule_enabled(&db, &paused.id, false, &guard(&paused)).unwrap();
        assert_eq!(again.enabled, false);
        assert_eq!(again.updated_at, paused.updated_at);
    }

    #[test]
    fn a_toggle_that_lost_the_race_to_a_recurrence_edit_is_rejected() {
        let db = Db::open_in_memory().unwrap();
        let paused = disabled_schedule(&db, "Edited", "0 0 9 * * *", OnMissed::Run);
        let stale = guard(&paused);

        // Another surface edits the recurrence while this toggle is in flight.
        db.upsert_schedule(
            UpsertScheduleInput {
                workflow_id: paused.workflow_id.clone(),
                cron: "0 30 7 * * *".into(),
                enabled: false,
                on_missed: OnMissed::Run,
            },
            None,
        )
        .unwrap();

        let rejected = set_schedule_enabled(&db, &paused.id, true, &stale);
        assert!(
            rejected.is_err(),
            "a toggle rendered against a different recurrence must not arm the new one"
        );

        // The persisted recurrence wins; the stale toggle changed nothing.
        let persisted = db.get_schedule(&paused.id).unwrap().unwrap();
        assert_eq!(persisted.cron, "0 30 7 * * *");
        assert!(!persisted.enabled);
        assert_eq!(persisted.next_run_at, None);

        // The same toggle succeeds once it is rendered against the saved rule.
        let accepted = set_schedule_enabled(&db, &paused.id, true, &guard(&persisted)).unwrap();
        assert!(accepted.enabled);
        assert_eq!(accepted.cron, "0 30 7 * * *");
        assert!(accepted.next_run_at.is_some());
    }

    #[test]
    fn a_stale_toggle_that_targets_the_persisted_value_changes_nothing() {
        let db = Db::open_in_memory().unwrap();
        let paused = disabled_schedule(&db, "Already on", "0 0 9 * * *", OnMissed::Run);
        let stale = guard(&paused);

        // Another surface enables it while the switch for "off" is in flight.
        let enabled = set_schedule_enabled(&db, &paused.id, true, &guard(&paused)).unwrap();

        // Turning off from the stale render is a real change against the guard
        // the caller rendered... but the target it saw (off) is what it wanted,
        // so only the guard mismatch decides: the write is rejected.
        assert!(set_schedule_enabled(&db, &paused.id, false, &stale).is_err());
        let persisted = db.get_schedule(&paused.id).unwrap().unwrap();
        assert!(persisted.enabled);
        assert_eq!(persisted.next_run_at, enabled.next_run_at);

        // Requesting the value the row already has never touches it.
        let again = set_schedule_enabled(&db, &paused.id, true, &stale).unwrap();
        assert_eq!(again.updated_at, enabled.updated_at);
        assert_eq!(again.next_run_at, enabled.next_run_at);
    }

    #[test]
    fn a_removed_schedule_cannot_be_toggled_back_to_life() {
        let db = Db::open_in_memory().unwrap();
        let paused = disabled_schedule(&db, "Removed", "0 0 9 * * *", OnMissed::Run);
        let stale = guard(&paused);
        db.delete_schedule_for_workflow(&paused.workflow_id)
            .unwrap();

        assert!(set_schedule_enabled(&db, &paused.id, true, &stale).is_err());
        assert!(db.get_schedule(&paused.id).unwrap().is_none());
    }
}
