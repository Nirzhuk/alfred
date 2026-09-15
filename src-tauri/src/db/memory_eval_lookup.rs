//! Fixed-label evaluation for the bounded agent memory lookup contract.
//!
//! The fixture in `tests/fixtures/memory-quality/lookup-v1.json` is authored
//! independently of retrieval output. This evaluator seeds a fresh, real
//! SQLite schema, runs the production automatic retrieval/composer, and then
//! exercises `AgentMemoryLookupService` over temporary receipt artifacts and
//! evidence at a fixed clock. It measures lookup recovery and transport
//! invariants; it never invokes a provider or claims model usefulness.

use crate::db::agent_memory_lookup::{
    AgentMemoryLookupService, LookupContractError, LookupGetResponse, LookupSearchResponse,
    LookupServiceError, LookupSourceKind, LookupSourceState, LookupToolName, LookupTrace,
    LookupTraceClassification, UninterruptedLookup, LOOKUP_CONTRACT_VERSION, LOOKUP_LIMITS_V1,
};
use crate::db::artifact_chunks::persist_artifact_chunk_manifest_conn;
use crate::db::history::index_run_step;
use crate::db::memory_validity::artifact_content_digest;
use crate::db::memories::{write_canonical_memory, CanonicalMemoryRecord};
use crate::db::output_persistence::{
    insert_output_receipt, OutputDisposition, OutputReceiptKey, OutputReceiptSeed,
    OutputStorageIdentity,
};
use crate::db::{
    Db, DbError, EvidenceOrigin, EvidenceSourceInput, MemoryScopeType, MemoryStatus, MemoryType,
};
use crate::runner::memory_context::{
    compose_agent_memory_at, AutomaticMemoryCandidate, PromptValue, TextOrigin,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

const FIXTURE: &str = include_str!("../../../tests/fixtures/memory-quality/lookup-v1.json");
const FIXTURE_SHA256: &str =
    "6deeb417343762984b13f568764abf643a014932d25814ba9609aa8a497edfc6";
const REPORT_ENV: &str = "ALFRED_MEMORY_LOOKUP_EVAL_REPORT";
const INITIAL_MEMORY_LIMIT: usize = 12_000;
const INCREMENTAL_LOOKUP_LIMIT: usize = 16_000;
const LOOKUP_CALL_LIMIT: usize = 8;
const BODY_PAGE_LIMIT: usize = 4_000;
const SEARCH_RESPONSE_LIMIT: usize = 6_000;
const GET_RESPONSE_LIMIT: usize = 6_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    version: String,
    evaluator_version: String,
    lookup_contract_version: u16,
    fixed_clock: String,
    limits: FixtureLimits,
    cases: Vec<FixtureCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureLimits {
    initial_memory_bytes: usize,
    incremental_lookup_bytes: usize,
    lookup_calls: usize,
    body_page_bytes: usize,
    search_results: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    id: String,
    category: String,
    split: String,
    workflow_id: String,
    working_directory: String,
    run_id: String,
    node_id: String,
    task_question: String,
    expected_answer_facts: Vec<String>,
    initial_query: String,
    initial_required_ids: Vec<String>,
    initial_allowed_ids: Vec<String>,
    initial_forbidden_ids: Vec<String>,
    lookup_enabled: bool,
    automatic_retrieval_enabled: bool,
    #[serde(default)]
    permission_revoke_after_calls: Option<usize>,
    lookup_actions: Vec<LookupActionSpec>,
    expected_recovered_ids: Vec<String>,
    lookup_forbidden_ids: Vec<String>,
    #[serde(default)]
    lookup_allowed_historical_ids: Vec<String>,
    expected_no_evidence: bool,
    #[serde(default)]
    memories: Vec<MemorySpec>,
    #[serde(default)]
    history_records: Vec<HistorySpec>,
    #[serde(default)]
    budget_expectation: Option<BudgetExpectation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupActionSpec {
    tool: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    reference_source_id: Option<String>,
    #[serde(default)]
    cursor_from_previous: bool,
    expected_source_ids: Vec<String>,
    expected_source_kind: String,
    expected_state: String,
    #[serde(default)]
    expected_body_contains: Vec<String>,
    #[serde(default)]
    expected_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemorySpec {
    id: String,
    title: String,
    body: String,
    scope_type: String,
    #[serde(default)]
    owner_workflow_id: Option<String>,
    memory_type: String,
    source: String,
    status: String,
    pinned: bool,
    #[serde(default)]
    supersedes_id: Option<String>,
    #[serde(default)]
    fact_key: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
    salience: Option<i64>,
    #[serde(default)]
    observed_at: Option<String>,
    #[serde(default)]
    last_confirmed_at: Option<String>,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistorySpec {
    kind: String,
    alias: String,
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    artifact: bool,
    #[serde(default)]
    artifact_unit: Option<String>,
    #[serde(default)]
    artifact_repeat: Option<usize>,
    #[serde(default)]
    storage_id: Option<String>,
    #[serde(default)]
    receipt_state: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    source_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetExpectation {
    calls_used: usize,
    returned_bytes_at_most: usize,
    probe_ninth_call: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct TraceSourceReport {
    reference: String,
    source: String,
    source_id: String,
    version: String,
    state: String,
    page: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct TraceReport {
    tool: String,
    sources: Vec<TraceSourceReport>,
    returned_bytes: usize,
    classification: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct ActionReport {
    index: usize,
    tool: String,
    returned_source_ids: Vec<String>,
    response_bytes: usize,
    returned_bytes: usize,
    error: Option<String>,
    trace: Option<TraceReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct LookupCaseReport {
    id: String,
    category: String,
    split: String,
    initial_required_count: usize,
    initial_recovered_count: usize,
    initial_returned_ids: Vec<String>,
    initial_recall: Option<f64>,
    lookup_required_count: usize,
    lookup_recovered_count: usize,
    lookup_recovered_ids: Vec<String>,
    recovered_after_initial_miss_ids: Vec<String>,
    lookup_recall: Option<f64>,
    wrong_memory_uses: Vec<String>,
    expected_denied: Vec<String>,
    lookup_errors: Vec<String>,
    unexpected_errors: Vec<String>,
    initial_errors: Vec<String>,
    trace_violations: Vec<String>,
    artifact_violations: Vec<String>,
    budget_violations: Vec<String>,
    permission_violations: Vec<String>,
    initial_memory_bytes: usize,
    lookup_calls: usize,
    lookup_returned_bytes: usize,
    actions: Vec<ActionReport>,
    /// Entries are initial retrieval/composition followed by one duration per lookup action.
    latency_ms: Vec<u128>,
    expected_no_evidence: bool,
    /// No successful source was returned, and no initial or lookup error occurred.
    actual_no_evidence: bool,
    /// No successful source was returned, even if a permission or contract error occurred.
    actual_source_free: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct LookupAggregateReport {
    cases: usize,
    initial_required_count: usize,
    initial_recovered_count: usize,
    initial_recall: Option<f64>,
    lookup_required_count: usize,
    lookup_recovered_count: usize,
    lookup_recall: Option<f64>,
    recovered_after_initial_miss: usize,
    expected_no_evidence_cases: usize,
    actual_source_free_cases: usize,
    actual_no_evidence_cases: usize,
    unexpected_nonempty_no_evidence_cases: usize,
    lookup_errors: usize,
    expected_denied: usize,
    wrong_memory_uses: usize,
    unexpected_errors: usize,
    initial_errors: usize,
    trace_violations: usize,
    artifact_violations: usize,
    budget_violations: usize,
    permission_violations: usize,
    initial_memory_bytes: usize,
    lookup_calls: usize,
    lookup_returned_bytes: usize,
    max_initial_memory_bytes: usize,
    max_lookup_calls: usize,
    max_lookup_returned_bytes: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct LookupEvaluationReport {
    fixture_version: String,
    evaluator_version: String,
    lookup_contract_version: u16,
    fixed_clock: String,
    fixture_digest: String,
    development: LookupAggregateReport,
    validation: LookupAggregateReport,
    current_runtime: LookupAggregateReport,
    cases: Vec<LookupCaseReport>,
    timing_excluded_from_repeatability: bool,
    limitations: Vec<String>,
}

struct EvalTempDir {
    path: PathBuf,
}

impl EvalTempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("alfred-memory-lookup-eval-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create lookup evaluation temporary root");
        Self { path }
    }
}

impl Drop for EvalTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone, Copy)]
struct PageExpectation {
    page: u32,
    next_offset: usize,
}

struct SeededCase {
    db: Arc<Db>,
    _root: EvalTempDir,
    aliases: HashMap<String, String>,
    reverse_aliases: HashMap<String, String>,
    bodies: HashMap<String, String>,
    artifact_memory_ids: HashMap<String, String>,
}

impl SeededCase {
    fn add_alias(&mut self, alias: impl Into<String>, source_id: impl Into<String>, body: Option<String>) {
        let alias = alias.into();
        let source_id = source_id.into();
        assert!(self.aliases.insert(alias.clone(), source_id.clone()).is_none(), "duplicate source alias {alias}");
        assert!(self.reverse_aliases.insert(source_id.clone(), alias).is_none(), "duplicate source id {source_id}");
        if let Some(body) = body {
            self.bodies.insert(source_id, body);
        }
    }


    fn alias_for(&self, source_id: &str) -> String {
        self.reverse_aliases
            .get(source_id)
            .cloned()
            .unwrap_or_else(|| format!("unknown:{source_id}"))
    }
}

fn load_fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("lookup-v1 fixture must be valid JSON")
}

fn parse_clock(fixture: &Fixture) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&fixture.fixed_clock)
        .expect("lookup fixture fixed clock must be RFC3339")
        .with_timezone(&Utc)
}

fn fixture_digest() -> String {
    format!("{:x}", Sha256::digest(FIXTURE.as_bytes()))
}

fn insert_workflow(
    db: &Db,
    id: &str,
    working_directory: &str,
    automatic_retrieval_enabled: bool,
    lookup_enabled: bool,
) -> Result<(), DbError> {
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO workflows
             (id, name, description, working_directory, memory_retrieval_enabled,
              memory_lookup_enabled, graph_json, created_at, updated_at)
             VALUES (?1, ?1, '', ?2, ?3, ?4, '{\"nodes\":[],\"edges\":[]}', ?5, ?5)",
            params![
                id,
                working_directory,
                if automatic_retrieval_enabled { 1 } else { 0 },
                if lookup_enabled { 1 } else { 0 },
                "2026-08-01T00:00:00Z"
            ],
        )?;
        Ok(())
    })
}

fn insert_run(db: &Db, workflow_id: &str, run_id: &str) -> Result<(), DbError> {
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO runs
             (id, workflow_id, trigger_kind, status, started_at, finished_at, created_at)
             VALUES (?1, ?2, 'manual', 'completed', '2026-08-18T10:00:00Z',
                     '2026-08-18T10:05:00Z', '2026-08-18T10:00:00Z')",
            params![run_id, workflow_id],
        )?;
        Ok(())
    })
}

