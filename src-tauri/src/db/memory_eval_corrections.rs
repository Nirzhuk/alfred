//! Frozen correction labels evaluated through real evidence, candidate, and canonical DB paths.
use crate::db::memory_conflicts::{discover_review_targets_at, ReviewedMemoryTarget};
use crate::db::memory_curation::{
    validate_candidate_suggestion_with_targets_at, ApproveMemoryCandidateInput,
    CandidateReviewContext, CandidateStatus, ListMemoryCandidatesInput, MemoryCandidate,
    ReviewerSuggestionRaw, UpdateMemoryCandidateInput,
};
use crate::db::memory_evidence::{EvidenceCitation, EvidenceEnvelope, EvidenceSourceInput};
use crate::db::{CreateMemoryInput, Db, UpdateMemoryInput};
use chrono::{DateTime, Duration, Utc};
use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

const FIXTURE: &str = include_str!("../../../tests/fixtures/memory-quality/correction-v1.json");
const DIGEST: &str = "c370db4160f7f33cb3bd86f344b87b4fa7e61b7c8e793ebc47490ee50407abf9";

fn clock(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn seed(db: &Db, input: &Value) {
    let workflow = input["workflow_id"].as_str().unwrap();
    super::insert_workflow(db, workflow, input["working_directory"].as_str().unwrap()).unwrap();
    let mut seeded_workflows = HashSet::from([workflow.to_string()]);
    for row in input["canonical"].as_array().unwrap() {
        let owner = if row["scope_type"] == "workflow" {
            row["scope_key"].as_str().unwrap()
        } else {
            workflow
        };
        if seeded_workflows.insert(owner.into()) {
            super::insert_workflow(db, owner, &format!("/fixture/owners/{owner}")).unwrap();
        }
        let created: CreateMemoryInput = serde_json::from_value(json!({
            "id": row["id"], "workflowId": owner, "title": row["title"], "body": row["body"],
            "scopeType": row["scope_type"], "memoryType": row["memory_type"], "source": row["source"],
            "status": row["status"], "observedAt": row["observed_at"],
            "validFrom": row["valid_from"], "expiresAt": row["expires_at"]
        })).unwrap();
        let saved = db.create_memory(created).unwrap();
        assert_eq!(saved.id, row["id"]);
        assert_eq!(saved.scope_key, row["scope_key"]);
        assert_eq!(
            saved.semantic_revision,
            row["semantic_revision"].as_i64().unwrap()
        );
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE memories SET created_at = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    saved.id,
                    row["created_at"].as_str(),
                    row["updated_at"].as_str()
                ],
            )?;
            Ok(())
        })
        .unwrap();
    }
}

fn capture(db: &Db, input: &Value, run: &str, at: DateTime<Utc>) -> EvidenceEnvelope {
    let workflow = input["workflow_id"].as_str().unwrap();
    db.with_conn(|conn| {
        conn.execute("INSERT INTO runs(id, workflow_id, status, created_at) VALUES (?1, ?2, 'completed', ?3)",
            params![run, workflow, at.to_rfc3339()])?;
        Ok(())
    }).unwrap();
    assert!(db
        .ensure_memory_review_job(run, workflow, "claude_code", None)
        .unwrap());
    assert!(db.claim_memory_review(run).unwrap());
    let sources = match &input["evidence"] {
        Value::Array(sources) => sources.clone(),
        source => vec![source.clone()],
    };
    db.capture_run_evidence_sources(
        sources
            .iter()
            .enumerate()
            .map(|(index, source)| EvidenceSourceInput {
                run_id: run.into(),
                step_id: None,
                node_id: Some(format!("observation-{index}")),
                origin: serde_json::from_value(source["origin"].clone()).unwrap(),
                producer_identity: None,
                source_label: Some(format!("Synthetic observation {index}")),
                observed_at: clock(source["recorded_at"].as_str().unwrap()),
                text: source["quote"].as_str().unwrap().into(),
                omission_reason: None,
            })
            .collect(),
    )
    .unwrap();
    db.build_run_evidence_envelope(run).unwrap()
}

