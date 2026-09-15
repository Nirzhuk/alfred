# UI acceptance for plans 042 and 045–052

This is the pending acceptance checklist. It is not a test result.
The assigned tester uses GPT-5.6 Luna with extra-high reasoning effort.

Use an isolated browser tab and synthetic Tauri IPC fixtures. The available
Argent Chromium target is `chromium-cdp-9222`; discover it again before testing.
An isolated Vite server is running at `http://127.0.0.1:1422/` to separate QA
browser storage from the user's development tab. The coordinator owns this server.
Read the interaction skill and discover every tap target from the current DOM.
Keep command-call evidence alongside screenshots, since a screenshot cannot
prove whether an import, approval, retry, or run command fired.

Do not modify the user's live database, trigger destinations, or provider account.
Browser fixtures do not prove native packaged deep-link behavior or real provider
isolation. Record those gates separately.

| Plan | Required visible checks |
| --- | --- |
| 042 | Share entry points; completed-save snapshot; reusable setup fields; exact executable-content review; unlisted-link disclosure; copy and preview; ambiguous-upload retry; publication list and revocation after source workflow deletion |
| 042 | Import by pasted link and queued deep link; graph/requirements/content preview; account then resource setup; local file and memory selections; missing requirements; validation failure; cancellation; duplicate delivery; independent imported canvas with disabled starts |
| 046 | Review off; supported fixture; unavailable provider/version; precise input disclosure; failed job; retry; rollback after settings IPC failure; global and workflow controls |
| 047 | Successful empty recall versus unavailable recall; lexical/local recall wording; raw reports available for deliberate selection; historical and current ranking versions |
| 048 | Normal, empty, large, omitted, and missing-artifact selections; editable unavailable selections; explicit-use traces; count-based omission reasons and actual byte totals |
| 049 | Evidence beside approval; source/node identity; incomplete/deleted evidence; invalid citations; edited claim handling; approved provenance after queue cleanup |
| 050 | Replacement and retraction previews; evidence and observed/validity dates; stale target/candidate revisions; changed or deleted target; refreshed conflict state; historical inactive claim labels |
| 051 | Saved, skipped, failed, and deleted Output receipts; history reload; storage-only retry; saved report reading; deletion; inactive workflow refresh; replay without a memory-create or workflow-run call |
| 052 | Separate read-tool opt-in; unsupported runtime explanation; rollback on persistence failure; immediate disabled state after revocation; distinction from automatic recall |

For every changed view, check light and dark themes, keyboard reachability,
accessible names, loading/error layout, long text, and clipping. Shared searches
must preserve typing, clear/refocus, and picker keyboard navigation inside their
actual modal or nested-dialog consumer.

Record each scenario's expected behavior, observed behavior, command evidence,
screenshot path, and verdict. A fixture-backed supported state must be labeled
as synthetic and must not be presented as proof of live runtime support.

## Sharing review regressions

Use the actual ShareDialog, ImportLinkDialog, and PublicationList through
`http://127.0.0.1:1422/tests/ui/sharing-plans.html`. Query arguments are
`mode=share|import|manage`, `theme=light|dark`, and optional
`scenario=delayed|resource-failure|publish-failure|existing|revoked`.
All IPC is synthetic; this fixture does not prove native deep-link delivery,
provider authorization, database persistence, or a public service deployment.

In both themes:

1. Share: move from Describe to Make reusable. Select “Retain a public example
   for Release message”. Its editor must remain visible while empty and Continue
   must be disabled. Enter a public example, inspect the final JSON, and verify
   both message targets receive the value and the recipient question is removed.
   Create the synthetic publication, inspect its link, then revoke it in the
   management list. No real publish call is allowed during QA.
2. Import: Review account offers the connected Claude account, excluding the
   expired and incompatible accounts. Team connection loads Team channel and
   Incoming channel through its secondary bindings. Partner connection loads
   independently. Browse Notes folder and verify the mock IPC asks for one
   directory and the resulting value is a string. Complete setup and inspect the
   synthetic check/import payload, including disabled starts and local values.
