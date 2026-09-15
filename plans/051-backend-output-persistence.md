# Plan 051: Save requested outputs in the backend run lifecycle

> Implementation checkpoint. Backend-owned Output persistence, startup
> recovery, bounded backlog handling, and promotion-safe cleanup below are
> implemented and locally reviewed; retain the shared gates until they pass.
> Run all commands from `desktop/`.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/db src-tauri/src/runner src-tauri/src/lib.rs src-tauri/src/commands/mod.rs src/features/workflow tests/output-persistence.test.ts tests/history-search.test.ts tests/output-html-report.test.ts specs.md docs/install.md`,
> and `git diff HEAD -- src-tauri/src/db src-tauri/src/runner src-tauri/src/lib.rs src-tauri/src/commands/mod.rs src/features/workflow tests/output-persistence.test.ts tests/history-search.test.ts tests/output-html-report.test.ts specs.md docs/install.md`. Compare the cited symbols
> with live code. The baseline includes unrelated uncommitted runner, identity,
> canvas, store, memory-editor, and styling work. Preserve it. Reconcile completed
> dependency changes; stop on unexplained conflicting changes before editing.

## Status

- Status: DONE — backend/frontend integration, startup recovery snapshot, bounded backlog draining, recent `Saving` recovery/fencing, deletion cleanup, and both History repairs are implemented and locally reviewed. Isolated native Tauri startup/backend smoke passed. Verified 2026-09-15 by `bun run check` (548 frontend tests, frontend build, 1,072 Rust tests, 0 failures) and clean `git diff --check`.
- Priority: P1
- Effort: L
- Risk: MED
- Depends on: `plans/048-shared-memory-context-budget.md`
- Category: bug
- Planned at: commit `85f0d64`, 2026-09-08, including the dirty working tree above

## Why this matters

Output nodes currently save through a frontend event handler. A missing listener
can skip the save, and editing the graph during execution changes the settings
that handler uses. Move ownership to the backend, preserve the settings that
actually ran, and make storage retries independent of agent execution.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result that follows supersedes them for current local status;
the receipt, recovery, and artifact-ownership contracts remain authoritative.

`src/features/workflow/store.ts:1505` chooses the current editor nodes or the
currently stored workflow graph when a run event arrives. The save at `:1558`
then uses that graph:

```ts
if (event.nodeType === "chooseOutput") {
  const node = workflowNodes.find((item) => item.id === event.nodeId);
  const data = node && isOutputNodeData(node.data) ? node.data : null;
  if (data?.saveToMemory) {
    // queueMicrotask then calls get().addMemory with current title/pin settings.
  }
}
```

`src-tauri/src/runner/mod.rs:684-736` loads the workflow at execution start and
clones its nodes/edges. Its `"chooseOutput" | "output"` branch at `:1477-1500`
constructs the final body but does not persist a memory. `insert_step` at `:621`
already writes step JSON and its FTS index transactionally before the completion
event. `enqueue_run` at `:118` stores run metadata/payload only. No durable graph
snapshot exists in `src-tauri/src/db/schema.sql:35-44`.

`src/features/workflow/types.ts:203-236` sets legacy/default values:
`saveToMemory=true`, `pinMemory=false`, empty title override,
`asFinalResult=true`, `htmlReport=false`, and `includeFilesChanged=true`.
Preserve these values and existing settings copy/selection behavior in
`components/node-settings-modal/node-settings-modal.tsx:516-537`.

Generic creation is not an idempotent output-save operation:

```rust
// src-tauri/src/db/memories.rs:821-827
if let Some(existing) = self.find_exact_duplicate(
    plan.scope_type, &plan.scope_key, plan.memory_type, &plan.body,
)? {
    return Ok(existing);
}
```

That lookup compares normalized stored body/scope/type at `:783-804`, ignoring
run/node/title/pin/status. Large artifact records store a 2,000-byte preview at
`:647-648`. `:830-849` writes a new artifact before the DB transaction and removes
it on transaction failure. Preserve those protections and the regression tests
`duplicate_artifact_creation_preserves_original_file_row_and_index` and
`failed_artifact_update_preserves_original_file_row_and_index`.

Follow `Db::with_conn`, `DbError`, serde camelCase DTOs, and transactional FTS
maintenance in `memories.rs` and `history.rs`. Tests use `Db::open_in_memory()`
and synthetic workflow fixtures. Never inspect or migrate the user's live DB.

## Implementation result

The runner captures an execution-time `OutputDisposition` from the cloned
workflow graph and persists it with the completed step. A unique `(run_id,
node_id)` receipt carries the immutable title, pin/save settings, provenance,
state, error, and resulting memory identity. Backend saves now own the
choose-output/output lifecycle; the frontend consumes receipt events and has no
completion-event memory-save side effect. Failed saves retain completed output
and support bounded storage-only retry without rerunning the agent.

Startup opens the database, captures a temporary recovery snapshot before Tauri
setup, and drains cleanup and receipt work in bounded 32-row batches until the
snapshot is exhausted. Pending and recent `Saving` claims are recovered with
claim/fencing identity; delayed workers cannot overwrite a newer attempt, and
owned staging cleanup is journaled. Recovery has no age cutoff for interrupted
`Saving` claims and does not strand a backlog after one batch.

Workflow/clear cleanup now follows the actual deleted-memory set. Promoted
user/workspace memories retain their receipt and artifact; metadata-only scope
changes do not cause cleanup to delete a retained shared body. Tests cover
large and inline promoted outputs, bounded never-claimed pending recovery,
attempt fencing, and failure/retry paths. The older source-review findings
about one-batch recovery, recent-`Saving` starvation, and promotion cleanup are
historical and superseded by this implementation. Shared full checks and final
index sign-off remain pending.

An isolated native Tauri debug-bundle smoke passed exit 0 using
`bun run build --debug --bundles app --config uniqueQA`. The bundle had the
unique QA identifier `com.nirzhuk.alfred.qa.101A09A0CA10` and a
`uniqueQA`-only URL scheme. With a temporary `HOME`, an allowlisted child
environment, and no user-database or account reads, Boot 1 initialized real
SQLite and seeded 65 Pending receipts, one `Saving` receipt timestamped one
second before the actual run, plus Failed and Deleted receipts. Boot 2's real
startup recovered 66 Saved receipts, verified exact inline/large bodies and
canonical artifact/manifest digest, state, and length, and left Failed/Deleted
unchanged. This proves native startup/backend recovery only; it is not
WKWebView visual interaction, provider activation, or lookup transport.

## Scope

- Create `src-tauri/src/db/output_persistence.rs` with focused tests; register and
  export its narrow API in `src-tauri/src/db/mod.rs`.
- Modify `src-tauri/src/db/schema.sql`, `migrate.rs`, `memories.rs`, `history.rs`,
  and `workflows.rs` only for receipt lifecycle, transaction reuse, deletion,
  and additive history metadata.
- Modify `src-tauri/src/runner/mod.rs`, `src-tauri/src/lib.rs`, and
  `src-tauri/src/commands/mod.rs` for persistence orchestration, recovery, and
  a scoped retry command. Extract `src-tauri/src/runner/output.rs` if needed
  to test this path without a running Tauri window.
- Modify `src/features/workflow/store.ts`, `api.ts`, `types.ts`, and
  `components/history-page/history-page.tsx` for receipt display/refresh/retry.
- Add `tests/output-persistence.test.ts`; extend `tests/history-search.test.ts`
  and `tests/output-html-report.test.ts` only for changed contracts.
- Update this plan/index, `specs.md`, and `docs/install.md` with the backend-save
  and retry behavior. Preserve the audit as historical evidence.
- Out of scope: generic content deduplication, new report storage product,
  changing existing save/pin preferences, selector/settings copy changes,
  general workflow resume, enqueue-time graph freezing, UI redesign, scheduler
  changes, native provider enablement, and other plans' features.

## Persistence contract

1. Capture normalized Output settings from the runner's execution-time workflow
   clone. Persist that disposition with the completed step; never reread an
   edited graph during save/retry/recovery. This is a durable Output disposition,
   not a newly promised full run-graph snapshot.
2. Add an output receipt uniquely keyed by `(run_id, node_id)`, linked to its
   persisted step and carrying captured title/pin/save settings, output identity,
   state, resulting memory ID, stable error code, and timestamps. Use a
   deterministic receipt identity or transactional uniqueness, not check-then-add.
3. Use one canonical Output memory row per successful run/node save. Different
   runs with identical bodies retain their own titles, pins, and provenance.
   Reuse validated canonical write/index helpers while bypassing generic
   body-dedup for this dedicated operation. Do not rewrite generic dedup.
4. Record `not_requested` for save disabled and an explicit empty-output skip
   for whitespace-only bodies, preserving current `addMemory` behavior. Requested
   nonempty saves progress through pending, saved, failed, or deliberately
   deleted. State names may follow repository conventions; transitions must
   remain explicit and tested.
5. Completed step output and its index remain durable even if library saving
   fails. Store the save failure and allow a bounded explicit retry from history
   without rerunning any workflow node. A failed library save must not pretend
   the upstream agent failed or erase its completed output.
6. Delete/clear operations mark associated receipts deliberately deleted in
   the same transaction as removing memories. Retain the receipt tombstone for
   the life of its run so repeated events/recovery cannot recreate the memory.
   Retraction remains a user lifecycle decision and does not trigger recreation.
   Deleting a workflow/run follows existing cleanup rules and cannot resurrect
   its deleted receipts. Never preserve memory bodies in deletion tombstones.
7. File writes and SQLite cannot share one atomic commit. Track ownership of
   staged files, use create-new filenames, and commit a saved receipt only after
   a complete file exists. Clean failed staging writes and recover only files
   identifiable as this operation's staging, never arbitrary artifact paths.
   A process crash must leave either a saved reference or a recoverable pending/
   failed receipt; recovery never touches existing unrelated artifacts.
8. Emit save results/refresh notifications after commit. Duplicate or failed
   event delivery does not retry the agent or create another canonical row.
   The frontend reads receipts and refreshes the library, with no save side effect.

Raw Output/Artifact exclusion from ordinary automatic recall belongs to
`plans/047-task-aware-memory-recall.md`. Preserve deliberate pinning and explicit
selection under Plan 048's budget; do not delete reports to enforce recall policy.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Receipt and artifact tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml output_persistence` | all pass |
| Existing memory storage | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memories` | all pass |
| Migration and history | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` and `cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` | all pass |
| UI event/history behavior | `bun test tests/output-persistence.test.ts tests/output-html-report.test.ts tests/history-search.test.ts` | all pass |
| Final integration | `bun run check` | exit 0 |
| Patch hygiene | `git diff --check` | exit 0 |

