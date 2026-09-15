# Plan 051 UI QA

Date: 2026-09-10  
Source snapshot: `85f0d64` plus the current untracked output-persistence
fixture and frontend working tree  
Target: isolated Vite `http://127.0.0.1:1422`, dedicated CUA Chrome 152 tab
`461987571`, CDP device `chromium-cdp-9222`  
Fixture: `tests/ui/output-persistence-plans.html` /
`tests/ui/output-persistence-plans.tsx`

This is actual UI QA against the production `HistoryPage`, output receipt
components, Memories inspector, and run-event store with synthetic IPC. Every
interaction used AX discovery before the action. The fixture does not prove
Tauri transport, Rust handlers, real database transactions, provider calls,
restart recovery, or native receipt durability. No user browser tab, native
app, live provider, or user database was touched. CUA screenshots were
visually inspected in-session; this CUA backend does not expose a filesystem
screenshot path. The coordinator independently retained Argent screenshot
`argent-screenshot-chromium-cdp-9222-1789021584172-21551.png` for the dark
multiple/inactive layout and
`argent-screenshot-chromium-cdp-9222-1789021635973-21551.png` for the light
retry-refresh-failure state.

## Receipt and status matrix

The following scenarios were loaded and inspected in both `theme=light` and
`theme=dark`. Where a Retry control was present, it was activated through its
AX-discovered button and the visible state and diagnostics were read after the
operation.

| Scenario | Result | Actual UI and diagnostics |
| --- | --- | --- |
| `saved` | Pass, light + dark | Receipt showed `Saved to Memories`, `The output is stored in Memories.`, and `Output · Pinned when saved`. Expanding Output exposed the captured body, including the graph-edited-after-run text. Opening the receipt opened the actual Memories inspector and displayed captured title/body, pinned state, `Workflow output` source, and the stored memory ID. The captured title/pin metadata remained in diagnostics even though the current graph settings differ. |
| `failed` → Retry | Pass, light + dark | Initial receipt showed `Save failed`, `Alfred could not add this output to Memories.`, and `Retry saving Captured report title`. Retry changed it to Saved and created the synthetic memory. Diagnostics recorded exactly `{runId:"output-qa-run", nodeId:"output-qa-node"}` for `retry_output_persistence`; the saved receipt retained captured title, `pinMemory:true`, and the output body. No `create_memory` or `start_workflow` call occurred. |
| `retry-failure` | Pass, light + dark | Retry remained a storage failure, showed `Output couldn’t be saved. Try again.`, and left the Retry control available. Diagnostics recorded the identity-only retry, no refresh call after the rejected save, the original `database_write` state, no memory, and no synthetic errors. |
| `retry-refresh-failure` | Pass, light + dark | Retry changed the receipt to Saved and retained its captured title/pin metadata, then showed `Output was saved, but History couldn’t refresh. Reload History to see the latest details.`. Diagnostics showed a successful saved receipt and memory, identity-only retry payload, and no create/run calls. A refresh warning did not relabel the successful storage as failed. |
| `not-requested` | Pass, light + dark | Receipt showed `Not saved` and `Saving to Memories was off when this output ran.` with no memory link or Retry action. Diagnostics had no storage identity, memory ID, or mutation call. |
| `empty` | Pass, light + dark | Receipt showed `Nothing to save` and `The completed output contained no text to save.` with no link or Retry action. Whitespace output produced no memory identity or mutation. |
| `pending` | Pass, light + dark | Receipt showed `Waiting to save` and `This output is queued to be saved by Alfred.`; no Retry action or memory link was offered. The pin request remained visible. |
| `saving` | Pass, light + dark | Receipt showed `Saving` and `Alfred is saving this output.`; no Retry action or memory link was offered. The pin request remained visible. |
| `deleted` | Pass, light + dark | Receipt showed `Removed from Memories` and `This output was deliberately removed and will not be recreated.`. Diagnostics showed `state:"deleted"`, a deletion timestamp, null `memoryId`, empty memories, and no mutation calls. |
| `legacy` | Pass, light + dark | History rendered without an Output saves section or receipt controls. The run and Memory context remained readable; diagnostics had `receipts:[]` and no output-persistence mutation. |
| `multiple` → secondary Retry | Pass, light + dark | Two receipts rendered together: primary Saved and secondary Save failed. Retrying the secondary changed both rows to Saved and displayed both memory links. Diagnostics recorded the secondary identity-only retry with `{runId:"output-qa-run", nodeId:"output-qa-secondary"}`, both captured dispositions, both memory IDs, and no synthetic errors. Opening the secondary link rendered the actual inspector with title `Secondary report`, pinned state, output body, `Workflow output`, and the matching memory provenance. |

The two captured receipts use the same completed step output but preserve their
individual receipt titles and node identities. This was checked in the visible
rows and in the diagnostics snapshot after retry.

## Inactive workflow and refresh failure

`saved&active=false` was run in light mode from installation-wide History. The
initial `list_run_history` used `workflowId:null` while the page remained in
the other active workflow. Activating the AX-discovered saved-memory link
called the production navigation path, requested `get_workflow` for
`output-qa-workflow`, loaded the corresponding memory list, and opened the
actual inspector. Diagnostics showed
`requestedMemories:[{memoryId:"8ac34c22-58e1-43f0-92ae-8d4f67283598",workflowId:"output-qa-workflow"}]`,
`activeWorkflowId:"output-qa-workflow"`, the matching library ID, and
`storeError:null`. The coordinator independently accepted the dark
multiple/inactive layout using Argent screenshot
`argent-screenshot-chromium-cdp-9222-1789021584172-21551.png`.