fn suggestion(input: &Value, envelope: &EvidenceEnvelope) -> ReviewerSuggestionRaw {
    let proposal = &input["proposal"];
    let source = if input["evidence"].is_array() {
        &input["evidence"][0]
    } else {
        &input["evidence"]
    };
    let fragment = &envelope.fragments[0];
    let citation = EvidenceCitation {
        source_id: fragment.source_id.clone(),
        fragment_id: fragment.fragment_id.clone(),
        fragment_digest: fragment.digest.clone(),
        normalization_version: fragment.normalization_version,
        quote: fragment.text.clone(),
        byte_start: 0,
        byte_end: fragment.text.len(),
    };
    serde_json::from_value(json!({
        "operation": proposal["operation"], "targetMemoryId": proposal["target_memory_id"],
        "scopeType": proposal["scope_type"], "memoryType": proposal["memory_type"],
        "title": proposal["title"], "body": proposal["body"],
        "validFrom": proposal["valid_from"], "expiresAt": proposal["expires_at"],
        "confidence": 0.9, "rationale": "The exact captured observation supports this proposal.",
        "evidenceSubject": source["subject"], "isInference": false, "citations": [citation]
    }))
    .unwrap()
}

fn snapshot(db: &Db) -> BTreeMap<String, Value> {
    let ids = db
        .with_conn(|conn| {
            let mut statement = conn.prepare("SELECT id FROM memories ORDER BY id")?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ids)
        })
        .unwrap();
    ids.into_iter()
        .map(|id| {
            let memory = db.get_memory(&id).unwrap().unwrap();
            (id, serde_json::to_value(memory).unwrap())
        })
        .collect()
}

fn mutations(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) -> usize {
    before
        .keys()
        .chain(after.keys())
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|id| before.get(*id) != after.get(*id))
        .count()
}

fn semantic_outcome(code: &str) -> &str {
    match code {
        "duplicate_content" | "duplicate_replacement" => "exact_duplicate",
        "supersede_unchanged" => "no_op",
        "target_snapshot_incomplete" | "target_not_visible" | "target_snapshot_missing" => {
            "target_snapshot_unavailable"
        }
        "target_inactive" | "target_revision_changed" | "target_digest_changed" => "target_changed",
        "proposal_expired" => "expired",
        "unsupported_expires_at" | "unsupported_valid_from" => "unsupported_validity_date",
        "source_removed" | "evidence_validation_failed" | "evidence_source_missing" => {
            "evidence_unavailable"
        }
        other => other,
    }
}

fn approve(db: &Db, displayed: &MemoryCandidate, at: DateTime<Utc>) -> (String, String) {
    match db.approve_memory_candidate_at(
        ApproveMemoryCandidateInput {
            id: displayed.id.clone(),
            expected_candidate_revision: displayed.candidate_revision,
        },
        at,
    ) {
        Ok(current) => {
            let code = if current.status == CandidateStatus::Approved {
                "approved"
            } else {
                current
                    .blocked_code
                    .as_deref()
                    .unwrap_or("blocked_without_code")
            };
            (semantic_outcome(code).into(), code.into())
        }
        Err(error) => {
            let raw = error.to_string();
            // Source deletion can retain a blocked proposal; report its recorded cause.
            let current = db.get_memory_candidate(&displayed.id).unwrap();
            if let Some(code) = current
                .as_ref()
                .and_then(|candidate| candidate.blocked_code.as_deref())
            {
                return (semantic_outcome(code).into(), raw);
            }
            // Whole-run deletion cascades the proposal and its captured sources.
            // A UI holding that proposal still cannot approve the deleted evidence.
            if raw == "candidate_not_found"
                && current.is_none()
                && !displayed.evidence.is_empty()
                && db
                    .list_run_evidence_sources(&displayed.review_run_id)
                    .unwrap()
                    .is_empty()
            {
                return ("evidence_unavailable".into(), raw);
            }
            (semantic_outcome(&raw).into(), raw)
        }
    }
}