3. Delayed import: select Team and Partner connections while both resource loads
   are pending. Use `window.sharingFixture.releaseResources(connectionId)` only
   to release the fake provider response. Release Partner before Team; both
   resource controls must retain their own choices. Change Team to Alternate
   before releasing an old Team response; the old response must not repopulate
   Team or Incoming channel. A new Alternate response must populate both.
4. Failure/existing scenarios: show resource errors with reload available, a
   revoked-link error, publication failure with editable preview, and the
   already-imported notice without creating a second copy. Check keyboard focus,
   labels, overflow, and console errors in the actual modal.

The deterministic setup-model tests separately cover multi-page intersection,
repeated-page rejection, bounded pagination, and stale response cancellation.

## Memory context and selection fixtures

`http://127.0.0.1:1422/tests/ui/memory-context-plans.html` imports the actual
NodeSettingsModal, MemoriesInspector, and HistoryPage with synthetic IPC. Query
arguments are `screen=picker|library|history`, `theme=light|dark`, and
`scenario=normal|empty|large|unavailable|legacy|lookup-failure`. The library can
select a starting item with `memory=context-report|context-note|context-fact`.
The fixture exposes command calls, errors, selected IDs, and memory navigation
events through `window.contextFixture`.

Check expired, retracted, deleted, and large selected memories in the actual
picker. Remove an unavailable selection; search, clear/refocus, and deliberately
select the raw saved report or a linked note. Empty selection and failed linkable
lookup have their own scenarios. The library's Show original file action returns
a synthetic unavailable-file error and must not open a real path.

In History, compare empty recall with unavailable recall. Check explicit/pinned
use labels, current `lexical_v2` and retained `lexical_v1` versions, measured
per-step byte totals, size/unavailable/repeated omission counts, and trace-failure
copy. Verify clicks reference the expected memory. No workflow or agent executes
in this browser fixture; backend prompt/byte enforcement has separate tests.

## Evidence approval fixtures

`http://127.0.0.1:1422/tests/ui/memory-evidence-plans.html` mounts the actual
MemoriesInspector with synthetic IPC. Use `theme=light|dark`,
`screen=suggestions|library`, and these scenarios:

- `normal`, `multi`, and `malicious`: evidence placement, origins and source
  identities, multiple quotations, inference and model-assessment labels, literal
  text rendering, source-run navigation, and explicit approval.
- `edit-failure` and `approval-failure`: stable backend rejection codes, retained
  edit draft, unchanged canonical data, and retry availability.
- `legacy`, `removed`, `empty-evidence`, and `needs-reconfirmation`: unavailable
  evidence and blocked approval, with explicit rejection where supported.
- Library `queue-cleared`, `removed`, `manual`, `legacy`, and `evidence-failure`:
  durable canonical provenance, removed quotation state, honest provenance labels,
  and evidence-load errors. `queue-cleared` seeds the post-cleanup state; it does
  not prove a queue cleanup operation. Database tests cover that transaction.

During editing, confirm evidence, change the claim again, and verify confirmation
resets. Check title, body, type, and scope, then reconfirm and save. Read actual
payloads and fixture state via `window.evidenceFixture.calls`, `.candidates()`,
and `.memories()`. `.openedRuns` records source-run navigation callbacks; the
fixture displays the requested ID without fetching native history. The fixture
tests UI behavior, not citation validation or canonical database transactions.
Add `diagnostics=1` to expose a Show QA evidence button with the same recorded
commands and state as accessible text for computer-use tools without JS access.

## Output persistence fixtures

`http://127.0.0.1:1422/tests/ui/output-persistence-plans.html` uses the actual
HistoryPage, library dialog, and run-event store with synthetic IPC. Production
receipt UI is still being connected; fixture preparation is not an acceptance
result. Use `theme=light|dark`, optionally `active=false`, and scenarios `saved`,
`failed`, `retry-failure`, `retry-refresh-failure`, `refresh-failure`, `deleted`,
`not-requested`, `empty`, `pending`, `saving`, `multiple`, or `legacy`.

