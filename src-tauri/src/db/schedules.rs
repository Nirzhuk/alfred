use super::{Db, DbError};
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What Alfred does with a scheduled time that passed while it was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnMissed {
    /// Run as soon as Alfred opens (the behaviour that predates this setting).
    #[default]
    Run,
    /// Drop the missed run and wait for the next scheduled time.
    Skip,
    /// Leave the run due and let the user decide per schedule.
    Ask,
}

impl OnMissed {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Skip => "skip",
            Self::Ask => "ask",
        }
    }

    /// Unknown or hand-edited values fall back to `Run`, the documented
    /// default, so a corrupt row cannot silently stop firing a schedule.
    pub fn from_db(value: &str) -> Self {
        match value {
            "skip" => Self::Skip,
            "ask" => Self::Ask,
            _ => Self::Run,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub workflow_id: String,
    pub cron: String,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub on_missed: OnMissed,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleListItem {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub cron: String,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub on_missed: OnMissed,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertScheduleInput {
    pub workflow_id: String,
    pub cron: String,
    pub enabled: bool,
    pub on_missed: OnMissed,
}

/// Canvas position of a workflow's schedule node.
///
/// Display-only: `x`/`y` never affect when a schedule runs. Missing layout
/// means the canvas composes a deterministic default position instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleLayout {
    pub schedule_id: String,
    pub x: f64,
    pub y: f64,
    pub updated_at: String,
}

/// What the caller believed about an enabled-only toggle. `cron` and
/// `enabled` are the guard columns: a write that lost the race to another
/// surface (or to a recurrence edit) is rejected instead of applied.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEnabledGuard {
    pub cron: String,
    pub enabled: bool,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    let enabled: i64 = row.get(3)?;
    let on_missed: String = row.get(5)?;
    Ok(Schedule {
        id: row.get(0)?,
        workflow_id: row.get(1)?,
        cron: row.get(2)?,
        enabled: enabled != 0,
        next_run_at: row.get(4)?,
        on_missed: OnMissed::from_db(&on_missed),
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

const SCHEDULE_COLUMNS: &str =
    "id, workflow_id, cron, enabled, next_run_at, on_missed, created_at, updated_at";

impl Db {
    pub fn get_schedule(&self, schedule_id: &str) -> Result<Option<Schedule>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = ?1 LIMIT 1"
            ))?;
            let mut rows = stmt.query_map(params![schedule_id], map_row)?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn get_schedule_for_workflow(
        &self,
        workflow_id: &str,
    ) -> Result<Option<Schedule>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workflow_id = ?1 LIMIT 1"
            ))?;
            let mut rows = stmt.query_map(params![workflow_id], map_row)?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn list_enabled_schedules(&self) -> Result<Vec<Schedule>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SCHEDULE_COLUMNS} FROM schedules
                 WHERE enabled = 1
                 ORDER BY next_run_at ASC"
            ))?;
            let rows = stmt
                .query_map([], map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn list_all_schedules(&self) -> Result<Vec<ScheduleListItem>, DbError> {
        self.list_schedule_items(false)
    }

    /// Enabled schedules with their workflow name, for the open-time
    /// missed-run pass.
    pub fn list_enabled_schedule_items(&self) -> Result<Vec<ScheduleListItem>, DbError> {
        self.list_schedule_items(true)
    }

    fn list_schedule_items(&self, enabled_only: bool) -> Result<Vec<ScheduleListItem>, DbError> {
        let filter = if enabled_only { "WHERE s.enabled = 1" } else { "" };
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT s.id, s.workflow_id, w.name, s.cron, s.enabled,
                        s.next_run_at, s.on_missed, s.created_at, s.updated_at
                 FROM schedules s
                 INNER JOIN workflows w ON w.id = s.workflow_id
                 {filter}
                 ORDER BY s.enabled DESC, s.next_run_at IS NULL, s.next_run_at ASC, w.name ASC"
            ))?;
            let rows = stmt
                .query_map([], |row| {
                    let enabled: i64 = row.get(4)?;
                    let on_missed: String = row.get(6)?;
                    Ok(ScheduleListItem {
                        id: row.get(0)?,
                        workflow_id: row.get(1)?,
                        workflow_name: row.get(2)?,
                        cron: row.get(3)?,
                        enabled: enabled != 0,
                        next_run_at: row.get(5)?,
                        on_missed: OnMissed::from_db(&on_missed),
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn upsert_schedule(
        &self,
        input: UpsertScheduleInput,
        next_run_at: Option<String>,
    ) -> Result<Schedule, DbError> {
        // Ensure workflow exists.
        self.get_workflow(&input.workflow_id)?
            .ok_or_else(|| DbError::Other(format!("workflow not found: {}", input.workflow_id)))?;

        let existing = self.get_schedule_for_workflow(&input.workflow_id)?;
        let updated_at = now();

        if let Some(existing) = existing {
            self.with_conn(|conn| {
                conn.execute(
                    "UPDATE schedules
                     SET cron = ?1, enabled = ?2, next_run_at = ?3, on_missed = ?4, updated_at = ?5
                     WHERE id = ?6",
                    params![
                        input.cron,
                        if input.enabled { 1 } else { 0 },
                        next_run_at,
                        input.on_missed.as_str(),
                        updated_at,
                        existing.id
                    ],
                )?;
                Ok(())
            })?;
            return self
                .get_schedule_for_workflow(&input.workflow_id)?
                .ok_or_else(|| DbError::Other("failed to load updated schedule".into()));
        }

        let id = Uuid::new_v4().to_string();
        let created_at = updated_at.clone();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO schedules
                 (id, workflow_id, cron, enabled, next_run_at, on_missed, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    id,
                    input.workflow_id,
                    input.cron,
                    if input.enabled { 1 } else { 0 },
                    next_run_at,
                    input.on_missed.as_str(),
                    created_at
                ],
            )?;
            Ok(())
        })?;

        self.get_schedule_for_workflow(&input.workflow_id)?
            .ok_or_else(|| DbError::Other("failed to load created schedule".into()))
    }

    pub fn set_schedule_next_run(
        &self,
        schedule_id: &str,
        next_run_at: Option<String>,
    ) -> Result<(), DbError> {
        let updated_at = now();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE schedules SET next_run_at = ?1, updated_at = ?2 WHERE id = ?3",
                params![next_run_at, updated_at, schedule_id],
            )?;
            Ok(())
        })
    }

    /// Enabled-only mutation: writes `enabled` and the recomputed
    /// `next_run_at`, and never touches `cron` or `on_missed`. The guard makes a
    /// late write lose to whatever another surface persisted in the meantime.
    pub fn set_schedule_enabled(
        &self,
        schedule_id: &str,
        enabled: bool,
        next_run_at: Option<String>,
        guard: &ScheduleEnabledGuard,
    ) -> Result<Schedule, DbError> {
        let updated_at = now();
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE schedules
                 SET enabled = ?1, next_run_at = ?2, updated_at = ?3
                 WHERE id = ?4 AND cron = ?5 AND enabled = ?6",
                params![
                    if enabled { 1 } else { 0 },
                    next_run_at,
                    updated_at,
                    schedule_id,
                    guard.cron,
                    if guard.enabled { 1 } else { 0 },
                ],
            )?)
        })?;

        if changed == 0 {
            return match self.get_schedule(schedule_id)? {
                // The schedule still exists but no longer matches what the
                // caller rendered: refuse instead of resurrecting stale state.
                Some(_) => Err(DbError::Other(
                    "schedule changed while it was being updated — reopen it and try again".into(),
                )),
                None => Err(DbError::Other(format!(
                    "schedule not found: {schedule_id}"
                ))),
            };
        }

        self.get_schedule(schedule_id)?
            .ok_or_else(|| DbError::Other("failed to load updated schedule".into()))
    }

    pub fn get_schedule_layout(
        &self,
        schedule_id: &str,
    ) -> Result<Option<ScheduleLayout>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT schedule_id, x, y, updated_at
                 FROM schedule_canvas_layout WHERE schedule_id = ?1 LIMIT 1",
            )?;
            let mut rows = stmt.query_map(params![schedule_id], |row| {
                Ok(ScheduleLayout {
                    schedule_id: row.get(0)?,
                    x: row.get(1)?,
                    y: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?;
            Ok(rows.next().transpose()?)
        })
    }

    /// Persist a dragged node position. The schedule must still exist, so a
    /// delayed layout write cannot recreate a node for a removed schedule.
    pub fn set_schedule_layout(
        &self,
        schedule_id: &str,
        x: f64,
        y: f64,
    ) -> Result<ScheduleLayout, DbError> {
        if self.get_schedule(schedule_id)?.is_none() {
            return Err(DbError::Other(format!(
                "schedule not found: {schedule_id}"
            )));
        }
        let updated_at = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO schedule_canvas_layout (schedule_id, x, y, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(schedule_id) DO UPDATE
                   SET x = excluded.x, y = excluded.y, updated_at = excluded.updated_at",
                params![schedule_id, x, y, updated_at],
            )?;
            Ok(())
        })?;
        self.get_schedule_layout(schedule_id)?
            .ok_or_else(|| DbError::Other("failed to load saved schedule layout".into()))
    }

    pub fn delete_schedule_for_workflow(&self, workflow_id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM schedules WHERE workflow_id = ?1",
                params![workflow_id],
            )?;
            Ok(())
        })
    }
}

