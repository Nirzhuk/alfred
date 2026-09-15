//! Versioned, provider-neutral contracts and scoped service for bounded agent
//! memory reads. Native-host integration and provider advertisement remain
//! separate release gates.

use super::artifact_chunks::{
    load_artifact_chunk_manifest_range_conn, trusted_output_artifact_path, ArtifactChunkManifest,
    ArtifactVerificationState, ARTIFACT_CHUNK_SIZE,
};
use super::memories::get_memory_conn;
use super::memory_validity::{current_memory_validity, ValidityState};
use super::{Db, Memory, MemoryContext, MemoryStatus};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, TryLockError};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

pub const LOOKUP_CONTRACT_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupLimits {
    pub request_bytes: usize,
    pub query_bytes: usize,
    pub query_terms: usize,
    pub query_term_chars: usize,
    pub time_bound_bytes: usize,
    pub reference_bytes: usize,
    pub cursor_bytes: usize,
    pub search_results: usize,
    pub title_bytes: usize,
    pub provenance_bytes: usize,
    pub excerpt_bytes: usize,
    pub search_response_bytes: usize,
    pub body_page_bytes: usize,
    pub get_response_bytes: usize,
    pub calls_per_step: usize,
    pub returned_bytes_per_step: usize,
    pub read_deadline_ms: u64,
}

