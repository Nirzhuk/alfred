//! A deterministic, synthetic quality report for the v1 memory policy.
//!
//! This module intentionally lives behind `cfg(test)`. It seeds the real
//! SQLite schema and calls the production retriever/renderers; it does not
//! copy the ranking algorithm or call a provider. The fixture is a labelled
//! regression set, not a claim about model answer quality.

use super::history::index_memory;
use super::{is_expired_at, Db, DbError, MemoryContext, MemoryRetrievalRequest, RetrievalPurpose};
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;

#[path = "memory_eval_composition.rs"]
mod composition;

#[path = "memory_eval_corrections.rs"]
mod corrections;
#[path = "memory_eval_lookup.rs"]
mod lookup;

const FIXTURE_VERSION: &str = "v1";
const EVALUATOR_VERSION: &str = "memory-eval-1";
const POLICY_VERSION: &str = "lexical-plus-recent-v1";
const RETRIEVAL_MAX_BYTES: usize = 6_000;
const RETRIEVAL_ITEM_MAX_BYTES: usize = 1_200;
const BASELINE_PATH: &str = "tests/fixtures/memory-quality/baseline-v1.json";
const V2_BASELINE_PATH: &str = "tests/fixtures/memory-quality/baseline-v2.json";

#[derive(Debug, Deserialize)]
struct Fixture {
    version: String,
    evaluator_version: String,
    policy_version: String,
    fixed_clock: String,
    source_types: Vec<String>,
    typed_source_cases: Vec<String>,
    cohorts: BTreeMap<String, Vec<String>>,
    cases: Vec<FixtureCase>,
}