fn parse_scope(value: &str) -> MemoryScopeType {
    match value {
        "user" => MemoryScopeType::User,
        "workspace" => MemoryScopeType::Workspace,
        "workflow" => MemoryScopeType::Workflow,
        other => panic!("unknown lookup fixture scope {other}"),
    }
}

fn parse_memory_type(value: &str) -> MemoryType {
    match value {
        "preference" => MemoryType::Preference,
        "fact" => MemoryType::Fact,
        "decision" => MemoryType::Decision,
        "constraint" => MemoryType::Constraint,
        "lesson" => MemoryType::Lesson,
        "episode" => MemoryType::Episode,
        "checkpoint" => MemoryType::Checkpoint,
        "note" => MemoryType::Note,
        "output" => MemoryType::Output,
        "artifact" => MemoryType::Artifact,
        other => panic!("unknown lookup fixture memory type {other}"),
    }
}

fn parse_memory_status(value: &str) -> MemoryStatus {
    match value {
        "active" => MemoryStatus::Active,
        "superseded" => MemoryStatus::Superseded,
        "retracted" => MemoryStatus::Retracted,
        other => panic!("unknown lookup fixture memory status {other}"),
    }
}

fn insert_fixture_memory(db: &Db, case: &FixtureCase, spec: &MemorySpec) -> Result<(), DbError> {
    let owner = spec
        .owner_workflow_id
        .as_deref()
        .unwrap_or(&case.workflow_id);
    let scope_type = parse_scope(&spec.scope_type);
    let scope_key = match scope_type {
        MemoryScopeType::Workflow => owner.to_owned(),
        MemoryScopeType::Workspace => case.working_directory.clone(),
        MemoryScopeType::User => "local-user".into(),
    };
    let record = CanonicalMemoryRecord {
        id: spec.id.clone(),
        workflow_id: Some(owner.to_owned()),
        run_id: spec.run_id.clone(),
        node_id: spec.node_id.clone(),
        scope_type,
        scope_key,
        kind: spec.kind.clone().unwrap_or_else(|| "text".into()),
        memory_type: parse_memory_type(&spec.memory_type),
        source: spec.source.clone(),
        title: spec.title.clone(),
        body: spec.body.clone(),
        artifact_path: None,
        artifact_content_digest: None,
        pinned: spec.pinned,
        confidence: spec.confidence.unwrap_or(0.9),
        salience: spec.salience.unwrap_or(50),
        status: parse_memory_status(&spec.status),
        supersedes_id: spec.supersedes_id.clone(),
        fact_key: spec.fact_key.clone(),
        observed_at: spec.observed_at.clone(),
        last_confirmed_at: spec.last_confirmed_at.clone(),
        valid_from: spec.valid_from.clone(),
        expires_at: spec.expires_at.clone(),
        created_at: spec.created_at.clone(),
    };
    db.with_conn(|conn| {
        write_canonical_memory(conn, &record)?;
        Ok(())
    })
}

fn normalize_output_body(value: &str) -> String {
    value.replace("\r\n", "\n").trim().to_owned()
}

fn history_body(record: &HistorySpec) -> String {
    let body = if record.artifact {
        let unit = record
            .artifact_unit
            .as_deref()
            .or(record.output_text.as_deref())
            .unwrap_or("");
        unit.repeat(record.artifact_repeat.unwrap_or(1))
    } else {
        record.output_text.clone().unwrap_or_default()
    };
    normalize_output_body(&body)
}

fn bounded_prefix(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_owned();
    }
    let mut end = maximum;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
const ARTIFACT_CHUNK_SIZE: usize = 4_096;

fn expected_body_page(body: &str, offset: usize) -> Option<(String, usize)> {
    if offset > body.len() || !body.is_char_boundary(offset) {
        return None;
    }
    let page = bounded_prefix(&body[offset..], BODY_PAGE_LIMIT);
    let next_offset = offset + page.len();
    Some((page, next_offset))
}

fn plain_sha256(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn verify_artifact_page(
    seeded: &SeededCase,
    source_id: &str,
    body: &str,
    offset: usize,
    index: usize,
    artifact_violations: &mut Vec<String>,
) {
    let Some(memory_id) = seeded.artifact_memory_ids.get(source_id) else {
        artifact_violations.push(format!("artifact_memory_binding:{index}"));
        return;
    };
    let expected_digest = artifact_content_digest(body.as_bytes());
    let stored_digest = seeded
        .db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT artifact_content_digest FROM memories WHERE id = ?1",
                params![memory_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(DbError::from)
        })
        .expect("read lookup fixture artifact identity");
    if stored_digest.as_deref() != Some(expected_digest.as_str()) {
        artifact_violations.push(format!("artifact_content_identity:{index}"));
    }
    let manifest = seeded
        .db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT content_digest, digest_version, byte_length, chunk_size,
                        chunk_count, verification_state
                   FROM artifact_chunk_manifests
                  WHERE memory_id = ?1",
                params![memory_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(DbError::from)
        })
        .expect("read lookup fixture artifact manifest");
    let Some((content_digest, digest_version, byte_length, chunk_size, chunk_count, state)) = manifest
    else {
        artifact_violations.push(format!("artifact_manifest_missing:{index}"));
        return;
    };
    let expected_chunk_count =
        body.len().saturating_add(ARTIFACT_CHUNK_SIZE - 1) / ARTIFACT_CHUNK_SIZE;
    if content_digest != expected_digest
        || digest_version != 1
        || byte_length != body.len() as i64
        || chunk_size != ARTIFACT_CHUNK_SIZE as i64
        || chunk_count != expected_chunk_count as i64
        || state != "verified"
    {
        artifact_violations.push(format!("artifact_manifest_identity:{index}"));
    }
    let Some(page_end) = offset
        .checked_add(BODY_PAGE_LIMIT)
        .map(|end| end.min(body.len()))
    else {
        artifact_violations.push(format!("artifact_page_offset:{index}"));
        return;
    };
    if page_end <= offset {
        return;
    }
    let first_chunk = offset / ARTIFACT_CHUNK_SIZE;
    let last_chunk = (page_end - 1) / ARTIFACT_CHUNK_SIZE;
    let actual_chunks = seeded
        .db
        .with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT chunk_index, digest
                   FROM artifact_chunk_digests
                  WHERE memory_id = ?1 AND chunk_index BETWEEN ?2 AND ?3
                  ORDER BY chunk_index",
            )?;
            let rows = statement.query_map(
                params![memory_id, first_chunk as i64, last_chunk as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        })
        .expect("read lookup fixture artifact chunk digests");
    let expected_chunks = body
        .as_bytes()
        .chunks(ARTIFACT_CHUNK_SIZE)
        .enumerate()
        .skip(first_chunk)
        .take(last_chunk - first_chunk + 1)
        .map(|(chunk, bytes)| (chunk as i64, plain_sha256(bytes)))
        .collect::<Vec<_>>();
    if actual_chunks != expected_chunks {
        artifact_violations.push(format!("artifact_adjacent_chunk_digest:{index}"));
    }
}


fn validate_get_body(
    seeded: &SeededCase,
    alias: &str,
    source_id: &str,
    state: LookupSourceState,
    body: Option<&str>,
    page: u32,
    cursor_from_previous: bool,
    page_expectations: &mut HashMap<String, PageExpectation>,
    index: usize,
    artifact_violations: &mut Vec<String>,
) {
    let body_expected = matches!(
        state,
        LookupSourceState::Current | LookupSourceState::Historical | LookupSourceState::Incomplete
    );
    if !body_expected {
        if body.is_some_and(|value| !value.is_empty()) {
            artifact_violations.push(format!("unexpected_get_body:{index}"));
        }
        return;
    }
    let (expected_page, offset) = if cursor_from_previous {
        let Some(previous) = page_expectations.get(alias).copied() else {
            artifact_violations.push(format!("missing_page_expectation:{index}"));
            return;
        };
        (previous.page.saturating_add(1), previous.next_offset)
    } else {
        (0, 0)
    };
    if page != expected_page {
        artifact_violations.push(format!("unexpected_page:{index}"));
    }
    let Some(expected_body) = seeded.bodies.get(source_id) else {
        artifact_violations.push(format!("seeded_body_missing:{index}"));
        return;
    };
    let Some((expected_body_page, next_offset)) = expected_body_page(expected_body, offset) else {
        artifact_violations.push(format!("seeded_body_offset:{index}"));
        return;
    };
    if body != Some(expected_body_page.as_str()) {
        artifact_violations.push(format!("get_body_content:{index}"));
    }
    if seeded.artifact_memory_ids.contains_key(source_id) {
        verify_artifact_page(
            seeded,
            source_id,
            expected_body,
            offset,
            index,
            artifact_violations,
        );
    }
    page_expectations.insert(
        alias.to_owned(),
        PageExpectation {
            page,
            next_offset,
        },
    );
}

