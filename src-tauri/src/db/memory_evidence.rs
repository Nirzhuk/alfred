use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::memory_curation::contains_secret_like_material;
use super::{Db, DbError};

pub const EVIDENCE_SCHEMA_VERSION: u32 = 1;
pub const EVIDENCE_NORMALIZATION_VERSION: u32 = 1;
pub const EVIDENCE_SOURCE_MAX_BYTES: usize = 16 * 1024;
pub const EVIDENCE_RUN_MAX_BYTES: usize = 128 * 1024;
pub const EVIDENCE_CITATION_MAX_BYTES: usize = 1024;
pub const EVIDENCE_CITATION_MAX_COUNT: usize = 3;
pub const EVIDENCE_CITATION_AGGREGATE_MAX_BYTES: usize =
    EVIDENCE_CITATION_MAX_BYTES * EVIDENCE_CITATION_MAX_COUNT;
pub const EVIDENCE_REVIEW_CATALOG_MAX_BYTES: usize = 32 * 1024;
const EVIDENCE_REVIEW_FRAGMENT_BUDGET_BYTES: usize = 28 * 1024;

pub const SOURCE_TRUNCATED_MARKER: &str = "[Source truncated by Alfred.]";
pub const SOURCE_OMITTED_MARKER: &str = "[Source omitted by Alfred.]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOrigin {
    AuthoredTask,
    ConfiguredWorkflowInstruction,
    ExternalTriggerData,
    HostRecordedObservation,
    ModelOutput,
    PriorMemory,
    RetrievedHistoricalSummary,
}