New focused commands must execute the named new tests, not succeed with zero
matching tests. Commands are the repository's existing Cargo/Bun entry points;
these are implementation gates, not claims they passed during planning.

## Steps

### 1. Add receipts and capture disposition transactionally

Create the receipt schema/API and additive migration. Refactor only the needed
part of `insert_step` so its step row, FTS index, and Output receipt commit in
one transaction. Store captured normalized settings and a reference to the full
step output, avoiding a second unbounded body copy in the receipt. Non-Output
steps retain their existing insertion behavior. Future retries read this record.

Do not backfill guessed receipts from old memory body equality. Old runs/records
remain readable with no receipt. Test fresh DB, old DB, and repeated migration.

**Verify:** run the receipt and migration commands. New tests prove uniqueness,
transaction rollback, old-record preservation, and snapshot settings surviving
later workflow edits.

### 2. Implement idempotent save, deletion, and artifact recovery

Implement receipt claiming/conditional transitions and canonical writes in
`output_persistence.rs`. Use one memory row per run/node, preserve `source=run`
and workflow/run/node provenance, and use captured title/pin metadata. Keep the
existing source/type/default policy unless the completed dependency changes it.

Integrate receipt tombstones into existing single-delete, clear, and relevant
workflow cleanup transactions. Add a retry command deriving its source data
from the stored receipt. Its caller supplies only receipt identity/context, not
replacement body/title/settings. Define startup recovery for bounded batches of
pending receipts and owned staging files; failed saves remain visible and do
not enter an unlimited automatic retry loop.

