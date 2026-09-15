# Plan 047: Retrieve task-relevant memory and abstain on unrelated work

> Implementation checkpoint. Retrieval policy, query-source tracking, and
> ranking-version work below are implemented and locally reviewed; retain the
> final shared gate until it passes. Run all commands from `desktop/`.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/db src-tauri/src/runner src/features/workflow tests/memory-retrieval.test.ts tests/fixtures/memory-quality specs.md docs/install.md`,
> and `git diff HEAD -- src-tauri/src/db src-tauri/src/runner src/features/workflow tests/memory-retrieval.test.ts tests/fixtures/memory-quality specs.md docs/install.md`. Read the live symbols cited
> here. The baseline already contains unrelated uncommitted runner, identity,
> canvas, memory-editor, store, and styling work. Preserve it. Reconcile changes
> made by completed dependencies; report unexplained conflicting changes before
> editing. A clean checkout of the baseline commit omits some current UI files.

## Status

- Status: DONE — high review and scoped browser QA accepted; verified 2026-09-15 by `bun run check` (548 frontend tests, frontend build, 1,072 Rust tests, 0 failures) and clean `git diff --check`
- Effort: L
- Risk: MED
- Depends on: [045](045-memory-quality-evaluation.md); completed scopes/history 025–028
- Category: correctness / retrieval policy
- Planned at: commit `85f0d64`, 2026-09-08, dirty working tree

## Why this matters

Conversational prompts overconstrain today's FTS query, while unrelated recent
items can still be supplied. Change the automatic-recall policy to use bounded
task signals and require relevance. Saved reports remain available through
explicit selection and History. This deliberately replaces documented v1 policy;
it does not rewrite the user's History search semantics.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result below supersedes them for current local status; the
retrieval eligibility, query-source, and ranking-version contracts remain
authoritative.

In `src-tauri/src/db/history.rs:90`, the shared helper bounds words and finishes

```rust
let query_tail = utf8_tail(request.query_text, RETRIEVAL_QUERY_MAX_BYTES);
let fts_query = plain_text_fts_query(query_tail);
```

The retriever always appends up to ten recent candidates at line 285. The same
`ranked_candidates` function serves live recall and `retrieve_review_context`.
`runner/mod.rs:403` uses `base_prompt` as its query; trigger data is composed
separately. Explicit memory has already entered `context_prompt` by line 1031.
The existing `run_memory_uses` rows have scores and reasons but no ranking
version. Plan 027's maintenance section requires versioning before changing scores.

The existing scope/lifecycle rules and new-workflow on / migrated-workflow off
defaults stay intact. History AND search is a user-facing feature to retain.

## Scope

- `src-tauri/src/db/memory_retrieval.rs`; new `memory_query.rs` and registration
  in `db/mod.rs`; `memories.rs` only for shared eligibility helpers.
- `src-tauri/src/db/memory_curation.rs` only for retrieval-purpose signature
  propagation and Review-context regression tests.
- `src-tauri/src/runner/mod.rs`; new `runner/retrieval_query.rs` for bounded
  source collection and its tests.
- `src-tauri/src/db/schema.sql`, `migrate.rs`, and `history.rs` for ranking
  version metadata and DTOs, preserving History's search algorithm.
- `src/features/workflow/types.ts`, memory-use models/components under
  `components/history-page/`, and
  `components/memories-inspector/memory-automation-settings.tsx` for truthful copy.
- `tests/memory-retrieval.test.ts`, Plan 045's evaluation module/fixtures,
  `specs.md`, `docs/install.md`, this plan/index.
- Out of scope: embeddings/model downloads, cloud search, LLM query rewriting,
  automatic memory writes, agent-private scopes, History API permission changes,
  broad runner cleanup, provider changes.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Recall and query tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory` | all pass |
