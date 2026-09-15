//! Durable Output-save contracts.
//!
//! The runner captures [`OutputDisposition`] from its execution-time graph and
//! inserts an [`OutputReceiptSeed`] beside the completed run step. Storage code
//! then claims the receipt through [`OutputReceiptAction::StartSave`], writes
//! only files owned by [`OutputStorageIdentity`], and conditionally commits the
//! canonical memory plus `Saved` transition. A concurrent tombstone therefore
//! wins instead of letting a late save recreate a deleted memory.

use super::artifact_chunks::{trusted_artifact_target, trusted_output_artifact_path};
use super::memories::{
    normalize_body, validate_title, write_canonical_memory, CanonicalMemoryRecord, MemoryScopeType,
    MemoryStatus, MemoryType,
};
use super::{Db, DbError};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

pub const OUTPUT_STAGING_DIRECTORY: &str = ".output-staging";
pub const OUTPUT_ARTIFACT_DIRECTORY: &str = "outputs";
pub const OUTPUT_ARTIFACT_BODY_THRESHOLD: usize = 32 * 1024;
const OUTPUT_FILE_PREFIX: &str = "output-";
const OUTPUT_STAGING_SUFFIX: &str = ".stage";
const OUTPUT_ARTIFACT_SUFFIX: &str = ".txt";

/// Output settings captured from the graph clone that is actually executing.
///
/// These defaults intentionally match `normalizeOutputNodeData` in the
/// frontend. Keep all six settings even though only the first three control
/// memory creation: history and recovery must describe what ran without
/// consulting a graph that may since have changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDisposition {
    pub label: String,
    pub save_to_memory: bool,
    pub pin_memory: bool,
    pub memory_title: String,
    pub as_final_result: bool,
    pub html_report: bool,
    pub include_files_changed: bool,
}

impl Default for OutputDisposition {
    fn default() -> Self {
        Self {
            label: "Output".into(),
            save_to_memory: true,
            pin_memory: false,
            memory_title: String::new(),
            as_final_result: true,
            html_report: false,
            include_files_changed: true,
        }
    }
}

impl OutputDisposition {
    /// Normalize legacy or partially populated Output node data.
    pub fn capture(node_data: &Value) -> Self {
        let defaults = Self::default();
        let label = node_data
            .get("label")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&defaults.label)
            .to_owned();

        Self {
            label,
            save_to_memory: bool_setting(node_data, "saveToMemory", defaults.save_to_memory),
            pin_memory: bool_setting(node_data, "pinMemory", defaults.pin_memory),
            memory_title: node_data
                .get("memoryTitle")
                .and_then(Value::as_str)
                .unwrap_or(&defaults.memory_title)
                .to_owned(),
            as_final_result: bool_setting(node_data, "asFinalResult", defaults.as_final_result),
            html_report: bool_setting(node_data, "htmlReport", defaults.html_report),
            include_files_changed: bool_setting(
                node_data,
                "includeFilesChanged",
                defaults.include_files_changed,
            ),
        }
    }

    /// Resolve the title once, before the graph can change. Canonical memory
    /// validation remains responsible for enforcing its length limit.
    pub fn resolved_memory_title(&self) -> String {
        let override_title = self.memory_title.trim();
        if !override_title.is_empty() {
            return override_title.to_owned();
        }
        let label = self.label.trim();
        if label.is_empty() {
            "Output".into()
        } else {
            label.to_owned()
        }
    }

    pub fn initial_receipt_state(&self, output_body: &str) -> OutputReceiptState {
        if !self.save_to_memory {
            OutputReceiptState::NotRequested
        } else if output_body.trim().is_empty() {
            OutputReceiptState::SkippedEmpty
        } else {
            OutputReceiptState::Pending
        }
    }
}

fn bool_setting(data: &Value, key: &str, default: bool) -> bool {
    data.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// The database uniqueness key for one Output execution.
///
/// One node executes at most once per run today. A future repeated-node feature
/// must add an execution instance to this key before it can use this contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputReceiptKey {
    run_id: String,
    node_id: String,
}

impl OutputReceiptKey {
    pub fn new(
        run_id: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Result<Self, OutputIdentityError> {
        let run_id = run_id.into();
        let node_id = node_id.into();
        if run_id.trim().is_empty() {
            return Err(OutputIdentityError::EmptyRunId);
        }
        if node_id.trim().is_empty() {
            return Err(OutputIdentityError::EmptyNodeId);
        }
        Ok(Self { run_id, node_id })
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OutputIdentityError {
    #[error("output receipt run id cannot be empty")]
    EmptyRunId,
    #[error("output receipt node id cannot be empty")]
    EmptyNodeId,
    #[error("output receipt step id cannot be empty")]
    EmptyStepId,
}

/// A UUID allocated with the receipt and reused by every retry.
///
/// It is both the canonical memory ID and the ownership token for any spilled
/// artifact. Generic memory artifact names do not match these paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutputStorageIdentity(Uuid);

impl OutputStorageIdentity {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn memory_id(&self) -> String {
        self.0.to_string()
    }

    pub fn artifact_relative_path(&self) -> PathBuf {
        Path::new(OUTPUT_ARTIFACT_DIRECTORY).join(format!(
            "{OUTPUT_FILE_PREFIX}{}{OUTPUT_ARTIFACT_SUFFIX}",
            self.0
        ))
    }

    pub fn from_owned_artifact_relative_path(path: &Path) -> Option<Self> {
        parse_owned_relative_path(path, OUTPUT_ARTIFACT_DIRECTORY, OUTPUT_ARTIFACT_SUFFIX)
    }
}

impl Default for OutputStorageIdentity {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for OutputStorageIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A fresh fencing token for one transition into `saving`.
///
/// Recovery invalidates this token before a replacement worker may claim the
/// receipt. Durable transitions and temporary files must carry it so a delayed
/// worker cannot complete or clean up a newer attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputSaveAttemptIdentity(Uuid);

impl OutputSaveAttemptIdentity {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }

    fn parse(value: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }
}

impl fmt::Display for OutputSaveAttemptIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct OutputStagingIdentity {
    storage: OutputStorageIdentity,
    attempt: OutputSaveAttemptIdentity,
}

impl OutputStagingIdentity {
    fn new(storage: OutputStorageIdentity, attempt: OutputSaveAttemptIdentity) -> Self {
        Self { storage, attempt }
    }

    fn relative_path(self) -> PathBuf {
        Path::new(OUTPUT_STAGING_DIRECTORY).join(format!(
            "{OUTPUT_FILE_PREFIX}{}-{}{OUTPUT_STAGING_SUFFIX}",
            self.storage, self.attempt
        ))
    }

    /// Accept only an exact attempt-owned child of Alfred's Output staging
    /// directory. UUID lengths make the two identities unambiguous.
    fn from_relative_path(path: &Path) -> Option<Self> {
        let mut components = path.components();
        let file = match (components.next(), components.next(), components.next()) {
            (Some(Component::Normal(parent)), Some(Component::Normal(file)), None)
                if parent == OUTPUT_STAGING_DIRECTORY =>
            {
                file.to_str()?
            }
            _ => return None,
        };
        let identities = file
            .strip_prefix(OUTPUT_FILE_PREFIX)?
            .strip_suffix(OUTPUT_STAGING_SUFFIX)?;
        if identities.len() != 73 || identities.as_bytes().get(36) != Some(&b'-') {
            return None;
        }
        Some(Self {
            storage: OutputStorageIdentity::parse(&identities[..36]).ok()?,
            attempt: OutputSaveAttemptIdentity::parse(&identities[37..]).ok()?,
        })
    }
}

/// Proof that the receipt was tombstoned (or its run was removed) before
/// owned output files are removed. `remove_final = false` is reserved for
/// abandoned staging cleanup while a promoted canonical memory retains body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputDeletionCleanup {
    storage: OutputStorageIdentity,
    abandoned_attempt: Option<OutputSaveAttemptIdentity>,
    journal_generation: Option<i64>,
    remove_final: bool,
}

impl OutputDeletionCleanup {
    fn new(
        storage: OutputStorageIdentity,
        abandoned_attempt: Option<OutputSaveAttemptIdentity>,
    ) -> Self {
        Self {
            storage,
            abandoned_attempt,
            journal_generation: None,
            remove_final: true,
        }
    }

    fn with_generation(
        storage: OutputStorageIdentity,
        abandoned_attempt: Option<OutputSaveAttemptIdentity>,
        journal_generation: i64,
    ) -> Self {
        Self {
            storage,
            abandoned_attempt,
            journal_generation: Some(journal_generation),
            remove_final: true,
        }
    }

    fn stage_only_with_generation(
        storage: OutputStorageIdentity,
        abandoned_attempt: OutputSaveAttemptIdentity,
        journal_generation: i64,
    ) -> Self {
        Self {
            storage,
            abandoned_attempt: Some(abandoned_attempt),
            journal_generation: Some(journal_generation),
            remove_final: false,
        }
    }
}

fn parse_owned_relative_path(
    path: &Path,
    directory: &str,
    suffix: &str,
) -> Option<OutputStorageIdentity> {
    let mut components = path.components();
    match (components.next(), components.next(), components.next()) {
        (Some(Component::Normal(parent)), Some(Component::Normal(file)), None)
            if parent == directory =>
        {
            let file = file.to_str()?;
            let uuid = file
                .strip_prefix(OUTPUT_FILE_PREFIX)?
                .strip_suffix(suffix)?;
            OutputStorageIdentity::parse(uuid).ok()
        }
        _ => None,
    }
}

/// Durable receipt states. `Saving` is a conditional claim, not evidence that
/// the upstream workflow node is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputReceiptState {
    NotRequested,
    SkippedEmpty,
    Pending,
    Saving,
    Saved,
    Failed,
    Deleted,
}

impl OutputReceiptState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::SkippedEmpty => "skipped_empty",
            Self::Pending => "pending",
            Self::Saving => "saving",
            Self::Saved => "saved",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, OutputReceiptStateParseError> {
        match value {
            "not_requested" => Ok(Self::NotRequested),
            "skipped_empty" => Ok(Self::SkippedEmpty),
            "pending" => Ok(Self::Pending),
            "saving" => Ok(Self::Saving),
            "saved" => Ok(Self::Saved),
            "failed" => Ok(Self::Failed),
            "deleted" => Ok(Self::Deleted),
            _ => Err(OutputReceiptStateParseError(value.to_owned())),
        }
    }

    pub fn transition(
        self,
        action: OutputReceiptAction,
    ) -> Result<Self, OutputReceiptTransitionError> {
        let target = match (self, action) {
            (Self::Pending | Self::Failed, OutputReceiptAction::StartSave) => Self::Saving,
            (Self::Saving, OutputReceiptAction::CommitSaved) => Self::Saved,
            (Self::Saving, OutputReceiptAction::RecordFailure) => Self::Failed,
            (Self::Saving, OutputReceiptAction::RecoverInterruptedSave) => Self::Pending,
            (
                Self::Pending | Self::Saving | Self::Saved | Self::Failed | Self::Deleted,
                OutputReceiptAction::Tombstone,
            ) => Self::Deleted,
            _ => {
                return Err(OutputReceiptTransitionError {
                    state: self,
                    action,
                })
            }
        };
        Ok(target)
    }
}

impl fmt::Display for OutputReceiptState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown output receipt state `{0}`")]
pub struct OutputReceiptStateParseError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputReceiptAction {
    StartSave,
    CommitSaved,
    RecordFailure,
    RecoverInterruptedSave,
    Tombstone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("cannot apply {action:?} to output receipt in {state} state")]
pub struct OutputReceiptTransitionError {
    pub state: OutputReceiptState,
    pub action: OutputReceiptAction,
}

/// Stable failure categories stored on failed receipts. Raw SQLite and I/O
/// error strings stay out of durable history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputPersistenceErrorCode {
    InvalidDisposition,
    StepOutputUnavailable,
    ArtifactStageIo,
    ArtifactFinalizeIo,
    ArtifactContentMismatch,
    DatabaseWrite,
    Internal,
}

