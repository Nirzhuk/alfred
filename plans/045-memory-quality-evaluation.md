# Plan 045: Establish a reproducible memory quality baseline

> Implementation checkpoint. The evaluator and frozen fixtures below are
> implemented; retain the gate definitions and update the index only after the
> shared done criteria pass. Run all commands from `desktop/`.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/db tests/fixtures/memory-quality`,
> and `git diff HEAD -- src-tauri/src/db tests/fixtures/memory-quality`. Read the live symbols cited
> here. The baseline already contains unrelated uncommitted runner, identity,
> canvas, memory-editor, store, and styling work. Preserve it. Reconcile changes
> made by completed dependencies; report unexplained conflicting changes before
> editing. A clean checkout of the baseline commit omits some current UI files.


## Status

- Status: DONE (verified 2026-09-15: `bun run check` passed 548 frontend tests, the frontend build, and 1,072 Rust tests with 0 failures; `git diff --check` clean; earlier Rust checkpoint failures resolved)
- Priority: P1
- Effort: M
- Risk: LOW
- Depends on: completed memory foundation 025–028
- Category: tests
- Planned at: commit `85f0d64`, 2026-09-08, with the dirty working tree described above

## Implementation result

The evaluator is implemented in `src-tauri/src/db/memory_eval.rs` and is
registered only for Rust tests. It seeds a fresh SQLite database, maintains the
canonical FTS index, and calls the production retrieval/rendering paths. The
retriever now has a test-only fixed-clock entry seam, while the production
clock and policy remain unchanged. Expiry is evaluated through the same fixed
clock as ranking and the harness exercises pinned and explicit rendering as
separate layers. The report distinguishes scope/lifecycle, explicit-exclusion,
byte-budget, and retrieval-error invariants instead of collapsing them into one
counter. It also reports recall-off (automatic retrieval skipped while pins and
explicit selections remain) separately from the empty no-memory mode.

`tests/fixtures/memory-quality/v1.json` contains 50 authored synthetic cases,
split 30 development / 20 validation, with 10 expected-empty cases, concrete
per-case task/event/query sources, and frozen lexical/identifier/paraphrase,
stale-distractor, renderer, and empty cohorts. The checked-in `baseline-v1.json`
is frozen and reproducible. The v1 report records relevance misses from recent
fallback as quality failures; it does not turn them into safety failures or
tune the policy to pass its own benchmark. Empty required sets report null
recall and empty returned sets report null precision, with denominators shown in
each aggregate. The initially rejected optimistic snapshot is retained at
`/tmp/alfred-045-rejected-baseline.json` and is not the canonical baseline.
The final frozen report records development recall/precision of 1.0/0.5532 and
validation recall/precision of 0.9286/0.4242, with one validation relevance
miss, ten expected-empty cases, and zero scope/lifecycle, exclusion, budget,
retrieval-error, unsupported-use, and renderer lifecycle violations. A SHA-256
digest of the complete reviewed fixture protects query, source, memory, and
label contents when a later active policy is compared.

Focused verification completed:

| Command | Result |
| --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval -- --nocapture` | 6 tests passed after final cohort, renderer, digest, and policy-comparison corrections |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml runner::tests::background_memory_review::injected_reviewer_covers_accept_reject_and_invisible_target_evidence -- --exact --nocapture` | 1 injected-boundary lifecycle test passed: accepted create, malformed rejection, invisible target, and circular duplicate evidence |
| `ALFRED_MEMORY_EVAL_WRITE=baseline-v1 cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval::tests::baseline_snapshot -- --exact --nocapture` | Final corrected v1 report generated once after cohort rebalance and metric naming; overwrite refusal is enforced |
| `git diff --check` (target files) | pass |

The injected `ReviewAgent` boundary now has a focused lifecycle test in the
runner test module covering accepted create, malformed-output rejection, and
invisible cross-workflow supersede and circular duplicate evidence. It records
those as validator behavior and never treats them as model accuracy. The final
2026-09-15 `bun run check` passed 548 frontend tests, the frontend build, and
1,072 Rust tests with 0 failures, superseding the earlier 1,068-pass/3-fail
checkpoint; no assertion was weakened, and the index row is DONE.

## Why this matters

The current tests establish storage and ranking behavior but do not show whether
the memories supplied to an agent are useful. Build a small deterministic
evaluation using the real database and retriever so subsequent plans have a
fixed comparison. This plan records current failures honestly; it does not
change retrieval to make its own benchmark pass.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result above supersedes them for current local status; the
fixture and metric invariants remain authoritative.

`src-tauri/src/db/memory_retrieval.rs` exposes `Db::retrieve_memories` and tests
scope, fallback, ranking, rendering, and database errors. Its request is:

```rust
pub struct MemoryRetrievalRequest<'a> {
    pub workflow_id: &'a str,
    pub working_directory: Option<&'a str>,
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub query_text: &'a str,
    pub exclude_ids: &'a [String],
}
```

`src-tauri/src/db/mod.rs` provides `Db::open_in_memory()` behind `cfg(test)`.
Reuse the existing workflow/memory fixture pattern in retrieval tests, including
canonical FTS indexing. `tests/memory-retrieval.test.ts` covers UI controls,
not task accuracy. `package.json` defines `bun run check` as frontend tests,
frontend build, and Rust tests; CI runs it on Linux with Tauri dependencies.

The documented v1 policy intentionally uses lexical search plus recent fallback.
Record this as the baseline to improve, rather than rewriting historical tests
or claims. No user database or real conversation belongs in committed fixtures.

## Scope

- Create `src-tauri/src/db/memory_eval.rs` and register it with `#[cfg(test)]` in
  `src-tauri/src/db/mod.rs`.