fn parse_evidence_origin(value: &str) -> EvidenceOrigin {
    match value {
        "authored_task" => EvidenceOrigin::AuthoredTask,
        "configured_workflow_instruction" => EvidenceOrigin::ConfiguredWorkflowInstruction,
        "external_trigger_data" => EvidenceOrigin::ExternalTriggerData,
        "host_recorded_observation" => EvidenceOrigin::HostRecordedObservation,
        "model_output" => EvidenceOrigin::ModelOutput,
        "prior_memory" => EvidenceOrigin::PriorMemory,
        "retrieved_historical_summary" => EvidenceOrigin::RetrievedHistoricalSummary,
        other => panic!("unknown lookup fixture evidence origin {other}"),
    }
}

fn seed_history_record(
    seeded: &mut SeededCase,
    case: &FixtureCase,
    record: &HistorySpec,
) -> Result<(), DbError> {
    match record.kind.as_str() {
        "run_step" => {
            let workflow_id = record
                .workflow_id
                .as_deref()
                .unwrap_or(&case.workflow_id);
            let run_id = record.run_id.as_deref().unwrap_or(&case.run_id);
            let node_id = record.node_id.as_deref().unwrap_or("report");
            let body = history_body(record);
            let step_id = record.alias.clone();
            let input = json!({"fixture": case.id, "kind": "synthetic"});
            let output = json!({"text": body});
            seeded.db.with_conn(|conn| {
                let input_json = serde_json::to_string(&input)
                    .map_err(|_| DbError::Other("lookup fixture input serialization failed".into()))?;
                let output_json = serde_json::to_string(&output)
                    .map_err(|_| DbError::Other("lookup fixture output serialization failed".into()))?;
                conn.execute(
                    "INSERT INTO run_steps
                     (id, run_id, node_id, agent_provider, status, input_json, output_json,
                      started_at, finished_at, created_at)
                     VALUES (?1, ?2, ?3, 'synthetic', 'completed', ?4, ?5,
                             '2026-08-18T10:01:00Z', '2026-08-18T10:02:00Z',
                             '2026-08-18T10:01:00Z')",
                    params![step_id, run_id, node_id, input_json, output_json],
                )?;
                index_run_step(
                    conn,
                    &step_id,
                    run_id,
                    workflow_id,
                    node_id,
                    &input,
                    &output,
                    None,
                )?;
                Ok(())
            })?;

            if record.receipt_state.as_deref().unwrap_or("saved") != "saved" {
                return Err(DbError::Other(format!(
                    "lookup fixture run-step receipt {} is not saved",
                    record.alias
                )));
            }
            let key = OutputReceiptKey::new(run_id, node_id)
                .map_err(|error| DbError::Other(error.to_string()))?;
            let seed = OutputReceiptSeed::new(
                key,
                &step_id,
                OutputDisposition::default(),
                &body,
            )
            .map_err(|error| DbError::Other(error.to_string()))?;
            let storage = match record.storage_id.as_deref() {
                Some(value) => OutputStorageIdentity::parse(value)
                    .map_err(|error| DbError::Other(error.to_string()))?,
                None => seed
                    .storage_identity()
                    .ok_or_else(|| DbError::Other("lookup fixture receipt lacks storage identity".into()))?,
            };
            let artifact_path = storage.artifact_relative_path();
            let artifact_absolute_path = seeded._root.path.join(&artifact_path);
            if seed.artifact_required() {
                if let Some(parent) = artifact_absolute_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&artifact_absolute_path, body.as_bytes())?;
            }
            let memory_id = storage.memory_id();
            let memory_body = if seed.artifact_required() {
                bounded_prefix(&body, 2_000)
            } else {
                body.clone()
            };
            let artifact_digest = seed
                .artifact_required()
                .then(|| artifact_content_digest(body.as_bytes()));
            let artifact_reference = seed
                .artifact_required()
                .then(|| artifact_absolute_path.to_string_lossy().into_owned());
            let created_at = "2026-08-18T10:02:00Z";
            seeded.db.with_conn(|conn| {
                insert_output_receipt(conn, &seed, created_at)?;
                let transaction = conn.unchecked_transaction()?;
                transaction.execute(
                    "UPDATE output_receipts
                        SET storage_id = ?1
                      WHERE run_id = ?2 AND node_id = ?3 AND state = 'pending'",
                    params![memory_id, run_id, node_id],
                )?;
                let record = CanonicalMemoryRecord {
                    id: memory_id.clone(),
                    workflow_id: Some(workflow_id.to_owned()),
                    run_id: Some(run_id.to_owned()),
                    node_id: Some(node_id.to_owned()),
                    scope_type: MemoryScopeType::Workflow,
                    scope_key: workflow_id.to_owned(),
                    kind: if seed.artifact_required() {
                        "artifact".into()
                    } else {
                        "text".into()
                    },
                    memory_type: if seed.artifact_required() {
                        MemoryType::Artifact
                    } else {
                        MemoryType::Output
                    },
                    source: "run".into(),
                    title: seed.resolved_memory_title().to_owned(),
                    body: memory_body,
                    artifact_path: artifact_reference,
                    artifact_content_digest: artifact_digest.clone(),
                    pinned: false,
                    confidence: 1.0,
                    salience: 50,
                    status: MemoryStatus::Active,
                    supersedes_id: None,
                    fact_key: None,
                    observed_at: Some("2026-08-18T10:02:00Z".into()),
                    last_confirmed_at: None,
                    valid_from: None,
                    expires_at: None,
                    created_at: created_at.into(),
                };
                write_canonical_memory(&transaction, &record)?;
                if seed.artifact_required() {
                    persist_artifact_chunk_manifest_conn(
                        &transaction,
                        &memory_id,
                        artifact_digest.as_deref().unwrap(),
                        body.as_bytes(),
                        created_at,
                    )?;
                }
                transaction.execute(
                    "UPDATE output_receipts
                        SET state = 'saved', memory_id = ?1, updated_at = ?2
                      WHERE run_id = ?3 AND node_id = ?4",
                    params![memory_id, created_at, run_id, node_id],
                )?;
                transaction.commit()?;
                Ok(())
            })?;
            if seed.artifact_required() {
                seeded
                    .artifact_memory_ids
                    .insert(step_id.clone(), memory_id.clone());
            }
            seeded.add_alias(record.alias.clone(), step_id, Some(body));
            Ok(())
        }
        "evidence" => {
            let run_id = record.run_id.as_deref().unwrap_or(&case.run_id);
            let source = seeded
                .db
                .capture_run_evidence_sources(vec![EvidenceSourceInput {
                    run_id: run_id.to_owned(),
                    step_id: None,
                    node_id: record.node_id.clone(),
                    origin: parse_evidence_origin(
                        record.origin.as_deref().unwrap_or("host_recorded_observation"),
                    ),
                    producer_identity: Some("lookup-v1-synthetic".into()),
                    source_label: record.source_label.clone(),
                    observed_at: DateTime::parse_from_rfc3339("2026-08-18T10:03:00Z")
                        .expect("valid evidence fixture time")
                        .with_timezone(&Utc),
                    text: record.text.clone().unwrap_or_default(),
                    omission_reason: None,
                }])?
                .into_iter()
                .next()
                .ok_or_else(|| DbError::Other("lookup fixture evidence was not captured".into()))?;
            seeded.add_alias(
                record.alias.clone(),
                source.source_id,
                Some(source.normalized_text),
            );
            Ok(())
        }
        other => Err(DbError::Other(format!("unknown lookup history kind {other}"))),
    }
}

fn seed_case(case: &FixtureCase) -> SeededCase {
    let root = EvalTempDir::new();
    let db = Arc::new(
        Db::open_in_memory_with_artifact_root(root.path.clone())
            .expect("open lookup evaluation database"),
    );
    insert_workflow(
        &db,
        &case.workflow_id,
        &case.working_directory,
        case.automatic_retrieval_enabled,
        case.lookup_enabled,
    )
    .expect("seed lookup evaluation workflow");

    let mut extra_workflows = BTreeMap::<String, String>::new();
    for memory in &case.memories {
        if let Some(owner) = memory.owner_workflow_id.as_deref() {
            if owner != case.workflow_id {
                extra_workflows
                    .entry(owner.to_owned())
                    .or_insert_with(|| format!("{}/owner/{}", case.working_directory, owner));
            }
        }
    }
    for record in &case.history_records {
        if let Some(workflow_id) = record.workflow_id.as_deref() {
            if workflow_id != case.workflow_id {
                extra_workflows
                    .entry(workflow_id.to_owned())
                    .or_insert_with(|| format!("{}/history/{}", case.working_directory, workflow_id));
            }
        }
    }
    for workflow_id in extra_workflows.keys() {
        insert_workflow(&db, workflow_id, &extra_workflows[workflow_id], false, false)
            .expect("seed lookup evaluation extra workflow");
    }

    let mut run_workflows = HashMap::<String, String>::new();
    run_workflows.insert(case.run_id.clone(), case.workflow_id.clone());
    for record in &case.history_records {
        let workflow_id = record
            .workflow_id
            .as_deref()
            .unwrap_or(&case.workflow_id);
        let run_id = record.run_id.as_deref().unwrap_or(&case.run_id);
        if let Some(previous) = run_workflows.insert(run_id.to_owned(), workflow_id.to_owned()) {
            assert_eq!(previous, workflow_id, "fixture run must have one workflow");
        }
    }
    for (run_id, workflow_id) in &run_workflows {
        if run_id != &case.run_id {
            insert_run(&db, workflow_id, run_id).expect("seed lookup evaluation history run");
        }
    }
    insert_run(&db, &case.workflow_id, &case.run_id).expect("seed lookup evaluation run");

    let mut seeded = SeededCase {
        db,
        _root: root,
        aliases: HashMap::new(),
        reverse_aliases: HashMap::new(),
        bodies: HashMap::new(),
        artifact_memory_ids: HashMap::new(),
    };

    let mut memories = case.memories.iter().collect::<Vec<_>>();
    memories.sort_by_key(|memory| usize::from(memory.supersedes_id.is_some()));
    for memory in memories {
        insert_fixture_memory(&seeded.db, case, memory).expect("seed lookup evaluation memory");
        seeded.add_alias(memory.id.clone(), memory.id.clone(), Some(memory.body.clone()));
    }
    for record in &case.history_records {
        seed_history_record(&mut seeded, case, record)
            .expect("seed lookup evaluation historical source");
    }
    seeded
}