fn evaluate(case: &Value, fixed_clock: DateTime<Utc>) -> Value {
    let db = Db::open_in_memory().unwrap();
    let input = &case["input"];
    seed(&db, input);
    let original = snapshot(&db);
    let workflow = input["workflow_id"].as_str().unwrap();
    let workspace = input["working_directory"].as_str();
    let approval_time = case["clock"].as_str().map(clock).unwrap_or(fixed_clock);
    let review_time = fixed_clock - Duration::hours(1);
    let envelope = capture(&db, input, "correction-run", review_time);
    let mut discovery = discover_review_targets_at(
        &db,
        workflow,
        workspace,
        "correction-run",
        &envelope,
        review_time,
    );
    let mut measured = json!({});
    if case["scenario"] == "discover" {
        let mut target_ids = discovery
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect::<Vec<_>>();
        target_ids.sort();
        measured = json!({
            "required_target_ids": target_ids,
            "maximum_targets": discovery.targets.len(), "maximum_target_bytes": discovery.markdown.len(),
            // This deterministic DB path has no provider executor or provider callback.
            "additional_provider_calls": 0, "discovery_error": discovery.error_code,
        });
    } else {
        let raw = suggestion(input, &envelope);
        if input["proposal"]["target_shown_completely"] == false
            && input["proposal"]["target_memory_id"].is_string()
        {
            for target in &mut discovery.targets {
                target.complete = false;
            }
        }
        let visible = discovery
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect();
        let target_map: HashMap<String, ReviewedMemoryTarget> = discovery.target_map();
        let context = CandidateReviewContext {
            workflow_id: workflow,
            working_directory: workspace,
            visible_memory_ids: &visible,
            skip_target_visibility: false,
            exclude_pending_id: None,
            evidence_envelope: &envelope,
        };
        let validation = validate_candidate_suggestion_with_targets_at(
            &db,
            &context,
            &raw,
            &target_map,
            review_time,
        );
        let mut second_candidate = None;
        if case["scenario"] == "approve_two_candidates" {
            // Controlled interleaving: both reviews validate before either finalizes.
            let second_envelope = capture(&db, input, "correction-run-two", review_time);
            let second_raw = suggestion(input, &second_envelope);
            let second_context = CandidateReviewContext {
                evidence_envelope: &second_envelope,
                ..context
            };
            let second_validated = validate_candidate_suggestion_with_targets_at(
                &db,
                &second_context,
                &second_raw,
                &target_map,
                review_time,
            )
            .unwrap();
            db.finalize_review_success(
                "correction-run",
                workflow,
                &[validation.as_ref().unwrap().clone()],
            )
            .unwrap();
            db.finalize_review_success("correction-run-two", workflow, &[second_validated])
                .unwrap();
            second_candidate = db
                .list_memory_candidates(ListMemoryCandidatesInput {
                    workflow_id: workflow.into(),
                    status: None,
                })
                .unwrap()
                .into_iter()
                .find(|candidate| candidate.review_run_id == "correction-run-two");
        } else {
            let inserted = db
                .create_reviewed_candidates_at(
                    "correction-run",
                    workflow,
                    workspace,
                    &envelope,
                    &discovery.targets,
                    &[raw],
                    review_time,
                    5,
                )
                .unwrap();
            assert_eq!(
                inserted,
                usize::from(validation.is_ok()),
                "{} validation/finalization mismatch",
                case["id"]
            );
        }
        let displayed = db
            .list_memory_candidates(ListMemoryCandidatesInput {
                workflow_id: workflow.into(),
                status: None,
            })
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.review_run_id == "correction-run");
        if let Some(displayed) = displayed {
            let mutation = &input["mutation"];
            match mutation["kind"].as_str() {
                Some("canonical_edit") => {
                    let id = mutation["memory_id"].as_str().unwrap();
                    let old = db.get_memory(id).unwrap().unwrap();
                    let mut update =
                        json!({ "id": id, "expectedSemanticRevision": old.semantic_revision });
                    for (source, destination) in [("body", "body"), ("valid_from", "validFrom")] {
                        if let Some(value) = mutation.get(source) {
                            update[destination] = value.clone();
                        }
                    }
                    db.update_memory(serde_json::from_value::<UpdateMemoryInput>(update).unwrap())
                        .unwrap();
                    if mutation["preserve_updated_at"] == true {
                        db.with_conn(|conn| {
                            conn.execute(
                                "UPDATE memories SET updated_at = ?2 WHERE id = ?1",
                                params![id, old.updated_at],
                            )?;
                            Ok(())
                        })
                        .unwrap();
                    }
                }
                Some("candidate_edit") => {
                    db.update_memory_candidate(serde_json::from_value::<UpdateMemoryCandidateInput>(json!({
                        "id": displayed.id, "expectedCandidateRevision": displayed.candidate_revision,
                        "title": mutation["title"], "evidenceConfirmed": mutation["evidence_confirmed"]
                    })).unwrap()).unwrap();
                }
                Some("delete_source_run") => {
                    db.with_conn(|conn| {
                        conn.execute("DELETE FROM runs WHERE id = 'correction-run'", [])?;
                        Ok(())
                    })
                    .unwrap();
                }
                None => {}
                other => panic!("unhandled mutation {other:?}"),
            }
            let before_approval = snapshot(&db);
            let (outcome, raw_outcome) = approve(&db, &displayed, approval_time);
            measured["outcome"] = json!(outcome);
            measured["raw_approval_outcome"] = json!(raw_outcome);
            measured["approval_canonical_mutations"] =
                json!(mutations(&before_approval, &snapshot(&db)));
            if let Some(second) = second_candidate {
                let after_first = snapshot(&db);
                let (second_outcome, raw_second_outcome) = approve(&db, &second, approval_time);
                measured["approved_count"] = json!(
                    usize::from(outcome == "approved") + usize::from(second_outcome == "approved")
                );
                measured["second_approval_canonical_mutations"] =
                    json!(mutations(&after_first, &snapshot(&db)));
                measured["second_outcome"] = json!(second_outcome);
                measured["raw_second_approval_outcome"] = json!(raw_second_outcome);
            }
            let current = db.get_memory_candidate(&displayed.id).unwrap();
            measured["candidate_retained"] = json!(current.is_some());
            measured["candidate_remains_pending"] = json!(current
                .as_ref()
                .is_some_and(|candidate| candidate.status == CandidateStatus::Pending));
            measured["source_text_removed"] = json!(
                db.list_run_evidence_sources(&displayed.review_run_id)
                    .unwrap()
                    .is_empty()
                    && current.as_ref().is_none_or(|candidate| candidate
                        .evidence
                        .iter()
                        .all(|item| item.quote.is_none() && item.fragment_digest.is_none())
                        && candidate.rationale.is_empty())
            );
        } else {
            measured["outcome"] = json!(semantic_outcome(
                validation
                    .as_ref()
                    .err()
                    .copied()
                    .unwrap_or("candidate_not_created")
            ));
        }
    }
    let after = snapshot(&db);
    let added = after
        .iter()
        .filter(|(id, _)| !original.contains_key(*id))
        .map(|(_, row)| row)
        .collect::<Vec<_>>();
    measured["canonical_mutations"] = json!(mutations(&original, &after));
    measured["new_canonical_rows"] = json!(added.len());
    measured["target_status"] = input["proposal"]["target_memory_id"]
        .as_str()
        .and_then(|id| after.get(id))
        .map(|row| row["status"].clone())
        .unwrap_or(Value::Null);
    let prior_preserved = original.iter().all(|(id, row)| after.get(id) == Some(row));
    measured["prior_row_preserved"] = json!(prior_preserved);
    measured["unrelated_scope_unchanged"] = json!(prior_preserved);
    measured["edited_body_preserved"] = input["mutation"]["memory_id"]
        .as_str()
        .and_then(|id| after.get(id))
        .map(|row| json!(row["body"] == input["mutation"]["body"]))
        .unwrap_or(json!(false));
    for (key, field) in [
        ("normalized_valid_from", "validFrom"),
        ("last_confirmed_at", "lastConfirmedAt"),
        ("expires_at", "expiresAt"),
    ] {
        measured[key] = added
            .first()
            .map(|row| row[field].clone())
            .unwrap_or(Value::Null);
    }
    let mut violations = Vec::new();
    for (key, expected) in case["expected"].as_object().unwrap() {
        let actual = &measured[key];
        let passed = match key.as_str() {
            "required_target_ids" => expected
                .as_array()
                .unwrap()
                .iter()
                .all(|id| actual.as_array().unwrap().contains(id)),
            "maximum_targets" | "maximum_target_bytes" => {
                actual.as_u64().unwrap() <= expected.as_u64().unwrap()
            }
            _ => actual == expected,
        };
        if !passed {
            violations.push(format!("{key}: expected {expected}, measured {actual}"));
        }
    }
    if !measured["discovery_error"].is_null() {
        violations.push(format!("discovery error: {}", measured["discovery_error"]));
    }
    json!({ "id": case["id"], "group": case["group"], "expected": case["expected"], "measured": measured,
        "violations": violations, "rationale": case["rationale"] })
}

