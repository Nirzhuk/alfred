# Plan 050: Apply reviewed corrections to the right memory version

> Implementation checkpoint. Versioned correction, validity, evidence, and
> migration work below is implemented and locally reviewed; keep the final
> shared gates pending until they pass. Run all commands from `desktop/`.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/db src-tauri/src/runner/mod.rs src-tauri/src/commands/mod.rs src/features/workflow tests/memory-curation.test.ts tests/memory-model.test.ts tests/memories-inspector.test.ts tests/memory-store-actions.test.ts tests/fixtures/memory-quality specs.md docs/install.md`,
> and `git diff HEAD -- src-tauri/src/db src-tauri/src/runner/mod.rs src-tauri/src/commands/mod.rs src/features/workflow tests/memory-curation.test.ts tests/memory-model.test.ts tests/memories-inspector.test.ts tests/memory-store-actions.test.ts tests/fixtures/memory-quality specs.md docs/install.md`. Read the cited live symbols.
> Preserve the existing dirty runner, identity, memory-editor, store, canvas, and
> styling work. Reconcile completed Plans 047 and 049; report unexplained conflicts.

## Status

- Status: DONE. Backend high review, the 21-case frozen evaluation, and both-theme correction UI are accepted. Verified 2026-09-15 by `bun run check` (548 frontend tests, frontend build, 1,072 Rust tests, 0 failures) and clean `git diff --check`.
- Priority: P1
- Effort: L
- Risk: MED
- Depends on: `plans/049-evidence-backed-memory-curation.md` and
  `plans/047-task-aware-memory-recall.md`
- Category: correctness / migration
- Planned at: commit `85f0d64`, 2026-09-08, with the existing dirty working tree

## Why this matters

A correction can miss an old claim, create a second active version, or replace
a claim that somebody edited while the proposal was waiting. Existing transactions
prevent partial writes but do not compare the target version the reviewer saw.
Add focused conflict discovery, explicit target and candidate revisions, and
evidence-based dates while keeping every canonical change subject to approval.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result that follows supersedes them for current local status;
the revision, validity, and evidence invariants remain the governing contract.

`src-tauri/src/db/memory_curation.rs:1404` computes `candidate_content_hash` from
whitespace-collapsed body, scope key, and memory type. It does not recognize
equivalent wording. The duplicate scan at line 863 uses:

```sql
SELECT body FROM memories
WHERE scope_type = ?1 AND scope_key = ?2 AND memory_type = ?3
  AND status = 'active'
```

This includes expired rows. The duplicate guard at line 967 runs before the
operation switch, so it also applies to retract. A retraction quoting its own
target can therefore collide with that target.

`runner/mod.rs:2342` retains only IDs of shown memories. At
`memory_curation.rs:955`, approval checks target existence, active status, expiry,
and scope. It does not compare revision/content. `memories.rs:1026` updates body,
title, scope/type, dates, and status in place. `approve_memory_candidate` at
`memory_curation.rs:503` and `approveMemoryCandidate` at `workflow/api.ts:385`
accept an ID without the revision displayed in the UI.

Canonical inserts at `memory_curation.rs:1007` set `last_confirmed_at` and
`expires_at` to `None`. The schema already has `supersedes_id`, lifecycle states,
and those two timestamps. Preserve that model and its user/workspace/workflow
scopes. Do not replace SQLite or reinterpret old timestamps silently.

Plan 047 gives retrieval explicit Automatic and Review purposes. Review may
consider all ten currently valid memory types within 12 items/12 KiB. Automatic
recall has a narrower type policy, successful abstention, and versioned ranking.
Plan 049 supplies structured independent evidence, exact shown-source citations,
claim validation, and deletion-aware canonical provenance. Reuse both contracts.

## Implementation result

Canonical memories now carry semantic revisions and versioned candidate
snapshots/digests. Conflict discovery is bounded and context-specific; exact
duplicate handling is operation-aware, while approximate relationships remain
visible suggestions. Approval rechecks candidate and target revisions,
validity, scope, evidence, and lifecycle in one transaction, with no silent
target substitution or second reviewer call.

Observation/validity dates use the supported UTC interval policy and an injected
clock. Fresh and legacy migration/bootstrap paths preserve canonical history
and provenance, including same-timestamp editor hydration. The fixed-label
correction evaluator is frozen at 21/21 with repeated fresh-DB equality, and
both-theme correction UI checks are accepted. After 051/052 integration, the
final 2026-09-15 `bun run check` passed 548 frontend tests, the frontend build,
and 1,072 Rust tests with 0 failures, superseding the earlier 1,068-pass/3-fail
checkpoint; no assertion was weakened. This does not claim native/provider
acceptance.

## Scope

- Create `src-tauri/src/db/memory_conflicts.rs`; register in
  `src-tauri/src/db/mod.rs`.
- `src-tauri/src/db/memory_curation.rs`, `memory_evidence.rs`, `memories.rs`,
  `memory_retrieval.rs`, `memory_query.rs`, `schema.sql`, `migrate.rs`, and
  `history.rs` in the same DB directory.
- `src-tauri/src/runner/mod.rs`, only review target-context assembly and tests.
- `src-tauri/src/commands/mod.rs`, candidate/approval DTOs and canonical-memory
  edit revision/date plumbing only.
- `src/features/workflow/types.ts`, `src/features/workflow/api.ts`,
  `src/features/workflow/store.ts`, only existing canonical-memory edit calls,
  `src/features/workflow/memories.ts`, only prompt eligibility/date helpers, and
  `src/features/workflow/components/memories-inspector/{memories-inspector.tsx,suggestions-model.ts,memory-editor.tsx,memory-workspace.css}`.
- `tests/memory-curation.test.ts`, `tests/memory-model.test.ts`,
  `tests/memories-inspector.test.ts`, `tests/memory-store-actions.test.ts`, and
  Plan 045's `src-tauri/src/db/memory_eval.rs` plus
  `tests/fixtures/memory-quality/` for additive correction cases and reports.
- This plan/index and correction/validity wording in `specs.md` and `docs/install.md`.

Out of scope: automatic merges, approvals, or deletions; new provider calls;
semantic embeddings; cross-scope writes; new private-agent identities; uniform
expiry periods; changing ordinary History search; changing provider billing or
tool permissions; migration of private production data by the executor.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Conflict/version tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_conflicts` | all pass; matching tests exist |
| Memory regression | `cargo test --locked --manifest-path src-tauri/Cargo.toml memory` | all pass |
| Migrations | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` | all pass |
| Quality report | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval -- --nocapture` | fixed-label gates pass; reports printed |
| UI/models | `bun test tests/memory-curation.test.ts tests/memory-model.test.ts tests/memories-inspector.test.ts` | all pass |
| Integration | `bun run check` | exit 0 |
| Hygiene | `git diff --check` | exit 0 |