`refresh-failure&active=false` was then run in both themes. History remained
visible with no Memories inspector after activating the saved-memory link;
the navigation path requested the other workflow and `list_memories`, whose
synthetic failure was exposed in diagnostics as
`storeError:"Error: Synthetic library refresh unavailable"`. The AX tree
remained on History and no unconfirmed inspector opened. This satisfies the
bounded UI expectation; it does not establish native transport or database
behavior.

## Concurrent retry

`concurrent-retry` was run in both themes with the prescribed ordering:

1. Retry the primary failed receipt. The row changed to Saved while the first
   `get_run_history` remained held; diagnostics showed `retryCount:1` and
   `staleHistoryHeld:true`.
2. Retry the secondary receipt. Its save completed and the UI showed both
   rows as Saved plus `Output was saved, but History couldn’t refresh. Reload
   History to see the latest details.`. Diagnostics recorded the secondary
   identity-only retry and its failed refresh while the first refresh was
   still held.
3. Activate `Release stale history refresh`. After hiding and reopening the
   visible QA panel to read the updated snapshot, both receipts remained
   Saved, both memory IDs were present, `retryCount:2`,
   `staleHistoryHeld:false`, and `errors:[]`.

This confirms the UI keeps both successful storage results after independent,
out-of-order History refresh outcomes. The diagnostics panel is a snapshot
and must be reopened after an asynchronous refresh; this is a fixture QA
observation, not a product defect.

## Keyboard and visual checks

On the clean dark saved page, AX discovery showed the search field, workflow
scope checkboxes, Back to results, receipt link, Output disclosure, Replay
completion event, and Reload history. Keyboard traversal from the expanded
Output reached the Replay and Reload buttons in order. The captured output
body remained available to AX after expanding the Output disclosure. The
coordinator visually inspected the clean dark saved receipt and the light
retry-refresh-failure card; the retained Argent screenshot paths are listed
above.

## Replay status and limits

The visible `Replay completion event` control was present in every fixture
scenario, but replay acceptance is intentionally pending the owner’s signal
that the old frontend save branch has been removed and the completion hook is
stable. No replay pass is claimed here. Once that hook is confirmed, the
remaining bounded assertion is zero `create_memory` and `start_workflow`
calls, unchanged receipt/memory state, and the same result for an inactive
workflow.

Diagnostics snapshots for the completed cases reported `errors:[]`; this is
synthetic fixture error state only and is not a claim of a clean browser
console or native process logs. These browser checks cannot establish actual
receipt transactions, restart recovery, artifact storage, or native Tauri
behavior; the corresponding Rust/backend tests remain authoritative for
those concerns.

## Joint Plan 051/052 UI acceptance

Date: 2026-09-10  
Final external-driver result: **24/24 cases passed** (zero failures), across the
light and dark themes. The earlier Plan 051 checkpoints above are preserved as
historical evidence; this section records the joint 051/052 acceptance.

The final matrix covered:

- **12 lookup cases** (six scenarios in both themes): unavailable lookup stayed
  disabled while Automatic recall remained independent; supported lookup
  enabled and disabled with the scoped workflow payload; a failed lookup
  permission save rolled the switch back and restored retryability; an
  availability probe failure recovered through the visible Try again action;
  active-but-unavailable lookup remained disableable but could not be
  re-enabled; and workflow switching kept stale availability responses from
  changing the active workflow.
- **8 output replay cases** (saved and deleted receipts, active and inactive
  workflow, both themes): replaying twice did not duplicate persistence or
  invoke `create_memory`/`start_workflow`, and the captured receipt state
  remained authoritative.
- **4 History cases** (memory navigation and failed-output Retry, both themes):
  opening the captured memory used the production navigation path, while
  failed-save Retry persisted the receipt with its captured identity and
  supplied only the expected `{runId,nodeId}` retry identity.

The lookup-switch sequence additionally exercised stale workflow requests,
deferred saves completing after a workflow switch, and close/reopen lifecycle
guards. A late completion did not clobber the reopened workflow or surface
stale save feedback. The lookup-save-failure, lookup-probe-failure, and
lookup-switch cases therefore cover rollback, probe recovery, stale request/save
isolation, and close/reopen behavior in the same UI run.

The acceptance ran the production frontend components in isolated Vite
fixtures. Tauri IPC was synthetic (`mockIPC`); no native handler, database,
provider, agent runtime, lookup transport, credential, or live model call was
enabled. The checks establish frontend rendering, interaction, lifecycle
guards, and observed command/payload boundaries only. They do **not** establish
native transport, Rust persistence or artifact verification, provider
availability, model context capacity, restart recovery, or live end-to-end
lookup usefulness/cost. Native/backend acceptance remains covered by the
corresponding Rust and integration evidence.

The coordinator also visually inspected the unavailable-lookup settings state
at 1360×860 and the minimum 960×640 viewport in both themes; the lookup control
was visibly disabled with the explanatory availability status in each case.