pub fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CreateWorkflowInput;

    fn scheduled_workflow(db: &Db, name: &str) -> Schedule {
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
                cron: "0 0 9 * * *".into(),
                enabled: true,
                on_missed: OnMissed::Run,
            },
            Some("2026-09-08T09:00:00Z".into()),
        )
        .unwrap()
    }

    #[test]
    fn a_missing_layout_reads_as_none_and_a_drag_replaces_it() {
        let db = Db::open_in_memory().unwrap();
        let schedule = scheduled_workflow(&db, "Layout");

        assert!(db.get_schedule_layout(&schedule.id).unwrap().is_none());

        let saved = db.set_schedule_layout(&schedule.id, -320.0, 96.5).unwrap();
        assert_eq!(saved.schedule_id, schedule.id);
        assert_eq!((saved.x, saved.y), (-320.0, 96.5));
        let reloaded = db.get_schedule_layout(&schedule.id).unwrap().unwrap();
        assert_eq!((reloaded.x, reloaded.y), (-320.0, 96.5));

        db.set_schedule_layout(&schedule.id, 40.0, -12.0).unwrap();
        let moved = db.get_schedule_layout(&schedule.id).unwrap().unwrap();
        assert_eq!((moved.x, moved.y), (40.0, -12.0));
    }

    #[test]
    fn a_layout_write_never_recreates_a_removed_schedule() {
        let db = Db::open_in_memory().unwrap();
        let schedule = scheduled_workflow(&db, "Removed");
        db.delete_schedule_for_workflow(&schedule.workflow_id).unwrap();

        assert!(db.set_schedule_layout(&schedule.id, 10.0, 10.0).is_err());
        assert!(db.get_schedule(&schedule.id).unwrap().is_none());
        assert!(db.get_schedule_layout(&schedule.id).unwrap().is_none());
    }

    #[test]
    fn deleting_a_schedule_removes_its_canvas_layout() {
        let db = Db::open_in_memory().unwrap();
        let schedule = scheduled_workflow(&db, "Cascade");
        db.set_schedule_layout(&schedule.id, 12.0, 24.0).unwrap();

        db.delete_schedule_for_workflow(&schedule.workflow_id).unwrap();

        let remaining: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM schedule_canvas_layout WHERE schedule_id = ?1",
                    params![schedule.id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(remaining, 0);
    }
}
