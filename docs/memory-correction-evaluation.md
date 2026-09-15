# Correction policy evaluation

`tests/fixtures/memory-quality/correction-v1.json` contains 21 synthetic cases
in seven groups. The labels were written before the Plan 050 integration and
remain unchanged. They describe required outcomes, mutation counts, target
discovery, and evidence/date behavior; no similarity function generated them.

`src-tauri/src/db/memory_eval_corrections.rs` seeds the actual in-memory schema
through canonical writers, captures evidence through the production API, and
uses production target discovery, proposal validation, review finalization, and
approval. Its fixed clock is 2026-08-18T12:00:00Z, with an explicit boundary clock
in the offset-date case. It runs every case twice on fresh databases and compares
the measurements. Discovery IDs are compared as sets, matching the labels.

The focused report passed all 21 cases and independent high-effort review. This
is a deterministic policy regression result, not a measure of model answer
quality or evidence that a live reviewer/runtime is supported.

| Group | Passed |
| --- | --- |
| Exact duplicate rejection | 3/3 |
| Legitimate separate claims | 4/4 |
| Old-target discovery | 2/2 |
| Operation-specific duplicates | 1/1 |
| Stale approvals prevented | 4/4 |
| Temporal correctness | 4/4 |
| Unsupported corrections blocked | 3/3 |

The competing-approval case deliberately validates two reviews before either
finalizes, then approves both against the same recorded target snapshot. Only
one may change canonical memory. Whole-run deletion cascades the candidate and
captured sources; the report preserves the raw `candidate_not_found` result and
`candidate_retained: false`. Its semantic `evidence_unavailable` outcome requires
verified absence of the original sources, with zero canonical mutations.

Run the check from `desktop/`:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml correction_fixture_policy_report -- --nocapture
```

The measured baseline is `tests/fixtures/memory-quality/baseline-correction-v1.json`.
To export a new report, set `ALFRED_CORRECTION_REPORT_PATH` to a new absolute file
path. The evaluator uses `create_new` and refuses to overwrite an existing report.
The earlier retrieval and composition fixtures/reports remain frozen separately.