#[derive(Debug, Deserialize)]
struct FixtureCase {
    id: String,
    category: String,
    split: String,
    layer: String,
    evaluation_layers: Vec<String>,
    workflow_id: String,
    working_directory: String,
    run_id: String,
    node_id: String,
    query_text: String,
    task_question: String,
    prompt_input: String,
    sources: Vec<FixtureSource>,
    final_composition: FinalCompositionLabel,
    expected_answer_facts: Vec<String>,
    memories: Vec<FixtureMemory>,
    required_ids: Vec<String>,
    allowed_ids: Vec<String>,
    forbidden_ids: Vec<String>,
    #[serde(default)]
    exclude_ids: Vec<String>,
    expected_empty: bool,
    #[serde(default)]
    pinned_ids: Vec<String>,
    #[serde(default)]
    explicit_selection_ids: Vec<String>,
    #[serde(default)]
    renderer_required_pinned_ids: Vec<String>,
    #[serde(default)]
    renderer_required_explicit_ids: Vec<String>,
    #[serde(default)]
    renderer_allowed_pinned_ids: Vec<String>,
    #[serde(default)]
    renderer_allowed_explicit_ids: Vec<String>,
    #[serde(default)]
    renderer_forbidden_pinned_ids: Vec<String>,
    #[serde(default)]
    renderer_forbidden_explicit_ids: Vec<String>,
    #[serde(default)]
    context_template_inputs: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct FixtureSource {
    kind: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct FinalCompositionLabel {
    status: String,
    plan: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct FixtureMemory {
    id: String,
    title: String,
    body: String,
    scope_type: String,
    #[serde(default)]
    owner_workflow_id: Option<String>,
    #[serde(default)]
    linked_to: Option<String>,
    memory_type: String,
    source: String,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    pinned: bool,
    #[serde(default = "default_confidence")]
    confidence: f64,
    #[serde(default = "default_salience")]
    salience: i64,
    status: String,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    last_confirmed_at: Option<String>,
    created_at: String,
    updated_at: String,
}
fn default_confidence() -> f64 {
    0.9
}
fn default_salience() -> i64 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CaseReport {
    id: String,
    category: String,
    split: String,
    layer: String,
    evaluation_layers: Vec<String>,
    required_count: usize,
    recovered_count: usize,
    returned_count: usize,
    allowed_returned_count: usize,
    recall: Option<f64>,
    precision: Option<f64>,
    expected_empty: bool,
    actual_empty: bool,
    retrieved_ids: Vec<String>,
    known_misses: Vec<String>,
    irrelevant_returned: Vec<String>,
    quality_forbidden_returned: Vec<String>,
    retrieval_errors: Vec<String>,
    scope_lifecycle_violations: Vec<String>,
    exclusion_violations: Vec<String>,
    budget_violations: Vec<String>,
    unsupported_memory_uses: Vec<String>,
    rendered_bytes: usize,
    pinned_bytes: Option<usize>,
    explicit_bytes: Option<usize>,
    final_composition_evaluated: bool,
    query_bytes: usize,
    prompt_input_bytes: usize,
    query_bounded_to_bytes: usize,
    pinned_candidate_ids: Vec<String>,
    pinned_included_ids: Vec<String>,
    explicit_selected_ids: Vec<String>,
    explicit_included_ids: Vec<String>,
    pinned_lifecycle_violations: Vec<String>,
    explicit_lifecycle_violations: Vec<String>,
    pinned_forbidden_lifecycle_violations: Vec<String>,
    explicit_forbidden_lifecycle_violations: Vec<String>,
    pinned_unexpected_renderer_ids: Vec<String>,
    explicit_unexpected_renderer_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct AggregateReport {
    cases: usize,
    required_count: usize,
    recovered_count: usize,
    returned_count: usize,
    allowed_returned_count: usize,
    recall: Option<f64>,
    precision: Option<f64>,
    expected_empty_cases: usize,
    actual_empty_cases: usize,
    unexpected_nonempty_cases: usize,
    known_misses: usize,
    irrelevant_returned: usize,
    quality_forbidden_returned: usize,
    scope_lifecycle_violations: usize,
    exclusion_violations: usize,
    budget_violations: usize,
    retrieval_errors: usize,
    unsupported_memory_uses: usize,
    rendered_bytes: usize,
    pinned_lifecycle_violations: usize,
    explicit_lifecycle_violations: usize,
    pinned_forbidden_lifecycle_violations: usize,
    explicit_forbidden_lifecycle_violations: usize,
    pinned_unexpected_renderer_ids: usize,
    explicit_unexpected_renderer_ids: usize,
    long_query_cases: usize,
    long_prompt_cases: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct EvaluationReport {
    fixture_version: String,
    evaluator_version: String,
    policy_version: String,
    fixed_clock: String,
    fixture_digest: String,
    development: AggregateReport,
    validation: AggregateReport,
    current_runtime: AggregateReport,
    no_memory_baseline: NoMemoryReport,
    recall_off_baseline: RecallOffReport,
    categories: BTreeMap<String, AggregateReport>,
    cases: Vec<CaseReport>,
    limitations: Vec<String>,
    input_source_types: Vec<String>,
    frozen_cohorts: BTreeMap<String, Vec<String>>,
}

/// Versioned comparison output used when a later policy changes retrieval
/// results while the immutable v1 labels and report remain available.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PolicyComparison {
    baseline_fixture_version: String,
    baseline_policy_version: String,
    active_policy_version: String,
    baseline_recall: Option<f64>,
    active_recall: Option<f64>,
    recall_delta: Option<f64>,
    baseline_precision: Option<f64>,
    active_precision: Option<f64>,
    precision_delta: Option<f64>,
    development: MetricComparison,
    validation: MetricComparison,
    supported_lexical: MetricComparison,
    exact_identifier: MetricComparison,
    eligible_supported: MetricComparison,
    active_missed_ids: Vec<String>,
    active_raw_report_missed_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MetricSnapshot {
    cases: usize,
    required_count: usize,
    recovered_count: usize,
    returned_count: usize,
    allowed_returned_count: usize,
    recall: Option<f64>,
    precision: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MetricComparison {
    baseline: MetricSnapshot,
    active: MetricSnapshot,
    recall_delta: Option<f64>,
    precision_delta: Option<f64>,
}

fn metric_delta(baseline: Option<f64>, active: Option<f64>) -> Option<f64> {
    baseline.zip(active).map(|(before, after)| {
        ((after - before) * 1_000_000_000_000.0).round() / 1_000_000_000_000.0
    })
}

fn reviewed_fixture_digest() -> String {
    format!(
        "{:x}",
        Sha256::digest(include_str!("../../../tests/fixtures/memory-quality/v1.json").as_bytes())
    )
}

fn metric_snapshot(report: &AggregateReport) -> MetricSnapshot {
    MetricSnapshot {
        cases: report.cases,
        required_count: report.required_count,
        recovered_count: report.recovered_count,
        returned_count: report.returned_count,
        allowed_returned_count: report.allowed_returned_count,
        recall: report.recall,
        precision: report.precision,
    }
}

fn metric_comparison(baseline: AggregateReport, active: AggregateReport) -> MetricComparison {
    MetricComparison {
        recall_delta: metric_delta(baseline.recall, active.recall),
        precision_delta: metric_delta(baseline.precision, active.precision),
        baseline: metric_snapshot(&baseline),
        active: metric_snapshot(&active),
    }
}

fn aggregate_case_ids(report: &EvaluationReport, case_ids: &HashSet<&str>) -> AggregateReport {
    aggregate(
        &report
            .cases
            .iter()
            .filter(|case| case_ids.contains(case.id.as_str()))
            .cloned()
            .collect::<Vec<_>>(),
    )
}

fn compare_reports(
    fixture: &Fixture,
    baseline: &EvaluationReport,
    active: &EvaluationReport,
) -> PolicyComparison {
    let cohort = |name: &str| {
        fixture.cohorts[name]
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>()
    };
    let lexical = cohort("lexical");
    let identifier = cohort("identifier");
    let eligible_supported = lexical
        .iter()
        .copied()
        .filter(|case_id| {
            fixture
                .cases
                .iter()
                .find(|case| case.id == *case_id)
                .is_some_and(|case| {
                    case.required_ids.iter().all(|required_id| {
                        case.memories
                            .iter()
                            .find(|memory| memory.id == *required_id)
                            .is_some_and(|memory| {
                                super::MemoryType::from_db(&memory.memory_type).is_ok_and(
                                    |memory_type| {
                                        memory_type.eligible_for_automatic_recall(&memory.source)
                                    },
                                )
                            })
                    })
                })
        })
        .collect::<HashSet<_>>();
    let development = active
        .cases
        .iter()
        .filter(|case| case.split == "development")
        .map(|case| case.id.as_str())
        .collect::<HashSet<_>>();
    let validation = active
        .cases
        .iter()
        .filter(|case| case.split == "validation")
        .map(|case| case.id.as_str())
        .collect::<HashSet<_>>();
    let active_missed_ids = active
        .cases
        .iter()
        .flat_map(|case| case.known_misses.iter().cloned())
        .collect::<Vec<_>>();
    let active_raw_report_missed_ids = active
        .cases
        .iter()
        .filter(|case| case.category == "reports")
        .flat_map(|case| case.known_misses.iter().cloned())
        .collect::<Vec<_>>();
    PolicyComparison {
        baseline_fixture_version: baseline.fixture_version.clone(),
        baseline_policy_version: baseline.policy_version.clone(),
        active_policy_version: active.policy_version.clone(),
        baseline_recall: baseline.current_runtime.recall,
        active_recall: active.current_runtime.recall,
        recall_delta: metric_delta(
            baseline.current_runtime.recall,
            active.current_runtime.recall,
        ),
        baseline_precision: baseline.current_runtime.precision,
        active_precision: active.current_runtime.precision,
        precision_delta: metric_delta(
            baseline.current_runtime.precision,
            active.current_runtime.precision,
        ),
        development: metric_comparison(
            aggregate_case_ids(baseline, &development),
            aggregate_case_ids(active, &development),
        ),
        validation: metric_comparison(
            aggregate_case_ids(baseline, &validation),
            aggregate_case_ids(active, &validation),
        ),
        supported_lexical: metric_comparison(
            aggregate_case_ids(baseline, &lexical),
            aggregate_case_ids(active, &lexical),
        ),
        exact_identifier: metric_comparison(
            aggregate_case_ids(baseline, &identifier),
            aggregate_case_ids(active, &identifier),
        ),
        eligible_supported: metric_comparison(
            aggregate_case_ids(baseline, &eligible_supported),
            aggregate_case_ids(active, &eligible_supported),
        ),
        active_missed_ids,
        active_raw_report_missed_ids,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct NoMemoryReport {
    cases: usize,
    nonempty_results: usize,
    automatic_pins_and_explicit_selection_removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RecallOffReport {
    cases: usize,
    pinned_cases_with_results: usize,
    explicit_cases_with_results: usize,
    automatic_retrieval_skipped: bool,
}

fn aggregate(reports: &[CaseReport]) -> AggregateReport {
    let required_count = reports.iter().map(|r| r.required_count).sum::<usize>();
    let recovered_count = reports.iter().map(|r| r.recovered_count).sum::<usize>();
    let returned_count = reports.iter().map(|r| r.returned_count).sum::<usize>();
    let allowed_returned_count = reports
        .iter()
        .map(|r| r.allowed_returned_count)
        .sum::<usize>();
    let expected_empty_cases = reports.iter().filter(|r| r.expected_empty).count();
    let actual_empty_cases = reports.iter().filter(|r| r.actual_empty).count();
    AggregateReport {
        cases: reports.len(),
        required_count,
        recovered_count,
        returned_count,
        allowed_returned_count,
        recall: (required_count > 0).then(|| recovered_count as f64 / required_count as f64),
        precision: (returned_count > 0)
            .then(|| allowed_returned_count as f64 / returned_count as f64),
        expected_empty_cases,
        actual_empty_cases,
        unexpected_nonempty_cases: reports
            .iter()
            .filter(|r| r.expected_empty && !r.actual_empty)
            .count(),
        known_misses: reports.iter().map(|r| r.known_misses.len()).sum(),
        irrelevant_returned: reports.iter().map(|r| r.irrelevant_returned.len()).sum(),
        quality_forbidden_returned: reports
            .iter()
            .map(|r| r.quality_forbidden_returned.len())
            .sum(),
        scope_lifecycle_violations: reports
            .iter()
            .map(|r| r.scope_lifecycle_violations.len())
            .sum(),
        exclusion_violations: reports.iter().map(|r| r.exclusion_violations.len()).sum(),
        budget_violations: reports.iter().map(|r| r.budget_violations.len()).sum(),
        retrieval_errors: reports.iter().map(|r| r.retrieval_errors.len()).sum(),
        unsupported_memory_uses: reports
            .iter()
            .map(|r| r.unsupported_memory_uses.len())
            .sum(),
        rendered_bytes: reports.iter().map(|r| r.rendered_bytes).sum(),
        pinned_lifecycle_violations: reports
            .iter()
            .map(|r| r.pinned_lifecycle_violations.len())
            .sum(),
        explicit_lifecycle_violations: reports
            .iter()
            .map(|r| r.explicit_lifecycle_violations.len())
            .sum(),
        pinned_forbidden_lifecycle_violations: reports
            .iter()
            .map(|r| r.pinned_forbidden_lifecycle_violations.len())
            .sum(),
        explicit_forbidden_lifecycle_violations: reports
            .iter()
            .map(|r| r.explicit_forbidden_lifecycle_violations.len())
            .sum(),
        pinned_unexpected_renderer_ids: reports
            .iter()
            .map(|r| r.pinned_unexpected_renderer_ids.len())
            .sum(),
        explicit_unexpected_renderer_ids: reports
            .iter()
            .map(|r| r.explicit_unexpected_renderer_ids.len())
            .sum(),
        long_query_cases: reports
            .iter()
            .filter(|r| r.query_bytes > r.query_bounded_to_bytes)
            .count(),
        long_prompt_cases: reports
            .iter()
            .filter(|r| r.prompt_input_bytes > 8_000)
            .count(),
    }
}

fn load_fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/memory-quality/v1.json"
    ))
    .expect("memory-quality fixture must be valid JSON")
}

fn parse_clock(fixture: &Fixture) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&fixture.fixed_clock)
        .expect("fixture fixed_clock must be RFC3339")
        .with_timezone(&Utc)
}

fn insert_workflow(db: &Db, id: &str, working_directory: &str) -> Result<(), DbError> {
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO workflows
             (id, name, description, working_directory, graph_json, created_at, updated_at)
             VALUES (?1, ?2, '', ?3, '{\"nodes\":[],\"edges\":[]}', ?4, ?4)",
            params![id, id, working_directory, "2026-08-01T00:00:00Z"],
        )?;
        Ok(())
    })
}

fn seed_case(db: &Db, case: &FixtureCase) -> Result<(), DbError> {
    insert_workflow(db, &case.workflow_id, &case.working_directory)?;
    let mut owners = HashSet::new();
    for memory in &case.memories {
        if let Some(owner) = memory.owner_workflow_id.as_deref() {
            owners.insert(owner);
        }
    }
    for owner in owners {
        if owner != case.workflow_id {
            insert_workflow(db, owner, &format!("/fixture/owners/{owner}"))?;
        }
    }
    db.with_conn(|conn| {
        for memory in &case.memories {
            let owner = memory
                .owner_workflow_id
                .as_deref()
                .unwrap_or(&case.workflow_id);
            let scope_key = match memory.scope_type.as_str() {
                "workflow" => owner,
                "workspace" => &case.working_directory,
                "user" => "local-user",
                other => return Err(DbError::Other(format!("invalid fixture scope {other}"))),
            };
            conn.execute(
                "INSERT INTO memories
                 (id, workflow_id, run_id, node_id, scope_type, scope_key, kind,
                  memory_type, source, title, body, pinned, confidence, salience,
                  status, last_confirmed_at, expires_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'text', ?7, ?8, ?9, ?10, ?11,
                         ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                params![
                    memory.id,
                    owner,
                    memory.run_id,
                    memory.node_id,
                    memory.scope_type,
                    scope_key,
                    memory.memory_type,
                    memory.source,
                    memory.title,
                    memory.body,
                    if memory.pinned { 1 } else { 0 },
                    memory.confidence,
                    memory.salience,
                    memory.status,
                    memory.last_confirmed_at,
                    memory.expires_at,
                    memory.created_at,
                    memory.updated_at,
                ],
            )?;
            index_memory(conn, &memory.id)?;
            if let Some(consumer) = memory.linked_to.as_deref() {
                if consumer != case.workflow_id {
                    return Err(DbError::Other(format!(
                        "fixture link {consumer} does not match case {}",
                        case.id
                    )));
                }
                conn.execute(
                    "INSERT INTO memory_links (id, workflow_id, memory_id, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        format!("link-{}", memory.id),
                        consumer,
                        memory.id,
                        "2026-08-10T00:00:00Z"
                    ],
                )?;
            }
        }
        Ok(())
    })
}

fn renderer_lifecycle_violations(
    actual: &[String],
    required: &[String],
    allowed: &[String],
    forbidden: &[String],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let actual = actual.iter().collect::<HashSet<_>>();
    let missing = required
        .iter()
        .filter(|id| !actual.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    let forbidden = actual
        .iter()
        .filter(|id| forbidden.contains(id))
        .map(|id| (*id).clone())
        .collect::<Vec<_>>();
    let unexpected = actual
        .iter()
        .filter(|id| !allowed.contains(id) && !forbidden.contains(id))
        .map(|id| (*id).clone())
        .collect::<Vec<_>>();
    (missing, forbidden, unexpected)
}

fn evaluate_case(fixture: &Fixture, case: &FixtureCase, now: DateTime<Utc>) -> CaseReport {
    let db = Db::open_in_memory().expect("open eval db");
    seed_case(&db, case).expect("seed eval case");
    let query_source = case
        .sources
        .iter()
        .find(|source| source.kind == "query")
        .expect("fixture case query source");
    let request = MemoryRetrievalRequest {
        workflow_id: &case.workflow_id,
        working_directory: Some(&case.working_directory),
        run_id: &case.run_id,
        node_id: &case.node_id,
        query_text: &query_source.text,
        exclude_ids: &case.exclude_ids,
        purpose: RetrievalPurpose::Automatic,
    };
    let result = db.retrieve_memories_at(&request, now);
    let retrieved_ids = result
        .items
        .iter()
        .map(|item| item.memory_id.clone())
        .collect::<Vec<_>>();
    let required = case.required_ids.iter().collect::<HashSet<_>>();
    let allowed = case.allowed_ids.iter().collect::<HashSet<_>>();
    let forbidden = case.forbidden_ids.iter().collect::<HashSet<_>>();
    let returned = retrieved_ids.iter().collect::<HashSet<_>>();
    let known_misses = case
        .required_ids
        .iter()
        .filter(|id| !returned.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    let irrelevant_returned = retrieved_ids
        .iter()
        .filter(|id| !allowed.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    let quality_forbidden_returned = retrieved_ids
        .iter()
        .filter(|id| forbidden.contains(id))
        .cloned()
        .collect::<Vec<_>>();

    let mut retrieval_errors = Vec::new();
    let mut scope_lifecycle_violations = Vec::new();
    let mut exclusion_violations = Vec::new();
    let mut budget_violations = Vec::new();
    let unsupported_memory_uses = retrieved_ids
        .iter()
        .filter(|id| {
            db.get_memory(id).ok().flatten().is_some_and(|memory| {
                !memory
                    .memory_type
                    .eligible_for_automatic_recall(&memory.source)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    if result.error_code.is_some() {
        retrieval_errors.push("retrieval_error".into());
    }
    if result.rendered_bytes > RETRIEVAL_MAX_BYTES {
        budget_violations.push("retrieval_byte_budget".into());
    }
    if result
        .items
        .iter()
        .any(|item| item.rendered_bytes > RETRIEVAL_ITEM_MAX_BYTES)
    {
        budget_violations.push("retrieval_item_byte_budget".into());
    }
    if retrieved_ids.iter().any(|id| case.exclude_ids.contains(id)) {
        exclusion_violations.push("explicit_exclusion".into());
    }
    let visible = db
        .list_memories_for_context(
            &MemoryContext {
                workflow_id: case.workflow_id.clone(),
                working_directory: Some(case.working_directory.clone()),
            },
            false,
        )
        .expect("list eval context")
        .into_iter()
        .filter(|item| {
            item.memory.run_id.as_deref() != Some(case.run_id.as_str())
                && !is_expired_at(&item.memory, now)
        })
        .map(|item| item.memory.id)
        .collect::<HashSet<_>>();
    if retrieved_ids.iter().any(|id| !visible.contains(id)) {
        scope_lifecycle_violations.push("scope_or_lifecycle".into());
    }

    let pinned_context = db
        .format_pinned_context_at(
            &MemoryContext {
                workflow_id: case.workflow_id.clone(),
                working_directory: Some(case.working_directory.clone()),
            },
            now,
        )
        .expect("format pinned eval context");
    let pinned_bytes = (!case.pinned_ids.is_empty()).then_some(pinned_context.bytes);
    let pinned_included_ids = pinned_context.included_ids;
    let explicit_markdown = db
        .format_memories_context_at(&case.explicit_selection_ids, now)
        .expect("format explicit eval context");
    let explicit_bytes =
        (!case.explicit_selection_ids.is_empty()).then_some(explicit_markdown.len());
    let explicit_included_ids = case
        .explicit_selection_ids
        .iter()
        .filter(|id| {
            db.get_memory(id).ok().flatten().is_some_and(|memory| {
                explicit_markdown.contains(&format!("### Memory — {}", memory.title))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let (
        pinned_lifecycle_violations,
        pinned_forbidden_lifecycle_violations,
        pinned_unexpected_renderer_ids,
    ) = renderer_lifecycle_violations(
        &pinned_included_ids,
        &case.renderer_required_pinned_ids,
        &case.renderer_allowed_pinned_ids,
        &case.renderer_forbidden_pinned_ids,
    );
    let (
        explicit_lifecycle_violations,
        explicit_forbidden_lifecycle_violations,
        explicit_unexpected_renderer_ids,
    ) = renderer_lifecycle_violations(
        &explicit_included_ids,
        &case.renderer_required_explicit_ids,
        &case.renderer_allowed_explicit_ids,
        &case.renderer_forbidden_explicit_ids,
    );
    if pinned_bytes.is_some_and(|bytes| bytes > 6_000) {
        budget_violations.push("pinned_byte_budget".into());
    }

    let _ = fixture; // keeps the fixture contract visible at this boundary.
    let _ = &case.context_template_inputs;
    CaseReport {
        id: case.id.clone(),
        category: case.category.clone(),
        split: case.split.clone(),
        layer: case.layer.clone(),
        evaluation_layers: case.evaluation_layers.clone(),
        required_count: required.len(),
        recovered_count: required.intersection(&returned).count(),
        returned_count: retrieved_ids.len(),
        allowed_returned_count: retrieved_ids
            .iter()
            .filter(|id| allowed.contains(id))
            .count(),
        recall: (!required.is_empty())
            .then(|| required.intersection(&returned).count() as f64 / required.len() as f64),
        precision: (!retrieved_ids.is_empty()).then(|| {
            retrieved_ids
                .iter()
                .filter(|id| allowed.contains(id))
                .count() as f64
                / retrieved_ids.len() as f64
        }),
        expected_empty: case.expected_empty,
        actual_empty: retrieved_ids.is_empty(),
        retrieved_ids,
        known_misses,
        irrelevant_returned,
        quality_forbidden_returned,
        retrieval_errors,
        scope_lifecycle_violations,
        exclusion_violations,
        budget_violations,
        unsupported_memory_uses,
        rendered_bytes: result.rendered_bytes,
        pinned_bytes,
        explicit_bytes,
        final_composition_evaluated: false,
        query_bytes: query_source.text.len(),
        prompt_input_bytes: case.prompt_input.len(),
        query_bounded_to_bytes: 8_000,
        pinned_candidate_ids: case.pinned_ids.clone(),
        pinned_included_ids,
        explicit_selected_ids: case.explicit_selection_ids.clone(),
        explicit_included_ids,
        pinned_lifecycle_violations,
        explicit_lifecycle_violations,
        pinned_forbidden_lifecycle_violations,
        explicit_forbidden_lifecycle_violations,
        pinned_unexpected_renderer_ids,
        explicit_unexpected_renderer_ids,
    }
}

fn evaluate(fixture: &Fixture) -> EvaluationReport {
    let now = parse_clock(fixture);
    let reports = fixture
        .cases
        .iter()
        .map(|case| evaluate_case(fixture, case, now))
        .collect::<Vec<_>>();
    let mut by_category = BTreeMap::<String, Vec<CaseReport>>::new();
    for report in &reports {
        by_category
            .entry(report.category.clone())
            .or_default()
            .push(report.clone());
    }
    let categories = by_category
        .into_iter()
        .map(|(category, reports)| (category, aggregate(&reports)))
        .collect::<BTreeMap<_, _>>();
    let development = reports
        .iter()
        .filter(|report| report.split == "development")
        .cloned()
        .collect::<Vec<_>>();
    let validation = reports
        .iter()
        .filter(|report| report.split == "validation")
        .cloned()
        .collect::<Vec<_>>();
    let empty_baseline = fixture
        .cases
        .iter()
        .filter(|case| {
            let db = Db::open_in_memory().expect("open empty eval db");
            insert_workflow(&db, &case.workflow_id, &case.working_directory)
                .expect("seed empty eval workflow context");
            let query_source = case
                .sources
                .iter()
                .find(|source| source.kind == "query")
                .expect("fixture case query source");
            let request = MemoryRetrievalRequest {
                workflow_id: &case.workflow_id,
                working_directory: Some(&case.working_directory),
                run_id: &case.run_id,
                node_id: &case.node_id,
                query_text: &query_source.text,
                exclude_ids: &case.exclude_ids,
                purpose: RetrievalPurpose::Automatic,
            };
            let result = db.retrieve_memories_at(&request, now);
            let pinned = db
                .format_pinned_context_at(
                    &MemoryContext {
                        workflow_id: case.workflow_id.clone(),
                        working_directory: Some(case.working_directory.clone()),
                    },
                    now,
                )
                .expect("format empty pinned context");
            let explicit = db
                .format_memories_context_at(&[], now)
                .expect("format empty explicit context");
            assert!(pinned.included_ids.is_empty());
            assert!(explicit.is_empty());
            !result.items.is_empty()
        })
        .count();
    let recall_off_baseline =
        fixture
            .cases
            .iter()
            .fold((0usize, 0usize), |(pinned_count, explicit_count), case| {
                let db = Db::open_in_memory().expect("open recall-off eval db");
                seed_case(&db, case).expect("seed recall-off eval case");
                let context = MemoryContext {
                    workflow_id: case.workflow_id.clone(),
                    working_directory: Some(case.working_directory.clone()),
                };
                let pinned = db
                    .format_pinned_context_at(&context, now)
                    .expect("format recall-off pinned context");
                let explicit = db
                    .format_memories_context_at(&case.explicit_selection_ids, now)
                    .expect("format recall-off explicit context");
                (
                    pinned_count + usize::from(!pinned.included_ids.is_empty()),
                    explicit_count + usize::from(!explicit.is_empty()),
                )
            });
    EvaluationReport {
        fixture_version: fixture.version.clone(),
        evaluator_version: fixture.evaluator_version.clone(),
        policy_version: super::memory_retrieval::MEMORY_RETRIEVAL_POLICY_VERSION.into(),
        fixed_clock: fixture.fixed_clock.clone(),
        fixture_digest: reviewed_fixture_digest(),
        development: aggregate(&development),
        validation: aggregate(&validation),
        current_runtime: aggregate(&reports),
        no_memory_baseline: NoMemoryReport {
            cases: fixture.cases.len(),
            nonempty_results: empty_baseline,
            automatic_pins_and_explicit_selection_removed: true,
        },
        recall_off_baseline: RecallOffReport {
            cases: fixture.cases.len(),
            pinned_cases_with_results: recall_off_baseline.0,
            explicit_cases_with_results: recall_off_baseline.1,
            automatic_retrieval_skipped: true,
        },
        categories,
        cases: reports,
        limitations: vec![
            "This report retains the original retrieval and utility-renderer layers. Plan 048 final prompt composition and cross-layer budgets are evaluated separately in composition-v1.".into(),
            "The complete explicit-selection utility renderer is intentionally uncapped here. Agent dispatch uses the separately evaluated bounded structured composer.".into(),
            "Precision and recall measure memory-id retrieval labels, not a model's answer or extraction accuracy.".into(),
            "Wall-clock latency is diagnostic only and is omitted from the frozen report.".into(),
        ],
        input_source_types: fixture.source_types.clone(),
        frozen_cohorts: fixture.cohorts.clone(),
    }
}

fn assert_fixture_contract(fixture: &Fixture) {
    assert_eq!(fixture.version, FIXTURE_VERSION);
    assert_eq!(fixture.evaluator_version, EVALUATOR_VERSION);
    assert_eq!(fixture.policy_version, POLICY_VERSION);
    assert_eq!(fixture.source_types, ["task", "event", "query"]);
    for ids in fixture.cohorts.values() {
        assert!(!ids.is_empty(), "frozen cohort cannot be empty");
        for id in ids {
            assert!(fixture.cases.iter().any(|case| &case.id == id));
        }
    }
    assert!(fixture.cohorts.contains_key("lexical"));
    assert!(fixture.cohorts.contains_key("identifier"));
    assert!(fixture.cohorts.contains_key("paraphrase"));
    assert!(fixture.cohorts.contains_key("stale_distractor"));
    assert!(fixture.cohorts.contains_key("renderer"));
    assert!(fixture.cohorts.contains_key("empty"));
    for cohort in [
        "lexical",
        "identifier",
        "paraphrase",
        "stale_distractor",
        "renderer",
        "empty",
    ] {
        let ids = &fixture.cohorts[cohort];
        assert!(
            ids.iter().any(|id| fixture
                .cases
                .iter()
                .any(|case| &case.id == id && case.split == "development")),
            "{cohort} cohort needs development cases"
        );
        assert!(
            ids.iter().any(|id| fixture
                .cases
                .iter()
                .any(|case| &case.id == id && case.split == "validation")),
            "{cohort} cohort needs validation cases"
        );
    }
    assert_eq!(fixture.cases.len(), 50, "v1 must contain exactly 50 cases");
    assert!(fixture
        .cases
        .iter()
        .filter(|case| case.category == "stale_distractors")
        .all(|case| case.memories.len() >= 12));
    for case in fixture
        .cases
        .iter()
        .filter(|case| case.category == "stale_distractors")
    {
        let required_id = case
            .required_ids
            .first()
            .expect("stale case must have a required memory");
        let required_updated = case
            .memories
            .iter()
            .find(|memory| &memory.id == required_id)
            .and_then(|memory| DateTime::parse_from_rfc3339(&memory.updated_at).ok())
            .expect("stale required memory must have a timestamp");
        let recent_distractors = case
            .memories
            .iter()
            .filter(|memory| memory.id.as_str() != required_id.as_str())
            .filter(|memory| {
                DateTime::parse_from_rfc3339(&memory.updated_at)
                    .is_ok_and(|updated| updated > required_updated)
            })
            .count();
        assert!(
            recent_distractors >= 10,
            "{} must place the required fact behind ten recent distractors",
            case.id
        );
    }
    assert!(fixture
        .cases
        .iter()
        .any(|case| case.query_text.len() > 8_000));
    assert!(fixture
        .cases
        .iter()
        .any(|case| case.prompt_input.len() > 8_000));
    assert_eq!(
        fixture.cohorts["identifier"],
        ["c16", "c17", "c18", "c19", "c20"]
    );
    assert!(fixture.cohorts["stale_distractor"].iter().all(|id| fixture
        .cases
        .iter()
        .find(|case| &case.id == id)
        .is_some_and(|case| case.memories.len() >= 12)));
    let case_ids = fixture
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        fixture
            .typed_source_cases
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        case_ids,
        "every case must declare task/event/query source inputs"
    );
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|case| case.split == "development")
            .count(),
        30
    );
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|case| case.split == "validation")
            .count(),
        20
    );
    assert!(
        fixture
            .cases
            .iter()
            .filter(|case| case.expected_empty)
            .count()
            >= 10
    );
    let mut ids = HashSet::new();
    let mut memory_ids = HashSet::new();
    let categories = fixture
        .cases
        .iter()
        .map(|case| case.category.as_str())
        .collect::<HashSet<_>>();
    for case in &fixture.cases {
        assert!(ids.insert(&case.id), "duplicate case id {}", case.id);
        assert_eq!(case.layer, "automatic_retrieval");
        assert!(case
            .evaluation_layers
            .contains(&"automatic_retrieval".into()));
        if !case.pinned_ids.is_empty() {
            assert!(case.evaluation_layers.contains(&"renderer_pinned".into()));
        }
        if !case.explicit_selection_ids.is_empty() {
            assert!(case.evaluation_layers.contains(&"renderer_explicit".into()));
        }
        assert!(!case.task_question.is_empty());
        assert!(!case.prompt_input.is_empty());
        assert!(!case.expected_answer_facts.is_empty());
        let required = case.required_ids.iter().collect::<HashSet<_>>();
        let allowed = case.allowed_ids.iter().collect::<HashSet<_>>();
        assert!(
            required.is_subset(&allowed),
            "required ids must be allowed in {}",
            case.id
        );
        assert_eq!(
            case.expected_empty,
            case.required_ids.is_empty(),
            "empty labels must have no required memories in {}",
            case.id
        );
        assert_eq!(case.final_composition.status, "deferred");
        assert_eq!(case.final_composition.plan, "048");
        assert!(!case.final_composition.reason.is_empty());
        assert_eq!(
            case.sources.len(),
            3,
            "{} must have task/event/query inputs",
            case.id
        );
        let source_kinds = case
            .sources
            .iter()
            .map(|source| source.kind.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            source_kinds,
            ["event", "query", "task"].into_iter().collect()
        );
        for source in &case.sources {
            assert!(
                !source.text.is_empty(),
                "empty {} source in {}",
                source.kind,
                case.id
            );
        }
        assert_eq!(
            case.sources
                .iter()
                .find(|source| source.kind == "task")
                .unwrap()
                .text,
            case.task_question
        );
        assert_eq!(
            case.sources
                .iter()
                .find(|source| source.kind == "query")
                .unwrap()
                .text,
            case.query_text
        );
        let case_memory_ids = case
            .memories
            .iter()
            .map(|memory| &memory.id)
            .collect::<HashSet<_>>();
        for memory in &case.memories {
            assert!(
                memory_ids.insert(&memory.id),
                "duplicate memory id {}",
                memory.id
            );
            assert!(!memory.title.is_empty());
            assert!(!memory.body.is_empty());
            assert!(matches!(
                memory.scope_type.as_str(),
                "user" | "workspace" | "workflow"
            ));
            assert!(matches!(
                memory.status.as_str(),
                "active" | "retracted" | "superseded"
            ));
            assert!(matches!(
                memory.memory_type.as_str(),
                "fact" | "preference" | "lesson" | "note" | "episode" | "output"
            ));
            assert!(!memory.source.is_empty());
            DateTime::parse_from_rfc3339(&memory.created_at)
                .unwrap_or_else(|_| panic!("invalid created_at in {}", memory.id));
            DateTime::parse_from_rfc3339(&memory.updated_at)
                .unwrap_or_else(|_| panic!("invalid updated_at in {}", memory.id));
            for timestamp in [&memory.expires_at, &memory.last_confirmed_at]
                .into_iter()
                .flatten()
            {
                DateTime::parse_from_rfc3339(timestamp)
                    .unwrap_or_else(|_| panic!("invalid timestamp in {}", memory.id));
            }
        }
        for id in case
            .required_ids
            .iter()
            .chain(case.allowed_ids.iter())
            .chain(case.forbidden_ids.iter())
            .chain(case.exclude_ids.iter())
            .chain(case.pinned_ids.iter())
            .chain(case.explicit_selection_ids.iter())
            .chain(case.renderer_required_pinned_ids.iter())
            .chain(case.renderer_required_explicit_ids.iter())
            .chain(case.renderer_allowed_pinned_ids.iter())
            .chain(case.renderer_allowed_explicit_ids.iter())
            .chain(case.renderer_forbidden_pinned_ids.iter())
            .chain(case.renderer_forbidden_explicit_ids.iter())
        {
            assert!(
                case_memory_ids.contains(id),
                "{} references unknown memory {id}",
                case.id
            );
        }
        let mut titles = HashSet::new();
        for memory in &case.memories {
            assert!(
                titles.insert(&memory.title),
                "memory titles must be unique within {} for explicit id extraction",
                case.id
            );
        }
        assert!(
            case.renderer_required_pinned_ids
                .iter()
                .all(|id| case.pinned_ids.contains(id)),
            "pinned renderer expectations must be selected in {}",
            case.id
        );
        assert!(
            case.renderer_required_explicit_ids
                .iter()
                .all(|id| case.explicit_selection_ids.contains(id)),
            "explicit renderer expectations must be selected in {}",
            case.id
        );
        let overlap = case
            .allowed_ids
            .iter()
            .collect::<HashSet<_>>()
            .intersection(&case.forbidden_ids.iter().collect())
            .count();
        assert_eq!(overlap, 0, "allowed and forbidden overlap in {}", case.id);
    }
    for category in [
        "natural_wording",
        "stale_distractors",
        "exact_identifier",
        "multilingual",
        "scope_visibility",
        "lifecycle",
        "explicit_exclusion",
        "unrelated",
        "reports",
        "duplicates_conflicts",
        "no_memory",
    ] {
        assert!(categories.contains(category), "missing category {category}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_contract() {
        assert_fixture_contract(&load_fixture());
    }

    #[test]
    fn evaluation_report_is_deterministic_and_invariants_hold() {
        let fixture = load_fixture();
        assert_fixture_contract(&fixture);
        let first = evaluate(&fixture);
        let second = evaluate(&fixture);
        assert_eq!(first, second);
        assert_eq!(first.no_memory_baseline.nonempty_results, 0);
        assert!(first.recall_off_baseline.automatic_retrieval_skipped);
        assert!(first.recall_off_baseline.pinned_cases_with_results > 0);
        assert!(first.recall_off_baseline.explicit_cases_with_results > 0);
        assert_eq!(first.current_runtime.scope_lifecycle_violations, 0);
        assert_eq!(first.current_runtime.exclusion_violations, 0);
        assert_eq!(first.current_runtime.budget_violations, 0);
        assert_eq!(first.current_runtime.retrieval_errors, 0);
        assert_eq!(first.current_runtime.unsupported_memory_uses, 0);
        assert_eq!(first.current_runtime.pinned_lifecycle_violations, 0);
        assert_eq!(first.current_runtime.explicit_lifecycle_violations, 0);
        assert_eq!(
            first.current_runtime.pinned_forbidden_lifecycle_violations,
            0
        );
        assert_eq!(
            first
                .current_runtime
                .explicit_forbidden_lifecycle_violations,
            0
        );
        assert_eq!(first.current_runtime.pinned_unexpected_renderer_ids, 0);
        assert_eq!(first.current_runtime.explicit_unexpected_renderer_ids, 0);
        assert!(first.current_runtime.long_query_cases > 0);
        assert!(first.current_runtime.long_prompt_cases > 0);
        println!("{}", serde_json::to_string_pretty(&first).unwrap());
    }

    #[test]
    fn baseline_snapshot() {
        let fixture = load_fixture();
        assert_fixture_contract(&fixture);
        let report = evaluate(&fixture);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(BASELINE_PATH);
        if std::env::var("ALFRED_MEMORY_EVAL_WRITE").as_deref() == Ok("baseline-v1") {
            assert_eq!(fixture.version, "v1");
            assert!(
                !path.exists(),
                "refusing to overwrite frozen {BASELINE_PATH}"
            );
            fs::create_dir_all(path.parent().expect("baseline parent"))
                .expect("create baseline fixture directory");
            fs::write(&path, serde_json::to_string_pretty(&report).unwrap() + "\n")
                .expect("write baseline fixture");
            return;
        }
        let frozen = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read frozen {BASELINE_PATH}: {error}"));
        let frozen: EvaluationReport =
            serde_json::from_str(&frozen).expect("frozen baseline must match evaluator schema");
        assert!(
            frozen.current_runtime.irrelevant_returned > 0,
            "the frozen v1 report must keep known relevance misses visible"
        );
        if report.policy_version == frozen.policy_version {
            assert_eq!(
                report, frozen,
                "v1 baseline drifted; use a new fixture version"
            );
        } else {
            // Later policy plans may change runtime results. Keep the v1
            // artifact immutable while comparing the active policy against
            // its versioned metrics and the frozen authored labels.
            assert_eq!(report.fixture_version, frozen.fixture_version);
            assert_eq!(report.fixed_clock, frozen.fixed_clock);
            assert_eq!(report.fixture_digest, frozen.fixture_digest);
            assert_eq!(report.frozen_cohorts, frozen.frozen_cohorts);
            assert_eq!(
                report
                    .cases
                    .iter()
                    .map(|case| (&case.id, &case.category, &case.split))
                    .collect::<Vec<_>>(),
                frozen
                    .cases
                    .iter()
                    .map(|case| (&case.id, &case.category, &case.split))
                    .collect::<Vec<_>>()
            );
            let comparison = compare_reports(&fixture, &frozen, &report);
            for metrics in [
                &comparison.supported_lexical,
                &comparison.exact_identifier,
                &comparison.eligible_supported,
            ] {
                assert!(
                    metrics.active.recall >= metrics.baseline.recall,
                    "supported-cohort recall regressed"
                );
                assert!(
                    metrics.active.precision >= metrics.baseline.precision,
                    "supported-cohort precision regressed"
                );
            }
            assert_eq!(report.current_runtime.unexpected_nonempty_cases, 0);
            assert_eq!(report.current_runtime.scope_lifecycle_violations, 0);
            assert_eq!(report.current_runtime.exclusion_violations, 0);
            assert_eq!(report.current_runtime.budget_violations, 0);
            assert_eq!(report.current_runtime.retrieval_errors, 0);
            assert_eq!(report.current_runtime.unsupported_memory_uses, 0);
            assert!(report
                .cases
                .iter()
                .filter(|case| case.expected_empty)
                .all(|case| case.actual_empty));

            let v2_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join(V2_BASELINE_PATH);
            if std::env::var("ALFRED_MEMORY_EVAL_WRITE").as_deref() == Ok("baseline-v2") {
                assert!(
                    !v2_path.exists(),
                    "refusing to overwrite frozen {V2_BASELINE_PATH}"
                );
                fs::write(
                    &v2_path,
                    serde_json::to_string_pretty(&comparison).unwrap() + "\n",
                )
                .expect("write v2 comparison");
            } else {
                let frozen_v2: PolicyComparison = serde_json::from_str(
                    &fs::read_to_string(&v2_path)
                        .unwrap_or_else(|error| panic!("read {V2_BASELINE_PATH}: {error}")),
                )
                .expect("v2 comparison must parse");
                assert_eq!(comparison, frozen_v2, "v2 retrieval output drifted");
            }
            println!("{}", serde_json::to_string_pretty(&comparison).unwrap());
        }
    }

    #[test]
    fn validator_boundary_does_not_claim_model_accuracy() {
        use crate::db::{candidate_existing_memory_context, parse_reviewer_output};
        let db = Db::open_in_memory().unwrap();
        let raw = r#"{"candidates":[]}"#;
        let parsed = parse_reviewer_output(raw).unwrap();
        assert!(parsed.is_empty());
        let request = MemoryRetrievalRequest {
            workflow_id: "missing-workflow",
            working_directory: Some("/fixture/workspace"),
            run_id: "run",
            node_id: "node",
            query_text: "anything",
            exclude_ids: &[],
            purpose: RetrievalPurpose::Review,
        };
        assert!(candidate_existing_memory_context(&db, &request)
            .items
            .is_empty());
    }

    #[test]
    fn policy_comparison_keeps_v1_labels_when_active_policy_changes() {
        let frozen_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(BASELINE_PATH);
        let frozen: EvaluationReport = serde_json::from_str(
            &fs::read_to_string(frozen_path).expect("frozen baseline must be readable"),
        )
        .expect("frozen baseline must parse");
        let mut active = frozen.clone();
        active.policy_version = "lexical-plus-recent-v2".into();
        active.current_runtime.recall = Some(1.0);
        active.current_runtime.precision = Some(1.0);
        active.current_runtime.known_misses = 0;
        let fixture = load_fixture();
        let comparison = compare_reports(&fixture, &frozen, &active);
        assert_eq!(comparison.baseline_fixture_version, "v1");
        assert_eq!(comparison.baseline_policy_version, POLICY_VERSION);
        assert_eq!(comparison.active_policy_version, "lexical-plus-recent-v2");
        assert!(
            (comparison.recall_delta.unwrap() - (1.0 - frozen.current_runtime.recall.unwrap()))
                .abs()
                < f64::EPSILON,
            "recall delta must derive from the frozen report"
        );
        assert_eq!(
            comparison.precision_delta,
            Some(1.0 - frozen.current_runtime.precision.unwrap())
        );
    }

    #[test]
    fn renderer_violation_detector_catches_an_expired_leak() {
        let (missing, forbidden, unexpected) = renderer_lifecycle_violations(
            &["m31-expired".into()],
            &[],
            &[],
            &["m31-expired".into()],
        );
        assert!(missing.is_empty());
        assert_eq!(forbidden, vec!["m31-expired"]);
        assert!(unexpected.is_empty());
    }
}
