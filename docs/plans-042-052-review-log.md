# Review log for plans 042 and 045–052

Reviews compare this work against the source tree saved at
`/tmp/alfred-plans-baseline.kKbPiV`, including the user's earlier edits.

## First high-effort pass

Reviewer: GPT-6 Astra, high reasoning effort. Read-only review.
These findings describe rejected intermediate drafts, not accepted behavior.

### 042

- Worker default storage was recreated per request; D1 mapping was incorrect.
- Rust and TypeScript canonical bytes produced incompatible content digests.
- Import was non-atomic, ignored provenance/starts, and lacked durable retry identity.
- Setup values lacked typed validation and recipient capability checks.
- Validators did not enforce equivalent strict contracts.
- Export silently omitted semantic fields and lacked descriptor portability rules.
- Review dialogs hid executable content and did not wait for editor persistence.
- Publication ownership and request recovery were not durably recorded.
- Input/response streaming limits, concurrent quotas, and rate limits were absent.
- Preview copying, installer/support destinations, and deep-link integration were incomplete.

### 045

- The first fixture omitted required difficult cases and misleadingly reported no recall misses.
- Typed sources and layer expectations were declarations without per-case data.
- Fixed time did not reach pin/explicit renderers.
- Safety counters conflated isolation, relevance, and byte-budget failures.
- No-memory/recall-off modes and renderer coverage were too shallow.
- Frozen-baseline comparison needed a versioned active-policy path for later changes.
- Fake reviewer tests did not exercise acceptance, rejection, or circular evidence.

### 046

- Descendants retaining pipes could bypass deadlines and cleanup.
- Capability discovery could block while holding the database mutex.
- Queue overflow could not drain or retry; restart recovery was missing.
- Fake lifecycle tests depended on the installed real CLI.
- Cached capability state could miss executable drift before launch.
- Legacy unfinished jobs lacked an unavailable-status migration.
- Transport claims needed actual subprocess fixtures rather than decoder-only tests.
- Tool-event detection was late and did not cover nested events.
- Mutable requests could evade configured hard limits.
- Global availability UI and disclosure of existing-memory input were incomplete.

## Second high-effort pass: 046

The revised draft still needs startup wiring, a durable dispatcher that drains
all pending jobs without duplicate reservations or lost wakeups, and the exact
32-waiting/one-active bound. Production enqueue must obtain actual capability
identity rather than write the fixture's runtime version. Unsupported platforms
must remain unavailable unless process-tree cleanup is proven. Two subprocess
fixtures were vacuous, and the privacy acknowledgement still omitted existing
memory context. The same implementation owner is correcting these findings.

Resolved in this pass: Unix process-group cleanup, probing outside the database
lock, legacy-job migration, streamed nested tool rejection, hard request caps,
and dropping the output receiver before joining on failure. Ordinary adapter
fallback remains absent. Seventeen frontend curation tests passed; Rust checks
were blocked by an in-progress sharing command serialization error. No Rust
conformance pass is claimed from that review.

## Second high-effort pass: 045

The reviewer accepted the stronger fixture coverage and honest metrics, but
found four remaining evaluator defects. Renderer lifecycle checks counted missing
required IDs without detecting forbidden included IDs. Two tests still assumed
the current policy must retain v1's misses. The later-policy comparison did not
verify the complete frozen fixture content. The unsupported-use counter measured
seeded types instead of actual returned items. The owner is correcting these
before the coordinator accepts the freeze. The split will also include a
non-identifier natural-language case in lexical validation.

## Third high-effort pass: 046

The reviewer accepted startup wiring, exact queue capacity, discovered production
identity, executable-drift checks, unsupported-platform gating, subprocess
fixtures, and disclosure. One dispatcher defect remains: registering an immediate
callback while the reentrancy guard is held consumes the wakeup. The owner must
test actual durable dispatch of more than 32 supported and unsupported jobs,
completion during registration, and duplicate reservations. Permit-count tests
alone do not establish eventual processing.

Claude updated during the session to 2.1.265. Live activation remains unavailable;
the reviewed 2.1.263 fixture does not authorize a newer executable.

## Integration checks during implementation

`bun run check` failed at Rust compilation on `commands/sharing.rs:59` because
`SharedTemplate` did not satisfy the Tauri response serialization bound. Log:
`/tmp/alfred-plans-integration-01.log`. The sharing owner is fixing this new error.

