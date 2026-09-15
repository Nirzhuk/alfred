# Plan 049: Require traceable evidence for every memory proposal

> Implementation checkpoint. Structured evidence capture, validation, retention,
> deletion, and review UI work below are implemented and locally reviewed; keep
> the shared final gates pending until they pass. Run all commands from `desktop/`.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/db src-tauri/src/runner src-tauri/src/commands/mod.rs src/features/workflow src/features/settings/memory-review.ts tests/memory-curation.test.ts tests/memories-inspector.test.ts tests/history-search.test.ts specs.md docs/install.md`,
> and `git diff HEAD -- src-tauri/src/db src-tauri/src/runner src-tauri/src/commands/mod.rs src/features/workflow src/features/settings/memory-review.ts tests/memory-curation.test.ts tests/memories-inspector.test.ts tests/history-search.test.ts specs.md docs/install.md`. Read the cited live symbols.
> The baseline contains unrelated dirty runner, identity, memory-editor, store,
> canvas, and styling work. Preserve it. Reconcile completed Plans 046 and 048;
> report unexplained conflicting changes before editing.

## Status

- Status: DONE. Final backend high review accepted step-linked source deletion, claim-category enforcement, canonical qualification, and restored reference-only prior-memory capture. Both-theme focused UI passed with Argent and a dedicated CUA tab. Verified 2026-09-15 by `bun run check` (548 frontend tests, frontend build, 1,072 Rust tests, 0 failures) and clean `git diff --check`.
- Effort: L
- Risk: MED
- Depends on: the contract/conformance milestone of `plans/046-restricted-memory-curator.md` and
  `plans/048-shared-memory-context-budget.md`
- Category: correctness / migration
- Planned at: commit `85f0d64`, 2026-09-08, with the existing dirty working tree

## Why this matters

Review currently receives formatted prompts containing injected memory, then
stores suggestions without a supporting source. An agent can repeat an old
assertion and make it look newly learned. Persist original sources separately,
require verifiable evidence references, and show that evidence during approval.
Approved provenance must survive clearing the suggestions queue while respecting
deliberate deletion of the source history.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result that follows supersedes them for current local status;
the source/evidence invariants remain the governing contract.

`src-tauri/src/runner/mod.rs:1178` and line 1392 assign the fully composed prompt
to `persisted_input_prompt`; line 1901 persists:

```rust
let input = serde_json::json!({ "prompt": persisted_input_prompt });
```

`src-tauri/src/db/memory_curation.rs:1129` prioritizes final output, then line 1150
flattens step input strings. The prompt leaf is treated as high-priority input
even when it includes recalled memories. `ReviewerSuggestionRaw` at line 1308
contains operation, target, scope/type, title/body, confidence, and rationale.
It has no evidence field. `validate_candidate_suggestion` sets
`source_node_id: None` at line 1531.

`clear_decided_memory_candidates` at line 588 deletes approved candidate rows
while preserving canonical memory. `schema.sql:300` cascades review deletion
with run deletion; line 316 cascades candidate deletion with its review.
`src-tauri/src/db/workflows.rs:251` deletes run steps and runs when deleting a
workflow. Evidence attached only to the candidate cannot be durable provenance.

The existing parser denies unknown fields and makes no repair call. Approval
revalidates and writes canonical memory plus indexes in one transaction. Keep
those properties, camelCase DTOs, `DbError`, and the existing user/workspace/workflow
scope rules. Review remains off by default and never applies its own proposals.

Plan 048 introduces structured authored/context/memory segments before final
prompt rendering. Consume those structures directly. This plan must not recover
origin by splitting Markdown headings, delimiters, or saved formatted prompts.
Plan 046 supplies the only allowed review execution contract and its limits;
this plan adds no provider or model invocation.
Its live-activation blocker does not prevent this plan's implementation or
verification with injected restricted reviewer responses. Completing this plan
does not imply that automated production review is available.

## Implementation result

The runner now captures original authored and observed source segments before
prompt rendering and binds them to the generated step identity. Review receives
structured fragments rather than parsing rendered prompts; prior memory remains
reference-only comparison material. Candidate validation enforces claim
categories, exact source membership/quotes/digests, canonical qualification, and
the evidence rules for inference, proposal, edit, approval, and deletion.

Approved evidence is copied to canonical provenance independently of the
disposable candidate queue. Source-history deletion scrubs copied text and
digests while preserving the required bounded tombstone state; legacy pending
proposals remain blocked instead of receiving invented evidence. Reference-only
prior-memory capture was restored at composition boundaries. Final backend high
review and both-theme focused UI checks are accepted; the final 2026-09-15 `bun
run check` passed 548 frontend tests, the frontend build, and 1,072 Rust tests
with 0 failures, superseding the earlier 1,068-pass/3-fail checkpoint; no
assertion was weakened, and the index records DONE. Plan 046's
live-activation blocker does not prevent this fake-backed offline implementation.

## Scope

- Create `src-tauri/src/db/memory_evidence.rs`; register it in
  `src-tauri/src/db/mod.rs`.
- `src-tauri/src/db/memory_curation.rs`, `src-tauri/src/db/history.rs`,
  `src-tauri/src/db/memories.rs`, `src-tauri/src/db/workflows.rs`,
  `src-tauri/src/db/schema.sql`, and `src-tauri/src/db/migrate.rs`.
- `src-tauri/src/runner/mod.rs` and `src-tauri/src/runner/memory_context.rs`,
  Plan 048's `PromptContext`/`PromptPart`/origin structures, only source capture
  and review assembly. If that contract does not exist, stop before integration.
- `src-tauri/src/commands/mod.rs`, memory/history DTO plumbing only.
- `src/features/workflow/types.ts`, `src/features/workflow/api.ts`,
  `src/features/workflow/components/memories-inspector/memories-inspector.tsx`,
  `suggestions-model.ts`, and `memory-workspace.css` in that same directory.
- `src/features/settings/memory-review.ts`, `tests/memory-curation.test.ts`,
  `tests/memories-inspector.test.ts`, and `tests/history-search.test.ts`.
- This plan/index and memory provenance/deletion wording in `specs.md` and
  `docs/install.md`.

Out of scope: parsing legacy prompts into trusted sources; automatic approval;
semantic retrieval; target revision changes assigned to Plan 050; new scopes;
cloud storage; provider fallback; private transcript fixtures; a second review
or adjudication model call; changes to ordinary workflow agent permissions.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Evidence tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_evidence` | all pass; matching tests exist |
| Review/storage regression | `cargo test --locked --manifest-path src-tauri/Cargo.toml memory` | all pass |
| Upgrade tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` | all pass |
| History/deletion | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` | all pass |
| Frontend | `bun test tests/memory-curation.test.ts tests/memories-inspector.test.ts tests/history-search.test.ts` | all pass |
| Integration | `bun run check` | exit 0 |
| Hygiene | `git diff --check` | exit 0 |