- `src-tauri/src/db/memory_retrieval.rs` and `memories.rs` only for narrow
  fixed-clock evaluation seams; production policy and clock defaults stay unchanged.
- Create `tests/fixtures/memory-quality/v1.json`, `baseline-v1.json`, and
  `README.md` in that directory.
- Add focused fixture tests to the new evaluation module; add a fake review
  scenario only through the existing injected `ReviewAgent` test boundary.
- Update this plan/index. Documentation of evaluation belongs in the fixture README.
- Out of scope: production ranking, provider invocation, UI redesign, embeddings,
  schema migrations, network benchmarks, and changing any memory preference.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Existing memory baseline | `cargo test --locked --manifest-path src-tauri/Cargo.toml memory` | all pass |
| New evaluation | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval -- --nocapture` | invariant tests pass; versioned metric report printed |
| Explicit baseline generation | `ALFRED_MEMORY_EVAL_WRITE=baseline-v1 cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval::tests::baseline_snapshot -- --exact --nocapture` | writes the reviewed synthetic baseline to its fixed fixture path |
| Frontend controls | `bun test tests/memory-retrieval.test.ts tests/memory-curation.test.ts` | all pass |
| Final integration | `bun run check` | exit 0 |
| Patch hygiene | `git diff --check` | exit 0 |

## Steps

### 1. Define the fixture and metric contracts

Create 50 synthetic cases across natural wording, old relevant facts behind at
least ten recent distractors, exact identifiers, multilingual text, scoped
visibility, expiry/retraction, explicit exclusion, unrelated queries, reports,
and duplicated/conflicting claims. At least ten cases must have no relevant
memory. Include ordinary production-approval phrasing from the audit and
meaning-equivalent wording with no shared keywords.

Each case has a stable id/category, a fixed clock, workflow context, memories
with explicit source/type/times, typed task/event/query sources, optional pins,
explicit selections and context-template inputs, required ids, allowed ids,
forbidden ids, and an expected-empty flag. Keep a separate short task question and
expected answer facts for future model evaluation. Do not label relevant facts
by matching the current algorithm's output.

Separate automatic-retrieval labels from final-prompt labels. Record which
evaluation layer each case exercises. Template/final-composition expectations
can be fixed now but remain explicitly unevaluated until Plan 048 supplies the
real structured renderer; do not approximate the runner in this harness.

Define recall as recovered required ids / required ids; precision as allowed
returned ids / returned ids for nonempty results; report empty outputs
separately so they cannot inflate precision. Count scope/lifecycle violations,
unsupported memory uses, irrelevant returned items, rendered bytes, and
case-level results. Report empty-required denominators explicitly.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval::fixture_contract`
passes id uniqueness, reference validity, counts, and category coverage.

### 2. Evaluate the production code with fixed time

Exercise `Db::retrieve_memories` through test-only clock injection at the
existing `ranked_candidates(..., now)` seam if necessary. Also make expiry use
that clock in the evaluation path; avoid fixtures that expire relative to
today. Do not copy the ranking implementation into the evaluator.

Seed canonical rows and FTS with in-memory SQLite. Add a no-memory baseline
that removes automatic, pinned, and explicit memory in the test harness, and a
current-runtime baseline. Disabling automatic recall alone is a different mode
that preserves pins and explicit selections. Print machine-readable
JSON with fixture version, evaluator version, policy version, and per-category
metrics. Keep wall-clock latency diagnostic only in ordinary CI; no brittle
machine-dependent timing assertion.