The next full check compiled successfully, passed 501 frontend tests and the
frontend build, then reported 842 passing and 24 failing Rust tests. Twenty-three
failures concern subprocess startup/runtime fixtures; one is the new review
queue bound. Root is diagnosing the shared subprocess failures and the 046 owner
is fixing the queue test. Log: `/tmp/alfred-plans-integration-02.log`.

Runtime diagnosis: a focused startup case passed, all 21 managed-runtime tests
passed, and a subsequent run excluding the evaluator passed 861 tests. The next
full Rust repeat passed 866 tests and failed one existing OpenCode fake logout
test with `opencode_native_package_unverified`; that exact test passed alone.
No unrelated production runtime code was changed. Builds were changing between
runs, so these results do not establish a single cause for the intermittent
startup failures. Final checks must run against stable source.

Before freezing 045, the coordinator also required supported lexical/identifier
cases in both development and validation splits. The first revised split put
all supported cohort cases in development, making the validation gate empty.

## Resolution

The final bounded 045 review accepted all four substantive corrections and
requested one final historical assertion move. Root applied that move and all
six evaluator tests passed. The accepted fixture and report hashes are recorded
in the execution ledger. No further v1 regeneration is permitted.

042's next report still deferred UI entry points, the deep-link queue, real
recipient capability checks, and packaged evidence. Root rejected that completion
claim and returned the required integrations to the same dedicated agent.

The implementation owners received the findings. Remaining plans retain their
review gates. Live provider activation requires isolation proof;
passing offline fixtures does not supply that proof.

The final bounded 046 review accepted the dispatcher after the implementation
preserved wake requests arriving during an active drain, excluded reserved rows
from rearming, and stopped on classification-write failures. Tests now release
real queue permits while the dispatcher is active and verify new durable work
drains without an external manual retry. Contract/conformance is accepted;
the shared final check and live activation proof remain separate gates.

The first high-effort 047 review accepted eligibility, literal lexical matching,
scope chunking, ranking migration, History status, and frozen evaluation gates.
It found two query-source defects: twenty authored terms could crowd out every
normalized event term, and repeated template expansion could grow query-state
fragments without bound before final truncation. The same 047 owner is fixing
those issues while 048 works on separate memory composition and database seams.

The bounded 047 re-review accepted both corrections: retained query state is
bounded during construction/expansion, and bounded event identifiers precede
authored terms. An actual DB test recovers an event-only identifier after a
25-term authored task. The original frozen reports and labels remain unchanged.

042's second review still rejected local conformance. Confirmed defects include
service rejection of exported accountRef bindings; validation of unresolved
resource inputs; resource-ID lookup through name search; publisher-local start
paths/filters remaining in exports; inconsistent start target/type validation;
duplicate generated requirements; folder arrays where a string is required;
resource choices bound to the wrong account; weaker capability/expiry checks
than execution; URL-alias and restart import duplication; and incomplete reusable
draft editing/web requirements preview. Root also added real DB transaction tests:
the initial eight ran seven passes and one failure showing completion accepts a
digest different from the durable publication request. The changed-identity case
was then split into its own independent regression test.

042 Batch One now passes all nine coordinator-authored DB tests, including both
publication identity regressions. Local conformance remains unaccepted pending
Batch Two, strict resolved-input validation, and tests of the production export
and setup helpers. A `commands::sharing` test run initially matched zero tests;
contract fixtures were not counted as coverage of those missing paths.

Luna's actual-browser 046 testing found a React event lifetime crash and, after
that was fixed, a failed-save draft rollback defect. Root fixed both and the
related inability to edit already-enabled settings. The high reviewer accepted
the scoped settings changes and independently passed twenty focused tests.
Luna subsequently confirmed failed-Save rollback in both themes and the supported Save paths; evidence is in docs/plans-046-ui-results.md.

During 048 implementation, root identified category header bytes omitted from
later category admissions, Input script output incorrectly labeled authored,
a DB failure path that retried the failing query and panicked, and malformed
artifact UTF-8 being silently labeled complete. These were sent to the dedicated
owner before its stable high-review handback.