#[test]
fn correction_fixture_policy_report() {
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE.as_bytes())),
        DIGEST,
        "frozen correction labels changed"
    );
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 21);
    let fixed_clock = clock(fixture["fixed_clock"].as_str().unwrap());
    let reports = cases
        .iter()
        .map(|case| evaluate(case, fixed_clock))
        .collect::<Vec<_>>();
    assert_eq!(
        reports,
        cases
            .iter()
            .map(|case| evaluate(case, fixed_clock))
            .collect::<Vec<_>>(),
        "correction measurements must be deterministic across fresh databases"
    );
    let mut groups = BTreeMap::<String, Value>::new();
    for report in &reports {
        let group = groups
            .entry(report["group"].as_str().unwrap().into())
            .or_insert(json!({"cases": 0, "passed": 0}));
        group["cases"] = json!(group["cases"].as_u64().unwrap() + 1);
        group["passed"] = json!(
            group["passed"].as_u64().unwrap()
                + u64::from(report["violations"].as_array().unwrap().is_empty())
        );
    }
    let failures = reports
        .iter()
        .filter(|report| !report["violations"].as_array().unwrap().is_empty())
        .collect::<Vec<_>>();
    let report = json!({"fixture_version": fixture["version"], "evaluator_version": fixture["evaluator_version"],
        "fixture_digest": DIGEST, "fixed_clock": fixture["fixed_clock"], "cases": reports, "groups": groups,
        "passed": cases.len() - failures.len(), "total": cases.len(),
        "limitations": ["Synthetic deterministic DB policy evaluation; no live provider or native UI acceptance.",
            "The two-review race uses controlled production validation/finalization interleaving."]});
    let rendered = serde_json::to_string_pretty(&report).unwrap() + "\n";
    println!("{rendered}");
    if let Ok(path) = std::env::var("ALFRED_CORRECTION_REPORT_PATH") {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("correction report must use a new path; existing reports are immutable");
        file.write_all(rendered.as_bytes()).unwrap();
    }
    assert!(
        failures.is_empty(),
        "{} correction policy cases failed",
        failures.len()
    );
}
