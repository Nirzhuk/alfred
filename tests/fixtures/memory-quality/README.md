# Memory quality fixture v1

This directory contains the frozen, synthetic baseline for Alfred's v1 memory
retrieval policy. `v1.json` has exactly 50 labelled cases: 30 development and
20 validation. Ten cases intentionally have no relevant memory. The labels are
authored from the task question and expected answer facts; they are not
derived from the current lexical-plus-recent result.

The evaluator seeds a fresh SQLite database for each case, indexes the rows
through the same FTS maintenance function used by production, and calls the
real retrieval and existing pinned/explicit renderers at the fixture's fixed
clock. It reports memory-id recall and precision with empty-result denominators
shown separately. Scope, lifecycle, exclusion, and existing byte limits are
hard invariants. Relevance misses stay visible in the report and do not fail
the v1 behavior baseline; visible but disallowed items are reported as
`quality_forbidden_returned`, separately from safety invariants.

The fixture declares task, event, and query as the supported typed source
contract. Every case carries concrete values for all three source kinds;
`typed_source_cases` and the frozen `cohorts` object are contract metadata, not
top-level approximations. The `lexical`, `identifier`, `paraphrase`,
`stale_distractor`, `renderer`, and `empty` cohorts are frozen before later
policy tuning. Cases c11-c15 contain at least eleven recent distractors, and
c11 is a long, no-shared-keyword paraphrase that intentionally records a v1
relevance miss. Task questions and expected answer facts are kept separately
for future model evaluation. Every memory row spells out its type, source,
status, pinned state, and deterministic created/updated timestamps.

The current v1 harness calls the fixed-clock automatic retriever and the real
pinned and explicit renderers. It also reports a recall-off mode (automatic
retrieval skipped while pins/selections remain) separately from the empty mode
(rows, pins, and selections removed). Expired pinned/explicit rows are checked
at the same fixed clock as retrieval. Final prompt composition and the
cross-layer budget were unevaluated at the v1 freeze; the separate Plan 048
evaluation below supplies that coverage. No private data, live provider, network call, model accuracy claim,
or wall-clock benchmark is part of this baseline.

The first draft was rejected because its labels made retrieval look perfect and
did not contain the required stress cases. That draft is retained outside the
repository at `/tmp/alfred-045-rejected-baseline.json` for audit; the checked-in
`baseline-v1.json` was generated only after the fixture and metric corrections.

Future live evaluation is optional and must name the provider, model and
version, cost limit, repeat count, and answer rubric before it runs. It must
use reviewed synthetic or explicitly approved data and remain separate from
the offline test command. The runner's injected reviewer test covers accepted
create output, malformed output rejection, and an invisible cross-workflow
supersede target. It also covers a repeated existing claim rejected by the
duplicate hash guard as circular evidence. These are validator outcomes, not
evidence about model quality.

Generate the frozen report exactly once with:

```sh
ALFRED_MEMORY_EVAL_WRITE=baseline-v1 cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval::tests::baseline_snapshot -- --exact --nocapture
```

The generator accepts only `baseline-v1` and refuses to overwrite an existing
frozen report. Ordinary evaluation compares against the checked-in JSON. If a
later policy changes `MEMORY_RETRIEVAL_POLICY_VERSION`, the evaluator keeps the
v1 artifact and emits a `PolicyComparison` with recall/precision deltas instead
of treating the changed active result as a corrupted historical baseline.

`baseline-v2.json` freezes Plan 047's first `lexical_v2` comparison against that
unchanged v1 report. It records development and validation separately, preserves
whole-set losses, and gates the predeclared lexical and identifier cohorts.
Create it only when absent with `ALFRED_MEMORY_EVAL_WRITE=baseline-v2`; ordinary
runs refuse drift in either frozen artifact.

## Plan 048 composition evaluation

`composition-v1.json` inherits all fifty original cases and their split/relevance
labels without modifying v1. Six additional cases cover actual typed template
duplication/replacement, recall off, expired selection, literal marker text, and
the composer's automatic-memory removal after trace failure. Four stress cases
cover large Unicode bodies, category headers, competition between sources, and
complete utility rendering. The fixture's digest is checked by the evaluator.

The separate `db::memory_eval::composition` test calls the real clock-aware
exclusion, retrieval, template, and final composition functions. It measures the
final prompt's additional bytes against the same prompt with memory removed,
including the final separator. It checks those measured bytes against the
recorded count, 12,000-byte shared limit, 6,000-byte category limits, item/count
limits, source precedence, duplicates, scope/lifecycle, automatic exclusions,
and preserved authored text. Complete utility bodies are checked separately.
The existing runner tests remain responsible for actual dispatch and database
trace-failure orchestration; this evaluator invokes the composer's removal path.

Run all old and new evaluation checks with:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval -- --nocapture
```

Set `ALFRED_MEMORY_COMPOSITION_REPORT` to a task-owned temporary JSON path to
inspect the current complete result. Draft reports are reviewed before being
accepted in this directory. Composition relevance metrics include deliberate
pins/selections; they are not automatic-only recall metrics or model accuracy.

After high-effort acceptance of the production runner paths, root accepted
`baseline-composition-v1.json`. Its SHA-256 is
`8d9a27d7d1ca6dd6d3a9d4963c88f9168e651dab4fcf830725d37da78de8413a`.
All 60 cases pass the hard gates; maximum measured and recorded memory size is
10,967 bytes. Base composition recall is 36/40 and precision is 37/42, including
deliberate selection. The evaluator checks this historical report's digest and
refuses to overwrite a different report at an existing output path. Use a new
absolute temporary path when inspecting later behavior.

## Plan 052 incremental lookup evaluation

`lookup-v1.json` is the frozen synthetic fixture for permission-gated,
incremental memory lookup. Its SHA-256 is
`6deeb417343762984b13f568764abf643a014932d25814ba9609aa8a497edfc6`.
The accepted report is frozen as `baseline-lookup-v1.json`; its SHA-256 is
`55e65c88567676b19126707f26557e5ecc03baff934856de059db3ebb5fd7405`.
The report records 9/9 initial required sources and 13/13 lookup-required
sources, with 5 sources recovered after an initial miss, 29 lookup calls,
22,623 returned bytes, a maximum of 8 calls and 9,486 returned bytes in one
case, 4 expected denials, and 0 hard violations.

Generate a new draft report with an absolute path:

```sh
ALFRED_MEMORY_LOOKUP_EVAL_REPORT=/tmp/alfred-memory-lookup-report.json \
cargo test --locked --manifest-path src-tauri/Cargo.toml \
  db::memory_eval::lookup::tests::lookup_evaluation_is_repeatable_and_hard_invariants_hold \
  -- --exact --nocapture
```

The report writer requires a new path and syncs the complete JSON before
returning. `timing_excluded_from_repeatability` is true: `latency_ms[0]` is
initial retrieval/composition, followed by one duration for each lookup action.
The `recall_miss_recovery` category is an intended incremental-recall
scenario; the observed initial recall may already be complete. All fixtures
and results are synthetic and offline: they do not establish provider
availability, model answer accuracy, live usefulness, or live cost.