impl EvidenceOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthoredTask => "authored_task",
            Self::ConfiguredWorkflowInstruction => "configured_workflow_instruction",
            Self::ExternalTriggerData => "external_trigger_data",
            Self::HostRecordedObservation => "host_recorded_observation",
            Self::ModelOutput => "model_output",
            Self::PriorMemory => "prior_memory",
            Self::RetrievedHistoricalSummary => "retrieved_historical_summary",
        }
    }

    pub fn eligibility(self) -> EvidenceEligibility {
        match self {
            Self::AuthoredTask => EvidenceEligibility::AuthoredUserStatement,
            Self::ConfiguredWorkflowInstruction => EvidenceEligibility::WorkflowConfigurationOnly,
            Self::ExternalTriggerData => EvidenceEligibility::RecordedExternalData,
            Self::HostRecordedObservation => EvidenceEligibility::RecordedHostObservation,
            Self::ModelOutput | Self::PriorMemory | Self::RetrievedHistoricalSummary => {
                EvidenceEligibility::ComparisonOnly
            }
        }
    }

    fn from_db(value: &str) -> rusqlite::Result<Self> {
        match value {
            "authored_task" => Ok(Self::AuthoredTask),
            "configured_workflow_instruction" => Ok(Self::ConfiguredWorkflowInstruction),
            "external_trigger_data" => Ok(Self::ExternalTriggerData),
            "host_recorded_observation" => Ok(Self::HostRecordedObservation),
            "model_output" => Ok(Self::ModelOutput),
            "prior_memory" => Ok(Self::PriorMemory),
            "retrieved_historical_summary" => Ok(Self::RetrievedHistoricalSummary),
            _ => Err(invalid_evidence_column("origin")),
        }
    }

    fn supports(self, subject: EvidenceSubject) -> bool {
        match subject {
            EvidenceSubject::UserStatement => matches!(self, Self::AuthoredTask),
            EvidenceSubject::WorkflowConfiguration => matches!(
                self,
                Self::AuthoredTask | Self::ConfiguredWorkflowInstruction
            ),
            EvidenceSubject::RecordedObservation => matches!(
                self,
                Self::ExternalTriggerData | Self::HostRecordedObservation
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceEligibility {
    AuthoredUserStatement,
    WorkflowConfigurationOnly,
    RecordedExternalData,
    RecordedHostObservation,
    ComparisonOnly,
}

impl EvidenceEligibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthoredUserStatement => "authored_user_statement",
            Self::WorkflowConfigurationOnly => "workflow_configuration_only",
            Self::RecordedExternalData => "recorded_external_data",
            Self::RecordedHostObservation => "recorded_host_observation",
            Self::ComparisonOnly => "comparison_only",
        }
    }

    pub fn is_independently_eligible(self) -> bool {
        !matches!(self, Self::ComparisonOnly | Self::WorkflowConfigurationOnly)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSubject {
    UserStatement,
    WorkflowConfiguration,
    RecordedObservation,
}

impl EvidenceSubject {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserStatement => "user_statement",
            Self::WorkflowConfiguration => "workflow_configuration",
            Self::RecordedObservation => "recorded_observation",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "user_statement" => Some(Self::UserStatement),
            "workflow_configuration" => Some(Self::WorkflowConfiguration),
            "recorded_observation" => Some(Self::RecordedObservation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOmissionReason {
    RunByteLimit,
    SensitiveContent,
    ObservationUnavailable,
}

impl EvidenceOmissionReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::RunByteLimit => "run_byte_limit",
            Self::SensitiveContent => "sensitive_content",
            Self::ObservationUnavailable => "observation_unavailable",
        }
    }

    fn from_db(value: &str) -> rusqlite::Result<Self> {
        match value {
            "run_byte_limit" => Ok(Self::RunByteLimit),
            "sensitive_content" => Ok(Self::SensitiveContent),
            "observation_unavailable" => Ok(Self::ObservationUnavailable),
            _ => Err(invalid_evidence_column("omission_reason")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SourceCaptureState {
    Complete {
        original_bytes: usize,
    },
    Truncated {
        original_bytes: usize,
        captured_bytes: usize,
    },
    Omitted {
        original_bytes: usize,
        reason: EvidenceOmissionReason,
    },
}

impl SourceCaptureState {
    pub fn is_truncated(&self) -> bool {
        matches!(self, Self::Truncated { .. })
    }

    pub fn is_omitted(&self) -> bool {
        matches!(self, Self::Omitted { .. })
    }
}

#[derive(Debug, Clone)]
pub struct EvidenceSourceInput {
    pub run_id: String,
    pub step_id: Option<String>,
    pub node_id: Option<String>,
    pub origin: EvidenceOrigin,
    pub producer_identity: Option<String>,
    pub source_label: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub text: String,
    pub omission_reason: Option<EvidenceOmissionReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSource {
    pub source_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub node_id: Option<String>,
    pub origin: EvidenceOrigin,
    pub producer_identity: Option<String>,
    pub source_label: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub normalization_version: u32,
    pub normalized_text: String,
    pub capture_state: SourceCaptureState,
    pub digest: String,
}

impl EvidenceSource {
    pub fn shown_marker(&self) -> Option<&'static str> {
        match self.capture_state {
            SourceCaptureState::Complete { .. } => None,
            SourceCaptureState::Truncated { .. } => Some(SOURCE_TRUNCATED_MARKER),
            SourceCaptureState::Omitted { .. } => Some(SOURCE_OMITTED_MARKER),
        }
    }

    pub fn is_citeable(&self) -> bool {
        !self.capture_state.is_omitted() && !self.normalized_text.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceCaptureBudgetError {
    PersistedUsageExceedsLimit,
    DuplicatePersistedRun,
}

/// Run-byte accounting for source capture. Production callers initialize this
/// from `SUM(length(normalized_text))` while holding the same transaction used
/// to insert new sources, then retain it for every capture in that transaction.
/// A fresh value only represents a run with no persisted source text.
#[derive(Debug, Default)]
pub struct EvidenceCaptureBudget {
    used_run_bytes: HashMap<String, usize>,
}

impl EvidenceCaptureBudget {
    pub fn from_persisted_totals(
        totals: impl IntoIterator<Item = (String, usize)>,
    ) -> Result<Self, EvidenceCaptureBudgetError> {
        let mut budget = Self::default();
        for (run_id, used_bytes) in totals {
            if used_bytes > EVIDENCE_RUN_MAX_BYTES {
                return Err(EvidenceCaptureBudgetError::PersistedUsageExceedsLimit);
            }
            if budget.used_run_bytes.insert(run_id, used_bytes).is_some() {
                return Err(EvidenceCaptureBudgetError::DuplicatePersistedRun);
            }
        }
        Ok(budget)
    }

    pub fn used_bytes(&self, run_id: &str) -> usize {
        self.used_run_bytes.get(run_id).copied().unwrap_or(0)
    }

    /// Captures sources with host-assigned identity and provenance. The budget
    /// remains updated across calls, so later calls cannot reopen bytes consumed
    /// earlier in the same transaction.
    pub fn capture(
        &mut self,
        inputs: impl IntoIterator<Item = EvidenceSourceInput>,
    ) -> Vec<EvidenceSource> {
        inputs
            .into_iter()
            .map(|input| self.capture_one(input))
            .collect()
    }

    fn capture_one(&mut self, input: EvidenceSourceInput) -> EvidenceSource {
        let used_bytes = self.used_run_bytes.entry(input.run_id.clone()).or_default();
        let remaining = EVIDENCE_RUN_MAX_BYTES.saturating_sub(*used_bytes);
        let allowed = if input.omission_reason.is_some() {
            0
        } else {
            EVIDENCE_SOURCE_MAX_BYTES.min(remaining)
        };
        let normalized = normalize_evidence_text_bounded(&input.text, allowed);
        let captured_bytes = normalized.text.len();

        let capture_state = if let Some(reason) = input.omission_reason {
            SourceCaptureState::Omitted {
                original_bytes: normalized.normalized_bytes,
                reason,
            }
        } else if allowed == 0 && normalized.normalized_bytes > 0 {
            SourceCaptureState::Omitted {
                original_bytes: normalized.normalized_bytes,
                reason: EvidenceOmissionReason::RunByteLimit,
            }
        } else if normalized.normalized_bytes > captured_bytes {
            SourceCaptureState::Truncated {
                original_bytes: normalized.normalized_bytes,
                captured_bytes,
            }
        } else {
            SourceCaptureState::Complete {
                original_bytes: normalized.normalized_bytes,
            }
        };
        *used_bytes += captured_bytes;
        let digest = evidence_text_digest(EVIDENCE_NORMALIZATION_VERSION, &normalized.text);

        EvidenceSource {
            source_id: Uuid::new_v4().to_string(),
            run_id: input.run_id,
            step_id: input.step_id,
            node_id: input.node_id,
            origin: input.origin,
            producer_identity: input.producer_identity,
            source_label: input.source_label,
            observed_at: input.observed_at,
            normalization_version: EVIDENCE_NORMALIZATION_VERSION,
            normalized_text: normalized.text,
            capture_state,
            digest,
        }
    }
}

pub fn normalize_evidence_text(input: &str) -> String {
    normalize_evidence_text_bounded(input, usize::MAX).text
}

struct BoundedNormalizedText {
    text: String,
    normalized_bytes: usize,
}

fn normalize_evidence_text_bounded(input: &str, max_bytes: usize) -> BoundedNormalizedText {
    let mut normalized = String::with_capacity(input.len().min(max_bytes));
    let mut normalized_bytes = 0usize;
    let mut accepting_prefix = true;
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        let normalized_character = if character == '\r' {
            if matches!(chars.peek(), Some('\n')) {
                chars.next();
            }
            '\n'
        } else if character.is_control() && character != '\n' && character != '\t' {
            '\u{fffd}'
        } else {
            character
        };
        let character_bytes = normalized_character.len_utf8();
        normalized_bytes = normalized_bytes.saturating_add(character_bytes);
        if accepting_prefix && normalized.len().saturating_add(character_bytes) <= max_bytes {
            normalized.push(normalized_character);
        } else {
            accepting_prefix = false;
        }
    }
    BoundedNormalizedText {
        text: normalized,
        normalized_bytes,
    }
}

fn invalid_evidence_column(name: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(0, name.into(), rusqlite::types::Type::Text)
}

fn source_capture_columns(
    state: &SourceCaptureState,
) -> (&'static str, Option<&'static str>, usize, usize) {
    match state {
        SourceCaptureState::Complete { original_bytes } => {
            ("complete", None, *original_bytes, *original_bytes)
        }
        SourceCaptureState::Truncated {
            original_bytes,
            captured_bytes,
        } => ("truncated", None, *original_bytes, *captured_bytes),
        SourceCaptureState::Omitted {
            original_bytes,
            reason,
        } => ("omitted", Some(reason.as_str()), *original_bytes, 0),
    }
}

fn map_evidence_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceSource> {
    let observed_at: String = row.get(7)?;
    let observed_at = DateTime::parse_from_rfc3339(&observed_at)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?
        .with_timezone(&Utc);
    let capture_state_name: String = row.get(10)?;
    let omission_reason: Option<String> = row.get(11)?;
    let original_bytes = usize::try_from(row.get::<_, i64>(12)?)
        .map_err(|_| invalid_evidence_column("original_bytes"))?;
    let captured_bytes = usize::try_from(row.get::<_, i64>(13)?)
        .map_err(|_| invalid_evidence_column("captured_bytes"))?;
    let capture_state = match capture_state_name.as_str() {
        "complete" => SourceCaptureState::Complete { original_bytes },
        "truncated" => SourceCaptureState::Truncated {
            original_bytes,
            captured_bytes,
        },
        "omitted" => SourceCaptureState::Omitted {
            original_bytes,
            reason: EvidenceOmissionReason::from_db(
                omission_reason
                    .as_deref()
                    .ok_or_else(|| invalid_evidence_column("omission_reason"))?,
            )?,
        },
        _ => return Err(invalid_evidence_column("capture_state")),
    };

    Ok(EvidenceSource {
        source_id: row.get(0)?,
        run_id: row.get(1)?,
        step_id: row.get(2)?,
        node_id: row.get(3)?,
        origin: EvidenceOrigin::from_db(&row.get::<_, String>(4)?)?,
        producer_identity: row.get(5)?,
        source_label: row.get(6)?,
        observed_at,
        normalization_version: u32::try_from(row.get::<_, i64>(8)?)
            .map_err(|_| invalid_evidence_column("normalization_version"))?,
        normalized_text: row.get(9)?,
        capture_state,
        digest: row.get(14)?,
    })
}

const SOURCE_SELECT: &str = "id, run_id, step_id, node_id, origin, producer_identity,
    source_label, observed_at, normalization_version, normalized_text, capture_state,
    omission_reason, original_bytes, captured_bytes, content_digest";

pub(crate) fn list_run_evidence_sources_conn(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<EvidenceSource>, DbError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {SOURCE_SELECT} FROM run_evidence_sources
         WHERE run_id = ?1 ORDER BY observed_at, rowid"
    ))?;
    let sources = statement
        .query_map([run_id], map_evidence_source)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(sources)
}

fn insert_evidence_source(conn: &Connection, source: &EvidenceSource) -> Result<(), DbError> {
    let (capture_state, omission_reason, original_bytes, captured_bytes) =
        source_capture_columns(&source.capture_state);
    conn.execute(
        "INSERT INTO run_evidence_sources
           (id, run_id, step_id, node_id, origin, producer_identity, source_label,
            observed_at, normalization_version, normalized_text, capture_state,
            omission_reason, original_bytes, captured_bytes, content_digest)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            source.source_id,
            source.run_id,
            source.step_id,
            source.node_id,
            source.origin.as_str(),
            source.producer_identity,
            source.source_label,
            source.observed_at.to_rfc3339(),
            i64::from(source.normalization_version),
            source.normalized_text,
            capture_state,
            omission_reason,
            i64::try_from(original_bytes)
                .map_err(|_| DbError::Other("evidence_source_too_large".into()))?,
            i64::try_from(captured_bytes)
                .map_err(|_| DbError::Other("evidence_source_too_large".into()))?,
            source.digest,
        ],
    )?;
    Ok(())
}

/// Capture sources on an existing transaction. The producing step must have
/// already been inserted on this connection, so source text and step identity
/// commit or roll back together.
pub(crate) fn capture_run_evidence_sources_conn(
    conn: &Connection,
    mut inputs: Vec<EvidenceSourceInput>,
) -> Result<Vec<EvidenceSource>, DbError> {
    for input in &mut inputs {
        if input.omission_reason.is_none() && contains_secret_like_material(&input.text) {
            input.omission_reason = Some(EvidenceOmissionReason::SensitiveContent);
        }
        if input.origin == EvidenceOrigin::ExternalTriggerData
            && input
                .source_label
                .as_deref()
                .map_or(true, |label| label.trim().is_empty())
        {
            return Err(DbError::Other("evidence_source_label_required".into()));
        }
    }

    let run_ids = inputs
        .iter()
        .map(|input| input.run_id.clone())
        .collect::<HashSet<_>>();
    let mut totals = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        let used_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(length(CAST(normalized_text AS BLOB))), 0)
             FROM run_evidence_sources WHERE run_id = ?1",
            [&run_id],
            |row| row.get(0),
        )?;
        let used_bytes = usize::try_from(used_bytes)
            .map_err(|_| DbError::Other("evidence_run_budget_invalid".into()))?;
        totals.push((run_id, used_bytes));
    }
    let mut budget = EvidenceCaptureBudget::from_persisted_totals(totals)
        .map_err(|_| DbError::Other("evidence_run_budget_invalid".into()))?;
    let sources = budget.capture(inputs);
    for source in &sources {
        if let Some(step_id) = source.step_id.as_deref() {
            let belongs_to_run: bool = conn.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM run_steps WHERE id = ?1 AND run_id = ?2
                 )",
                params![step_id, source.run_id],
                |row| row.get(0),
            )?;
            if !belongs_to_run {
                return Err(DbError::Other("evidence_step_run_mismatch".into()));
            }
        }
        insert_evidence_source(conn, source)?;
    }
    Ok(sources)
}

