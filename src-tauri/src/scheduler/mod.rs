//! Schedule triggers for workflows.
//!
//! Workflows are always runnable manually. Schedules enqueue the same runner
//! with `trigger_kind = schedule` when due. The ticker only runs while the app
//! is open.

use crate::db::{parse_rfc3339, Db, UpsertScheduleInput};
use crate::runner::{start_run, RunTrigger};
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;
use tauri::AppHandle;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("{0}")]
    Message(String),
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
        },
        next_run_at,
    )
    .map_err(|e| SchedulerError::Message(e.to_string()))
}

/// Check due schedules and start runs. Returns how many runs were started.
pub fn tick(app: &AppHandle, db: &Db) -> Result<usize, SchedulerError> {
    tick_with_start(db, Utc::now(), |workflow_id| {
        start_run(app.clone(), db, workflow_id, RunTrigger::Schedule, None)
            .map(|_| ())
            .map_err(|e| SchedulerError::Message(e.to_string()))
    })
}

fn tick_with_start(
    db: &Db,
    now: DateTime<Utc>,
    mut start: impl FnMut(&str) -> Result<(), SchedulerError>,
) -> Result<usize, SchedulerError> {
    let schedules = db
        .list_enabled_schedules()
        .map_err(|e| SchedulerError::Message(e.to_string()))?;

    let mut fired = 0usize;

    for schedule in schedules {
        match tick_schedule(db, &schedule, now, &mut start) {
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
    start: &mut impl FnMut(&str) -> Result<(), SchedulerError>,
) -> Result<bool, SchedulerError> {
    match schedule.next_run_at.as_deref().and_then(parse_rfc3339) {
        Some(next) if next > now => return Ok(false),
        None => {
            let next = next_run_from_cron(&schedule.cron, now)?;
            db.set_schedule_next_run(&schedule.id, Some(next.to_rfc3339()))
                .map_err(|e| SchedulerError::Message(e.to_string()))?;
            return Ok(false);
        }
        Some(_) => {}
    }

    start(&schedule.workflow_id)?;
    let next = next_run_from_cron(&schedule.cron, now)?;
    db.set_schedule_next_run(&schedule.id, Some(next.to_rfc3339()))
        .map_err(|e| SchedulerError::Message(e.to_string()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, Schedule};

    fn schedule(db: &Db, name: &str, cron: &str, next: Option<&str>) -> Schedule {
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
            },
            next.map(str::to_string),
        )
        .unwrap()
    }

    #[test]
    fn a_failed_start_does_not_block_other_due_workflows_and_remains_retryable() {
        let db = Db::open_in_memory().unwrap();
        let now = parse_rfc3339("2026-09-08T12:00:00Z").unwrap();
        let busy = schedule(&db, "Busy", "0 * * * * *", Some("2026-09-08T11:00:00Z"));
        let ready = schedule(&db, "Ready", "0 * * * * *", Some("2026-09-08T11:01:00Z"));
        let mut attempted = Vec::new();

        let fired = tick_with_start(&db, now, |id| {
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
        let next = db
            .get_schedule_for_workflow(&ready.workflow_id)
            .unwrap()
            .unwrap()
            .next_run_at
            .unwrap();
        assert!(parse_rfc3339(&next).unwrap() > now);

        attempted.clear();
        assert_eq!(
            tick_with_start(&db, now, |id| {
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
        let now = parse_rfc3339("2026-09-08T12:00:00Z").unwrap();
        schedule(&db, "Invalid", "invalid cron", None);
        let ready = schedule(&db, "Ready", "0 * * * * *", Some("2026-09-08T11:00:00Z"));
        let mut attempted = Vec::new();
        assert_eq!(
            tick_with_start(&db, now, |id| {
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
        let now = parse_rfc3339("2026-09-08T12:00:00Z").unwrap();
        let missing = schedule(&db, "Missing", "0 * * * * *", None);
        schedule(&db, "Future", "0 * * * * *", Some("2026-09-08T13:00:00Z"));
        assert_eq!(tick_with_start(&db, now, |_| panic!("not due")).unwrap(), 0);
        let next = db
            .get_schedule_for_workflow(&missing.workflow_id)
            .unwrap()
            .unwrap()
            .next_run_at
            .unwrap();
        assert_eq!(
            parse_rfc3339(&next).unwrap(),
            now + chrono::Duration::minutes(1)
        );
    }
}