Here the current-runtime report covers production automatic retrieval and
existing database renderers for pins/explicit selections as separate layers.
Assert only the byte limits that already exist for each layer; report unbounded
explicit rendering as a limitation, not a passing unified budget. Full agent
composition and the new total-byte invariant become executable in Plan 048.

**Verify:** run the evaluation command twice; policy metrics and per-case ids
are identical, aside from explicitly excluded timing fields.

### 3. Freeze an honest baseline

Use the explicit baseline-generation command, review its synthetic output, and
retain the results as `baseline-v1.json`. Ordinary tests compare/print without
writing fixtures. The generator accepts only named fixture versions and refuses
to overwrite a frozen version. Later plans add a new active-policy result while
preserving v1 for comparison; do not retain obsolete production code just to
regenerate it. Add
separate tests for hard safety invariants, which must pass now, and comparison
reporting for quality failures, which must remain visible without failing the
existing behavior baseline. Do not mark known misses as successful retrieval.

Document which cases will become release gates in Plans 047–051. Freeze the
labels before tuning. Split the 50 cases into a 30-case development set and
20-case validation set by explicit fixture field; report both. These are
synthetic regression sets, not a generalization claim or a hidden benchmark.

**Verify:** the evaluation command reproduces the checked-in baseline and the
unrelated-query/long-prompt limitations appear in the report.

### 4. Distinguish policy tests from model evaluation

Use fixed reviewer outputs to test acceptance/rejection and circular-evidence
cases. Mark these as validator tests. They do not measure a model's extraction
accuracy. Document an optional future live-run protocol with explicit provider,
model/version, cost limit, number of repeats, answer rubric, and reviewed data.
No live provider dependency enters `bun run check`.

**Verify:** the evaluation and frontend commands pass with network/provider
access unavailable, followed by the final integration and hygiene commands.

## Done criteria

- [x] Fifty versioned synthetic cases and the fixed development/validation split exist.
- [x] Baseline JSON is reproducible against production retrieval.
- [x] Scope, lifecycle, byte-budget, and input-contract invariants pass.
- [x] Known relevance failures are visible, with unambiguous metric denominators.
- [x] No assertion equates fake reviewer behavior with real model accuracy.
- [x] Focused gates and `bun run check` pass; index status is updated by root after integration.

## Specific stop conditions

If a baseline violates scope or lifecycle isolation, record the failing case
and require a targeted fix before allowing downstream automatic-memory rollout.
If a proposed metric needs private user data, replace it with a synthetic case.
Do not block this baseline merely because relevance is poor; recording that
behavior is its purpose.

## Execution boundaries

A filtered test command must run its named cases; zero matching tests is not a
passing gate. Add the specified tests before using their new filters.

Use an isolated branch or worktree if another executor owns overlapping files.
Never reset, stash, overwrite, or commit the user's unrelated changes. Branch
suggestion: `advisor/045-memory-quality-evaluation`.
Do not push, publish, or open a PR unless authorized. Only mark this plan DONE
after its focused gates and `bun run check` pass. A pre-existing full-check
failure must be recorded with its exact command and cause; it is not a pass.

All schema changes must work on both fresh and existing databases, preserve
canonical ids, bodies, artifacts, links, and history, and be idempotent. Follow
`Db::with_conn`, `DbError`, serde camelCase DTOs, and transactional index
maintenance in `src-tauri/src/db/memories.rs`. Never edit a user's live database,
drop canonical rows to repair a migration, or call a provider in ordinary tests.

UI work must reuse the existing Memories/History views, state rollback patterns,
and shared controls. `docs/design-system.md` requires semantic tokens and stable
layout across loading/error states. Verify changed UI with isolated synthetic
IPC data and the available desktop/browser tools. Native macOS Tauri uses
WKWebView; do not assume it is a Chromium target. Read the applicable interaction
skill before UI automation and record the tested states.

Stop and report if the change would widen memory scope, require secret/profile
scraping, change the selected provider or billing source implicitly, or require
an unexplained out-of-scope dependency. Routine conflicts resolved by the stated
contracts do not require another product decision.

## Maintenance

Keep the tested behavior, fixture expectations, and `specs.md` /
`docs/install.md` descriptions aligned. Preserve the distinction between
reference data and authority: memory never grants tool access or overrides a
current user request. Use the existing user/workspace/workflow scopes and lexical
workspace normalization. No cloud memory service, agent-private identity system,
automatic approval, or vector database is part of this plan unless explicitly
specified in its scope.