The first high-effort 048 review rejected core conformance. Actual runner
branches still rendered full artifacts for initial step logging and Memory/
Template results before bounded dispatch. Output's file-change path could flatten
memory into GeneratedOutput and bypass the shared budget downstream. Entirely
unavailable selections vanished before accounting and retained stale last output.
Connected Apps swallowed projection failures and could execute with empty payloads.
The reviewer requires actual branch-flow tests with injected boundaries and
instrumented artifact reads. Direct calls to prepare_agent_prompt do not prove
the Agent/CustomAgent execution branches. The direct composition evaluator's
60 passing cases are useful but remain a draft until those runner defects pass.

042's third high review rejected ordinary CLI setup, target-kind validation
bypasses, canonical URL alias handling, and missing production export/setup
journey coverage. Root took the UI findings: canonical provider IDs, secondary
grouped targets, independent resource response ownership, and retain-example
state. Ten focused model tests pass; actual component browser QA is pending.

The 048 re-review accepted core corrections. Both real Agent branches use the
tested dispatch seam; Memory/Template logging avoids full reads; typed Output
references survive reentry; unavailable selections reach omission accounting;
and failed utility projection prevents action execution. The reviewer
independently passed eleven memory-runner tests and the action-failure test.
Root accepted and froze the final composition report after eight evaluation
tests passed.

042 frontend corrections passed high review. Backend review still found actual
app-event descriptorVersion round-trip failure, metadata-controlled resource
validation, skipped secondary targets, and a journey bypassing commit checks.
Root now owns those corrections and has nine passing command-path tests.
Browser sharing QA is running under Luna at extra-high effort.

The final 042 backend review accepted local conformance after the command-used
publisher preparation and checked import passed the two-recipient journey.
Saved event configs round-trip with current descriptor versions; descriptor-led
resource checks cover secondary targets and bypass picker caches. Ten sharing
command tests passed independently. The subsequent checkbox/error-scroll fixes
also passed scoped code review. Live service and packaged OS gates remain separate.

The 042 public-preview follow-up passed scoped high review: metadata contains
escaped public title/description only, mobile guidance retains the copy fallback,
and the loopback fixture serves the actual handler with synthetic data. The
reviewer independently passed fourteen service tests with forty-six assertions.
This code review does not substitute for the pending browser preview audit.

The independent 051 receipt/storage checkpoint is not yet accepted. Two findings
need correction before integration: (P1) recovery can replace a saving claim while
the old worker still holds the same storage identity; save/failure transitions
match only state and storage ID, allowing that worker to finish the replacement
claim. Use distinct claim ownership and prevent stale filesystem cleanup from
damaging a newer save; test recovery/reclaim with delayed old-worker completion.
(P2) the schema's state CHECK uses nullable hash/memory comparisons, so SQLite
accepts a saved row with NULL hash and memory ID. An isolated SQLite reproduction
against the actual schema confirmed this. Require explicit non-NULL values and
test invalid inserts plus memory deletion without a prior tombstone.

The reviewer's 051 test run did not execute tests: concurrent 049 integration
caused E0063 in memory_curation.rs (2016, 2033, 2208, 2224) and runner/mod.rs
(3080), from missing new evidence fields. Log:
/tmp/alfred-051-review-checkpoint.log. These are not attributed to 051. Shared
runner/schema/deletion hooks and feature-level acceptance remain outside this
independent checkpoint. No frozen memory fixture was modified.

The 051 independent checkpoint re-review accepted both corrections. Every save
attempt now has a unique fencing identity carried through recovery, success,
failure, and attempt-specific staging. Delayed workers cannot finish a replacement
claim or obtain final-file deletion authority while that replacement is live or
saved. Typed deletion cleanup covers late writes after tombstoning or run removal.
Explicit non-NULL schema checks reject invalid saved identities. The reviewer
independently passed all twenty-five output_persistence tests; this accepts the
module checkpoint, not the pending runner/migration/deletion integration.

The bounded 042 follow-up test uses the imported recipient graph, production
action interpolation, and the real action registry with a fixture executor. It
checks recipient connection/resource bindings and the injected summary, and
does not claim full workflow or native-agent execution. The visibility-only
prepare_action_request change is accepted. The 047/048 UI follow-up also passed
scoped source review: History displays each recorded step's memory-byte budget,
and Available copy follows automatic type/source eligibility without offering
recipient-side pin changes for linked memories. Actual visual QA remains separate.