Replay completion through the visible QA control. The current graph deliberately
has different title/pin settings from the captured receipt. Repeated completion
must make zero `create_memory` calls for both active and inactive workflows. The
fixture rejects unexpected creation and workflow execution commands. Retry may
supply only `runId` and `nodeId`, must preserve the captured disposition, and
must leave completed step output readable after failure. Refresh failure after a
successful retry must not relabel the saved receipt as a storage failure.

Inspect `window.outputFixture.calls`, `.errors`, `.receipts()`, `.memories()`,
and `.openedMemories`. Reload History through the visible QA control to check
durable metadata reads. Library navigation calls the production
`navigateWorkflowMemory` helper with the actual store's
`ensureWorkflowMemoryContext` and
renders the actual inspector. `active=false` starts from another workflow and
installation-wide History; diagnostics record the requested workflow, actual
active workflow, and loaded library IDs. The fixture does not mount the full
WorkflowCanvas; its event/close/navigation wiring has separate checks. These
browser checks cannot prove actual receipt transactions, restart recovery,
artifact storage, or native Tauri transport; Rust integration tests cover the
storage lifecycle with temporary files.
Add `diagnostics=1` to inspect actual recorded commands and state in the visible
QA evidence panel. Hide it before checking the product layout.

`scenario=concurrent-retry` starts with two failed receipts. Retry the first,
which returns Saved and holds an older History response. Retry the second,
whose save succeeds while its History refresh fails. Use Release stale history
refresh and verify both receipts still show Saved. The diagnostic counter must
record two identity-only retries. `scenario=refresh-failure&active=false` keeps
the library load unavailable; opening the saved-memory link must not open an
inspector with an unconfirmed context.

## Correction approval fixtures

`http://127.0.0.1:1422/tests/ui/memory-corrections-plans.html` mounts the actual
MemoriesInspector with synthetic IPC. Use `theme=light|dark`, `diagnostics=1`,
and `scenario=replacement|retract|relationship|stale-target|stale-candidate|stale-candidate-edit|source-removed|future|expired|stale-manual|refresh-before-edit`.
Both-theme results and the completed fixture accessibility repair are recorded
in [050 UI results](plans-050-ui-results.md).

For concurrent-change scenarios, first select the suggestion, then use Apply
concurrent change before approval. A changed target keeps its newer canary body
and original edit timestamp. A changed candidate increments its own revision.
Source removal scrubs the quotation and blocks the retained candidate. Check
error copy, refreshed state, and zero approval mutations in the QA evidence panel.
Every approval must send only `input.id` and `input.expectedCandidateRevision`.

Check that replacement updates the selected target, while retraction creates no
replacement row. The platform confirmation result is synthetic; use
`scenario=retract&confirm=cancel` to verify cancellation sends no approval command.
A possible relationship must remain an unresolved comparison, and approval of
its create operation must preserve the existing row. Future/expired suggestions
must explain why they cannot apply now. The fixture does not advance a real clock
or establish backend date validation; the frozen correction evaluation does.

`stale-manual` starts in the canonical library. Begin editing, change the body,
then Apply concurrent change. This updates the backend body/status/revision and
refreshes the actual store while the draft is dirty. Saving must send the revision
the draft began with, keep the newer canary body superseded, and retain the user's
draft with a changed-memory explanation. A successful overwrite would increment
the fixture's canonical mutation counter and expose a lost-update bug.

`stale-candidate-edit` exercises the same interleaving for a suggestion draft.
After starting an edit, the concurrent-change control updates the backend
candidate and emits the real `memory://candidates-changed` event through Tauri's
event mock. Saving must retain the draft's original revision and reject the stale
edit, while keeping the other window's updated suggestion intact.

`refresh-before-edit` applies the same-timestamp canonical change before Edit.
The editor must show the new canary body and use revision 2 for a subsequent
title-only save. The unchanged body must remain canary after saving.
