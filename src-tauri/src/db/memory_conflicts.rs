//! Deterministic comparison-target discovery and correction conflict checks.
//!
//! This module contains no provider boundary. It gathers existing memories
//! from the same review-visible scopes, records the exact semantic snapshot
//! shown to the reviewer, and performs local duplicate/possible-conflict
//! checks both when a proposal is stored and when it is approved.

use super::memories::{get_memory_conn, is_current_at, Memory, MemoryScopeType, MemoryType};
use super::memory_evidence::EvidenceEnvelope;
use super::memory_retrieval::{MemoryRetrievalRequest, RetrievalPurpose};
use super::{Db, DbError};
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) const CORRECTION_TARGET_MAX_ITEMS: usize = 12;
pub(crate) const CORRECTION_TARGET_MAX_BYTES: usize = 12 * 1024;
const CORRECTION_TARGET_BODY_MAX_BYTES: usize = 1_200;
const FACT_KEY_PART_MAX_CHARS: usize = 128;
const POSSIBLE_CONFLICT_LIMIT: usize = 8;
const REFLOW_HASH_VERSION: u32 = 2;
const PROPOSAL_HASH_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedMemoryTarget {
    pub id: String,
    pub semantic_revision: i64,
    pub content_digest: String,
    pub scope_type: MemoryScopeType,
    pub scope_key: String,
    pub memory_type: MemoryType,
    pub title: String,
    pub body: String,
    pub fact_key: Option<String>,
    pub observed_at: Option<String>,
    pub last_confirmed_at: Option<String>,
    pub valid_from: Option<String>,
    pub expires_at: Option<String>,
    pub complete: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ReviewTargetDiscovery {
    pub markdown: String,
    pub targets: Vec<ReviewedMemoryTarget>,
    pub omitted_count: usize,
    pub error_code: Option<&'static str>,
}

impl ReviewTargetDiscovery {
    pub fn target_map(&self) -> HashMap<String, ReviewedMemoryTarget> {
        self.targets
            .iter()
            .cloned()
            .map(|target| (target.id.clone(), target))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictRelation {
    ExactDuplicate,
    SameFactKey,
    PossibleRelationship,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PossibleMemoryConflict {
    pub memory_id: String,
    pub title: String,
    pub relation: ConflictRelation,
}

#[derive(Debug, Clone, Copy)]
pub struct ProposalIdentityInput<'a> {
    pub operation: &'a str,
    pub target_memory_id: Option<&'a str>,
    pub target_semantic_revision: Option<i64>,
    pub scope_type: MemoryScopeType,
    pub scope_key: &'a str,
    pub memory_type: MemoryType,
    pub body: &'a str,
    pub valid_from: Option<&'a str>,
    pub expires_at: Option<&'a str>,
}

fn normalized_reflow(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.len().to_be_bytes());
    hasher.update(name.as_bytes());
    hasher.update(value.len().to_be_bytes());
    hasher.update(value.as_bytes());
}

fn hash_optional(hasher: &mut Sha256, name: &str, value: Option<&str>) {
    hash_field(
        hasher,
        &format!("{name}_present"),
        if value.is_some() { "1" } else { "0" },
    );
    if let Some(value) = value {
        hash_field(hasher, name, value);
    }
}

pub(crate) fn canonical_reflow_hash(
    scope_type: MemoryScopeType,
    scope_key: &str,
    memory_type: MemoryType,
    body: &str,
    valid_from: Option<&str>,
    expires_at: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        "reflow_hash_version",
        &REFLOW_HASH_VERSION.to_string(),
    );
    hash_field(&mut hasher, "scope_type", scope_type.as_str());
    hash_field(&mut hasher, "scope_key", scope_key);
    hash_field(&mut hasher, "memory_type", memory_type.as_str());
    hash_field(&mut hasher, "body", &normalized_reflow(body));
    hash_optional(&mut hasher, "valid_from", valid_from);
    hash_optional(&mut hasher, "expires_at", expires_at);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn proposal_identity_hash(input: ProposalIdentityInput<'_>) -> String {
    let reflow = canonical_reflow_hash(
        input.scope_type,
        input.scope_key,
        input.memory_type,
        input.body,
        input.valid_from,
        input.expires_at,
    );
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        "proposal_hash_version",
        &PROPOSAL_HASH_VERSION.to_string(),
    );
    hash_field(&mut hasher, "operation", input.operation);
    hash_optional(&mut hasher, "target_memory_id", input.target_memory_id);
    let target_revision = input
        .target_semantic_revision
        .map(|value| value.to_string());
    hash_optional(
        &mut hasher,
        "target_semantic_revision",
        target_revision.as_deref(),
    );
    hash_field(&mut hasher, "canonical_reflow_hash", &reflow);
    format!("{:x}", hasher.finalize())
}

fn normalize_fact_part(value: &str) -> Option<String> {
    let normalized = value
        .split_whitespace()
        .map(|part| part.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() || normalized.chars().count() > FACT_KEY_PART_MAX_CHARS {
        return None;
    }
    Some(normalized)
}

/// A fact key is scoped by construction and cannot collide across user,
/// workspace, and workflow namespaces.
pub(crate) fn normalized_fact_key(
    scope_type: MemoryScopeType,
    scope_key: &str,
    entity: &str,
    attribute: &str,
) -> Option<String> {
    let scope = normalize_fact_part(scope_key)?;
    let entity = normalize_fact_part(entity)?;
    let attribute = normalize_fact_part(attribute)?;
    Some(format!(
        "fact-v1|{}|{}:{}|{}:{}|{}:{}",
        scope_type.as_str(),
        scope.len(),
        scope,
        entity.len(),
        entity,
        attribute.len(),
        attribute
    ))
}

fn render_target(
    db: &Db,
    memory: &Memory,
    remaining: usize,
) -> Option<(String, ReviewedMemoryTarget)> {
    let body_budget = CORRECTION_TARGET_BODY_MAX_BYTES.min(remaining.saturating_sub(640));
    if body_budget < 80 {
        return None;
    }
    let excerpt = db.bounded_memory_excerpt(memory, body_budget);
    let target = ReviewedMemoryTarget {
        id: memory.id.clone(),
        semantic_revision: memory.semantic_revision,
        content_digest: memory.content_digest.clone(),
        scope_type: memory.scope_type,
        scope_key: memory.scope_key.clone(),
        memory_type: memory.memory_type,
        title: memory.title.clone(),
        body: excerpt.rendered,
        fact_key: memory.fact_key.clone(),
        observed_at: memory.observed_at.clone(),
        last_confirmed_at: memory.last_confirmed_at.clone(),
        valid_from: memory.valid_from.clone(),
        expires_at: memory.expires_at.clone(),
        // Artifact text lives outside SQLite and can change without a row
        // revision. It remains useful comparison material, but never a
        // complete destructive target snapshot.
        complete: memory.artifact_path.is_none() && !excerpt.truncated && !excerpt.incomplete,
    };
    let rendered = serde_json::to_string(&target).ok()? + "\n";
    (rendered.len() <= remaining).then_some((rendered, target))
}

/// Search each eligible evidence fragment independently, then allocate result
/// slots round-robin. This keeps an early/high-volume source from consuming the
/// entire comparison budget when one run contains several subjects.
pub(crate) fn discover_review_targets_at(
    db: &Db,
    workflow_id: &str,
    working_directory: Option<&str>,
    run_id: &str,
    envelope: &EvidenceEnvelope,
    now: DateTime<Utc>,
) -> ReviewTargetDiscovery {
    let mut per_fragment = Vec::new();
    let mut unavailable = false;
    for fragment in &envelope.fragments {
        let result = db.retrieve_memories_at(
            &MemoryRetrievalRequest {
                workflow_id,
                working_directory,
                run_id,
                node_id: "memory-review-targets",
                query_text: &fragment.text,
                exclude_ids: &[],
                purpose: RetrievalPurpose::Review,
            },
            now,
        );
        unavailable |= result.error_code.is_some();
        per_fragment.push(
            result
                .items
                .into_iter()
                .map(|item| item.memory_id)
                .collect::<Vec<_>>(),
        );
    }

    let discovered_count = per_fragment.iter().flatten().collect::<HashSet<_>>().len();
    let mut ordered_ids = Vec::new();
    let mut seen = HashSet::new();
    let max_depth = per_fragment.iter().map(Vec::len).max().unwrap_or_default();
    for depth in 0..max_depth {
        for ids in &per_fragment {
            if let Some(id) = ids.get(depth) {
                if seen.insert(id.clone()) {
                    ordered_ids.push(id.clone());
                }
            }
        }
    }

    let mut markdown = String::new();
    let mut targets = Vec::new();
    for id in ordered_ids {
        if targets.len() >= CORRECTION_TARGET_MAX_ITEMS {
            break;
        }
        let Ok(Some(memory)) = db.get_memory(&id) else {
            continue;
        };
        if !is_current_at(&memory, now) {
            continue;
        }
        let remaining = CORRECTION_TARGET_MAX_BYTES.saturating_sub(markdown.len());
        let Some((rendered, target)) = render_target(db, &memory, remaining) else {
            continue;
        };
        markdown.push_str(&rendered);
        targets.push(target);
    }
    ReviewTargetDiscovery {
        omitted_count: discovered_count.saturating_sub(targets.len()),
        markdown,
        targets,
        error_code: unavailable.then_some("memory_retrieval_unavailable"),
    }
}

fn meaningful_tokens(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|token| token.chars().count() >= 3)
        .collect()
}

fn token_similarity(left: &str, right: &str) -> f64 {
    let left = meaningful_tokens(left);
    let right = meaningful_tokens(right);
    let union = left.union(&right).count();
    if union == 0 {
        return 0.0;
    }
    left.intersection(&right).count() as f64 / union as f64
}

pub(crate) fn possible_conflicts_conn(
    conn: &rusqlite::Connection,
    scope_type: MemoryScopeType,
    scope_key: &str,
    memory_type: MemoryType,
    body: &str,
    fact_key: Option<&str>,
    exclude_memory_id: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Vec<PossibleMemoryConflict>, DbError> {
    let mut statement = conn.prepare(
        "SELECT id FROM memories
         WHERE scope_type = ?1 AND scope_key = ?2 AND memory_type = ?3
           AND status = 'active' AND (?4 IS NULL OR id != ?4)
         ORDER BY updated_at DESC, id ASC",
    )?;
    let ids = statement
        .query_map(
            params![
                scope_type.as_str(),
                scope_key,
                memory_type.as_str(),
                exclude_memory_id
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let proposed_hash = canonical_reflow_hash(scope_type, scope_key, memory_type, body, None, None);
    let mut by_id = BTreeMap::new();
    for id in ids {
        let Some(memory) = get_memory_conn(conn, &id)? else {
            continue;
        };
        if !is_current_at(&memory, now) {
            continue;
        }
        let current_hash = canonical_reflow_hash(
            memory.scope_type,
            &memory.scope_key,
            memory.memory_type,
            &memory.body,
            None,
            None,
        );
        let relation = if current_hash == proposed_hash {
            Some(ConflictRelation::ExactDuplicate)
        } else if fact_key.is_some() && memory.fact_key.as_deref() == fact_key {
            Some(ConflictRelation::SameFactKey)
        } else if token_similarity(body, &memory.body) >= 0.6 {
            Some(ConflictRelation::PossibleRelationship)
        } else {
            None
        };
        if let Some(relation) = relation {
            by_id.insert(
                memory.id.clone(),
                PossibleMemoryConflict {
                    memory_id: memory.id,
                    title: memory.title,
                    relation,
                },
            );
        }
    }
    Ok(by_id.into_values().take(POSSIBLE_CONFLICT_LIMIT).collect())
}

pub(crate) fn exact_current_duplicate_conn(
    conn: &rusqlite::Connection,
    scope_type: MemoryScopeType,
    scope_key: &str,
    memory_type: MemoryType,
    body: &str,
    valid_from: Option<&str>,
    expires_at: Option<&str>,
    exclude_memory_id: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Option<String>, DbError> {
    let proposed = canonical_reflow_hash(
        scope_type,
        scope_key,
        memory_type,
        body,
        valid_from,
        expires_at,
    );
    let mut statement = conn.prepare(
        "SELECT id FROM memories
         WHERE scope_type = ?1 AND scope_key = ?2 AND memory_type = ?3
           AND status = 'active' AND (?4 IS NULL OR id != ?4)
         ORDER BY id",
    )?;
    let ids = statement
        .query_map(
            params![
                scope_type.as_str(),
                scope_key,
                memory_type.as_str(),
                exclude_memory_id
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        let Some(memory) = get_memory_conn(conn, &id)? else {
            continue;
        };
        if is_current_at(&memory, now)
            && canonical_reflow_hash(
                memory.scope_type,
                &memory.scope_key,
                memory.memory_type,
                &memory.body,
                memory.valid_from.as_deref(),
                memory.expires_at.as_deref(),
            ) == proposed
        {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod revision {
    use super::*;
    use crate::db::{CreateMemoryInput, CreateWorkflowInput};
    use serde_json::json;

    #[test]
    fn proposal_identity_separates_operations_and_target_versions() {
        let input = ProposalIdentityInput {
            operation: "supersede",
            target_memory_id: Some("memory-1"),
            target_semantic_revision: Some(1),
            scope_type: MemoryScopeType::User,
            scope_key: "local-user",
            memory_type: MemoryType::Fact,
            body: "Timezone is UTC",
            valid_from: None,
            expires_at: None,
        };
        let first = proposal_identity_hash(input);
        assert_ne!(
            first,
            proposal_identity_hash(ProposalIdentityInput {
                operation: "retract",
                ..input
            })
        );
        assert_ne!(
            first,
            proposal_identity_hash(ProposalIdentityInput {
                target_semantic_revision: Some(2),
                ..input
            })
        );
    }

    #[test]
    fn database_trigger_bumps_each_semantic_field_but_not_ui_metadata() {
        let db = Db::open_in_memory().unwrap();
        let workflow_id = db
            .create_workflow(CreateWorkflowInput {
                name: "Revision audit".into(),
                description: String::new(),
                working_directory: "/tmp/revision-audit".into(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .unwrap()
            .id;
        for (id, body) in [
            ("revision-parent", "Parent"),
            ("revision-subject", "Original"),
        ] {
            db.create_memory(CreateMemoryInput {
                workflow_id: workflow_id.clone(),
                title: id.into(),
                body: body.into(),
                run_id: None,
                node_id: None,
                kind: None,
                scope_type: Some(MemoryScopeType::Workflow),
                memory_type: Some(MemoryType::Fact),
                source: Some("manual".into()),
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
                id: Some(id.into()),
            })
            .unwrap();
        }

        let initial = db.get_memory("revision-subject").unwrap().unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories SET pinned = 1, confidence = 0.7, salience = 77,
                    updated_at = 'same-visible-time'
                 WHERE id = 'revision-subject'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let metadata = db.get_memory("revision-subject").unwrap().unwrap();
        assert_eq!(metadata.semantic_revision, initial.semantic_revision);
        assert_eq!(metadata.content_digest, initial.content_digest);

        let mutations = [
            "title = 'Changed title'",
            "body = 'Changed body'",
            "kind = 'text'",
            "artifact_path = '/tmp/revision-reference'",
            "artifact_content_digest = 'artifact-digest'",
            "scope_type = 'user', scope_key = 'local-user'",
            "memory_type = 'note'",
            "status = 'superseded'",
            "supersedes_id = 'revision-parent'",
            "fact_key = 'fact-v1|revision'",
            "observed_at = '2026-09-01T00:00:00Z'",
            "last_confirmed_at = '2026-09-02T00:00:00Z'",
            "valid_from = '2026-09-03T00:00:00Z'",
            "expires_at = '2027-09-03T00:00:00Z'",
        ];
        let mut prior = metadata;
        for mutation in mutations {
            db.with_conn(|conn| {
                conn.execute(
                    &format!(
                        "UPDATE memories SET {mutation}, updated_at = 'same-visible-time'
                         WHERE id = 'revision-subject'"
                    ),
                    [],
                )?;
                Ok(())
            })
            .unwrap();
            let changed = db.get_memory("revision-subject").unwrap().unwrap();
            assert_eq!(
                changed.semantic_revision,
                prior.semantic_revision + 1,
                "{mutation} must advance the semantic revision"
            );
            assert_ne!(
                changed.content_digest, prior.content_digest,
                "{mutation} must alter the canonical digest"
            );
            prior = changed;
        }
    }
}

#[cfg(test)]
mod discovery {
    use super::*;
    use crate::db::{CreateMemoryInput, CreateWorkflowInput};
    use serde_json::json;
    use std::fs;

    #[test]
    fn artifact_targets_are_comparison_only_even_when_the_bounded_read_fits() {
        let db = Db::open_in_memory().unwrap();
        let workflow_id = db
            .create_workflow(CreateWorkflowInput {
                name: "Artifact review".into(),
                description: String::new(),
                working_directory: "/tmp/artifact-review".into(),
                folder_id: None,
                graph: json!({"nodes": [], "edges": []}),
            })
            .unwrap()
            .id;
        let memory = db
            .create_memory(CreateMemoryInput {
                workflow_id,
                title: "Artifact claim".into(),
                body: "The artifact says deployment is Sunday.".into(),
                run_id: None,
                node_id: None,
                kind: None,
                scope_type: Some(MemoryScopeType::Workflow),
                memory_type: Some(MemoryType::Fact),
                source: Some("manual".into()),
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
                id: Some("artifact-target".into()),
            })
            .unwrap();
        let artifact_root = db.artifact_root().join("artifacts").join("artifact-review");
        fs::create_dir_all(&artifact_root).unwrap();
        let path = artifact_root.join(format!("{}.txt", uuid::Uuid::new_v4()));
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories
                 SET kind = 'artifact', artifact_path = ?1,
                     artifact_content_digest = 'stored-before-read'
                 WHERE id = ?2",
                params![path.to_string_lossy(), memory.id],
            )?;
            Ok(())
        })
        .unwrap();
        let memory = db.get_memory("artifact-target").unwrap().unwrap();
        let (_, first) = render_target(&db, &memory, 4_096).unwrap();
        assert!(!first.complete);
        assert!(first.body.contains("deployment is Sunday"));

        fs::write(&path, "The artifact now says deployment is Saturday.").unwrap();
        let (_, changed) = render_target(&db, &memory, 4_096).unwrap();
        assert!(!changed.complete);
        assert!(changed.body.contains("deployment is Saturday"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn fact_keys_include_scope_and_reject_unbounded_parts() {
        assert_ne!(
            normalized_fact_key(MemoryScopeType::User, "local-user", "Alice", "timezone"),
            normalized_fact_key(MemoryScopeType::Workflow, "local-user", "Alice", "timezone")
        );
        assert!(normalized_fact_key(MemoryScopeType::User, "local-user", "", "timezone").is_none());
        assert!(normalized_fact_key(
            MemoryScopeType::User,
            "local-user",
            &"x".repeat(129),
            "timezone"
        )
        .is_none());
    }
}

#[cfg(test)]
mod duplicates {
    use super::*;

    #[test]
    fn identity_includes_scope_type_type_interval_and_identifier_case() {
        let base = canonical_reflow_hash(
            MemoryScopeType::User,
            "same",
            MemoryType::Fact,
            "Uses  Neovim",
            None,
            None,
        );
        assert_eq!(
            base,
            canonical_reflow_hash(
                MemoryScopeType::User,
                "same",
                MemoryType::Fact,
                "Uses Neovim",
                None,
                None
            )
        );
        assert_ne!(
            base,
            canonical_reflow_hash(
                MemoryScopeType::Workflow,
                "same",
                MemoryType::Fact,
                "Uses Neovim",
                None,
                None
            )
        );
        assert_ne!(
            base,
            canonical_reflow_hash(
                MemoryScopeType::User,
                "same",
                MemoryType::Fact,
                "Uses Neovim",
                Some("2026-10-01T00:00:00Z"),
                None
            )
        );
        let upper = canonical_reflow_hash(
            MemoryScopeType::Workflow,
            "workflow-1",
            MemoryType::Fact,
            "The config key is API_URL.",
            None,
            None,
        );
        let lower = canonical_reflow_hash(
            MemoryScopeType::Workflow,
            "workflow-1",
            MemoryType::Fact,
            "The config key is api_url.",
            None,
            None,
        );
        assert_ne!(upper, lower);
    }

    #[test]
    fn fuzzy_similarity_is_only_a_possible_relationship() {
        assert!(token_similarity("Alice uses dark theme", "Alice now uses dark theme") >= 0.6);
        assert!(token_similarity("Alice uses dark theme", "Bob ships release builds") < 0.6);
    }
}

#[cfg(test)]
mod approval {
    use crate::db::memory_validity::{
        validate_current_target, validate_reviewed_target_snapshot, CurrentTarget,
        ReviewedTargetSnapshot, TargetLifecycle,
    };

    #[test]
    fn target_validation_never_substitutes_a_changed_version_or_lifecycle() {
        let reviewed = validate_reviewed_target_snapshot(
            ReviewedTargetSnapshot {
                semantic_revision: Some(4),
                content_digest: Some(
                    "4444444444444444444444444444444444444444444444444444444444444444",
                ),
                scope_type: "workflow",
                scope_key: "workflow-1",
                complete: true,
            },
            true,
        )
        .unwrap();
        let unchanged = CurrentTarget {
            semantic_revision: 4,
            content_digest: "4444444444444444444444444444444444444444444444444444444444444444",
            scope_type: "workflow",
            scope_key: "workflow-1",
            lifecycle: TargetLifecycle::Active,
        };
        assert!(validate_current_target(reviewed, Some(unchanged)).is_ok());
        assert_eq!(
            validate_current_target(
                reviewed,
                Some(CurrentTarget {
                    semantic_revision: 5,
                    ..unchanged
                })
            )
            .unwrap_err()
            .as_code(),
            "target_changed"
        );
        assert_eq!(
            validate_current_target(
                reviewed,
                Some(CurrentTarget {
                    lifecycle: TargetLifecycle::Superseded,
                    ..unchanged
                })
            )
            .unwrap_err()
            .as_code(),
            "target_superseded"
        );
    }
}

#[cfg(test)]
mod validity {
    use crate::db::memory_validity::{current_memory_validity, ValidityState};
    use chrono::TimeZone;
    use chrono::Utc;

    #[test]
    fn validity_clock_is_explicit_and_uses_exclusive_expiry() {
        let now = Utc.with_ymd_and_hms(2026, 9, 9, 12, 0, 0).single().unwrap();
        assert_eq!(now.to_rfc3339(), "2026-09-09T12:00:00+00:00");
        assert_eq!(
            current_memory_validity(true, None, Some("2026-09-09T12:00:01Z"), now).unwrap(),
            ValidityState::Current
        );
        assert_eq!(
            current_memory_validity(true, None, Some("2026-09-09T12:00:00Z"), now).unwrap(),
            ValidityState::Expired
        );
        assert_eq!(
            current_memory_validity(true, Some("2026-09-09T12:00:01Z"), None, now).unwrap(),
            ValidityState::NotStarted
        );
    }
}