## Steps

### 1. Define source, evidence, and normalization contracts

In `memory_evidence.rs`, define immutable host-assigned source records with source
ID, run ID, optional step/node ID, origin, producer identity when available,
observation time, normalization version, bounded text, truncation state, and
digest. Origins distinguish authored task, configured workflow instruction,
external trigger data, host-recorded observation, model output, and prior memory.
Never let the reviewer promote external text or model output to a user statement.

Encode evidence eligibility in a host-owned table, not a reviewer-supplied flag:
authored user statements and directly recorded external/host observations can
support claims about what they actually state or observe. Configured workflow
instructions can support a claim about that workflow's configuration only;
they do not prove a personal preference or an external fact. External text keeps
its producer/source label and cannot become a user instruction. Model output,
prior memory, and retrieved historical summaries are comparison material and
cannot be the sole independent support. A host receipt can verify that an action
completed, but does not establish that a model-generated report is true.

Use one versioned normalization function for storage, reviewer display, quote
validation, and UI. Normalize line endings and prohibited controls once; keep
exact resulting text stable. Each shown source fragment has its own ID and
digest, plus a reference to its parent source. A citation contains source ID,
exact quote, and UTF-8 byte start/end in that shown fragment. Validate against
that exact fragment, never a freshly reformatted source.

Require one to three evidence citations per candidate, each at most 1 KiB, within
the review and candidate aggregate byte limits. The envelope carries a schema
version. Add a host-computed claim digest binding operation, target, scope/type,
title/body, and selected evidence to the validation result. Model confidence
stays an uncalibrated assessment, never proof or a truth probability.