## Steps

### 1. Add semantic memory revisions and displayed-candidate revisions

Add a monotonic `semantic_revision` to canonical memories, starting at 1. Bump
it atomically for changes to body/title, artifact reference/content identity,
scope/type, lifecycle/supersession, fact key, and observation/validity dates.
Pin, salience, and UI-only metadata changes do not change semantic revision.
Use a database trigger or a single enforced mutation path, with tests for every
existing canonical writer including supersede/retract and imports. Do not use
wall-clock timestamps as the sole revision token.

Guard the canonical edit path too. `update_memory` currently reads existing
fields before its write transaction. Require the displayed semantic revision on
edits and compare it under that transaction, returning `memory_changed` on a
mismatch. A stale edit must not restore an old active status after a correction
has superseded the row. Update existing edit callers/rollback behavior within
the scoped API, store, and editor files.

Add `candidate_revision`, expected target semantic revision, and expected target
content digest to candidates. The digest includes canonical semantic fields in
a versioned encoding; compare it in addition to revision. Capture it from the
same snapshot as the target text shown before the model call, not a fresh lookup
after the response arrives. Record whether the target was shown completely.
Model-proposed destructive operations require a complete bounded target; a
truncated/artifact-only preview is insufficient for automatic target selection.

Keep legacy canonical rows intact with revision 1 and nullable new metadata.
Legacy pending targeted candidates with no snapshot become blocked
`target_snapshot_unavailable`; never pretend a newly read revision was reviewed.
Plan 049 already blocks evidence-less legacy candidates. Add columns/indexes in
both fresh schema and idempotent compatibility migrations, preserving foreign
keys, FTS state, provenance, lifecycle links, and trace ranking versions.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_conflicts::revision`
and `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` pass
same-timestamp edits, every semantic writer, metadata-only changes, legacy
targeted candidates, stale canonical edit after supersession, fresh/old databases,
and migration twice. `bun test tests/memory-store-actions.test.ts` passes.

### 2. Gather relevant comparison targets within the single review call

In `memory_conflicts.rs`, build bounded queries from individual eligible source
fragments and stable entities/identifiers in Plan 049's catalog. Reuse Plan 047's
literal-safe lexical query builder and Review visibility/type policy. Union and
deduplicate targets, allocate fairly across source fragments, and preserve the
12-item/12-KiB comparison budget. An optional fact key is a normalized tuple of
scope, entity, and attribute, never a globally unique unscoped string.

Existing memories remain comparison material, not fresh evidence. Each shown
target includes ID, revision, digest, scope/type, relevant dates, and completeness.
The single restricted reviewer may select create/supersede/retract and explain
the relation using independent evidence. Targets not shown to the model cannot
be invented or silently attached after its response.

After that response, perform deterministic DB matching per proposed claim to
identify exact duplicates and possible additional conflicts. This is a local
lookup, not another model call. Fuzzy/token similarity yields only a possible
relationship. Different subjects, dates, or attributes may be separate truths;
approximate matching never proves equivalence and never changes canonical rows.
Keep original reviewed target snapshots unchanged if a later lookup differs.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_conflicts::discovery`
passes an old relevant claim behind at least 20 recent distractors, multiple
subjects in one run, shared words with distinct meanings, scope isolation,
incomplete targets, deterministic budget/tie-breaks, and zero additional provider
invocations. The `memory` regression command passes.