| Runner integration | `cargo test --locked --manifest-path src-tauri/Cargo.toml runner::tests::memory` | all pass |
| Query-source state | `cargo test --locked --manifest-path src-tauri/Cargo.toml runner::retrieval_query` | all new source/reset/template tests pass |
| History compatibility | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` | all pass |
| Migration | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` | all pass |
| Quality comparison | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval -- --nocapture` | required gates below pass; v1/v2 comparison printed |
| UI controls | `bun test tests/memory-retrieval.test.ts` | all pass |
| Full gate | `bun run check` and `git diff --check` | both exit 0 |

## Steps

### 1. Separate retrieval purpose and query sources

Introduce `RetrievalPurpose::{Automatic, Review}`; Plan 052 may add bounded
interactive lookup later. Make purpose explicit at every caller. Keep Review's
independent 12-item/12-KiB bound and ability to find currently valid targets of
all ten memory types. Correction discovery must not inherit Automatic's type
filter accidentally.

Automatic type eligibility is fixed here:

| Type | Eligible for automatic recall |
| --- | --- |
| preference, fact, decision, constraint, lesson | yes |
| note | yes when manually authored or approved through review |
| episode, checkpoint, output, artifact | no |

All selected active/unexpired types remain eligible for pins and explicit nodes.
Do not delete or reclassify any existing row. Review exclusion does not imply a
history row is deleted.

Add a `TaskQueryState` in parallel with both current context and last output
until Plan 048 unifies prompt origins. It records authored Input/Prompt text, authored template
text with append/replace semantics, and separately bounded event fields. Preserve
origin on any observation supplied as a query hint. In this version, only
authored task/template text and normalized trigger fields qualify. Ordinary
model output and arbitrary tool text are excluded. Memory-node body text,
pinned/retrieved text, trust headings, identity instructions, and provider
wrappers never become task query text. If a template expands context, derive
query text from its eligible source parts, not the already formatted prompt.
Plan 048 will reuse these origins when it structures prompt rendering.
Mirror append/replace/reset and `{{context}}`/`{{output}}` substitutions in this
parallel representation. Memory → template `{{output}}` → Agent preserves the
memory origin and contributes no query terms from that body.

For connected-app events, extract only already-normalized useful fields from
the existing payload: message/subject text and stable project/ticket/document
identifiers when present. Apply a fixed allowlist in the helper, 2,000-byte
event allocation within the 8,000-byte total query-input bound, and never fetch
additional data. Event terms influence relevance only; they cannot change scope
or authorize actions. An absent eligible source yields successful abstention.

**Verify:** the query/runner commands pass event-only identifiers, explicit-memory
exclusion, generic boilerplate, source reset, and both Agent/Custom agent cases.

### 2. Add safe ranked partial lexical retrieval

Implement a dedicated query builder in `db/memory_query.rs`. Keep parameterized
SQL and quote literal terms. Do not reuse or alter the History AND helper.
Normalize Unicode/diacritics consistently with FTS, split punctuation, deduplicate
terms, and bound term count/length. Preserve exact identifiers as an additional
signal. A small documented stop-word/boilerplate list can remove common wording;
it must not erase identifiers or imply general multilingual understanding.

Use disjunctive candidate retrieval plus token/identifier coverage and BM25 for
ranking. Require actual query evidence before admitting a candidate. Select a
deterministic relevance threshold using only Plan 045's development cases,
record it as a named policy constant with its rationale, and evaluate unchanged
validation cases. Scope, recency, salience, and model confidence can only
rerank admitted candidates. They cannot admit a nonmatch. Remove recent-only
injection from Automatic. Review uses the same evidence-based admission rule
over its broader type set, without recent-only padding; this is also an explicit
v2 change. Plan 050 will add candidate-specific conflict discovery rather than
relying on recent fallback. Retain historical `recent` trace interpretation.

Preserve chunked visible-ID matching beyond 400 ids and stable tie-breaks.
Bound candidates and avoid loading artifact bodies merely to rank them.
Do not promise recovery of paraphrases with no lexical or identifier overlap.

**Verify:** recall tests and the quality command pass the audit's natural
release-checklist example behind ten unrelated recent records, event-only
identifiers, punctuation/diacritics, and the existing large-visible-set case.

### 3. Version trace semantics and successful empty results

Add `ranking_version` to memory-use metadata with an idempotent migration.
Historical lexical/recent records receive `lexical_v1`; historical and new
pinned rows receive `pinned_v1`. New Automatic/Review scores use `lexical_v2`. Preserve
old reasons and rows. Return a non-error empty result when no eligible relevant
memory exists; database failure remains `memory_retrieval_unavailable`.

Record the policy version with included uses and expose it in History details.
Keep no-body/no-query telemetry. Count omissions and abstention separately.
Do not describe ranking scores or model confidence as measured truth.

**Verify:** migration/history tests preserve old traces, repeated startup is a
no-op, and UI tests distinguish zero relevant memories from unavailable recall.

### 4. Gate the policy change and update descriptions

Freeze `lexical_v2` output alongside v1 without overwriting baseline artifacts.
All scope/lifecycle cases must return zero forbidden ids. All predesignated
unrelated/empty/noisy cases must abstain. The audit's conversational lexical
regressions must recover required ids, and exact-identifier cases must not
regress. Compare precision and required recall against v1 separately for the
predeclared supported lexical/identifier cohorts and eligible memory types;
neither may regress there. Also report whole-set metrics and coverage lost by
removing accidental recent matches or raw-report recall. Zero-overlap semantic
cases stay labeled misses, not hidden exclusions or false lexical successes.
Declare cohorts before tuning and preserve every original case/required-id label.

Update existing assertions that intentionally required v1 recent fallback,
while retaining v1 fixture results. Explain local lexical matching, successful
empty results, and raw-report exclusion in the current automation controls and
docs. No new settings or provider call is needed.

**Verify:** run every command in the table. Record development and validation
metrics separately, followed by the complete integration result.

## Done criteria

- [x] Automatic and Review purposes have explicit, tested eligibility.
- [x] Task/event queries exclude supplied memory and use literal-safe matching.
- [x] All empty/unrelated fixtures abstain; named lexical regressions pass.
- [x] Exact identifiers, scope/lifecycle, exclusions, off mode, and error behavior pass.
- [x] Historical traces keep their meaning; v2 ranking is versioned.
- [x] Quality comparison and full integration gates pass; index is updated.

## Implementation result

Plan 047 now uses the named `lexical_v2` policy with a development-selected
minimum term-coverage threshold of `0.25`. It requires lexical or exact
identifier evidence, removes recent-only admission, applies the fixed Automatic
type table, and preserves Review's broader ten-type view under its 12-item /
12-KiB bound. Query state keeps authored task, authored template, and allowlisted
event sources separate from memory and generated output. Stable event
identifiers receive a bounded position within the 20-term ceiling even after a
long authored task. Template aliases and placeholder ordering mirror prompt
substitution under the 8-KiB query cap, while retained query values are capped
at 6 KiB and 64 fragments during construction to prevent repeated context
expansion from growing state exponentially.

The unchanged v1 fixture and report SHA-256 values are
`f19b5e3ef701b31aaaed9aae8ab55f80d1a305a286b6c7e43c468bcc9e063d8e` and
`35d6bad040de6be65e36a5e8969a18f532865db0b86403053cd3b979cb32edfb`.
`baseline-v2.json` freezes the initial Plan 047 comparison. Development recall
is 25/26 (`0.9615`) with precision 25/28 (`0.8929`); validation recall is 10/14
(`0.7143`) with precision 11/13 (`0.8462`). Whole-set recall is 35/40 (`0.875`)
and precision is 36/41 (`0.8780`). The predeclared lexical cohort remains 11/11
at precision 1.0 and the identifier cohort remains 5/5 at precision 1.0. All ten
expected-empty cases abstain, with zero scope/lifecycle, exclusion, budget,
retrieval-error, or unsupported-use violations. The frozen report explicitly
keeps zero-overlap misses `m11-required`, `m23`, and `m38-required`, plus raw
report exclusions `m43` and `m44`; the whole-set recall decline is not presented
as a quality improvement.

Focused verification completed on 2026-09-09:

- `db::memory` aggregate command: 52 passed.
- `runner::retrieval_query`: 10 passed, including the long-authored-task event
  identifier database regression and retained-state expansion bound.
- `db::memory_query`: 5 passed.
- `db::memory_retrieval::tests`: 8 passed.
- `runner::tests::memory`: 7 passed.
- `db::history`: 10 passed.
- `db::migrate`: 18 passed.
- `db::memory_curation::tests::review_existing_memory_context_is_capped`: 1 passed.
- `db::memory_eval`: 6 passed.
- `bun test tests/memory-retrieval.test.ts`: 8 passed.
- `bun run build:frontend` and `git diff --check`: passed.

The focused results above remain accepted. The final 2026-09-15 `bun run check`
passed 548 frontend tests, the frontend build, and 1,072 Rust tests with 0
failures, superseding the earlier 1,068-pass/3-fail checkpoint; no assertion
was weakened.

## Specific stop conditions

If acceptable lexical quality requires a model call or embedding dependency,
document the limitation and propose a separate measured experiment. Do not add
one silently. If v2 violates Step 4's fixed validation gates for supported
cohorts or hard invariants, do not enable it by changing fixture labels.
Whole-set losses outside those cohorts remain reported limitations, as defined
in Step 4. Preserve manual search behavior throughout.


## Execution boundaries

A filtered test command must run its named cases; zero matching tests is not a
passing gate. Add the specified tests before using their new filters.

Use an isolated branch or worktree if another executor owns overlapping files.
Never reset, stash, overwrite, or commit the user's unrelated changes. Branch
suggestion: `advisor/047-task-aware-memory-recall`.
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
