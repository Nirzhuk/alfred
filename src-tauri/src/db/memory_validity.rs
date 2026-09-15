//! Shared contracts for canonical-memory validity and optimistic revisions.
//!
//! Integration points are deliberately narrow:
//!
//! - canonical inserts start at [`INITIAL_REVISION`];
//! - every canonical writer classifies its before/after values with
//!   [`semantic_revision_reasons`] and calls [`next_revision`] in the same
//!   transaction as the write;
//! - target snapshots use [`canonical_content_digest`] and store the digest
//!   version beside the digest;
//! - retrieval, explicit selections, pins, duplicate checks, and approval use
//!   [`current_memory_validity`] with their supplied clock;
//! - candidate edits, evidence replacement, and state transitions classify
//!   their before/after values with [`candidate_revision_reasons`]. Operation
//!   and target identity are absent from the editable snapshot because those
//!   fields are immutable after insertion.
//!
//! Revision audit stamps and deletion tombstones may persist
//! [`RevisionReasonSet`]. They must not persist before/after values, claim text,
//! artifact paths, quotes, rationales, or content digests. This keeps the
//! retained record useful for concurrency diagnosis without retaining deleted
//! source material.

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

pub(crate) const INITIAL_REVISION: i64 = 1;
pub(crate) const CANONICAL_CONTENT_DIGEST_VERSION: u32 = 1;
pub(crate) const ARTIFACT_CONTENT_DIGEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValidityState {
    Current,
    Inactive,
    NotStarted,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryDateError {
    InvalidObservedAt,
    InvalidLastConfirmedAt,
    InvalidValidFrom,
    InvalidExpiresAt,
    InvalidValidityInterval,
}

impl MemoryDateError {
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::InvalidObservedAt => "invalid_observed_at",
            Self::InvalidLastConfirmedAt => "invalid_last_confirmed_at",
            Self::InvalidValidFrom => "invalid_valid_from",
            Self::InvalidExpiresAt => "invalid_expires_at",
            Self::InvalidValidityInterval => "invalid_validity_interval",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedMemoryDates {
    pub(crate) observed_at: Option<String>,
    pub(crate) last_confirmed_at: Option<String>,
    pub(crate) valid_from: Option<String>,
    pub(crate) expires_at: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MemoryDatesInput<'a> {
    pub(crate) observed_at: Option<&'a str>,
    pub(crate) last_confirmed_at: Option<&'a str>,
    pub(crate) valid_from: Option<&'a str>,
    pub(crate) expires_at: Option<&'a str>,
}

pub(crate) fn normalize_memory_dates(
    input: MemoryDatesInput<'_>,
) -> Result<NormalizedMemoryDates, MemoryDateError> {
    let observed_at = parse_optional(input.observed_at, MemoryDateError::InvalidObservedAt)?;
    let last_confirmed_at = parse_optional(
        input.last_confirmed_at,
        MemoryDateError::InvalidLastConfirmedAt,
    )?;
    let valid_from = parse_optional(input.valid_from, MemoryDateError::InvalidValidFrom)?;
    let expires_at = parse_optional(input.expires_at, MemoryDateError::InvalidExpiresAt)?;

    if matches!((valid_from, expires_at), (Some(start), Some(end)) if start >= end) {
        return Err(MemoryDateError::InvalidValidityInterval);
    }

    Ok(NormalizedMemoryDates {
        observed_at: observed_at.map(format_utc),
        last_confirmed_at: last_confirmed_at.map(format_utc),
        valid_from: valid_from.map(format_utc),
        expires_at: expires_at.map(format_utc),
    })
}

/// `valid_from` is inclusive and `expires_at` is exclusive.
pub(crate) fn current_memory_validity(
    active: bool,
    valid_from: Option<&str>,
    expires_at: Option<&str>,
    now: DateTime<Utc>,
) -> Result<ValidityState, MemoryDateError> {
    let dates = normalize_memory_dates(MemoryDatesInput {
        observed_at: None,
        last_confirmed_at: None,
        valid_from,
        expires_at,
    })?;
    if !active {
        return Ok(ValidityState::Inactive);
    }

    let valid_from = parse_normalized(dates.valid_from.as_deref());
    if valid_from.is_some_and(|start| start > now) {
        return Ok(ValidityState::NotStarted);
    }
    let expires_at = parse_normalized(dates.expires_at.as_deref());
    if expires_at.is_some_and(|end| end <= now) {
        return Ok(ValidityState::Expired);
    }
    Ok(ValidityState::Current)
}

/// Confirmation time comes from the supporting source's recorded clock. The
/// approval clock is intentionally not accepted by this function.
pub(crate) fn last_confirmed_at_from_evidence(observed_at: DateTime<Utc>) -> String {
    format_utc(observed_at)
}

fn parse_optional(
    value: Option<&str>,
    error: MemoryDateError,
) -> Result<Option<DateTime<Utc>>, MemoryDateError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|_| error)
        })
        .transpose()
}