### 3. Make duplicate decisions depend on operation and current validity

Keep an exact hash for reflow duplicates, but include scope type as well as key
and memory type in its versioned identity. Do not conflate candidate deduplication
with an operation's identity. Proposal identity also includes operation, target,
target revision, and supported validity interval so unrelated actions do not
collide under the existing run/hash uniqueness rule.

For create, an exact currently valid canonical duplicate blocks with a stable
code. For supersede, an identical target is a no-op; exclude the target when
checking for an already-existing replacement. For retract, do not reject merely
because the proposal quotes its own target body. A retraction still requires
independent correction evidence and a live matching target snapshot. Expired,
superseded, or retracted rows do not block a new currently valid fact, but remain
available as explicitly historical comparison metadata where needed.

Detect duplicates and contradictory operations within one reviewer response and
across pending reviews. Keep distinct overlapping candidates visible when the
relationship is uncertain; present a possible-conflict warning rather than
automatically deleting either. Recheck deterministic duplicates under approval's
transaction because another candidate may have been approved in the meantime.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_conflicts::duplicates`
passes exact/reflow duplicate create, unchanged supersede, replacement duplicate,
retract quoting target, expired-active prior row, different scopes/types/intervals,
and two pending proposals racing toward the same canonical change.

### 4. Apply corrections with optimistic concurrency at approval

Change approval and edit commands to require `expectedCandidateRevision`. Load
the candidate inside the transaction and reject a stale displayed revision with
`candidate_changed`, leaving it pending for refresh. Increment candidate revision
on every material edit, evidence change, or state transition. Operation and target
remain immutable on candidate edits.
Preserve Plan 049's claim/evidence digest validation.

Inside the same approval transaction, recheck current workflow scope inputs,
target visibility, lifecycle, validity, semantic revision, and content digest.
Compare against the snapshot actually shown. A changed target becomes blocked
`target_changed`; a missing/inactive/moved target keeps a precise stable code.
Never refresh the expected target silently or apply the proposal to a successor.
Write canonical lifecycle changes, evidence, FTS indexes, and candidate decision
atomically. Infrastructure failure rolls back and leaves the candidate pending.

A possible-conflict result from Step 2 may reveal a target that the reviewer did
not see. Show that relationship for inspection, but do not retarget or change the
operation of the pending proposal. The user can reject it and supply a correction
through an explicitly requested later workflow/review or the existing independent
manual memory controls. That manual action keeps manual provenance and cannot
convert the rejected proposal into a model-verified correction. No second
autonomous reviewer call occurs, and uncertainty remains visible.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_conflicts::approval`
passes same-ID body/date edits, deleted/retracted/superseded/moved targets, stale
candidate display, attempted operation/target edits, two approvals, missing evidence,
cross-scope target IDs, transaction rollback, and no silent target substitution.

### 5. Record observation and validity dates without inventing expiry

Add nullable `observed_at` and `valid_from` to canonical memory and proposal
metadata. Keep existing `expires_at` as exclusive validity end; do not add a
second competing end field. Unknown dates remain null. Source observation time
comes from host-recorded provenance. A claimed effective date or expiry needs
an eligible quote that supports it; model-generated dates alone are rejected.

Use RFC3339 timestamps normalized to UTC and enforce ordered intervals. A memory
is currently valid when active, `valid_from` is absent or not after the supplied
clock, and `expires_at` is absent or after that clock. Reuse one clock-aware
helper in retrieval, pins/explicit memory eligibility, deduplication, and approval.
Preserve pre-existing date behavior for legacy null fields. History remains
searchable independent of current validity.

Update the frontend `isMemoryPromptEligible` helper and relevant picker/editor
states to include `validFrom` using an injectable clock, matching the backend
boundaries. Backend checks remain authoritative if the displayed state becomes
stale. Pin storage may retain a future-dated selection; prompt rendering must
exclude it until valid. Do not silently unpin or delete it.

