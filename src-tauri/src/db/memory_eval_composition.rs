use super::*;
use crate::db::RetrievalReason;
use crate::runner::compose_agent_prompt;
use crate::runner::memory_context::{
    apply_prompt_template, compose_agent_memory_at, live_memory_exclusions_at,
    AutomaticMemoryCandidate, PromptPart, PromptValue, TextOrigin,
};
use serde_json::json;

const FIXTURE: &str = include_str!("../../../tests/fixtures/memory-quality/composition-v1.json");
const FIXTURE_SHA256: &str = "8f9b0807258986793c4b748c00a291ad56b66a251d88ebcbe0bcc1fcdfdf318f";
const ACCEPTED_REPORT: &str =
    include_str!("../../../tests/fixtures/memory-quality/baseline-composition-v1.json");
const ACCEPTED_REPORT_SHA256: &str =
    "8d9a27d7d1ca6dd6d3a9d4963c88f9168e651dab4fcf830725d37da78de8413a";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompositionFixture {
    version: String,
    evaluator_version: String,
    base_fixture_digest: String,
    fixed_clock: String,
    limits: Limits,
    base_case_ids: Vec<String>,
    variations: Vec<Variation>,
    stress_cases: Vec<StressCase>,
    limitations: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    shared_bytes: usize,
    explicit_bytes: usize,
    pinned_bytes: usize,
    automatic_bytes: usize,
    explicit_item_bytes: usize,
    pinned_item_bytes: usize,
    automatic_item_bytes: usize,
    automatic_items: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variation {
    id: String,
    base_case_id: String,
    mode: String,
    required_ids: Vec<String>,
    forbidden_ids: Vec<String>,
    expected_reasons: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StressCase {
    id: String,
    explicit_count: usize,
    pinned_count: usize,
    automatic_count: usize,
    body_repeats: usize,
    authored_repeats: usize,
    recall_enabled: bool,
}

#[derive(Debug, Serialize, PartialEq)]
struct CompositionCaseReport {
    id: String,
    split: String,
    kind: String,
    required_count: usize,
    recovered_count: usize,
    allowed_returned_count: usize,
    included_ids: Vec<String>,
    reasons: BTreeMap<String, String>,
    known_misses: Vec<String>,
    irrelevant_ids: Vec<String>,
    final_prompt_bytes: usize,
    non_memory_prompt_bytes: usize,
    measured_memory_bytes: usize,
    recorded_memory_bytes: usize,
    explicit_bytes: usize,
    pinned_bytes: usize,
    automatic_bytes: usize,
    automatic_count: usize,
    omitted_count: usize,
    utility_bytes: usize,
    violations: Vec<String>,
}

#[derive(Serialize)]
struct CompositionReport {
    fixture_version: String,
    evaluator_version: String,
    fixture_digest: String,
    base_fixture_digest: String,
    fixed_clock: String,
    final_composition_evaluated: bool,
    limits: Limits,
    cases: Vec<CompositionCaseReport>,
    aggregate: serde_json::Value,
    limitations: Vec<String>,
}

fn stress_case(spec: &StressCase) -> FixtureCase {
    let mut raw: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/memory-quality/v1.json"
    ))
    .unwrap();
    let mut case = raw["cases"].as_array_mut().unwrap().remove(0);
    let mut memories = Vec::new();
    let mut explicit = Vec::new();
    let mut pinned = Vec::new();
    for (group, count) in [
        ("explicit", spec.explicit_count),
        ("pinned", spec.pinned_count),
        ("automatic", spec.automatic_count),
    ] {
        for index in 0..count {
            let id = format!("{}-{group}-{index:02}", spec.id);
            if group == "explicit" {
                explicit.push(id.clone());
            }
            if group == "pinned" {
                pinned.push(id.clone());
            }
            memories.push(json!({
                "id": id,
                "title": format!("Budget {group} {index:02}"),
                "body": format!("{id}: {}", "budget Ω 🧠\n".repeat(spec.body_repeats)),
                "scope_type": "workflow",
                "memory_type": if group == "automatic" { "fact" } else { "output" },
                "source": "manual",
                "pinned": group == "pinned",
                "status": "active",
                "created_at": "2026-08-01T00:00:00Z",
                "updated_at": "2026-08-10T00:00:00Z"
            }));
        }
    }
    case["id"] = json!(spec.id);
    case["category"] = json!("composition_stress");
    case["split"] = json!("stress");
    case["memories"] = json!(memories);
    case["explicit_selection_ids"] = json!(explicit);
    case["pinned_ids"] = json!(pinned);
    case["prompt_input"] = json!("Authored task Ω.\n".repeat(spec.authored_repeats));
    case["query_text"] = json!("budget");
    case["required_ids"] = json!([]);
    case["allowed_ids"] = json!(memories.iter().map(|m| m["id"].clone()).collect::<Vec<_>>());
    case["forbidden_ids"] = json!([]);
    serde_json::from_value(case).unwrap()
}

