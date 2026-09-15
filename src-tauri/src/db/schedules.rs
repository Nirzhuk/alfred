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