Set `last_confirmed_at` only from a supporting independent observation/user
statement's recorded time. Merely approving a proposal today does not prove the
fact was reconfirmed today. Stable preferences and lessons have no default expiry.
Review reminders, if later needed, are not expiry and belong in separate scope.

Future-effective corrections must not supersede the current fact early. Keep
their proposals pending and return `validity_not_started` until their effective
time, then require ordinary user approval. Do not add a timer, hidden job, or
automatic activation. The UI shows the pending effective date. Recheck all dates
against the approval clock, because a proposal may expire while waiting.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_conflicts::validity`
passes unknown dates, explicit UTC offsets, malformed/inverted intervals,
boundary instants, future correction, expired pending proposal, stable preference,
old observation approved today, and current/history visibility differences.

### 6. Show the correction, evidence, versions, and dates together

Update existing Suggestions and Memory detail controls to show current target,
proposed replacement/retraction, independent evidence, possible conflicts, and
validity dates. Refresh after `candidate_changed`; explain `target_changed` and
require a new explicitly requested proposal or independent manual correction.
Do not auto-retry the old approval.
Keep fuzzy matches labelled possible relationships rather than duplicates known
to be equivalent. A user may explicitly keep separate claims when evidence
supports that decision; never disguise that choice as model confirmation.

Extend fixed-label Plan 045 evaluation with additive correction cases, preserving
the historical baseline. Measure exact duplicate rejection, legitimate separate
claims retained, old-target discovery, stale approvals prevented, temporal
correctness, and unsupported corrections blocked. Do not use the same similarity
function to generate ground-truth labels.

Use isolated synthetic IPC/UI QA for create, supersede, retract, possible
conflict, changed target, changed candidate, missing evidence, and future date.
Reuse existing semantic tokens and state rollback patterns. Follow the applicable
interaction skill; macOS Tauri uses WKWebView and may not expose Chromium tools.

**Verify:** `bun test tests/memory-curation.test.ts tests/memory-model.test.ts tests/memories-inspector.test.ts`
and the quality-report command pass. Finish with `bun run check` and
`git diff --check`, both exit 0.

## Test plan

Create `memory_conflicts::{revision,discovery,duplicates,approval,validity}` tests
against the real in-memory SQLite schema, canonical writers, and FTS. Follow
existing transactional approval tests in `memory_curation.rs`. Inject clocks and
fake restricted review responses; provider calls are forbidden in ordinary tests.
Use controlled interleavings, not timing sleeps, to test stale target/candidate
approval. Include history-deletion/evidence cleanup regression from Plan 049.

## Done criteria

- [x] Context-specific comparison finds the named old-target regression within budget.
- [x] Approximate matches remain suggestions and never prove equivalence or merge rows.
- [x] Exactly one configured review invocation remains the maximum per job.
- [x] Target snapshot revision/digest and displayed candidate revision are required.
- [x] All semantic writers participate in revision updates; metadata-only changes are defined.
- [x] Retract/expired-row/no-op cases obey operation-specific duplicate rules.
- [x] Evidence, versions, lifecycle, FTS, and decision commit in one transaction.
- [x] Explicit supported dates govern validity; no universal expiry or early supersession.
- [x] Fresh/legacy/idempotent migrations preserve canonical history and provenance.
- [x] Focused/evaluation/full gates and synthetic UI QA pass; index status is accurate.

## STOP conditions

Stop if fixing an uncertain relation requires silently merging claims, inventing
evidence, adding another provider call, or weakening scope/approval. Keep it a
visible unresolved suggestion. A future extra adjudication call requires separate
explicit configuration, cost limits, and a later plan; it is not a fallback here.

Stop if target completeness or revision identity cannot be established. Offer
bounded unavailable/changed-target behavior instead of destructive inference.
If future validity would require an automatic scheduler, keep the proposal
pending under the policy above. Continue fake-backed work if Plan 046's live
activation is blocked; do not enable the old reviewer.

Stop on unexplained source drift, an out-of-scope dependency, or the same failing
verification twice after a reasonable fix. Record pre-existing check failures
with command/cause and do not count them as a pass.

## Execution and maintenance

Suggested branch: `advisor/050-memory-corrections-and-validity`. Use an isolated
worktree if another executor owns overlapping files. Preserve unrelated dirty
work and do not reset, stash, overwrite, push, or publish it. Commit logical
units with descriptive messages only within authorized scope.

Review new canonical write paths for semantic revision updates and new date
consumers for the shared clock-aware validity helper. Version content digests and
fact-key normalization explicitly. Recheck source deletion cleanup when adding
correction metadata. Existing scopes, approval, and the distinction between
memory reference data and tool authority remain fixed product contracts.