impl OutputPersistenceErrorCode {
    const RETRYABLE: [Self; 3] = [
        Self::ArtifactStageIo,
        Self::ArtifactFinalizeIo,
        Self::DatabaseWrite,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidDisposition => "invalid_disposition",
            Self::StepOutputUnavailable => "step_output_unavailable",
            Self::ArtifactStageIo => "artifact_stage_io",
            Self::ArtifactFinalizeIo => "artifact_finalize_io",
            Self::ArtifactContentMismatch => "artifact_content_mismatch",
            Self::DatabaseWrite => "database_write",
            Self::Internal => "internal",
        }
    }

    pub fn is_retryable(self) -> bool {
        Self::RETRYABLE.contains(&self)
    }

    pub fn parse(value: &str) -> Result<Self, OutputPersistenceErrorCodeParseError> {
        match value {
            "invalid_disposition" => Ok(Self::InvalidDisposition),
            "step_output_unavailable" => Ok(Self::StepOutputUnavailable),
            "artifact_stage_io" => Ok(Self::ArtifactStageIo),
            "artifact_finalize_io" => Ok(Self::ArtifactFinalizeIo),
            "artifact_content_mismatch" => Ok(Self::ArtifactContentMismatch),
            "database_write" => Ok(Self::DatabaseWrite),
            "internal" => Ok(Self::Internal),
            _ => Err(OutputPersistenceErrorCodeParseError(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown output persistence error code `{0}`")]
pub struct OutputPersistenceErrorCodeParseError(String);

/// Values inserted transactionally with a completed Output step. The full body
/// remains in `run_steps.output_json`; the receipt never duplicates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputReceiptSeed {
    key: OutputReceiptKey,
    step_id: String,
    disposition: OutputDisposition,
    resolved_memory_title: String,
    state: OutputReceiptState,
    storage_identity: Option<OutputStorageIdentity>,
    output_sha256: Option<String>,
    artifact_required: bool,
}

impl OutputReceiptSeed {
    pub fn new(
        key: OutputReceiptKey,
        step_id: impl Into<String>,
        disposition: OutputDisposition,
        output_body: &str,
    ) -> Result<Self, OutputIdentityError> {
        let step_id = step_id.into();
        if step_id.trim().is_empty() {
            return Err(OutputIdentityError::EmptyStepId);
        }
        let canonical_body = normalize_body(output_body);
        let state = disposition.initial_receipt_state(&canonical_body);
        let resolved_memory_title = disposition.resolved_memory_title();
        let storage_identity =
            (state == OutputReceiptState::Pending).then(OutputStorageIdentity::new);
        let output_sha256 = storage_identity.map(|_| sha256_hex(canonical_body.as_bytes()));
        let artifact_required =
            storage_identity.is_some() && canonical_body.len() >= OUTPUT_ARTIFACT_BODY_THRESHOLD;
        Ok(Self {
            key,
            step_id,
            disposition,
            resolved_memory_title,
            state,
            storage_identity,
            output_sha256,
            artifact_required,
        })
    }

    pub fn key(&self) -> &OutputReceiptKey {
        &self.key
    }

    pub fn step_id(&self) -> &str {
        &self.step_id
    }

    pub fn disposition(&self) -> &OutputDisposition {
        &self.disposition
    }

    pub fn resolved_memory_title(&self) -> &str {
        &self.resolved_memory_title
    }

    pub fn state(&self) -> OutputReceiptState {
        self.state
    }

    pub fn storage_identity(&self) -> Option<OutputStorageIdentity> {
        self.storage_identity
    }

    pub fn output_sha256(&self) -> Option<&str> {
        self.output_sha256.as_deref()
    }

    pub fn artifact_required(&self) -> bool {
        self.artifact_required
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn output_preview_body(body: &str, max: usize) -> String {
    if body.len() <= max {
        return body.to_owned();
    }
    let mut end = max.min(body.len());
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n\n… [full content stored as artifact]", &body[..end])
}

pub const OUTPUT_PERSISTENCE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS output_receipts (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  step_id TEXT NOT NULL UNIQUE REFERENCES run_steps(id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  save_requested INTEGER NOT NULL CHECK (save_requested IN (0, 1)),
  pin_memory INTEGER NOT NULL CHECK (pin_memory IN (0, 1)),
  memory_title_override TEXT NOT NULL,
  resolved_memory_title TEXT NOT NULL,
  as_final_result INTEGER NOT NULL CHECK (as_final_result IN (0, 1)),
  html_report INTEGER NOT NULL CHECK (html_report IN (0, 1)),
  include_files_changed INTEGER NOT NULL CHECK (include_files_changed IN (0, 1)),
  state TEXT NOT NULL CHECK (state IN (
    'not_requested', 'skipped_empty', 'pending', 'saving', 'saved', 'failed', 'deleted'
  )),
  storage_id TEXT UNIQUE,
  output_sha256 TEXT,
  artifact_required INTEGER NOT NULL DEFAULT 0 CHECK (artifact_required IN (0, 1)),
  memory_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
  error_code TEXT CHECK (error_code IS NULL OR error_code IN (
    'invalid_disposition', 'step_output_unavailable', 'artifact_stage_io',
    'artifact_finalize_io', 'artifact_content_mismatch', 'database_write', 'internal'
  )),
  saving_started_at TEXT,
  claim_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  PRIMARY KEY (run_id, node_id),
  CHECK (
    (state = 'not_requested' AND save_requested = 0 AND storage_id IS NULL
      AND output_sha256 IS NULL AND memory_id IS NULL AND error_code IS NULL)
    OR (state = 'skipped_empty' AND save_requested = 1 AND storage_id IS NULL
      AND output_sha256 IS NULL AND memory_id IS NULL AND error_code IS NULL)
    OR (state IN ('pending', 'saving') AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NULL AND error_code IS NULL)
    OR (state = 'saved' AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NOT NULL AND memory_id = storage_id AND error_code IS NULL)
    OR (state = 'failed' AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NULL AND error_code IS NOT NULL)
    OR (state = 'deleted' AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NULL AND error_code IS NULL)
  ),
  CHECK ((state = 'deleted') = (deleted_at IS NOT NULL)),
  CHECK (
    (state = 'saving' AND saving_started_at IS NOT NULL AND claim_id IS NOT NULL)
    OR (state != 'saving' AND saving_started_at IS NULL AND claim_id IS NULL)
  )
);
CREATE INDEX IF NOT EXISTS idx_output_receipts_recovery
  ON output_receipts(state, saving_started_at, updated_at);
CREATE INDEX IF NOT EXISTS idx_output_receipts_memory_id
  ON output_receipts(memory_id) WHERE memory_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS output_cleanup_journal (
  storage_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL DEFAULT '',
  generation INTEGER NOT NULL CHECK (generation > 0),
  remove_final INTEGER NOT NULL CHECK (remove_final IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (storage_id, attempt_id)
);
CREATE INDEX IF NOT EXISTS idx_output_cleanup_journal_updated
  ON output_cleanup_journal(updated_at, storage_id, attempt_id);
"#;
fn ensure_output_active_attempts_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS output_active_attempts (
           storage_id TEXT NOT NULL,
           attempt_id TEXT NOT NULL,
           PRIMARY KEY (storage_id, attempt_id)
         )",
    )?;
    Ok(())
}

fn mark_output_attempt_active_conn(
    conn: &Connection,
    storage: OutputStorageIdentity,
    attempt: OutputSaveAttemptIdentity,
) -> Result<(), DbError> {
    ensure_output_active_attempts_table(conn)?;
    conn.execute(
        "INSERT INTO output_active_attempts (storage_id, attempt_id)
         VALUES (?1, ?2)
         ON CONFLICT(storage_id, attempt_id) DO NOTHING",
        params![storage.memory_id(), attempt.to_string()],
    )?;
    Ok(())
}

fn clear_output_attempt_active_conn(
    conn: &Connection,
    storage: OutputStorageIdentity,
    attempt: OutputSaveAttemptIdentity,
) -> Result<(), DbError> {
    ensure_output_active_attempts_table(conn)?;
    conn.execute(
        "DELETE FROM output_active_attempts
          WHERE storage_id = ?1 AND attempt_id = ?2",
        params![storage.memory_id(), attempt.to_string()],
    )?;
    Ok(())
}

fn output_attempt_active_conn(
    conn: &Connection,
    storage: OutputStorageIdentity,
    attempt: OutputSaveAttemptIdentity,
) -> Result<bool, DbError> {
    ensure_output_active_attempts_table(conn)?;
    conn.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM output_active_attempts
            WHERE storage_id = ?1 AND attempt_id = ?2
         )",
        params![storage.memory_id(), attempt.to_string()],
        |row| row.get(0),
    )
    .map_err(Into::into)
}


fn enqueue_output_cleanup_journal_conn(
    conn: &Connection,
    storage: OutputStorageIdentity,
    attempt: Option<OutputSaveAttemptIdentity>,
    remove_final: bool,
    queued_at: &str,
) -> Result<i64, DbError> {
    let storage_id = storage.memory_id();
    let attempt_id = attempt.map(|value| value.to_string()).unwrap_or_default();
    conn.execute(
        "INSERT INTO output_cleanup_journal
            (storage_id, attempt_id, generation, remove_final, created_at, updated_at)
         VALUES (?1, ?2, 1, ?3, ?4, ?4)
         ON CONFLICT(storage_id, attempt_id) DO UPDATE SET
            generation = generation + 1,
            remove_final = CASE
              WHEN remove_final = 1 OR excluded.remove_final = 1
              THEN 1 ELSE 0 END,
            updated_at = excluded.updated_at",
        params![storage_id, attempt_id, remove_final, queued_at],
    )?;
    conn.query_row(
        "SELECT generation FROM output_cleanup_journal
          WHERE storage_id = ?1 AND attempt_id = ?2",
        params![storage.memory_id(), attempt_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}
fn cleanup_generation_conn(
    conn: &Connection,
    storage: OutputStorageIdentity,
    attempt: Option<OutputSaveAttemptIdentity>,
) -> Result<Option<i64>, DbError> {
    conn.query_row(
        "SELECT generation FROM output_cleanup_journal
          WHERE storage_id = ?1 AND attempt_id = ?2",
        params![
            storage.memory_id(),
            attempt.map(|value| value.to_string()).unwrap_or_default(),
        ],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}



pub(crate) fn create_output_persistence_schema(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(OUTPUT_PERSISTENCE_SCHEMA)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputReceipt {
    #[serde(flatten)]
    key: OutputReceiptKey,
    pub step_id: String,
    pub disposition: OutputDisposition,
    pub resolved_memory_title: String,
    pub state: OutputReceiptState,
    pub storage_identity: Option<OutputStorageIdentity>,
    pub output_sha256: Option<String>,
    pub artifact_required: bool,
    pub memory_id: Option<String>,
    pub error_code: Option<OutputPersistenceErrorCode>,
    pub saving_started_at: Option<String>,
    #[serde(skip)]
    claim_identity: Option<OutputSaveAttemptIdentity>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl OutputReceipt {
    pub fn key(&self) -> &OutputReceiptKey {
        &self.key
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputClaimMode {
    Initial,
    ExplicitRetry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSaveClaim {
    pub receipt: OutputReceipt,
    pub output_body: String,
    attempt_identity: OutputSaveAttemptIdentity,
}

impl OutputSaveClaim {
    fn storage_identity(&self) -> Result<OutputStorageIdentity, DbError> {
        self.receipt
            .storage_identity
            .ok_or_else(|| DbError::Other("saving output receipt has no storage identity".into()))
    }

    fn staging_identity(&self) -> Result<OutputStagingIdentity, DbError> {
        Ok(OutputStagingIdentity::new(
            self.storage_identity()?,
            self.attempt_identity,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputClaimOutcome {
    Claimed(OutputSaveClaim),
    NotClaimable(OutputReceipt),
    SourceUnavailable(OutputReceipt),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputPersistenceOutcome {
    Saved(OutputReceipt),
    Failed(OutputReceipt),
    NotClaimable(OutputReceipt),
    SourceUnavailable(OutputReceipt),
    Superseded(Option<OutputReceipt>),
}

impl OutputPersistenceOutcome {
    pub fn into_receipt(self) -> Option<OutputReceipt> {
        match self {
            Self::Saved(receipt)
            | Self::Failed(receipt)
            | Self::NotClaimable(receipt)
            | Self::SourceUnavailable(receipt) => Some(receipt),
            Self::Superseded(receipt) => receipt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredOutputSave {
    pub receipt: OutputReceipt,
    abandoned_attempt: OutputSaveAttemptIdentity,
    journal_generation: i64,
}

impl RecoveredOutputSave {
    fn staging_identity(&self) -> Result<OutputStagingIdentity, DbError> {
        let storage = self.receipt.storage_identity.ok_or_else(|| {
            DbError::Other("recovered output receipt has no storage identity".into())
        })?;
        Ok(OutputStagingIdentity::new(storage, self.abandoned_attempt))
    }

    fn cleanup(&self) -> Option<OutputDeletionCleanup> {
        self.receipt.storage_identity.map(|storage| {
            OutputDeletionCleanup::stage_only_with_generation(
                storage,
                self.abandoned_attempt,
                self.journal_generation,
            )
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct OutputRecoveryItem {
    key: OutputReceiptKey,
    recovered: Option<RecoveredOutputSave>,
}

fn receipt_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutputReceipt> {
    let state_text: String = row.get(11)?;
    let state = OutputReceiptState::parse(&state_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(11, Type::Text, Box::new(error))
    })?;
    let storage_text: Option<String> = row.get(12)?;
    let storage_identity = storage_text
        .as_deref()
        .map(OutputStorageIdentity::parse)
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(12, Type::Text, Box::new(error))
        })?;
    let error_text: Option<String> = row.get(16)?;
    let error_code = error_text
        .as_deref()
        .map(OutputPersistenceErrorCode::parse)
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(16, Type::Text, Box::new(error))
        })?;
    let claim_text: Option<String> = row.get(18)?;
    let claim_identity = claim_text
        .as_deref()
        .map(OutputSaveAttemptIdentity::parse)
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(18, Type::Text, Box::new(error))
        })?;
    Ok(OutputReceipt {
        key: OutputReceiptKey {
            run_id: row.get(0)?,
            node_id: row.get(1)?,
        },
        step_id: row.get(2)?,
        disposition: OutputDisposition {
            label: row.get(3)?,
            save_to_memory: row.get(4)?,
            pin_memory: row.get(5)?,
            memory_title: row.get(6)?,
            as_final_result: row.get(8)?,
            html_report: row.get(9)?,
            include_files_changed: row.get(10)?,
        },
        resolved_memory_title: row.get(7)?,
        state,
        storage_identity,
        output_sha256: row.get(13)?,
        artifact_required: row.get(14)?,
        memory_id: row.get(15)?,
        error_code,
        saving_started_at: row.get(17)?,
        claim_identity,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        deleted_at: row.get(21)?,
    })
}

fn load_output_receipt_conn(
    conn: &Connection,
    key: &OutputReceiptKey,
) -> Result<Option<OutputReceipt>, DbError> {
    conn.query_row(
        "SELECT run_id, node_id, step_id, label, save_requested, pin_memory,
                memory_title_override, resolved_memory_title, as_final_result,
                html_report, include_files_changed, state, storage_id, output_sha256,
                artifact_required, memory_id, error_code, saving_started_at,
                claim_id, created_at, updated_at, deleted_at
           FROM output_receipts WHERE run_id = ?1 AND node_id = ?2",
        params![key.run_id(), key.node_id()],
        receipt_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn insert_output_receipt(
    conn: &Connection,
    seed: &OutputReceiptSeed,
    created_at: &str,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO output_receipts (
           run_id, node_id, step_id, label, save_requested, pin_memory,
           memory_title_override, resolved_memory_title, as_final_result,
           html_report, include_files_changed, state, storage_id, output_sha256,
           artifact_required, created_at, updated_at
         ) VALUES (
           ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
           ?15, ?16, ?16
         )",
        params![
            seed.key().run_id(),
            seed.key().node_id(),
            seed.step_id(),
            &seed.disposition().label,
            seed.disposition().save_to_memory,
            seed.disposition().pin_memory,
            &seed.disposition().memory_title,
            seed.resolved_memory_title(),
            seed.disposition().as_final_result,
            seed.disposition().html_report,
            seed.disposition().include_files_changed,
            seed.state().as_str(),
            seed.storage_identity().map(|identity| identity.memory_id()),
            seed.output_sha256(),
            seed.artifact_required(),
            created_at,
        ],
    )?;
    Ok(())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl Db {
    pub fn get_output_receipt(
        &self,
        key: &OutputReceiptKey,
    ) -> Result<Option<OutputReceipt>, DbError> {
        self.with_conn(|conn| load_output_receipt_conn(conn, key))
    }

    pub fn list_output_receipts_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<OutputReceipt>, DbError> {
        self.with_conn(|conn| list_output_receipts_for_run_conn(conn, run_id))
    }

    pub fn claim_output_save(
        &self,
        key: &OutputReceiptKey,
        mode: OutputClaimMode,
    ) -> Result<OutputClaimOutcome, DbError> {
        self.claim_output_save_at(key, mode, &now())
    }

    fn claim_output_save_at(
        &self,
        key: &OutputReceiptKey,
        mode: OutputClaimMode,
        claimed_at: &str,
    ) -> Result<OutputClaimOutcome, DbError> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let attempt_identity = OutputSaveAttemptIdentity::new();
            let changed = match mode {
                OutputClaimMode::Initial => transaction.execute(
                    "UPDATE output_receipts
                        SET state = 'saving', saving_started_at = ?1, error_code = NULL,
                            claim_id = ?4, updated_at = ?1
                      WHERE run_id = ?2 AND node_id = ?3 AND state = 'pending'",
                    params![
                        claimed_at,
                        key.run_id(),
                        key.node_id(),
                        attempt_identity.to_string(),
                    ],
                )?,
                OutputClaimMode::ExplicitRetry => transaction.execute(
                    "UPDATE output_receipts
                        SET state = 'saving', saving_started_at = ?1, error_code = NULL,
                            claim_id = ?4, updated_at = ?1
                      WHERE run_id = ?2 AND node_id = ?3 AND state = 'failed'
                        AND error_code IN (?5, ?6, ?7)",
                    params![
                        claimed_at,
                        key.run_id(),
                        key.node_id(),
                        attempt_identity.to_string(),
                        OutputPersistenceErrorCode::RETRYABLE[0].as_str(),
                        OutputPersistenceErrorCode::RETRYABLE[1].as_str(),
                        OutputPersistenceErrorCode::RETRYABLE[2].as_str(),
                    ],
                )?,
            };
            if changed == 0 {
                let receipt = load_output_receipt_conn(&transaction, key)?.ok_or_else(|| {
                    DbError::Other(format!(
                        "output receipt not found: {}/{}",
                        key.run_id(),
                        key.node_id()
                    ))
                })?;
                transaction.commit()?;
                return Ok(OutputClaimOutcome::NotClaimable(receipt));
            }

            let receipt = load_output_receipt_conn(&transaction, key)?
                .ok_or_else(|| DbError::Other("claimed output receipt disappeared".into()))?;
            let storage_identity = receipt.storage_identity.ok_or_else(|| {
                DbError::Other("claimed output receipt has no storage identity".into())
            })?;
            mark_output_attempt_active_conn(
                &transaction,
                storage_identity,
                attempt_identity,
            )?;
            let output_json: Option<String> = transaction
                .query_row(
                    "SELECT output_json FROM run_steps WHERE id = ?1 AND run_id = ?2",
                    params![receipt.step_id, key.run_id()],
                    |row| row.get(0),
                )
                .optional()?;
            let output_body = output_json
                .as_deref()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                .and_then(|value| {
                    value
                        .get("text")
                        .and_then(Value::as_str)
                        .map(normalize_body)
                });
            let source_matches = output_body.as_deref().is_some_and(|body| {
                let actual_digest = sha256_hex(body.as_bytes());
                receipt.output_sha256.as_deref() == Some(actual_digest.as_str())
            });
            if !source_matches {
                transaction.execute(
                    "UPDATE output_receipts
                        SET state = 'failed', saving_started_at = NULL,
                            claim_id = NULL, error_code = 'step_output_unavailable', updated_at = ?1
                      WHERE run_id = ?2 AND node_id = ?3 AND state = 'saving'
                        AND claim_id = ?4",
                    params![
                        claimed_at,
                        key.run_id(),
                        key.node_id(),
                        attempt_identity.to_string(),
                    ],
                )?;
                clear_output_attempt_active_conn(
                    &transaction,
                    storage_identity,
                    attempt_identity,
                )?;
                let failed = load_output_receipt_conn(&transaction, key)?
                    .ok_or_else(|| DbError::Other("failed output receipt disappeared".into()))?;
                transaction.commit()?;
                return Ok(OutputClaimOutcome::SourceUnavailable(failed));
            }
            transaction.commit()?;
            Ok(OutputClaimOutcome::Claimed(OutputSaveClaim {
                receipt,
                output_body: output_body.expect("source match requires output body"),
                attempt_identity,
            }))
        })
    }

    pub fn record_output_save_failure(
        &self,
        claim: &OutputSaveClaim,
        error_code: OutputPersistenceErrorCode,
    ) -> Result<bool, DbError> {
        let result = self.record_output_save_failure_at(claim, error_code, &now());
        if let Ok(storage) = claim.storage_identity() {
            if let Err(error) = self.clear_output_attempt_active(storage, claim.attempt_identity) {
                eprintln!("failed to clear output save attempt marker: {error}");
            }
        }
        result
    }

    fn record_output_save_failure_at(
        &self,
        claim: &OutputSaveClaim,
        error_code: OutputPersistenceErrorCode,
        failed_at: &str,
    ) -> Result<bool, DbError> {
        let storage_identity = claim.storage_identity()?;
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            enqueue_output_cleanup_journal_conn(
                &transaction,
                storage_identity,
                Some(claim.attempt_identity),
                false,
                failed_at,
            )?;
            let changed = transaction.execute(
                "UPDATE output_receipts
                    SET state = 'failed', saving_started_at = NULL, error_code = ?1,
                        claim_id = NULL, updated_at = ?2
                  WHERE run_id = ?3 AND node_id = ?4 AND state = 'saving'
                    AND storage_id = ?5 AND claim_id = ?6",
                params![
                    error_code.as_str(),
                    failed_at,
                    claim.receipt.key().run_id(),
                    claim.receipt.key().node_id(),
                    storage_identity.memory_id(),
                    claim.attempt_identity.to_string(),
                ],
            )?;
            transaction.commit()?;
            Ok(changed == 1)
        })
    }
    /// Convert an unclaimed pending receipt into a durable, retryable failure.
    ///
    /// This is safe after a claim/transaction error because the caller never
    /// received an owned saving claim. Saving rows are intentionally untouched:
    /// only their fencing token may transition them.
    pub(crate) fn record_output_save_failure_without_claim(
        &self,
        key: &OutputReceiptKey,
        error_code: OutputPersistenceErrorCode,
        sink: Option<&dyn OutputPersistenceEventSink>,
    ) -> Result<Option<OutputReceipt>, DbError> {
        let receipt = self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE output_receipts
                    SET state = 'failed', saving_started_at = NULL, error_code = ?1,
                        claim_id = NULL, updated_at = ?2
                  WHERE run_id = ?3 AND node_id = ?4 AND state = 'pending'",
                params![
                    error_code.as_str(),
                    now(),
                    key.run_id(),
                    key.node_id(),
                ],
            )?;
            if changed == 1 {
                load_output_receipt_conn(conn, key)
            } else {
                Ok(None)
            }
        })?;
        if let Some(receipt) = &receipt {
            self.notify_output_persistence(sink, receipt);
        }
        Ok(receipt)
    }


    /// Capture the exact pending and saving work that existed before startup
    /// recovery. Saving rows retain their claim identity so a later worker
    /// cannot be stolen; pending rows are captured as unclaimed work.
    ///
    /// The snapshot lives in a SQLite temporary table rather than an
    /// unbounded Rust vector. It is populated atomically before any recovery
    /// work starts, so claims created by this process after the snapshot
    /// cannot prolong this startup pass.
    pub fn capture_output_recovery_snapshot(&self) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS output_recovery_claims (
                   run_id TEXT NOT NULL,
                   node_id TEXT NOT NULL,
                   claim_id TEXT,
                   deferred INTEGER NOT NULL DEFAULT 0,
                   PRIMARY KEY (run_id, node_id)
                 )",
            )?;
            transaction.execute("DELETE FROM output_recovery_claims", [])?;
            transaction.execute(
                "INSERT INTO output_recovery_claims (run_id, node_id, claim_id)
                   SELECT run_id, node_id, claim_id
                     FROM output_receipts
                    WHERE state IN ('pending', 'saving')",
                [],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }
    /// Retain a current-process claim for a later bounded recovery pass after
    /// persistence failed. Deferred rows are excluded from this startup pass,
    /// so one bad claim cannot starve independent snapshot work.
    pub(crate) fn requeue_output_recovery_item(
        &self,
        key: &OutputReceiptKey,
    ) -> Result<bool, DbError> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS output_recovery_claims (
                   run_id TEXT NOT NULL,
                   node_id TEXT NOT NULL,
                   claim_id TEXT,
                   deferred INTEGER NOT NULL DEFAULT 0,
                   PRIMARY KEY (run_id, node_id)
                 )",
            )?;
            let Some(receipt) = load_output_receipt_conn(&transaction, key)? else {
                transaction.commit()?;
                return Ok(false);
            };
            let claim_id = match receipt.state {
                OutputReceiptState::Pending => None,
                OutputReceiptState::Saving => receipt.claim_identity.map(|claim| claim.to_string()),
                _ => {
                    transaction.commit()?;
                    return Ok(false);
                }
            };
            transaction.execute(
                "INSERT INTO output_recovery_claims
                    (run_id, node_id, claim_id, deferred)
                 VALUES (?1, ?2, ?3, 1)
                 ON CONFLICT(run_id, node_id) DO UPDATE SET
                    claim_id = excluded.claim_id, deferred = 1",
                params![key.run_id(), key.node_id(), claim_id],
            )?;
            transaction.commit()?;
            Ok(true)
        })
    }
    pub(crate) fn acknowledge_output_cleanup(
        &self,
        cleanup: OutputDeletionCleanup,
    ) -> Result<(), DbError> {
        let Some(generation) = cleanup.journal_generation else {
            return Ok(());
        };
        self.with_conn(|conn| {
            ensure_output_active_attempts_table(conn)?;
            conn.execute(
                "DELETE FROM output_cleanup_journal
                  WHERE storage_id = ?1 AND attempt_id = ?2 AND generation = ?3
                    AND (?4 = 1 OR remove_final = 0)
                    AND NOT EXISTS (
                      SELECT 1 FROM output_active_attempts
                       WHERE storage_id = ?1 AND attempt_id = ?2
                    )",
                params![
                    cleanup.storage.memory_id(),
                    cleanup
                        .abandoned_attempt
                        .map(|attempt| attempt.to_string())
                        .unwrap_or_default(),
                    generation,
                    cleanup.remove_final,
                ],
            )?;
            Ok(())
        })
    }

    fn clear_output_attempt_active(
        &self,
        storage: OutputStorageIdentity,
        attempt: OutputSaveAttemptIdentity,
    ) -> Result<(), DbError> {
        self.with_conn(|conn| clear_output_attempt_active_conn(conn, storage, attempt))
    }

    pub(crate) fn output_cleanup_is_active(
        &self,
        cleanup: OutputDeletionCleanup,
    ) -> Result<bool, DbError> {
        let Some(attempt) = cleanup.abandoned_attempt else {
            return Ok(false);
        };
        self.with_conn(|conn| output_attempt_active_conn(conn, cleanup.storage, attempt))
    }

    /// Process a bounded journal batch. Rows remain durable when a path is
    /// rejected, an active matching claim still owns its stage, or removal
    /// fails; the next startup can retry them without scanning user paths.
    pub(crate) fn process_output_cleanup_journal(
        &self,
        artifacts: &OutputArtifactStore,
        limit: usize,
    ) -> Result<usize, DbError> {
        let limit = limit.clamp(1, 100);
        let rows = self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT storage_id, attempt_id, generation, remove_final
                   FROM output_cleanup_journal
                  ORDER BY updated_at, storage_id, attempt_id
                  LIMIT ?1",
            )?;
            let rows = statement
                .query_map(params![limit as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, bool>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;

        let mut processed = 0usize;
        let mut first_error = None;
        for (storage_text, attempt_text, generation, remove_final) in rows {
            let storage = match OutputStorageIdentity::parse(&storage_text) {
                Ok(storage) => storage,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(DbError::Other(format!(
                            "invalid output cleanup storage identity: {error}"
                        )));
                    }
                    continue;
                }
            };
            let attempt = if attempt_text.is_empty() {
                None
            } else {
                match OutputSaveAttemptIdentity::parse(&attempt_text) {
                    Ok(attempt) => Some(attempt),
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(DbError::Other(format!(
                                "invalid output cleanup attempt identity: {error}"
                            )));
                        }
                        continue;
                    }
                }
            };
            if let Some(attempt) = attempt {
                let active = self.with_conn(|conn| {
                    ensure_output_active_attempts_table(conn)?;
                    let durable: bool = conn.query_row(
                        "SELECT EXISTS(
                           SELECT 1 FROM output_receipts
                            WHERE storage_id = ?1 AND state = 'saving' AND claim_id = ?2
                         )",
                        params![storage.memory_id(), attempt.to_string()],
                        |row| row.get(0),
                    )?;
                    if durable {
                        Ok(true)
                    } else {
                        output_attempt_active_conn(conn, storage, attempt)
                    }
                })?;
                if active {
                    continue;
                }
            }

            let canonical_exists = self.with_conn(|conn| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1)",
                    params![storage.memory_id()],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(Into::into)
            })?;
            let cleanup = OutputDeletionCleanup::with_generation(storage, attempt, generation);
            let mut removal_error = None;
            if remove_final && !canonical_exists {
                if let Err(error) = remove_owned_output_file_checked(&artifacts.root, storage) {
                    removal_error = Some(error);
                }
            }
            if let Some(attempt) = attempt {
                let stage = OutputStagingIdentity::new(storage, attempt);
                if let Err(error) =
                    remove_owned_file_checked(&artifacts.root, &stage.relative_path())
                {
                    if removal_error.is_none() {
                        removal_error = Some(error);
                    }
                }
            }
            if let Some(error) = removal_error {
                if first_error.is_none() {
                    first_error = Some(DbError::Io(error));
                }
                continue;
            }
            if canonical_exists && remove_final {
                if let Err(error) = self.acknowledge_output_cleanup(cleanup) {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    continue;
                }
                processed = processed.saturating_add(1);
                continue;
            }
            if let Err(error) = self.acknowledge_output_cleanup(cleanup) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                continue;
            }
            processed = processed.saturating_add(1);
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(processed),
        }
    }



    fn recover_captured_output_saves(
        &self,
        limit: usize,
    ) -> Result<(Vec<OutputRecoveryItem>, bool), DbError> {
        let limit = limit.clamp(1, 100);
        let recovered_at = now();
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS output_recovery_claims (
                   run_id TEXT NOT NULL,
                   node_id TEXT NOT NULL,
                   claim_id TEXT,
                   deferred INTEGER NOT NULL DEFAULT 0,
                   PRIMARY KEY (run_id, node_id)
                 )",
            )?;
            let claims = {
                let mut statement = transaction.prepare(
                    "SELECT run_id, node_id, claim_id
                       FROM output_recovery_claims
                      WHERE deferred = 0
                      ORDER BY run_id, node_id
                      LIMIT ?1",
                )?;
                let rows = statement
                    .query_map(params![limit as i64], |row| {
                        Ok((
                            OutputReceiptKey {
                                run_id: row.get(0)?,
                                node_id: row.get(1)?,
                            },
                            row.get::<_, Option<String>>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let mut recovered = Vec::with_capacity(claims.len());
            for (key, captured_claim_id) in claims {
                let recovered_attempt = captured_claim_id
                    .as_deref()
                    .map(OutputSaveAttemptIdentity::parse)
                    .transpose()
                    .map_err(|error| {
                        DbError::Other(format!("invalid output save attempt identity: {error}"))
                    })?;
                let mut journal_generation = None;
                let changed = if let Some(attempt) = recovered_attempt {
                    let current = load_output_receipt_conn(&transaction, &key)?;
                    if let Some(receipt) = current.as_ref().filter(|receipt| {
                        receipt.state == OutputReceiptState::Saving
                            && receipt.claim_identity == Some(attempt)
                    }) {
                        let storage = receipt.storage_identity.ok_or_else(|| {
                            DbError::Other("saving output receipt has no storage identity".into())
                        })?;
                        journal_generation = Some(enqueue_output_cleanup_journal_conn(
                            &transaction,
                            storage,
                            Some(attempt),
                            false,
                            &recovered_at,
                        )?);
                    }
                    transaction.execute(
                        "UPDATE output_receipts
                            SET state = 'pending', saving_started_at = NULL, claim_id = NULL,
                                updated_at = ?1
                          WHERE run_id = ?2 AND node_id = ?3 AND state = 'saving'
                            AND claim_id = ?4",
                        params![
                            recovered_at,
                            key.run_id(),
                            key.node_id(),
                            attempt.to_string(),
                        ],
                    )?
                } else {
                    0
                };
                transaction.execute(
                    "DELETE FROM output_recovery_claims
                      WHERE run_id = ?1 AND node_id = ?2
                        AND (claim_id = ?3 OR (claim_id IS NULL AND ?3 IS NULL))",
                    params![key.run_id(), key.node_id(), captured_claim_id.as_deref()],
                )?;

                let item = if changed == 1 {
                    load_output_receipt_conn(&transaction, &key)?.map(|receipt| {
                        OutputRecoveryItem {
                            key: key.clone(),
                            recovered: Some(RecoveredOutputSave {
                                receipt,
                                abandoned_attempt: recovered_attempt
                                    .expect("saving recovery has an attempt identity"),
                                journal_generation: journal_generation
                                    .expect("saving recovery has a cleanup journal generation"),
                            }),
                        }
                    })
                } else if captured_claim_id.is_none() {
                    load_output_receipt_conn(&transaction, &key)?
                        .filter(|receipt| receipt.state == OutputReceiptState::Pending)
                        .map(|_| OutputRecoveryItem {
                            key: key.clone(),
                            recovered: None,
                        })
                } else {
                    None
                };
                if let Some(item) = item {
                    recovered.push(item);
                }
            }
            let has_more: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM output_recovery_claims WHERE deferred = 0)",
                [],
                |row| row.get(0),
            )?;
            transaction.commit()?;
            Ok((recovered, has_more))
        })
    }

    /// Authorize final-file cleanup for a worker that lost its claim only when
    /// the durable lifecycle can no longer accept a replacement save.
    pub fn deletion_cleanup_for_lost_claim(
        &self,
        claim: &OutputSaveClaim,
    ) -> Result<Option<OutputDeletionCleanup>, DbError> {
        let storage = claim.storage_identity()?;
        self.with_conn(|conn| {
            ensure_output_active_attempts_table(conn)?;
            if output_attempt_active_conn(conn, storage, claim.attempt_identity)? {
                return Ok(None);
            }
            let canonical_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1)",
                params![storage.memory_id()],
                |row| row.get(0),
            )?;
            let attempt = Some(claim.attempt_identity);
            if canonical_exists {
                let active: bool = conn.query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM output_receipts
                        WHERE storage_id = ?1 AND state = 'saving' AND claim_id = ?2
                     )",
                    params![storage.memory_id(), claim.attempt_identity.to_string()],
                    |row| row.get(0),
                )?;
                if active {
                    return Ok(None);
                }
                let generation = match cleanup_generation_conn(conn, storage, attempt)? {
                    Some(generation) => generation,
                    None => enqueue_output_cleanup_journal_conn(
                        conn,
                        storage,
                        attempt,
                        false,
                        &now(),
                    )?,
                };
                return Ok(Some(OutputDeletionCleanup::stage_only_with_generation(
                    storage,
                    claim.attempt_identity,
                    generation,
                )));
            }

            match load_output_receipt_conn(conn, claim.receipt.key())? {
                Some(receipt)
                    if receipt.state == OutputReceiptState::Deleted
                        && receipt.storage_identity == Some(storage) =>
                {
                    let cleanup = cleanup_generation_conn(conn, storage, attempt)?
                        .map(|generation| {
                            OutputDeletionCleanup::with_generation(
                                storage,
                                attempt,
                                generation,
                            )
                        })
                        .unwrap_or_else(|| OutputDeletionCleanup::new(storage, attempt));
                    Ok(Some(cleanup))
                }
                None => {
                    let run_exists: bool = conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM runs WHERE id = ?1)",
                        params![claim.receipt.key().run_id()],
                        |row| row.get(0),
                    )?;
                    if !run_exists {
                        let cleanup = cleanup_generation_conn(conn, storage, attempt)?
                            .map(|generation| {
                                OutputDeletionCleanup::with_generation(
                                    storage,
                                    attempt,
                                    generation,
                                )
                            })
                            .unwrap_or_else(|| OutputDeletionCleanup::new(storage, attempt));
                        Ok(Some(cleanup))
                    } else {
                        Ok(None)
                    }
                }
                _ => Ok(None),
            }
        })
    }

    /// Claim and persist one Output receipt without rerunning its workflow
    /// node. The canonical row and saved transition share one transaction.
    pub fn persist_output_receipt(
        &self,
        key: &OutputReceiptKey,
        mode: OutputClaimMode,
        artifacts: &OutputArtifactStore,
        sink: Option<&dyn OutputPersistenceEventSink>,
    ) -> Result<OutputPersistenceOutcome, DbError> {
        let claim = match self.claim_output_save(key, mode) {
            Ok(OutputClaimOutcome::Claimed(claim)) => claim,
            Ok(OutputClaimOutcome::NotClaimable(receipt)) => {
                return Ok(OutputPersistenceOutcome::NotClaimable(receipt));
            }
            Ok(OutputClaimOutcome::SourceUnavailable(receipt)) => {
                self.notify_output_persistence(sink, &receipt);
                return Ok(OutputPersistenceOutcome::SourceUnavailable(receipt));
            }
            Err(error) => {
                if let Err(failure_error) = self.record_output_save_failure_without_claim(
                    key,
                    OutputPersistenceErrorCode::DatabaseWrite,
                    sink,
                ) {
                    eprintln!(
                        "failed to record unclaimed output persistence failure for {}/{}: {failure_error}",
                        key.run_id(),
                        key.node_id()
                    );
                }
                return Err(error);
            }
        };

        if validate_title(&claim.receipt.resolved_memory_title).is_err() {
            return self.finish_output_failure(
                &claim,
                OutputPersistenceErrorCode::InvalidDisposition,
                artifacts,
                sink,
            );
        }

        let artifact_path = match artifacts.ensure_for_claim(&claim) {
            Ok(path) => path,
            Err(error) => {
                return self.finish_output_failure(&claim, error.code(), artifacts, sink);
            }
        };
        let storage = claim.storage_identity()?;
        let saved_at = now();
        let artifact_path = artifact_path.map(|path| path.to_string_lossy().into_owned());
        let write = self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let workflow_id: String = transaction.query_row(
                "SELECT workflow_id FROM runs WHERE id = ?1",
                params![claim.receipt.key().run_id()],
                |row| row.get(0),
            )?;
            let record = CanonicalMemoryRecord {
                id: storage.memory_id(),
                workflow_id: Some(workflow_id.clone()),
                run_id: Some(claim.receipt.key().run_id().to_owned()),
                node_id: Some(claim.receipt.key().node_id().to_owned()),
                scope_type: MemoryScopeType::Workflow,
                scope_key: workflow_id,
                kind: if artifact_path.is_some() {
                    "artifact".into()
                } else {
                    "text".into()
                },
                memory_type: if artifact_path.is_some() {
                    MemoryType::Artifact
                } else {
                    MemoryType::Output
                },
                source: "run".into(),
                title: claim.receipt.resolved_memory_title.clone(),
                body: if artifact_path.is_some() {
                    output_preview_body(&claim.output_body, 2_000)
                } else {
                    claim.output_body.clone()
                },
                artifact_path: artifact_path.clone(),
                artifact_content_digest: artifact_path.as_ref().map(|_| {
                    super::memory_validity::artifact_content_digest(claim.output_body.as_bytes())
                }),
                pinned: claim.receipt.disposition.pin_memory,
                confidence: 1.0,
                salience: 50,
                status: MemoryStatus::Active,
                supersedes_id: None,
                fact_key: None,
                observed_at: None,
                last_confirmed_at: None,
                valid_from: None,
                expires_at: None,
                created_at: claim.receipt.created_at.clone(),
            };
            write_canonical_memory(&transaction, &record)?;
            if let Some(content_digest) = record.artifact_content_digest.as_deref() {
                super::artifact_chunks::persist_artifact_chunk_manifest_conn(
                    &transaction,
                    &record.id,
                    content_digest,
                    claim.output_body.as_bytes(),
                    &claim.receipt.created_at,
                )?;
            }
            mark_output_receipt_saved(&transaction, &claim, &saved_at)?;
            let receipt = load_output_receipt_conn(&transaction, key)?
                .ok_or_else(|| DbError::Other("saved output receipt disappeared".into()))?;
            transaction.commit()?;
            Ok(receipt)
        });

        match write {
            Ok(receipt) => {
                artifacts.remove_claim_staging(&claim);
                if let Err(error) = self.clear_output_attempt_active(
                    claim.storage_identity()?,
                    claim.attempt_identity,
                ) {
                    eprintln!("failed to clear output save attempt marker: {error}");
                }
                self.notify_output_persistence(sink, &receipt);
                Ok(OutputPersistenceOutcome::Saved(receipt))
            }
            Err(_) => self.finish_output_failure(
                &claim,
                OutputPersistenceErrorCode::DatabaseWrite,
                artifacts,
                sink,
            ),
        }
    }

    /// Process one bounded batch from the startup snapshot. The returned
    /// boolean reflects unconsumed snapshot rows, not successful outcomes:
    /// tombstoned, concurrently claimed, or failed rows must not strand the
    /// rest of a larger backlog.
    pub fn recover_and_persist_output_snapshot_batch(
        &self,
        limit: usize,
        artifacts: &OutputArtifactStore,
        sink: Option<&dyn OutputPersistenceEventSink>,
    ) -> Result<(Vec<OutputPersistenceOutcome>, bool), DbError> {
        let limit = limit.clamp(1, 100);
        let (items, has_more) = self.recover_captured_output_saves(limit)?;
        let mut outcomes = Vec::with_capacity(items.len());
        let mut first_error = None;
        for item in items {
            if let Some(recovered) = &item.recovered {
                match artifacts.remove_recovered_staging(recovered) {
                    Ok(()) => {
                        if let Some(cleanup) = recovered.cleanup() {
                        if let Some(attempt) = cleanup.abandoned_attempt {
                            if let Err(error) = self.clear_output_attempt_active(
                                cleanup.storage,
                                attempt,
                            ) {
                                eprintln!(
                                    "failed to clear recovered output attempt marker: {error}"
                                );
                            }
                        }
                            if let Err(error) = self.acknowledge_output_cleanup(cleanup) {
                                eprintln!(
                                    "failed to acknowledge recovered output staging cleanup: {error}"
                                );
                                if first_error.is_none() {
                                    first_error = Some(error);
                                }
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("recovered output staging cleanup failed: {error}");
                        if first_error.is_none() {
                            first_error = Some(DbError::Io(error));
                        }
                    }
                }
            }
            match self.persist_output_receipt(
                &item.key,
                OutputClaimMode::Initial,
                artifacts,
                sink,
            ) {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    match self.record_output_save_failure_without_claim(
                        &item.key,
                        OutputPersistenceErrorCode::DatabaseWrite,
                        sink,
                    ) {
                        Ok(Some(receipt)) => {
                            outcomes.push(OutputPersistenceOutcome::Failed(receipt));
                        }
                        Ok(None) => match self.requeue_output_recovery_item(&item.key) {
                            Ok(true) => {
                                eprintln!(
                                    "deferred output persistence recovery for {}/{}: {error}",
                                    item.key.run_id(),
                                    item.key.node_id()
                                );
                            }
                            Ok(false) => {}
                            Err(requeue_error) => {
                                if first_error.is_none() {
                                    first_error = Some(requeue_error);
                                }
                            }
                        },
                        Err(failure_error) => match self.requeue_output_recovery_item(&item.key) {
                            Ok(true) => {
                                eprintln!(
                                    "deferred output persistence recovery for {}/{}: {failure_error}",
                                    item.key.run_id(),
                                    item.key.node_id()
                                );
                            }
                            Ok(false) => {
                                if first_error.is_none() {
                                    first_error = Some(failure_error);
                                }
                            }
                            Err(requeue_error) => {
                                if first_error.is_none() {
                                    first_error = Some(requeue_error);
                                }
                            }
                        },
                    }
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok((outcomes, has_more)),
        }
    }
    fn finish_output_failure(
        &self,
        claim: &OutputSaveClaim,
        error_code: OutputPersistenceErrorCode,
        artifacts: &OutputArtifactStore,
        sink: Option<&dyn OutputPersistenceEventSink>,
    ) -> Result<OutputPersistenceOutcome, DbError> {
        artifacts.remove_claim_staging(claim);
        let recorded = self.record_output_save_failure(claim, error_code)?;
        if recorded {
            let receipt = self
                .get_output_receipt(claim.receipt.key())?
                .ok_or_else(|| DbError::Other("failed output receipt disappeared".into()))?;
            self.notify_output_persistence(sink, &receipt);
            return Ok(OutputPersistenceOutcome::Failed(receipt));
        }

        if let Some(cleanup) = self.deletion_cleanup_for_lost_claim(claim)? {
            if let Err(error) = artifacts.remove_deleted_files_checked(cleanup) {
                eprintln!("deferred output deletion cleanup failed: {error}");
            } else if let Err(error) = self.acknowledge_output_cleanup(cleanup) {
                eprintln!("failed to acknowledge output deletion cleanup: {error}");
            }
        }
        Ok(OutputPersistenceOutcome::Superseded(
            self.get_output_receipt(claim.receipt.key())?,
        ))
    }

    fn notify_output_persistence(
        &self,
        sink: Option<&dyn OutputPersistenceEventSink>,
        receipt: &OutputReceipt,
    ) {
        let workflow_id = self.with_conn(|conn| {
            conn.query_row(
                "SELECT workflow_id FROM runs WHERE id = ?1",
                params![receipt.key().run_id()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Into::into)
        });
        if let Ok(Some(workflow_id)) = workflow_id {
            notify_output_persistence(
                sink,
                &OutputPersistenceEvent {
                    workflow_id,
                    receipt: receipt.clone(),
                },
            );
        }
    }
}

pub(crate) fn list_output_receipts_for_run_conn(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<OutputReceipt>, DbError> {
    let mut statement = conn.prepare(
        "SELECT run_id, node_id, step_id, label, save_requested, pin_memory,
                memory_title_override, resolved_memory_title, as_final_result,
                html_report, include_files_changed, state, storage_id, output_sha256,
                artifact_required, memory_id, error_code, saving_started_at,
                claim_id, created_at, updated_at, deleted_at
           FROM output_receipts WHERE run_id = ?1
           ORDER BY created_at, node_id",
    )?;
    let rows = statement.query_map(params![run_id], receipt_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn tombstone_output_receipt_by_key(
    conn: &Connection,
    key: &OutputReceiptKey,
    deleted_at: &str,
) -> Result<Option<OutputDeletionCleanup>, DbError> {
    let receipt = load_output_receipt_conn(conn, key)?.ok_or_else(|| {
        DbError::Other(format!(
            "output receipt not found: {}/{}",
            key.run_id(),
            key.node_id()
        ))
    })?;
    let journal_generation = receipt
        .storage_identity
        .map(|storage| {
            enqueue_output_cleanup_journal_conn(
                conn,
                storage,
                receipt.claim_identity,
                true,
                deleted_at,
            )
        })
        .transpose()?;
    let changed = conn.execute(
        "UPDATE output_receipts
            SET state = 'deleted', memory_id = NULL, error_code = NULL,
                saving_started_at = NULL, claim_id = NULL,
                deleted_at = ?1, updated_at = ?1
          WHERE run_id = ?2 AND node_id = ?3
            AND state IN ('pending', 'saving', 'saved', 'failed')",
        params![deleted_at, key.run_id(), key.node_id()],
    )?;
    Ok((changed == 1)
        .then_some(receipt.storage_identity)
        .flatten()
        .map(|storage| {
            OutputDeletionCleanup::with_generation(
                storage,
                receipt.claim_identity,
                journal_generation.expect("storage identity has cleanup journal generation"),
            )
        }))
}
pub(crate) fn mark_output_receipt_saved(
    conn: &Connection,
    claim: &OutputSaveClaim,
    saved_at: &str,
) -> Result<(), DbError> {
    let storage_identity = claim.storage_identity()?;
    let storage_id = storage_identity.memory_id();
    let changed = conn.execute(
        "UPDATE output_receipts
            SET state = 'saved', memory_id = ?1, error_code = NULL,
                saving_started_at = NULL, claim_id = NULL, updated_at = ?2
          WHERE run_id = ?3 AND node_id = ?4 AND state = 'saving'
            AND storage_id = ?1 AND claim_id = ?5",
        params![
            storage_id,
            saved_at,
            claim.receipt.key().run_id(),
            claim.receipt.key().node_id(),
            claim.attempt_identity.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(DbError::Other(
            "output receipt no longer owns the save claim".into(),
        ));
    }
    Ok(())
}


pub(crate) fn tombstone_output_receipt_for_memory(
    conn: &Connection,
    memory_id: &str,
    deleted_at: &str,
) -> Result<Option<OutputDeletionCleanup>, DbError> {
    let storage_id: Option<String> = conn
        .query_row(
            "SELECT storage_id FROM output_receipts
              WHERE memory_id = ?1 AND state = 'saved'",
            params![memory_id],
            |row| row.get(0),
        )
        .optional()?;
    let storage = storage_id
        .map(|value| {
            OutputStorageIdentity::parse(&value).map_err(|error| {
                DbError::Other(format!("invalid output storage identity: {error}"))
            })
        })
        .transpose()?;
    let journal_generation = storage
        .map(|storage| enqueue_output_cleanup_journal_conn(conn, storage, None, true, deleted_at))
        .transpose()?;
    conn.execute(
        "UPDATE output_receipts
            SET state = 'deleted', memory_id = NULL, error_code = NULL,
                saving_started_at = NULL, claim_id = NULL,
                deleted_at = ?1, updated_at = ?1
          WHERE memory_id = ?2 AND state = 'saved'",
        params![deleted_at, memory_id],
    )?;
    Ok(storage.map(|storage| {
        OutputDeletionCleanup::with_generation(
            storage,
            None,
            journal_generation.expect("storage identity has cleanup journal generation"),
        )
    }))
}
pub(crate) fn tombstone_output_receipts_for_workflow(
    conn: &Connection,
    workflow_id: &str,
    deleted_at: &str,
) -> Result<Vec<OutputDeletionCleanup>, DbError> {
    // Unsaved claims are fenced by origin workflow. Saved receipts are
    // tombstoned only when their canonical memory is one of the workflow rows
    // that the caller is about to delete; user/workspace promotions remain
    // live and retain their artifact.
    let owned_claims = {
        let mut statement = conn.prepare(
            "SELECT r.storage_id, r.claim_id
               FROM output_receipts r
              WHERE r.run_id IN (SELECT id FROM runs WHERE workflow_id = ?1)
                AND r.storage_id IS NOT NULL
                AND (
                  r.state IN ('pending', 'saving', 'failed')
                  OR (
                    r.state = 'saved'
                    AND EXISTS (
                      SELECT 1 FROM memories m
                       WHERE m.id = r.memory_id
                         AND m.workflow_id = ?1
                         AND m.scope_type = 'workflow'
                    )
                  )
                )",
        )?;
        let rows = statement
            .query_map(params![workflow_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let cleanups = owned_claims
        .into_iter()
        .map(|(storage, attempt)| {
            let storage = OutputStorageIdentity::parse(&storage).map_err(|error| {
                DbError::Other(format!("invalid output storage identity: {error}"))
            })?;
            let attempt = attempt
                .as_deref()
                .map(OutputSaveAttemptIdentity::parse)
                .transpose()
                .map_err(|error| {
                    DbError::Other(format!("invalid output save attempt identity: {error}"))
                })?;
            let journal_generation =
                enqueue_output_cleanup_journal_conn(conn, storage, attempt, true, deleted_at)?;
            Ok(OutputDeletionCleanup::with_generation(
                storage,
                attempt,
                journal_generation,
            ))
        })
        .collect::<Result<Vec<_>, DbError>>()?;
    conn.execute(
        "UPDATE output_receipts
            SET state = 'deleted', memory_id = NULL, error_code = NULL,
                saving_started_at = NULL, claim_id = NULL,
                deleted_at = ?1, updated_at = ?1
          WHERE run_id IN (SELECT id FROM runs WHERE workflow_id = ?2)
            AND (
              state IN ('pending', 'saving', 'failed')
              OR (
                state = 'saved'
                AND memory_id IN (
                  SELECT id FROM memories
                   WHERE workflow_id = ?2 AND scope_type = 'workflow'
                )
              )
            )",
        params![deleted_at, workflow_id],
    )?;
    Ok(cleanups)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPersistenceEvent {
    pub workflow_id: String,
    pub receipt: OutputReceipt,
}

pub trait OutputPersistenceEventSink {
    fn emit(&self, event: &OutputPersistenceEvent) -> Result<(), String>;
}

/// Notifications are best effort and always follow the durable transition.
pub fn notify_output_persistence(
    sink: Option<&dyn OutputPersistenceEventSink>,
    event: &OutputPersistenceEvent,
) {
    if let Some(sink) = sink {
        let _ = sink.emit(event);
    }
}

#[derive(Debug, Error)]
pub enum OutputArtifactError {
    #[error("output artifact staging failed: {0}")]
    StageIo(#[source] io::Error),
    #[error("output artifact finalization failed: {0}")]
    FinalizeIo(#[source] io::Error),
    #[error("owned output artifact content does not match its receipt")]
    ContentMismatch,
}

impl OutputArtifactError {
    pub fn code(&self) -> OutputPersistenceErrorCode {
        match self {
            Self::StageIo(_) => OutputPersistenceErrorCode::ArtifactStageIo,
            Self::FinalizeIo(_) => OutputPersistenceErrorCode::ArtifactFinalizeIo,
            Self::ContentMismatch => OutputPersistenceErrorCode::ArtifactContentMismatch,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutputArtifactStore {
    root: PathBuf,
}

impl OutputArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn ensure_for_claim(
        &self,
        claim: &OutputSaveClaim,
    ) -> Result<Option<PathBuf>, OutputArtifactError> {
        if !claim.receipt.artifact_required {
            return Ok(None);
        }
        let identity = claim
            .receipt
            .storage_identity
            .ok_or(OutputArtifactError::ContentMismatch)?;
        let expected_digest = claim
            .receipt
            .output_sha256
            .as_deref()
            .ok_or(OutputArtifactError::ContentMismatch)?;
        if sha256_hex(claim.output_body.as_bytes()) != expected_digest {
            return Err(OutputArtifactError::ContentMismatch);
        }

        let staging_identity = OutputStagingIdentity::new(identity, claim.attempt_identity);
        let stage_relative = staging_identity.relative_path();
        let artifact_relative = identity.artifact_relative_path();
        ensure_owned_output_directory(&self.root, &stage_relative)
            .map_err(OutputArtifactError::StageIo)?;
        ensure_owned_output_directory(&self.root, &artifact_relative)
            .map_err(OutputArtifactError::StageIo)?;
        let stage_path = trusted_artifact_target(&self.root, &stage_relative)
            .ok_or_else(|| {
                OutputArtifactError::StageIo(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "output staging path is not owned by the artifact root",
                ))
            })?;
        let artifact_path = trusted_artifact_target(&self.root, &artifact_relative)
            .ok_or_else(|| {
                OutputArtifactError::FinalizeIo(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "output artifact path is not owned by the artifact root",
                ))
            })?;
        if artifact_path.exists() {
            verify_file_digest(
                &artifact_path,
                expected_digest,
                OutputArtifactError::FinalizeIo,
            )?;
            remove_owned_file_checked(&self.root, &stage_relative)
                .map_err(OutputArtifactError::StageIo)?;
            return Ok(Some(artifact_path));
        }

        match write_new_stage(&stage_path, claim.output_body.as_bytes()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                match verify_file_digest(
                    &stage_path,
                    expected_digest,
                    OutputArtifactError::StageIo,
                ) {
                    Ok(()) => {}
                    Err(OutputArtifactError::ContentMismatch) => {
                        remove_owned_file_checked(&self.root, &stage_relative)
                            .map_err(OutputArtifactError::StageIo)?;
                        write_new_stage(&stage_path, claim.output_body.as_bytes())
                            .map_err(OutputArtifactError::StageIo)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(OutputArtifactError::StageIo(error)),
        }
        verify_file_digest(
            &stage_path,
            expected_digest,
            OutputArtifactError::StageIo,
        )?;
        match fs::hard_link(&stage_path, &artifact_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                verify_file_digest(
                    &artifact_path,
                    expected_digest,
                    OutputArtifactError::FinalizeIo,
                )?;
            }
            Err(error) => return Err(OutputArtifactError::FinalizeIo(error)),
        }
        verify_file_digest(
            &artifact_path,
            expected_digest,
            OutputArtifactError::FinalizeIo,
        )?;
        remove_owned_file_checked(&self.root, &stage_relative)
            .map_err(OutputArtifactError::StageIo)?;
        Ok(Some(artifact_path))
    }

    /// Remove only the temporary file owned by this exact claim attempt.
    pub fn remove_claim_staging(&self, claim: &OutputSaveClaim) {
        if let Ok(staging_identity) = claim.staging_identity() {
            let _ = remove_owned_file_checked(&self.root, &staging_identity.relative_path());
        }
    }

    /// Remove the exact abandoned staging file returned by one bounded
    /// recovery transition.
    pub fn remove_recovered_staging(
        &self,
        recovered: &RecoveredOutputSave,
    ) -> io::Result<()> {
        let staging_identity = recovered.staging_identity().map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, error.to_string())
        })?;
        remove_owned_file_checked(&self.root, &staging_identity.relative_path())
    }

    /// Remove final and temporary files only after a transaction returns proof
    /// that their receipt is deliberately deleted.
    pub fn remove_deleted_files_checked(
        &self,
        cleanup: OutputDeletionCleanup,
    ) -> io::Result<()> {
        if cleanup.remove_final {
            remove_owned_output_file_checked(&self.root, cleanup.storage)?;
        }
        if let Some(attempt) = cleanup.abandoned_attempt {
            let staging_identity = OutputStagingIdentity::new(cleanup.storage, attempt);
            remove_owned_file_checked(&self.root, &staging_identity.relative_path())?;
        }
        Ok(())
    }

    /// Best-effort compatibility wrapper for callers that cannot surface
    /// cleanup failures; the durable journal retains any failed removal.
    pub fn remove_deleted_files(&self, cleanup: OutputDeletionCleanup) {
        let _ = self.remove_deleted_files_checked(cleanup);
    }
}
fn remove_owned_file_checked(root: &Path, relative: &Path) -> io::Result<()> {
    if owned_output_directory_missing(root, relative)? {
        return Ok(());
    }
    let resolved = trusted_artifact_target(root, relative).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to remove an untrusted output path",
        )
    })?;
    match fs::symlink_metadata(&resolved) {
        Ok(_) => fs::remove_file(resolved),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn ensure_owned_output_directory(root: &Path, relative: &Path) -> io::Result<()> {
    let canonical_root = fs::canonicalize(root)?;
    let Component::Normal(directory) = relative.components().next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "empty output path")
    })? else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path has non-normal component",
        ));
    };
    let directory = canonical_root.join(directory);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "output directory is a symlink",
        )),
        Ok(metadata) if !metadata.is_dir() => Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "output directory is not a directory",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            match fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "output directory is a symlink",
                )),
                Ok(metadata) if metadata.is_dir() => Ok(()),
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "output directory is not a directory",
                )),
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn owned_output_directory_missing(root: &Path, relative: &Path) -> io::Result<bool> {
    let canonical_root = fs::canonicalize(root)?;
    let Component::Normal(directory) = relative.components().next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "empty output path")
    })? else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path has non-normal component",
        ));
    };
    let directory = canonical_root.join(directory);
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "output directory is a symlink",
        )),
        Ok(metadata) if !metadata.is_dir() => Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "output directory is not a directory",
        )),
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}
fn remove_owned_output_file_checked(
    root: &Path,
    storage: OutputStorageIdentity,
) -> io::Result<()> {
    let relative = storage.artifact_relative_path();
    if owned_output_directory_missing(root, &relative)? {
        return Ok(());
    }
    let resolved = trusted_artifact_target(root, &relative).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to remove an untrusted output artifact",
        )
    })?;
    match fs::symlink_metadata(&resolved) {
        Ok(_) => {
            let resolved = trusted_output_artifact_path(root, &relative).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing to remove an untrusted output artifact",
                )
            })?;
            fs::remove_file(resolved)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn write_new_stage(path: &Path, body: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = file.write_all(body).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

fn verify_file_digest(
    path: &Path,
    expected_digest: &str,
    map_io_error: fn(io::Error) -> OutputArtifactError,
) -> Result<(), OutputArtifactError> {
    let mut file = File::open(path).map_err(map_io_error)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(map_io_error)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    if format!("{:x}", digest.finalize()) == expected_digest {
        Ok(())
    } else {
        Err(OutputArtifactError::ContentMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use serde_json::json;

    struct ArtifactTestDirectory {
        path: PathBuf,
    }

    impl ArtifactTestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("alfred-output-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create output artifact test directory");
            Self { path }
        }
    }

    impl Drop for ArtifactTestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn insert_run(db: &Db, run_id: &str) {
        db.with_conn(|conn| {
            create_output_persistence_schema(conn)?;
            conn.execute(
                "INSERT OR IGNORE INTO workflows
                 (id, name, created_at, updated_at) VALUES ('workflow-1', 'Workflow', 't0', 't0')",
                [],
            )?;
            conn.execute(
                "INSERT INTO runs (id, workflow_id, created_at)
                 VALUES (?1, 'workflow-1', 't0')",
                params![run_id],
            )?;
            Ok(())
        })
        .expect("insert run fixture");
    }

    fn insert_receipt(
        db: &Db,
        run_id: &str,
        node_id: &str,
        step_id: &str,
        body: &str,
        disposition: OutputDisposition,
    ) -> OutputReceiptKey {
        insert_run(db, run_id);
        let key = OutputReceiptKey::new(run_id, node_id).unwrap();
        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let output_json = serde_json::to_string(&json!({ "text": body }))
                .map_err(|error| DbError::Other(error.to_string()))?;
            transaction.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, status, input_json, output_json, created_at)
                 VALUES (?1, ?2, ?3, 'completed', '{}', ?4, 't1')",
                params![step_id, run_id, node_id, output_json],
            )?;
            let seed = OutputReceiptSeed::new(key.clone(), step_id, disposition, body)
                .map_err(|error| DbError::Other(error.to_string()))?;
            insert_output_receipt(&transaction, &seed, "t1")?;
            transaction.commit()?;
            Ok(())
        })
        .expect("insert step and receipt fixture");
        key
    }

    fn insert_output_memory(
        conn: &Connection,
        receipt: &OutputReceipt,
        body: &str,
    ) -> Result<(), DbError> {
        let storage_id = receipt
            .storage_identity
            .expect("requested receipt has storage identity")
            .memory_id();
        conn.execute(
            "INSERT INTO memories
             (id, workflow_id, run_id, node_id, scope_type, scope_key,
              memory_type, source, title, body, pinned, created_at, updated_at)
             VALUES (?1, 'workflow-1', ?2, ?3, 'workflow', 'workflow-1', 'output',
                     'run', ?4, ?5, ?6, 't3', 't3')",
            params![
                storage_id,
                receipt.key().run_id(),
                receipt.key().node_id(),
                &receipt.resolved_memory_title,
                body,
                receipt.disposition.pin_memory,
            ],
        )?;
        Ok(())
    }
    #[test]
    fn output_persistence_schema_is_repeatable_and_does_not_backfill_old_steps() {
        let db = Db::open_in_memory().unwrap();
        insert_run(&db, "legacy-run");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, status, created_at)
                 VALUES ('legacy-step', 'legacy-run', 'output-1', 'completed', 't0')",
                [],
            )?;
            create_output_persistence_schema(conn)?;
            create_output_persistence_schema(conn)?;
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM output_receipts", [], |row| row.get(0))?;
            assert_eq!(count, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn output_persistence_schema_rejects_saved_rows_with_nullable_identity_fields() {
        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-invalid-saved",
            "output-1",
            "step-invalid-saved",
            "body",
            OutputDisposition::default(),
        );
        let receipt = db.get_output_receipt(&key).unwrap().unwrap();
        let storage = receipt.storage_identity.unwrap();

        db.with_conn(|conn| {
            insert_output_memory(conn, &receipt, "body")?;
            let null_hash = conn.execute(
                "UPDATE output_receipts
                    SET state = 'saved', output_sha256 = NULL, memory_id = ?1
                  WHERE run_id = ?2 AND node_id = ?3",
                params![storage.memory_id(), key.run_id(), key.node_id()],
            );
            assert!(null_hash.is_err());

            let null_memory = conn.execute(
                "UPDATE output_receipts SET state = 'saved', memory_id = NULL
                  WHERE run_id = ?1 AND node_id = ?2",
                params![key.run_id(), key.node_id()],
            );
            assert!(null_memory.is_err());
            assert_eq!(
                load_output_receipt_conn(conn, &key)?.unwrap().state,
                OutputReceiptState::Pending
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn output_persistence_facade_writes_one_canonical_row_and_fts_entry() {
        let db = Db::open_in_memory().unwrap();
        let mut disposition = OutputDisposition::default();
        disposition.memory_title = "  Durable result  ".into();
        disposition.pin_memory = true;
        let key = insert_receipt(
            &db,
            "run-facade",
            "output-1",
            "step-facade",
            "canonical result",
            disposition,
        );
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);

        let saved = match db
            .persist_output_receipt(&key, OutputClaimMode::Initial, &store, None)
            .unwrap()
        {
            OutputPersistenceOutcome::Saved(receipt) => receipt,
            other => panic!("expected saved output, got {other:?}"),
        };
        let memory_id = saved.memory_id.as_deref().unwrap();
        let memory = db.get_memory(memory_id).unwrap().unwrap();
        assert_eq!(memory.id, saved.storage_identity.unwrap().memory_id());
        assert_eq!(memory.workflow_id.as_deref(), Some("workflow-1"));
        assert_eq!(memory.run_id.as_deref(), Some("run-facade"));
        assert_eq!(memory.node_id.as_deref(), Some("output-1"));
        assert_eq!(memory.scope_type, MemoryScopeType::Workflow);
        assert_eq!(memory.scope_key, "workflow-1");
        assert_eq!(memory.kind, "text");
        assert_eq!(memory.memory_type, MemoryType::Output);
        assert_eq!(memory.source, "run");
        assert_eq!(memory.title, "Durable result");
        assert_eq!(memory.body, "canonical result");
        assert!(memory.pinned);
        assert_eq!(memory.confidence, 1.0);
        assert_eq!(memory.salience, 50);
        assert_eq!(memory.status, MemoryStatus::Active);
        assert_eq!(memory.provenance_state, "legacy_unknown");
        db.with_conn(|conn| {
            let indexed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_id = ?1",
                params![memory_id],
                |row| row.get(0),
            )?;
            assert_eq!(indexed, 1);
            Ok(())
        })
        .unwrap();

        assert!(matches!(
            db.persist_output_receipt(&key, OutputClaimMode::Initial, &store, None)
                .unwrap(),
            OutputPersistenceOutcome::NotClaimable(OutputReceipt {
                state: OutputReceiptState::Saved,
                ..
            })
        ));
        db.with_conn(|conn| {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
            assert_eq!(count, 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn output_persistence_facade_keeps_identical_outputs_distinct_by_run() {
        let db = Db::open_in_memory().unwrap();
        let body = "same artifact body\n".repeat(2_000);
        let first = insert_receipt(
            &db,
            "run-facade-one",
            "output-1",
            "step-facade-one",
            &body,
            OutputDisposition {
                memory_title: "First title".into(),
                ..OutputDisposition::default()
            },
        );
        let second = insert_receipt(
            &db,
            "run-facade-two",
            "output-1",
            "step-facade-two",
            &body,
            OutputDisposition {
                memory_title: "Second title".into(),
                ..OutputDisposition::default()
            },
        );
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        for key in [&first, &second] {
            assert!(matches!(
                db.persist_output_receipt(key, OutputClaimMode::Initial, &store, None)
                    .unwrap(),
                OutputPersistenceOutcome::Saved(_)
            ));
        }

        let first_receipt = db.get_output_receipt(&first).unwrap().unwrap();
        let second_receipt = db.get_output_receipt(&second).unwrap().unwrap();
        let first_memory = db
            .get_memory(first_receipt.memory_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        let second_memory = db
            .get_memory(second_receipt.memory_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_ne!(first_memory.id, second_memory.id);
        assert_ne!(first_memory.artifact_path, second_memory.artifact_path);
        assert_eq!(
            fs::read_to_string(first_memory.artifact_path.as_deref().unwrap()).unwrap(),
            normalize_body(&body)
        );
        assert_eq!(
            fs::read_to_string(second_memory.artifact_path.as_deref().unwrap()).unwrap(),
            normalize_body(&body)
        );
        assert_eq!(
            std::collections::HashSet::from([
                first_memory.title.as_str(),
                second_memory.title.as_str()
            ]),
            std::collections::HashSet::from(["First title", "Second title"])
        );
    }

    #[test]
    fn output_persistence_facade_rejects_invalid_captured_title_before_writing() {
        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-invalid-title",
            "output-1",
            "step-invalid-title",
            "valid body",
            OutputDisposition {
                memory_title: "x".repeat(161),
                ..OutputDisposition::default()
            },
        );
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);

        let failed = match db
            .persist_output_receipt(&key, OutputClaimMode::Initial, &store, None)
            .unwrap()
        {
            OutputPersistenceOutcome::Failed(receipt) => receipt,
            other => panic!("expected invalid disposition failure, got {other:?}"),
        };
        assert_eq!(
            failed.error_code,
            Some(OutputPersistenceErrorCode::InvalidDisposition)
        );
        assert!(matches!(
            db.persist_output_receipt(&key, OutputClaimMode::ExplicitRetry, &store, None)
                .unwrap(),
            OutputPersistenceOutcome::NotClaimable(_)
        ));
        db.with_conn(|conn| {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
            assert_eq!(count, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn output_persistence_facade_records_db_failure_and_retries_storage_only() {
        use std::cell::Cell;

        struct FailingSink(Cell<usize>);
        impl OutputPersistenceEventSink for FailingSink {
            fn emit(&self, _event: &OutputPersistenceEvent) -> Result<(), String> {
                self.0.set(self.0.get() + 1);
                Err("listener unavailable".into())
            }
        }

        let db = Db::open_in_memory().unwrap();
        let body = "retry without rerun\n".repeat(2_000);
        let key = insert_receipt(
            &db,
            "run-facade-retry",
            "output-1",
            "step-facade-retry",
            &body,
            OutputDisposition::default(),
        );
        db.with_conn(|conn| {
            conn.execute_batch(
                "CREATE TEMP TRIGGER inject_output_memory_failure
                 BEFORE INSERT ON memories
                 BEGIN SELECT RAISE(ABORT, 'injected output memory failure'); END;",
            )?;
            Ok(())
        })
        .unwrap();
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        let sink = FailingSink(Cell::new(0));

        let failed = match db
            .persist_output_receipt(&key, OutputClaimMode::Initial, &store, Some(&sink))
            .unwrap()
        {
            OutputPersistenceOutcome::Failed(receipt) => receipt,
            other => panic!("expected durable save failure, got {other:?}"),
        };
        assert_eq!(
            failed.error_code,
            Some(OutputPersistenceErrorCode::DatabaseWrite)
        );
        assert_eq!(sink.0.get(), 1);
        assert!(failed.memory_id.is_none());
        let artifact = directory
            .path
            .join(failed.storage_identity.unwrap().artifact_relative_path());
        assert_eq!(
            fs::read_to_string(&artifact).unwrap(),
            normalize_body(&body)
        );
        db.with_conn(|conn| {
            conn.execute_batch("DROP TRIGGER inject_output_memory_failure")?;
            let step_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_steps WHERE id = 'step-facade-retry'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(step_count, 1);
            Ok(())
        })
        .unwrap();

        assert!(matches!(
            db.persist_output_receipt(&key, OutputClaimMode::ExplicitRetry, &store, Some(&sink),)
                .unwrap(),
            OutputPersistenceOutcome::Saved(_)
        ));
        assert_eq!(sink.0.get(), 2);
        assert_eq!(
            fs::read_to_string(&artifact).unwrap(),
            normalize_body(&body)
        );
        db.with_conn(|conn| {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
            assert_eq!(count, 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn output_persistence_facade_recovers_large_output_from_step_and_owned_stage() {
        let db = Db::open_in_memory().unwrap();
        let body = "recovered canonical report\n".repeat(2_000);
        let key = insert_receipt(
            &db,
            "run-facade-recover",
            "output-1",
            "step-facade-recover",
            &body,
            OutputDisposition::default(),
        );
        let interrupted = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "2026-01-01T00:00:00Z")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected interrupted claim, got {other:?}"),
        };
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        let abandoned_stage = directory
            .path
            .join(interrupted.staging_identity().unwrap().relative_path());
        fs::create_dir_all(abandoned_stage.parent().unwrap()).unwrap();
        fs::write(&abandoned_stage, "partial").unwrap();

        db.capture_output_recovery_snapshot().unwrap();
        let (outcomes, has_more) = db
            .recover_and_persist_output_snapshot_batch(1, &store, None)
            .unwrap();
        assert!(!has_more);
        assert!(matches!(
            outcomes.as_slice(),
            [OutputPersistenceOutcome::Saved(_)]
        ));
        assert!(!abandoned_stage.exists());
        let receipt = db.get_output_receipt(&key).unwrap().unwrap();
        let memory = db
            .get_memory(receipt.memory_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(memory.memory_type, MemoryType::Artifact);
        assert_eq!(memory.kind, "artifact");
        assert!(memory.body.ends_with("[full content stored as artifact]"));
        assert_eq!(
            fs::read_to_string(memory.artifact_path.as_deref().unwrap()).unwrap(),
            normalize_body(&body)
        );
    }

    #[test]
    fn output_persistence_facade_drains_never_claimed_pending_receipts_in_bounded_batches() {
        let db = Db::open_in_memory().unwrap();
        let first = insert_receipt(
            &db,
            "run-pending-one",
            "output-1",
            "step-pending-one",
            "first pending body",
            OutputDisposition::default(),
        );
        let second = insert_receipt(
            &db,
            "run-pending-two",
            "output-1",
            "step-pending-two",
            "second pending body",
            OutputDisposition::default(),
        );
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        db.capture_output_recovery_snapshot().unwrap();
        let (first_batch, first_has_more) = db
            .recover_and_persist_output_snapshot_batch(1, &store, None)
            .unwrap();
        assert!(first_has_more);
        assert_eq!(first_batch.len(), 1);
        assert!(matches!(first_batch[0], OutputPersistenceOutcome::Saved(_)));
        let states = [first.clone(), second.clone()]
            .map(|key| db.get_output_receipt(&key).unwrap().unwrap().state);
        assert_eq!(
            states
                .iter()
                .filter(|state| **state == OutputReceiptState::Saved)
                .count(),
            1
        );
        assert_eq!(
            states
                .iter()
                .filter(|state| **state == OutputReceiptState::Pending)
                .count(),
            1
        );

        let (second_batch, second_has_more) = db
            .recover_and_persist_output_snapshot_batch(1, &store, None)
            .unwrap();
        assert!(!second_has_more);
        assert_eq!(second_batch.len(), 1);
        assert_eq!(
            db.get_output_receipt(&first).unwrap().unwrap().state,
            OutputReceiptState::Saved
        );
        assert_eq!(
            db.get_output_receipt(&second).unwrap().unwrap().state,
            OutputReceiptState::Saved
        );
    }

    #[test]
    fn output_persistence_receipt_is_atomic_with_step_and_keeps_captured_settings() {
        let db = Db::open_in_memory().unwrap();
        let mut disposition = OutputDisposition::default();
        disposition.pin_memory = true;
        disposition.memory_title = "  Captured title  ".into();
        let key = insert_receipt(&db, "run-1", "output-1", "step-1", "body", disposition);

        let receipt = db.get_output_receipt(&key).unwrap().unwrap();
        assert_eq!(receipt.state, OutputReceiptState::Pending);
        assert!(receipt.disposition.pin_memory);
        assert_eq!(receipt.resolved_memory_title, "Captured title");
        assert_eq!(
            receipt.output_sha256.as_deref(),
            Some(sha256_hex(b"body").as_str())
        );

        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, status, created_at)
                 VALUES ('step-replay', 'run-1', 'output-1', 'completed', 't2')",
                [],
            )?;
            let replay = OutputReceiptSeed::new(
                key.clone(),
                "step-replay",
                OutputDisposition::default(),
                "body",
            )
            .map_err(|error| DbError::Other(error.to_string()))?;
            assert!(insert_output_receipt(&transaction, &replay, "t2").is_err());
            drop(transaction);
            let replay_step_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_steps WHERE id = 'step-replay'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(replay_step_count, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn output_persistence_claims_initial_and_retry_work_without_rerunning_the_step() {
        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-claim",
            "output-1",
            "step-claim",
            "  canonical body\r\n",
            OutputDisposition::default(),
        );

        let claimed = db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap();
        let claim = match claimed {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert_eq!(claim.output_body, "canonical body");
        assert_eq!(claim.receipt.state, OutputReceiptState::Saving);
        assert!(matches!(
            db.claim_output_save_at(&key, OutputClaimMode::Initial, "t3")
                .unwrap(),
            OutputClaimOutcome::NotClaimable(OutputReceipt {
                state: OutputReceiptState::Saving,
                ..
            })
        ));

        assert!(db
            .record_output_save_failure_at(&claim, OutputPersistenceErrorCode::DatabaseWrite, "t4",)
            .unwrap());
        assert!(matches!(
            db.claim_output_save_at(&key, OutputClaimMode::Initial, "t5")
                .unwrap(),
            OutputClaimOutcome::NotClaimable(OutputReceipt {
                state: OutputReceiptState::Failed,
                ..
            })
        ));
        assert!(matches!(
            db.claim_output_save_at(&key, OutputClaimMode::ExplicitRetry, "t6")
                .unwrap(),
            OutputClaimOutcome::Claimed(OutputSaveClaim {
                receipt: OutputReceipt {
                    state: OutputReceiptState::Saving,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn output_persistence_changed_step_source_fails_closed_and_cannot_retry() {
        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-source",
            "output-1",
            "step-source",
            "original",
            OutputDisposition::default(),
        );
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE run_steps SET output_json = '{\"text\":\"changed\"}' WHERE id = 'step-source'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        assert!(matches!(
            db.claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
                .unwrap(),
            OutputClaimOutcome::SourceUnavailable(OutputReceipt {
                state: OutputReceiptState::Failed,
                error_code: Some(OutputPersistenceErrorCode::StepOutputUnavailable),
                ..
            })
        ));
        assert!(matches!(
            db.claim_output_save_at(&key, OutputClaimMode::ExplicitRetry, "t3")
                .unwrap(),
            OutputClaimOutcome::NotClaimable(_)
        ));
    }

    #[test]
    fn output_persistence_save_and_delete_each_commit_with_their_memory_change() {
        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-save",
            "output-1",
            "step-save",
            "saved body",
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let storage = claim.receipt.storage_identity.unwrap();

        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            insert_output_memory(&transaction, &claim.receipt, &claim.output_body)?;
            mark_output_receipt_saved(&transaction, &claim, "t3")?;
            transaction.commit()?;
            Ok(())
        })
        .unwrap();
        let saved = db.get_output_receipt(&key).unwrap().unwrap();
        assert_eq!(saved.state, OutputReceiptState::Saved);
        assert_eq!(
            saved.memory_id.as_deref(),
            Some(storage.memory_id().as_str())
        );

        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            assert_eq!(
                tombstone_output_receipt_for_memory(&transaction, &storage.memory_id(), "t4",)?,
                Some(OutputDeletionCleanup::with_generation(storage, None, 1))
            );
            transaction.execute(
                "DELETE FROM memories WHERE id = ?1",
                params![storage.memory_id()],
            )?;
            transaction.commit()?;
            Ok(())
        })
        .unwrap();
        let deleted = db.get_output_receipt(&key).unwrap().unwrap();
        assert_eq!(deleted.state, OutputReceiptState::Deleted);
        assert_eq!(deleted.memory_id, None);
        assert_eq!(deleted.error_code, None);
        assert_eq!(deleted.deleted_at.as_deref(), Some("t4"));
    }
    #[test]
    fn output_persistence_cleanup_preserves_promoted_large_and_inline_outputs() {
        let directory = ArtifactTestDirectory::new();
        let db = Db::open_in_memory_with_artifact_root(&directory.path).unwrap();
        let large_body = "promoted report\n".repeat(3_000);
        let large_key = insert_receipt(
            &db,
            "run-promoted-large",
            "output-1",
            "step-promoted-large",
            &large_body,
            OutputDisposition::default(),
        );
        let store = db.output_artifact_store();
        let large_saved = match db
            .persist_output_receipt(&large_key, OutputClaimMode::Initial, &store, None)
            .unwrap()
        {
            OutputPersistenceOutcome::Saved(receipt) => receipt,
            other => panic!("expected promoted large output to save, got {other:?}"),
        };
        let large_memory_id = large_saved.memory_id.clone().unwrap();
        let large_artifact = db
            .get_memory(&large_memory_id)
            .unwrap()
            .unwrap()
            .artifact_path
            .unwrap();
        assert!(std::path::Path::new(&large_artifact).exists());
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                    SET scope_type = 'user', scope_key = 'local-user'
                  WHERE id = ?1",
                params![large_memory_id],
            )?;
            Ok(())
        })
        .unwrap();

        assert_eq!(db.clear_memories("workflow-1").unwrap(), 0);
        let promoted_large = db.get_memory(&large_memory_id).unwrap().unwrap();
        assert_eq!(promoted_large.scope_type, MemoryScopeType::User);
        assert_eq!(
            db.get_output_receipt(&large_key).unwrap().unwrap().state,
            OutputReceiptState::Saved
        );
        assert!(std::path::Path::new(&large_artifact).exists());
        assert_eq!(
            std::fs::read_to_string(&large_artifact).unwrap(),
            normalize_body(&large_body)
        );

        let inline_key = insert_receipt(
            &db,
            "run-promoted-inline",
            "output-1",
            "step-promoted-inline",
            "promoted inline output",
            OutputDisposition::default(),
        );
        let inline_saved = match db
            .persist_output_receipt(&inline_key, OutputClaimMode::Initial, &store, None)
            .unwrap()
        {
            OutputPersistenceOutcome::Saved(receipt) => receipt,
            other => panic!("expected promoted inline output to save, got {other:?}"),
        };
        let inline_memory_id = inline_saved.memory_id.clone().unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                    SET scope_type = 'user', scope_key = 'local-user'
                  WHERE id = ?1",
                params![inline_memory_id],
            )?;
            Ok(())
        })
        .unwrap();

        db.delete_workflow("workflow-1").unwrap();
        let promoted_large = db.get_memory(&large_memory_id).unwrap().unwrap();
        assert_eq!(promoted_large.scope_type, MemoryScopeType::User);
        assert_eq!(promoted_large.workflow_id, None);
        assert!(std::path::Path::new(&large_artifact).exists());
        let promoted_inline = db.get_memory(&inline_memory_id).unwrap().unwrap();
        assert_eq!(promoted_inline.scope_type, MemoryScopeType::User);
        assert_eq!(promoted_inline.workflow_id, None);
        assert_eq!(promoted_inline.body, "promoted inline output");
    }

    #[test]
    fn output_persistence_saved_memory_cannot_be_deleted_before_its_tombstone() {
        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-delete-guard",
            "output-1",
            "step-delete-guard",
            "guarded body",
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let storage = claim.storage_identity().unwrap();

        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            insert_output_memory(&transaction, &claim.receipt, &claim.output_body)?;
            mark_output_receipt_saved(&transaction, &claim, "t3")?;
            transaction.commit()?;

            assert!(conn
                .execute(
                    "DELETE FROM memories WHERE id = ?1",
                    params![storage.memory_id()],
                )
                .is_err());
            let memory_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE id = ?1",
                params![storage.memory_id()],
                |row| row.get(0),
            )?;
            assert_eq!(memory_count, 1);
            assert_eq!(
                load_output_receipt_conn(conn, &key)?.unwrap().state,
                OutputReceiptState::Saved
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn output_persistence_tombstone_wins_a_pending_save_race() {
        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-race",
            "output-1",
            "step-race",
            "body",
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let storage = claim.receipt.storage_identity.unwrap();
        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let owned = tombstone_output_receipts_for_workflow(&transaction, "workflow-1", "t3")?;
            assert_eq!(
                owned,
                vec![OutputDeletionCleanup::with_generation(
                    storage,
                    Some(claim.attempt_identity),
                    1,
                )]
            );
            transaction.commit()?;
            Ok(())
        })
        .unwrap();

        let late_save_committed = db
            .with_conn(|conn| {
                let transaction = conn.unchecked_transaction()?;
                insert_output_memory(&transaction, &claim.receipt, &claim.output_body)?;
                match mark_output_receipt_saved(&transaction, &claim, "t4") {
                    Ok(()) => {
                        transaction.commit()?;
                        Ok(true)
                    }
                    Err(_) => {
                        transaction.rollback()?;
                        Ok(false)
                    }
                }
            })
            .unwrap();
        assert!(!late_save_committed);
        db.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE id = ?1",
                params![storage.memory_id()],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            db.get_output_receipt(&key).unwrap().unwrap().state,
            OutputReceiptState::Deleted
        );
    }

    #[test]
    fn output_persistence_late_deleted_worker_scrubs_only_its_owned_files() {
        let db = Db::open_in_memory().unwrap();
        let body = "deleted report\n".repeat(3_000);
        let key = insert_receipt(
            &db,
            "run-delete-overlap",
            "output-1",
            "step-delete-overlap",
            &body,
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let storage = claim.storage_identity().unwrap();
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        let initial_cleanup = db
            .with_conn(|conn| {
                let transaction = conn.unchecked_transaction()?;
                let cleanup = tombstone_output_receipt_by_key(&transaction, &key, "t3")?;
                transaction.commit()?;
                Ok(cleanup.unwrap())
            })
            .unwrap();
        assert!(db.output_cleanup_is_active(initial_cleanup).unwrap());
        crate::db::memories::remove_output_deletion_files(&db, &store, [initial_cleanup]);

        let artifact = store
            .ensure_for_claim(&claim)
            .unwrap()
            .expect("late large output spills");
        assert!(artifact.exists());
        let late_save_committed = db
            .with_conn(|conn| {
                let transaction = conn.unchecked_transaction()?;
                insert_output_memory(&transaction, &claim.receipt, &claim.output_body)?;
                match mark_output_receipt_saved(&transaction, &claim, "t4") {
                    Ok(()) => {
                        transaction.commit()?;
                        Ok(true)
                    }
                    Err(_) => {
                        transaction.rollback()?;
                        Ok(false)
                    }
                }
            })
            .unwrap();
        assert!(!late_save_committed);
        assert!(db
            .deletion_cleanup_for_lost_claim(&claim)
            .unwrap()
            .is_none());
        db.clear_output_attempt_active(storage, claim.attempt_identity)
            .unwrap();

        let final_cleanup = db
            .deletion_cleanup_for_lost_claim(&claim)
            .unwrap()
            .expect("deleted receipt authorizes late-file cleanup");
        store.remove_deleted_files(final_cleanup);
        assert!(!artifact.exists());
        assert_eq!(
            db.get_output_receipt(&key).unwrap().unwrap().state,
            OutputReceiptState::Deleted
        );
    }

    #[test]
    fn output_persistence_late_worker_can_scrub_after_its_run_is_removed() {
        let db = Db::open_in_memory().unwrap();
        let body = "removed run report\n".repeat(3_000);
        let key = insert_receipt(
            &db,
            "run-removed-overlap",
            "output-1",
            "step-removed-overlap",
            &body,
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let storage = claim.storage_identity().unwrap();
        db.with_conn(|conn| {
            conn.execute("DELETE FROM runs WHERE id = ?1", params![key.run_id()])?;
            Ok(())
        })
        .unwrap();
        assert!(db.get_output_receipt(&key).unwrap().is_none());

        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        let artifact = store
            .ensure_for_claim(&claim)
            .unwrap()
            .expect("late removed-run output spills");
        assert!(db
            .deletion_cleanup_for_lost_claim(&claim)
            .unwrap()
            .is_none());
        db.clear_output_attempt_active(storage, claim.attempt_identity)
            .unwrap();
        let cleanup = db
            .deletion_cleanup_for_lost_claim(&claim)
            .unwrap()
            .expect("removed run authorizes late-file cleanup");
        store.remove_deleted_files(cleanup);
        assert!(!artifact.exists());
    }
    #[test]
    fn output_persistence_lost_claim_cannot_scrub_promoted_memory_after_workflow_delete() {
        let directory = ArtifactTestDirectory::new();
        let db = Db::open_in_memory_with_artifact_root(&directory.path).unwrap();
        let body = "promoted overlap report\n".repeat(3_000);
        let key = insert_receipt(
            &db,
            "run-promoted-overlap",
            "output-1",
            "step-promoted-overlap",
            &body,
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let store = db.output_artifact_store();
        let artifact = store
            .ensure_for_claim(&claim)
            .unwrap()
            .expect("promoted overlap output spills");
        let storage = claim.storage_identity().unwrap();
        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            insert_output_memory(&transaction, &claim.receipt, &claim.output_body)?;
            transaction.execute(
                "UPDATE memories
                    SET scope_type = 'user', scope_key = 'local-user'
                  WHERE id = ?1",
                params![storage.memory_id()],
            )?;
            mark_output_receipt_saved(&transaction, &claim, "t3")?;
            transaction.commit()?;
            Ok(())
        })
        .unwrap();

        db.delete_workflow("workflow-1").unwrap();
        assert!(db.get_memory(&storage.memory_id()).unwrap().is_some());
        let stage = store.root.join(claim.staging_identity().unwrap().relative_path());
        fs::write(&stage, "abandoned stage").unwrap();
        assert!(db
            .deletion_cleanup_for_lost_claim(&claim)
            .unwrap()
            .is_none());
        db.clear_output_attempt_active(storage, claim.attempt_identity)
            .unwrap();
        let cleanup = db
            .deletion_cleanup_for_lost_claim(&claim)
            .unwrap()
            .expect("promoted overlap retains final but cleans old stage");
        store.remove_deleted_files(cleanup);
        assert!(!stage.exists());
        assert!(artifact.exists());
    }

    #[test]
    fn output_persistence_recovery_is_bounded_and_leaves_failed_rows_for_explicit_retry() {
        let db = Db::open_in_memory().unwrap();
        let mut keys = Vec::new();
        for index in 0..3 {
            let key = insert_receipt(
                &db,
                &format!("run-recover-{index}"),
                "output-1",
                &format!("step-recover-{index}"),
                "body",
                OutputDisposition::default(),
            );
            assert!(matches!(
                db.claim_output_save_at(&key, OutputClaimMode::Initial, "2026-01-01T00:00:00Z")
                    .unwrap(),
                OutputClaimOutcome::Claimed(_)
            ));
            keys.push(key);
        }
        let failed_key = insert_receipt(
            &db,
            "run-recover-failed",
            "output-1",
            "step-recover-failed",
            "body",
            OutputDisposition::default(),
        );
        let failed_claim = match db
            .claim_output_save_at(
                &failed_key,
                OutputClaimMode::Initial,
                "2026-01-01T00:00:00Z",
            )
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        db.record_output_save_failure_at(
            &failed_claim,
            OutputPersistenceErrorCode::DatabaseWrite,
            "2026-01-01T00:00:01Z",
        )
        .unwrap();

        db.capture_output_recovery_snapshot().unwrap();
        let (recovered, has_more) = db.recover_captured_output_saves(2).unwrap();
        assert_eq!(recovered.len(), 2);
        assert!(has_more);
        assert!(recovered.iter().all(|item| {
            item.recovered
                .as_ref()
                .is_some_and(|recovery| recovery.receipt.state == OutputReceiptState::Pending)
        }));
        let states = keys
            .iter()
            .map(|key| db.get_output_receipt(key).unwrap().unwrap().state)
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            vec![
                OutputReceiptState::Pending,
                OutputReceiptState::Pending,
                OutputReceiptState::Saving,
            ]
        );
        assert_eq!(
            db.get_output_receipt(&failed_key).unwrap().unwrap().state,
            OutputReceiptState::Failed
        );
    }

    #[test]
    fn output_persistence_startup_snapshot_drains_backlog_without_stealing_new_claims() {
        let db = Db::open_in_memory().unwrap();
        let mut pending = Vec::new();
        for index in 0..65 {
            pending.push(insert_receipt(
                &db,
                &format!("run-snapshot-pending-{index:02}"),
                "output-1",
                &format!("step-snapshot-pending-{index:02}"),
                "snapshot body",
                OutputDisposition::default(),
            ));
        }
        let interrupted_key = insert_receipt(
            &db,
            "run-snapshot-interrupted",
            "output-1",
            "step-snapshot-interrupted",
            "snapshot body",
            OutputDisposition::default(),
        );
        assert!(matches!(
            db.claim_output_save_at(
                &interrupted_key,
                OutputClaimMode::Initial,
                "2026-01-01T00:00:00Z",
            )
            .unwrap(),
            OutputClaimOutcome::Claimed(_)
        ));

        let failed_key = insert_receipt(
            &db,
            "run-snapshot-failed",
            "output-1",
            "step-snapshot-failed",
            "snapshot body",
            OutputDisposition::default(),
        );
        let failed_claim = match db
            .claim_output_save_at(
                &failed_key,
                OutputClaimMode::Initial,
                "2026-01-01T00:00:00Z",
            )
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected failed fixture claim, got {other:?}"),
        };
        db.record_output_save_failure_at(
            &failed_claim,
            OutputPersistenceErrorCode::DatabaseWrite,
            "2026-01-01T00:00:01Z",
        )
        .unwrap();

        db.capture_output_recovery_snapshot().unwrap();
        let fresh_key = insert_receipt(
            &db,
            "run-snapshot-fresh",
            "output-1",
            "step-snapshot-fresh",
            "snapshot body",
            OutputDisposition::default(),
        );
        assert!(matches!(
            db.claim_output_save_at(
                &fresh_key,
                OutputClaimMode::Initial,
                "2026-01-01T00:00:02Z",
            )
            .unwrap(),
            OutputClaimOutcome::Claimed(_)
        ));

        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        let mut processed = 0;
        let mut batches = 0;
        loop {
            let (outcomes, has_more) = db
                .recover_and_persist_output_snapshot_batch(32, &store, None)
                .unwrap();
            processed += outcomes.len();
            batches += 1;
            if !has_more {
                break;
            }
        }
        assert_eq!(processed, 66);
        assert!(batches >= 3);
        for key in pending.into_iter().chain([interrupted_key]) {
            assert_eq!(
                db.get_output_receipt(&key).unwrap().unwrap().state,
                OutputReceiptState::Saved
            );
        }
        assert_eq!(
            db.get_output_receipt(&failed_key).unwrap().unwrap().state,
            OutputReceiptState::Failed
        );
        assert_eq!(
            db.get_output_receipt(&fresh_key).unwrap().unwrap().state,
            OutputReceiptState::Saving
        );
    }

    #[test]
    fn output_persistence_recovery_fences_delayed_worker_from_replacement_claim() {
        let db = Db::open_in_memory().unwrap();
        let body = "fenced report\n".repeat(3_000);
        let key = insert_receipt(
            &db,
            "run-fenced",
            "output-1",
            "step-fenced",
            &body,
            OutputDisposition::default(),
        );
        let old_claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "2026-01-01T00:00:00Z")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected old claim, got {other:?}"),
        };
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        let old_stage = directory
            .path
            .join(old_claim.staging_identity().unwrap().relative_path());
        fs::create_dir_all(old_stage.parent().unwrap()).unwrap();
        fs::write(&old_stage, "crashed attempt staging").unwrap();
        db.capture_output_recovery_snapshot().unwrap();
        let (recovered, has_more) = db.recover_captured_output_saves(1).unwrap();
        assert_eq!(recovered.len(), 1);
        assert!(!has_more);
        let recovery = recovered[0].recovered.as_ref().unwrap();
        assert_eq!(recovery.receipt.state, OutputReceiptState::Pending);
        assert_eq!(recovery.receipt.claim_identity, None);
        let journal_count: i64 = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM output_cleanup_journal",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(journal_count, 1);
        store.remove_recovered_staging(recovery).unwrap();
        assert!(!old_stage.exists());
        let cleanup = recovery.cleanup().unwrap();
        assert!(db.output_cleanup_is_active(cleanup).unwrap());
        db.acknowledge_output_cleanup(cleanup).unwrap();
        let journal_count: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM output_cleanup_journal",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(journal_count, 1);
        db.clear_output_attempt_active(cleanup.storage, cleanup.abandoned_attempt.unwrap())
            .unwrap();
        db.acknowledge_output_cleanup(cleanup).unwrap();
        let journal_count: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM output_cleanup_journal",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(journal_count, 0);

        let replacement_claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "2026-01-01T00:00:02Z")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected replacement claim, got {other:?}"),
        };
        assert_ne!(
            old_claim.attempt_identity,
            replacement_claim.attempt_identity
        );
        assert!(!db
            .record_output_save_failure_at(
                &old_claim,
                OutputPersistenceErrorCode::DatabaseWrite,
                "2026-01-01T00:00:03Z",
            )
            .unwrap());

        let stale_save_committed = db
            .with_conn(|conn| {
                let transaction = conn.unchecked_transaction()?;
                insert_output_memory(&transaction, &old_claim.receipt, &old_claim.output_body)?;
                match mark_output_receipt_saved(&transaction, &old_claim, "2026-01-01T00:00:04Z") {
                    Ok(()) => {
                        transaction.commit()?;
                        Ok(true)
                    }
                    Err(_) => {
                        transaction.rollback()?;
                        Ok(false)
                    }
                }
            })
            .unwrap();
        assert!(!stale_save_committed);

        let artifact = store
            .ensure_for_claim(&replacement_claim)
            .unwrap()
            .expect("large replacement output spills");
        let replacement_stage = directory.path.join(
            replacement_claim
                .staging_identity()
                .unwrap()
                .relative_path(),
        );
        fs::write(&old_stage, "stale temporary data").unwrap();
        fs::write(&replacement_stage, "replacement temporary data").unwrap();
        store.remove_claim_staging(&old_claim);
        assert!(!old_stage.exists());
        assert!(replacement_stage.exists());
        assert_eq!(
            fs::read_to_string(&artifact).unwrap(),
            replacement_claim.output_body
        );

        db.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            insert_output_memory(
                &transaction,
                &replacement_claim.receipt,
                &replacement_claim.output_body,
            )?;
            mark_output_receipt_saved(&transaction, &replacement_claim, "2026-01-01T00:00:05Z")?;
            transaction.commit()?;
            Ok(())
        })
        .unwrap();
        assert!(!db
            .record_output_save_failure_at(
                &old_claim,
                OutputPersistenceErrorCode::DatabaseWrite,
                "2026-01-01T00:00:06Z",
            )
            .unwrap());
        assert_eq!(
            db.get_output_receipt(&key).unwrap().unwrap().state,
            OutputReceiptState::Saved
        );
        assert_eq!(
            fs::read_to_string(&artifact).unwrap(),
            replacement_claim.output_body
        );
        fs::write(&old_stage, "late stale stage").unwrap();
        let stale_cleanup = db
            .deletion_cleanup_for_lost_claim(&old_claim)
            .unwrap()
            .expect("released stale claim may scrub only its old stage");
        store.remove_deleted_files(stale_cleanup);
        assert!(!old_stage.exists());
        assert!(artifact.exists());
        assert_eq!(
            db.get_output_receipt(&key).unwrap().unwrap().state,
            OutputReceiptState::Saved
        );
        assert_eq!(
            fs::read_to_string(&artifact).unwrap(),
            replacement_claim.output_body
        );
        let saved_memory_id = db
            .get_output_receipt(&key)
            .unwrap()
            .unwrap()
            .memory_id
            .unwrap();
        let saved_memory = db.get_memory(&saved_memory_id).unwrap().unwrap();
        assert_eq!(saved_memory.body, replacement_claim.output_body);
        store.remove_claim_staging(&replacement_claim);
    }

    #[test]
    fn output_persistence_artifact_store_recovers_owned_files_without_touching_others() {
        let db = Db::open_in_memory().unwrap();
        let body = "large report\n".repeat(3_000);
        assert!(body.len() >= OUTPUT_ARTIFACT_BODY_THRESHOLD);
        let key = insert_receipt(
            &db,
            "run-artifact",
            "output-1",
            "step-artifact",
            &body,
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert!(claim.receipt.artifact_required);
        let identity = claim.receipt.storage_identity.unwrap();
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);

        let artifact = store
            .ensure_for_claim(&claim)
            .unwrap()
            .expect("large output spills");
        assert_eq!(fs::read_to_string(&artifact).unwrap(), claim.output_body);
        assert!(!directory
            .path
            .join(claim.staging_identity().unwrap().relative_path())
            .exists());
        assert_eq!(
            store.ensure_for_claim(&claim).unwrap().as_deref(),
            Some(artifact.as_path())
        );

        let unrelated = directory.path.join("unrelated-artifact.txt");
        fs::write(&unrelated, "keep").unwrap();
        let cleanup = db
            .with_conn(|conn| {
                let transaction = conn.unchecked_transaction()?;
                let cleanup = tombstone_output_receipt_by_key(&transaction, &key, "t3")?;
                transaction.commit()?;
                Ok(cleanup.expect("saving receipt has owned storage"))
            })
            .unwrap();
        store.remove_deleted_files(cleanup);
        assert!(!artifact.exists());
        assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
    }

    #[test]
    fn output_persistence_artifact_store_recovers_partial_stage_and_rejects_conflicting_final() {
        let db = Db::open_in_memory().unwrap();
        let body = "x".repeat(OUTPUT_ARTIFACT_BODY_THRESHOLD);
        let key = insert_receipt(
            &db,
            "run-artifact-conflict",
            "output-1",
            "step-artifact-conflict",
            &body,
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let identity = claim.receipt.storage_identity.unwrap();
        let directory = ArtifactTestDirectory::new();
        let store = OutputArtifactStore::new(&directory.path);
        let stage = directory
            .path
            .join(claim.staging_identity().unwrap().relative_path());
        fs::create_dir_all(stage.parent().unwrap()).unwrap();
        fs::write(&stage, "partial").unwrap();

        let artifact = store.ensure_for_claim(&claim).unwrap().unwrap();
        assert_eq!(fs::read_to_string(&artifact).unwrap(), body);
        assert!(!stage.exists());

        fs::write(&artifact, "conflicting final").unwrap();
        let error = store.ensure_for_claim(&claim).unwrap_err();
        assert!(matches!(error, OutputArtifactError::ContentMismatch));
        assert_eq!(
            error.code(),
            OutputPersistenceErrorCode::ArtifactContentMismatch
        );
        assert_eq!(fs::read_to_string(artifact).unwrap(), "conflicting final");

        let invalid_root = directory.path.join("root-is-a-file");
        fs::write(&invalid_root, "not a directory").unwrap();
        let error = OutputArtifactStore::new(invalid_root)
            .ensure_for_claim(&claim)
            .unwrap_err();
        assert!(matches!(error, OutputArtifactError::StageIo(_)));
        assert_eq!(error.code(), OutputPersistenceErrorCode::ArtifactStageIo);
    }

    #[test]
    fn output_persistence_notification_is_optional_and_failure_is_best_effort() {
        use std::cell::Cell;

        struct FailingSink(Cell<usize>);
        impl OutputPersistenceEventSink for FailingSink {
            fn emit(&self, _event: &OutputPersistenceEvent) -> Result<(), String> {
                self.0.set(self.0.get() + 1);
                Err("listener unavailable".into())
            }
        }

        let db = Db::open_in_memory().unwrap();
        let key = insert_receipt(
            &db,
            "run-event",
            "output-1",
            "step-event",
            "body",
            OutputDisposition::default(),
        );
        let receipt = db.get_output_receipt(&key).unwrap().unwrap();
        let event = OutputPersistenceEvent {
            workflow_id: "workflow-1".into(),
            receipt,
        };
        notify_output_persistence(None, &event);
        let sink = FailingSink(Cell::new(0));
        notify_output_persistence(Some(&sink), &event);
        assert_eq!(sink.0.get(), 1);
        assert_eq!(
            db.get_output_receipt(&key).unwrap().unwrap().state,
            OutputReceiptState::Pending
        );
    }

    fn fixed_storage_identity() -> OutputStorageIdentity {
        OutputStorageIdentity::parse("550e8400-e29b-41d4-a716-446655440000")
            .expect("valid fixture UUID")
    }

    fn fixed_attempt_identity() -> OutputSaveAttemptIdentity {
        OutputSaveAttemptIdentity::parse("4c22939e-f0d7-4ae3-9b91-c4a0e7735843")
            .expect("valid fixture UUID")
    }

    #[test]
    fn output_persistence_captures_frontend_defaults_and_legacy_data() {
        let defaults = OutputDisposition::capture(&Value::Null);
        assert_eq!(defaults, OutputDisposition::default());
        assert_eq!(defaults.resolved_memory_title(), "Output");

        let invalid_types = OutputDisposition::capture(&json!({
            "label": "   ",
            "saveToMemory": "false",
            "pinMemory": 1,
            "memoryTitle": false,
            "asFinalResult": null,
            "htmlReport": [],
            "includeFilesChanged": {}
        }));
        assert_eq!(invalid_types, OutputDisposition::default());
    }

    #[test]
    fn output_persistence_captures_all_execution_time_settings() {
        let mut live_node_data = json!({
            "label": "  Release report  ",
            "saveToMemory": false,
            "pinMemory": true,
            "memoryTitle": "  Shipped title  ",
            "asFinalResult": false,
            "htmlReport": true,
            "includeFilesChanged": false
        });
        let captured = OutputDisposition::capture(&live_node_data);

        live_node_data["pinMemory"] = json!(false);
        live_node_data["memoryTitle"] = json!("Edited after run start");

        assert_eq!(captured.label, "  Release report  ");
        assert!(!captured.save_to_memory);
        assert!(captured.pin_memory);
        assert_eq!(captured.resolved_memory_title(), "Shipped title");
        assert!(!captured.as_final_result);
        assert!(captured.html_report);
        assert!(!captured.include_files_changed);
    }

    #[test]
    fn output_persistence_initial_state_distinguishes_disabled_empty_and_requested() {
        let mut disposition = OutputDisposition::default();
        disposition.save_to_memory = false;
        assert_eq!(
            disposition.initial_receipt_state("durable body"),
            OutputReceiptState::NotRequested
        );

        disposition.save_to_memory = true;
        assert_eq!(
            disposition.initial_receipt_state(" \n\t "),
            OutputReceiptState::SkippedEmpty
        );
        assert_eq!(
            disposition.initial_receipt_state("durable body"),
            OutputReceiptState::Pending
        );
    }

    #[test]
    fn output_persistence_identity_is_run_and_node_and_rejects_empty_parts() {
        let key = OutputReceiptKey::new("run-1", "output-1").expect("valid identity");
        assert_eq!(key.run_id(), "run-1");
        assert_eq!(key.node_id(), "output-1");
        assert_eq!(
            OutputReceiptKey::new(" ", "output-1"),
            Err(OutputIdentityError::EmptyRunId)
        );
        assert_eq!(
            OutputReceiptKey::new("run-1", "\n"),
            Err(OutputIdentityError::EmptyNodeId)
        );
    }

    #[test]
    fn output_persistence_seed_allocates_storage_only_for_requested_nonempty_output() {
        let requested = OutputReceiptSeed::new(
            OutputReceiptKey::new("run-1", "output-1").unwrap(),
            "step-1",
            OutputDisposition::default(),
            "body",
        )
        .unwrap();
        assert_eq!(requested.state(), OutputReceiptState::Pending);
        assert!(requested.storage_identity().is_some());
        assert_eq!(requested.resolved_memory_title(), "Output");

        let empty = OutputReceiptSeed::new(
            OutputReceiptKey::new("run-2", "output-1").unwrap(),
            "step-2",
            OutputDisposition::default(),
            "  ",
        )
        .unwrap();
        assert_eq!(empty.state(), OutputReceiptState::SkippedEmpty);
        assert!(empty.storage_identity().is_none());
        assert_eq!(
            OutputReceiptSeed::new(
                OutputReceiptKey::new("run-3", "output-1").unwrap(),
                " ",
                OutputDisposition::default(),
                "body"
            ),
            Err(OutputIdentityError::EmptyStepId)
        );
    }

    #[test]
    fn output_persistence_save_retry_recovery_and_delete_transitions_are_explicit() {
        assert_eq!(
            OutputReceiptState::Pending.transition(OutputReceiptAction::StartSave),
            Ok(OutputReceiptState::Saving)
        );
        assert_eq!(
            OutputReceiptState::Saving.transition(OutputReceiptAction::RecordFailure),
            Ok(OutputReceiptState::Failed)
        );
        assert_eq!(
            OutputReceiptState::Failed.transition(OutputReceiptAction::StartSave),
            Ok(OutputReceiptState::Saving)
        );
        assert_eq!(
            OutputReceiptState::Saving.transition(OutputReceiptAction::RecoverInterruptedSave),
            Ok(OutputReceiptState::Pending)
        );
        assert_eq!(
            OutputReceiptState::Saving.transition(OutputReceiptAction::CommitSaved),
            Ok(OutputReceiptState::Saved)
        );
        for state in [
            OutputReceiptState::Pending,
            OutputReceiptState::Saving,
            OutputReceiptState::Failed,
            OutputReceiptState::Saved,
            OutputReceiptState::Deleted,
        ] {
            assert_eq!(
                state.transition(OutputReceiptAction::Tombstone),
                Ok(OutputReceiptState::Deleted),
                "tombstone from {state}"
            );
        }
    }

    #[test]
    fn output_persistence_terminal_states_cannot_restart_or_overwrite_deletion() {
        for state in [
            OutputReceiptState::NotRequested,
            OutputReceiptState::SkippedEmpty,
            OutputReceiptState::Saved,
            OutputReceiptState::Deleted,
        ] {
            assert!(state.transition(OutputReceiptAction::StartSave).is_err());
        }
        assert!(OutputReceiptState::Deleted
            .transition(OutputReceiptAction::CommitSaved)
            .is_err());
        assert!(OutputReceiptState::Deleted
            .transition(OutputReceiptAction::RecordFailure)
            .is_err());
    }

    #[test]
    fn output_persistence_composite_key_deduplicates_replay_but_separates_runs() {
        use std::collections::HashSet;

        let first = OutputReceiptKey::new("run-1", "output-1").unwrap();
        let replay = OutputReceiptKey::new("run-1", "output-1").unwrap();
        let other_run = OutputReceiptKey::new("run-2", "output-1").unwrap();
        let mut keys = HashSet::new();
        assert!(keys.insert(first));
        assert!(!keys.insert(replay));
        assert!(keys.insert(other_run));

        let first_storage = OutputReceiptSeed::new(
            OutputReceiptKey::new("run-1", "output-1").unwrap(),
            "step-1",
            OutputDisposition::default(),
            "identical body",
        )
        .unwrap()
        .storage_identity()
        .unwrap();
        let other_storage = OutputReceiptSeed::new(
            OutputReceiptKey::new("run-2", "output-1").unwrap(),
            "step-2",
            OutputDisposition::default(),
            "identical body",
        )
        .unwrap()
        .storage_identity()
        .unwrap();
        assert_ne!(first_storage, other_storage);
    }

    #[test]
    fn output_persistence_owned_paths_round_trip_and_reject_unowned_files() {
        let identity = fixed_storage_identity();
        let staging_identity = OutputStagingIdentity::new(identity, fixed_attempt_identity());
        let stage = staging_identity.relative_path();
        let artifact = identity.artifact_relative_path();

        assert_eq!(
            OutputStagingIdentity::from_relative_path(&stage),
            Some(staging_identity)
        );
        assert_eq!(
            OutputStorageIdentity::from_owned_artifact_relative_path(&artifact),
            Some(identity)
        );
        for unowned in [
            Path::new("550e8400-e29b-41d4-a716-446655440000.txt"),
            Path::new("artifacts/550e8400-e29b-41d4-a716-446655440000.txt"),
            Path::new(".output-staging/../unrelated.txt"),
            Path::new(".output-staging/output-not-a-uuid.stage"),
            Path::new(".output-staging/nested/output-550e8400-e29b-41d4-a716-446655440000.stage"),
        ] {
            assert_eq!(
                OutputStagingIdentity::from_relative_path(unowned),
                None,
                "accepted unowned path {}",
                unowned.display()
            );
        }
    }

    #[test]
    fn output_persistence_state_and_error_codes_have_stable_serialized_names() {
        for (state, expected) in [
            (OutputReceiptState::NotRequested, "not_requested"),
            (OutputReceiptState::SkippedEmpty, "skipped_empty"),
            (OutputReceiptState::Pending, "pending"),
            (OutputReceiptState::Saving, "saving"),
            (OutputReceiptState::Saved, "saved"),
            (OutputReceiptState::Failed, "failed"),
            (OutputReceiptState::Deleted, "deleted"),
        ] {
            assert_eq!(state.as_str(), expected);
            assert_eq!(OutputReceiptState::parse(expected), Ok(state));
            assert_eq!(serde_json::to_value(state).unwrap(), json!(expected));
        }
        assert!(OutputReceiptState::parse("completed").is_err());
        for (code, expected, retryable) in [
            (
                OutputPersistenceErrorCode::InvalidDisposition,
                "invalid_disposition",
                false,
            ),
            (
                OutputPersistenceErrorCode::StepOutputUnavailable,
                "step_output_unavailable",
                false,
            ),
            (
                OutputPersistenceErrorCode::ArtifactStageIo,
                "artifact_stage_io",
                true,
            ),
            (
                OutputPersistenceErrorCode::ArtifactFinalizeIo,
                "artifact_finalize_io",
                true,
            ),
            (
                OutputPersistenceErrorCode::ArtifactContentMismatch,
                "artifact_content_mismatch",
                false,
            ),
            (
                OutputPersistenceErrorCode::DatabaseWrite,
                "database_write",
                true,
            ),
            (OutputPersistenceErrorCode::Internal, "internal", false),
        ] {
            assert_eq!(code.as_str(), expected);
            assert_eq!(OutputPersistenceErrorCode::parse(expected), Ok(code));
            assert_eq!(code.is_retryable(), retryable);
            assert_eq!(serde_json::to_value(code).unwrap(), json!(expected));
        }
        assert!(OutputPersistenceErrorCode::parse("sqlite_busy").is_err());
        assert!(!OutputPersistenceErrorCode::InvalidDisposition.is_retryable());
    }
    #[test]
    fn output_cleanup_ack_is_generation_fenced() {
        let db = Db::open_in_memory().unwrap();
        let storage = fixed_storage_identity();
        let attempt = Some(fixed_attempt_identity());
        let (first, second) = db
            .with_conn(|conn| {
                create_output_persistence_schema(conn)?;
                let first = enqueue_output_cleanup_journal_conn(
                    conn,
                    storage,
                    attempt,
                    true,
                    "t1",
                )?;
                let second = enqueue_output_cleanup_journal_conn(
                    conn,
                    storage,
                    attempt,
                    false,
                    "t2",
                )?;
                Ok((first, second))
            })
            .unwrap();
        assert_eq!(second, first + 1);
        let stale_cleanup = OutputDeletionCleanup::with_generation(storage, attempt, first);
        db.acknowledge_output_cleanup(stale_cleanup).unwrap();
        let current: (i64, bool) = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT generation, remove_final
                       FROM output_cleanup_journal
                      WHERE storage_id = ?1 AND attempt_id = ?2",
                    params![storage.memory_id(), attempt.unwrap().to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(current, (second, true));
    }
    #[test]
    fn output_cleanup_journal_reclaims_failed_attempt_stage() {
        let db = Db::open_in_memory().unwrap();
        let body = "stage-journal ".repeat(3_000);
        let key = insert_receipt(
            &db,
            "run-stage-journal",
            "output-1",
            "step-stage-journal",
            &body,
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let store = db.output_artifact_store();
        let stage = claim.staging_identity().unwrap().relative_path();
        let stage_path = store.root.join(&stage);
        fs::create_dir_all(stage_path.parent().unwrap()).unwrap();
        fs::write(&stage_path, &body).unwrap();
        assert!(db
            .record_output_save_failure(&claim, OutputPersistenceErrorCode::DatabaseWrite)
            .unwrap());
        assert_eq!(
            db.get_output_receipt(&key).unwrap().unwrap().state,
            OutputReceiptState::Failed
        );
        assert_eq!(db.process_output_cleanup_journal(&store, 32).unwrap(), 1);
        assert!(!stage_path.exists());
        let remaining: i64 = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM output_cleanup_journal",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(remaining, 0);
    }
    #[test]
    fn output_cleanup_journal_defers_active_writer_until_restart() {
        let directory = ArtifactTestDirectory::new();
        let db = Db::open_in_memory_with_artifact_root(&directory.path).unwrap();
        let key = insert_receipt(
            &db,
            "run-active-cleanup",
            "output-1",
            "step-active-cleanup",
            &"active output ".repeat(3_000),
            OutputDisposition::default(),
        );
        let claim = match db
            .claim_output_save_at(&key, OutputClaimMode::Initial, "t2")
            .unwrap()
        {
            OutputClaimOutcome::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let storage = claim.storage_identity().unwrap();
        let store = db.output_artifact_store();
        let stage_path = store.root.join(claim.staging_identity().unwrap().relative_path());
        fs::create_dir_all(stage_path.parent().unwrap()).unwrap();
        fs::write(&stage_path, "writer-owned stage").unwrap();

        let cleanup = db
            .with_conn(|conn| {
                let transaction = conn.unchecked_transaction()?;
                let cleanup = tombstone_output_receipt_by_key(&transaction, &key, "t3")?
                    .expect("tombstone creates cleanup proof");
                transaction.commit()?;
                Ok(cleanup)
            })
            .unwrap();
        assert!(db.output_cleanup_is_active(cleanup).unwrap());
        crate::db::memories::remove_output_deletion_files(&db, &store, [cleanup]);
        assert!(stage_path.exists(), "active writer stage must remain durable");
        fs::write(&stage_path, "late writer output").unwrap();
        let journal_count: i64 = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM output_cleanup_journal",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(journal_count, 1);

        // A process restart drops the temporary active-attempt marker; the
        // durable journal then owns cleanup of the stale writer's files.
        db.with_conn(|conn| {
            conn.execute_batch("DROP TABLE IF EXISTS temp.output_active_attempts")?;
            Ok(())
        })
        .unwrap();
        assert!(!db.output_cleanup_is_active(cleanup).unwrap());
        assert_eq!(db.process_output_cleanup_journal(&store, 32).unwrap(), 1);
        assert!(!stage_path.exists());
        let final_path = store.root.join(storage.artifact_relative_path());
        assert!(!final_path.exists());
        assert_eq!(
            db.with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM output_cleanup_journal",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
            })
            .unwrap(),
            0
        );
    }

    #[test]
    fn output_cleanup_journal_drains_more_than_one_bounded_batch() {
        let directory = ArtifactTestDirectory::new();
        let db = Db::open_in_memory_with_artifact_root(&directory.path).unwrap();
        let store = db.output_artifact_store();
        let mut stages = Vec::new();
        for index in 0..65 {
            let storage = OutputStorageIdentity::new();
            let attempt = OutputSaveAttemptIdentity::new();
            let relative = OutputStagingIdentity::new(storage, attempt).relative_path();
            let path = store.root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("stale stage {index}")).unwrap();
            db.with_conn(|conn| {
                enqueue_output_cleanup_journal_conn(
                    conn,
                    storage,
                    Some(attempt),
                    false,
                    &format!("t{index}"),
                )?;
                Ok(())
            })
            .unwrap();
            stages.push(path);
        }

        let mut processed = 0;
        loop {
            let batch = db.process_output_cleanup_journal(&store, 32).unwrap();
            processed += batch;
            if batch < 32 {
                break;
            }
        }
        assert_eq!(processed, 65);
        assert!(stages.into_iter().all(|path| !path.exists()));
        let journal_count: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM output_cleanup_journal",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(journal_count, 0);
    }
}