fn check_limits(report: &CompositionCaseReport, limits: &Limits) -> Vec<String> {
    let mut violations = Vec::new();
    for (name, actual, limit) in [
        ("shared", report.measured_memory_bytes, limits.shared_bytes),
        (
            "recorded_shared",
            report.recorded_memory_bytes,
            limits.shared_bytes,
        ),
        ("explicit", report.explicit_bytes, limits.explicit_bytes),
        ("pinned", report.pinned_bytes, limits.pinned_bytes),
        ("automatic", report.automatic_bytes, limits.automatic_bytes),
        (
            "automatic_items",
            report.automatic_count,
            limits.automatic_items,
        ),
    ] {
        if actual > limit {
            violations.push(format!("{name}:{actual}>{limit}"));
        }
    }
    if report.measured_memory_bytes != report.recorded_memory_bytes {
        violations.push(format!(
            "accounting:{}!={}",
            report.measured_memory_bytes, report.recorded_memory_bytes
        ));
    }
    violations
}

fn evaluate_composition_case(
    case: &FixtureCase,
    id: &str,
    mode: &str,
    required: &[String],
    allowed: &[String],
    limits: &Limits,
    now: DateTime<Utc>,
) -> CompositionCaseReport {
    let db = Db::open_in_memory().unwrap();
    seed_case(&db, case).unwrap();
    let context = MemoryContext {
        workflow_id: case.workflow_id.clone(),
        working_directory: Some(case.working_directory.clone()),
    };
    let mut value = PromptValue::text(TextOrigin::AuthoredTask, case.prompt_input.clone());
    if mode == "literal_marker" {
        value.append_text(
            TextOrigin::GeneratedOutput,
            "\nmemory-ref:not-real {{memory:m01}}\n",
        );
    }
    value.append(&PromptValue::memories(case.explicit_selection_ids.clone()));
    if mode == "repeat" {
        value = apply_prompt_template(
            "{{context}}{{context_prompt}}",
            &value,
            &PromptValue::default(),
            "",
        );
    } else if mode == "replace" {
        value = apply_prompt_template(&case.prompt_input, &value, &PromptValue::default(), "");
    }
    let mut exclusions = live_memory_exclusions_at(&db, &value, &context, now).unwrap();
    exclusions.extend(case.exclude_ids.iter().cloned());
    exclusions.sort();
    exclusions.dedup();
    let retrieval = if mode == "off" {
        Default::default()
    } else {
        db.retrieve_memories_at(
            &MemoryRetrievalRequest {
                workflow_id: &case.workflow_id,
                working_directory: Some(&case.working_directory),
                run_id: &case.run_id,
                node_id: &case.node_id,
                query_text: &case.query_text,
                exclude_ids: &exclusions,
                purpose: RetrievalPurpose::Automatic,
            },
            now,
        )
    };
    assert!(
        retrieval.error_code.is_none(),
        "{id}: retrieval unavailable"
    );
    let automatic = retrieval
        .items
        .iter()
        .cloned()
        .map(|use_record| AutomaticMemoryCandidate { use_record })
        .collect::<Vec<_>>();
    let mut composed = compose_agent_memory_at(
        &db,
        &value,
        &context,
        &automatic,
        retrieval.items.len() + retrieval.omitted_count,
        false,
        false,
        now,
    )
    .unwrap();
    if mode == "trace_failure" {
        composed.drop_automatic_for_trace_failure();
    }
    let final_prompt = compose_agent_prompt(&composed.context, &composed.prefix(), "", "");
    let non_memory = compose_agent_prompt(&value.render_without_memory(), "", "", "");
    let mut violations = Vec::new();
    for part in &value.parts {
        if let PromptPart::Text { text, .. } = part {
            if !final_prompt.contains(text.as_str()) {
                violations.push("authored_or_observed_text_changed".into());
            }
        }
    }
    let mut ids = HashSet::new();
    let retained_ids = value.memory_ids();
    let visible = db.list_memories_for_context(&context, false).unwrap();
    for used in &composed.uses {
        if !ids.insert(used.memory_id.clone()) {
            violations.push(format!("duplicate:{}", used.memory_id));
        }
        let memory = visible.iter().find(|item| item.memory.id == used.memory_id);
        if memory.is_none_or(|item| is_expired_at(&item.memory, now)) {
            violations.push(format!("scope_or_lifecycle:{}", used.memory_id));
        }
        if retained_ids.contains(&used.memory_id) {
            if used.reason != RetrievalReason::Explicit {
                violations.push(format!("retained_source_precedence:{}", used.memory_id));
            }
        } else if memory.is_some_and(|item| item.memory.pinned) {
            if used.reason != RetrievalReason::Pinned {
                violations.push(format!("pinned_source_precedence:{}", used.memory_id));
            }
        } else if !matches!(used.reason, RetrievalReason::Lexical) {
            violations.push(format!("unselected_source:{}", used.memory_id));
        }
        let max = match used.reason {
            RetrievalReason::Explicit => limits.explicit_item_bytes,
            RetrievalReason::Pinned => limits.pinned_item_bytes,
            RetrievalReason::Lexical | RetrievalReason::Recent => limits.automatic_item_bytes,
        };
        if used.rendered_bytes > max {
            violations.push(format!("item_budget:{}", used.memory_id));
        }
        if used.reason == RetrievalReason::Lexical && exclusions.contains(&used.memory_id) {
            violations.push(format!("automatic_exclusion:{}", used.memory_id));
        }
        if used.reason == RetrievalReason::Lexical
            && memory.is_some_and(|item| {
                !item
                    .memory
                    .memory_type
                    .eligible_for_automatic_recall(&item.memory.source)
            })
        {
            violations.push(format!("automatic_type:{}", used.memory_id));
        }
        if mode == "off" && used.reason == RetrievalReason::Lexical {
            violations.push("recall_off_injected_automatic".into());
        }
    }
    let rendered_ids = final_prompt
        .lines()
        .filter_map(|line| {
            line.strip_prefix("ID: ")
                .and_then(|rest| rest.split_once(" · Scope: "))
                .map(|(id, _)| id.to_owned())
        })
        .collect::<Vec<_>>();
    if rendered_ids.len() != ids.len() || rendered_ids.iter().any(|id| !ids.contains(id)) {
        violations.push("rendered_trace_mismatch".into());
    }
    let utility = value.render_complete_at(&db, &context, now).unwrap();
    for memory in &case.memories {
        if mode != "replace"
            && case.explicit_selection_ids.contains(&memory.id)
            && visible
                .iter()
                .any(|item| item.memory.id == memory.id && !is_expired_at(&item.memory, now))
            && !utility.contains(&memory.body)
        {
            violations.push(format!("utility_body_truncated:{}", memory.id));
        }
    }
    let included_ids = composed
        .uses
        .iter()
        .map(|item| item.memory_id.clone())
        .collect::<Vec<_>>();
    if case.expected_empty && !included_ids.is_empty() {
        violations.push("expected_empty_context_was_nonempty".into());
    }
    let mut report = CompositionCaseReport {
        id: id.into(),
        split: case.split.clone(),
        kind: mode.into(),
        required_count: required.len(),
        recovered_count: required.iter().filter(|id| ids.contains(*id)).count(),
        allowed_returned_count: included_ids
            .iter()
            .filter(|id| allowed.contains(id))
            .count(),
        known_misses: required
            .iter()
            .filter(|id| !ids.contains(*id))
            .cloned()
            .collect(),
        irrelevant_ids: included_ids
            .iter()
            .filter(|id| !allowed.contains(id))
            .cloned()
            .collect(),
        reasons: composed
            .uses
            .iter()
            .map(|item| (item.memory_id.clone(), item.reason.as_str().into()))
            .collect(),
        included_ids,
        final_prompt_bytes: final_prompt.len(),
        non_memory_prompt_bytes: non_memory.len(),
        measured_memory_bytes: final_prompt.len().saturating_sub(non_memory.len()),
        recorded_memory_bytes: composed.memory_bytes,
        explicit_bytes: composed.explicit_bytes(),
        pinned_bytes: composed.pinned_bytes(),
        automatic_bytes: composed.automatic_bytes,
        automatic_count: composed.automatic_count,
        omitted_count: composed.omissions.total(),
        utility_bytes: utility.len(),
        violations,
    };
    report.violations.extend(check_limits(&report, limits));
    report
}

