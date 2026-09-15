# Plan 050 UI QA

Date: 2026-09-10  
Source snapshot: `85f0d64` plus the untracked correction fixture and current
frontend working tree  
Target: isolated Vite `http://127.0.0.1:1422`, visible dedicated CUA Chrome
tabs `461987457`, `461987467`, and focused retest tab `461987530`, Chrome 152  
Fixture: `tests/ui/memory-corrections-plans.html` /
`tests/ui/memory-corrections-plans.tsx`

This is actual UI QA against `MemoriesInspector` with synthetic IPC. It does
not prove Tauri transport, Rust handler behavior, the real database, provider
calls, or backend date validation. No user browser tab, native app, live
provider, or user database was touched.

## Stable correction matrix

The following flows were run in both light and dark themes with actual AX
discovery before every interaction. The fixture's visible QA panel was opened
after the interaction to inspect recorded calls and canonical state, then the
page was reloaded for the next case. CUA screenshots were visually inspected
in-session; the CUA backend does not expose a filesystem screenshot path.

| Scenario | Result | Visible result and synthetic IPC evidence |
| --- | --- | --- |
| `replacement` | Pass, light + dark | Detail showed `Supersede`, current target revision 1; approval showed `Suggestion approved.`, target became superseded, and one new active memory row was created. Approval payload was exactly `{id:"correction-qa-candidate", expectedCandidateRevision:1}`. Light diagnostics reported `canonicalMutations:2`. |
| `retract` | Pass, light + dark | Detail showed `Retract`; approval showed `Suggestion approved.`, target became `retracted`, and no replacement row appeared. Diagnostics recorded the warning `plugin:dialog\|message` followed by the exact `{id, expectedCandidateRevision}` approval input; `canonicalMutations:1`. |
| `retract&confirm=cancel` | Pass, dark | Warning prompt call had `buttons:"OkCancel"` and the fixture returned cancel. No approval call occurred; candidate stayed `pending`, target stayed `active`, and `canonicalMutations:0`. |
| `relationship` | Pass, light + dark | Detail displayed `Possible relationships` and explicitly said matches were not merged or used as replacement targets. Approval created a separate active memory while preserving the existing target row. Candidate operation was `create`, with null target/revision fields. |
| `future` | Pass, light + dark | Detail displayed `Effective 1 Jan 2099, 01:00`. Approval produced `This correction is not effective yet. Approve it on or after its effective date.` No approval IPC was sent by the UI preflight; `canonicalMutations:0`. |
| `expired` | Pass, light + dark | Detail displayed `Expires 2 Aug 2026, 02:00`; approval changed the candidate to `Blocked`, showed `This suggestion expired before approval.`, and retained the target unchanged. Diagnostics recorded `{id, expectedCandidateRevision:1}`, `blockedCode:"proposal_expired"`, and `canonicalMutations:0`. |

The visible dark and light surfaces showed the same operation/status wording and
date disclosures. Replacement/retraction/relationship details retained the
recorded-observation evidence label and quote before approval.

## Concurrent and removed-source cases

`stale-target`, light: Pass. The AX-visible concurrent control was activated
before selecting the suggestion. Approval sent exactly
`{id:"correction-qa-candidate", expectedCandidateRevision:1}`, then the UI
showed `The target memory changed after this suggestion was reviewed.` and
refreshed the target to the newer canary body with `current revision 2`.
Diagnostics showed `blockedCode:"target_changed"`, candidate status `blocked`,
target body `ALFRED-4821 deploys to canary.`, `semanticRevision:2`, and
`canonicalMutations:0`.

`source-removed`, light: Pass. After the AX-visible concurrent control was
activated, approval showed `This suggestion can no longer be approved.` and
`Its source history was deleted, so this suggestion can no longer be approved.`
The evidence area stated that copied quotes and digests were removed. The
diagnostics panel recorded candidate status `blocked`,
`blockedCode:"source_removed"`, `evidenceState:"source_removed"`, null quote,
null source/digest offsets, and `canonicalMutations:0`.

## Draft-version paths and original fixture blocker

The owner-repaired `stale-manual` and `stale-candidate-edit` paths were not
accepted. The real stale-manual editor opened and accepted a changed draft,
but the fixture's fixed-position `Apply concurrent change` button was visible
in the CUA screenshot while omitted from the live AX tree and keyboard focus
cycle once the modal editor was open. The same AX omission occurred for the
concurrent control on `stale-candidate` and for the dark concurrent cases.
Under the no-screenshot-coordinate rule, the tester did not activate a control
that was not discoverable. Therefore no stale-manual or stale-candidate-edit
Save result, and no dark stale-target/source-removed result, is claimed.