fn parse_tool(value: &str) -> LookupToolName {
    match value {
        "memory.search" => LookupToolName::MemorySearch,
        "memory.get" => LookupToolName::MemoryGet,
        "history.search" => LookupToolName::HistorySearch,
        "history.get" => LookupToolName::HistoryGet,
        other => panic!("unknown lookup tool {other}"),
    }
}

fn source_name(source: LookupSourceKind) -> &'static str {
    match source {
        LookupSourceKind::Memory => "memory",
        LookupSourceKind::RunStep => "run_step",
        LookupSourceKind::Evidence => "evidence",
    }
}

fn state_name(state: LookupSourceState) -> &'static str {
    match state {
        LookupSourceState::Current => "current",
        LookupSourceState::Historical => "historical",
        LookupSourceState::Incomplete => "incomplete",
        LookupSourceState::Unavailable => "unavailable",
        LookupSourceState::Deleted => "deleted",
        LookupSourceState::Expired => "expired",
        LookupSourceState::Retracted => "retracted",
        LookupSourceState::Replaced => "replaced",
        LookupSourceState::Unlinked => "unlinked",
    }
}

fn error_name(error: &LookupServiceError) -> String {
    match error {
        LookupServiceError::InvalidScope => "invalid_scope".into(),
        LookupServiceError::StorageUnavailable => "storage_unavailable".into(),
        LookupServiceError::ArtifactUnavailable => "artifact_unavailable".into(),
        LookupServiceError::Plan050IntegrationRequired => "plan050_integration_required".into(),
        LookupServiceError::Cancelled => "cancelled".into(),
        LookupServiceError::TimedOut => "timed_out".into(),
        LookupServiceError::Contract(contract) => match contract {
            LookupContractError::MalformedRequest => "malformed_request".into(),
            LookupContractError::RequestTooLarge => "request_too_large".into(),
            LookupContractError::EmptyQuery => "empty_query".into(),
            LookupContractError::QueryTooLarge => "query_too_large".into(),
            LookupContractError::TooManyQueryTerms => "too_many_query_terms".into(),
            LookupContractError::QueryTermTooLong => "query_term_too_long".into(),
            LookupContractError::InvalidResultLimit => "invalid_result_limit".into(),
            LookupContractError::InvalidTimeFilter => "invalid_time_filter".into(),
            LookupContractError::InvalidReference => "invalid_reference".into(),
            LookupContractError::InvalidCursor => "invalid_cursor".into(),
            LookupContractError::ResponseFieldTooLarge(field) => {
                format!("response_field_too_large:{field}")
            }
            LookupContractError::InvalidResponse => "invalid_response".into(),
            LookupContractError::ResponseTooLarge => "response_too_large".into(),
            LookupContractError::StepCallBudgetExhausted => "step_call_budget_exhausted".into(),
            LookupContractError::StepByteBudgetExhausted => "step_byte_budget_exhausted".into(),
        },
    }
}

fn expected_source_set(case: &FixtureCase) -> HashSet<String> {
    case.expected_recovered_ids.iter().cloned().collect()
}


fn source_kind_matches(expected: &str, sources: &[LookupSourceKind]) -> bool {
    if sources.is_empty() {
        return matches!(expected, "empty" | "memory" | "run_step" | "evidence" | "mixed");
    }
    if expected == "mixed" {
        let distinct = [
            sources.contains(&LookupSourceKind::Memory),
            sources.contains(&LookupSourceKind::RunStep),
            sources.contains(&LookupSourceKind::Evidence),
        ];
        return distinct.into_iter().filter(|present| *present).count() >= 2;
    }
    let Some(expected) = (match expected {
        "memory" => Some(LookupSourceKind::Memory),
        "run_step" => Some(LookupSourceKind::RunStep),
        "evidence" => Some(LookupSourceKind::Evidence),
        _ => None,
    }) else {
        return false;
    };
    !sources.is_empty() && sources.iter().all(|source| *source == expected)
}

fn state_matches(expected: &str, states: &[LookupSourceState]) -> bool {
    if expected == "empty" {
        return states.is_empty();
    }
    if expected == "historical" {
        return !states.is_empty() && states.iter().all(|state| *state == LookupSourceState::Historical);
    }
    let expected = match expected {
        "current" => LookupSourceState::Current,
        "incomplete" => LookupSourceState::Incomplete,
        "unavailable" => LookupSourceState::Unavailable,
        "deleted" => LookupSourceState::Deleted,
        "expired" => LookupSourceState::Expired,
        "retracted" => LookupSourceState::Retracted,
        "replaced" => LookupSourceState::Replaced,
        "unlinked" => LookupSourceState::Unlinked,
        _ => return false,
    };
    !states.is_empty() && states.iter().all(|state| *state == expected)
}

fn trace_report(seeded: &SeededCase, trace: &LookupTrace) -> TraceReport {
    TraceReport {
        tool: serde_json::to_value(trace.tool)
            .expect("lookup tool serializes")
            .as_str()
            .unwrap()
            .to_owned(),
        sources: trace
            .sources
            .iter()
            .map(|source| TraceSourceReport {
                reference: "opaque".into(),
                source: source_name(source.source).into(),
                source_id: seeded.alias_for(&source.source_id),
                version: source.version.clone(),
                state: state_name(source.state).into(),
                page: source.page,
            })
            .collect(),
        returned_bytes: trace.returned_bytes,
        classification: match trace.classification {
            LookupTraceClassification::RetrievedData => "retrieved_data".into(),
        },
    }
}

fn check_trace(
    tool: LookupToolName,
    output: &str,
    trace: &LookupTrace,
    violations: &mut Vec<String>,
) {
    if trace.classification != LookupTraceClassification::RetrievedData {
        violations.push("trace_classification".into());
    }
    if trace.tool != tool {
        violations.push("trace_tool".into());
    }
    if trace.returned_bytes != output.len() {
        violations.push(format!("trace_bytes:{}!={}", trace.returned_bytes, output.len()));
    }
    if trace.sources.iter().any(|source| source.reference.trim().is_empty()) {
        violations.push("trace_source_reference".into());
    }
    if trace.sources.iter().any(|source| source.source_id.trim().is_empty()) {
        violations.push("trace_source_identity".into());
    }
    if trace.sources.iter().any(|source| source.version.trim().is_empty()) {
        violations.push("trace_source_version".into());
    }
}