fn union_ids(groups: &[&[String]]) -> Vec<String> {
    groups
        .iter()
        .flat_map(|ids| ids.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn evaluate_composition() -> CompositionReport {
    let fixture: CompositionFixture = serde_json::from_str(FIXTURE).unwrap();
    let base = load_fixture();
    assert_eq!(fixture.version, "composition-v1");
    assert_eq!(fixture.evaluator_version, "memory-composition-eval-1");
    assert_eq!(fixture.base_fixture_digest, reviewed_fixture_digest());
    assert_eq!(fixture.fixed_clock, base.fixed_clock);
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE.as_bytes())),
        FIXTURE_SHA256
    );
    assert_eq!(
        fixture.base_case_ids,
        base.cases.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
    );
    let now = parse_clock(&base);
    let mut reports = Vec::new();
    for case in &base.cases {
        let required = union_ids(&[
            &case.required_ids,
            &case.renderer_required_explicit_ids,
            &case.renderer_required_pinned_ids,
        ]);
        let allowed = union_ids(&[
            &case.allowed_ids,
            &case.renderer_allowed_explicit_ids,
            &case.renderer_allowed_pinned_ids,
        ]);
        let mut report = evaluate_composition_case(
            case,
            &case.id,
            "base",
            &required,
            &allowed,
            &fixture.limits,
            now,
        );
        let supported = ["lexical", "identifier"]
            .iter()
            .any(|cohort| base.cohorts[*cohort].contains(&case.id))
            && case.required_ids.iter().all(|id| {
                case.memories
                    .iter()
                    .find(|memory| memory.id == *id)
                    .is_some_and(|memory| {
                        super::super::MemoryType::from_db(&memory.memory_type)
                            .is_ok_and(|kind| kind.eligible_for_automatic_recall(&memory.source))
                    })
            });
        if supported && (!report.known_misses.is_empty() || !report.irrelevant_ids.is_empty()) {
            report
                .violations
                .push("supported_cohort_composition_regression".into());
        }
        reports.push(report);
    }
    for variation in &fixture.variations {
        assert!(matches!(
            variation.mode.as_str(),
            "repeat" | "replace" | "off" | "trace_failure" | "literal_marker"
        ));
        let case = base
            .cases
            .iter()
            .find(|case| case.id == variation.base_case_id)
            .unwrap();
        let allowed = union_ids(&[
            &case.allowed_ids,
            &case.renderer_allowed_explicit_ids,
            &case.renderer_allowed_pinned_ids,
        ]);
        let mut report = evaluate_composition_case(
            case,
            &variation.id,
            &variation.mode,
            &variation.required_ids,
            &allowed,
            &fixture.limits,
            now,
        );
        for id in &variation.required_ids {
            if !report.included_ids.contains(id) {
                report
                    .violations
                    .push(format!("required_variation_missing:{id}"));
            }
        }
        for id in &variation.forbidden_ids {
            if report.included_ids.contains(id) {
                report
                    .violations
                    .push(format!("forbidden_variation_included:{id}"));
            }
        }
        for (id, reason) in &variation.expected_reasons {
            if report.reasons.get(id) != Some(reason) {
                report.violations.push(format!("source_precedence:{id}"));
            }
        }
        reports.push(report);
    }
    for stress in &fixture.stress_cases {
        let case = stress_case(stress);
        let mode = if stress.recall_enabled {
            "stress"
        } else {
            "off"
        };
        let mut report = evaluate_composition_case(
            &case,
            &stress.id,
            mode,
            &[],
            &case.allowed_ids,
            &fixture.limits,
            now,
        );
        if report.omitted_count == 0 {
            report
                .violations
                .push("stress_did_not_exercise_omission".into());
        }
        if stress.authored_repeats > 1
            && report.non_memory_prompt_bytes <= fixture.limits.shared_bytes
        {
            report
                .violations
                .push("stress_did_not_preserve_large_authored_input".into());
        }
        reports.push(report);
    }
    let base_reports = reports
        .iter()
        .filter(|case| case.kind == "base")
        .collect::<Vec<_>>();
    let aggregate_split = |split: Option<&str>| {
        let cases = base_reports
            .iter()
            .filter(|case| split.is_none_or(|s| case.split == s))
            .collect::<Vec<_>>();
        let required = cases.iter().map(|case| case.required_count).sum::<usize>();
        let recovered = cases.iter().map(|case| case.recovered_count).sum::<usize>();
        let returned = cases
            .iter()
            .map(|case| case.included_ids.len())
            .sum::<usize>();
        let allowed = cases
            .iter()
            .map(|case| case.allowed_returned_count)
            .sum::<usize>();
        json!({"cases":cases.len(),"required":required,"recovered":recovered,"returned":returned,
            "allowed":allowed,"recall":(required>0).then(||recovered as f64/required as f64),
            "precision":(returned>0).then(||allowed as f64/returned as f64)})
    };
    let aggregate = json!({
        "base": aggregate_split(None),
        "development": aggregate_split(Some("development")),
        "validation": aggregate_split(Some("validation")),
        "total_cases": reports.len(),
        "hard_violations": reports.iter().map(|case|case.violations.len()).sum::<usize>(),
        "maximum_measured_memory_bytes":reports.iter().map(|case|case.measured_memory_bytes).max(),
        "maximum_recorded_memory_bytes":reports.iter().map(|case|case.recorded_memory_bytes).max(),
        "notes":"Composition metrics include deliberate pins and selections; compare automatic-only policy metrics in the separate frozen v2 report."
    });
    CompositionReport {
        fixture_version: fixture.version,
        evaluator_version: fixture.evaluator_version,
        fixture_digest: FIXTURE_SHA256.into(),
        base_fixture_digest: fixture.base_fixture_digest,
        fixed_clock: fixture.fixed_clock,
        final_composition_evaluated: true,
        limits: fixture.limits,
        cases: reports,
        aggregate,
        limitations: fixture.limitations,
    }
}