Set bounded source capture at 16 KiB per source and 128 KiB per run, with explicit
omission/truncation markers. Use the stricter Plan 046 limit when building the
review request. Never silently make omitted bytes citeable. Original source
capture is separate from prompt composition and memory injection.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_evidence::contract`
passes origin immutability, deterministic normalization, UTF-8 offsets, exact
quotes, digests, schema version, byte limits, and truncation tests.

### 2. Add storage and honest legacy handling

Add normalized tables for run evidence sources, candidate evidence, and canonical
memory evidence. Canonical evidence belongs to a canonical memory ID independently
of the disposable candidate row. Add curation contract version and evidence state
to candidates. Keep source IDs and normalization/digest metadata host-generated.

Implement additive migrations in `migrate.rs` and align `schema.sql`. Base schema
executes before compatibility migrations, so create new indexes on added columns
only after those columns exist. Preserve canonical IDs, scopes, bodies, artifact
paths, links, run history, review settings, and existing decisions. Migration must
be idempotent and transactional where multiple changes depend on one another.

Mark historical canonical memories as legacy/unknown provenance without changing
their content. Existing pending candidates become blocked with
`legacy_evidence_unavailable`; do not synthesize quotes from formatted history.
They remain inspectable/rejectable until explicit queue cleanup. To propose a
new claim, run a fresh source-recorded workflow. A legacy candidate cannot be
converted into an evidence-verified candidate through an arbitrary edit or a
"manual" flag. Existing independent manual-memory creation remains separately
labelled manual and never gains fabricated run evidence.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate`
passes fresh database, pre-curation database, current legacy candidates, repeated
migration, retained IDs/artifacts/links, and rollback-on-failure cases.

### 3. Capture original sources before rendering prompts

In the runner, persist authored task and configured instruction segments from
Plan 048 before adding memories, identity headers, skills, or upstream model
outputs. Capture trigger fields separately with external-data provenance. Record
the actual origin of upstream segments; a prior agent's answer is model output.
Persist host/tool observations only when the runner has an actual recorded result,
not because an agent says that a command or external action succeeded.

Reuse step/run identifiers and timestamps. A copied prior memory records its
memory ID and version/reference as background, never independent confirmation.
If an execution route exposes no recorded observations, the evidence catalog
has no observed-result entries for it. Do not broaden that route to capture raw
credentials or entire tool streams. Apply the existing redaction/screening policy
before storage and before outbound review; reject credential-like source content
from evidence capture and expose a bounded omission reason.

Keep existing formatted history readable for compatibility, but review only the
new structured source catalog. Extend `get_run_history` with source metadata for
the approval UI. Do not silently classify old histories as newly recorded sources.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_evidence::capture`
and `cargo test --locked --manifest-path src-tauri/Cargo.toml memory` pass
synthetic authored/trigger/upstream/observation, unavailable-observation, redaction,
and circular-memory fixtures. The captured authored text contains no injected
memory text unless that text was independently present in the original source.

### 4. Require evidence in parsing, validation, and claim edits

Replace flattened digest assembly with a bounded catalog of exact shown source
fragments. Keep prior memories in a distinct comparison section. Update
`ReviewerSuggestionRaw`, `ValidatedCandidate`, and `build_review_prompt` to require
citations. The single reviewer call may return zero candidates. Invalid candidates
are rejected without model repair or another call.

Validate source membership in this run/review, origin, quote bytes, ranges,
digest/version, and evidence availability. Derive `source_node_id` from validated
sources; represent multi-source provenance explicitly rather than guessing one.
At least one citation must be eligible independent evidence. Prior memory and
model assertions alone cannot create, confirm, supersede, or retract a claim.
An inference can be proposed only when it cites independent evidence and is
visibly labelled inference. Exact quotations prove source linkage, not entailment.

Operation and target remain immutable on candidate edits. User changes to claim
title/body, scope, type, or evidence invalidate the stored claim/evidence validation
digest. The edit flow requires selecting or
reconfirming eligible existing source fragments and passes the entire proposal
through the same validator. Do not permit an edit to rewrite the source, invent
an origin, suppress the inference label, or retain an old validation marker.
Recheck the current claim digest and evidence inside approval's transaction.
Human approval judges whether the cited evidence supports the edited assertion;
do not claim that quote matching automates that semantic judgment.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_evidence::validation`
passes fabricated-ID, wrong-run, changed-quote, stale-digest, omitted/truncated span,
ineligible-origin, circular-evidence, legitimate inference, and edited-claim cases.
`cargo test --locked --manifest-path src-tauri/Cargo.toml memory` remains green.

### 5. Make approval and deletion preserve the right data

In `apply_approved_candidate`, copy bounded evidence and origin metadata to
canonical memory evidence in the same transaction as the canonical change,
index maintenance, and candidate decision. Clearing decided suggestions deletes
queue evidence but leaves canonical provenance intact. Infrastructure failure
rolls back everything and leaves the proposal pending.