The 052 Step 1 high review found one P2 contract inconsistency before freeze:
history.search accepts a Memory source in Historical state, but history.get
rejects Memory sources and memory.get rejects Historical state. Such a valid
search hit has no compatible get result. Align the search/get source-state
matrix (historical memory may consistently use Evidence) and add a round-trip
matrix regression. Other scoped request/response limits, strict unknown-field
decoding, RFC3339 bounds, Unicode query limits, serialized overhead, and step
counters passed review. The reviewer independently passed all eight contract
tests; log: /tmp/alfred-052-review-step1.log. Core scoped reads, cursor authority,
permission/deadline wiring, and native advertisement remain later steps; adapter
activation remains blocked. No production source or frozen fixture was changed
by this review.

The 052 Step 1 re-review accepted the source/state correction. history.get now
accepts historical Memory sources consistently with history.search while still
rejecting Current responses. The new matrix covers every accepted search
source/state pair and its corresponding get response. The reviewer independently
passed all nine focused Rust tests; log: /tmp/alfred-052-review-step1-final.log.
This accepts only Step 1's bounded contract and feasibility checkpoint. Scoped
database reads, cursor authorization, host permissions, and provider advertisement
remain unimplemented or blocked; no native provider was enabled by this change.

The first full 049 high review rejected conformance on production lifecycle and
provenance gaps. (P1) Every runner capture uses step_id=None, including completed
host/model results before insert_step. Actual step deletion therefore does not
activate the step FK/source scrub proven by the synthetic non-NULL-step fixture.
Capture original authored fields and recorded results once at their producing
step, atomically tied to its persisted ID; downstream forwarding must not create
unlinked transcript copies. Verify the runner-used capture/approval path followed
by direct step deletion, including downstream forwarding and pending candidates.

(P2) validate_candidate_suggestion checks the reviewer-selected evidenceSubject
against source origin but never checks that subject against proposal scope/type.
Configured-only evidence can therefore pass as workflow_configuration while the
proposal is a user-scoped preference. Add deterministic subject/claim-category
restrictions through proposal, edit, and approval validation, and a configured-only
user-preference regression; quotation matching alone remains distinct from human
entailment judgment. (P2) isInference/evidenceSubject exist only on disposable
candidates. Canonical evidence copying/DTOs omit them, so clearing an approved
inference loses its qualification permanently. Persist canonical qualification
and show it after queue cleanup. The candidate evidence list also substitutes
sourceLabel for origin, unlike canonical evidence; always display host origin.

The reviewer independently passed 22 evidence tests, 34 curation tests, and 36
focused frontend tests (165 assertions). Passing fixture tests do not close the
production step-lineage gap. Logs are /tmp/alfred-049-review-{evidence,curation,
frontend}.log. Source text storage obeys the transactional 16/128 KiB limits;
full in-memory Text cloning before capture remains an allocation consideration,
not an additional artifact-read or proven storage-cap bypass. Original-site
capture removes the redundant forwarding copies. No production source or frozen
evaluation fixture was edited during this review.

049 repair checkpoint: production insert_step now binds original-node sources
to its generated step ID and captures them with the step/FTS transaction.
Forwarded prompts are no longer recaptured at Agent boundaries. Configured
claim-category restrictions apply during proposal/edit/approval, and canonical
subject/inference qualification survives candidate cleanup and remains visible.
The reviewer independently passed 36 curation tests. The three prior findings
are resolved, but removing forwarded capture also removed every production
PriorMemory source: its typed_prompt_sources arm is test-only. The owner is
restoring reference-only comparison capture for actually supplied memory at
original selection/composition boundaries; final 049 acceptance awaits that fix.

The independent 051 canonical-save facade passed high review and 31 focused tests
independently (/tmp/alfred-051-review-facade.log). It reuses canonical row/FTS
writing within the saved-receipt transaction, preserves distinct run/node output
identities, records failure without rerunning a node, and bounds pending recovery
batches. This is module acceptance only; shared runner, deletion, migration,
retry-command, and startup integration remain separate.

052 independent SQLite core is rejected despite 12 passing focused tests
(/tmp/alfred-052-review-core.log). Three concrete corrections are required:

- P1: history_step_candidates uses snippet(run_step_fts, -1, ...) at
  agent_memory_lookup.rs:1158 and matches all indexed input/output/error columns.
  history.rs:468 indexes the fully formatted input prompt, including supplied
  memory. After a link is removed or a memory becomes unavailable, a search can
  return its copied prompt text as historical "model output" without checking
  that memory's access or state. Use eligible source-specific history records and
  proven output blocks; do not expose composed input/metadata snippets. Regress
  an indexed Agent prompt containing a linked-memory sentinel, unlink/retract
  it, and verify neither history search nor get replays that copied source.