#[test]
fn final_composition_report_is_deterministic_and_enforces_fixed_limits() {
    assert_eq!(
        format!("{:x}", Sha256::digest(ACCEPTED_REPORT.as_bytes())),
        ACCEPTED_REPORT_SHA256,
        "accepted composition report must remain unchanged"
    );
    let first = evaluate_composition();
    let second = evaluate_composition();
    assert_eq!(first.cases, second.cases);
    let serialized = serde_json::to_string_pretty(&first).unwrap() + "\n";
    if let Some(path) = std::env::var_os("ALFRED_MEMORY_COMPOSITION_REPORT") {
        use std::io::Write;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => file.write_all(serialized.as_bytes()).unwrap(),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                assert_eq!(
                    fs::read_to_string(path).unwrap(),
                    serialized,
                    "report output exists; use a new draft path instead of overwriting it"
                );
            }
            Err(error) => panic!("could not write composition report: {error}"),
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&first.aggregate).unwrap()
    );
    let failures = first
        .cases
        .iter()
        .filter(|case| !case.violations.is_empty())
        .map(|case| format!("{}: {:?}", case.id, case.violations))
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn composition_budget_counter_detects_independent_injected_violations() {
    let fixture: CompositionFixture = serde_json::from_str(FIXTURE).unwrap();
    let mut report = evaluate_composition().cases.remove(0);
    report.measured_memory_bytes = fixture.limits.shared_bytes + 1;
    report.recorded_memory_bytes = 0;
    report.pinned_bytes = fixture.limits.pinned_bytes + 1;
    report.automatic_count = fixture.limits.automatic_items + 1;
    let failures = check_limits(&report, &fixture.limits);
    assert!(failures
        .iter()
        .any(|failure| failure.starts_with("shared:")));
    assert!(failures
        .iter()
        .any(|failure| failure.starts_with("accounting:")));
    assert!(failures
        .iter()
        .any(|failure| failure.starts_with("pinned:")));
    assert!(failures
        .iter()
        .any(|failure| failure.starts_with("automatic_items:")));
}