fn parse_normalized(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn format_utc(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevisionError {
    InvalidCurrentRevision,
    RevisionExhausted,
}

impl RevisionError {
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::InvalidCurrentRevision => "invalid_revision",
            Self::RevisionExhausted => "revision_exhausted",
        }
    }
}

pub(crate) fn next_revision(current: i64, changed: bool) -> Result<i64, RevisionError> {
    if current < INITIAL_REVISION {
        return Err(RevisionError::InvalidCurrentRevision);
    }
    if !changed {
        return Ok(current);
    }
    current
        .checked_add(1)
        .ok_or(RevisionError::RevisionExhausted)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateSnapshotError {
    Changed,
    EvidenceUnavailable,
    ClaimChanged,
}

impl CandidateSnapshotError {
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::Changed => "candidate_changed",
            Self::EvidenceUnavailable => "candidate_evidence_unavailable",
            Self::ClaimChanged => "candidate_claim_changed",
        }
    }
}

/// Compare the candidate version shown to the user before revalidating Plan
/// 049's stored claim/evidence digest. Callers must run this inside approval's
/// transaction after loading the current candidate row.
pub(crate) fn validate_candidate_snapshot(
    displayed_revision: i64,
    current_revision: i64,
    stored_claim_digest: Option<&str>,
    recomputed_claim_digest: Option<&str>,
) -> Result<(), CandidateSnapshotError> {
    if displayed_revision != current_revision {
        return Err(CandidateSnapshotError::Changed);
    }
    let Some(stored_claim_digest) = stored_claim_digest else {
        return Err(CandidateSnapshotError::EvidenceUnavailable);
    };
    let Some(recomputed_claim_digest) = recomputed_claim_digest else {
        return Err(CandidateSnapshotError::EvidenceUnavailable);
    };
    if stored_claim_digest != recomputed_claim_digest {
        return Err(CandidateSnapshotError::ClaimChanged);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetLifecycle {
    Active,
    Superseded,
    Retracted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetSnapshotError {
    Unavailable,
    Incomplete,
    Missing,
    Moved,
    Superseded,
    Retracted,
    Changed,
}

impl TargetSnapshotError {
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::Unavailable => "target_snapshot_unavailable",
            Self::Incomplete => "target_snapshot_incomplete",
            Self::Missing => "target_missing",
            Self::Moved => "target_moved",
            Self::Superseded => "target_superseded",
            Self::Retracted => "target_retracted",
            Self::Changed => "target_changed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReviewedTargetSnapshot<'a> {
    pub(crate) semantic_revision: Option<i64>,
    pub(crate) content_digest: Option<&'a str>,
    pub(crate) scope_type: &'a str,
    pub(crate) scope_key: &'a str,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValidatedTargetSnapshot<'a> {
    pub(crate) semantic_revision: i64,
    pub(crate) content_digest: &'a str,
    pub(crate) scope_type: &'a str,
    pub(crate) scope_key: &'a str,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentTarget<'a> {
    pub(crate) semantic_revision: i64,
    pub(crate) content_digest: &'a str,
    pub(crate) scope_type: &'a str,
    pub(crate) scope_key: &'a str,
    pub(crate) lifecycle: TargetLifecycle,
}

pub(crate) fn validate_reviewed_target_snapshot(
    snapshot: ReviewedTargetSnapshot<'_>,
    require_complete: bool,
) -> Result<ValidatedTargetSnapshot<'_>, TargetSnapshotError> {
    let Some(semantic_revision) = snapshot
        .semantic_revision
        .filter(|revision| *revision >= INITIAL_REVISION)
    else {
        return Err(TargetSnapshotError::Unavailable);
    };
    let Some(content_digest) = snapshot
        .content_digest
        .filter(|digest| valid_digest(digest))
    else {
        return Err(TargetSnapshotError::Unavailable);
    };
    if require_complete && !snapshot.complete {
        return Err(TargetSnapshotError::Incomplete);
    }
    Ok(ValidatedTargetSnapshot {
        semantic_revision,
        content_digest,
        scope_type: snapshot.scope_type,
        scope_key: snapshot.scope_key,
        complete: snapshot.complete,
    })
}

/// Recheck the exact reviewed target. This never follows a successor and never
/// refreshes the stored snapshot to the current row.
pub(crate) fn validate_current_target(
    reviewed: ValidatedTargetSnapshot<'_>,
    current: Option<CurrentTarget<'_>>,
) -> Result<(), TargetSnapshotError> {
    let Some(current) = current else {
        return Err(TargetSnapshotError::Missing);
    };
    if reviewed.scope_type != current.scope_type || reviewed.scope_key != current.scope_key {
        return Err(TargetSnapshotError::Moved);
    }
    match current.lifecycle {
        TargetLifecycle::Active => {}
        TargetLifecycle::Superseded => return Err(TargetSnapshotError::Superseded),
        TargetLifecycle::Retracted => return Err(TargetSnapshotError::Retracted),
    }
    if reviewed.semantic_revision != current.semantic_revision
        || reviewed.content_digest != current.content_digest
    {
        return Err(TargetSnapshotError::Changed);
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum SemanticRevisionReason {
    Title = 1 << 0,
    Body = 1 << 1,
    ContentKind = 1 << 2,
    ArtifactReference = 1 << 3,
    ArtifactContent = 1 << 4,
    Scope = 1 << 5,
    MemoryType = 1 << 6,
    Lifecycle = 1 << 7,
    Supersession = 1 << 8,
    FactKey = 1 << 9,
    ObservedAt = 1 << 10,
    LastConfirmedAt = 1 << 11,
    ValidFrom = 1 << 12,
    ExpiresAt = 1 << 13,
}

impl SemanticRevisionReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Body => "body",
            Self::ContentKind => "content_kind",
            Self::ArtifactReference => "artifact_reference",
            Self::ArtifactContent => "artifact_content",
            Self::Scope => "scope",
            Self::MemoryType => "memory_type",
            Self::Lifecycle => "lifecycle",
            Self::Supersession => "supersession",
            Self::FactKey => "fact_key",
            Self::ObservedAt => "observed_at",
            Self::LastConfirmedAt => "last_confirmed_at",
            Self::ValidFrom => "valid_from",
            Self::ExpiresAt => "expires_at",
        }
    }
}

const ALL_SEMANTIC_REASONS: [SemanticRevisionReason; 14] = [
    SemanticRevisionReason::Title,
    SemanticRevisionReason::Body,
    SemanticRevisionReason::ContentKind,
    SemanticRevisionReason::ArtifactReference,
    SemanticRevisionReason::ArtifactContent,
    SemanticRevisionReason::Scope,
    SemanticRevisionReason::MemoryType,
    SemanticRevisionReason::Lifecycle,
    SemanticRevisionReason::Supersession,
    SemanticRevisionReason::FactKey,
    SemanticRevisionReason::ObservedAt,
    SemanticRevisionReason::LastConfirmedAt,
    SemanticRevisionReason::ValidFrom,
    SemanticRevisionReason::ExpiresAt,
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RevisionReasonSet(u16);

impl RevisionReasonSet {
    fn insert(&mut self, reason: SemanticRevisionReason) {
        self.0 |= reason as u16;
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn contains(self, reason: SemanticRevisionReason) -> bool {
        self.0 & reason as u16 != 0
    }

    /// Closed, bounded storage encoding. It contains classifications only and
    /// is safe to retain after source-derived details are scrubbed.
    pub(crate) fn storage_value(self) -> String {
        ALL_SEMANTIC_REASONS
            .into_iter()
            .filter(|reason| self.contains(*reason))
            .map(SemanticRevisionReason::as_str)
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CanonicalSemanticFields<'a> {
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
    pub(crate) content_kind: &'a str,
    pub(crate) artifact_reference: Option<&'a str>,
    pub(crate) artifact_content_digest: Option<&'a str>,
    pub(crate) scope_type: &'a str,
    pub(crate) scope_key: &'a str,
    pub(crate) memory_type: &'a str,
    pub(crate) status: &'a str,
    pub(crate) supersedes_id: Option<&'a str>,
    pub(crate) fact_key: Option<&'a str>,
    pub(crate) observed_at: Option<&'a str>,
    pub(crate) last_confirmed_at: Option<&'a str>,
    pub(crate) valid_from: Option<&'a str>,
    pub(crate) expires_at: Option<&'a str>,
}

pub(crate) fn semantic_revision_reasons(
    before: CanonicalSemanticFields<'_>,
    after: CanonicalSemanticFields<'_>,
) -> RevisionReasonSet {
    let mut reasons = RevisionReasonSet::default();
    add_reason(
        &mut reasons,
        before.title != after.title,
        SemanticRevisionReason::Title,
    );
    add_reason(
        &mut reasons,
        before.body != after.body,
        SemanticRevisionReason::Body,
    );
    add_reason(
        &mut reasons,
        before.content_kind != after.content_kind,
        SemanticRevisionReason::ContentKind,
    );
    add_reason(
        &mut reasons,
        before.artifact_reference != after.artifact_reference,
        SemanticRevisionReason::ArtifactReference,
    );
    add_reason(
        &mut reasons,
        before.artifact_content_digest != after.artifact_content_digest,
        SemanticRevisionReason::ArtifactContent,
    );
    add_reason(
        &mut reasons,
        before.scope_type != after.scope_type || before.scope_key != after.scope_key,
        SemanticRevisionReason::Scope,
    );
    add_reason(
        &mut reasons,
        before.memory_type != after.memory_type,
        SemanticRevisionReason::MemoryType,
    );
    add_reason(
        &mut reasons,
        before.status != after.status,
        SemanticRevisionReason::Lifecycle,
    );
    add_reason(
        &mut reasons,
        before.supersedes_id != after.supersedes_id,
        SemanticRevisionReason::Supersession,
    );
    add_reason(
        &mut reasons,
        before.fact_key != after.fact_key,
        SemanticRevisionReason::FactKey,
    );
    add_reason(
        &mut reasons,
        before.observed_at != after.observed_at,
        SemanticRevisionReason::ObservedAt,
    );
    add_reason(
        &mut reasons,
        before.last_confirmed_at != after.last_confirmed_at,
        SemanticRevisionReason::LastConfirmedAt,
    );
    add_reason(
        &mut reasons,
        before.valid_from != after.valid_from,
        SemanticRevisionReason::ValidFrom,
    );
    add_reason(
        &mut reasons,
        before.expires_at != after.expires_at,
        SemanticRevisionReason::ExpiresAt,
    );
    reasons
}

fn add_reason(set: &mut RevisionReasonSet, changed: bool, reason: SemanticRevisionReason) {
    if changed {
        set.insert(reason);
    }
}

/// Digest the exact persisted semantic values. This is intentionally distinct
/// from whitespace-normalized duplicate identity and from Plan 049's evidence
/// claim digest.
pub(crate) fn canonical_content_digest(fields: CanonicalSemanticFields<'_>) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        "canonical_content_digest_version",
        &CANONICAL_CONTENT_DIGEST_VERSION.to_string(),
    );
    hash_field(&mut hasher, "title", fields.title);
    hash_field(&mut hasher, "body", fields.body);
    hash_field(&mut hasher, "content_kind", fields.content_kind);
    hash_optional(&mut hasher, "artifact_reference", fields.artifact_reference);
    hash_optional(
        &mut hasher,
        "artifact_content_digest",
        fields.artifact_content_digest,
    );
    hash_field(&mut hasher, "scope_type", fields.scope_type);
    hash_field(&mut hasher, "scope_key", fields.scope_key);
    hash_field(&mut hasher, "memory_type", fields.memory_type);
    hash_field(&mut hasher, "status", fields.status);
    hash_optional(&mut hasher, "supersedes_id", fields.supersedes_id);
    hash_optional(&mut hasher, "fact_key", fields.fact_key);
    hash_optional(&mut hasher, "observed_at", fields.observed_at);
    hash_optional(&mut hasher, "last_confirmed_at", fields.last_confirmed_at);
    hash_optional(&mut hasher, "valid_from", fields.valid_from);
    hash_optional(&mut hasher, "expires_at", fields.expires_at);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn artifact_content_digest(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        "artifact_content_digest_version",
        &ARTIFACT_CONTENT_DIGEST_VERSION.to_string(),
    );
    hasher.update(content.len().to_be_bytes());
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

fn hash_optional(hasher: &mut Sha256, name: &str, value: Option<&str>) {
    match value {
        Some(value) => {
            hash_field(hasher, &format!("{name}_present"), "1");
            hash_field(hasher, name, value);
        }
        None => hash_field(hasher, &format!("{name}_present"), "0"),
    }
}

fn hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.len().to_be_bytes());
    hasher.update(name.as_bytes());
    hasher.update(value.len().to_be_bytes());
    hasher.update(value.as_bytes());
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CandidateMaterialFields<'a> {
    pub(crate) scope_type: &'a str,
    pub(crate) scope_key: &'a str,
    pub(crate) memory_type: &'a str,
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
    pub(crate) confidence: f64,
    pub(crate) rationale: &'a str,
    pub(crate) evidence_claim_digest: Option<&'a str>,
    pub(crate) status: &'a str,
    pub(crate) blocked_code: Option<&'a str>,
    pub(crate) observed_at: Option<&'a str>,
    pub(crate) last_confirmed_at: Option<&'a str>,
    pub(crate) valid_from: Option<&'a str>,
    pub(crate) expires_at: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CandidateRevisionReasons {
    pub(crate) claim_changed: bool,
    pub(crate) evidence_changed: bool,
    pub(crate) state_changed: bool,
    pub(crate) dates_changed: bool,
}

impl CandidateRevisionReasons {
    pub(crate) fn is_empty(self) -> bool {
        !self.claim_changed && !self.evidence_changed && !self.state_changed && !self.dates_changed
    }
}

pub(crate) fn candidate_revision_reasons(
    before: CandidateMaterialFields<'_>,
    after: CandidateMaterialFields<'_>,
) -> CandidateRevisionReasons {
    CandidateRevisionReasons {
        claim_changed: before.scope_type != after.scope_type
            || before.scope_key != after.scope_key
            || before.memory_type != after.memory_type
            || before.title != after.title
            || before.body != after.body
            || before.confidence.to_bits() != after.confidence.to_bits()
            || before.rationale != after.rationale,
        evidence_changed: before.evidence_claim_digest != after.evidence_claim_digest,
        state_changed: before.status != after.status || before.blocked_code != after.blocked_code,
        dates_changed: before.observed_at != after.observed_at
            || before.last_confirmed_at != after.last_confirmed_at
            || before.valid_from != after.valid_from
            || before.expires_at != after.expires_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn clock() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).single().unwrap()
    }

    fn semantic() -> CanonicalSemanticFields<'static> {
        CanonicalSemanticFields {
            title: "Deploy window",
            body: "Sunday at 02:00 UTC",
            content_kind: "text",
            artifact_reference: None,
            artifact_content_digest: None,
            scope_type: "workflow",
            scope_key: "workflow-1",
            memory_type: "fact",
            status: "active",
            supersedes_id: None,
            fact_key: Some("workflow-1\0deploy\0window"),
            observed_at: Some("2026-09-07T08:00:00Z"),
            last_confirmed_at: Some("2026-09-07T08:00:00Z"),
            valid_from: None,
            expires_at: None,
        }
    }

    fn candidate() -> CandidateMaterialFields<'static> {
        CandidateMaterialFields {
            scope_type: "workflow",
            scope_key: "workflow-1",
            memory_type: "fact",
            title: "Deploy window",
            body: "Sunday at 02:00 UTC",
            confidence: 0.9,
            rationale: "User supplied the corrected window",
            evidence_claim_digest: Some("claim-digest"),
            status: "pending",
            blocked_code: None,
            observed_at: Some("2026-09-07T08:00:00Z"),
            last_confirmed_at: Some("2026-09-07T08:00:00Z"),
            valid_from: None,
            expires_at: None,
        }
    }

    #[test]
    fn unknown_dates_keep_active_memory_current() {
        assert_eq!(
            current_memory_validity(true, None, None, clock()),
            Ok(ValidityState::Current)
        );
        assert_eq!(
            current_memory_validity(false, None, None, clock()),
            Ok(ValidityState::Inactive)
        );
    }

    #[test]
    fn validity_start_is_inclusive_and_end_is_exclusive() {
        let boundary = "2026-09-08T12:00:00Z";
        assert_eq!(
            current_memory_validity(true, Some(boundary), None, clock()),
            Ok(ValidityState::Current)
        );
        assert_eq!(
            current_memory_validity(true, None, Some(boundary), clock()),
            Ok(ValidityState::Expired)
        );
        assert_eq!(
            current_memory_validity(true, Some("2026-09-08T12:00:00.001Z"), None, clock()),
            Ok(ValidityState::NotStarted)
        );
    }

    #[test]
    fn timestamps_are_normalized_to_utc_and_intervals_are_ordered() {
        let normalized = normalize_memory_dates(MemoryDatesInput {
            observed_at: Some("2026-09-08T14:00:00+02:00"),
            last_confirmed_at: None,
            valid_from: Some("2026-09-08T14:00:00+02:00"),
            expires_at: Some("2026-09-08T14:00:01+02:00"),
        })
        .unwrap();
        assert_eq!(
            normalized.observed_at.as_deref(),
            Some("2026-09-08T12:00:00Z")
        );
        assert_eq!(
            normalized.valid_from.as_deref(),
            Some("2026-09-08T12:00:00Z")
        );
        assert_eq!(
            normalized.expires_at.as_deref(),
            Some("2026-09-08T12:00:01Z")
        );

        let inverted = normalize_memory_dates(MemoryDatesInput {
            observed_at: None,
            last_confirmed_at: None,
            valid_from: Some("2026-09-08T12:00:01Z"),
            expires_at: Some("2026-09-08T12:00:00Z"),
        });
        assert_eq!(inverted, Err(MemoryDateError::InvalidValidityInterval));
        let empty = normalize_memory_dates(MemoryDatesInput {
            observed_at: None,
            last_confirmed_at: None,
            valid_from: Some("2026-09-08T12:00:00Z"),
            expires_at: Some("2026-09-08T12:00:00Z"),
        });
        assert_eq!(empty, Err(MemoryDateError::InvalidValidityInterval));
    }

    #[test]
    fn malformed_dates_have_field_specific_stable_codes() {
        let error = normalize_memory_dates(MemoryDatesInput {
            observed_at: None,
            last_confirmed_at: None,
            valid_from: Some("tomorrow"),
            expires_at: None,
        })
        .unwrap_err();
        assert_eq!(error.as_code(), "invalid_valid_from");
        assert_eq!(
            current_memory_validity(true, None, Some("later"), clock()),
            Err(MemoryDateError::InvalidExpiresAt)
        );
    }

    #[test]
    fn confirmation_uses_evidence_clock_instead_of_approval_clock() {
        let evidence_clock = Utc.with_ymd_and_hms(2024, 2, 3, 4, 5, 6).single().unwrap();
        assert_eq!(
            last_confirmed_at_from_evidence(evidence_clock),
            "2024-02-03T04:05:06Z"
        );
    }

    #[test]
    fn semantic_changes_are_bounded_reason_codes() {
        let before = semantic();
        let mut after = before;
        after.title = "Updated deploy window";
        after.status = "superseded";
        after.valid_from = Some("2026-10-01T00:00:00Z");
        let reasons = semantic_revision_reasons(before, after);
        assert!(reasons.contains(SemanticRevisionReason::Title));
        assert!(reasons.contains(SemanticRevisionReason::Lifecycle));
        assert!(reasons.contains(SemanticRevisionReason::ValidFrom));
        assert_eq!(reasons.storage_value(), "title,lifecycle,valid_from");
        assert!(!reasons.storage_value().contains("Updated deploy window"));
    }

    #[test]
    fn every_semantic_field_changes_the_digest() {
        let original = semantic();
        let digest = canonical_content_digest(original);
        assert_eq!(digest.len(), 64);

        let mutations: [fn(&mut CanonicalSemanticFields<'static>); 15] = [
            |value| value.title = "Other",
            |value| value.body = "Other",
            |value| value.content_kind = "artifact",
            |value| value.artifact_reference = Some("artifact://other"),
            |value| value.artifact_content_digest = Some("artifact-digest"),
            |value| value.scope_type = "user",
            |value| value.scope_key = "local-user",
            |value| value.memory_type = "decision",
            |value| value.status = "superseded",
            |value| value.supersedes_id = Some("memory-0"),
            |value| value.fact_key = Some("other"),
            |value| value.observed_at = Some("2026-09-07T09:00:00Z"),
            |value| value.last_confirmed_at = Some("2026-09-07T09:00:00Z"),
            |value| value.valid_from = Some("2026-09-09T00:00:00Z"),
            |value| value.expires_at = Some("2026-10-01T00:00:00Z"),
        ];
        for mutate in mutations {
            let mut changed = original;
            mutate(&mut changed);
            assert_ne!(digest, canonical_content_digest(changed));
        }
    }

    #[test]
    fn digest_distinguishes_absent_from_empty_and_field_boundaries() {
        let original = semantic();
        let mut empty_fact_key = original;
        empty_fact_key.fact_key = Some("");
        assert_ne!(
            canonical_content_digest(original),
            canonical_content_digest(empty_fact_key)
        );

        let mut first = original;
        first.title = "ab";
        first.body = "c";
        let mut second = original;
        second.title = "a";
        second.body = "bc";
        assert_ne!(
            canonical_content_digest(first),
            canonical_content_digest(second)
        );
    }

    #[test]
    fn revision_is_monotonic_and_metadata_only_callers_can_keep_it_stable() {
        assert_eq!(next_revision(INITIAL_REVISION, false), Ok(INITIAL_REVISION));
        assert_eq!(next_revision(INITIAL_REVISION, true), Ok(2));
        assert_eq!(
            next_revision(0, true),
            Err(RevisionError::InvalidCurrentRevision)
        );
        assert_eq!(
            next_revision(i64::MAX, true),
            Err(RevisionError::RevisionExhausted)
        );
        assert_eq!(
            RevisionError::RevisionExhausted.as_code(),
            "revision_exhausted"
        );
    }

    #[test]
    fn candidate_revision_tracks_claim_evidence_dates_and_state() {
        let before = candidate();
        assert!(candidate_revision_reasons(before, before).is_empty());

        let mut claim = before;
        claim.body = "Saturday at 02:00 UTC";
        assert!(candidate_revision_reasons(before, claim).claim_changed);

        let mut evidence = before;
        evidence.evidence_claim_digest = Some("new-digest");
        assert!(candidate_revision_reasons(before, evidence).evidence_changed);

        let mut dates = before;
        dates.valid_from = Some("2026-10-01T00:00:00Z");
        assert!(candidate_revision_reasons(before, dates).dates_changed);

        let mut state = before;
        state.status = "blocked";
        state.blocked_code = Some("target_changed");
        assert!(candidate_revision_reasons(before, state).state_changed);
    }

    #[test]
    fn candidate_snapshot_checks_displayed_revision_before_evidence_digest() {
        assert_eq!(
            validate_candidate_snapshot(2, 3, Some("stored"), Some("changed")),
            Err(CandidateSnapshotError::Changed)
        );
        assert_eq!(
            validate_candidate_snapshot(3, 3, None, None),
            Err(CandidateSnapshotError::EvidenceUnavailable)
        );
        assert_eq!(
            validate_candidate_snapshot(3, 3, Some("stored"), Some("changed")),
            Err(CandidateSnapshotError::ClaimChanged)
        );
        assert_eq!(
            validate_candidate_snapshot(3, 3, Some("stored"), Some("stored")),
            Ok(())
        );
        assert_eq!(
            CandidateSnapshotError::Changed.as_code(),
            "candidate_changed"
        );
    }

    #[test]
    fn legacy_or_incomplete_destructive_target_snapshots_are_blocked() {
        let legacy = ReviewedTargetSnapshot {
            semantic_revision: None,
            content_digest: None,
            scope_type: "workflow",
            scope_key: "workflow-1",
            complete: true,
        };
        assert_eq!(
            validate_reviewed_target_snapshot(legacy, true),
            Err(TargetSnapshotError::Unavailable)
        );

        let incomplete = ReviewedTargetSnapshot {
            semantic_revision: Some(1),
            content_digest: Some(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            complete: false,
            ..legacy
        };
        assert_eq!(
            validate_reviewed_target_snapshot(incomplete, true),
            Err(TargetSnapshotError::Incomplete)
        );
        assert!(validate_reviewed_target_snapshot(incomplete, false).is_ok());
    }

    #[test]
    fn target_recheck_returns_precise_conflicts_without_following_successors() {
        let digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let reviewed = validate_reviewed_target_snapshot(
            ReviewedTargetSnapshot {
                semantic_revision: Some(2),
                content_digest: Some(digest),
                scope_type: "workflow",
                scope_key: "workflow-1",
                complete: true,
            },
            true,
        )
        .unwrap();
        let current = CurrentTarget {
            semantic_revision: 2,
            content_digest: digest,
            scope_type: "workflow",
            scope_key: "workflow-1",
            lifecycle: TargetLifecycle::Active,
        };
        assert_eq!(validate_current_target(reviewed, Some(current)), Ok(()));
        assert_eq!(
            validate_current_target(reviewed, None),
            Err(TargetSnapshotError::Missing)
        );

        let mut moved = current;
        moved.scope_key = "workflow-2";
        assert_eq!(
            validate_current_target(reviewed, Some(moved)),
            Err(TargetSnapshotError::Moved)
        );

        let mut superseded = current;
        superseded.lifecycle = TargetLifecycle::Superseded;
        assert_eq!(
            validate_current_target(reviewed, Some(superseded)),
            Err(TargetSnapshotError::Superseded)
        );

        let mut retracted = current;
        retracted.lifecycle = TargetLifecycle::Retracted;
        assert_eq!(
            validate_current_target(reviewed, Some(retracted)),
            Err(TargetSnapshotError::Retracted)
        );

        let mut changed = current;
        changed.semantic_revision = 3;
        assert_eq!(
            validate_current_target(reviewed, Some(changed)),
            Err(TargetSnapshotError::Changed)
        );
        assert_eq!(TargetSnapshotError::Changed.as_code(), "target_changed");
    }
}