- P2: memory_state at :1056 evaluates validity against the session's captured
  self.now. Natural expiry after search never takes effect during that session.
  Inject a clock sampled per call, retain deterministic tests, and test search
  before expiry followed by get/search after expiry without editing the row.
- P1: LookupCall checkpoints at :1790 only surround blocking work. Db::with_conn
  mutex/SQLite work and File::open/seek/read at :2057 onward cannot be interrupted
  by those checks, so the advertised 5-second deadline is not enforced. Add a
  bounded cancellable execution mechanism and regular-file validation, with
  stalled database/file fixtures proving deadline return and bounded worker
  cleanup. Permission/native-host/Plan050 identity reconciliation remains an
  explicitly pending dependency rather than accepted production authorization.

049 final backend re-review accepted the restored reference capture. Both actual
Agent paths derive reference-only sources from final `composition.uses`; captured
data contains memory ID, use reason, and ranking version, and the step transaction
binds the generated FK. The reviewer independently passed
`runner::tests::memory::pinned_and_manual_memories_are_not_recalled_twice` and the
injected review/approval/source-step-deletion lifecycle on the same freshly built
test binary. A second Cargo invocation encountered an in-progress 052 LookupCall
signature mismatch, which is unrelated to this scoped acceptance and remains a
combined-tree gate. Root authorized 050 shared integration after this review.

051 independent frontend receipt checkpoint accepted on high review. The DTO
matches the current Rust receipt serialization, retry requests contain only
run/node identity, and the three retryable failures match the backend allowlist.
Rendering covers all seven states, suppresses actions for legacy empty data,
uses canonical memory IDs for opening saved rows, and distinguishes requested
pinning from completed pinning. The reviewer independently passed 7 component
tests / 50 assertions (/tmp/alfred-051-review-frontend.log). These are static
component/contract checks, not parent async or navigation integration proof.
Parent retry error handling, preserving a successful retry through refresh
failure, inactive-workflow navigation, event/store wiring, and actual browser
interaction remain pending as documented in the 051 integration handoff. No
blocking defect was found in these four standalone frontend files; this is not
full Plan 051 acceptance. No production file or frozen fixture was edited.

052 repaired independent-core re-review (2026-09-10): historical prompt leakage
and the frozen clock are resolved. Search now restricts step FTS matching and
snippets to output_text behind a saved receipt; the regression includes copied
prompt/error sentinels and a successful safe-output get. Production calls sample
the injectable clock, and the natural-expiry regression passes without row edits.
Deadline conformance remains rejected on two concrete paths:

- P1: StepSnapshot::version at agent_memory_lookup.rs:2073 calls
  file_metadata_identity at :2322, whose path.metadata() runs synchronously on the
  caller. Both historical search and get execute it before the bounded file
  worker. A stalled filesystem metadata operation therefore still prevents the
  promised deadline return. Move metadata/version probing through the bounded
  file mechanism and test actual execute_json search/get with an injected stalled
  metadata reader, including bounded worker ownership and cleanup.
- P1: lookup_with_conn at :2114 watches the deadline and interrupts SQLite, but
  leaves its busy handler unchanged. The installed rusqlite initializes a 5,000ms
  busy timeout; interrupt does not stop that sleep/retry loop. An external lock
  reached after earlier work, or cancellation while waiting, can exceed the
  remaining deadline. An independent SQLite lock probe with a 20ms interrupt
  still waited roughly 475ms for a 400ms busy timeout. Use a bounded busy policy
  with safe restoration, or equivalent bounded execution, and regress a real
  second-connection EXCLUSIVE lock through the lookup path. Existing tests cover
  the Rust mutex and CPU-heavy SQL, which do not exercise this case.

The independent focused suite compiled and ran 16 tests: 15 passed, including
all four new repair regressions; the remaining scope test failed its evidence
subject CHECK (/tmp/alfred-052-repair-review-core.log). The owner changed that
fixture to user_statement after this run, so its rerun remains pending. This
review does not activate adapters or accept pending shared integration. Findings
were sent to root and the 052 owner; production and frozen fixtures were untouched.