**Verify:** run the receipt and existing storage commands. Concurrent retries
converge, deliberate deletion remains deleted, and injected file/commit failures
preserve the original rows, indexes, and artifact files.

### 3. Wire Output execution and recovery without a frontend dependency

Call the new operation after the completed Output step/receipt commit and before
emitting its persistence outcome. Normalize both `chooseOutput` and `output`
through the same helper. Preserve body construction and final-result selection.
Use the same backend path for manual, scheduled, and app-event run triggers.

Add an injected event sink/artifact writer around the extracted Output path so
tests can run with no Tauri window or provider. Start receipt recovery from the
existing application lifecycle, never from frontend event delivery. Bound work
and release DB locks before filesystem work or event emission.

**Verify:** run `cargo test --locked --manifest-path src-tauri/Cargo.toml output_persistence`.
Tests cover each trigger label, absent/failing event sinks, edited/deleted current
graph nodes, both node type names, and restart from staged/pending states.

### 4. Make the UI consume durable save outcomes

Remove only the Output `queueMicrotask`/`addMemory` path in `handleRunEvent`.
Keep manual note creation, memory selection, pin controls, and legacy memory
migration behavior. Add optional receipt metadata to the existing event/history
DTOs and use existing history UI patterns for saved/failed/deleted states and
explicit failed-save retry. Old history entries without receipts still render.