fn execute_lookup_action(
    seeded: &mut SeededCase,
    index: usize,
    action: &LookupActionSpec,
    session: &crate::db::agent_memory_lookup::AgentMemoryLookupSession,
    references: &mut HashMap<String, String>,
    versions: &mut HashMap<String, String>,
    cursors: &mut HashMap<String, String>,
    page_expectations: &mut HashMap<String, PageExpectation>,
    violations: &mut Vec<String>,
    artifact_violations: &mut Vec<String>,
    permission_violations: &mut Vec<String>,
) -> (ActionReport, u128, Vec<String>) {
    let tool = parse_tool(&action.tool);
    let source_alias = action.reference_source_id.as_deref();
    let requested_reference = source_alias.and_then(|alias| references.get(alias).cloned());
    let encoded = match tool {
        LookupToolName::MemorySearch | LookupToolName::HistorySearch => {
            let query = action
                .query
                .as_deref()
                .expect("search action must have query");
            serde_json::to_vec(&json!({
                "query": query,
                "limit": action.limit.unwrap_or(LOOKUP_LIMITS_V1.search_results),
            }))
            .expect("serialize lookup search request")
        }
        LookupToolName::MemoryGet | LookupToolName::HistoryGet => {
            let alias = source_alias.expect("get action must name a source alias");
            let reference = references
                .get(alias)
                .unwrap_or_else(|| panic!("get action references unknown prior source {alias}"));
            let cursor = if action.cursor_from_previous {
                if !page_expectations.contains_key(alias) {
                    artifact_violations.push(format!("missing_page_expectation:{index}"));
                }
                match cursors.get(alias).cloned() {
                    Some(cursor) => Some(cursor),
                    None => {
                        violations.push(format!("missing_cursor:{index}"));
                        None
                    }
                }
            } else {
                None
            };
            serde_json::to_vec(&json!({"reference": reference, "cursor": cursor}))
                .expect("serialize lookup get request")
        }
    };

    let started = Instant::now();
    let result = session.execute_json_with_trace(tool, &encoded, &UninterruptedLookup);
    let elapsed = started.elapsed().as_millis();
    let mut returned_source_ids = Vec::new();
    let mut response_bytes = 0usize;
    let mut returned_bytes = 0usize;
    let mut error = None;
    let mut trace_value = None;

    match result {
        Ok((output, trace)) => {
            response_bytes = output.len();
            returned_bytes = trace.returned_bytes;
            if response_bytes > match tool {
                LookupToolName::MemorySearch | LookupToolName::HistorySearch => SEARCH_RESPONSE_LIMIT,
                LookupToolName::MemoryGet | LookupToolName::HistoryGet => GET_RESPONSE_LIMIT,
            } {
                violations.push(format!("response_size:{index}"));
            }
            check_trace(tool, &output, &trace, violations);
            trace_value = Some(trace_report(seeded, &trace));
            match tool {
                LookupToolName::MemorySearch | LookupToolName::HistorySearch => {
                    let response: LookupSearchResponse = serde_json::from_str(&output)
                        .expect("lookup search response must parse");
                    response.validate().expect("lookup search response validates");
                    if response.tool != tool {
                        violations.push(format!("response_tool:{index}"));
                    }
                    let response_sources = response
                        .results
                        .iter()
                        .map(|hit| hit.source)
                        .collect::<Vec<_>>();
                    let response_states = response
                        .results
                        .iter()
                        .map(|hit| hit.state)
                        .collect::<Vec<_>>();
                    if !source_kind_matches(&action.expected_source_kind, &response_sources) {
                        violations.push(format!("source_kind:{index}"));
                    }
                    if !state_matches(&action.expected_state, &response_states) {
                        violations.push(format!("source_state:{index}"));
                    }

                    let mut response_references = HashSet::new();
                    let mut matched_trace_references = HashSet::new();
                    let mut response_source_aliases = Vec::with_capacity(response.results.len());
                    for hit in &response.results {
                        if !response_references.insert(hit.reference.clone()) {
                            violations.push(format!("duplicate_response_reference:{index}"));
                        }
                        let Some(trace_source) = trace.sources.iter().find(|source| {
                            source.reference == hit.reference
                                && !matched_trace_references.contains(&source.reference)
                        }) else {
                            violations.push(format!("trace_missing_hit_source:{index}"));
                            continue;
                        };
                        matched_trace_references.insert(trace_source.reference.clone());
                        if trace_source.source != hit.source || trace_source.state != hit.state {
                            violations.push(format!("trace_hit_mismatch:{index}"));
                        } else {
                            let alias = seeded.alias_for(&trace_source.source_id);
                            response_source_aliases.push(alias.clone());
                            if action.expected_error.is_none() {
                                if versions
                                    .get(&alias)
                                    .is_some_and(|previous| previous != &trace_source.version)
                                {
                                    violations.push(format!("source_version_changed:{index}"));
                                }
                                versions.insert(alias.clone(), trace_source.version.clone());
                                references.insert(alias, hit.reference.clone());
                            }
                        }
                        if action
                            .expected_body_contains
                            .iter()
                            .any(|needle| !hit.excerpt.contains(needle))
                        {
                            violations.push(format!("search_excerpt:{index}"));
                        }
                    }
                    if matched_trace_references.len() != trace.sources.len()
                        || response_references.len() != trace.sources.len()
                    {
                        violations.push(format!("trace_response_mapping:{index}"));
                    }
                    if action.expected_error.is_some() {
                        violations.push(format!("expected_error_missing:{index}"));
                        response_source_aliases.clear();
                    }
                    let mut expected = action.expected_source_ids.clone();
                    expected.sort();
                    let mut actual = response_source_aliases.clone();
                    actual.sort();
                    if actual != expected {
                        violations.push(format!("source_ids:{index}"));
                    }
                    returned_source_ids = response_source_aliases;
                }
                LookupToolName::MemoryGet | LookupToolName::HistoryGet => {
                    let response: LookupGetResponse = serde_json::from_str(&output)
                        .expect("lookup get response must parse");
                    response.validate().expect("lookup get response validates");
                    if response.tool != tool {
                        violations.push(format!("response_tool:{index}"));
                    }
                    let expected_alias = source_alias.expect("get source alias");
                    let mut response_source_aliases = Vec::new();
                    if trace.sources.is_empty() {
                        violations.push(format!("get_trace_missing_source:{index}"));
                    }
                    if trace.sources.len() != 1 {
                        violations.push(format!("get_trace_source_count:{index}"));
                    }
                    if let Some(trace_source) = trace.sources.first() {
                        let trace_matches_response = trace.sources.len() == 1
                            && requested_reference.as_deref() == Some(response.reference.as_str())
                            && response.reference == trace_source.reference
                            && response.source == trace_source.source
                            && response.state == trace_source.state
                            && trace_source.page == Some(response.page)
                            && response.source_version == trace_source.version;
                        if !trace_matches_response {
                            violations.push(format!("get_trace_mismatch:{index}"));
                        } else if action.expected_error.is_none() {
                            let alias = seeded.alias_for(&trace_source.source_id);
                            if alias != expected_alias {
                                violations.push(format!("get_source_alias:{index}"));
                            } else {
                                if versions
                                    .get(&alias)
                                    .is_some_and(|previous| previous != &trace_source.version)
                                {
                                    violations.push(format!("source_version_changed:{index}"));
                                }
                                versions.insert(alias.clone(), trace_source.version.clone());
                                response_source_aliases.push(alias.clone());
                                validate_get_body(
                                    seeded,
                                    &alias,
                                    &trace_source.source_id,
                                    response.state,
                                    response.body.as_deref(),
                                    response.page,
                                    action.cursor_from_previous,
                                    page_expectations,
                                    index,
                                    artifact_violations,
                                );
                            }
                        }
                    }
                    if action.cursor_from_previous {
                        if response.page == 0 {
                            artifact_violations.push(format!("cursor_did_not_advance:{index}"));
                        }
                        if response
                            .body
                            .as_deref()
                            .is_none_or(|body| body.trim().is_empty())
                        {
                            artifact_violations.push(format!("continuation_empty_body:{index}"));
                        }
                    }
                    if !state_matches(&action.expected_state, &[response.state]) {
                        violations.push(format!("get_state:{index}"));
                    }
                    if !source_kind_matches(&action.expected_source_kind, &[response.source]) {
                        violations.push(format!("get_kind:{index}"));
                    }
                    if response.body.as_ref().is_some_and(|body| body.len() > BODY_PAGE_LIMIT) {
                        artifact_violations.push(format!("body_page_size:{index}"));
                    }
                    if action
                        .expected_body_contains
                        .iter()
                        .any(|needle| !response.body.as_deref().unwrap_or("").contains(needle))
                    {
                        violations.push(format!("get_body:{index}"));
                    }
                    if action.expected_error.is_some() {
                        violations.push(format!("expected_error_missing:{index}"));
                        response_source_aliases.clear();
                    }
                    let mut expected = action.expected_source_ids.clone();
                    expected.sort();
                    let mut actual = response_source_aliases.clone();
                    actual.sort();
                    if actual != expected {
                        violations.push(format!("source_ids:{index}"));
                    }
                    returned_source_ids = response_source_aliases;
                    if let Some(next_cursor) = response.next_cursor {
                        cursors.insert(expected_alias.to_owned(), next_cursor);
                    }
                }
            }
        }
        Err(actual_error) => {
            let actual_name = error_name(&actual_error);
            error = Some(actual_name.clone());
            if action.expected_error.as_deref() != Some(actual_name.as_str()) {
                violations.push(format!("unexpected_error:{index}:{actual_name}"));
            }
            if action.expected_state == "permission_denied" && actual_name != "invalid_scope" {
                permission_violations.push(format!("permission_error:{index}:{actual_name}"));
            }
        }
    }

    (
        ActionReport {
            index,
            tool: action.tool.clone(),
            returned_source_ids,
            response_bytes,
            returned_bytes,
            error,
            trace: trace_value,
        },
        elapsed,
        source_alias.into_iter().map(ToOwned::to_owned).collect(),
    )
}