050 correction evaluator high review (2026-09-10): the test-only evaluator uses
production evidence capture/catalog, target discovery, central suggestion
validation, atomic finalization, canonical edits, and revision-bound approval.
The two-review race validates both suggestions before finalizing either review
and measures both approval mutations. Source-run deletion is reported honestly:
raw candidate_not_found is retained, and the evidence_unavailable semantic label
requires both candidate absence and absence of captured sources. Frozen labels
are compared against measured state; no model-quality claim or provider execution
is made. The fixture digest remains c370db4160f7f33cb3bd86f344b87b4fa7e61b7c8e793ebc47490ee50407abf9.

One P2 blocks accepting the new repeatability gate: the independent run failed
memory_eval_corrections.rs:494 because :243 records required_target_ids in
discovery order. correction-two-subjects produced [route, other-route] versus
[other-route, route] across fresh databases; random captured-source IDs can
change equal-time ordering. Both runs otherwise have zero case violations.
Normalize this set-valued measurement before comparing reports; do not change
the frozen labels or production ranking. The focused failure and full compared
reports are in /tmp/alfred-050-correction-review.log. Evaluator acceptance awaits
this correction and its rerun; no implementation or fixture was edited by review.

050 correction evaluator accepted after measured target IDs were sorted according
to their frozen set semantics. The independent rerun passed all 21 cases and
fresh-database report equality (/tmp/alfred-050-correction-review-repeat.log).
No frozen label or production ranking change was required.

052 targeted deadline re-review: both P1 residuals are resolved. Metadata identity
probes now use the same bounded worker as page reads; actual search/get tests
exercise timeout, unchanged byte accounting, and cleanup. SQLite reads temporarily
disable the default busy timeout, retry busy results with call checkpoints, and
restore the prior timeout; a real second-connection exclusive-lock test passes.
The reviewer independently passed all 18 tests
(/tmp/alfred-052-final-deadline-review.log).

One directly introduced P2 remains at agent_memory_lookup.rs:1830-1837:
run_file_operation sends its result before ActiveGuard clears file_worker_active.
The receiver can return while that flag is still true, so the next sequential
operation is rejected as StorageUnavailable. Historical get now performs metadata
then page reads, making this handoff part of the ordinary path. Release completed
operation ownership before publishing its result, retaining unwind cleanup, and
test the helper with a deterministic post-publication worker barrier. One hundred
ordinary artifact-paging repetitions passed; this is a source-proven scheduling
race, not a reproduced stress failure. Root and the owner received the finding.
Independent module acceptance awaits this small correction; shared/native
integration is still outside the accepted scope. No implementation was edited.

050 first integrated-backend high review (2026-09-10): rejected on five concrete
findings, sent to root and the owner before handoff. The accepted 21-case policy
evaluator remains useful but does not cover these paths.

- P1 — Existing-profile startup fails before migrations. Db::open executes
  schema.sql before apply_migrations; schema.sql:390 creates the target-revision
  index on columns absent from old memory_candidates. Loading the exact initial
  baseline schema into an isolated SQLite DB and then executing current schema
  reproduced `no such column: expected_target_memory_id`. Defer dependent index
  creation until ensure_column/migration, or stage bootstrap safely. Regress the
  actual production old-schema -> schema.sql -> migrations order, twice.
- P1 — Approval scope is not revalidated atomically. memory_curation.rs:758 reads
  memory_context before the approval transaction and passes that stale workspace
  to apply_approved_candidate (:1441). A concurrent workflow directory change can
  therefore approve a workspace claim into the old scope. Load current workflow
  scope inputs inside the same transaction; use a controlled pre-transaction
  directory-change regression and assert no canonical write.
- P1 — Artifact target completeness lacks content binding. memory_conflicts.rs
  render_target (:202) marks a small artifact complete after bounded reading but
  copies the row's previously stored digest. Neither that read nor approval
  validates the artifact against this identity. File changes without a DB edit
  can leave the shown bytes and reviewed identity inconsistent, or let approval
  correct a changed file. Use verified immutable content identity with rechecks,
  or keep artifact targets incomplete for destructive operations. Regress a
  short artifact changed both before snapshot capture and after proposal capture.