This is a fixture accessibility blocker, not a product pass/fail. Exposing the
concurrent-change control inside the modal's accessible tree will allow the
remaining revision-2/draft-retention checks to run honestly. The fixture's
visible state otherwise showed the expected synthetic control and no browser
console error was observed during the completed flows.

## Portal-fix completion and focused retest

The fixture then added `tests/ui/qa-modal-portal.tsx`, which portals the
synthetic concurrent control and QA evidence panel into the active modal's
`role="dialog"` / `aria-modal` tree. Re-tested with the visible dedicated CUA
Chrome tab `461987530` (Chrome 152), URL `127.0.0.1:1422`, with AX discovery
before every action. The controls were discoverable and actionable; no page-level
error prevented the flows. The earlier blocker above is superseded by this
retest. Console scope is bounded: the tester did not claim a clean browser
console; the coordinator later observed extension wallet warnings and a
fixture/HMR duplicate-`createRoot` warning during active edits.

| Focused flow | Result | Actual UI and diagnostic evidence |
| --- | --- | --- |
| `stale-target`, light + dark | Pass | After applying the concurrent change, approval sent `{id:"correction-qa-candidate", expectedCandidateRevision:1}`. UI showed `Suggestion blocked. The target memory changed after this suggestion was reviewed.`; target displayed the canary body and `Reviewed revision 1; current revision 2`. Candidate was `blocked` with `blockedCode:"target_changed"`; `canonicalMutations:0`. |
| `source-removed`, light + dark | Pass | UI showed `This suggestion can no longer be approved.` with the source-history-deleted explanation. Diagnostics showed candidate `blocked`, revision 2, `blockedCode:"source_removed"`, null quote/source offsets/digest, `sourceRemoved:true`, and `canonicalMutations:0`. |
| `stale-candidate`, light + dark | Pass | Approval sent `expectedCandidateRevision:1` after the fixture advanced the candidate to revision 2. UI showed `This suggestion changed. Refresh it before deciding.` and retained the updated other-window title. Candidate remained pending; `canonicalMutations:0`. |
| `stale-manual`, light + dark | Pass | Changed the draft body, applied the concurrent target update, then saved. `update_memory` sent `expectedSemanticRevision:1` with the edited body. UI showed `Could not save these changes. Your draft is still here; try saving again.`; draft remained visible while the canonical memory retained the newer canary body at revision 2 and `superseded` status. `canonicalMutations:0`. |
| `stale-candidate-edit`, light + dark | Pass | Changed draft content and reconfirmed evidence before applying the concurrent candidate update. Save sent `expectedCandidateRevision:1`; UI showed `This suggestion changed. Refresh it before deciding.` and retained the draft. Diagnostics showed candidate revision 2 with the updated other-window title/body, `candidateEdits:0`, and `canonicalMutations:0`. |
| `retract&confirm=cancel`, light | Pass | The `plugin:dialog|message` warning used `buttons:"OkCancel"`; cancel produced no approval IPC. Candidate stayed pending, target active, and `canonicalMutations:0`. This completes the earlier dark-only cancel coverage. |

## Refresh-before-edit hydration retest

The new `refresh-before-edit` fixture scenario was run in both themes. The
concurrent target update was applied before opening Edit, the editor hydrated
the newer canary body, and a title-only edit saved successfully. Diagnostics
recorded `update_memory` with `expectedSemanticRevision:2` and the canary body;
the visible result was `Memory saved.` with the canary body retained and the
new title rendered. The synthetic snapshot showed `errors:[]` and
`mutationApplied:true`.

## Keyboard/visibility check

The production memory editor was opened in both themes through the actual
fixture. Starting on Title, four keyboard `Tab` presses reached the
AX-discoverable `More details` summary; `Space` expanded it. Keyboard focus
then scrolled the feature pane to show Content kind, Status, Importance,
Confidence, Observed, Last confirmed, Valid from, and Expires. Light and dark
CUA screenshots showed the expanded summary, all four date controls, their help
copy, and the footer in the viewport. The dedicated CUA backend does not expose
filesystem screenshot paths; the representative dark tab remains available as
tab `461987530` for independent inspection.

The coordinator independently accepted the visual layout using Argent screenshot
`argent-screenshot-chromium-cdp-9222-1789011963521-21551.png` (light target and
evidence) and dedicated CUA tab `461987539` (dark and light manual Edit → More details
→ Observed, all four dates, labels/help, and footer fit). That tab was closed
after inspection; focused CUA tab `461987530` remains open.

These results remain browser-fixture evidence only: the fixture does not prove
Tauri transport, Rust handlers, real database retention, or live provider
behavior.