fn evaluate_case(case: &FixtureCase, now: DateTime<Utc>) -> LookupCaseReport {
    let mut seeded = seed_case(case);
    let context = crate::db::MemoryContext {
        workflow_id: case.workflow_id.clone(),
        working_directory: Some(case.working_directory.clone()),
    };
    let retrieval_request = crate::db::MemoryRetrievalRequest {
        workflow_id: &case.workflow_id,
        working_directory: Some(&case.working_directory),
        run_id: &case.run_id,
        node_id: &case.node_id,
        query_text: &case.initial_query,
        exclude_ids: &[],
        purpose: crate::db::RetrievalPurpose::Automatic,
    };
    let started = Instant::now();
    let initial_retrieval = if case.automatic_retrieval_enabled {
        seeded.db.retrieve_memories_at(&retrieval_request, now)
    } else {
        Default::default()
    };
    let initial_elapsed = started.elapsed().as_millis();
    let mut initial_errors = Vec::new();
    if let Some(error_code) = initial_retrieval.error_code {
        initial_errors.push(error_code.to_owned());
    }
    let automatic = initial_retrieval
        .items
        .iter()
        .cloned()
        .map(|use_record| AutomaticMemoryCandidate { use_record })
        .collect::<Vec<_>>();
    let automatic_count = initial_retrieval.items.len() + initial_retrieval.omitted_count;
    let prompt_value = PromptValue::text(TextOrigin::AuthoredTask, case.task_question.clone());
    let composition = compose_agent_memory_at(
        &seeded.db,
        &prompt_value,
        &context,
        &automatic,
        automatic_count,
        initial_retrieval.error_code.is_some(),
        false,
        now,
    )
    .expect("lookup evaluation composition succeeds");
    let initial_returned_ids = composition
        .uses
        .iter()
        .map(|item| item.memory_id.clone())
        .collect::<Vec<_>>();
    let initial_returned_set = initial_returned_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let initial_required_set = case
        .initial_required_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let initial_recovered_count = initial_required_set
        .iter()
        .filter(|id| initial_returned_set.contains(*id))
        .count();
    let mut wrong_memory_uses = initial_returned_ids
        .iter()
        .filter(|id| case.initial_forbidden_ids.contains(*id))
        .map(|id| format!("initial:{id}"))
        .collect::<Vec<_>>();
    wrong_memory_uses.extend(
        initial_returned_ids
            .iter()
            .filter(|id| !case.initial_allowed_ids.contains(*id))
            .map(|id| format!("initial_irrelevant:{id}")),
    );
    let initial_memory_bytes = composition.memory_bytes;
    let mut trace_violations = Vec::new();
    let mut artifact_violations = Vec::new();
    let mut budget_violations = Vec::new();
    let mut permission_violations = Vec::new();
    if initial_memory_bytes > INITIAL_MEMORY_LIMIT {
        budget_violations.push(format!("initial_memory_bytes:{initial_memory_bytes}"));
    }
    if initial_retrieval.error_code.is_some() {
        trace_violations.push("initial_retrieval_unavailable".into());
    }

    let service = AgentMemoryLookupService::new(seeded.db.clone(), &seeded._root.path);
    let session = service
        .start_session_at(
            &case.run_id,
            &case.node_id,
            Some(Path::new(&case.working_directory)),
            now,
        )
        .expect("lookup evaluation session starts");
    let mut references = HashMap::<String, String>::new();
    let mut versions = HashMap::<String, String>::new();
    let mut cursors = HashMap::<String, String>::new();
    let mut page_expectations = HashMap::<String, PageExpectation>::new();
    let mut actions = Vec::new();
    let mut latency_ms = vec![initial_elapsed];
    let mut revoked = false;
    for (index, action) in case.lookup_actions.iter().enumerate() {
        if !revoked
            && case
                .permission_revoke_after_calls
                .is_some_and(|after| session.budget().expect("lookup budget").calls_used() == after)
        {
            seeded
                .db
                .with_conn(|conn| {
                    conn.execute(
                        "UPDATE workflows SET memory_lookup_enabled = 0 WHERE id = ?1",
                        [&case.workflow_id],
                    )?;
                    Ok(())
                })
                .expect("revoke lookup permission in fixture");
            revoked = true;
        }
        let (action_report, elapsed, _) = execute_lookup_action(
            &mut seeded,
            index,
            action,
            &session,
            &mut references,
            &mut versions,
            &mut cursors,
            &mut page_expectations,
            &mut trace_violations,
            &mut artifact_violations,
            &mut permission_violations,
        );
        latency_ms.push(elapsed);
        actions.push(action_report);
    }

    let budget = session.budget().expect("lookup evaluation budget");
    if budget.calls_used() > LOOKUP_CALL_LIMIT {
        budget_violations.push(format!("lookup_calls:{}", budget.calls_used()));
    }
    if budget.returned_bytes() > INCREMENTAL_LOOKUP_LIMIT {
        budget_violations.push(format!("lookup_returned_bytes:{}", budget.returned_bytes()));
    }
    if let Some(expectation) = &case.budget_expectation {
        if budget.calls_used() != expectation.calls_used {
            budget_violations.push(format!(
                "expected_calls:{}!={}",
                budget.calls_used(),
                expectation.calls_used
            ));
        }
        if budget.returned_bytes() > expectation.returned_bytes_at_most {
            budget_violations.push(format!(
                "expected_returned_bytes:{}>{}",
                budget.returned_bytes(),
                expectation.returned_bytes_at_most
            ));
        }
        if expectation.probe_ninth_call {
            let probe = session.execute_json(
                LookupToolName::MemorySearch,
                br#"{"query":"budget alpha","limit":1}"#,
                &UninterruptedLookup,
            );
            if !matches!(
                probe,
                Err(LookupServiceError::Contract(
                    LookupContractError::StepCallBudgetExhausted
                ))
            ) {
                budget_violations.push("ninth_call_not_rejected".into());
            }
        }
    }

    let lookup_actual_ids = actions
        .iter()
        .flat_map(|action| action.returned_source_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let expected_recovered = expected_source_set(case);
    let lookup_recovered_ids = lookup_actual_ids
        .iter()
        .filter(|id| expected_recovered.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let initial_aliases = initial_returned_ids.iter().cloned().collect::<HashSet<_>>();
    let recovered_after_initial_miss_ids = lookup_recovered_ids
        .iter()
        .filter(|id| !initial_aliases.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    for id in &lookup_actual_ids {
        if case.lookup_forbidden_ids.contains(id) {
            wrong_memory_uses.push(format!("lookup:{id}"));
        } else if !expected_recovered.contains(id)
            && !case.lookup_allowed_historical_ids.contains(id)
        {
            wrong_memory_uses.push(format!("lookup_irrelevant:{id}"));
        }
    }
    let lookup_errors = actions
        .iter()
        .filter_map(|action| {
            action
                .error
                .as_ref()
                .map(|error| format!("{}:{error}", action.index))
        })
        .collect::<Vec<_>>();
    let actual_source_free = initial_returned_ids.is_empty() && lookup_actual_ids.is_empty();
    let actual_no_evidence =
        actual_source_free && initial_errors.is_empty() && lookup_errors.is_empty();
    if case.expected_no_evidence && !actual_source_free {
        trace_violations.push("no_evidence_case_returned_data".into());
    }
    let expected_evidence_ids = case
        .initial_required_ids
        .iter()
        .chain(case.expected_recovered_ids.iter())
        .chain(
            case.lookup_actions
                .iter()
                .flat_map(|action| action.expected_source_ids.iter()),
        )
        .cloned()
        .collect::<HashSet<_>>();
    if !case.expected_no_evidence && expected_evidence_ids.is_empty() {
        trace_violations.push("labelled_evidence_case_has_no_expected_source".into());
    }
    let expected_denied = actions
        .iter()
        .zip(case.lookup_actions.iter())
        .filter_map(|(action, spec)| {
            (spec.expected_state == "permission_denied"
                && spec.expected_error.is_some()
                && action.error.as_deref() == spec.expected_error.as_deref())
            .then(|| format!("{}:{}", action.index, action.error.as_deref().unwrap_or("error")))
        })
        .collect::<Vec<_>>();
    if !expected_recovered.is_empty() && lookup_recovered_ids.is_empty() {
        trace_violations.push("expected_lookup_evidence_not_recovered".into());
    }

    let lookup_recall = (!expected_recovered.is_empty())
        .then(|| lookup_recovered_ids.len() as f64 / expected_recovered.len() as f64);
    LookupCaseReport {
        id: case.id.clone(),
        category: case.category.clone(),
        split: case.split.clone(),
        initial_required_count: case.initial_required_ids.len(),
        initial_recovered_count,
        initial_returned_ids,
        initial_recall: (!initial_required_set.is_empty())
            .then(|| initial_recovered_count as f64 / initial_required_set.len() as f64),
        lookup_required_count: expected_recovered.len(),
        lookup_recovered_count: lookup_recovered_ids.len(),
        lookup_recovered_ids,
        recovered_after_initial_miss_ids,
        lookup_recall,
        expected_denied,
        wrong_memory_uses,
        lookup_errors,
        unexpected_errors: actions
            .iter()
            .zip(case.lookup_actions.iter())
            .filter_map(|(action, spec)| {
                action
                    .error
                    .as_deref()
                    .filter(|error| spec.expected_error.as_deref() != Some(*error))
                    .map(|error| format!("{}:{}", action.index, error))
            })
            .collect(),
        initial_errors,
        trace_violations,
        artifact_violations,
        budget_violations,
        permission_violations,
        initial_memory_bytes,
        lookup_calls: budget.calls_used(),
        lookup_returned_bytes: budget.returned_bytes(),
        actions,
        latency_ms,
        expected_no_evidence: case.expected_no_evidence,
        actual_source_free,
        actual_no_evidence,
    }
}

fn aggregate(reports: &[LookupCaseReport]) -> LookupAggregateReport {
    let initial_required_count = reports
        .iter()
        .map(|report| report.initial_required_count)
        .sum::<usize>();
    let initial_recovered_count = reports
        .iter()
        .map(|report| report.initial_recovered_count)
        .sum::<usize>();
    let lookup_required_count = reports
        .iter()
        .map(|report| report.lookup_required_count)
        .sum::<usize>();
    let lookup_recovered_count = reports
        .iter()
        .map(|report| report.lookup_recovered_count)
        .sum::<usize>();
    LookupAggregateReport {
        cases: reports.len(),
        initial_required_count,
        initial_recovered_count,
        initial_recall: (initial_required_count > 0)
            .then(|| initial_recovered_count as f64 / initial_required_count as f64),
        lookup_required_count,
        lookup_recovered_count,
        lookup_recall: (lookup_required_count > 0)
            .then(|| lookup_recovered_count as f64 / lookup_required_count as f64),
        recovered_after_initial_miss: reports
            .iter()
            .map(|report| report.recovered_after_initial_miss_ids.len())
            .sum(),
        expected_no_evidence_cases: reports
            .iter()
            .filter(|report| report.expected_no_evidence)
            .count(),
        actual_source_free_cases: reports
            .iter()
            .filter(|report| report.actual_source_free)
            .count(),
        actual_no_evidence_cases: reports
            .iter()
            .filter(|report| report.actual_no_evidence)
            .count(),
        unexpected_nonempty_no_evidence_cases: reports
            .iter()
            .filter(|report| report.expected_no_evidence && !report.actual_source_free)
            .count(),
        expected_denied: reports
            .iter()
            .map(|report| report.expected_denied.len())
            .sum(),
        lookup_errors: reports
            .iter()
            .map(|report| report.lookup_errors.len())
            .sum(),
        wrong_memory_uses: reports
            .iter()
            .map(|report| report.wrong_memory_uses.len())
            .sum(),
        unexpected_errors: reports
            .iter()
            .map(|report| report.unexpected_errors.len())
            .sum(),
        initial_errors: reports.iter().map(|report| report.initial_errors.len()).sum(),
        trace_violations: reports
            .iter()
            .map(|report| report.trace_violations.len())
            .sum(),
        artifact_violations: reports
            .iter()
            .map(|report| report.artifact_violations.len())
            .sum(),
        budget_violations: reports
            .iter()
            .map(|report| report.budget_violations.len())
            .sum(),
        permission_violations: reports
            .iter()
            .map(|report| report.permission_violations.len())
            .sum(),
        initial_memory_bytes: reports
            .iter()
            .map(|report| report.initial_memory_bytes)
            .sum(),
        lookup_calls: reports.iter().map(|report| report.lookup_calls).sum(),
        lookup_returned_bytes: reports
            .iter()
            .map(|report| report.lookup_returned_bytes)
            .sum(),
        max_initial_memory_bytes: reports
            .iter()
            .map(|report| report.initial_memory_bytes)
            .max()
            .unwrap_or(0),
        max_lookup_calls: reports
            .iter()
            .map(|report| report.lookup_calls)
            .max()
            .unwrap_or(0),
        max_lookup_returned_bytes: reports
            .iter()
            .map(|report| report.lookup_returned_bytes)
            .max()
            .unwrap_or(0),
    }
}

fn evaluate(fixture: &Fixture) -> LookupEvaluationReport {
    let now = parse_clock(fixture);
    let cases = fixture
        .cases
        .iter()
        .map(|case| evaluate_case(case, now))
        .collect::<Vec<_>>();
    let development = cases
        .iter()
        .filter(|case| case.split == "development")
        .cloned()
        .collect::<Vec<_>>();
    let validation = cases
        .iter()
        .filter(|case| case.split == "validation")
        .cloned()
        .collect::<Vec<_>>();
    LookupEvaluationReport {
        fixture_version: fixture.version.clone(),
        evaluator_version: fixture.evaluator_version.clone(),
        lookup_contract_version: fixture.lookup_contract_version,
        fixed_clock: fixture.fixed_clock.clone(),
        fixture_digest: fixture_digest(),
        development: aggregate(&development),
        validation: aggregate(&validation),
        current_runtime: aggregate(&cases),
        cases,
        timing_excluded_from_repeatability: true,
        limitations: vec![
            "Labels are synthetic necessary-memory and expected-fact annotations authored before production evaluation.".into(),
            "The recall_miss_recovery category labels an intended incremental-recall scenario; observed initial recall may already be complete.".into(),
            "Recovery, wrong-memory use, bytes, calls, and latency measure host/database behavior, not model answer accuracy.".into(),
            "Opaque references, generated evidence UUIDs, and wall-clock latency are excluded from repeatability comparison.".into(),
            "No provider, account, billing source, live runtime, or live usefulness/cost evidence is exercised.".into(),
            "Native remaining-context allowance and live model usefulness or cost are unmeasured by this host evaluator.".into(),
            "Initial memory injection is measured through the production retriever and composer; lookup reads are measured through AgentMemoryLookupService.".into(),
            "Source-free responses, successful no-evidence abstentions, expected permission denials, and unexpected errors are reported separately.".into(),
            "Automatic-off cases follow the production retrieval_enabled gate and compose with no automatic candidates.".into(),
        ],
    }
}

fn stable_report(report: &LookupEvaluationReport) -> LookupEvaluationReport {
    let mut stable = report.clone();
    for case in &mut stable.cases {
        case.latency_ms.clear();
    }
    stable
}

fn write_draft_report(report: &LookupEvaluationReport) {
    let Ok(path) = std::env::var(REPORT_ENV) else {
        return;
    };
    let path = PathBuf::from(path);
    assert!(path.is_absolute(), "{REPORT_ENV} must be an absolute draft path");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create lookup draft parent");
    }
    let rendered =
        serde_json::to_string_pretty(report).expect("serialize lookup draft") + "\n";
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("lookup draft path must be new; existing reports are immutable");
    file.write_all(rendered.as_bytes())
        .expect("write lookup draft report");
    file.sync_all().expect("sync lookup draft report");
}

fn assert_fixture_contract(fixture: &Fixture) {
    assert_eq!(fixture.version, "lookup-v1");
    assert_eq!(fixture.evaluator_version, "memory-lookup-eval-1");
    assert_eq!(fixture.lookup_contract_version, LOOKUP_CONTRACT_VERSION);
    assert_eq!(fixture_digest(), FIXTURE_SHA256);
    assert_eq!(fixture.limits.initial_memory_bytes, INITIAL_MEMORY_LIMIT);
    assert_eq!(fixture.limits.incremental_lookup_bytes, INCREMENTAL_LOOKUP_LIMIT);
    assert_eq!(fixture.limits.lookup_calls, LOOKUP_CALL_LIMIT);
    assert_eq!(fixture.limits.body_page_bytes, BODY_PAGE_LIMIT);
    assert_eq!(fixture.limits.search_results, LOOKUP_LIMITS_V1.search_results);
    assert_eq!(fixture.cases.len(), 12);

    let mut ids = HashSet::new();
    let mut categories = HashSet::new();
    let mut development = 0usize;
    let mut validation = 0usize;
    let mut artifacts = 0usize;
    let mut empty = 0usize;
    let mut memory_ids = HashSet::new();
    let mut aliases = HashSet::new();
    let mut has_automatic_off_lookup_recovery = false;
    let mut has_automatic_on_lookup_off = false;
    let mut has_both_off_abstention = false;
    for case in &fixture.cases {
        assert!(ids.insert(&case.id), "duplicate lookup case {}", case.id);
        categories.insert(case.category.as_str());
        match case.split.as_str() {
            "development" => development += 1,
            "validation" => validation += 1,
            other => panic!("unknown lookup fixture split {other}"),
        }
        assert!(!case.task_question.trim().is_empty());
        assert!(!case.expected_answer_facts.is_empty());
        assert!(!case.initial_query.trim().is_empty());
        assert!(!case.lookup_actions.is_empty());
        if case.expected_no_evidence {
            empty += 1;
        }
        for memory in &case.memories {
            assert!(memory_ids.insert(&memory.id), "duplicate memory id {}", memory.id);
            assert!(!memory.title.trim().is_empty());
            assert!(!memory.body.trim().is_empty());
            assert!(matches!(memory.scope_type.as_str(), "user" | "workspace" | "workflow"));
            assert!(matches!(memory.status.as_str(), "active" | "superseded" | "retracted"));
            assert!(memory.confidence.unwrap_or(0.9) >= 0.0);
            assert!(memory.confidence.unwrap_or(0.9) <= 1.0);
        }
        for record in &case.history_records {
            assert!(aliases.insert(&record.alias), "duplicate historical alias {}", record.alias);
            match record.kind.as_str() {
                "run_step" => {
                    assert!(record.output_text.is_some());
                    if record.artifact {
                        artifacts += 1;
                        assert!(record.artifact_repeat.unwrap_or(0) > 1);
                        let body = history_body(record);
                        assert!(body.len() >= 32 * 1024);
                    }
                }
                "evidence" => {
                    assert!(!record.text.as_deref().unwrap_or("").is_empty());
                }
                other => panic!("unknown lookup history kind {other}"),
            }
        }
        let known = case
            .memories
            .iter()
            .map(|memory| memory.id.as_str())
            .chain(case.history_records.iter().map(|record| record.alias.as_str()))
            .collect::<HashSet<_>>();
        for source_id in case
            .initial_required_ids
            .iter()
            .chain(case.initial_allowed_ids.iter())
            .chain(case.initial_forbidden_ids.iter())
            .chain(case.expected_recovered_ids.iter())
            .chain(case.lookup_forbidden_ids.iter())
            .chain(case.lookup_allowed_historical_ids.iter())
        {
            assert!(known.contains(source_id.as_str()), "unknown fixture source {source_id} in {}", case.id);
        }
        for action in &case.lookup_actions {
            let tool = parse_tool(&action.tool);
            match tool {
                LookupToolName::MemorySearch | LookupToolName::HistorySearch => {
                    assert!(!action.query.as_deref().unwrap_or("").trim().is_empty());
                }
                LookupToolName::MemoryGet | LookupToolName::HistoryGet => {
                    assert!(action.reference_source_id.is_some());
                }
            }
            for source_id in &action.expected_source_ids {
                assert!(known.contains(source_id.as_str()), "unknown action source {source_id} in {}", case.id);
            }
            if action.expected_error.is_some() {
                assert!(
                    action.expected_source_ids.is_empty(),
                    "error action cannot label returned sources in {}",
                    case.id
                );
            }
        }
        let has_expected_evidence = case
            .initial_required_ids
            .iter()
            .chain(case.expected_recovered_ids.iter())
            .chain(
                case.lookup_actions
                    .iter()
                    .flat_map(|action| action.expected_source_ids.iter()),
            )
            .next()
            .is_some();
        assert!(
            case.expected_no_evidence || has_expected_evidence,
            "non-empty evidence case {} must label a source",
            case.id
        );
        has_automatic_off_lookup_recovery |=
            case.lookup_enabled
                && !case.automatic_retrieval_enabled
                && !case.expected_recovered_ids.is_empty();
        has_automatic_on_lookup_off |=
            !case.lookup_enabled
                && case.automatic_retrieval_enabled
                && !case.initial_required_ids.is_empty()
                && case
                    .lookup_actions
                    .iter()
                    .any(|action| action.expected_error.as_deref() == Some("invalid_scope"));
        has_both_off_abstention |=
            !case.lookup_enabled && !case.automatic_retrieval_enabled && case.expected_no_evidence;
    }
    assert_eq!(development, 7);
    assert_eq!(validation, 5);
    assert!(empty >= 2);
    assert!(artifacts >= 2);
    assert!(categories.contains("recall_miss_recovery"));
    assert!(categories.contains("historical_reports"));
    assert!(categories.contains("correction_aware_reads"));
    assert!(categories.contains("wrong_source_privacy"));
    assert!(categories.contains("independent_permissions"));
    assert!(categories.contains("bounded_artifacts"));
    assert!(categories.contains("shared_budget_and_calls"));
    assert!(categories.contains("no_evidence"));
    assert!(has_automatic_off_lookup_recovery);
    assert!(has_automatic_on_lookup_off);
    assert!(has_both_off_abstention);
    let budget_case = fixture
        .cases
        .iter()
        .find(|case| case.category == "shared_budget_and_calls")
        .expect("budget fixture case");
    assert_eq!(budget_case.lookup_actions.len(), LOOKUP_CALL_LIMIT);
    assert_eq!(budget_case.budget_expectation.as_ref().unwrap().calls_used, LOOKUP_CALL_LIMIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_fixture_contract() {
        assert_fixture_contract(&load_fixture());
    }

    #[test]
    fn lookup_evaluation_is_repeatable_and_hard_invariants_hold() {
        let fixture = load_fixture();
        assert_fixture_contract(&fixture);
        let first = evaluate(&fixture);
        let second = evaluate(&fixture);
        assert_eq!(stable_report(&first), stable_report(&second));
        let trace_diagnostics = first
            .cases
            .iter()
            .filter(|case| !case.trace_violations.is_empty())
            .map(|case| (case.id.as_str(), &case.trace_violations))
            .collect::<Vec<_>>();
        for aggregate in [&first.current_runtime, &first.development, &first.validation] {
            assert_eq!(aggregate.initial_errors, 0);
            assert_eq!(aggregate.unexpected_errors, 0);
            assert_eq!(aggregate.wrong_memory_uses, 0);
            assert_eq!(
                aggregate.trace_violations,
                0,
                "trace violations by case: {trace_diagnostics:?}"
            );
            assert_eq!(aggregate.artifact_violations, 0);
            assert_eq!(aggregate.budget_violations, 0);
            assert_eq!(aggregate.permission_violations, 0);
            assert!(aggregate.max_initial_memory_bytes <= INITIAL_MEMORY_LIMIT);
            assert!(aggregate.max_lookup_calls <= LOOKUP_CALL_LIMIT);
            assert!(aggregate.max_lookup_returned_bytes <= INCREMENTAL_LOOKUP_LIMIT);
        }
        for case_id in ["l01", "l02"] {
            let fixture_case = fixture
                .cases
                .iter()
                .find(|case| case.id == case_id)
                .expect("recall-recovery fixture case");
            let report = first
                .cases
                .iter()
                .find(|case| case.id == case_id)
                .expect("recall-recovery case report");
            let mut expected_lookup = fixture_case.expected_recovered_ids.clone();
            expected_lookup.sort();
            let mut actual_lookup = report.lookup_recovered_ids.clone();
            actual_lookup.sort();
            assert_eq!(actual_lookup, expected_lookup);
            for source_id in fixture_case
                .initial_required_ids
                .iter()
                .chain(fixture_case.expected_recovered_ids.iter())
            {
                assert!(
                    report.initial_returned_ids.contains(source_id)
                        || report.lookup_recovered_ids.contains(source_id),
                    "{case_id} necessary source {source_id} was not observed"
                );
            }
            if report.initial_recovered_count < report.initial_required_count {
                assert!(!report.recovered_after_initial_miss_ids.is_empty());
            }
        }
        let automatic_off = first
            .cases
            .iter()
            .find(|case| case.id == "l07")
            .expect("automatic-off lookup-on case report");
        assert_eq!(automatic_off.initial_returned_ids, Vec::<String>::new());
        assert!(!automatic_off.recovered_after_initial_miss_ids.is_empty());
        let automatic_on_lookup_off = first
            .cases
            .iter()
            .find(|case| case.id == "l12")
            .expect("automatic-on lookup-off case report");
        assert_eq!(automatic_on_lookup_off.initial_recovered_count, 1);
        assert_eq!(
            automatic_on_lookup_off.actions[0].error.as_deref(),
            Some("invalid_scope")
        );
        let both_off = first
            .cases
            .iter()
            .find(|case| case.id == "l11")
            .expect("both-off abstention case report");
        assert!(both_off.actual_source_free);
        assert!(!both_off.actual_no_evidence);
        assert_eq!(both_off.lookup_errors.len(), 2);
        assert!(first.current_runtime.recovered_after_initial_miss > 0);
        assert_eq!(first.current_runtime.expected_no_evidence_cases, 2);
        assert_eq!(first.current_runtime.actual_source_free_cases, 2);
        assert_eq!(first.current_runtime.actual_no_evidence_cases, 1);
        assert_eq!(first.current_runtime.expected_denied, 4);
        assert_eq!(first.current_runtime.lookup_errors, 4);
        assert!(first.timing_excluded_from_repeatability);
        write_draft_report(&first);
        println!("{}", serde_json::to_string_pretty(&first).expect("serialize lookup evaluation"));
    }

    #[test]
    fn lookup_evaluation_requires_response_backed_get_sources_and_separates_errors() {
        let report = evaluate(&load_fixture());
        let historical = report
            .cases
            .iter()
            .find(|case| case.id == "l03")
            .expect("historical case report");
        for action in historical
            .actions
            .iter()
            .filter(|action| action.tool == "history.get")
        {
            assert_eq!(action.returned_source_ids.len(), 1);
            assert!(action.error.is_none());

            assert!(action.trace.as_ref().is_some_and(|trace| trace.sources.len() == 1));
        }

        let revoked = report
            .cases
            .iter()
            .find(|case| case.id == "l08")
            .expect("revoked permission case report");
        let revoked_get = revoked
            .actions
            .iter()
            .find(|action| action.tool == "memory.get")
            .expect("revoked get action");
        assert_eq!(revoked_get.returned_source_ids, Vec::<String>::new());
        assert_eq!(revoked_get.error.as_deref(), Some("invalid_scope"));

        let automatic_only = report
            .cases
            .iter()
            .find(|case| case.id == "l12")
            .expect("automatic-only case report");
        assert!(!automatic_only.actual_source_free);
        assert_eq!(automatic_only.lookup_errors.len(), 1);
        assert!(automatic_only
            .trace_violations
            .iter()
            .all(|violation| violation != "labelled_evidence_case_has_no_expected_source"));
        let privacy = report
            .cases
            .iter()
            .find(|case| case.id == "l06")
            .expect("privacy case report");
        assert!(privacy.actual_no_evidence);
        assert!(privacy.lookup_errors.is_empty());
    }

    #[test]
    fn lookup_budget_is_shared_across_interleaved_memory_and_history_calls() {
        let fixture = load_fixture();
        let case = fixture
            .cases
            .iter()
            .find(|case| case.id == "l05")
            .expect("interleaved budget fixture case");
        let seeded = seed_case(case);
        let service = AgentMemoryLookupService::new(seeded.db.clone(), &seeded._root.path);
        let session = service
            .start_session_at(
                &case.run_id,
                &case.node_id,
                Some(Path::new(&case.working_directory)),
                parse_clock(&fixture),
            )
            .expect("start interleaved budget session");
        let memory_request =
            serde_json::to_vec(&json!({"query": "shipping window", "limit": 1}))
                .expect("serialize memory budget request");
        let history_request =
            serde_json::to_vec(&json!({"query": "shipping window", "limit": 1}))
                .expect("serialize history budget request");
        for index in 0..LOOKUP_CALL_LIMIT {
            let (tool, request) = if index % 2 == 0 {
                (LookupToolName::MemorySearch, memory_request.as_slice())
            } else {
                (LookupToolName::HistorySearch, history_request.as_slice())
            };
            let (output, trace) = session
                .execute_json_with_trace(tool, request, &UninterruptedLookup)
                .expect("interleaved lookup call succeeds");
            assert!(!output.is_empty());
            assert_eq!(trace.tool, tool);
        }
        let budget = session.budget().expect("read interleaved lookup budget");
        assert_eq!(budget.calls_used(), LOOKUP_CALL_LIMIT);
        assert!(budget.returned_bytes() <= INCREMENTAL_LOOKUP_LIMIT);
        assert!(matches!(
            session.execute_json(
                LookupToolName::HistorySearch,
                &history_request,
                &UninterruptedLookup
            ),
            Err(LookupServiceError::Contract(
                LookupContractError::StepCallBudgetExhausted
            ))
        ));
    }


    #[test]
    fn lookup_report_keeps_live_usefulness_and_cost_unproven() {
        let report = evaluate(&load_fixture());
        assert!(report
            .limitations
            .iter()
            .any(|limitation| limitation.contains("model answer accuracy")));
        assert!(report
            .limitations
            .iter()
            .any(|limitation| limitation.contains("live usefulness/cost")));
        assert!(report.timing_excluded_from_repeatability);
        assert!(report.cases.iter().all(|case| case.lookup_recall.is_none_or(|value| (0.0..=1.0).contains(&value))));
    }
}