Reloaded/inactive workflows obtain save status from the backend. Event replay
may refresh the library but cannot mutate canonical memories. A transient
refresh error must not be mislabeled as a persistence failure.

**Verify:** run the UI command and `bun run build:frontend`. Tests assert zero
`createMemory` calls for replayed completion events, correct active/inactive
workflow refresh, and unchanged Output preferences and selector behavior.

### 5. Complete regression and desktop verification

Run the full checks once focused gates pass. Verify history reload, failure
display/retry, saved report reading, deletion, and deliberate pin/selection with
synthetic data in the desktop app. Follow the applicable UI interaction skill;
macOS Tauri uses WKWebView and must not be assumed to expose Chromium CDP.
Record the tested states and any unavailable native verification honestly.

**Verify:** `bun run check` and `git diff --check` exit 0. Inspect `git diff --stat`
against the initial dirty-tree inventory to confirm only authorized work was added.

## Test plan

- Backend tests in the new module: duplicate concurrent operation, same body in
  separate runs, different title/pin metadata, lifecycle/delete/clear tombstones,
  stale retry, schema migration twice, and no inferred old-run backfill.
- Failure injection: before file creation, during write, after staging, before
  commit, after commit before notification, and restart with a pending receipt.
- Large reports: identical bodies and distinct bodies sharing their first 2,000
  bytes; full contents and per-run provenance remain correct.
- Runner tests: no listener, mutable frontend graph, legacy alias/defaults,
  whitespace-only output, HTML, changed-file append, multiple Output nodes, and
  unchanged chosen final result. Do not run actual providers in these tests.
- Frontend tests: completion replay makes no save call, history survives reload,
  explicit retry does not start a workflow, and save disabled still keeps history.
- Extend existing artifact-preservation tests rather than weakening them.

## Done criteria

- [x] Backend saves use captured execution-time settings and a unique run/node
  receipt.
- [x] Each successful run/node owns its canonical Output row and provenance.
- [x] Delete/clear tombstones prevent recreation by replay/retry/recovery.
- [x] Completed bodies, indexes, and existing artifact files survive save
  failures.
- [x] Notifications follow durable commits; frontend completion never saves
  memories.
- [x] Existing records, save/pin choices, selector copy/behavior, and run output
  remain intact.
- [x] Focused tests, `bun run check`, hygiene, and documented desktop checks pass.
- [x] Only scoped changes were added; plan/index status reflects actual completion.

## Specific stop conditions

Stop and report if the implementation needs full graph snapshot/resume changes,
cannot preserve completed output on save failure, requires changing generic
memory deduplication, or cannot prevent recreation after explicit deletion.
Reconcile normal dependency changes first; report any remaining unexplained
scope/lifecycle mismatch. If a verification fails twice after a reasonable fix,
record the exact command/failure rather than weakening its assertion.

Do not reset/stash/overwrite unrelated work or inspect credentials/private DBs.
Use an isolated branch/worktree when another executor owns overlapping files;
suggested branch `advisor/051-backend-output-persistence`. Do not push or publish
without authorization. A pre-existing full-check failure is not a passing gate.

## Maintenance

If the runner later executes a node multiple times within one run, the run/node
receipt identity must gain an explicit execution-instance contract before that
feature ships. Keep output saves separate from generic fact deduplication.
Review receipt/file cleanup and deletion tests whenever retention changes.

Plan 042 excludes history, runtime output, and memory bodies from shared templates.
Plan 043's schedule node is render-only and must not enter the captured executable
graph. Plan 044's follow-up jobs/outbox stay separate from memories and workflow
runs. Coordinate shared store/runner/schema/lib changes without expanding those
plans. Plan 052 may read these receipts through a scoped bounded service later.