pub const LOOKUP_LIMITS_V1: LookupLimits = LookupLimits {
    request_bytes: 4_096,
    query_bytes: 2_000,
    query_terms: 24,
    query_term_chars: 64,
    time_bound_bytes: 64,
    reference_bytes: 256,
    cursor_bytes: 512,
    search_results: 8,
    title_bytes: 256,
    provenance_bytes: 512,
    excerpt_bytes: 1_200,
    search_response_bytes: 6_000,
    body_page_bytes: 4_000,
    get_response_bytes: 6_000,
    calls_per_step: 8,
    returned_bytes_per_step: 16_000,
    read_deadline_ms: 5_000,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupToolName {
    #[serde(rename = "memory.search")]
    MemorySearch,
    #[serde(rename = "memory.get")]
    MemoryGet,
    #[serde(rename = "history.search")]
    HistorySearch,
    #[serde(rename = "history.get")]
    HistoryGet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemorySearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryGetRequest {
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistorySearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_filter: Option<LookupTimeFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryGetRequest {
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LookupTimeFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

pub trait LookupRequest: Sized + Serialize + DeserializeOwned {
    fn validate_fields(&self) -> Result<(), LookupContractError>;

    fn decode_json(encoded: &[u8]) -> Result<Self, LookupContractError> {
        validate_request_size(encoded.len())?;
        let request: Self =
            serde_json::from_slice(encoded).map_err(|_| LookupContractError::MalformedRequest)?;
        request.validate_fields()?;
        Ok(request)
    }

    fn validate(&self) -> Result<(), LookupContractError> {
        let encoded =
            serde_json::to_vec(self).map_err(|_| LookupContractError::MalformedRequest)?;
        validate_request_size(encoded.len())?;
        self.validate_fields()
    }
}

impl LookupRequest for MemorySearchRequest {
    fn validate_fields(&self) -> Result<(), LookupContractError> {
        validate_query(&self.query)?;
        validate_result_limit(self.limit)
    }
}

impl LookupRequest for HistorySearchRequest {
    fn validate_fields(&self) -> Result<(), LookupContractError> {
        validate_query(&self.query)?;
        validate_result_limit(self.limit)?;
        if let Some(filter) = &self.time_filter {
            filter.validate()?;
        }
        Ok(())
    }
}

impl LookupRequest for MemoryGetRequest {
    fn validate_fields(&self) -> Result<(), LookupContractError> {
        validate_reference(&self.reference)?;
        validate_cursor(self.cursor.as_deref())
    }
}

impl LookupRequest for HistoryGetRequest {
    fn validate_fields(&self) -> Result<(), LookupContractError> {
        validate_reference(&self.reference)?;
        validate_cursor(self.cursor.as_deref())
    }
}

impl LookupTimeFilter {
    pub fn validate(&self) -> Result<(), LookupContractError> {
        if self.from.is_none() && self.to.is_none() {
            return Err(LookupContractError::InvalidTimeFilter);
        }
        let from = self.from.as_deref().map(parse_time_bound).transpose()?;
        let to = self.to.as_deref().map(parse_time_bound).transpose()?;
        if from.zip(to).is_some_and(|(from, to)| from > to) {
            return Err(LookupContractError::InvalidTimeFilter);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupSourceKind {
    Memory,
    RunStep,
    Evidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupSourceState {
    Current,
    Historical,
    Incomplete,
    Unavailable,
    Deleted,
    Expired,
    Retracted,
    Replaced,
    Unlinked,
}

impl LookupSourceState {
    fn may_return_body(self) -> bool {
        matches!(self, Self::Current | Self::Historical | Self::Incomplete)
    }

    fn is_historical(self) -> bool {
        self != Self::Current
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LookupSearchHit {
    pub reference: String,
    pub source: LookupSourceKind,
    pub state: LookupSourceState,
    pub title: String,
    pub provenance: String,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LookupSearchResponse {
    pub contract_version: u16,
    pub tool: LookupToolName,
    pub results: Vec<LookupSearchHit>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LookupGetResponse {
    pub contract_version: u16,
    pub tool: LookupToolName,
    pub reference: String,
    pub source: LookupSourceKind,
    pub state: LookupSourceState,
    pub source_version: String,
    pub page: u32,
    pub provenance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl LookupSearchResponse {
    pub fn validate(&self) -> Result<usize, LookupContractError> {
        if self.contract_version != LOOKUP_CONTRACT_VERSION
            || !matches!(
                self.tool,
                LookupToolName::MemorySearch | LookupToolName::HistorySearch
            )
            || self.results.len() > LOOKUP_LIMITS_V1.search_results
        {
            return Err(LookupContractError::InvalidResponse);
        }
        for hit in &self.results {
            validate_reference(&hit.reference)?;
            validate_required_bounded_field(&hit.title, LOOKUP_LIMITS_V1.title_bytes, "title")?;
            validate_required_bounded_field(
                &hit.provenance,
                LOOKUP_LIMITS_V1.provenance_bytes,
                "provenance",
            )?;
            validate_bounded_field(&hit.excerpt, LOOKUP_LIMITS_V1.excerpt_bytes, "excerpt")?;
            if let Some(observed_at) = hit.observed_at.as_deref() {
                parse_time_bound(observed_at)?;
            }
            let valid_source_and_state = match self.tool {
                LookupToolName::MemorySearch => {
                    hit.source == LookupSourceKind::Memory
                        && hit.state == LookupSourceState::Current
                }
                LookupToolName::HistorySearch => {
                    hit.state.is_historical() && hit.observed_at.is_some()
                }
                _ => false,
            };
            let valid_excerpt = if hit.state.may_return_body() {
                !hit.excerpt.trim().is_empty()
            } else {
                hit.excerpt.is_empty() && !hit.truncated
            };
            if !valid_source_and_state || !valid_excerpt {
                return Err(LookupContractError::InvalidResponse);
            }
        }
        serialized_response_size(self, LOOKUP_LIMITS_V1.search_response_bytes)
    }
}

impl LookupGetResponse {
    pub fn validate(&self) -> Result<usize, LookupContractError> {
        if self.contract_version != LOOKUP_CONTRACT_VERSION
            || !matches!(
                self.tool,
                LookupToolName::MemoryGet | LookupToolName::HistoryGet
            )
            || self.source_version.trim().is_empty()
        {
            return Err(LookupContractError::InvalidResponse);
        }
        let valid_source_and_state = match self.tool {
            LookupToolName::MemoryGet => {
                self.source == LookupSourceKind::Memory
                    && self.state != LookupSourceState::Historical
            }
            LookupToolName::HistoryGet => self.state.is_historical(),
            _ => false,
        };
        if !valid_source_and_state {
            return Err(LookupContractError::InvalidResponse);
        }
        validate_reference(&self.reference)?;
        validate_required_bounded_field(
            &self.provenance,
            LOOKUP_LIMITS_V1.provenance_bytes,
            "provenance",
        )?;
        if let Some(body) = self.body.as_deref() {
            validate_bounded_field(body, LOOKUP_LIMITS_V1.body_page_bytes, "body")?;
        }
        validate_cursor(self.next_cursor.as_deref())?;

        if self.state.may_return_body()
            != self
                .body
                .as_deref()
                .is_some_and(|body| !body.is_empty())
        {
            return Err(LookupContractError::InvalidResponse);
        }
        if self.truncated != self.next_cursor.is_some() {
            return Err(LookupContractError::InvalidResponse);
        }
        if self.truncated && !self.state.may_return_body() {
            return Err(LookupContractError::InvalidResponse);
        }
        serialized_response_size(self, LOOKUP_LIMITS_V1.get_response_bytes)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LookupStepBudget {
    calls_used: usize,
    returned_bytes: usize,
}

impl LookupStepBudget {
    pub fn calls_used(&self) -> usize {
        self.calls_used
    }

    pub fn returned_bytes(&self) -> usize {
        self.returned_bytes
    }

    pub fn begin_call(&mut self) -> Result<(), LookupContractError> {
        if self.calls_used >= LOOKUP_LIMITS_V1.calls_per_step {
            return Err(LookupContractError::StepCallBudgetExhausted);
        }
        self.calls_used += 1;
        Ok(())
    }

    pub fn record_returned_bytes(&mut self, bytes: usize) -> Result<(), LookupContractError> {
        let total = self
            .returned_bytes
            .checked_add(bytes)
            .ok_or(LookupContractError::StepByteBudgetExhausted)?;
        if total > LOOKUP_LIMITS_V1.returned_bytes_per_step {
            return Err(LookupContractError::StepByteBudgetExhausted);
        }
        self.returned_bytes = total;
        Ok(())
    }

    /// Refund bytes charged for a result the host rejected before delivery.
    /// The call itself stays counted so rejected retries remain bounded.
    pub fn release_returned_bytes(&mut self, bytes: usize) {
        self.returned_bytes = self.returned_bytes.saturating_sub(bytes);
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LookupContractError {
    #[error("agent memory lookup request is malformed")]
    MalformedRequest,
    #[error("agent memory lookup request exceeds 4096 UTF-8 bytes")]
    RequestTooLarge,
    #[error("agent memory lookup query must not be empty")]
    EmptyQuery,
    #[error("agent memory lookup query exceeds 2000 UTF-8 bytes")]
    QueryTooLarge,
    #[error("agent memory lookup query exceeds 24 terms")]
    TooManyQueryTerms,
    #[error("agent memory lookup query term exceeds 64 Unicode characters")]
    QueryTermTooLong,
    #[error("agent memory lookup result count must be between 1 and 8")]
    InvalidResultLimit,
    #[error("agent memory lookup time filter is invalid")]
    InvalidTimeFilter,
    #[error("agent memory lookup reference is invalid")]
    InvalidReference,
    #[error("agent memory lookup cursor is invalid")]
    InvalidCursor,
    #[error("agent memory lookup response field {0} exceeds its UTF-8 byte limit")]
    ResponseFieldTooLarge(&'static str),
    #[error("agent memory lookup response schema is invalid")]
    InvalidResponse,
    #[error("agent memory lookup response exceeds its serialized byte limit")]
    ResponseTooLarge,
    #[error("agent memory lookup step call budget is exhausted")]
    StepCallBudgetExhausted,
    #[error("agent memory lookup step byte budget is exhausted")]
    StepByteBudgetExhausted,
}

fn validate_request_size(bytes: usize) -> Result<(), LookupContractError> {
    if bytes > LOOKUP_LIMITS_V1.request_bytes {
        Err(LookupContractError::RequestTooLarge)
    } else {
        Ok(())
    }
}

fn validate_query(query: &str) -> Result<(), LookupContractError> {
    if query.trim().is_empty() {
        return Err(LookupContractError::EmptyQuery);
    }
    if query.len() > LOOKUP_LIMITS_V1.query_bytes {
        return Err(LookupContractError::QueryTooLarge);
    }
    let terms = query.split_whitespace().collect::<Vec<_>>();
    if terms.len() > LOOKUP_LIMITS_V1.query_terms {
        return Err(LookupContractError::TooManyQueryTerms);
    }
    if terms
        .iter()
        .any(|term| term.chars().count() > LOOKUP_LIMITS_V1.query_term_chars)
    {
        return Err(LookupContractError::QueryTermTooLong);
    }
    Ok(())
}

fn validate_result_limit(limit: Option<usize>) -> Result<(), LookupContractError> {
    if limit.is_some_and(|limit| limit == 0 || limit > LOOKUP_LIMITS_V1.search_results) {
        Err(LookupContractError::InvalidResultLimit)
    } else {
        Ok(())
    }
}

fn validate_reference(reference: &str) -> Result<(), LookupContractError> {
    if reference.trim().is_empty() || reference.len() > LOOKUP_LIMITS_V1.reference_bytes {
        Err(LookupContractError::InvalidReference)
    } else {
        Ok(())
    }
}

fn validate_cursor(cursor: Option<&str>) -> Result<(), LookupContractError> {
    if cursor.is_some_and(|cursor| {
        cursor.trim().is_empty() || cursor.len() > LOOKUP_LIMITS_V1.cursor_bytes
    }) {
        Err(LookupContractError::InvalidCursor)
    } else {
        Ok(())
    }
}

fn parse_time_bound(value: &str) -> Result<DateTime<chrono::FixedOffset>, LookupContractError> {
    if value.is_empty() || value.len() > LOOKUP_LIMITS_V1.time_bound_bytes {
        return Err(LookupContractError::InvalidTimeFilter);
    }
    DateTime::parse_from_rfc3339(value).map_err(|_| LookupContractError::InvalidTimeFilter)
}

fn validate_bounded_field(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), LookupContractError> {
    if value.len() > maximum {
        Err(LookupContractError::ResponseFieldTooLarge(field))
    } else {
        Ok(())
    }
}

fn validate_required_bounded_field(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), LookupContractError> {
    if value.trim().is_empty() {
        Err(LookupContractError::InvalidResponse)
    } else {
        validate_bounded_field(value, maximum, field)
    }
}

fn serialized_response_size(
    response: &impl Serialize,
    maximum: usize,
) -> Result<usize, LookupContractError> {
    let bytes = serde_json::to_vec(response).map_err(|_| LookupContractError::InvalidResponse)?;
    if bytes.len() > maximum {
        Err(LookupContractError::ResponseTooLarge)
    } else {
        Ok(bytes.len())
    }
}

/// Returns the longest prefix within `max_bytes` without splitting a UTF-8
/// code point. This is the byte accounting rule used by Plan 048 composition.
pub(crate) fn utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// Canonical memory revision contract required by the lookup service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupSchemaState {
    SemanticRevisionV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupTraceClassification {
    RetrievedData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupTraceSource {
    pub reference: String,
    pub source: LookupSourceKind,
    pub source_id: String,
    pub version: String,
    pub state: LookupSourceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupTrace {
    pub tool: LookupToolName,
    pub sources: Vec<LookupTraceSource>,
    pub returned_bytes: usize,
    pub classification: LookupTraceClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupAbort {
    Cancelled,
    TimedOut,
}

pub trait LookupCallControl: Send + Sync {
    fn checkpoint(&self) -> Result<(), LookupAbort>;
}

#[derive(Debug, Default)]
pub struct UninterruptedLookup;

impl LookupCallControl for UninterruptedLookup {
    fn checkpoint(&self) -> Result<(), LookupAbort> {
        Ok(())
    }
}

pub trait LookupClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default)]
struct SystemLookupClock;

impl LookupClock for SystemLookupClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug)]
struct RawLookupRead {
    bytes: Vec<u8>,
    total_bytes: usize,
}

trait LookupFileReader: Send + Sync {
    fn read_page(
        &self,
        path: &Path,
        offset: usize,
        maximum: usize,
    ) -> Result<BodyPage, LookupServiceError>;

    /// Read a bounded raw byte range for canonical chunk verification.
    ///
    /// The default keeps synthetic readers source-compatible. Production
    /// readers override it so the service can verify exact chunk bytes rather
    /// than trusting a decoded preview.
    fn read_raw(
        &self,
        path: &Path,
        offset: usize,
        maximum: usize,
    ) -> Result<RawLookupRead, LookupServiceError> {
        let page = self.read_page(path, offset, maximum)?;
        let total_bytes = offset
            .checked_add(page.body.len())
            .ok_or(LookupServiceError::StorageUnavailable)?;
        Ok(RawLookupRead {
            bytes: page.body.into_bytes(),
            total_bytes,
        })
    }

    #[cfg(test)]
    fn after_publish(&self) {}
}

#[derive(Debug, Default)]
struct RegularLookupFileReader;

impl LookupFileReader for RegularLookupFileReader {

    fn read_page(
        &self,
        path: &Path,
        offset: usize,
        maximum: usize,
    ) -> Result<BodyPage, LookupServiceError> {
        page_from_regular_file(path, offset, maximum)
    }

    fn read_raw(
        &self,
        path: &Path,
        offset: usize,
        maximum: usize,
    ) -> Result<RawLookupRead, LookupServiceError> {
        let metadata = regular_file_metadata(path)?;
        let total_bytes = usize::try_from(metadata.len())
            .map_err(|_| LookupServiceError::StorageUnavailable)?;
        if offset > total_bytes {
            return Err(LookupContractError::InvalidCursor.into());
        }
        let read_length = total_bytes
            .saturating_sub(offset)
            .min(maximum);
        let mut file = File::open(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                LookupServiceError::ArtifactUnavailable
            } else {
                LookupServiceError::StorageUnavailable
            }
        })?;
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| LookupServiceError::StorageUnavailable)?;
        let mut bytes = Vec::with_capacity(read_length);
        file.take(read_length as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| LookupServiceError::StorageUnavailable)?;
        Ok(RawLookupRead { bytes, total_bytes })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LookupServiceError {
    #[error(transparent)]
    Contract(#[from] LookupContractError),
    #[error("agent memory lookup scope is invalid")]
    InvalidScope,
    #[error("agent memory lookup storage is unavailable")]
    StorageUnavailable,
    #[error("agent memory lookup artifact is unavailable")]
    ArtifactUnavailable,
    #[error("agent memory lookup requires Plan 050 schema integration")]
    Plan050IntegrationRequired,
    #[error("agent memory lookup was cancelled")]
    Cancelled,
    #[error("agent memory lookup timed out")]
    TimedOut,
}

pub struct AgentMemoryLookupService {
    db: Arc<Db>,
    output_artifact_root: PathBuf,
    clock: Arc<dyn LookupClock>,
    read_deadline: Duration,
    file_worker_active: Arc<AtomicBool>,
    file_reader: Arc<dyn LookupFileReader>,
}

impl AgentMemoryLookupService {
    pub fn new(db: Arc<Db>, output_artifact_root: impl Into<PathBuf>) -> Self {
        Self {
            db,
            output_artifact_root: output_artifact_root.into(),
            clock: Arc::new(SystemLookupClock),
            read_deadline: Duration::from_millis(LOOKUP_LIMITS_V1.read_deadline_ms),
            file_worker_active: Arc::new(AtomicBool::new(false)),
            file_reader: Arc::new(RegularLookupFileReader),
        }
    }

    #[cfg(test)]
    fn with_test_runtime(
        db: Arc<Db>,
        output_artifact_root: impl Into<PathBuf>,
        clock: Arc<dyn LookupClock>,
        read_deadline: Duration,
        file_reader: Arc<dyn LookupFileReader>,
    ) -> Self {
        Self {
            db,
            output_artifact_root: output_artifact_root.into(),
            clock,
            read_deadline,
            file_worker_active: Arc::new(AtomicBool::new(false)),
            file_reader,
        }
    }

    pub fn start_session(
        &self,
        run_id: &str,
        node_id: &str,
        captured_working_directory: Option<&Path>,
    ) -> Result<AgentMemoryLookupSession, LookupServiceError> {
        self.start_session_with_clock(
            run_id,
            node_id,
            captured_working_directory,
            self.clock.clone(),
        )
    }

    pub(crate) fn start_session_at(
        &self,
        run_id: &str,
        node_id: &str,
        captured_working_directory: Option<&Path>,
        now: DateTime<Utc>,
    ) -> Result<AgentMemoryLookupSession, LookupServiceError> {
        self.start_session_with_clock(
            run_id,
            node_id,
            captured_working_directory,
            Arc::new(FixedLookupClock(now)),
        )
    }

    fn start_session_with_clock(
        &self,
        run_id: &str,
        node_id: &str,
        captured_working_directory: Option<&Path>,
        clock: Arc<dyn LookupClock>,
    ) -> Result<AgentMemoryLookupSession, LookupServiceError> {
        if run_id.trim().is_empty() || node_id.trim().is_empty() {
            return Err(LookupServiceError::InvalidScope);
        }
        let workflow_id = storage(self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT workflow_id FROM runs WHERE id = ?1",
                [run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Into::into)
        }))?
        .ok_or(LookupServiceError::InvalidScope)?;
        let context = storage(self.db.memory_context(&workflow_id))?;
        if let Some(expected) = context.working_directory.as_deref() {
            let captured = captured_working_directory
                .and_then(normalize_absolute_path)
                .ok_or(LookupServiceError::InvalidScope)?;
            if captured != PathBuf::from(expected) {
                return Err(LookupServiceError::InvalidScope);
            }
        }
        let schema_state = inspect_lookup_schema(&self.db)?;
        let scope_fingerprint = digest_fields(&[
            "lookup_scope_v1",
            run_id,
            node_id,
            &workflow_id,
            context.working_directory.as_deref().unwrap_or(""),
        ]);
        Ok(AgentMemoryLookupSession {
            db: self.db.clone(),
            output_artifact_root: self.output_artifact_root.clone(),
            run_id: run_id.to_owned(),
            node_id: node_id.to_owned(),
            context,
            scope_fingerprint,
            schema_state,
            clock,
            read_deadline: self.read_deadline,
            file_worker_active: self.file_worker_active.clone(),
            file_reader: self.file_reader.clone(),
            tokens: Mutex::new(TokenRegistry::default()),
            budget: Mutex::new(LookupStepBudget::default()),
            traces: Mutex::new(Vec::new()),
        })
    }
}

struct FixedLookupClock(DateTime<Utc>);

impl LookupClock for FixedLookupClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

pub struct AgentMemoryLookupSession {
    db: Arc<Db>,
    output_artifact_root: PathBuf,
    run_id: String,
    node_id: String,
    context: MemoryContext,
    scope_fingerprint: String,
    schema_state: LookupSchemaState,
    clock: Arc<dyn LookupClock>,
    read_deadline: Duration,
    file_worker_active: Arc<AtomicBool>,
    file_reader: Arc<dyn LookupFileReader>,
    tokens: Mutex<TokenRegistry>,
    budget: Mutex<LookupStepBudget>,
    traces: Mutex<Vec<LookupTrace>>,
}

impl AgentMemoryLookupSession {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn workflow_id(&self) -> &str {
        &self.context.workflow_id
    }

    pub fn schema_state(&self) -> LookupSchemaState {
        self.schema_state
    }

    pub fn budget(&self) -> Result<LookupStepBudget, LookupServiceError> {
        Ok(*locked(&self.budget)?)
    }

    pub fn traces(&self) -> Result<Vec<LookupTrace>, LookupServiceError> {
        Ok(locked(&self.traces)?.clone())
    }

    pub fn execute_json(
        &self,
        tool: LookupToolName,
        encoded_request: &[u8],
        control: &dyn LookupCallControl,
    ) -> Result<String, LookupServiceError> {
        self.execute_json_with_trace(tool, encoded_request, control)
            .map(|(output, _)| output)
    }

    pub fn release_returned_bytes(&self, bytes: usize) -> Result<(), LookupServiceError> {
        locked(&self.budget)?.release_returned_bytes(bytes);
        Ok(())
    }

    pub fn execute_json_with_trace(
        &self,
        tool: LookupToolName,
        encoded_request: &[u8],
        control: &dyn LookupCallControl,
    ) -> Result<(String, LookupTrace), LookupServiceError> {
        let call = LookupCall::begin(self, control)?;
        self.ensure_scope_active(&call)?;
        let response = match tool {
            LookupToolName::MemorySearch => LookupResponse::Search(
                self.search_memories(MemorySearchRequest::decode_json(encoded_request)?, &call)?,
            ),
            LookupToolName::MemoryGet => LookupResponse::Get(
                self.get_memory(MemoryGetRequest::decode_json(encoded_request)?, &call)?,
            ),
            LookupToolName::HistorySearch => LookupResponse::Search(
                self.search_history(HistorySearchRequest::decode_json(encoded_request)?, &call)?,
            ),
            LookupToolName::HistoryGet => LookupResponse::Get(
                self.get_history(HistoryGetRequest::decode_json(encoded_request)?, &call)?,
            ),
        };
        call.checkpoint()?;
        let (bytes, sources) = match &response {
            LookupResponse::Search(response) => {
                let bytes = response.validate()?;
                let sources = response
                    .results
                    .iter()
                    .map(|hit| {
                        self.trace_source(&hit.reference, hit.source, hit.state, None)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (bytes, sources)
            }
            LookupResponse::Get(response) => {
                let bytes = response.validate()?;
                let source = self.trace_source(
                    &response.reference,
                    response.source,
                    response.state,
                    Some(response.page),
                )?;
                (bytes, vec![source])
            }
        };
        let trace = LookupTrace {
            tool,
            sources,
            returned_bytes: bytes,
            classification: LookupTraceClassification::RetrievedData,
        };
        let output =
            serde_json::to_string(&response).map_err(|_| LookupServiceError::StorageUnavailable)?;
        if output.len() != bytes {
            return Err(LookupServiceError::StorageUnavailable);
        }
        try_locked(&self.budget)?.record_returned_bytes(bytes)?;
        try_locked(&self.traces)?.push(trace.clone());
        Ok((output, trace))
    }

    fn search_memories(
        &self,
        request: MemorySearchRequest,
        call: &LookupCall<'_>,
    ) -> Result<LookupSearchResponse, LookupServiceError> {
        let requested = request.limit.unwrap_or(LOOKUP_LIMITS_V1.search_results);
        let query = lookup_fts_query(&request.query);
        let now = call.now.to_rfc3339();
        let candidates = lookup_with_conn(&self.db, call, |conn| {
            let mut statement = conn.prepare(
                "SELECT memory_fts.memory_id,
                        snippet(memory_fts, -1, '', '', '…', 32),
                        bm25(memory_fts, 0.0, 0.0, 1.0, 1.0)
                   FROM memory_fts
                   JOIN memories m ON m.id = memory_fts.memory_id
                  WHERE memory_fts MATCH ?1
                    AND m.status = 'active'
                    AND (m.valid_from IS NULL OR datetime(m.valid_from) <= datetime(?4))
                    AND (m.expires_at IS NULL OR datetime(m.expires_at) > datetime(?4))
                    AND (
                      (m.scope_type = 'user' AND m.scope_key = 'local-user')
                      OR (m.scope_type = 'workspace' AND m.scope_key = ?2)
                      OR (m.scope_type = 'workflow' AND (
                        m.scope_key = ?3 OR EXISTS (
                          SELECT 1 FROM memory_links l
                           WHERE l.workflow_id = ?3 AND l.memory_id = m.id
                        )
                      ))
                    )
                  ORDER BY bm25(memory_fts), m.updated_at DESC, m.id
                  LIMIT ?5",
            )?;
            let rows = statement
                .query_map(
                    params![
                        query,
                        self.context.working_directory.as_deref(),
                        self.context.workflow_id,
                        now,
                        (requested + 1) as i64
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        call.checkpoint()?;

        let more_candidates = candidates.len() > requested;
        let mut results = Vec::with_capacity(requested);
        for (memory_id, indexed_excerpt) in candidates.into_iter().take(requested) {
            call.checkpoint()?;
            let Some(memory) = self.load_memory(&memory_id, call)? else {
                continue;
            };
            if !self.memory_visible(&memory.id, call)?
                || self.memory_state(&memory, call)? != LookupSourceState::Current
            {
                continue;
            }
            let (rendered, excerpt_truncated) = if memory.artifact_path.is_some() {
                (memory.body.clone(), true)
            } else {
                let excerpt = self
                    .db
                    .bounded_memory_excerpt(&memory, LOOKUP_LIMITS_V1.excerpt_bytes);
                (excerpt.rendered, excerpt.truncated || excerpt.incomplete)
            };
            let rendered_excerpt = if rendered.trim().is_empty() {
                bounded_required(&indexed_excerpt, LOOKUP_LIMITS_V1.excerpt_bytes, "Memory")
            } else {
                bounded_required(&rendered, LOOKUP_LIMITS_V1.excerpt_bytes, "Memory")
            };
            let version = memory_version(&memory);
            let reference = self.register_reference(BoundSource::Memory {
                memory_id: memory.id.clone(),
                version,
            })?;
            results.push(LookupSearchHit {
                reference,
                source: LookupSourceKind::Memory,
                state: LookupSourceState::Current,
                title: bounded_required(&memory.title, LOOKUP_LIMITS_V1.title_bytes, "Memory"),
                provenance: bounded_required(
                    &memory_provenance(&memory, false),
                    LOOKUP_LIMITS_V1.provenance_bytes,
                    "memory",
                ),
                excerpt: rendered_excerpt,
                observed_at: Some(
                    memory
                        .observed_at
                        .clone()
                        .unwrap_or_else(|| memory.updated_at.clone()),
                ),
                truncated: excerpt_truncated,
            });
        }
        self.fit_search_response(LookupToolName::MemorySearch, results, more_candidates)
    }

    fn search_history(
        &self,
        request: HistorySearchRequest,
        call: &LookupCall<'_>,
    ) -> Result<LookupSearchResponse, LookupServiceError> {
        let requested = request.limit.unwrap_or(LOOKUP_LIMITS_V1.search_results);
        let query = lookup_fts_query(&request.query);
        let from = request
            .time_filter
            .as_ref()
            .and_then(|filter| filter.from.as_deref());
        let to = request
            .time_filter
            .as_ref()
            .and_then(|filter| filter.to.as_deref());
        let mut candidates = self.history_step_candidates(&query, from, to, requested + 1, call)?;
        call.checkpoint()?;
        candidates.extend(self.history_memory_candidates(&query, from, to, requested + 1, call)?);
        call.checkpoint()?;
        candidates.extend(self.history_evidence_candidates(
            &request.query,
            from,
            to,
            requested + 1,
            call,
        )?);
        call.checkpoint()?;
        candidates.sort_by(|left, right| {
            left.rank
                .total_cmp(&right.rank)
                .then_with(|| right.observed_at.cmp(&left.observed_at))
                .then_with(|| left.source_id.cmp(&right.source_id))
        });
        let more_candidates = candidates.len() > requested;
        candidates.truncate(requested);
        let mut results = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let reference = self.register_reference(candidate.binding)?;
            results.push(LookupSearchHit {
                reference,
                source: candidate.source,
                state: candidate.state,
                title: bounded_required(&candidate.title, LOOKUP_LIMITS_V1.title_bytes, "History"),
                provenance: bounded_required(
                    &candidate.provenance,
                    LOOKUP_LIMITS_V1.provenance_bytes,
                    "history",
                ),
                excerpt: if candidate.state.may_return_body() {
                    bounded_required(
                        &candidate.excerpt,
                        LOOKUP_LIMITS_V1.excerpt_bytes,
                        "Historical record",
                    )
                } else {
                    String::new()
                },
                observed_at: Some(candidate.observed_at),
                truncated: candidate.truncated && candidate.state.may_return_body(),
            });
        }
        self.fit_search_response(LookupToolName::HistorySearch, results, more_candidates)
    }

    fn get_memory(
        &self,
        request: MemoryGetRequest,
        call: &LookupCall<'_>,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        let binding = self.resolve_reference(&request.reference)?;
        let BoundSource::Memory { memory_id, version } = binding else {
            return Err(LookupContractError::InvalidReference.into());
        };
        self.read_memory_page(
            LookupToolName::MemoryGet,
            request.reference,
            request.cursor,
            memory_id,
            version,
            call,
        )
    }

    fn get_history(
        &self,
        request: HistoryGetRequest,
        call: &LookupCall<'_>,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        let binding = self.resolve_reference(&request.reference)?;
        match binding {
            BoundSource::Memory { memory_id, version } => self.read_memory_page(
                LookupToolName::HistoryGet,
                request.reference,
                request.cursor,
                memory_id,
                version,
                call,
            ),
            BoundSource::RunStep { step_id, version } => {
                self.read_step_page(request.reference, request.cursor, step_id, version, call)
            }
            BoundSource::Evidence {
                evidence_id,
                version,
            } => self.read_evidence_page(
                request.reference,
                request.cursor,
                evidence_id,
                version,
                call,
            ),
        }
    }

    fn fit_search_response(
        &self,
        tool: LookupToolName,
        mut results: Vec<LookupSearchHit>,
        mut truncated: bool,
    ) -> Result<LookupSearchResponse, LookupServiceError> {
        loop {
            let response = LookupSearchResponse {
                contract_version: LOOKUP_CONTRACT_VERSION,
                tool,
                results: results.clone(),
                truncated,
            };
            match response.validate() {
                Ok(_) => return Ok(response),
                Err(LookupContractError::ResponseTooLarge) if !results.is_empty() => {
                    results.pop();
                    truncated = true;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn load_memory(
        &self,
        memory_id: &str,
        call: &LookupCall<'_>,
    ) -> Result<Option<Memory>, LookupServiceError> {
        lookup_with_conn(&self.db, call, |conn| get_memory_conn(conn, memory_id))
    }

    fn memory_visible(
        &self,
        memory_id: &str,
        call: &LookupCall<'_>,
    ) -> Result<bool, LookupServiceError> {
        lookup_with_conn(&self.db, call, |conn| {
            conn.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM memories m
                    WHERE m.id = ?1 AND (
                      (m.scope_type = 'user' AND m.scope_key = 'local-user')
                      OR (m.scope_type = 'workspace' AND m.scope_key = ?2)
                      OR (m.scope_type = 'workflow' AND (
                        m.scope_key = ?3 OR EXISTS (
                          SELECT 1 FROM memory_links l
                           WHERE l.workflow_id = ?3 AND l.memory_id = m.id
                        )
                      ))
                    )
                 )",
                params![
                    memory_id,
                    self.context.working_directory.as_deref(),
                    self.context.workflow_id
                ],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
    }

    fn ensure_scope_active(&self, call: &LookupCall<'_>) -> Result<(), LookupServiceError> {
        let active = lookup_with_conn(&self.db, call, |conn| {
            conn.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM runs r
                   JOIN workflows w ON w.id = r.workflow_id
                  WHERE r.id = ?1 AND r.workflow_id = ?2
                    AND w.memory_lookup_enabled = 1
                 )",
                params![self.run_id, self.context.workflow_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(Into::into)
        })?;
        if active {
            Ok(())
        } else {
            Err(LookupServiceError::InvalidScope)
        }
    }

    fn memory_state(
        &self,
        memory: &Memory,
        call: &LookupCall<'_>,
    ) -> Result<LookupSourceState, LookupServiceError> {
        let active = memory.status == MemoryStatus::Active;
        let validity = current_memory_validity(
            active,
            memory.valid_from.as_deref(),
            memory.expires_at.as_deref(),
            call.now,
        )
        .map_err(|_| LookupServiceError::StorageUnavailable)?;
        Ok(match (memory.status, validity) {
            (MemoryStatus::Superseded, _) => LookupSourceState::Replaced,
            (MemoryStatus::Retracted, _) => LookupSourceState::Retracted,
            (_, ValidityState::Expired) => LookupSourceState::Expired,
            (_, ValidityState::NotStarted | ValidityState::Inactive) => {
                LookupSourceState::Unavailable
            }
            (_, ValidityState::Current) => LookupSourceState::Current,
        })
    }

    fn register_reference(&self, source: BoundSource) -> Result<String, LookupServiceError> {
        let mut tokens = try_locked(&self.tokens)?;
        loop {
            let token = format!("mlr1_{}", Uuid::new_v4().simple());
            if !tokens.references.contains_key(&token) {
                tokens.references.insert(
                    token.clone(),
                    ReferenceBinding {
                        scope_fingerprint: self.scope_fingerprint.clone(),
                        source,
                    },
                );
                return Ok(token);
            }
        }
    }

    fn resolve_reference(&self, reference: &str) -> Result<BoundSource, LookupServiceError> {
        let tokens = try_locked(&self.tokens)?;
        let binding = tokens
            .references
            .get(reference)
            .ok_or(LookupContractError::InvalidReference)?;
        if binding.scope_fingerprint != self.scope_fingerprint {
            return Err(LookupContractError::InvalidReference.into());
        }
        Ok(binding.source.clone())
    }
    fn trace_source(
        &self,
        reference: &str,
        source: LookupSourceKind,
        state: LookupSourceState,
        page: Option<u32>,
    ) -> Result<LookupTraceSource, LookupServiceError> {
        let binding = self.resolve_reference(reference)?;
        let (source_id, version) = binding.identity();
        Ok(LookupTraceSource {
            reference: reference.to_owned(),
            source,
            source_id: source_id.to_owned(),
            version: version.to_owned(),
            state,
            page,
        })
    }

    fn page_position(
        &self,
        reference: &str,
        cursor: Option<&str>,
        version: &str,
    ) -> Result<(usize, u32), LookupServiceError> {
        let Some(cursor) = cursor else {
            return Ok((0, 0));
        };
        let tokens = try_locked(&self.tokens)?;
        let binding = tokens
            .cursors
            .get(cursor)
            .ok_or(LookupContractError::InvalidCursor)?;
        if binding.scope_fingerprint != self.scope_fingerprint
            || binding.reference != reference
            || binding.source_version != version
        {
            return Err(LookupContractError::InvalidCursor.into());
        }
        Ok((binding.offset, binding.page))
    }

    fn register_cursor(
        &self,
        reference: &str,
        source_version: &str,
        offset: usize,
        page: u32,
    ) -> Result<String, LookupServiceError> {
        let mut tokens = try_locked(&self.tokens)?;
        loop {
            let token = format!("mlc1_{}", Uuid::new_v4().simple());
            if !tokens.cursors.contains_key(&token) {
                tokens.cursors.insert(
                    token.clone(),
                    CursorBinding {
                        scope_fingerprint: self.scope_fingerprint.clone(),
                        reference: reference.to_owned(),
                        source_version: source_version.to_owned(),
                        offset,
                        page,
                    },
                );
                return Ok(token);
            }
        }
    }
}

impl AgentMemoryLookupSession {
    fn history_step_candidates(
        &self,
        query: &str,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
        call: &LookupCall<'_>,
    ) -> Result<Vec<HistoryCandidate>, LookupServiceError> {
        if !lookup_table_exists(&self.db, call, "output_receipts")? {
            return Ok(Vec::new());
        }
        let output_query = format!("output_text : ({query})");
        let rows = lookup_with_conn(&self.db, call, |conn| {
            let mut statement = conn.prepare(
                "SELECT run_step_fts.step_id,
                        snippet(run_step_fts, 5, '', '', '…', 32),
                        bm25(run_step_fts, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0)
                   FROM run_step_fts
                   JOIN run_steps rs ON rs.id = run_step_fts.step_id
                   JOIN output_receipts o
                     ON o.run_id = rs.run_id AND o.step_id = rs.id
                  WHERE run_step_fts MATCH ?1
                    AND run_step_fts.workflow_id = ?2
                    AND o.state = 'saved'
                    AND (?3 IS NULL OR datetime(rs.created_at) >= datetime(?3))
                    AND (?4 IS NULL OR datetime(rs.created_at) <= datetime(?4))
                  ORDER BY bm25(run_step_fts), rs.created_at DESC, rs.id
                  LIMIT ?5",
            )?;
            let rows = statement
                .query_map(
                    params![
                        output_query,
                        self.context.workflow_id,
                        from,
                        to,
                        limit as i64
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, f64>(2)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        let mut candidates = Vec::with_capacity(rows.len());
        for (step_id, excerpt, rank) in rows {
            let Some(snapshot) = self.load_step_snapshot(&step_id, call)? else {
                continue;
            };
            let state = snapshot.state();
            let version = self.step_version(&snapshot, call)?;
            candidates.push(HistoryCandidate {
                source_id: step_id.clone(),
                source: LookupSourceKind::RunStep,
                state,
                title: snapshot.title.clone(),
                provenance: format!(
                    "saved workflow output • receipt verified • status: {}",
                    snapshot.status
                ),
                excerpt,
                observed_at: snapshot.observed_at.clone(),
                truncated: true,
                rank,
                binding: BoundSource::RunStep { step_id, version },
            });
        }
        Ok(candidates)
    }

    fn history_memory_candidates(
        &self,
        query: &str,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
        call: &LookupCall<'_>,
    ) -> Result<Vec<HistoryCandidate>, LookupServiceError> {
        let now = call.now.to_rfc3339();
        let rows = lookup_with_conn(&self.db, call, |conn| {
            let mut statement = conn.prepare(
                "SELECT memory_fts.memory_id,
                        snippet(memory_fts, -1, '', '', '…', 32),
                        bm25(memory_fts, 0.0, 0.0, 1.0, 1.0)
                   FROM memory_fts
                   JOIN memories m ON m.id = memory_fts.memory_id
                  WHERE memory_fts MATCH ?1
                    AND (?2 IS NULL OR datetime(COALESCE(m.observed_at, m.updated_at)) >= datetime(?2))
                    AND (?3 IS NULL OR datetime(COALESCE(m.observed_at, m.updated_at)) <= datetime(?3))
                    AND NOT (
                      m.status = 'active'
                      AND (m.valid_from IS NULL OR datetime(m.valid_from) <= datetime(?6))
                      AND (m.expires_at IS NULL OR datetime(m.expires_at) > datetime(?6))
                    )
                    AND (
                      (m.scope_type = 'user' AND m.scope_key = 'local-user')
                      OR (m.scope_type = 'workspace' AND m.scope_key = ?4)
                      OR (m.scope_type = 'workflow' AND (
                        m.scope_key = ?5 OR EXISTS (
                          SELECT 1 FROM memory_links l
                           WHERE l.workflow_id = ?5 AND l.memory_id = m.id
                        )
                      ))
                    )
                  ORDER BY bm25(memory_fts), m.updated_at DESC, m.id
                  LIMIT ?7",
            )?;
            let rows = statement
                .query_map(
                    params![
                        query,
                        from,
                        to,
                        self.context.working_directory.as_deref(),
                        self.context.workflow_id,
                        now,
                        limit as i64
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, f64>(2)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        let mut candidates = Vec::with_capacity(rows.len());
        for (memory_id, excerpt, rank) in rows {
            let Some(memory) = self.load_memory(&memory_id, call)? else {
                continue;
            };
            if !self.memory_visible(&memory.id, call)? {
                continue;
            }
            let state = self.memory_state(&memory, call)?;
            if state == LookupSourceState::Current {
                continue;
            }
            candidates.push(HistoryCandidate {
                source_id: memory_id.clone(),
                source: LookupSourceKind::Memory,
                state,
                title: memory.title.clone(),
                provenance: memory_provenance(&memory, true),
                excerpt,
                observed_at: memory
                    .observed_at
                    .clone()
                    .unwrap_or_else(|| memory.updated_at.clone()),
                truncated: true,
                rank,
                binding: BoundSource::Memory {
                    memory_id,
                    version: memory_version(&memory),
                },
            });
        }
        Ok(candidates)
    }

    fn history_evidence_candidates(
        &self,
        query: &str,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
        call: &LookupCall<'_>,
    ) -> Result<Vec<HistoryCandidate>, LookupServiceError> {
        let terms = query.split_whitespace().collect::<Vec<_>>();
        if terms.is_empty() {
            return Err(LookupContractError::EmptyQuery.into());
        }
        lookup_with_conn(&self.db, call, |conn| {
            let term_filters = terms
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    format!(
                        "instr(lower(es.normalized_text), lower(?{})) > 0",
                        index + 2
                    )
                })
                .collect::<Vec<_>>()
                .join(" AND ");
            let from_index = terms.len() + 2;
            let to_index = terms.len() + 3;
            let limit_index = terms.len() + 4;
            let sql = format!(
                "SELECT es.id, COALESCE(NULLIF(es.source_label, ''), 'Historical evidence'),
                        es.origin, es.observed_at, es.capture_state, es.content_digest,
                        substr(es.normalized_text, 1, 600)
                   FROM run_evidence_sources es
                   JOIN runs r ON r.id = es.run_id
                  WHERE r.workflow_id = ?1
                    AND {term_filters}
                    AND (?{from_index} IS NULL OR datetime(es.observed_at) >= datetime(?{from_index}))
                    AND (?{to_index} IS NULL OR datetime(es.observed_at) <= datetime(?{to_index}))
                  ORDER BY es.observed_at DESC, es.id
                  LIMIT ?{limit_index}"
            );
            let mut statement = conn.prepare(&sql)?;
            let mut values = Vec::with_capacity(terms.len() + 4);
            values.push(rusqlite::types::Value::Text(
                self.context.workflow_id.clone(),
            ));
            values.extend(
                terms
                    .iter()
                    .map(|term| rusqlite::types::Value::Text((*term).to_owned())),
            );
            values.push(
                from.map(|value| rusqlite::types::Value::Text(value.to_owned()))
                    .unwrap_or(rusqlite::types::Value::Null),
            );
            values.push(
                to.map(|value| rusqlite::types::Value::Text(value.to_owned()))
                    .unwrap_or(rusqlite::types::Value::Null),
            );
            values.push(rusqlite::types::Value::Integer(limit as i64));
            let rows = statement
                .query_map(rusqlite::params_from_iter(values), |row| {
                    let source_id = row.get::<_, String>(0)?;
                    let title = row.get::<_, String>(1)?;
                    let origin = row.get::<_, String>(2)?;
                    let observed_at = row.get::<_, String>(3)?;
                    let capture_state = row.get::<_, String>(4)?;
                    let digest = row.get::<_, String>(5)?;
                    let excerpt = row.get::<_, String>(6)?;
                    let state = evidence_state(&capture_state);
                    Ok(HistoryCandidate {
                        source_id: source_id.clone(),
                        source: LookupSourceKind::Evidence,
                        state,
                        title,
                        provenance: format!("historical evidence • origin: {origin}"),
                        excerpt,
                        observed_at,
                        truncated: capture_state == "truncated",
                        rank: 0.0,
                        binding: BoundSource::Evidence {
                            evidence_id: source_id,
                            version: evidence_version(&digest, &capture_state),
                        },
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    fn read_memory_page(
        &self,
        tool: LookupToolName,
        reference: String,
        cursor: Option<String>,
        memory_id: String,
        expected_version: String,
        call: &LookupCall<'_>,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        call.checkpoint()?;
        let Some(memory) = self.load_memory(&memory_id, call)? else {
            return self.empty_get(
                tool,
                reference,
                LookupSourceKind::Memory,
                LookupSourceState::Deleted,
                expected_version,
                "memory deleted",
            );
        };
        if !self.memory_visible(&memory_id, call)? {
            return self.empty_get(
                tool,
                reference,
                LookupSourceKind::Memory,
                LookupSourceState::Unlinked,
                memory_version(&memory),
                "memory no longer linked to this workflow",
            );
        }
        let current_version = memory_version(&memory);
        let provenance = memory_provenance(&memory, tool == LookupToolName::HistoryGet);
        let mut state = self.memory_state(&memory, call)?;
        if (current_version != expected_version && state == LookupSourceState::Current)
            || (tool == LookupToolName::HistoryGet && state == LookupSourceState::Current)
        {
            state = LookupSourceState::Replaced;
        }
        let artifact = memory.artifact_path.is_some();
        if !state.may_return_body() {
            return self.empty_get(
                tool,
                reference,
                LookupSourceKind::Memory,
                state,
                current_version,
                &provenance,
            );
        }
        let (offset, page) = self.page_position(&reference, cursor.as_deref(), &current_version)?;
        let body_page = if artifact {
            let Some(content_digest) = memory.artifact_content_digest.as_deref() else {
                return self.empty_get(
                    tool,
                    reference,
                    LookupSourceKind::Memory,
                    LookupSourceState::Unavailable,
                    current_version,
                    "canonical artifact identity unavailable",
                );
            };
            let Some(path) = self.canonical_artifact_path(&memory) else {
                return self.empty_get(
                    tool,
                    reference,
                    LookupSourceKind::Memory,
                    LookupSourceState::Unavailable,
                    current_version,
                    "canonical artifact locator unavailable",
                );
            };
            match self.read_canonical_artifact_page(
                &memory.id,
                content_digest,
                path,
                offset,
                LOOKUP_LIMITS_V1.body_page_bytes,
                call,
            ) {
                Ok(page) => page,
                Err(LookupServiceError::ArtifactUnavailable) => {
                    return self.empty_get(
                        tool,
                        reference,
                        LookupSourceKind::Memory,
                        LookupSourceState::Unavailable,
                        current_version,
                        "canonical artifact unavailable",
                    )
                }
                Err(error) => return Err(error),
            }
        } else {
            page_from_str(&memory.body, offset, LOOKUP_LIMITS_V1.body_page_bytes)?
        };
        call.checkpoint()?;
        if body_page.body.is_empty() {
            return self.empty_get(
                tool,
                reference,
                LookupSourceKind::Memory,
                LookupSourceState::Unavailable,
                current_version,
                &provenance,
            );
        }
        self.paged_get(
            tool,
            reference,
            LookupSourceKind::Memory,
            state,
            current_version,
            page,
            &provenance,
            body_page,
        )
    }

    fn read_step_page(
        &self,
        reference: String,
        cursor: Option<String>,
        step_id: String,
        expected_version: String,
        call: &LookupCall<'_>,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        let Some(snapshot) = self.load_step_snapshot(&step_id, call)? else {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::RunStep,
                LookupSourceState::Deleted,
                expected_version,
                "run step deleted",
            );
        };
        if snapshot.receipt_state.as_deref() == Some("deleted") {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::RunStep,
                LookupSourceState::Deleted,
                expected_version,
                "saved output deleted",
            );
        }
        let current_version = self.step_version(&snapshot, call)?;
        if current_version != expected_version {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::RunStep,
                LookupSourceState::Replaced,
                current_version,
                "run step version changed",
            );
        }
        let state = snapshot.state();
        let Some(expected_digest) = snapshot.output_digest.as_deref() else {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::RunStep,
                LookupSourceState::Unavailable,
                current_version,
                "trusted output identity unavailable",
            );
        };
        let (offset, page) = self.page_position(&reference, cursor.as_deref(), &current_version)?;
        let body_page = if snapshot.artifact_required {
            let Some(storage_id) = snapshot.storage_id.as_deref() else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved artifact identity unavailable",
                );
            };
            let Ok(_identity) = super::OutputStorageIdentity::parse(storage_id) else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved artifact identity invalid",
                );
            };
            let Some(memory_id) = snapshot.memory_id.as_deref() else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved artifact memory unavailable",
                );
            };
            if memory_id != storage_id {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved artifact ownership mismatch",
                );
            }
            let Some(memory) = self.load_memory(memory_id, call)? else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved artifact memory unavailable",
                );
            };
            let Some(content_digest) = memory.artifact_content_digest.as_deref() else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved artifact content identity unavailable",
                );
            };
            let Some(path) = self.canonical_artifact_path(&memory) else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved artifact locator unavailable",
                );
            };
            match self.read_canonical_artifact_page(
                memory_id,
                content_digest,
                path,
                offset,
                LOOKUP_LIMITS_V1.body_page_bytes,
                call,
            ) {
                Ok(page) => page,
                Err(LookupServiceError::ArtifactUnavailable) => {
                    return self.empty_get(
                        LookupToolName::HistoryGet,
                        reference,
                        LookupSourceKind::RunStep,
                        LookupSourceState::Unavailable,
                        current_version,
                        "saved artifact unavailable",
                    )
                }
                Err(error) => return Err(error),
            }
        } else {
            let Some(memory_id) = snapshot.memory_id.as_deref() else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved output memory unavailable",
                );
            };
            let Some(memory) = self.load_memory(memory_id, call)? else {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved output memory unavailable",
                );
            };
            if digest_text(&memory.body) != expected_digest {
                return self.empty_get(
                    LookupToolName::HistoryGet,
                    reference,
                    LookupSourceKind::RunStep,
                    LookupSourceState::Unavailable,
                    current_version,
                    "saved output identity mismatch",
                );
            }
            page_from_str(&memory.body, offset, LOOKUP_LIMITS_V1.body_page_bytes)?
        };
        call.checkpoint()?;
        if body_page.body.is_empty() {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::RunStep,
                LookupSourceState::Unavailable,
                current_version,
                "saved output body unavailable",
            );
        }
        self.paged_get(
            LookupToolName::HistoryGet,
            reference,
            LookupSourceKind::RunStep,
            state,
            current_version,
            page,
            "saved workflow output • receipt verified",
            body_page,
        )
    }

    fn read_evidence_page(
        &self,
        reference: String,
        cursor: Option<String>,
        evidence_id: String,
        expected_version: String,
        call: &LookupCall<'_>,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        let Some(snapshot) = self.load_evidence_snapshot(&evidence_id, call)? else {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::Evidence,
                LookupSourceState::Deleted,
                expected_version,
                "historical evidence deleted",
            );
        };
        let current_version = evidence_version(&snapshot.digest, &snapshot.capture_state);
        if current_version != expected_version {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::Evidence,
                LookupSourceState::Replaced,
                current_version,
                "historical evidence version changed",
            );
        }
        let state = evidence_state(&snapshot.capture_state);
        if !state.may_return_body() {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::Evidence,
                state,
                current_version,
                "historical evidence unavailable",
            );
        }
        let (offset, page) = self.page_position(&reference, cursor.as_deref(), &current_version)?;
        let body_page =
            self.evidence_blob_page(&evidence_id, offset, LOOKUP_LIMITS_V1.body_page_bytes, call)?;
        call.checkpoint()?;
        if body_page.body.is_empty() {
            return self.empty_get(
                LookupToolName::HistoryGet,
                reference,
                LookupSourceKind::Evidence,
                LookupSourceState::Unavailable,
                current_version,
                "historical evidence body unavailable",
            );
        }
        self.paged_get(
            LookupToolName::HistoryGet,
            reference,
            LookupSourceKind::Evidence,
            state,
            current_version,
            page,
            "workflow evidence",
            body_page,
        )
    }

    fn read_file_page(
        &self,
        path: PathBuf,
        offset: usize,
        maximum: usize,
        call: &LookupCall<'_>,
    ) -> Result<BodyPage, LookupServiceError> {
        self.run_file_operation(call, move |reader| reader.read_page(&path, offset, maximum))
    }


    fn read_raw_file(
        &self,
        path: PathBuf,
        offset: usize,
        maximum: usize,
        call: &LookupCall<'_>,
    ) -> Result<RawLookupRead, LookupServiceError> {
        self.run_file_operation(call, move |reader| reader.read_raw(&path, offset, maximum))
    }

    fn read_canonical_artifact_page(
        &self,
        memory_id: &str,
        content_digest: &str,
        path: PathBuf,
        offset: usize,
        maximum: usize,
        call: &LookupCall<'_>,
    ) -> Result<BodyPage, LookupServiceError> {
        if maximum == 0 || maximum > LOOKUP_LIMITS_V1.body_page_bytes {
            return Err(LookupServiceError::StorageUnavailable);
        }
        let metadata = self
            .artifact_manifest(memory_id, content_digest, 0, 0, call)?
            .ok_or(LookupServiceError::ArtifactUnavailable)?;
        if metadata.state != ArtifactVerificationState::Verified
            || metadata.memory_id != memory_id
            || metadata.content_digest != content_digest
            || metadata.chunk_size != ARTIFACT_CHUNK_SIZE
            || metadata.byte_length == 0
        {
            return Err(LookupServiceError::ArtifactUnavailable);
        }
        let expected_chunk_count = metadata
            .byte_length
            .checked_add(metadata.chunk_size - 1)
            .and_then(|length| length.checked_div(metadata.chunk_size))
            .ok_or(LookupServiceError::ArtifactUnavailable)?;
        if metadata.chunk_count != expected_chunk_count {
            return Err(LookupServiceError::ArtifactUnavailable);
        }
        if offset > metadata.byte_length {
            return Err(LookupContractError::InvalidCursor.into());
        }
        if offset == metadata.byte_length {
            return Err(LookupServiceError::ArtifactUnavailable);
        }
        let page_end = offset
            .checked_add(maximum)
            .ok_or(LookupServiceError::StorageUnavailable)?;
        let first_chunk = offset / metadata.chunk_size;
        let last_chunk = (page_end.min(metadata.byte_length) - 1) / metadata.chunk_size;
        if last_chunk.saturating_sub(first_chunk) > 1 {
            return Err(LookupServiceError::ArtifactUnavailable);
        }
        let metadata_byte_length = metadata.byte_length;
        let metadata_chunk_count = metadata.chunk_count;
        let manifest = if first_chunk == 0 && last_chunk == 0 {
            metadata
        } else {
            self.artifact_manifest(
                memory_id,
                content_digest,
                first_chunk,
                last_chunk,
                call,
            )?
            .ok_or(LookupServiceError::ArtifactUnavailable)?
        };
        if manifest.state != ArtifactVerificationState::Verified
            || manifest.memory_id != memory_id
            || manifest.content_digest != content_digest
            || manifest.chunk_size != ARTIFACT_CHUNK_SIZE
            || manifest.byte_length != metadata_byte_length
            || manifest.chunk_count != metadata_chunk_count
            || manifest.chunks.len() != last_chunk - first_chunk + 1
            || manifest
                .chunks
                .iter()
                .enumerate()
                .any(|(index, chunk)| chunk.index != first_chunk + index)
        {
            return Err(LookupServiceError::ArtifactUnavailable);
        }
        let base_offset = first_chunk
            .checked_mul(manifest.chunk_size)
            .ok_or(LookupServiceError::ArtifactUnavailable)?;
        let span_bytes = (last_chunk - first_chunk + 1)
            .checked_mul(manifest.chunk_size)
            .ok_or(LookupServiceError::ArtifactUnavailable)?;
        let bytes_to_read = manifest
            .byte_length
            .checked_sub(base_offset)
            .ok_or(LookupServiceError::ArtifactUnavailable)?
            .min(span_bytes);
        let raw = self.read_raw_file(path, base_offset, bytes_to_read, call)?;
        if raw.total_bytes != manifest.byte_length || raw.bytes.len() != bytes_to_read {
            return Err(LookupServiceError::ArtifactUnavailable);
        }
        for (index, chunk) in manifest.chunks.iter().enumerate() {
            let start = index * manifest.chunk_size;
            let end = (start + manifest.chunk_size).min(bytes_to_read);
            if end <= start || digest_bytes(&raw.bytes[start..end]) != chunk.digest {
                return Err(LookupServiceError::ArtifactUnavailable);
            }
        }
        let offset_in_read = offset - base_offset;
        let verified_bytes = &raw.bytes[offset_in_read..];
        let decoded = match std::str::from_utf8(verified_bytes) {
            Ok(decoded) => decoded,
            Err(error) if error.error_len().is_none() => {
                let read_end = base_offset
                    .checked_add(raw.bytes.len())
                    .ok_or(LookupServiceError::ArtifactUnavailable)?;
                if read_end == raw.total_bytes {
                    return Err(LookupServiceError::ArtifactUnavailable);
                }
                std::str::from_utf8(&verified_bytes[..error.valid_up_to()])
                    .map_err(|_| LookupServiceError::ArtifactUnavailable)?
            }
            Err(_) => return Err(LookupServiceError::ArtifactUnavailable),
        };
        let body = utf8_prefix(decoded, maximum).to_owned();
        let consumed = offset
            .checked_add(body.len())
            .ok_or(LookupServiceError::StorageUnavailable)?;
        Ok(BodyPage {
            body,
            next_offset: (consumed < manifest.byte_length).then_some(consumed),
        })
    }

    fn canonical_artifact_path(&self, memory: &Memory) -> Option<PathBuf> {
        let stored_path = Path::new(memory.artifact_path.as_deref()?);
        if let Some(path) = self.db.trusted_memory_artifact_path(stored_path) {
            return Some(path);
        }

        let root = normalize_absolute_path(&self.output_artifact_root)?;
        let stored = normalize_absolute_path(stored_path)?;
        let relative = stored.strip_prefix(&root).ok()?;
        let owned = super::OutputStorageIdentity::from_owned_artifact_relative_path(relative)?;
        if owned.memory_id() != memory.id {
            return None;
        }
        trusted_output_artifact_path(&root, &stored)
    }

    fn artifact_manifest(
        &self,
        memory_id: &str,
        content_digest: &str,
        first_chunk: usize,
        last_chunk: usize,
        call: &LookupCall<'_>,
    ) -> Result<Option<ArtifactChunkManifest>, LookupServiceError> {
        lookup_with_conn(&self.db, call, |conn| {
            load_artifact_chunk_manifest_range_conn(
                conn,
                memory_id,
                content_digest,
                first_chunk,
                last_chunk,
            )
        })
    }

    fn artifact_manifest_version(manifest: &ArtifactChunkManifest) -> String {
        format!(
            "artifact-v1:{}",
            digest_fields(&[
                &manifest.memory_id,
                &manifest.content_digest,
                &manifest.digest_version.to_string(),
                &manifest.byte_length.to_string(),
                &manifest.chunk_size.to_string(),
                &manifest.chunk_count.to_string(),
                manifest.state.as_str(),
            ])
        )
    }

    fn step_version(
        &self,
        snapshot: &StepSnapshot,
        call: &LookupCall<'_>,
    ) -> Result<String, LookupServiceError> {
        let artifact_identity = if snapshot.artifact_required {
            let Some(storage_id) = snapshot.storage_id.as_deref() else {
                return Ok(snapshot.version("artifact_identity_unavailable"));
            };
            let Ok(_identity) = super::OutputStorageIdentity::parse(storage_id) else {
                return Ok(snapshot.version("artifact_identity_invalid"));
            };
            let Some(memory_id) = snapshot.memory_id.as_deref() else {
                return Ok(snapshot.version("artifact_memory_unavailable"));
            };
            if memory_id != storage_id {
                return Ok(snapshot.version("artifact_ownership_mismatch"));
            }
            let Some(memory) = self.load_memory(memory_id, call)? else {
                return Ok(snapshot.version("artifact_memory_unavailable"));
            };
            let Some(content_digest) = memory.artifact_content_digest.as_deref() else {
                return Ok(snapshot.version("artifact_content_identity_unavailable"));
            };
            let manifest = self.artifact_manifest(memory_id, content_digest, 0, 0, call)?;
            match manifest {
                Some(manifest) if manifest.state == ArtifactVerificationState::Verified => {
                    Self::artifact_manifest_version(&manifest)
                }
                Some(_) => "artifact_legacy_unverified".into(),
                None => "artifact_manifest_unavailable".into(),
            }
        } else {
            "inline_output".into()
        };
        Ok(snapshot.version(&artifact_identity))
    }

    fn run_file_operation<T: Send + 'static>(
        &self,
        call: &LookupCall<'_>,
        operation: impl FnOnce(Arc<dyn LookupFileReader>) -> Result<T, LookupServiceError>
            + Send
            + 'static,
    ) -> Result<T, LookupServiceError> {
        if self
            .file_worker_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(LookupServiceError::StorageUnavailable);
        }

        let active = self.file_worker_active.clone();
        let reader = self.file_reader.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            struct ActiveGuard(Option<Arc<AtomicBool>>);
            impl ActiveGuard {
                fn release(&mut self) {
                    if let Some(active) = self.0.take() {
                        active.store(false, Ordering::Release);
                    }
                }
            }
            impl Drop for ActiveGuard {
                fn drop(&mut self) {
                    self.release();
                }
            }
            let mut guard = ActiveGuard(Some(active));
            let result = operation(reader.clone());
            guard.release();
            let _ = sender.send(result);
            #[cfg(test)]
            reader.after_publish();
        });

        loop {
            call.checkpoint()?;
            match receiver.recv_timeout(Duration::from_millis(1)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(LookupServiceError::StorageUnavailable)
                }
            }
        }
    }

    fn paged_get(
        &self,
        tool: LookupToolName,
        reference: String,
        source: LookupSourceKind,
        state: LookupSourceState,
        source_version: String,
        page: u32,
        provenance: &str,
        body_page: BodyPage,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        let next_cursor = body_page
            .next_offset
            .map(|offset| self.register_cursor(&reference, &source_version, offset, page + 1))
            .transpose()?;
        Ok(LookupGetResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool,
            reference,
            source,
            state,
            source_version,
            page,
            provenance: provenance.into(),
            body: Some(body_page.body),
            truncated: next_cursor.is_some(),
            next_cursor,
        })
    }

    fn empty_get(
        &self,
        tool: LookupToolName,
        reference: String,
        source: LookupSourceKind,
        state: LookupSourceState,
        source_version: String,
        provenance: &str,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        Ok(LookupGetResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool,
            reference,
            source,
            state,
            source_version,
            page: 0,
            provenance: provenance.into(),
            body: None,
            truncated: false,
            next_cursor: None,
        })
    }

    fn load_step_snapshot(
        &self,
        step_id: &str,
        call: &LookupCall<'_>,
    ) -> Result<Option<StepSnapshot>, LookupServiceError> {
        lookup_with_conn(&self.db, call, |conn| {
            conn.query_row(
                "SELECT rs.id, rs.status,
                        COALESCE(NULLIF(rs.skill_name, ''), NULLIF(rs.agent_provider, ''), rs.node_id),
                        rs.created_at, o.state, o.storage_id, o.output_sha256,
                        COALESCE(o.artifact_required, 0), o.memory_id, o.updated_at
                   FROM run_steps rs
                   JOIN runs r ON r.id = rs.run_id
                   LEFT JOIN output_receipts o
                     ON o.run_id = rs.run_id AND o.step_id = rs.id
                  WHERE rs.id = ?1 AND r.workflow_id = ?2",
                params![step_id, self.context.workflow_id],
                |row| {
                    Ok(StepSnapshot {
                        step_id: row.get(0)?,
                        status: row.get(1)?,
                        title: row.get(2)?,
                        observed_at: row.get(3)?,
                        receipt_state: row.get(4)?,
                        storage_id: row.get(5)?,
                        output_digest: row.get(6)?,
                        artifact_required: row.get::<_, i64>(7)? != 0,
                        memory_id: row.get(8)?,
                        receipt_updated_at: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        })
    }

    fn load_evidence_snapshot(
        &self,
        evidence_id: &str,
        call: &LookupCall<'_>,
    ) -> Result<Option<EvidenceSnapshot>, LookupServiceError> {
        lookup_with_conn(&self.db, call, |conn| {
            conn.query_row(
                "SELECT es.capture_state, es.content_digest
                   FROM run_evidence_sources es
                   JOIN runs r ON r.id = es.run_id
                  WHERE es.id = ?1 AND r.workflow_id = ?2",
                params![evidence_id, self.context.workflow_id],
                |row| {
                    Ok(EvidenceSnapshot {
                        capture_state: row.get(0)?,
                        digest: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        })
    }

    fn evidence_blob_page(
        &self,
        evidence_id: &str,
        offset: usize,
        maximum: usize,
        call: &LookupCall<'_>,
    ) -> Result<BodyPage, LookupServiceError> {
        let row = lookup_with_conn(&self.db, call, |conn| {
            conn.query_row(
                "SELECT length(CAST(es.normalized_text AS BLOB)),
                        substr(CAST(es.normalized_text AS BLOB), ?2, ?3)
                   FROM run_evidence_sources es
                   JOIN runs r ON r.id = es.run_id
                  WHERE es.id = ?1 AND r.workflow_id = ?4",
                params![
                    evidence_id,
                    (offset + 1) as i64,
                    (maximum + 4) as i64,
                    self.context.workflow_id
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(Into::into)
        })?
        .ok_or(LookupContractError::InvalidReference)?;
        page_from_chunk(row.1, offset, row.0 as usize, maximum)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum LookupResponse {
    Search(LookupSearchResponse),
    Get(LookupGetResponse),
}

struct LookupCall<'a> {
    control: &'a dyn LookupCallControl,
    deadline: Instant,
    now: DateTime<Utc>,
}

impl<'a> LookupCall<'a> {
    fn begin(
        session: &AgentMemoryLookupSession,
        control: &'a dyn LookupCallControl,
    ) -> Result<Self, LookupServiceError> {
        let started = Instant::now();
        let call = Self {
            control,
            deadline: started + session.read_deadline,
            now: session.clock.now(),
        };
        loop {
            match session.budget.try_lock() {
                Ok(mut budget) => {
                    budget.begin_call()?;
                    break;
                }
                Err(TryLockError::WouldBlock) => {
                    call.checkpoint()?;
                    thread::sleep(Duration::from_millis(1));
                }
                Err(TryLockError::Poisoned(_)) => {
                    return Err(LookupServiceError::StorageUnavailable)
                }
            }
        }
        call.checkpoint()?;
        Ok(call)
    }

    fn checkpoint(&self) -> Result<(), LookupServiceError> {
        match self.control.checkpoint() {
            Ok(()) => {}
            Err(LookupAbort::Cancelled) => return Err(LookupServiceError::Cancelled),
            Err(LookupAbort::TimedOut) => return Err(LookupServiceError::TimedOut),
        }
        if Instant::now() >= self.deadline {
            return Err(LookupServiceError::TimedOut);
        }
        Ok(())
    }

    fn abort_requested(&self) -> bool {
        self.control.checkpoint().is_err() || Instant::now() >= self.deadline
    }
}

#[derive(Default)]
struct TokenRegistry {
    references: HashMap<String, ReferenceBinding>,
    cursors: HashMap<String, CursorBinding>,
}

#[derive(Clone)]
struct ReferenceBinding {
    scope_fingerprint: String,
    source: BoundSource,
}

#[derive(Debug, Clone)]
enum BoundSource {
    Memory {
        memory_id: String,
        version: String,
    },
    RunStep {
        step_id: String,
        version: String,
    },
    Evidence {
        evidence_id: String,
        version: String,
    },
}

impl BoundSource {
    fn identity(&self) -> (&str, &str) {
        match self {
            Self::Memory { memory_id, version } => (memory_id, version),
            Self::RunStep { step_id, version } => (step_id, version),
            Self::Evidence {
                evidence_id,
                version,
            } => (evidence_id, version),
        }
    }
}

#[derive(Clone)]
struct CursorBinding {
    scope_fingerprint: String,
    reference: String,
    source_version: String,
    offset: usize,
    page: u32,
}

struct HistoryCandidate {
    source_id: String,
    source: LookupSourceKind,
    state: LookupSourceState,
    title: String,
    provenance: String,
    excerpt: String,
    observed_at: String,
    truncated: bool,
    rank: f64,
    binding: BoundSource,
}

struct StepSnapshot {
    step_id: String,
    status: String,
    title: String,
    observed_at: String,
    receipt_state: Option<String>,
    storage_id: Option<String>,
    output_digest: Option<String>,
    artifact_required: bool,
    memory_id: Option<String>,
    receipt_updated_at: Option<String>,
}

impl StepSnapshot {
    fn state(&self) -> LookupSourceState {
        if self.status == "completed" {
            LookupSourceState::Historical
        } else {
            LookupSourceState::Incomplete
        }
    }

    fn version(&self, artifact_identity: &str) -> String {
        format!(
            "receipt-v1:{}",
            digest_fields(&[
                &self.step_id,
                &self.status,
                self.receipt_state.as_deref().unwrap_or("none"),
                self.storage_id.as_deref().unwrap_or(""),
                self.output_digest.as_deref().unwrap_or("unversioned"),
                self.memory_id.as_deref().unwrap_or(""),
                self.receipt_updated_at.as_deref().unwrap_or(""),
                artifact_identity,
            ])
        )
    }
}

struct EvidenceSnapshot {
    capture_state: String,
    digest: String,
}

struct BodyPage {
    body: String,
    next_offset: Option<usize>,
}

fn storage<T>(result: Result<T, super::DbError>) -> Result<T, LookupServiceError> {
    result.map_err(|_| LookupServiceError::StorageUnavailable)
}

fn lookup_with_conn<T>(
    db: &Db,
    call: &LookupCall<'_>,
    mut operation: impl FnMut(&Connection) -> Result<T, super::DbError>,
) -> Result<T, LookupServiceError> {
    let connection = loop {
        call.checkpoint()?;
        match db.conn.try_lock() {
            Ok(connection) => break connection,
            Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(1)),
            Err(TryLockError::Poisoned(_)) => return Err(LookupServiceError::StorageUnavailable),
        }
    };
    call.checkpoint()?;

    let previous_busy_timeout_ms = storage(
        connection
            .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
            .map_err(Into::into),
    )?;
    let previous_busy_timeout_ms = u64::try_from(previous_busy_timeout_ms)
        .map_err(|_| LookupServiceError::StorageUnavailable)?;
    storage(connection.busy_timeout(Duration::ZERO).map_err(Into::into))?;

    let finished = AtomicBool::new(false);
    let interrupt = connection.get_interrupt_handle();
    let result = thread::scope(|scope| {
        let watcher = scope.spawn(|| {
            while !finished.load(Ordering::Acquire) {
                if call.abort_requested() {
                    interrupt.interrupt();
                    break;
                }
                thread::park_timeout(Duration::from_millis(1));
            }
        });
        let result = loop {
            call.checkpoint()?;
            match operation(&connection) {
                Err(error) if db_error_is_busy(&error) => {
                    thread::sleep(Duration::from_millis(1));
                }
                result => break storage(result),
            }
        };
        finished.store(true, Ordering::Release);
        watcher.thread().unpark();
        let _ = watcher.join();
        result
    });
    let restore = connection
        .busy_timeout(Duration::from_millis(previous_busy_timeout_ms))
        .map_err(Into::into);
    storage(restore)?;
    if let Err(abort) = call.checkpoint() {
        return Err(abort);
    }
    result
}

fn db_error_is_busy(error: &super::DbError) -> bool {
    matches!(
        error,
        super::DbError::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
            if matches!(
                failure.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

fn lookup_table_exists(
    db: &Db,
    call: &LookupCall<'_>,
    table: &str,
) -> Result<bool, LookupServiceError> {
    lookup_with_conn(db, call, |conn| {
        conn.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
             )",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(Into::into)
    })
}

fn locked<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, LookupServiceError> {
    mutex
        .lock()
        .map_err(|_| LookupServiceError::StorageUnavailable)
}

fn try_locked<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, LookupServiceError> {
    match mutex.try_lock() {
        Ok(guard) => Ok(guard),
        Err(TryLockError::WouldBlock | TryLockError::Poisoned(_)) => {
            Err(LookupServiceError::StorageUnavailable)
        }
    }
}

fn inspect_lookup_schema(db: &Db) -> Result<LookupSchemaState, LookupServiceError> {
    let columns = storage(db.with_conn(|conn| {
        let mut statement = conn.prepare("PRAGMA table_info(memories)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(columns)
    }))?;
    if [
        "semantic_revision",
        "artifact_content_digest",
        "observed_at",
        "valid_from",
    ]
    .iter()
    .all(|required| columns.iter().any(|column| column == required))
    {
        Ok(LookupSchemaState::SemanticRevisionV1)
    } else {
        Err(LookupServiceError::Plan050IntegrationRequired)
    }
}

fn normalize_absolute_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut output = PathBuf::new();
    let mut normal = 0usize;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            Component::RootDir => output.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(value) => {
                output.push(value);
                normal += 1;
            }
            Component::ParentDir if normal > 0 => {
                output.pop();
                normal -= 1;
            }
            Component::ParentDir => return None,
        }
    }
    Some(output)
}

fn lookup_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn bounded_required(value: &str, maximum: usize, fallback: &str) -> String {
    let value = value.trim();
    let value = if value.is_empty() { fallback } else { value };
    utf8_prefix(value, maximum).to_owned()
}

fn memory_provenance(memory: &Memory, historical: bool) -> String {
    let history = if historical { " history" } else { "" };
    let qualification = if memory.evidence_is_inference {
        "inferred claim"
    } else {
        "recorded claim"
    };
    let subject = memory
        .evidence_subject
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unspecified subject");
    format!(
        "{} memory{} • source: {} • provenance: {} • qualification: {} • subject: {}",
        memory.scope_type.as_str(),
        history,
        memory.source,
        memory.provenance_state,
        qualification,
        subject
    )
}

fn digest_fields(fields: &[&str]) -> String {
    let mut digest = Sha256::new();
    for field in fields {
        digest.update(field.len().to_be_bytes());
        digest.update(field.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn memory_version(memory: &Memory) -> String {
    format!(
        "memory-v1:{}",
        digest_fields(&[
            &memory.id,
            &memory.semantic_revision.to_string(),
            &memory.content_digest,
            memory.artifact_content_digest.as_deref().unwrap_or(""),
            &memory.source,
            &memory.provenance_state,
            memory.evidence_subject.as_deref().unwrap_or(""),
            if memory.evidence_is_inference {
                "inferred"
            } else {
                "recorded"
            },
            &format!("{:?}", memory.status),
            memory.supersedes_id.as_deref().unwrap_or(""),
            memory.valid_from.as_deref().unwrap_or(""),
            memory.expires_at.as_deref().unwrap_or(""),
        ])
    )
}

fn evidence_state(capture_state: &str) -> LookupSourceState {
    match capture_state {
        "complete" => LookupSourceState::Historical,
        "truncated" => LookupSourceState::Incomplete,
        _ => LookupSourceState::Unavailable,
    }
}

fn evidence_version(digest: &str, capture_state: &str) -> String {
    format!("evidence-v1:{}", digest_fields(&[digest, capture_state]))
}

fn digest_bytes(value: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(value);
    format!("{:x}", digest.finalize())
}

fn digest_text(value: &str) -> String {
    digest_bytes(value.as_bytes())
}


fn regular_file_metadata(path: &Path) -> Result<std::fs::Metadata, LookupServiceError> {
    let link_metadata = path.symlink_metadata().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            LookupServiceError::ArtifactUnavailable
        } else {
            LookupServiceError::StorageUnavailable
        }
    })?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(LookupServiceError::StorageUnavailable);
    }
    Ok(link_metadata)
}

fn page_from_str(
    value: &str,
    offset: usize,
    maximum: usize,
) -> Result<BodyPage, LookupServiceError> {
    if offset > value.len() || !value.is_char_boundary(offset) {
        return Err(LookupContractError::InvalidCursor.into());
    }
    let remaining = &value[offset..];
    let body = utf8_prefix(remaining, maximum).to_owned();
    let consumed = offset + body.len();
    Ok(BodyPage {
        body,
        next_offset: (consumed < value.len()).then_some(consumed),
    })
}

fn page_from_regular_file(
    path: &Path,
    offset: usize,
    maximum: usize,
) -> Result<BodyPage, LookupServiceError> {
    regular_file_metadata(path)?;
    let mut file = File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            LookupServiceError::ArtifactUnavailable
        } else {
            LookupServiceError::StorageUnavailable
        }
    })?;
    let metadata = file.metadata().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            LookupServiceError::ArtifactUnavailable
        } else {
            LookupServiceError::StorageUnavailable
        }
    })?;
    if !metadata.is_file() {
        return Err(LookupServiceError::StorageUnavailable);
    }
    let total = metadata.len() as usize;
    if offset > total {
        return Err(LookupContractError::InvalidCursor.into());
    }
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|_| LookupServiceError::StorageUnavailable)?;
    let mut bytes = Vec::with_capacity(maximum + 4);
    file.take((maximum + 4) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| LookupServiceError::StorageUnavailable)?;
    page_from_chunk(bytes, offset, total, maximum)
}

fn page_from_chunk(
    mut bytes: Vec<u8>,
    offset: usize,
    total: usize,
    maximum: usize,
) -> Result<BodyPage, LookupServiceError> {
    if offset > total {
        return Err(LookupContractError::InvalidCursor.into());
    }
    if let Err(error) = std::str::from_utf8(&bytes) {
        if error.error_len().is_some() {
            return Err(LookupServiceError::StorageUnavailable);
        }
        bytes.truncate(error.valid_up_to());
    }
    let decoded =
        std::str::from_utf8(&bytes).map_err(|_| LookupServiceError::StorageUnavailable)?;
    let body = utf8_prefix(decoded, maximum).to_owned();
    let consumed = offset + body.len();
    Ok(BodyPage {
        body,
        next_offset: (consumed < total).then_some(consumed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::artifact_chunks::persist_artifact_chunk_manifest_conn;
    use crate::db::history::index_run_step;
    use crate::db::memory_validity::artifact_content_digest;
    use crate::db::output_persistence::create_output_persistence_schema;
    use crate::db::{
        CreateMemoryInput, CreateWorkflowInput, EvidenceOrigin, EvidenceSourceInput,
        MemoryScopeType, MemoryType, OutputStorageIdentity,
    };
    use serde_json::{json, Value};
    use std::fs;
    use std::sync::Barrier;
    use std::thread;
    use std::time::Duration;

    struct ManualLookupClock(Mutex<DateTime<Utc>>);

    impl ManualLookupClock {
        fn new(now: &str) -> Self {
            Self(Mutex::new(
                DateTime::parse_from_rfc3339(now)
                    .expect("valid test time")
                    .with_timezone(&Utc),
            ))
        }

        fn set(&self, now: &str) {
            *self.0.lock().unwrap() = DateTime::parse_from_rfc3339(now)
                .expect("valid test time")
                .with_timezone(&Utc);
        }
    }

    impl LookupClock for ManualLookupClock {
        fn now(&self) -> DateTime<Utc> {
            *self.0.lock().unwrap()
        }
    }

    struct SlowLookupFileReader {
        delay: Duration,
    }

    impl LookupFileReader for SlowLookupFileReader {

        fn read_page(
            &self,
            _path: &Path,
            offset: usize,
            maximum: usize,
        ) -> Result<BodyPage, LookupServiceError> {
            thread::sleep(self.delay);
            page_from_str("slow file body", offset, maximum)
        }
    }


    struct HandoffLookupFileReader {
        hold_first_published_worker: AtomicBool,
        published_barrier: Arc<Barrier>,
    }

    impl LookupFileReader for HandoffLookupFileReader {

        fn read_page(
            &self,
            _path: &Path,
            offset: usize,
            maximum: usize,
        ) -> Result<BodyPage, LookupServiceError> {
            page_from_str("handoff body", offset, maximum)
        }

        fn after_publish(&self) {
            if self
                .hold_first_published_worker
                .swap(false, Ordering::AcqRel)
            {
                self.published_barrier.wait();
                self.published_barrier.wait();
            }
        }
    }

    struct LookupTestDirectory {
        path: PathBuf,
    }

    impl LookupTestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("alfred-lookup-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create lookup test directory");
            Self { path }
        }
    }

    impl Drop for LookupTestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn create_lookup_workflow(db: &Db, name: &str, working_directory: &Path) -> String {
        let id = db
            .create_workflow(CreateWorkflowInput {
                name: name.into(),
                description: String::new(),
                working_directory: working_directory.to_string_lossy().into_owned(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .expect("create lookup workflow")
            .id;
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE workflows SET memory_lookup_enabled = 1 WHERE id = ?1",
                [&id],
            )?;
            Ok(())
        })
        .expect("enable lookup workflow fixture");
        id
    }

    fn insert_lookup_run(db: &Db, workflow_id: &str, run_id: &str) {
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs
                 (id, workflow_id, trigger_kind, status, started_at, created_at)
                 VALUES (?1, ?2, 'manual', 'running', '2026-09-09T10:00:00Z',
                         '2026-09-09T10:00:00Z')",
                params![run_id, workflow_id],
            )?;
            Ok(())
        })
        .expect("insert lookup run");
    }

    fn create_lookup_memory(
        db: &Db,
        id: &str,
        workflow_id: &str,
        scope_type: MemoryScopeType,
        title: &str,
        body: &str,
    ) -> Memory {
        db.create_memory(CreateMemoryInput {
            workflow_id: workflow_id.into(),
            title: title.into(),
            body: body.into(),
            run_id: None,
            node_id: None,
            kind: Some("note".into()),
            scope_type: Some(scope_type),
            memory_type: Some(MemoryType::Fact),
            source: Some("manual".into()),
            pinned: Some(false),
            confidence: Some(1.0),
            salience: Some(50),
            status: Some(MemoryStatus::Active),
            supersedes_id: None,
            fact_key: None,
            observed_at: None,
            last_confirmed_at: None,
            valid_from: None,
            expires_at: None,
            id: Some(id.into()),
        })
        .expect("create lookup memory")
    }

    fn configure_lookup_artifact(db: &Db, memory_id: &str, path: &Path, body: &str) {
        let content_digest = artifact_content_digest(body.as_bytes());
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                    SET kind = 'artifact', memory_type = 'artifact',
                        artifact_path = ?1, artifact_content_digest = ?2
                  WHERE id = ?3",
                params![path.to_string_lossy().as_ref(), content_digest, memory_id],
            )?;
            persist_artifact_chunk_manifest_conn(
                conn,
                memory_id,
                &content_digest,
                body.as_bytes(),
                "2026-09-09T10:02:00Z",
            )?;
            Ok(())
        })
        .expect("configure lookup artifact");
    }

    fn start_lookup_session(
        db: Arc<Db>,
        root: &Path,
        run_id: &str,
        node_id: &str,
        working_directory: &Path,
    ) -> AgentMemoryLookupSession {
        AgentMemoryLookupService::new(db, root)
            .start_session_at(
                run_id,
                node_id,
                Some(working_directory),
                DateTime::parse_from_rfc3339("2026-09-09T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .expect("start lookup session")
    }

    fn execute_search(
        session: &AgentMemoryLookupSession,
        tool: LookupToolName,
        query: &str,
    ) -> LookupSearchResponse {
        let encoded = serde_json::to_vec(&json!({"query": query, "limit": 8})).unwrap();
        let response = session
            .execute_json(tool, &encoded, &UninterruptedLookup)
            .expect("execute lookup search");
        serde_json::from_str(&response).expect("decode lookup search response")
    }

    fn execute_get(
        session: &AgentMemoryLookupSession,
        tool: LookupToolName,
        reference: &str,
        cursor: Option<&str>,
    ) -> Result<LookupGetResponse, LookupServiceError> {
        let encoded = serde_json::to_vec(&json!({
            "reference": reference,
            "cursor": cursor,
        }))
        .unwrap();
        session
            .execute_json(tool, &encoded, &UninterruptedLookup)
            .and_then(|response| {
                serde_json::from_str(&response).map_err(|_| LookupServiceError::StorageUnavailable)
            })
    }

    fn insert_indexed_step(
        db: &Db,
        workflow_id: &str,
        run_id: &str,
        step_id: &str,
        node_id: &str,
        output: Value,
    ) {
        insert_indexed_step_with_input(
            db,
            workflow_id,
            run_id,
            step_id,
            node_id,
            json!({}),
            output,
            None,
        );
    }

    fn insert_indexed_step_with_input(
        db: &Db,
        workflow_id: &str,
        run_id: &str,
        step_id: &str,
        node_id: &str,
        input: Value,
        output: Value,
        error: Option<&str>,
    ) {
        db.with_conn(|conn| {
            let input_json = serde_json::to_string(&input)
                .map_err(|_| super::super::DbError::Other("serialize fixture".into()))?;
            let output_json = serde_json::to_string(&output)
                .map_err(|_| super::super::DbError::Other("serialize fixture".into()))?;
            conn.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, agent_provider, status, input_json, output_json,
                  error, started_at, finished_at, created_at)
                 VALUES (?1, ?2, ?3, 'codex', 'completed', ?4, ?5, ?6,
                         '2026-09-09T10:01:00Z', '2026-09-09T10:02:00Z',
                         '2026-09-09T10:01:00Z')",
                params![step_id, run_id, node_id, input_json, output_json, error],
            )?;
            index_run_step(
                conn,
                step_id,
                run_id,
                workflow_id,
                node_id,
                &input,
                &output,
                error,
            )?;
            Ok(())
        })
        .expect("insert indexed step");
    }

    fn search_hit() -> LookupSearchHit {
        LookupSearchHit {
            reference: "memory_ref_1".into(),
            source: LookupSourceKind::Memory,
            state: LookupSourceState::Current,
            title: "Release decision".into(),
            provenance: "workflow memory".into(),
            excerpt: "Use the signed release checklist.".into(),
            observed_at: Some("2026-09-09T12:00:00Z".into()),
            truncated: false,
        }
    }

    fn get_response() -> LookupGetResponse {
        LookupGetResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool: LookupToolName::MemoryGet,
            reference: "memory_ref_1".into(),
            source: LookupSourceKind::Memory,
            state: LookupSourceState::Current,
            source_version: "version_1".into(),
            page: 0,
            provenance: "workflow memory".into(),
            body: Some("Use the signed release checklist.".into()),
            truncated: false,
            next_cursor: None,
        }
    }

    #[test]
    fn sqlite_memory_reads_bind_scope_and_revalidate_mutations_and_links() {
        let directory = LookupTestDirectory::new();
        let workspace_a = directory.path.join("workspace-a");
        let workspace_b = directory.path.join("workspace-b");
        fs::create_dir_all(&workspace_a).unwrap();
        fs::create_dir_all(&workspace_b).unwrap();
        let db = Arc::new(Db::open_in_memory().expect("open lookup database"));
        let workflow_a = create_lookup_workflow(&db, "Lookup A", &workspace_a);
        let workflow_b = create_lookup_workflow(&db, "Lookup B", &workspace_b);
        insert_lookup_run(&db, &workflow_a, "lookup-run-a");
        insert_lookup_run(&db, &workflow_b, "lookup-run-b");

        create_lookup_memory(
            &db,
            "owned-memory",
            &workflow_a,
            MemoryScopeType::Workflow,
            "Owned scopeNeedle",
            "Owned scopeNeedle body",
        );
        create_lookup_memory(
            &db,
            "user-memory",
            &workflow_a,
            MemoryScopeType::User,
            "User scopeNeedle",
            "User scopeNeedle body",
        );
        create_lookup_memory(
            &db,
            "workspace-memory",
            &workflow_a,
            MemoryScopeType::Workspace,
            "Workspace scopeNeedle",
            "Workspace scopeNeedle body",
        );
        create_lookup_memory(
            &db,
            "linked-memory",
            &workflow_b,
            MemoryScopeType::Workflow,
            "Linked scopeNeedle",
            "Linked scopeNeedle body",
        );
        create_lookup_memory(
            &db,
            "unrelated-memory",
            &workflow_b,
            MemoryScopeType::Workflow,
            "Unrelated scopeNeedle",
            "Unrelated scopeNeedle body",
        );
        create_lookup_memory(
            &db,
            "artifact-memory",
            &workflow_a,
            MemoryScopeType::Workflow,
            "Artifact scopeNeedle",
            "Artifact scopeNeedle trusted preview",
        );
        let untrusted_artifact = directory.path.join("untrusted-memory-artifact.txt");
        fs::write(
            &untrusted_artifact,
            "scopeNeedle contents must not be read before Plan 050 identity",
        )
        .unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories SET artifact_path = ?1 WHERE id = 'artifact-memory'",
                [untrusted_artifact.to_string_lossy().as_ref()],
            )?;
            conn.execute(
                "UPDATE memories
                    SET evidence_is_inference = 1, evidence_subject = 'user_statement'
                  WHERE id = 'owned-memory'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db.link_memory(&workflow_a, "linked-memory")
            .expect("link memory fixture");

        let session = start_lookup_session(
            db.clone(),
            &directory.path,
            "lookup-run-a",
            "agent-node",
            &workspace_a,
        );
        assert_eq!(
            session.schema_state(),
            LookupSchemaState::SemanticRevisionV1
        );
        let found = execute_search(&session, LookupToolName::MemorySearch, "scopeNeedle");
        let titles = found
            .results
            .iter()
            .map(|hit| hit.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(found.results.len(), 5);
        assert!(titles.contains(&"Owned scopeNeedle"));
        assert!(titles.contains(&"User scopeNeedle"));
        assert!(titles.contains(&"Workspace scopeNeedle"));
        assert!(titles.contains(&"Linked scopeNeedle"));
        assert!(titles.contains(&"Artifact scopeNeedle"));
        assert!(!titles.contains(&"Unrelated scopeNeedle"));
        assert!(found.results.iter().all(|hit| {
            hit.reference.starts_with("mlr1_")
                && !hit.reference.contains("memory")
                && hit.provenance.contains("provenance:")
        }));
        let inferred = found
            .results
            .iter()
            .find(|hit| hit.title == "Owned scopeNeedle")
            .unwrap();
        assert!(inferred
            .provenance
            .contains("qualification: inferred claim"));
        assert!(inferred.provenance.contains("subject: user_statement"));
        let artifact = found
            .results
            .iter()
            .find(|hit| hit.title == "Artifact scopeNeedle")
            .unwrap();
        assert_eq!(artifact.excerpt, "Artifact scopeNeedle trusted preview");
        assert!(artifact.truncated);
        let artifact_get = execute_get(
            &session,
            LookupToolName::MemoryGet,
            &artifact.reference,
            None,
        )
        .unwrap();
        assert_eq!(artifact_get.state, LookupSourceState::Unavailable);
        assert!(artifact_get.body.is_none());

        let owned = found
            .results
            .iter()
            .find(|hit| hit.title == "Owned scopeNeedle")
            .unwrap();
        let owned_body =
            execute_get(&session, LookupToolName::MemoryGet, &owned.reference, None).unwrap();
        assert_eq!(owned_body.state, LookupSourceState::Current);
        assert_eq!(owned_body.body.as_deref(), Some("Owned scopeNeedle body"));

        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                    SET evidence_subject = 'workflow_configuration'
                  WHERE id = 'owned-memory'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let requalified =
            execute_get(&session, LookupToolName::MemoryGet, &owned.reference, None).unwrap();
        assert_eq!(requalified.state, LookupSourceState::Replaced);
        assert!(requalified
            .provenance
            .contains("subject: workflow_configuration"));
        assert!(requalified.body.is_none());

        let linked = found
            .results
            .iter()
            .find(|hit| hit.title == "Linked scopeNeedle")
            .unwrap();
        db.unlink_memory(&workflow_a, "linked-memory").unwrap();
        let unlinked =
            execute_get(&session, LookupToolName::MemoryGet, &linked.reference, None).unwrap();
        assert_eq!(unlinked.state, LookupSourceState::Unlinked);

        let user = found
            .results
            .iter()
            .find(|hit| hit.title == "User scopeNeedle")
            .unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories SET expires_at = '2026-09-09T11:00:00Z',
                 updated_at = '2026-09-09T12:02:00Z' WHERE id = 'user-memory'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let expired =
            execute_get(&session, LookupToolName::MemoryGet, &user.reference, None).unwrap();
        assert_eq!(expired.state, LookupSourceState::Expired);

        let other_session = start_lookup_session(
            db.clone(),
            &directory.path,
            "lookup-run-b",
            "agent-node",
            &workspace_b,
        );
        assert_eq!(
            execute_get(
                &other_session,
                LookupToolName::MemoryGet,
                &owned.reference,
                None
            ),
            Err(LookupServiceError::Contract(
                LookupContractError::InvalidReference
            ))
        );

        db.with_conn(|conn| {
            conn.execute(
                "UPDATE workflows SET memory_retrieval_enabled = 0 WHERE id = ?1",
                [&workflow_a],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(
            session
                .execute_json(
                    LookupToolName::MemorySearch,
                    br#"{"query":"scopeNeedle"}"#,
                    &UninterruptedLookup,
                )
                .is_ok(),
            "automatic recall permission must not authorize or deny lookup reads"
        );
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE workflows SET memory_lookup_enabled = 0 WHERE id = ?1",
                [&workflow_a],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            session.execute_json(
                LookupToolName::MemorySearch,
                br#"{"query":"scopeNeedle"}"#,
                &UninterruptedLookup,
            ),
            Err(LookupServiceError::InvalidScope)
        );
    }

    #[test]
    fn memory_search_filters_expired_hits_before_applying_result_limit() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let workflow = create_lookup_workflow(&db, "Eligibility", &workspace);
        insert_lookup_run(&db, &workflow, "eligibility-run");
        create_lookup_memory(
            &db,
            "expired-eligible-memory",
            &workflow,
            MemoryScopeType::Workflow,
            "eligibleNeedle expired",
            "eligibleNeedle expired body",
        );
        create_lookup_memory(
            &db,
            "current-eligible-memory",
            &workflow,
            MemoryScopeType::Workflow,
            "eligibleNeedle current",
            "eligibleNeedle current body",
        );
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                    SET expires_at = '2026-09-09T11:00:00Z',
                        updated_at = '2026-09-09T12:01:00Z'
                  WHERE id = 'expired-eligible-memory'",
                [],
            )?;
            conn.execute(
                "UPDATE memories
                    SET updated_at = '2026-09-09T11:00:00Z'
                  WHERE id = 'current-eligible-memory'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let session = start_lookup_session(
            db,
            &directory.path,
            "eligibility-run",
            "agent-node",
            &workspace,
        );
        let encoded = br#"{"query":"eligibleNeedle","limit":1}"#;
        let response = session
            .execute_json(LookupToolName::MemorySearch, encoded, &UninterruptedLookup)
            .expect("execute eligible search");
        let response: LookupSearchResponse =
            serde_json::from_str(&response).expect("decode eligible search");
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].title, "eligibleNeedle current");
    }

    #[test]
    fn history_memory_filters_current_rows_before_applying_result_limit() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let workflow = create_lookup_workflow(&db, "History limit", &workspace);
        insert_lookup_run(&db, &workflow, "history-limit-run");
        create_lookup_memory(
            &db,
            "history-current-a",
            &workflow,
            MemoryScopeType::Workflow,
            "historyLimitNeedle current alpha",
            &"historyLimitNeedle ".repeat(32),
        );
        create_lookup_memory(
            &db,
            "history-current-b",
            &workflow,
            MemoryScopeType::Workflow,
            "historyLimitNeedle current beta",
            &"historyLimitNeedle ".repeat(16),
        );
        create_lookup_memory(
            &db,
            "history-historical",
            &workflow,
            MemoryScopeType::Workflow,
            "historyLimitNeedle historical",
            "historyLimitNeedle historical body",
        );
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                    SET status = 'superseded', updated_at = '2026-09-09T09:00:00Z'
                  WHERE id = 'history-historical'",
                [],
            )?;
            conn.execute(
                "UPDATE memories
                    SET updated_at = '2026-09-09T12:00:00Z'
                  WHERE id IN ('history-current-a', 'history-current-b')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let session = start_lookup_session(
            db,
            &directory.path,
            "history-limit-run",
            "agent-node",
            &workspace,
        );
        let response = session
            .execute_json(
                LookupToolName::HistorySearch,
                br#"{"query":"historyLimitNeedle","limit":1}"#,
                &UninterruptedLookup,
            )
            .expect("execute history memory search");
        let response: LookupSearchResponse =
            serde_json::from_str(&response).expect("decode history memory search");
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].title, "historyLimitNeedle historical");
        assert_eq!(response.results[0].state, LookupSourceState::Replaced);
    }

    #[test]
    fn validity_clock_is_sampled_for_each_call_and_natural_expiry_is_revalidated() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let workflow = create_lookup_workflow(&db, "Clock", &workspace);
        insert_lookup_run(&db, &workflow, "clock-run");
        create_lookup_memory(
            &db,
            "expiring-memory",
            &workflow,
            MemoryScopeType::Workflow,
            "clockNeedle",
            "clockNeedle body",
        );
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories SET expires_at = '2026-09-09T10:00:01Z'
                  WHERE id = 'expiring-memory'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let clock = Arc::new(ManualLookupClock::new("2026-09-09T10:00:00Z"));
        let service = AgentMemoryLookupService::with_test_runtime(
            db,
            &directory.path,
            clock.clone(),
            Duration::from_secs(5),
            Arc::new(RegularLookupFileReader),
        );
        let session = service
            .start_session("clock-run", "agent-node", Some(&workspace))
            .unwrap();
        let found = execute_search(&session, LookupToolName::MemorySearch, "clockNeedle");
        assert_eq!(found.results.len(), 1);
        let reference = found.results[0].reference.clone();

        clock.set("2026-09-09T10:00:02Z");
        let expired = execute_get(&session, LookupToolName::MemoryGet, &reference, None).unwrap();
        assert_eq!(expired.state, LookupSourceState::Expired);
        assert!(expired.body.is_none());
        assert!(
            execute_search(&session, LookupToolName::MemorySearch, "clockNeedle")
                .results
                .is_empty()
        );
    }

    #[test]
    fn interior_whitespace_pages_remain_readable_and_bounded() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let workflow = create_lookup_workflow(&db, "Whitespace pages", &workspace);
        insert_lookup_run(&db, &workflow, "whitespace-pages-run");
        let body = format!("whitespaceNeedle{}", " ".repeat(5_000));
        create_lookup_memory(
            &db,
            "whitespace-pages-memory",
            &workflow,
            MemoryScopeType::Workflow,
            "Whitespace page memory",
            &body,
        );

        let session = start_lookup_session(
            db,
            &directory.path,
            "whitespace-pages-run",
            "agent-node",
            &workspace,
        );
        let found = execute_search(&session, LookupToolName::MemorySearch, "whitespaceNeedle");
        let reference = found.results[0].reference.clone();
        let first = execute_get(
            &session,
            LookupToolName::MemoryGet,
            &reference,
            None,
        )
        .unwrap();
        assert_eq!(first.state, LookupSourceState::Current);
        assert_eq!(first.body.as_ref().unwrap().len(), LOOKUP_LIMITS_V1.body_page_bytes);
        let cursor = first.next_cursor.as_deref().expect("next whitespace page");

        let second = execute_get(
            &session,
            LookupToolName::MemoryGet,
            &reference,
            Some(cursor),
        )
        .unwrap();
        assert_eq!(second.state, LookupSourceState::Current);
        assert_eq!(second.body.as_deref(), Some(&body[4_000..]));
        assert!(!second.truncated);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn history_search_never_replays_copied_prompt_or_error_text_as_saved_output() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        db.with_conn(create_output_persistence_schema).unwrap();
        let workflow = create_lookup_workflow(&db, "History output", &workspace);
        let source_workflow = create_lookup_workflow(&db, "Linked source", &workspace);
        insert_lookup_run(&db, &workflow, "prompt-run");
        create_lookup_memory(
            &db,
            "linked-prompt-memory",
            &source_workflow,
            MemoryScopeType::Workflow,
            "Revocable source",
            "revokedLinkedSentinel",
        );
        db.link_memory(&workflow, "linked-prompt-memory").unwrap();

        insert_indexed_step_with_input(
            &db,
            &workflow,
            "prompt-run",
            "prompt-step",
            "agent-node",
            json!({"prompt": "revokedLinkedSentinel", "errorCopy": "privateErrorSentinel"}),
            json!({"text": "safeResultNeedle"}),
            Some("privateErrorSentinel"),
        );
        let identity = OutputStorageIdentity::new();
        let storage_id = identity.memory_id();
        create_lookup_memory(
            &db,
            &storage_id,
            &workflow,
            MemoryScopeType::Workflow,
            "Saved safe output",
            "safeResultNeedle",
        );
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO output_receipts
                 (run_id, node_id, step_id, label, save_requested, pin_memory,
                  memory_title_override, resolved_memory_title, as_final_result,
                  html_report, include_files_changed, state, storage_id,
                  output_sha256, artifact_required, memory_id, created_at, updated_at)
                 VALUES ('prompt-run', 'agent-node', 'prompt-step', 'Output',
                         1, 0, '', 'Saved safe output', 0, 0, 0, 'saved',
                         ?1, ?2, 0, ?1, '2026-09-09T10:02:00Z',
                         '2026-09-09T10:02:00Z')",
                params![storage_id, digest_text("safeResultNeedle")],
            )?;
            Ok(())
        })
        .unwrap();

        db.unlink_memory(&workflow, "linked-prompt-memory").unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories SET status = 'retracted'
                  WHERE id = 'linked-prompt-memory'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let session =
            start_lookup_session(db, &directory.path, "prompt-run", "agent-node", &workspace);
        assert!(execute_search(
            &session,
            LookupToolName::HistorySearch,
            "revokedLinkedSentinel",
        )
        .results
        .is_empty());
        assert!(execute_search(
            &session,
            LookupToolName::HistorySearch,
            "privateErrorSentinel",
        )
        .results
        .is_empty());
        let safe = execute_search(&session, LookupToolName::HistorySearch, "safeResultNeedle");
        assert_eq!(safe.results.len(), 1);
        assert_eq!(safe.results[0].source, LookupSourceKind::RunStep);
        assert!(safe.results[0].provenance.contains("receipt verified"));
        let body = execute_get(
            &session,
            LookupToolName::HistoryGet,
            &safe.results[0].reference,
            None,
        )
        .unwrap();
        assert_eq!(body.body.as_deref(), Some("safeResultNeedle"));
    }

    #[test]
    fn database_lock_wait_and_slow_sql_obey_deadline_without_spending_byte_budget() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let workflow = create_lookup_workflow(&db, "Deadline", &workspace);
        insert_lookup_run(&db, &workflow, "deadline-run");
        let service = AgentMemoryLookupService::with_test_runtime(
            db.clone(),
            &directory.path,
            Arc::new(ManualLookupClock::new("2026-09-09T10:00:00Z")),
            Duration::from_millis(20),
            Arc::new(RegularLookupFileReader),
        );
        let session = service
            .start_session("deadline-run", "agent-node", Some(&workspace))
            .unwrap();

        let (locked_sender, locked_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let locked_db = db.clone();
        let holder = thread::spawn(move || {
            locked_db
                .with_conn(|_conn| {
                    locked_sender.send(()).unwrap();
                    let _ = release_receiver.recv();
                    Ok(())
                })
                .unwrap();
        });
        locked_receiver.recv().unwrap();
        let before = session.budget().unwrap();
        let started = Instant::now();
        let locked_result = session.execute_json(
            LookupToolName::MemorySearch,
            br#"{"query":"deadlineNeedle"}"#,
            &UninterruptedLookup,
        );
        assert_eq!(locked_result, Err(LookupServiceError::TimedOut));
        assert!(started.elapsed() < Duration::from_millis(250));
        let after_lock = session.budget().unwrap();
        assert_eq!(after_lock.calls_used, before.calls_used + 1);
        assert_eq!(after_lock.returned_bytes, before.returned_bytes);
        release_sender.send(()).unwrap();
        holder.join().unwrap();

        let call = LookupCall::begin(&session, &UninterruptedLookup).unwrap();
        let started = Instant::now();
        let slow_sql = lookup_with_conn(&db, &call, |conn| {
            conn.query_row(
                "WITH RECURSIVE numbers(value) AS (
                   VALUES(0) UNION ALL SELECT value + 1 FROM numbers
                   WHERE value < 100000000
                 ) SELECT sum(value) FROM numbers",
                [],
                |_row| Ok(()),
            )?;
            Ok(())
        });
        assert_eq!(slow_sql, Err(LookupServiceError::TimedOut));
        assert!(started.elapsed() < Duration::from_millis(500));
        let after_sql = session.budget().unwrap();
        assert_eq!(after_sql.calls_used, after_lock.calls_used + 1);
        assert_eq!(after_sql.returned_bytes, before.returned_bytes);
    }

    #[test]
    fn external_sqlite_lock_obeys_deadline_and_restores_busy_timeout() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let database_path = directory.path.join("lookup.sqlite");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        connection
            .execute_batch(include_str!("schema.sql"))
            .unwrap();
        super::super::migrate::apply_migrations(&connection).unwrap();
        let db = Arc::new(Db {
            conn: Mutex::new(connection),
            artifact_root: directory.path.clone(),
        });
        let workflow = create_lookup_workflow(&db, "External lock", &workspace);
        insert_lookup_run(&db, &workflow, "external-lock-run");
        let service = AgentMemoryLookupService::with_test_runtime(
            db.clone(),
            &directory.path,
            Arc::new(ManualLookupClock::new("2026-09-09T10:00:00Z")),
            Duration::from_millis(20),
            Arc::new(RegularLookupFileReader),
        );
        let session = service
            .start_session("external-lock-run", "agent-node", Some(&workspace))
            .unwrap();

        let blocker = Connection::open(&database_path).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let before = session.budget().unwrap();
        let started = Instant::now();
        let result = session.execute_json(
            LookupToolName::MemorySearch,
            br#"{"query":"lockedNeedle"}"#,
            &UninterruptedLookup,
        );
        assert_eq!(result, Err(LookupServiceError::TimedOut));
        assert!(started.elapsed() < Duration::from_millis(250));
        let after = session.budget().unwrap();
        assert_eq!(after.calls_used, before.calls_used + 1);
        assert_eq!(after.returned_bytes, before.returned_bytes);
        blocker.execute_batch("ROLLBACK;").unwrap();

        let restored_timeout = db
            .with_conn(|conn| {
                conn.query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
                    .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(restored_timeout, 5_000);
    }

    #[test]
    fn slow_file_read_times_out_with_one_worker_and_cleans_up_after_completion() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let workflow = create_lookup_workflow(&db, "File deadline", &workspace);
        insert_lookup_run(&db, &workflow, "file-deadline-run");
        let service = AgentMemoryLookupService::with_test_runtime(
            db,
            &directory.path,
            Arc::new(ManualLookupClock::new("2026-09-09T10:00:00Z")),
            Duration::from_millis(20),
            Arc::new(SlowLookupFileReader {
                delay: Duration::from_millis(100),
            }),
        );
        let worker_state = service.file_worker_active.clone();
        let session = service
            .start_session("file-deadline-run", "agent-node", Some(&workspace))
            .unwrap();
        let before = session.budget().unwrap();
        let call = LookupCall::begin(&session, &UninterruptedLookup).unwrap();
        let started = Instant::now();
        let result = session.read_file_page(
            directory.path.join("synthetic-file"),
            0,
            LOOKUP_LIMITS_V1.body_page_bytes,
            &call,
        );
        assert!(matches!(result, Err(LookupServiceError::TimedOut)));
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(worker_state.load(Ordering::Acquire));
        let after = session.budget().unwrap();
        assert_eq!(after.calls_used, before.calls_used + 1);
        assert_eq!(after.returned_bytes, before.returned_bytes);
        let second_call = LookupCall::begin(&session, &UninterruptedLookup).unwrap();
        assert!(matches!(
            session.read_file_page(
                directory.path.join("second-synthetic-file"),
                0,
                LOOKUP_LIMITS_V1.body_page_bytes,
                &second_call,
            ),
            Err(LookupServiceError::StorageUnavailable)
        ));
        let after_second = session.budget().unwrap();
        assert_eq!(after_second.calls_used, before.calls_used + 2);
        assert_eq!(after_second.returned_bytes, before.returned_bytes);

        let cleanup_deadline = Instant::now() + Duration::from_millis(500);
        while worker_state.load(Ordering::Acquire) && Instant::now() < cleanup_deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(!worker_state.load(Ordering::Acquire));
        assert!(matches!(
            page_from_regular_file(&directory.path, 0, 16),
            Err(LookupServiceError::StorageUnavailable)
        ));
    }

    #[test]
    fn completed_file_worker_releases_ownership_before_publishing_result() {
        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        let workflow = create_lookup_workflow(&db, "File handoff", &workspace);
        insert_lookup_run(&db, &workflow, "file-handoff-run");
        let published_barrier = Arc::new(Barrier::new(2));
        let service = AgentMemoryLookupService::with_test_runtime(
            db,
            &directory.path,
            Arc::new(ManualLookupClock::new("2026-09-09T10:00:00Z")),
            Duration::from_millis(500),
            Arc::new(HandoffLookupFileReader {
                hold_first_published_worker: AtomicBool::new(true),
                published_barrier: published_barrier.clone(),
            }),
        );
        let worker_state = service.file_worker_active.clone();
        let session = service
            .start_session("file-handoff-run", "agent-node", Some(&workspace))
            .unwrap();

        let first_call = LookupCall::begin(&session, &UninterruptedLookup).unwrap();
        let first = session
            .read_file_page(directory.path.join("first"), 0, 100, &first_call)
            .unwrap();
        assert_eq!(first.body, "handoff body");
        published_barrier.wait();
        assert!(!worker_state.load(Ordering::Acquire));

        let second_call = LookupCall::begin(&session, &UninterruptedLookup).unwrap();
        let second = session
            .read_file_page(directory.path.join("second"), 0, 100, &second_call)
            .unwrap();
        assert_eq!(second.body, "handoff body");
        published_barrier.wait();
    }

    #[test]
    fn receipt_backed_artifact_pages_are_bounded_and_revalidate_between_calls() {
        let directory = LookupTestDirectory::new();
        let workspace_a = directory.path.join("workspace-a");
        let workspace_b = directory.path.join("workspace-b");
        fs::create_dir_all(&workspace_a).unwrap();
        fs::create_dir_all(&workspace_b).unwrap();
        let db = Arc::new(Db::open_in_memory().expect("open lookup database"));
        db.with_conn(create_output_persistence_schema).unwrap();
        let workflow_a = create_lookup_workflow(&db, "History A", &workspace_a);
        let workflow_b = create_lookup_workflow(&db, "History B", &workspace_b);
        insert_lookup_run(&db, &workflow_a, "history-run-a");
        insert_lookup_run(&db, &workflow_b, "history-run-b");
        insert_indexed_step(
            &db,
            &workflow_a,
            "history-run-a",
            "history-step-a",
            "output-node",
            json!({"text": "artifactNeedle output"}),
        );

        let identity = OutputStorageIdentity::new();
        let storage_id = identity.memory_id();
        let body = format!("artifactNeedle {}", "é".repeat(400_000));
        let artifact_path = directory.path.join(identity.artifact_relative_path());
        fs::create_dir_all(artifact_path.parent().unwrap()).unwrap();
        fs::write(&artifact_path, &body).unwrap();
        create_lookup_memory(
            &db,
            &storage_id,
            &workflow_a,
            MemoryScopeType::Workflow,
            "Saved artifact output",
            "artifactNeedle bounded preview",
        );
        configure_lookup_artifact(&db, &storage_id, &artifact_path, &body);
        let output_digest = digest_text(&body);
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO output_receipts
                 (run_id, node_id, step_id, label, save_requested, pin_memory,
                  memory_title_override, resolved_memory_title, as_final_result,
                  html_report, include_files_changed, state, storage_id,
                  output_sha256, artifact_required, memory_id, created_at, updated_at)
                 VALUES ('history-run-a', 'output-node', 'history-step-a', 'Output',
                         1, 0, '', 'Saved artifact output', 0, 0, 0, 'saved',
                         ?1, ?2, 1, ?1, '2026-09-09T10:02:00Z',
                         '2026-09-09T10:02:00Z')",
                params![storage_id, output_digest],
            )?;
            Ok(())
        })
        .unwrap();

        let session = start_lookup_session(
            db.clone(),
            &directory.path,
            "history-run-a",
            "agent-node",
            &workspace_a,
        );
        let found = execute_search(&session, LookupToolName::HistorySearch, "artifactNeedle");
        let step = found
            .results
            .iter()
            .find(|hit| hit.source == LookupSourceKind::RunStep)
            .expect("find run-step hit");
        let first =
            execute_get(&session, LookupToolName::HistoryGet, &step.reference, None).unwrap();
        assert_eq!(first.state, LookupSourceState::Historical);
        assert!(first.body.as_ref().unwrap().len() <= LOOKUP_LIMITS_V1.body_page_bytes);
        assert!(first.body.as_ref().unwrap().ends_with('é'));
        assert!(first.truncated);
        let cursor = first.next_cursor.clone().expect("next artifact cursor");
        let second =
            execute_get(&session, LookupToolName::HistoryGet, &step.reference, Some(&cursor))
                .unwrap();
        let first_body = first.body.as_deref().unwrap();
        let second_body = second.body.as_deref().unwrap();
        assert!(second_body.starts_with('é'));
        assert!(second_body.ends_with('é'));
        let reconstructed_len = first_body.len() + second_body.len();
        assert_eq!(
            format!("{first_body}{second_body}"),
            body[..reconstructed_len]
        );
        assert!(second.next_cursor.is_some());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outputs_path = directory.path.join("outputs");
            let outside_outputs = directory.path.join("outputs-target");
            fs::rename(&outputs_path, &outside_outputs).unwrap();
            symlink(&outside_outputs, &outputs_path).unwrap();
            let escaped = execute_get(
                &session,
                LookupToolName::HistoryGet,
                &step.reference,
                None,
            )
            .unwrap();
            assert_eq!(escaped.state, LookupSourceState::Unavailable);
            assert!(escaped.body.is_none());
            fs::remove_file(&outputs_path).unwrap();
            fs::rename(&outside_outputs, &outputs_path).unwrap();
        }

        let other_session = start_lookup_session(
            db.clone(),
            &directory.path,
            "history-run-b",
            "agent-node",
            &workspace_b,
        );
        assert_eq!(
            execute_get(
                &other_session,
                LookupToolName::HistoryGet,
                &step.reference,
                None
            ),
            Err(LookupServiceError::Contract(
                LookupContractError::InvalidReference
            ))
        );

        fs::remove_file(&artifact_path).unwrap();
        let missing = execute_get(
            &session,
            LookupToolName::HistoryGet,
            &step.reference,
            Some(&cursor),
        )
        .unwrap();
        assert_eq!(missing.state, LookupSourceState::Unavailable);
        assert!(missing.body.is_none());

        thread::sleep(Duration::from_millis(5));
        fs::write(&artifact_path, "z".repeat(body.len())).unwrap();
        let changed = execute_get(
            &session,
            LookupToolName::HistoryGet,
            &step.reference,
            Some(&cursor),
        )
        .unwrap();
        assert_eq!(changed.state, LookupSourceState::Unavailable);
        assert!(changed.body.is_none());
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE output_receipts
                    SET state = 'deleted', memory_id = NULL,
                        deleted_at = '2026-09-09T12:03:00Z',
                        updated_at = '2026-09-09T12:03:00Z'
                  WHERE run_id = 'history-run-a' AND node_id = 'output-node'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let deleted = execute_get(
            &session,
            LookupToolName::HistoryGet,
            &step.reference,
            None,
        )
        .unwrap();
        assert_eq!(deleted.state, LookupSourceState::Deleted);
        assert!(deleted.body.is_none());
    }


    #[test]
    fn promoted_generic_artifact_remains_readable_after_origin_workflow_deletion() {
        let directory = LookupTestDirectory::new();
        let workspace_a = directory.path.join("workspace-a");
        let workspace_b = directory.path.join("workspace-b");
        fs::create_dir_all(&workspace_a).unwrap();
        fs::create_dir_all(&workspace_b).unwrap();
        let db = Arc::new(
            Db::open_in_memory_with_artifact_root(directory.path.clone())
                .expect("open lookup database"),
        );
        let workflow_a = create_lookup_workflow(&db, "Generic origin", &workspace_a);
        let workflow_b = create_lookup_workflow(&db, "Generic destination", &workspace_b);
        insert_lookup_run(&db, &workflow_b, "generic-destination-run");

        let memory_id = Uuid::new_v4().to_string();
        let artifact_name = format!("{}.txt", Uuid::new_v4());
        let artifact_path = directory
            .path
            .join("artifacts")
            .join(&workflow_a)
            .join(artifact_name);
        fs::create_dir_all(artifact_path.parent().unwrap()).unwrap();
        let body = "promotedGenericNeedle ".to_owned() + &"界".repeat(500);
        fs::write(&artifact_path, &body).unwrap();
        create_lookup_memory(
            &db,
            &memory_id,
            &workflow_a,
            MemoryScopeType::Workflow,
            "Promoted generic artifact",
            "promotedGenericNeedle preview",
        );
        configure_lookup_artifact(&db, &memory_id, &artifact_path, &body);

        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                    SET workflow_id = NULL, scope_type = 'user', scope_key = 'local-user'
                  WHERE id = ?1",
                [&memory_id],
            )?;
            conn.execute("DELETE FROM workflows WHERE id = ?1", [&workflow_a])?;
            Ok(())
        })
        .unwrap();

        let session = start_lookup_session(
            db,
            &directory.path,
            "generic-destination-run",
            "agent-node",
            &workspace_b,
        );
        let found = execute_search(
            &session,
            LookupToolName::MemorySearch,
            "promotedGenericNeedle",
        );
        let hit = found
            .results
            .iter()
            .find(|hit| hit.title == "Promoted generic artifact")
            .expect("find promoted generic artifact");
        let page =
            execute_get(&session, LookupToolName::MemoryGet, &hit.reference, None).unwrap();
        assert_eq!(page.state, LookupSourceState::Current);
        assert_eq!(page.body.as_deref(), Some(body.as_str()));
    }

    #[test]
    fn promoted_output_artifact_remains_readable_after_origin_receipt_deletion() {
        let directory = LookupTestDirectory::new();
        let workspace_a = directory.path.join("workspace-a");
        let workspace_b = directory.path.join("workspace-b");
        fs::create_dir_all(&workspace_a).unwrap();
        fs::create_dir_all(&workspace_b).unwrap();
        let db = Arc::new(
            Db::open_in_memory_with_artifact_root(directory.path.clone())
                .expect("open lookup database"),
        );
        db.with_conn(create_output_persistence_schema).unwrap();
        let workflow_a = create_lookup_workflow(&db, "Output origin", &workspace_a);
        let workflow_b = create_lookup_workflow(&db, "Output destination", &workspace_b);
        insert_lookup_run(&db, &workflow_a, "output-origin-run");
        insert_lookup_run(&db, &workflow_b, "output-destination-run");
        insert_indexed_step(
            &db,
            &workflow_a,
            "output-origin-run",
            "output-origin-step",
            "output-node",
            json!({"text": "promotedOutputNeedle"}),
        );

        let identity = OutputStorageIdentity::new();
        let storage_id = identity.memory_id();
        let artifact_path = directory.path.join(identity.artifact_relative_path());
        fs::create_dir_all(artifact_path.parent().unwrap()).unwrap();
        let body = "promotedOutputNeedle ".to_owned() + &"x".repeat(7_000);
        fs::write(&artifact_path, &body).unwrap();
        create_lookup_memory(
            &db,
            &storage_id,
            &workflow_a,
            MemoryScopeType::Workflow,
            "Promoted output artifact",
            "promotedOutputNeedle preview",
        );
        configure_lookup_artifact(&db, &storage_id, &artifact_path, &body);
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO output_receipts
                 (run_id, node_id, step_id, label, save_requested, pin_memory,
                  memory_title_override, resolved_memory_title, as_final_result,
                  html_report, include_files_changed, state, storage_id,
                  output_sha256, artifact_required, memory_id, created_at, updated_at)
                 VALUES ('output-origin-run', 'output-node', 'output-origin-step', 'Output',
                         1, 0, '', 'Promoted output artifact', 0, 0, 0, 'saved',
                         ?1, ?2, 1, ?1, '2026-09-09T10:02:00Z',
                         '2026-09-09T10:02:00Z')",
                params![storage_id, digest_text(&body)],
            )?;
            conn.execute(
                "UPDATE memories
                    SET workflow_id = NULL, scope_type = 'user', scope_key = 'local-user'
                  WHERE id = ?1",
                [&storage_id],
            )?;
            conn.execute(
                "DELETE FROM output_receipts WHERE run_id = 'output-origin-run'",
                [],
            )?;
            conn.execute("DELETE FROM workflows WHERE id = ?1", [&workflow_a])?;
            Ok(())
        })
        .unwrap();

        let session = start_lookup_session(
            db,
            &directory.path,
            "output-destination-run",
            "agent-node",
            &workspace_b,
        );
        let found = execute_search(
            &session,
            LookupToolName::MemorySearch,
            "promotedOutputNeedle",
        );
        let hit = found
            .results
            .iter()
            .find(|hit| hit.title == "Promoted output artifact")
            .expect("find promoted output artifact");
        let page =
            execute_get(&session, LookupToolName::MemoryGet, &hit.reference, None).unwrap();
        assert_eq!(page.state, LookupSourceState::Current);
        assert_eq!(page.body.as_deref(), Some(&body[..4_000]));
        assert!(page.truncated);
    }

    #[test]
    fn evidence_pages_share_budgets_trace_as_data_and_honor_cancellation() {
        struct Cancelled;
        impl LookupCallControl for Cancelled {
            fn checkpoint(&self) -> Result<(), LookupAbort> {
                Err(LookupAbort::Cancelled)
            }
        }

        let directory = LookupTestDirectory::new();
        let workspace = directory.path.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let db = Arc::new(Db::open_in_memory().expect("open lookup database"));
        let workflow = create_lookup_workflow(&db, "Evidence", &workspace);
        insert_lookup_run(&db, &workflow, "evidence-run");
        let text = format!("evidenceNeedle beta {}", "界".repeat(5_300));
        let evidence = db
            .capture_run_evidence_sources(vec![EvidenceSourceInput {
                run_id: "evidence-run".into(),
                step_id: None,
                node_id: Some("source-node".into()),
                origin: EvidenceOrigin::HostRecordedObservation,
                producer_identity: Some("host-fixture".into()),
                source_label: Some("Recorded observation".into()),
                observed_at: DateTime::parse_from_rfc3339("2026-09-09T10:03:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                text,
                omission_reason: None,
            }])
            .expect("capture evidence")
            .remove(0);
        let alpha_only = db
            .capture_run_evidence_sources(vec![EvidenceSourceInput {
                run_id: "evidence-run".into(),
                step_id: None,
                node_id: Some("alpha-only-node".into()),
                origin: EvidenceOrigin::HostRecordedObservation,
                producer_identity: Some("host-fixture".into()),
                source_label: Some("Alpha-only observation".into()),
                observed_at: DateTime::parse_from_rfc3339("2026-09-09T10:03:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                text: "evidenceNeedle alphaOnly".into(),
                omission_reason: None,
            }])
            .expect("capture alpha-only evidence")
            .remove(0);

        let session = start_lookup_session(
            db.clone(),
            &directory.path,
            "evidence-run",
            "agent-node",
            &workspace,
        );
        let found = execute_search(
            &session,
            LookupToolName::HistorySearch,
            "evidenceNeedle beta",
        );
        let search_trace = session
            .traces()
            .unwrap()
            .into_iter()
            .next()
            .expect("record history search trace");
        assert!(search_trace.sources.iter().any(|source| {
            source.source == LookupSourceKind::Evidence
                && source.source_id == evidence.source_id
                && source.version.starts_with("evidence-v1:")
                && source.page.is_none()
        }));
        assert!(search_trace
            .sources
            .iter()
            .all(|source| source.source_id != alpha_only.source_id));
        let hit = found
            .results
            .iter()
            .find(|hit| hit.source == LookupSourceKind::Evidence)
            .expect("find evidence hit");
        let mut cursor = None;
        for _ in 0..3 {
            let page = execute_get(
                &session,
                LookupToolName::HistoryGet,
                &hit.reference,
                cursor.as_deref(),
            )
            .expect("read evidence page within budget");
            assert!(page.body.as_ref().unwrap().len() <= 4_000);
            cursor = page.next_cursor;
        }
        let page_trace = session
            .traces()
            .unwrap()
            .into_iter()
            .last()
            .expect("record history get trace");
        assert_eq!(page_trace.sources.len(), 1);
        assert_eq!(page_trace.sources[0].reference, hit.reference);
        assert_eq!(page_trace.sources[0].source_id, evidence.source_id);
        assert_eq!(page_trace.sources[0].page, Some(2));
        let before = session.budget().unwrap().returned_bytes();
        assert_eq!(
            execute_get(
                &session,
                LookupToolName::HistoryGet,
                &hit.reference,
                cursor.as_deref()
            ),
            Err(LookupServiceError::Contract(
                LookupContractError::StepByteBudgetExhausted
            ))
        );
        assert_eq!(session.budget().unwrap().returned_bytes(), before);
        assert!(session.traces().unwrap().iter().all(|trace| {
            trace.classification == LookupTraceClassification::RetrievedData
                && trace.returned_bytes > 0
        }));

        let mutation_session = start_lookup_session(
            db.clone(),
            &directory.path,
            "evidence-run",
            "mutation-node",
            &workspace,
        );
        let found = execute_search(
            &mutation_session,
            LookupToolName::HistorySearch,
            "evidenceNeedle beta",
        );
        let hit = found
            .results
            .iter()
            .find(|hit| hit.source == LookupSourceKind::Evidence)
            .unwrap();
        let first = execute_get(
            &mutation_session,
            LookupToolName::HistoryGet,
            &hit.reference,
            None,
        )
        .unwrap();
        let changed_text = format!("evidenceNeedle changed {}", "界".repeat(5_299));
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE run_evidence_sources
                    SET normalized_text = ?1, content_digest = ?2
                  WHERE id = ?3",
                params![changed_text, digest_text(&changed_text), evidence.source_id],
            )?;
            Ok(())
        })
        .unwrap();
        let replaced = execute_get(
            &mutation_session,
            LookupToolName::HistoryGet,
            &hit.reference,
            first.next_cursor.as_deref(),
        )
        .unwrap();
        assert_eq!(replaced.state, LookupSourceState::Replaced);

        let cancelled_session = start_lookup_session(
            db,
            &directory.path,
            "evidence-run",
            "cancel-node",
            &workspace,
        );
        assert_eq!(
            cancelled_session.execute_json(
                LookupToolName::HistorySearch,
                br#"{"query":"evidenceNeedle beta"}"#,
                &Cancelled
            ),
            Err(LookupServiceError::Cancelled)
        );
        assert_eq!(cancelled_session.budget().unwrap().calls_used(), 1);
    }

    fn assert_search_hit_has_compatible_get(
        search_tool: LookupToolName,
        get_tool: LookupToolName,
        source: LookupSourceKind,
        state: LookupSourceState,
    ) {
        let readable = state.may_return_body();
        let hit = LookupSearchHit {
            reference: "opaque_ref".into(),
            source,
            state,
            title: "Source title".into(),
            provenance: "bounded provenance".into(),
            excerpt: if readable {
                "bounded excerpt".into()
            } else {
                String::new()
            },
            observed_at: Some("2026-09-09T12:00:00Z".into()),
            truncated: false,
        };
        LookupSearchResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool: search_tool,
            results: vec![hit],
            truncated: false,
        }
        .validate()
        .expect("search hit accepted by the contract");

        LookupGetResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool: get_tool,
            reference: "opaque_ref".into(),
            source,
            state,
            source_version: "version_1".into(),
            page: 0,
            provenance: "bounded provenance".into(),
            body: readable.then(|| "bounded body".into()),
            truncated: false,
            next_cursor: None,
        }
        .validate()
        .expect("accepted search hit has a compatible get response");
    }

    #[test]
    fn v1_limits_and_tool_names_are_frozen() {
        assert_eq!(LOOKUP_CONTRACT_VERSION, 1);
        assert_eq!(
            LOOKUP_LIMITS_V1,
            LookupLimits {
                request_bytes: 4_096,
                query_bytes: 2_000,
                query_terms: 24,
                query_term_chars: 64,
                time_bound_bytes: 64,
                reference_bytes: 256,
                cursor_bytes: 512,
                search_results: 8,
                title_bytes: 256,
                provenance_bytes: 512,
                excerpt_bytes: 1_200,
                search_response_bytes: 6_000,
                body_page_bytes: 4_000,
                get_response_bytes: 6_000,
                calls_per_step: 8,
                returned_bytes_per_step: 16_000,
                read_deadline_ms: 5_000,
            }
        );
        assert_eq!(
            serde_json::to_string(&LookupToolName::MemorySearch).unwrap(),
            r#""memory.search""#
        );
        assert_eq!(
            serde_json::to_string(&LookupToolName::HistoryGet).unwrap(),
            r#""history.get""#
        );
    }

    #[test]
    fn request_schemas_reject_unknown_and_malformed_fields() {
        let valid = MemorySearchRequest::decode_json(br#"{"query":"release","limit":8}"#)
            .expect("valid request");
        assert_eq!(valid.limit, Some(8));

        for malformed in [
            br#"{"query":"release","workflowId":"other"}"#.as_slice(),
            br#"{"query":7}"#.as_slice(),
            br#"{"query":"release"#.as_slice(),
        ] {
            assert_eq!(
                MemorySearchRequest::decode_json(malformed),
                Err(LookupContractError::MalformedRequest)
            );
        }

        assert_eq!(
            MemoryGetRequest::decode_json(br#"{"reference":"ref","path":"/tmp/db"}"#),
            Err(LookupContractError::MalformedRequest)
        );
        assert_eq!(
            HistorySearchRequest::decode_json(
                br#"{"query":"release","timeFilter":{"from":"2026-01-01T00:00:00Z","extra":true}}"#
            ),
            Err(LookupContractError::MalformedRequest)
        );
        assert_eq!(
            HistoryGetRequest::decode_json(br#"{"reference":"ref","runId":"other"}"#),
            Err(LookupContractError::MalformedRequest)
        );

        let encoded = br#"{"query":"release"}"#;
        let mut exact_request = encoded.to_vec();
        exact_request.resize(LOOKUP_LIMITS_V1.request_bytes, b' ');
        MemorySearchRequest::decode_json(&exact_request).expect("exact serialized request limit");
        exact_request.push(b' ');
        assert_eq!(
            MemorySearchRequest::decode_json(&exact_request),
            Err(LookupContractError::RequestTooLarge)
        );
    }

    #[test]
    fn queries_enforce_utf8_bytes_unicode_terms_and_serialized_request_bytes() {
        let mut exact_byte_terms = (0..7).map(|_| "🦀".repeat(64)).collect::<Vec<_>>();
        exact_byte_terms.push(format!("{}a", "🦀".repeat(50)));
        let exact_bytes = exact_byte_terms.join(" ");
        assert_eq!(exact_bytes.len(), LOOKUP_LIMITS_V1.query_bytes);
        MemorySearchRequest {
            query: exact_bytes,
            limit: Some(1),
        }
        .validate()
        .expect("exact query byte limit");

        let exact_terms = (0..24)
            .map(|_| "a".repeat(64))
            .collect::<Vec<_>>()
            .join(" ");
        HistorySearchRequest {
            query: exact_terms,
            time_filter: None,
            limit: None,
        }
        .validate()
        .expect("exact term limits");

        assert_eq!(
            MemorySearchRequest {
                query: "é".repeat(1_001),
                limit: None,
            }
            .validate(),
            Err(LookupContractError::QueryTooLarge)
        );
        assert_eq!(
            MemorySearchRequest {
                query: (0..25).map(|_| "a").collect::<Vec<_>>().join(" "),
                limit: None,
            }
            .validate(),
            Err(LookupContractError::TooManyQueryTerms)
        );
        assert_eq!(
            MemorySearchRequest {
                query: "界".repeat(65),
                limit: None,
            }
            .validate(),
            Err(LookupContractError::QueryTermTooLong)
        );
        assert_eq!(
            MemorySearchRequest {
                query: "release".into(),
                limit: Some(9),
            }
            .validate(),
            Err(LookupContractError::InvalidResultLimit)
        );
        assert_eq!(
            MemorySearchRequest {
                query: "release".into(),
                limit: Some(0),
            }
            .validate(),
            Err(LookupContractError::InvalidResultLimit)
        );
        assert_eq!(
            MemorySearchRequest {
                query: " \n\t ".into(),
                limit: None,
            }
            .validate(),
            Err(LookupContractError::EmptyQuery)
        );

        let escaped = serde_json::to_vec(&MemorySearchRequest {
            query: "\u{0001}".repeat(1_000),
            limit: None,
        })
        .unwrap();
        assert!(escaped.len() > LOOKUP_LIMITS_V1.request_bytes);
        assert_eq!(
            MemorySearchRequest::decode_json(&escaped),
            Err(LookupContractError::RequestTooLarge)
        );
    }

    #[test]
    fn time_reference_and_cursor_boundaries_are_exact() {
        let exact_time = format!("2026-09-09T12:00:00.{}Z", "1".repeat(43));
        assert_eq!(exact_time.len(), LOOKUP_LIMITS_V1.time_bound_bytes);
        LookupTimeFilter {
            from: Some(exact_time),
            to: None,
        }
        .validate()
        .expect("exact RFC3339 time-bound limit");

        HistorySearchRequest {
            query: "release".into(),
            time_filter: Some(LookupTimeFilter {
                from: Some("2026-09-09T10:00:00+02:00".into()),
                to: Some("2026-09-09T11:00:00+02:00".into()),
            }),
            limit: Some(8),
        }
        .validate()
        .expect("ordered RFC3339 range");

        for filter in [
            LookupTimeFilter {
                from: None,
                to: None,
            },
            LookupTimeFilter {
                from: Some("2026-09-09".into()),
                to: None,
            },
            LookupTimeFilter {
                from: Some("2026-09-09T12:00:00Z".into()),
                to: Some("2026-09-09T11:00:00Z".into()),
            },
            LookupTimeFilter {
                from: Some("x".repeat(65)),
                to: None,
            },
        ] {
            assert_eq!(
                filter.validate(),
                Err(LookupContractError::InvalidTimeFilter)
            );
        }

        MemoryGetRequest {
            reference: "é".repeat(128),
            cursor: Some("界".repeat(170) + "aa"),
        }
        .validate()
        .expect("exact UTF-8 reference and cursor limits");
        assert_eq!(
            HistoryGetRequest {
                reference: "é".repeat(129),
                cursor: None,
            }
            .validate(),
            Err(LookupContractError::InvalidReference)
        );
        assert_eq!(
            HistoryGetRequest {
                reference: "ref".into(),
                cursor: Some("界".repeat(171)),
            }
            .validate(),
            Err(LookupContractError::InvalidCursor)
        );
    }

    #[test]
    fn search_response_counts_fields_and_serialized_overhead() {
        let mut hit = search_hit();
        hit.reference = "é".repeat(128);
        hit.title = "é".repeat(128);
        hit.provenance = "界".repeat(170) + "aa";
        hit.excerpt = "界".repeat(400);
        let response = LookupSearchResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool: LookupToolName::MemorySearch,
            results: vec![hit.clone()],
            truncated: false,
        };
        let bytes = response.validate().expect("exact field byte limits");
        assert_eq!(bytes, serde_json::to_vec(&response).unwrap().len());

        hit.reference = "ref".into();
        let oversized = LookupSearchResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool: LookupToolName::MemorySearch,
            results: vec![hit; 8],
            truncated: true,
        };
        assert!(serde_json::to_vec(&oversized).unwrap().len() > 6_000);
        assert_eq!(
            oversized.validate(),
            Err(LookupContractError::ResponseTooLarge)
        );

        let mut too_many = response.clone();
        too_many.results = vec![search_hit(); 9];
        assert_eq!(
            too_many.validate(),
            Err(LookupContractError::InvalidResponse)
        );

        for (field, error) in [
            (
                LookupSearchHit {
                    title: "界".repeat(86),
                    ..search_hit()
                },
                LookupContractError::ResponseFieldTooLarge("title"),
            ),
            (
                LookupSearchHit {
                    provenance: "界".repeat(171),
                    ..search_hit()
                },
                LookupContractError::ResponseFieldTooLarge("provenance"),
            ),
            (
                LookupSearchHit {
                    excerpt: "界".repeat(401),
                    ..search_hit()
                },
                LookupContractError::ResponseFieldTooLarge("excerpt"),
            ),
        ] {
            assert_eq!(
                LookupSearchResponse {
                    contract_version: LOOKUP_CONTRACT_VERSION,
                    tool: LookupToolName::MemorySearch,
                    results: vec![field],
                    truncated: false,
                }
                .validate(),
                Err(error)
            );
        }

        let mut wrong_state = search_hit();
        wrong_state.state = LookupSourceState::Historical;
        assert_eq!(
            LookupSearchResponse {
                contract_version: LOOKUP_CONTRACT_VERSION,
                tool: LookupToolName::MemorySearch,
                results: vec![wrong_state],
                truncated: false,
            }
            .validate(),
            Err(LookupContractError::InvalidResponse)
        );

        let mut wrong_source = search_hit();
        wrong_source.source = LookupSourceKind::Evidence;
        assert_eq!(
            LookupSearchResponse {
                contract_version: LOOKUP_CONTRACT_VERSION,
                tool: LookupToolName::MemorySearch,
                results: vec![wrong_source],
                truncated: false,
            }
            .validate(),
            Err(LookupContractError::InvalidResponse)
        );

        let mut inactive = search_hit();
        inactive.source = LookupSourceKind::Evidence;
        inactive.state = LookupSourceState::Expired;
        inactive.excerpt.clear();
        LookupSearchResponse {
            contract_version: LOOKUP_CONTRACT_VERSION,
            tool: LookupToolName::HistorySearch,
            results: vec![inactive.clone()],
            truncated: false,
        }
        .validate()
        .expect("history search preserves an inactive status");
        inactive.observed_at = None;
        assert_eq!(
            LookupSearchResponse {
                contract_version: LOOKUP_CONTRACT_VERSION,
                tool: LookupToolName::HistorySearch,
                results: vec![inactive],
                truncated: false,
            }
            .validate(),
            Err(LookupContractError::InvalidResponse)
        );

        let mut empty_title = search_hit();
        empty_title.title = " ".into();
        assert_eq!(
            LookupSearchResponse {
                contract_version: LOOKUP_CONTRACT_VERSION,
                tool: LookupToolName::MemorySearch,
                results: vec![empty_title],
                truncated: false,
            }
            .validate(),
            Err(LookupContractError::InvalidResponse)
        );

        let malformed = serde_json::to_vec(&serde_json::json!({
            "contractVersion": 1,
            "tool": "memory.search",
            "results": [],
            "truncated": false,
            "metadata": {"path": "/tmp/private"}
        }))
        .unwrap();
        assert!(serde_json::from_slice::<LookupSearchResponse>(&malformed).is_err());
    }

    #[test]
    fn every_accepted_search_source_and_state_has_a_compatible_get_contract() {
        assert_search_hit_has_compatible_get(
            LookupToolName::MemorySearch,
            LookupToolName::MemoryGet,
            LookupSourceKind::Memory,
            LookupSourceState::Current,
        );

        let historical_states = [
            LookupSourceState::Historical,
            LookupSourceState::Incomplete,
            LookupSourceState::Unavailable,
            LookupSourceState::Deleted,
            LookupSourceState::Expired,
            LookupSourceState::Retracted,
            LookupSourceState::Replaced,
            LookupSourceState::Unlinked,
        ];
        for source in [
            LookupSourceKind::Memory,
            LookupSourceKind::RunStep,
            LookupSourceKind::Evidence,
        ] {
            for state in historical_states {
                assert_search_hit_has_compatible_get(
                    LookupToolName::HistorySearch,
                    LookupToolName::HistoryGet,
                    source,
                    state,
                );
            }

            let mut current = search_hit();
            current.source = source;
            assert_eq!(
                LookupSearchResponse {
                    contract_version: LOOKUP_CONTRACT_VERSION,
                    tool: LookupToolName::HistorySearch,
                    results: vec![current],
                    truncated: false,
                }
                .validate(),
                Err(LookupContractError::InvalidResponse)
            );
        }
    }

    #[test]
    fn get_response_enforces_page_state_cursor_and_whole_envelope() {
        let mut response = get_response();
        response.body = Some("界".repeat(1_333) + "a");
        response.provenance = "é".repeat(256);
        let bytes = response
            .validate()
            .expect("exact body and provenance limits");
        assert_eq!(bytes, serde_json::to_vec(&response).unwrap().len());
        assert!(bytes <= LOOKUP_LIMITS_V1.get_response_bytes);

        response.body = Some("界".repeat(1_334));
        assert_eq!(
            response.validate(),
            Err(LookupContractError::ResponseFieldTooLarge("body"))
        );

        let mut unavailable = get_response();
        unavailable.state = LookupSourceState::Unavailable;
        assert_eq!(
            unavailable.validate(),
            Err(LookupContractError::InvalidResponse)
        );
        unavailable.body = None;
        unavailable.provenance = "artifact unavailable".into();
        unavailable
            .validate()
            .expect("explicit unavailable response");

        let mut history = get_response();
        history.tool = LookupToolName::HistoryGet;
        history.source = LookupSourceKind::Evidence;
        history.state = LookupSourceState::Historical;
        history.validate().expect("historical evidence response");
        history.source = LookupSourceKind::Memory;
        history.validate().expect("historical memory response");
        history.state = LookupSourceState::Current;
        assert_eq!(
            history.validate(),
            Err(LookupContractError::InvalidResponse)
        );

        let mut empty_provenance = get_response();
        empty_provenance.provenance = " ".into();
        assert_eq!(
            empty_provenance.validate(),
            Err(LookupContractError::InvalidResponse)
        );

        let mut empty_body = get_response();
        empty_body.body = Some(String::new());
        assert_eq!(
            empty_body.validate(),
            Err(LookupContractError::InvalidResponse)
        );

        let mut truncated = get_response();
        truncated.truncated = true;
        assert_eq!(
            truncated.validate(),
            Err(LookupContractError::InvalidResponse)
        );
        truncated.next_cursor = Some("cursor_2".into());
        truncated.validate().expect("cursor-backed truncated page");

        let mut escaped = get_response();
        escaped.body = Some("\u{0001}".repeat(1_000));
        escaped.provenance = "\u{0002}".repeat(500);
        assert!(serde_json::to_vec(&escaped).unwrap().len() > 6_000);
        assert_eq!(
            escaped.validate(),
            Err(LookupContractError::ResponseTooLarge)
        );
    }

    #[test]
    fn per_step_accounting_uses_actual_returned_bytes() {
        let mut budget = LookupStepBudget::default();
        for _ in 0..LOOKUP_LIMITS_V1.calls_per_step {
            budget.begin_call().expect("call within budget");
        }
        assert_eq!(budget.calls_used(), 8);
        assert_eq!(
            budget.begin_call(),
            Err(LookupContractError::StepCallBudgetExhausted)
        );

        budget
            .record_returned_bytes(LOOKUP_LIMITS_V1.returned_bytes_per_step)
            .expect("exact byte budget");
        assert_eq!(budget.returned_bytes(), 16_000);
        assert_eq!(
            budget.record_returned_bytes(1),
            Err(LookupContractError::StepByteBudgetExhausted)
        );
        assert_eq!(budget.returned_bytes(), 16_000);

        budget.release_returned_bytes(6_000);
        assert_eq!(budget.returned_bytes(), 10_000);
        budget.release_returned_bytes(usize::MAX);
        assert_eq!(budget.returned_bytes(), 0);
        assert_eq!(budget.calls_used(), 8);
    }

    #[test]
    fn utf8_prefix_matches_plan_048_byte_accounting() {
        assert_eq!(utf8_prefix("abc", 2), "ab");
        assert_eq!(utf8_prefix("éé", 1), "");
        assert_eq!(utf8_prefix("éé", 2), "é");
        assert_eq!(utf8_prefix("界a", 3), "界");
        assert!(std::str::from_utf8(utf8_prefix("界界", 5).as_bytes()).is_ok());
    }
}