Deliberate source-history deletion has different semantics. Before deleting a
source/run/step, remove its copied quotations, source text, content digests, and
source-derived free-text rationale from candidate and canonical provenance.
Keep only a minimal tombstone with `source_removed`, origin category, and safe
identity metadata where needed. Block pending candidates that lose required
evidence. Preserve separately approved canonical claim bodies according to the
existing memory-deletion policy; history deletion must not secretly retain the
deleted transcript inside provenance. Explain this distinction in data UI copy.

Centralize cleanup in `memory_evidence.rs` and call it from workflow/history
deletion transactions. Add database delete triggers or equivalent enforced
constraints so deleting a source/run directly cannot leave copied provenance
behind. Run deletion already cascades review/candidate rows; retained canonical
evidence must be scrubbed before that cascade. Explicit memory deletion removes
its own evidence. Source deletion during approval must either win and block
approval, or run afterward and scrub the approved evidence.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_evidence::retention`
passes queue cleanup, source/run/step/workflow deletion, direct SQL deletion,
memory deletion, approval rollback, and deletion/approval interleaving cases.
`cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` passes.

### 6. Put supporting evidence beside approval

Extend existing DTOs and Suggestions/Memory detail views with source origin,
quote, originating run/step link, inference status, and evidence unavailable or
source removed states. Show supporting evidence before the approval controls.
Label model confidence as a model assessment or remove the percentage from
this decision view. Never style it as verified accuracy.

Update candidate edit controls to require evidence reconfirmation after a claim
change. Display stable backend rejection codes and roll back optimistic changes
on failure. Preserve existing scopes, approval confirmations, disabled states,
and semantic design tokens. Use synthetic IPC data for UI verification; record
normal, multi-source, legacy-blocked, edited, and source-removed states. Follow
the applicable interaction skill; native macOS Tauri uses WKWebView.

**Verify:** `bun test tests/memory-curation.test.ts tests/memories-inspector.test.ts tests/history-search.test.ts`,
then `bun run check` and `git diff --check`, all exit 0.

## Test plan

Create `memory_evidence::{contract,capture,validation,retention}` tests using
`Db::open_in_memory()` and synthetic canonical rows. Use existing
`memory_curation.rs` approval tests and `tests/memory-curation.test.ts` as patterns.
No provider calls are necessary: fake reviewer responses exercise the complete
parser, evidence validator, proposal storage, approval, and deletion path.
Test invalid evidence independently from semantic support; neither fake outputs
nor quote matching establishes real model extraction accuracy.

## Done criteria

- [x] Original authored/trigger/observation data has host-recorded provenance.
- [x] Review reads structured sources without parsing formatted prompts.
- [x] All proposals require exact validated evidence and a current claim digest.
- [x] Prior memory/model assertions alone never confirm a claim.
- [x] Claim edits invalidate linkage and pass the same evidence validator.
- [x] Approved provenance survives candidate cleanup independently of the queue.
- [x] Deliberate source deletion scrubs copied text/digests and marks source
  removed.
- [x] Legacy pending proposals cannot bypass evidence requirements.
- [x] Fresh/legacy/idempotent migrations and deletion races are tested.
- [x] Focused tests, full check, patch hygiene, and isolated UI verification pass.
- [x] Only scoped files changed and the index records the actual result.

## STOP conditions

Stop before source integration if Plan 048 has no structured segment contract.
Never substitute parsing of rendered prompts. Restricted review may stay blocked
for live activation under Plan 046; continue fake-backed evidence work, but do
not reactivate the ordinary coding reviewer to obtain test output.

Stop if a proposed evidence source exists only in a model assertion, if history
deletion would retain copied transcript content, or if a legacy proposal needs
invented evidence to become approvable. Report missing observation support as a
bounded limitation. Stop on unexplained source drift, an out-of-scope dependency,
or a gate failing twice after a reasonable fix. Record pre-existing full-check
failures with exact command/cause; do not mark them passed.

## Execution and maintenance

Use an isolated branch/worktree when overlapping files have another owner.
Suggested branch: `advisor/049-evidence-backed-memory-curation`. Preserve unrelated
dirty work; do not reset, stash, overwrite, push, or publish it. Commit logical
units with descriptive messages only within the authorized scope.

Version normalization whenever its byte transformation changes. Preserve old
normalized fragments for validation until their source is deliberately deleted;
never silently recompute old quote offsets against new formatting. Audit every
new history deletion route for evidence cleanup. Plan 050 adds version-checked
corrections and temporal policy; this plan must leave those fields extensible
without claiming semantic certainty. No credential value, private transcript,
or production memory belongs in tests, logs, or plan documents.