/// Turn delivered native lookup identities into comparison-only evidence
/// packets. This deliberately records no lookup body: only the backend-owned
/// source reference, version, state, and bounded page identity are captured.
pub(crate) fn lookup_trace_evidence_inputs_conn(
    conn: &Connection,
    run_id: &str,
    node_id: &str,
) -> Result<Vec<EvidenceSourceInput>, DbError> {
    let mut statement = conn.prepare(
        "SELECT t.tool, t.request_id, t.created_at,
                s.reference, s.source_kind, s.source_id, s.version, s.state, s.page
           FROM memory_lookup_traces t
           JOIN memory_lookup_trace_sources s ON s.trace_id = t.id
          WHERE t.run_id = ?1
            AND t.node_id = ?2
            AND t.step_id IS NULL
            AND t.delivery = 'delivered'
          ORDER BY t.created_at, t.id, s.ordinal",
    )?;
    let mut rows = statement.query(params![run_id, node_id])?;
    let mut inputs = Vec::new();
    while let Some(row) = rows.next()? {
        let observed_at = DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
            .map_err(|_| DbError::Other("lookup_trace_timestamp_invalid".into()))?
            .with_timezone(&Utc);
        let tool: String = row.get(0)?;
        let request_id: String = row.get(1)?;
        let reference: String = row.get(3)?;
        let source_kind: String = row.get(4)?;
        let source_id: String = row.get(5)?;
        let version: String = row.get(6)?;
        let state: String = row.get(7)?;
        let page: Option<i64> = row.get(8)?;
        let page = page.map(|value| value.to_string()).unwrap_or_else(|| "none".into());
        let text = format!(
            "lookup source={source_kind}:{source_id} version={version} state={state} \
             reference={reference} page={page}"
        );
        inputs.push(EvidenceSourceInput {
            run_id: run_id.to_owned(),
            step_id: None,
            node_id: Some(node_id.to_owned()),
            origin: if source_kind == "memory" {
                EvidenceOrigin::PriorMemory
            } else {
                EvidenceOrigin::RetrievedHistoricalSummary
            },
            producer_identity: Some(format!("native_lookup:{tool}:{request_id}")),
            source_label: Some("native_memory_lookup".into()),
            observed_at,
            text,
            omission_reason: None,
        });
    }
    Ok(inputs)
}

/// Deliberate history/workflow cleanup entry point. The delete trigger scrubs
/// copied quotes and digests even if a caller deletes a run or step directly.
pub(crate) fn delete_workflow_evidence_sources_conn(
    conn: &Connection,
    workflow_id: &str,
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM run_evidence_sources
         WHERE run_id IN (SELECT id FROM runs WHERE workflow_id = ?1)",
        [workflow_id],
    )?;
    Ok(())
}