- P2 — Editing a supersede can bypass the no-op guard. update_memory_candidate
  uses skip_target_visibility (:649), which leaves target_snapshot absent in
  validation; supersede_unchanged (:2278) is checked only with that snapshot.
  apply_approved_candidate has no unchanged-target check. Create a valid pending
  replacement, edit its body back to the original target with reconfirmation,
  then approve: this must not supersede the target with an identical new row.
  Preserve reviewed identity while enforcing the operation-specific guard during
  edit and again inside approval.
- P2 — Date-only evidence permits an invented time. The fallback in
  quote_supports_normalized_instant (:2396) compares only date_naive, so a quote
  containing 2026-08-20 authorizes 2026-08-20T12:34:56Z. Require an exact supported
  instant, or explicitly normalize a date-only token to a documented midnight
  boundary. Add proposal/edit regressions for arbitrary same-day times and keep
  valid timezone-offset equivalence passing.

Writer/digest field lists and fresh/migrated trigger definitions align for the
declared semantic fields; manual semantic edits clear canonical evidence in the
write transaction, and sharing consumers now use the common validity helper.
Those observations do not replace the owner's pending grouped regression gates.
The independent conflict filter ran five tests successfully
(/tmp/alfred-050-review-conflicts.log). Review made no implementation or frozen
fixture edits; this is a bounded correction batch, not full Plan 050 acceptance.

052 independent-core final targeted review accepted (2026-09-10). The remaining
worker handoff race is fixed: operation completion explicitly releases an
Option-owned guard before publishing its result, so later Drop cannot clear a
successor's ownership. The deterministic regression keeps the first worker alive
behind a post-publication barrier while a successor completes through the same
production helper. Timeout/cancellation cleanup and single active file-operation
ownership remain intact. The reviewer independently passed all 19 focused tests
(/tmp/alfred-052-worker-handoff-review.log), including metadata deadlines and
external SQLite lock handling. This accepts the independent core checkpoint only;
shared integration and provider activation remain pending. All five Plan 050
backend findings remain open. No implementation or fixture was edited by review.

050 backend repair re-review (2026-09-10): all five prior findings are resolved.
New-column objects are deferred until migration; exact initial-baseline schema
followed by current schema now succeeds, and the production-order migration test
passes. Approval resolves workflow context under its transaction. Artifact targets
remain explicitly incomplete comparison material. Targeted edits preserve reviewed
identity and both edit/approval reject unchanged supersedes. Date-only evidence
supports midnight only; equivalent explicit offsets remain valid. Reflow hashing
now preserves identifier case. Store creation forwards all date fields, and active
manual/suggestion drafts retain their original expected revisions across refresh.
Review caught a same-timestamp refresh-before-Edit hydration gap; the owner added
semanticRevision to the guarded hydration dependency list, which resolves it
without replacing an active edit. Root owns the actual UI regression.

Independent checks passed: 8 conflict tests, 12 approval tests, 1 bootstrap test,
1 exact-date test, and 54 frontend tests / 249 assertions. Logs are
/tmp/alfred-050-recheck-{conflicts,approval,bootstrap,dates}.log and
/tmp/alfred-050-051-recheck-frontend.log. This closes the bounded source findings;
the owner's remaining writer matrix, broader gates, and UI checks remain required.

051 frontend integration is not yet accepted. Two P2 findings remain:

- history-page.tsx:272 replaces the whole detail with one retry's refreshed
  snapshot. retainSuccessfulRetry protects only that retry's receipt. A real-helper
  controlled interleaving reproduced the failure: commands A and B both return
  Saved, B's refresh fails, then A's stale refresh contains B Failed; displayed B
  becomes Failed. Merge against the current terminal receipts for every output,
  keeping Deleted precedence, and test two overlapping retries with deferred
  refreshes. Single-retry failed/null/stale refresh tests do not cover this case.
- history-format.ts:107 assumes activeWorkflowId proves the Memories context is
  loaded. Production selectWorkflow sets that ID before loadMemories finishes,
  and loadMemories catches its errors, so the navigation can open after a failed
  read or immediately while an already-active target is still loading. Require
  successful context loading before focus, and test the actual store with deferred
  and rejected listMemories calls rather than a stub selection that throws.

Exact retry payload and Canvas generation cancellation wiring otherwise match
the intended frontend contract. Native receipts/events, persistence store wiring,
and the old store save-side-effect removal remain explicitly pending. No full
Plan 051 backend acceptance is implied; root received the repair requirements.
Review changed only this log, not implementation or frozen fixtures.