impl Db {
    /// Persist source capture and its run-byte accounting in one transaction.
    /// Callers pass original structured segments, never a rendered prompt.
    pub fn capture_run_evidence_sources(
        &self,
        inputs: Vec<EvidenceSourceInput>,
    ) -> Result<Vec<EvidenceSource>, DbError> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let sources = capture_run_evidence_sources_conn(&transaction, inputs)?;
            transaction.commit()?;
            Ok(sources)
        })
    }

    pub fn list_run_evidence_sources(&self, run_id: &str) -> Result<Vec<EvidenceSource>, DbError> {
        self.with_conn(|conn| list_run_evidence_sources_conn(conn, run_id))
    }

    pub fn build_run_evidence_envelope(&self, run_id: &str) -> Result<EvidenceEnvelope, DbError> {
        let mut sources = self.list_run_evidence_sources(run_id)?;
        sources.sort_by_key(|source| {
            let comparison_only =
                source.origin.eligibility() == EvidenceEligibility::ComparisonOnly;
            (
                comparison_only,
                source.observed_at.timestamp_micros(),
                source.source_id.clone(),
            )
        });
        let mut envelope = EvidenceEnvelope::new(run_id, Vec::new());
        for source in sources {
            let Some(fragment) = EvidenceFragment::from_source(&source) else {
                envelope.omitted_source_count += 1;
                let notice = EvidenceCatalogOmission {
                    source_id: source.source_id,
                    origin: source.origin,
                    source_label: source.source_label,
                    capture_state: source.capture_state,
                    reason: EvidenceCatalogOmissionReason::SourceOmitted,
                };
                envelope.omissions.push(notice);
                if serde_json::to_vec(&envelope)
                    .map_err(|_| DbError::Other("evidence_catalog_serialization_failed".into()))?
                    .len()
                    > EVIDENCE_REVIEW_CATALOG_MAX_BYTES
                {
                    envelope.omissions.pop();
                }
                continue;
            };
            envelope.fragments.push(fragment);
            let serialized_bytes = serde_json::to_vec(&envelope)
                .map_err(|_| DbError::Other("evidence_catalog_serialization_failed".into()))?
                .len();
            if serialized_bytes > EVIDENCE_REVIEW_FRAGMENT_BUDGET_BYTES {
                envelope.fragments.pop();
                envelope.catalog_omitted_source_count += 1;
                envelope.omissions.push(EvidenceCatalogOmission {
                    source_id: source.source_id,
                    origin: source.origin,
                    source_label: source.source_label,
                    capture_state: source.capture_state,
                    reason: EvidenceCatalogOmissionReason::CatalogByteLimit,
                });
                if serde_json::to_vec(&envelope)
                    .map_err(|_| DbError::Other("evidence_catalog_serialization_failed".into()))?
                    .len()
                    > EVIDENCE_REVIEW_CATALOG_MAX_BYTES
                {
                    envelope.omissions.pop();
                }
            }
        }
        Ok(envelope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceFragment {
    pub fragment_id: String,
    pub source_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub node_id: Option<String>,
    pub origin: EvidenceOrigin,
    pub producer_identity: Option<String>,
    pub source_label: Option<String>,
    pub normalization_version: u32,
    pub text: String,
    pub digest: String,
    pub capture_state: SourceCaptureState,
}

impl EvidenceFragment {
    pub fn from_source(source: &EvidenceSource) -> Option<Self> {
        if !source.is_citeable() {
            return None;
        }
        Some(Self {
            fragment_id: Uuid::new_v4().to_string(),
            source_id: source.source_id.clone(),
            run_id: source.run_id.clone(),
            step_id: source.step_id.clone(),
            node_id: source.node_id.clone(),
            origin: source.origin,
            producer_identity: source.producer_identity.clone(),
            source_label: source.source_label.clone(),
            normalization_version: source.normalization_version,
            text: source.normalized_text.clone(),
            digest: evidence_text_digest(source.normalization_version, &source.normalized_text),
            capture_state: source.capture_state.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceCatalogOmissionReason {
    SourceOmitted,
    CatalogByteLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceCatalogOmission {
    pub source_id: String,
    pub origin: EvidenceOrigin,
    pub source_label: Option<String>,
    pub capture_state: SourceCaptureState,
    pub reason: EvidenceCatalogOmissionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceCitation {
    pub source_id: String,
    pub fragment_id: String,
    pub fragment_digest: String,
    pub normalization_version: u32,
    pub quote: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceEnvelope {
    pub schema_version: u32,
    pub run_id: String,
    pub fragments: Vec<EvidenceFragment>,
    pub omissions: Vec<EvidenceCatalogOmission>,
    pub omitted_source_count: usize,
    pub catalog_omitted_source_count: usize,
}

impl EvidenceEnvelope {
    pub fn new(run_id: impl Into<String>, fragments: Vec<EvidenceFragment>) -> Self {
        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            run_id: run_id.into(),
            fragments,
            omissions: Vec::new(),
            omitted_source_count: 0,
            catalog_omitted_source_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedCitation {
    citation: EvidenceCitation,
    fragment: EvidenceFragment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedEvidence {
    schema_version: u32,
    subject: EvidenceSubject,
    citations: Vec<ValidatedCitation>,
}

impl ValidatedCitation {
    pub fn citation(&self) -> &EvidenceCitation {
        &self.citation
    }

    pub fn origin(&self) -> EvidenceOrigin {
        self.fragment.origin
    }

    pub fn fragment(&self) -> &EvidenceFragment {
        &self.fragment
    }
}

impl ValidatedEvidence {
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn subject(&self) -> EvidenceSubject {
        self.subject
    }

    pub fn citations(&self) -> &[ValidatedCitation] {
        &self.citations
    }

    /// Preserve a legacy single-node hint only when every cited source has the
    /// same concrete node. Multi-source provenance remains in evidence rows.
    pub fn common_source_node_id(&self) -> Option<String> {
        let mut node: Option<&str> = None;
        for citation in &self.citations {
            let current = citation.fragment.node_id.as_deref()?;
            match node {
                None => node = Some(current),
                Some(existing) if existing == current => {}
                Some(_) => return None,
            }
        }
        node.map(str::to_string)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEvidenceItem {
    pub ordinal: usize,
    pub source_id: Option<String>,
    pub fragment_id: String,
    pub source_run_id: String,
    pub source_step_id: Option<String>,
    pub source_node_id: Option<String>,
    pub origin: EvidenceOrigin,
    pub producer_identity: Option<String>,
    pub source_label: Option<String>,
    pub quote: Option<String>,
    pub byte_start: Option<usize>,
    pub byte_end: Option<usize>,
    pub normalization_version: Option<u32>,
    pub fragment_digest: Option<String>,
    pub source_removed: bool,
}

fn usize_sql(value: usize, code: &'static str) -> Result<i64, DbError> {
    i64::try_from(value).map_err(|_| DbError::Other(code.into()))
}

/// Persist validated candidate evidence on the caller's transaction. All
/// source identity and origin fields come from the host-owned envelope.
pub(crate) fn write_candidate_evidence_conn(
    conn: &Connection,
    candidate_id: &str,
    evidence: &ValidatedEvidence,
    created_at: &str,
) -> Result<(), DbError> {
    for (ordinal, validated) in evidence.citations().iter().enumerate() {
        let citation = validated.citation();
        let fragment = validated.fragment();
        conn.execute(
            "INSERT INTO memory_candidate_evidence
               (id, candidate_id, ordinal, source_id, fragment_id, source_run_id,
                source_step_id, source_node_id, origin, producer_identity, source_label,
                quote, byte_start, byte_end, normalization_version, fragment_digest,
                source_removed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                     ?14, ?15, ?16, 0, ?17)",
            params![
                Uuid::new_v4().to_string(),
                candidate_id,
                usize_sql(ordinal, "invalid_evidence_ordinal")?,
                citation.source_id,
                citation.fragment_id,
                fragment.run_id,
                fragment.step_id,
                fragment.node_id,
                fragment.origin.as_str(),
                fragment.producer_identity,
                fragment.source_label,
                citation.quote,
                usize_sql(citation.byte_start, "invalid_evidence_range")?,
                usize_sql(citation.byte_end, "invalid_evidence_range")?,
                i64::from(citation.normalization_version),
                citation.fragment_digest,
                created_at,
            ],
        )?;
    }
    Ok(())
}

/// Copy queue evidence to canonical provenance before a decided candidate may
/// be cleared. This runs in the same transaction as the canonical memory write.
pub(crate) fn copy_candidate_evidence_to_memory_conn(
    conn: &Connection,
    candidate_id: &str,
    memory_id: &str,
    created_at: &str,
) -> Result<(), DbError> {
    let copied = conn.execute(
        "INSERT INTO memory_evidence
           (id, memory_id, ordinal, source_id, fragment_id, source_run_id,
            source_step_id, source_node_id, origin, producer_identity, source_label,
            quote, byte_start, byte_end, normalization_version, fragment_digest,
            source_removed, created_at)
         SELECT lower(hex(randomblob(16))), ?2,
                ordinal + COALESCE((SELECT MAX(existing.ordinal) + 1
                                      FROM memory_evidence existing
                                     WHERE existing.memory_id = ?2), 0),
                source_id, fragment_id,
                source_run_id, source_step_id, source_node_id, origin, producer_identity,
                source_label, quote, byte_start, byte_end, normalization_version,
                fragment_digest, source_removed, ?3
           FROM memory_candidate_evidence WHERE candidate_id = ?1 ORDER BY ordinal",
        params![candidate_id, memory_id, created_at],
    )?;
    if copied == 0 {
        return Err(DbError::Other("candidate_evidence_missing".into()));
    }
    Ok(())
}

fn map_memory_evidence_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEvidenceItem> {
    let ordinal =
        usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_evidence_column("ordinal"))?;
    let byte_start = row
        .get::<_, Option<i64>>(10)?
        .map(usize::try_from)
        .transpose()
        .map_err(|_| invalid_evidence_column("byte_start"))?;
    let byte_end = row
        .get::<_, Option<i64>>(11)?
        .map(usize::try_from)
        .transpose()
        .map_err(|_| invalid_evidence_column("byte_end"))?;
    let normalization_version = row
        .get::<_, Option<i64>>(12)?
        .map(u32::try_from)
        .transpose()
        .map_err(|_| invalid_evidence_column("normalization_version"))?;
    Ok(MemoryEvidenceItem {
        ordinal,
        source_id: row.get(1)?,
        fragment_id: row.get(2)?,
        source_run_id: row.get(3)?,
        source_step_id: row.get(4)?,
        source_node_id: row.get(5)?,
        origin: EvidenceOrigin::from_db(&row.get::<_, String>(6)?)?,
        producer_identity: row.get(7)?,
        source_label: row.get(8)?,
        quote: row.get(9)?,
        byte_start,
        byte_end,
        normalization_version,
        fragment_digest: row.get(13)?,
        source_removed: row.get::<_, i64>(14)? != 0,
    })
}

const PROVENANCE_SELECT: &str = "ordinal, source_id, fragment_id, source_run_id,
    source_step_id, source_node_id, origin, producer_identity, source_label, quote,
    byte_start, byte_end, normalization_version, fragment_digest, source_removed";

fn list_evidence_items_conn(
    conn: &Connection,
    table: &str,
    owner_column: &str,
    owner_id: &str,
) -> Result<Vec<MemoryEvidenceItem>, DbError> {
    let sql = format!(
        "SELECT {PROVENANCE_SELECT} FROM {table} WHERE {owner_column} = ?1 ORDER BY ordinal"
    );
    let mut statement = conn.prepare(&sql)?;
    let items = statement
        .query_map([owner_id], map_memory_evidence_item)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(items)
}

pub(crate) fn list_candidate_evidence_conn(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Vec<MemoryEvidenceItem>, DbError> {
    list_evidence_items_conn(
        conn,
        "memory_candidate_evidence",
        "candidate_id",
        candidate_id,
    )
}

impl Db {
    pub fn list_candidate_evidence(
        &self,
        candidate_id: &str,
    ) -> Result<Vec<MemoryEvidenceItem>, DbError> {
        self.with_conn(|conn| list_candidate_evidence_conn(conn, candidate_id))
    }

    pub fn list_memory_evidence(
        &self,
        memory_id: &str,
    ) -> Result<Vec<MemoryEvidenceItem>, DbError> {
        self.with_conn(|conn| {
            list_evidence_items_conn(conn, "memory_evidence", "memory_id", memory_id)
        })
    }

    pub(crate) fn load_candidate_validated_evidence(
        &self,
        candidate_id: &str,
        run_id: &str,
        subject: EvidenceSubject,
    ) -> Result<Result<ValidatedEvidence, &'static str>, DbError> {
        self.with_conn(|conn| validate_candidate_evidence_conn(conn, candidate_id, run_id, subject))
    }
}

impl ValidatedEvidence {
    pub(crate) fn envelope(&self, run_id: &str) -> EvidenceEnvelope {
        EvidenceEnvelope::new(
            run_id,
            self.citations
                .iter()
                .map(|item| item.fragment.clone())
                .collect(),
        )
    }

    pub(crate) fn citation_values(&self) -> Vec<EvidenceCitation> {
        self.citations
            .iter()
            .map(|item| item.citation.clone())
            .collect()
    }
}

/// Reload and validate a candidate's copied quotations against the current
/// immutable source rows while approval holds its transaction.
pub(crate) fn validate_candidate_evidence_conn(
    conn: &Connection,
    candidate_id: &str,
    run_id: &str,
    subject: EvidenceSubject,
) -> Result<Result<ValidatedEvidence, &'static str>, DbError> {
    let items = list_evidence_items_conn(
        conn,
        "memory_candidate_evidence",
        "candidate_id",
        candidate_id,
    )?;
    if items.is_empty() {
        return Ok(Err("candidate_evidence_missing"));
    }
    if items.iter().any(|item| item.source_removed) {
        return Ok(Err("source_removed"));
    }

    let mut fragments = Vec::with_capacity(items.len());
    let mut citations = Vec::with_capacity(items.len());
    for item in items {
        let Some(source_id) = item.source_id.as_deref() else {
            return Ok(Err("source_removed"));
        };
        let source = conn
            .query_row(
                &format!("SELECT {SOURCE_SELECT} FROM run_evidence_sources WHERE id = ?1"),
                [source_id],
                map_evidence_source,
            )
            .optional()?;
        let Some(source) = source else {
            return Ok(Err("source_removed"));
        };
        if source.run_id != run_id
            || item.source_run_id != run_id
            || item.origin != source.origin
            || item.source_step_id != source.step_id
            || item.source_node_id != source.node_id
            || item.producer_identity != source.producer_identity
            || item.source_label != source.source_label
        {
            return Ok(Err("evidence_source_changed"));
        }
        let (Some(quote), Some(byte_start), Some(byte_end), Some(version), Some(digest)) = (
            item.quote,
            item.byte_start,
            item.byte_end,
            item.normalization_version,
            item.fragment_digest,
        ) else {
            return Ok(Err("source_removed"));
        };
        let normalized_text = source.normalized_text;
        let source_digest = evidence_text_digest(source.normalization_version, &normalized_text);
        fragments.push(EvidenceFragment {
            fragment_id: item.fragment_id.clone(),
            source_id: source.source_id.clone(),
            run_id: source.run_id,
            step_id: source.step_id,
            node_id: source.node_id,
            origin: source.origin,
            producer_identity: source.producer_identity,
            source_label: source.source_label,
            normalization_version: source.normalization_version,
            text: normalized_text,
            digest: source_digest,
            capture_state: source.capture_state,
        });
        citations.push(EvidenceCitation {
            source_id: source.source_id,
            fragment_id: item.fragment_id,
            fragment_digest: digest,
            normalization_version: version,
            quote,
            byte_start,
            byte_end,
        });
    }
    let envelope = EvidenceEnvelope::new(run_id, fragments);
    Ok(validate_evidence_citations(&envelope, &citations, subject)
        .map_err(EvidenceValidationError::as_code))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceValidationError {
    UnsupportedSchemaVersion,
    CitationCount,
    CitationTooLarge,
    CitationAggregateTooLarge,
    DuplicateCitation,
    UnknownFragment,
    WrongRun,
    WrongSource,
    NormalizationVersionMismatch,
    ChangedFragmentDigest,
    InvalidByteRange,
    ChangedQuote,
    IndependentEvidenceRequired,
}

impl EvidenceValidationError {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::UnsupportedSchemaVersion => "unsupported_evidence_schema",
            Self::CitationCount => "invalid_evidence_count",
            Self::CitationTooLarge => "evidence_quote_too_large",
            Self::CitationAggregateTooLarge => "evidence_aggregate_too_large",
            Self::DuplicateCitation => "duplicate_evidence_citation",
            Self::UnknownFragment => "unknown_evidence_fragment",
            Self::WrongRun => "evidence_wrong_run",
            Self::WrongSource => "evidence_wrong_source",
            Self::NormalizationVersionMismatch => "evidence_normalization_mismatch",
            Self::ChangedFragmentDigest => "evidence_fragment_changed",
            Self::InvalidByteRange => "invalid_evidence_range",
            Self::ChangedQuote => "evidence_quote_changed",
            Self::IndependentEvidenceRequired => "independent_evidence_required",
        }
    }
}

pub fn validate_evidence_citations(
    envelope: &EvidenceEnvelope,
    citations: &[EvidenceCitation],
    subject: EvidenceSubject,
) -> Result<ValidatedEvidence, EvidenceValidationError> {
    if envelope.schema_version != EVIDENCE_SCHEMA_VERSION {
        return Err(EvidenceValidationError::UnsupportedSchemaVersion);
    }
    if citations.is_empty() || citations.len() > EVIDENCE_CITATION_MAX_COUNT {
        return Err(EvidenceValidationError::CitationCount);
    }
    if citations.iter().any(|citation| {
        citation.quote.is_empty() || citation.quote.len() > EVIDENCE_CITATION_MAX_BYTES
    }) {
        return Err(EvidenceValidationError::CitationTooLarge);
    }
    let aggregate_bytes = citations.iter().try_fold(0usize, |total, citation| {
        total.checked_add(citation.quote.len())
    });
    if !matches!(aggregate_bytes, Some(total) if total <= EVIDENCE_CITATION_AGGREGATE_MAX_BYTES) {
        return Err(EvidenceValidationError::CitationAggregateTooLarge);
    }

    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(citations.len());
    let mut independently_supported = false;

    for citation in citations {
        if !seen.insert((
            citation.fragment_id.as_str(),
            citation.byte_start,
            citation.byte_end,
        )) {
            return Err(EvidenceValidationError::DuplicateCitation);
        }
        let fragment = envelope
            .fragments
            .iter()
            .find(|fragment| fragment.fragment_id == citation.fragment_id)
            .ok_or(EvidenceValidationError::UnknownFragment)?;
        if fragment.run_id != envelope.run_id {
            return Err(EvidenceValidationError::WrongRun);
        }
        if fragment.source_id != citation.source_id {
            return Err(EvidenceValidationError::WrongSource);
        }
        if !is_supported_normalization_version(fragment.normalization_version)
            || !is_supported_normalization_version(citation.normalization_version)
            || citation.normalization_version != fragment.normalization_version
        {
            return Err(EvidenceValidationError::NormalizationVersionMismatch);
        }
        let expected_digest = evidence_text_digest(fragment.normalization_version, &fragment.text);
        if citation.fragment_digest != fragment.digest || fragment.digest != expected_digest {
            return Err(EvidenceValidationError::ChangedFragmentDigest);
        }
        let quoted = fragment
            .text
            .get(citation.byte_start..citation.byte_end)
            .ok_or(EvidenceValidationError::InvalidByteRange)?;
        if quoted != citation.quote {
            return Err(EvidenceValidationError::ChangedQuote);
        }
        independently_supported |= fragment.origin.supports(subject);
        validated.push(ValidatedCitation {
            citation: citation.clone(),
            fragment: fragment.clone(),
        });
    }

    if !independently_supported {
        return Err(EvidenceValidationError::IndependentEvidenceRequired);
    }

    Ok(ValidatedEvidence {
        schema_version: envelope.schema_version,
        subject,
        citations: validated,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct ClaimDigestInput<'a> {
    pub operation: &'a str,
    pub target_memory_id: Option<&'a str>,
    pub scope_type: &'a str,
    pub scope_key: &'a str,
    pub memory_type: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub is_inference: bool,
}

pub fn evidence_claim_digest(claim: ClaimDigestInput<'_>, evidence: &ValidatedEvidence) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        "evidence_schema",
        &evidence.schema_version.to_string(),
    );
    hash_field(&mut hasher, "evidence_subject", evidence.subject.as_str());
    hash_field(&mut hasher, "operation", claim.operation);
    hash_field(
        &mut hasher,
        "target_memory_id",
        claim.target_memory_id.unwrap_or(""),
    );
    hash_field(&mut hasher, "scope_type", claim.scope_type);
    hash_field(&mut hasher, "scope_key", claim.scope_key);
    hash_field(&mut hasher, "memory_type", claim.memory_type);
    hash_field(&mut hasher, "title", claim.title);
    hash_field(&mut hasher, "body", claim.body);
    hash_field(
        &mut hasher,
        "is_inference",
        if claim.is_inference { "1" } else { "0" },
    );

    let mut citations = evidence.citations.iter().collect::<Vec<_>>();
    citations.sort_by(|left, right| {
        left.citation
            .fragment_id
            .cmp(&right.citation.fragment_id)
            .then(left.citation.byte_start.cmp(&right.citation.byte_start))
            .then(left.citation.byte_end.cmp(&right.citation.byte_end))
    });
    for item in citations {
        hash_field(&mut hasher, "source_id", &item.citation.source_id);
        hash_field(&mut hasher, "fragment_id", &item.citation.fragment_id);
        hash_field(
            &mut hasher,
            "fragment_digest",
            &item.citation.fragment_digest,
        );
        hash_field(
            &mut hasher,
            "normalization_version",
            &item.citation.normalization_version.to_string(),
        );
        hash_field(&mut hasher, "origin", item.origin().as_str());
        hash_field(
            &mut hasher,
            "eligibility",
            item.origin().eligibility().as_str(),
        );
        hash_field(&mut hasher, "quote", &item.citation.quote);
        hash_field(
            &mut hasher,
            "byte_start",
            &item.citation.byte_start.to_string(),
        );
        hash_field(&mut hasher, "byte_end", &item.citation.byte_end.to_string());
    }
    format!("{:x}", hasher.finalize())
}

pub fn evidence_text_digest(normalization_version: u32, text: &str) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        "normalization_version",
        &normalization_version.to_string(),
    );
    hash_field(&mut hasher, "text", text);
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(
        u64::try_from(name.len())
            .expect("field name length fits u64")
            .to_be_bytes(),
    );
    hasher.update(name.as_bytes());
    hasher.update(
        u64::try_from(value.len())
            .expect("field value length fits u64")
            .to_be_bytes(),
    );
    hasher.update(value.as_bytes());
}

fn is_supported_normalization_version(version: u32) -> bool {
    matches!(version, EVIDENCE_NORMALIZATION_VERSION)
}

#[cfg(test)]
mod contract {
    use super::*;
    use chrono::TimeZone;

    fn input(origin: EvidenceOrigin, text: impl Into<String>) -> EvidenceSourceInput {
        EvidenceSourceInput {
            run_id: "run-1".into(),
            step_id: Some("step-1".into()),
            node_id: Some("node-1".into()),
            origin,
            producer_identity: Some("synthetic-fixture".into()),
            source_label: Some("test input".into()),
            observed_at: Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).single().unwrap(),
            text: text.into(),
            omission_reason: None,
        }
    }

    fn source(origin: EvidenceOrigin, text: &str) -> EvidenceSource {
        EvidenceCaptureBudget::default()
            .capture([input(origin, text)])
            .into_iter()
            .next()
            .unwrap()
    }

    fn fragment_and_citation(
        origin: EvidenceOrigin,
        text: &str,
        quote: &str,
    ) -> (EvidenceFragment, EvidenceCitation) {
        let fragment = EvidenceFragment::from_source(&source(origin, text)).unwrap();
        let byte_start = fragment.text.find(quote).unwrap();
        let citation = EvidenceCitation {
            source_id: fragment.source_id.clone(),
            fragment_id: fragment.fragment_id.clone(),
            fragment_digest: fragment.digest.clone(),
            normalization_version: fragment.normalization_version,
            quote: quote.into(),
            byte_start,
            byte_end: byte_start + quote.len(),
        };
        (fragment, citation)
    }

    #[test]
    fn origin_eligibility_is_host_owned_and_category_specific() {
        assert_eq!(
            EvidenceOrigin::AuthoredTask.eligibility(),
            EvidenceEligibility::AuthoredUserStatement
        );
        assert_eq!(
            EvidenceOrigin::ConfiguredWorkflowInstruction.eligibility(),
            EvidenceEligibility::WorkflowConfigurationOnly
        );
        assert_eq!(
            EvidenceOrigin::ExternalTriggerData.eligibility(),
            EvidenceEligibility::RecordedExternalData
        );
        assert_eq!(
            EvidenceOrigin::HostRecordedObservation.eligibility(),
            EvidenceEligibility::RecordedHostObservation
        );
        for origin in [
            EvidenceOrigin::ModelOutput,
            EvidenceOrigin::PriorMemory,
            EvidenceOrigin::RetrievedHistoricalSummary,
        ] {
            assert_eq!(origin.eligibility(), EvidenceEligibility::ComparisonOnly);
        }
    }

    #[test]
    fn normalization_is_versioned_deterministic_and_replaces_controls() {
        let raw = "first\r\nsecond\rthird\tkept\0bad\u{0085}control";
        let expected = "first\nsecond\nthird\tkept\u{fffd}bad\u{fffd}control";
        assert_eq!(normalize_evidence_text(raw), expected);
        assert_eq!(normalize_evidence_text(raw), normalize_evidence_text(raw));
        assert_eq!(EVIDENCE_NORMALIZATION_VERSION, 1);
    }

    #[test]
    fn utf8_byte_offsets_select_the_exact_normalized_quote() {
        let (fragment, citation) = fragment_and_citation(
            EvidenceOrigin::AuthoredTask,
            "Café ☕ is preferred",
            "☕ is",
        );
        assert_eq!(
            fragment
                .text
                .get(citation.byte_start..citation.byte_end)
                .unwrap(),
            citation.quote
        );
        let envelope = EvidenceEnvelope::new("run-1", vec![fragment]);
        assert!(validate_evidence_citations(
            &envelope,
            &[citation],
            EvidenceSubject::UserStatement
        )
        .is_ok());
    }

    #[test]
    fn quote_validation_rejects_non_boundaries_and_changed_quotes() {
        let (fragment, citation) =
            fragment_and_citation(EvidenceOrigin::AuthoredTask, "Café is preferred", "Café");
        let envelope = EvidenceEnvelope::new("run-1", vec![fragment]);

        let mut split_code_point = citation.clone();
        split_code_point.byte_end = 4;
        split_code_point.quote = "Caf".into();
        assert_eq!(
            validate_evidence_citations(
                &envelope,
                &[split_code_point],
                EvidenceSubject::UserStatement
            ),
            Err(EvidenceValidationError::InvalidByteRange)
        );

        let mut changed = citation;
        changed.quote = "Tea".into();
        assert_eq!(
            validate_evidence_citations(&envelope, &[changed], EvidenceSubject::UserStatement),
            Err(EvidenceValidationError::ChangedQuote)
        );
    }

    #[test]
    fn source_and_fragment_digests_bind_version_and_exact_text() {
        let captured = source(EvidenceOrigin::AuthoredTask, "Remember café");
        let fragment = EvidenceFragment::from_source(&captured).unwrap();
        assert_eq!(
            captured.digest,
            evidence_text_digest(EVIDENCE_NORMALIZATION_VERSION, "Remember café")
        );
        assert_eq!(
            captured.digest,
            "dcafe8d1abf0e792e2e93547abb7b99a3c9e46a698f79493f190b56960ec0cb2"
        );
        assert_eq!(
            captured.digest,
            "dcafe8d1abf0e792e2e93547abb7b99a3c9e46a698f79493f190b56960ec0cb2"
        );
        assert_eq!(
            captured.digest,
            "dcafe8d1abf0e792e2e93547abb7b99a3c9e46a698f79493f190b56960ec0cb2"
        );
        assert_eq!(fragment.digest, captured.digest);
        assert_ne!(
            fragment.digest,
            evidence_text_digest(EVIDENCE_NORMALIZATION_VERSION + 1, &fragment.text)
        );
        assert_ne!(
            fragment.digest,
            evidence_text_digest(EVIDENCE_NORMALIZATION_VERSION, "Remember tea")
        );
    }

    #[test]
    fn schema_version_is_explicit_and_strict() {
        let (fragment, citation) = fragment_and_citation(
            EvidenceOrigin::AuthoredTask,
            "Keep this setting",
            "this setting",
        );
        let mut envelope = EvidenceEnvelope::new("run-1", vec![fragment]);
        assert_eq!(envelope.schema_version, EVIDENCE_SCHEMA_VERSION);
        envelope.schema_version += 1;
        assert_eq!(
            validate_evidence_citations(
                &envelope,
                &[citation.clone()],
                EvidenceSubject::UserStatement
            ),
            Err(EvidenceValidationError::UnsupportedSchemaVersion)
        );
    }

    #[test]
    fn source_capture_respects_utf8_source_and_run_limits() {
        let oversized = "é".repeat(EVIDENCE_SOURCE_MAX_BYTES);
        let mut budget = EvidenceCaptureBudget::default();
        let mut sources =
            budget.capture((0..8).map(|_| input(EvidenceOrigin::AuthoredTask, oversized.clone())));
        sources.extend(budget.capture([input(EvidenceOrigin::AuthoredTask, oversized)]));
        let mut second_run = input(EvidenceOrigin::AuthoredTask, "z".repeat(32));
        second_run.run_id = "run-2".into();
        sources.extend(budget.capture([second_run]));
        let first_run_bytes = sources
            .iter()
            .filter(|item| item.run_id == "run-1")
            .map(|item| item.normalized_text.len())
            .sum::<usize>();

        assert_eq!(first_run_bytes, EVIDENCE_RUN_MAX_BYTES);
        assert!(sources[..8].iter().all(|item| {
            item.normalized_text.len() == EVIDENCE_SOURCE_MAX_BYTES
                && item.capture_state.is_truncated()
                && item
                    .normalized_text
                    .is_char_boundary(item.normalized_text.len())
                && item.shown_marker() == Some(SOURCE_TRUNCATED_MARKER)
        }));
        assert!(sources[8].capture_state.is_omitted());
        assert_eq!(sources[8].shown_marker(), Some(SOURCE_OMITTED_MARKER));
        assert!(EvidenceFragment::from_source(&sources[8]).is_none());
        assert_eq!(sources[9].normalized_text.len(), 32);
        assert!(!sources[9].capture_state.is_truncated());
    }

    #[test]
    fn capture_budget_starts_from_persisted_usage_and_rejects_invalid_totals() {
        let mut budget = EvidenceCaptureBudget::from_persisted_totals([(
            "run-1".into(),
            EVIDENCE_RUN_MAX_BYTES - 4,
        )])
        .unwrap();
        let captured = budget
            .capture([input(EvidenceOrigin::AuthoredTask, "ééé")])
            .remove(0);
        assert_eq!(captured.normalized_text, "éé");
        assert!(captured.capture_state.is_truncated());
        assert_eq!(budget.used_bytes("run-1"), EVIDENCE_RUN_MAX_BYTES);

        let omitted = budget
            .capture([input(EvidenceOrigin::AuthoredTask, "later")])
            .remove(0);
        assert!(omitted.capture_state.is_omitted());
        assert!(omitted.normalized_text.is_empty());

        assert_eq!(
            EvidenceCaptureBudget::from_persisted_totals([(
                "run-1".into(),
                EVIDENCE_RUN_MAX_BYTES + 1
            )])
            .unwrap_err(),
            EvidenceCaptureBudgetError::PersistedUsageExceedsLimit
        );
        assert_eq!(
            EvidenceCaptureBudget::from_persisted_totals([
                ("run-1".into(), 1),
                ("run-1".into(), 2)
            ])
            .unwrap_err(),
            EvidenceCaptureBudgetError::DuplicatePersistedRun
        );
    }

    #[test]
    fn capture_normalizes_huge_expanding_input_with_only_a_bounded_prefix() {
        let repetitions = 2 * 1024 * 1024;
        let huge = "\0\r\n".repeat(repetitions);
        let captured = source(EvidenceOrigin::AuthoredTask, &huge);

        assert_eq!(captured.normalized_text.len(), EVIDENCE_SOURCE_MAX_BYTES);
        assert_eq!(
            captured.capture_state,
            SourceCaptureState::Truncated {
                original_bytes: repetitions * 4,
                captured_bytes: EVIDENCE_SOURCE_MAX_BYTES,
            }
        );
        assert_eq!(
            captured.normalized_text,
            "\u{fffd}\n".repeat(EVIDENCE_SOURCE_MAX_BYTES / 4)
        );
    }

    #[test]
    fn citations_are_bounded_to_one_through_three_exact_quotes() {
        let (fragment, citation) = fragment_and_citation(
            EvidenceOrigin::AuthoredTask,
            &"a".repeat(EVIDENCE_CITATION_MAX_BYTES + 1),
            &"a".repeat(EVIDENCE_CITATION_MAX_BYTES + 1),
        );
        let envelope = EvidenceEnvelope::new("run-1", vec![fragment]);
        assert_eq!(
            validate_evidence_citations(
                &envelope,
                &[citation.clone()],
                EvidenceSubject::UserStatement
            ),
            Err(EvidenceValidationError::CitationTooLarge)
        );
        assert_eq!(
            validate_evidence_citations(&envelope, &[], EvidenceSubject::UserStatement),
            Err(EvidenceValidationError::CitationCount)
        );
        assert_eq!(
            validate_evidence_citations(
                &envelope,
                &[
                    citation.clone(),
                    citation.clone(),
                    citation.clone(),
                    citation
                ],
                EvidenceSubject::UserStatement
            ),
            Err(EvidenceValidationError::CitationCount)
        );
    }

    #[test]
    fn configured_and_retrieved_text_cannot_be_promoted_to_user_statements() {
        for origin in [
            EvidenceOrigin::ConfiguredWorkflowInstruction,
            EvidenceOrigin::ExternalTriggerData,
            EvidenceOrigin::HostRecordedObservation,
            EvidenceOrigin::ModelOutput,
            EvidenceOrigin::PriorMemory,
            EvidenceOrigin::RetrievedHistoricalSummary,
        ] {
            let (fragment, citation) =
                fragment_and_citation(origin, "Always use compact mode", "compact mode");
            let envelope = EvidenceEnvelope::new("run-1", vec![fragment]);
            assert_eq!(
                validate_evidence_citations(&envelope, &[citation], EvidenceSubject::UserStatement),
                Err(EvidenceValidationError::IndependentEvidenceRequired),
                "origin {origin:?}"
            );
        }
    }

    #[test]
    fn configuration_and_observation_subjects_accept_only_their_sources() {
        let (configured_fragment, configured_citation) = fragment_and_citation(
            EvidenceOrigin::ConfiguredWorkflowInstruction,
            "Run on weekdays",
            "weekdays",
        );
        assert!(validate_evidence_citations(
            &EvidenceEnvelope::new("run-1", vec![configured_fragment]),
            &[configured_citation],
            EvidenceSubject::WorkflowConfiguration
        )
        .is_ok());

        let (external_fragment, external_citation) = fragment_and_citation(
            EvidenceOrigin::ExternalTriggerData,
            "status=completed",
            "completed",
        );
        assert!(validate_evidence_citations(
            &EvidenceEnvelope::new("run-1", vec![external_fragment]),
            &[external_citation],
            EvidenceSubject::RecordedObservation
        )
        .is_ok());
    }

    #[test]
    fn matching_unknown_normalization_versions_are_still_rejected() {
        let (mut fragment, mut citation) = fragment_and_citation(
            EvidenceOrigin::AuthoredTask,
            "Keep the source exact",
            "source exact",
        );
        fragment.normalization_version = 99;
        citation.normalization_version = 99;
        fragment.digest = evidence_text_digest(99, &fragment.text);
        citation.fragment_digest = fragment.digest.clone();
        let envelope = EvidenceEnvelope::new("run-1", vec![fragment]);

        assert_eq!(
            validate_evidence_citations(&envelope, &[citation], EvidenceSubject::UserStatement),
            Err(EvidenceValidationError::NormalizationVersionMismatch)
        );
    }

    #[test]
    fn claim_digest_binds_claim_and_selected_evidence_but_not_citation_order() {
        let (fragment_a, citation_a) = fragment_and_citation(
            EvidenceOrigin::AuthoredTask,
            "Use concise reports",
            "concise reports",
        );
        let (fragment_b, citation_b) = fragment_and_citation(
            EvidenceOrigin::AuthoredTask,
            "Keep headings short",
            "headings short",
        );
        let envelope = EvidenceEnvelope::new("run-1", vec![fragment_a, fragment_b]);
        let evidence = validate_evidence_citations(
            &envelope,
            &[citation_a.clone(), citation_b.clone()],
            EvidenceSubject::UserStatement,
        )
        .unwrap();
        let reversed = validate_evidence_citations(
            &envelope,
            &[citation_b.clone(), citation_a.clone()],
            EvidenceSubject::UserStatement,
        )
        .unwrap();
        let claim = ClaimDigestInput {
            operation: "create",
            target_memory_id: None,
            scope_type: "user",
            scope_key: "local-user",
            memory_type: "preference",
            title: "Report style",
            body: "Use concise reports with short headings.",
            is_inference: false,
        };
        let digest = evidence_claim_digest(claim, &evidence);
        assert_eq!(digest, evidence_claim_digest(claim, &reversed));
        assert_eq!(digest.len(), 64);

        let workflow_evidence = validate_evidence_citations(
            &envelope,
            &[citation_a, citation_b],
            EvidenceSubject::WorkflowConfiguration,
        )
        .unwrap();
        assert_ne!(digest, evidence_claim_digest(claim, &workflow_evidence));

        let changed_claim = ClaimDigestInput {
            body: "Use long reports.",
            ..claim
        };
        assert_ne!(digest, evidence_claim_digest(changed_claim, &evidence));
    }
}

#[cfg(test)]
mod capture {
    use super::*;
    use crate::db::{CreateWorkflowInput, Db};
    use chrono::TimeZone;
    use serde_json::json;

    fn fixture() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Evidence capture".into(),
                description: String::new(),
                working_directory: "/synthetic/evidence".into(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs (id, workflow_id, status, created_at)
                 VALUES ('run-evidence', ?1, 'completed', '2026-09-08T12:00:00Z')",
                [&workflow.id],
            )?;
            Ok(())
        })
        .unwrap();
        (db, workflow.id)
    }

    fn input(text: impl Into<String>) -> EvidenceSourceInput {
        EvidenceSourceInput {
            run_id: "run-evidence".into(),
            step_id: None,
            node_id: Some("input".into()),
            origin: EvidenceOrigin::AuthoredTask,
            producer_identity: None,
            source_label: Some("authored task".into()),
            observed_at: Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).single().unwrap(),
            text: text.into(),
            omission_reason: None,
        }
    }

    #[test]
    fn persisted_run_budget_survives_separate_capture_calls() {
        let (db, _) = fixture();
        for _ in 0..8 {
            let source = db
                .capture_run_evidence_sources(vec![input("a".repeat(EVIDENCE_SOURCE_MAX_BYTES))])
                .unwrap()
                .remove(0);
            assert!(!source.capture_state.is_omitted());
        }
        let omitted = db
            .capture_run_evidence_sources(vec![input("later")])
            .unwrap()
            .remove(0);
        assert_eq!(omitted.normalized_text, "");
        assert_eq!(
            omitted.capture_state,
            SourceCaptureState::Omitted {
                original_bytes: 5,
                reason: EvidenceOmissionReason::RunByteLimit,
            }
        );
        let stored = db.list_run_evidence_sources("run-evidence").unwrap();
        assert_eq!(stored.len(), 9);
        assert_eq!(
            stored
                .iter()
                .map(|source| source.normalized_text.len())
                .sum::<usize>(),
            EVIDENCE_RUN_MAX_BYTES
        );
    }

    #[test]
    fn sensitive_and_unavailable_inputs_are_explicit_omissions() {
        let (db, _) = fixture();
        let mut unavailable = input("tool stream was not recorded");
        unavailable.origin = EvidenceOrigin::HostRecordedObservation;
        unavailable.omission_reason = Some(EvidenceOmissionReason::ObservationUnavailable);
        let sources = db
            .capture_run_evidence_sources(vec![input("API_KEY=synthetic-secret"), unavailable])
            .unwrap();
        assert_eq!(sources[0].normalized_text, "");
        assert!(matches!(
            sources[0].capture_state,
            SourceCaptureState::Omitted {
                reason: EvidenceOmissionReason::SensitiveContent,
                ..
            }
        ));
        assert!(matches!(
            sources[1].capture_state,
            SourceCaptureState::Omitted {
                reason: EvidenceOmissionReason::ObservationUnavailable,
                ..
            }
        ));
        assert!(db
            .build_run_evidence_envelope("run-evidence")
            .unwrap()
            .fragments
            .is_empty());
    }

    #[test]
    fn outbound_catalog_reports_truncation_source_omissions_and_catalog_omissions() {
        let (db, _) = fixture();
        let mut inputs = vec![input("z".repeat(EVIDENCE_SOURCE_MAX_BYTES + 100))];
        inputs.push(input("API_KEY=synthetic-secret"));
        inputs.extend((0..3).map(|_| input("\\\"".repeat(EVIDENCE_SOURCE_MAX_BYTES / 2))));
        db.capture_run_evidence_sources(inputs).unwrap();

        let envelope = db.build_run_evidence_envelope("run-evidence").unwrap();
        let request_bytes = serde_json::to_vec(&envelope).unwrap();
        assert!(request_bytes.len() <= EVIDENCE_REVIEW_CATALOG_MAX_BYTES);
        assert!(envelope.fragments.iter().any(|fragment| {
            matches!(fragment.capture_state, SourceCaptureState::Truncated { .. })
        }));
        assert_eq!(envelope.omitted_source_count, 1);
        assert!(envelope.omissions.iter().any(|notice| {
            notice.reason == EvidenceCatalogOmissionReason::SourceOmitted
                && matches!(notice.capture_state, SourceCaptureState::Omitted { .. })
        }));
        assert!(envelope.catalog_omitted_source_count > 0);
        assert!(envelope
            .omissions
            .iter()
            .any(|notice| { notice.reason == EvidenceCatalogOmissionReason::CatalogByteLimit }));
        assert!(!String::from_utf8(request_bytes)
            .unwrap()
            .contains("synthetic-secret"));
    }

    #[test]
    fn step_identity_must_belong_to_the_source_run() {
        let (db, workflow_id) = fixture();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs (id, workflow_id, status, created_at)
                 VALUES ('other-run', ?1, 'completed', '2026-09-08T12:00:00Z')",
                [&workflow_id],
            )?;
            conn.execute(
                "INSERT INTO run_steps (id, run_id, node_id, status, created_at)
                 VALUES ('other-step', 'other-run', 'input', 'completed', '2026-09-08T12:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let mut source = input("remember this");
        source.step_id = Some("other-step".into());
        assert!(matches!(
            db.capture_run_evidence_sources(vec![source]),
            Err(DbError::Other(code)) if code == "evidence_step_run_mismatch"
        ));
        assert!(db
            .list_run_evidence_sources("run-evidence")
            .unwrap()
            .is_empty());
    }
}

#[cfg(test)]
mod retention {
    use super::*;
    use crate::db::{CreateMemoryInput, CreateWorkflowInput, MemoryScopeType, MemoryType};
    use chrono::TimeZone;
    use serde_json::json;

    struct RetentionFixture {
        db: Db,
        workflow_id: String,
        memory_id: String,
        source_id: String,
    }

    fn fixture(with_step: bool) -> RetentionFixture {
        let db = Db::open_in_memory().unwrap();
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Retention".into(),
                description: String::new(),
                working_directory: "/synthetic/retention".into(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO runs (id, workflow_id, status, created_at) VALUES ('run-r', ?1, 'completed', '2026-09-08T12:00:00Z')", [&workflow.id])?;
            if with_step {
                conn.execute("INSERT INTO run_steps (id, run_id, node_id, status, created_at) VALUES ('step-r', 'run-r', 'input', 'completed', '2026-09-08T12:00:00Z')", [])?;
            }
            conn.execute("INSERT INTO memory_reviews (run_id, workflow_id, status, provider, candidate_count, created_at) VALUES ('run-r', ?1, 'completed', 'claude_code', 1, '2026-09-08T12:00:00Z')", [&workflow.id])?;
            Ok(())
        }).unwrap();
        let source = db
            .capture_run_evidence_sources(vec![EvidenceSourceInput {
                run_id: "run-r".into(),
                step_id: with_step.then(|| "step-r".into()),
                node_id: Some("input".into()),
                origin: EvidenceOrigin::AuthoredTask,
                producer_identity: None,
                source_label: Some("user input".into()),
                observed_at: Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).single().unwrap(),
                text: "The user prefers concise release notes.".into(),
                omission_reason: None,
            }])
            .unwrap()
            .remove(0);
        let memory = db
            .create_memory(CreateMemoryInput {
                workflow_id: workflow.id.clone(),
                title: "Release notes".into(),
                body: "Use concise release notes.".into(),
                run_id: None,
                node_id: None,
                kind: None,
                scope_type: Some(MemoryScopeType::User),
                memory_type: Some(MemoryType::Preference),
                source: Some("review".into()),
                pinned: None,
                confidence: None,
                salience: None,
                status: None,
                supersedes_id: None,
                fact_key: None,
                observed_at: None,
                last_confirmed_at: None,
                valid_from: None,
                expires_at: None,
                id: Some("memory-r".into()),
            })
            .unwrap();
        let envelope = db.build_run_evidence_envelope("run-r").unwrap();
        let fragment = &envelope.fragments[0];
        let quote = "prefers concise";
        let start = fragment.text.find(quote).unwrap();
        let validated = validate_evidence_citations(
            &envelope,
            &[EvidenceCitation {
                source_id: fragment.source_id.clone(),
                fragment_id: fragment.fragment_id.clone(),
                fragment_digest: fragment.digest.clone(),
                normalization_version: fragment.normalization_version,
                quote: quote.into(),
                byte_start: start,
                byte_end: start + quote.len(),
            }],
            EvidenceSubject::UserStatement,
        )
        .unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO memory_candidates
                 (id, review_run_id, workflow_id, operation, scope_type, scope_key,
                  memory_type, title, body, confidence, rationale, content_hash,
                  curation_contract_version, evidence_state, evidence_subject, claim_digest,
                  status, created_at)
                 VALUES ('candidate-r','run-r',?1,'create','workflow',?1,'preference',
                         'Release notes','Use concise release notes.',0.8,'source rationale',
                         'hash',1,'verified','user_statement','digest','pending','2026-09-08T12:00:00Z')",
                [&workflow.id],
            )?;
            write_candidate_evidence_conn(conn, "candidate-r", &validated, "2026-09-08T12:00:00Z")?;
            copy_candidate_evidence_to_memory_conn(conn, "candidate-r", &memory.id, "2026-09-08T12:00:00Z")?;
            conn.execute("UPDATE memories SET provenance_state = 'evidence_verified' WHERE id = ?1", [&memory.id])?;
            Ok(())
        }).unwrap();
        RetentionFixture {
            db,
            workflow_id: workflow.id,
            memory_id: memory.id,
            source_id: source.source_id,
        }
    }

    fn assert_scrubbed(fixture: &RetentionFixture, candidate_remains: bool) {
        if candidate_remains {
            let candidate_scrub = fixture
                .db
                .with_conn(|conn| {
                    let scrubbed = conn.query_row(
                        "SELECT status, blocked_code, rationale, claim_digest, evidence_state
                 FROM memory_candidates WHERE id = 'candidate-r'",
                        [],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<String>>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, String>(4)?,
                            ))
                        },
                    )?;
                    Ok(scrubbed)
                })
                .unwrap();
            assert_eq!(candidate_scrub.0, "blocked");
            assert_eq!(candidate_scrub.1.as_deref(), Some("source_removed"));
            assert_eq!(candidate_scrub.2, "");
            assert_eq!(candidate_scrub.3, None);
            assert_eq!(candidate_scrub.4, "source_removed");
        }
        let canonical = fixture.db.list_memory_evidence(&fixture.memory_id).unwrap();
        assert_eq!(canonical.len(), 1);
        assert!(canonical[0].source_removed);
        assert_eq!(canonical[0].quote, None);
        assert_eq!(canonical[0].fragment_digest, None);
        let memory = fixture.db.get_memory(&fixture.memory_id).unwrap().unwrap();
        assert_eq!(memory.body, "Use concise release notes.");
        assert_eq!(memory.provenance_state, "source_removed");
    }

    #[test]
    fn direct_source_deletion_scrubs_all_copied_transcript_material() {
        let fixture = fixture(false);
        fixture
            .db
            .with_conn(|conn| {
                conn.execute(
                    "DELETE FROM run_evidence_sources WHERE id = ?1",
                    [&fixture.source_id],
                )?;
                Ok(())
            })
            .unwrap();
        assert_scrubbed(&fixture, true);
        fixture.db.delete_memory(&fixture.memory_id).unwrap();
        assert!(fixture
            .db
            .list_memory_evidence(&fixture.memory_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn direct_run_deletion_cascades_through_source_scrubbing() {
        let fixture = fixture(false);
        fixture
            .db
            .with_conn(|conn| {
                conn.execute("DELETE FROM runs WHERE id = 'run-r'", [])?;
                Ok(())
            })
            .unwrap();
        assert_scrubbed(&fixture, false);
    }

    #[test]
    fn direct_step_deletion_cascades_through_source_scrubbing() {
        let fixture = fixture(true);
        fixture
            .db
            .with_conn(|conn| {
                conn.execute("DELETE FROM run_steps WHERE id = 'step-r'", [])?;
                Ok(())
            })
            .unwrap();
        assert_scrubbed(&fixture, true);
    }

    #[test]
    fn direct_workflow_deletion_preserves_user_memory_and_scrubs_source() {
        let fixture = fixture(false);
        fixture
            .db
            .with_conn(|conn| {
                conn.execute(
                    "DELETE FROM workflows WHERE id = ?1",
                    [&fixture.workflow_id],
                )?;
                Ok(())
            })
            .unwrap();
        assert_scrubbed(&fixture, false);
        assert_eq!(
            fixture
                .db
                .get_memory(&fixture.memory_id)
                .unwrap()
                .unwrap()
                .workflow_id,
            None
        );
    }
}
