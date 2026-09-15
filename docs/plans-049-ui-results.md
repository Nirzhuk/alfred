# Plan 049 UI QA

Date: 2026-09-09  
Source snapshot: `85f0d64` plus the uncommitted 049 fixture and current frontend working tree  
Target: isolated Vite `http://127.0.0.1:1422`, Chrome 152 CDP `chromium-cdp-9222`, Argent DOM discovery and keyboard/tap interaction  
Fixture: `tests/ui/memory-evidence-plans.html` / `tests/ui/memory-evidence-plans.tsx`

This is browser QA against the actual `MemoriesInspector` with synthetic IPC handlers. It does not prove Tauri transport, Rust handlers, provider calls, real database retention, or native navigation. The `queue-cleared` state is seeded after cleanup; it does not prove that a deletion action performed the cleanup.

## Scenarios exercised

| Area | Result | Evidence |
| --- | --- | --- |
| Normal suggestion, light | Pass | Selected suggestion, opened Edit, edited title/body, reconfirmed evidence, and Save sent `update_memory_candidate` with `evidenceConfirmed:true`; UI showed `Suggestion updated.` and retained edited values. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963138884-21551.png` |
| Normal suggestion, dark | Pass | Rendered dark Memories view with pending candidate, model assessment `82%`, and evidence-backed list state. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963189922-21551.png` |
| Successful approval | Pass | `approve_memory_candidate {id:"evidence-qa-candidate"}` was followed by `list_memories {workflowId:"evidence-qa-workflow",includeHistory:true}`; candidate became `approved`, UI showed `Suggestion approved.`, and a second active memory appeared. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963508462-21551.png` |
| Multi-evidence inference, light | Pass | Detail view showed `Inference` and both `Release Instructions · Step Configured-Step-0` and `Notification Instructions · Step Configured-Step-1` quotes; Approve was enabled. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963223695-21551.png` |
| Malicious quote, dark | Pass | Literal `<img src=\"https://invalid.example/evidence-probe\" onerror=\"window.evidenceInjected=true\">` was visible as text. No injected DOM was observed and no external request was made by the fixture. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963231342-21551.png` |
| Legacy suggestion, light | Pass | Detail showed `Blocked`, “This older suggestion has no recorded evidence and cannot be approved”, and only enabled `Reject`; no Approve button was rendered. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963238943-21551.png` |
| Source removed suggestion, dark | Pass | Detail showed `Blocked`, “Its source history was deleted, so this suggestion can no longer be approved”, source history removed copy, and only enabled `Reject`. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963246408-21551.png` |
| Empty evidence, light | Pass | Detail showed `Recorded evidence is unavailable. This suggestion cannot be approved.` and only enabled `Reject`. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963253891-21551.png` |
| Needs reconfirmation, dark | Pass | Detail showed the evidence unavailable warning and only enabled `Reject`; no approval action was exposed. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963261288-21551.png` |
| Edit failure, light | Pass with observed retention | Actual edit changed fields, changing content reset the reconfirm checkbox to `false`, rechecking enabled Save, and Save sent `update_memory_candidate` with `evidenceConfirmed:true`. The fixture rejection left the editor open with changed fields and displayed “The recorded evidence is unavailable or changed. Reject this suggestion or run the workflow again.” Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963451604-21551.png` |
| Approval failure, dark | Pass | Actual Approve sent `{id:"evidence-qa-candidate"}`; candidate remained `pending`, no memory was added, and the same precise evidence failure alert was displayed. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963479648-21551.png` |
| Source-run navigation | Pass | Actual `Open source run in History` interaction recorded `openedRuns:["evidence-qa-run"]`; no native navigation was invoked. |

## Library states

| Scenario/theme | Result | Visible result and IPC evidence |
| --- | --- | --- |
| `queue-cleared`, light | Pass, seeded state | All memories showed the approved memory with `Provenance`, recorded quote, source label, configured node, `Available`, and `View source run`; candidate list was empty while canonical memory remained. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963517331-21551.png` |
| `removed`, dark | Pass | Library showed `Source history removed.` and retained the approved memory; candidate evidence had `source_removed` and null quote/digests. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963518430-21551.png` |
| `manual`, light | Pass | Library showed `Written manually. No run citation is expected.`, source `Written by you`, and `manual` provenance. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963519528-21551.png` |
| `legacy`, dark | Pass | Library showed `No verified curation citation is available for this memory.`; candidate was `legacy_unavailable` and blocked. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963521230-21551.png` |
| `evidence-failure`, light | Pass | Library showed `Evidence could not be loaded.` and `Try again`; clicking the discovered button issued a second `list_memory_evidence` call and retained the error state. Screenshots: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963522328-21551.png`, `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788963541379-21551.png` |

The initial edit-form tap on the textarea focused the surrounding details section in Argent. Keyboard focus traversal from the title input reached the textarea and checkbox reliably, so the edited Save flow above is an actual keyboard-accessible interaction. No coordinate was inferred from screenshots; every tap used the current Argent `describe` frame.

Visual finding: the edit screenshot shows the reconfirm checkbox and label over the top of the Content textarea, obscuring the first line of edited content. The Argent-described textarea frame also overlaps the checkbox row, and direct taps inside the textarea can focus the surrounding details section or toggle the checkbox. This is recorded for the owner; the QA fixture was not changed to mask it.

## Follow-up

The owner identified a 049 semantic gap requiring a retest after the next fixture/product update: queue-cleared must preserve and display canonical subject/inference, and candidate evidence must always display host origin/source label. This report records the pre-fix run; those fields were not treated as accepted until rechecked. Backend step deletion and configured-evidence-to-personal-preference behavior remain outside this synthetic UI proof.

## Focused-fix retest

After the container and fixture updates, Argent DOM geometry passed in both themes at a 1440×913 viewport. Light and dark edit forms both measured the Content textarea at `x=645,y=476.875,w=682,h=280,bottom=756.875` and the reconfirm row at `x=645,y=772.875,w=682,h=18.1875`; `overlap:false`. The dark textarea accepted a real Argent tap at its current described frame and keyboard text (`This workflow requires human approval before publishing. reviewed`); the checkbox remained unchecked until explicitly selected, then Save became enabled. Screenshots: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788964876101-21551.png` (light), `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788964905184-21551.png` (dark).

Queue-cleared library now passes in both themes. The actual library text shows `Evidence category: Workflow configuration · Inference — the approved claim goes beyond its exact quotes.` followed by both `Release Instructions · Configured Workflow Instruction · Step Configured-Step-0` and `Notification Instructions · Configured Workflow Instruction · Step Configured-Step-1`, each with its quote and source-run action. The fixture canonical memory contains `evidenceSubject:"workflow_configuration"` and `evidenceIsInference:true`, with no pending candidates. Screenshots: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788964998884-21551.png` (light), `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788965000001-21551.png` (dark).

The remaining direct editor and Save-failure draft retest is paused because Argent reports the root-owned CDP window is hidden/minimized: `gesture-scroll` returns “Cannot scroll: the Chromium window is hidden (minimized or fully occluded)”. This prevents honest nested scroll and tap verification for the rest of the edit form. The workflow-configuration editor option check is therefore not claimed from source inspection.

## Coordinator editor-layout correction

Root fixed the overlap with feature-local nonshrinking flex rules on
`.memory-suggestion-editor` and its content field. Shared form/modal defaults and
search styles were not changed. Frontend build passed; log:
`/tmp/alfred-049-editor-layout-build.log`.

Root then used Argent to open the light `edit-failure` fixture, select the
candidate, open Edit, and tap the textarea at its freshly described center.
The actual textarea was 280px tall, from y=476.875 to y=756.875; the confirmation
row began at y=772.875, leaving a 16px gap. The form uses `flex: 0 0 auto`, the
detail area scrolls (683px viewport / 984px content), and the direct tap focused
`TEXTAREA` with accessible name `Suggestion content`. Fixture errors were empty.
Screenshot:
`/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788964289521-21551.png`.

Luna subsequently confirmed geometry and canonical qualification in both themes,
as recorded above. Direct editor option interaction and the post-fix Save-failure
retest still need completion.

Root restarted the owned Chrome QA process with a visible window and selected
its only tab. Argent still reported `document.visibilityState === "hidden"`.
The browser process is now PID 6134, using the same isolated temporary profile
and CDP port 9222. The tester is attempting keyboard traversal or a separate
computer-use QA tab to finish the remaining interactions. Existing user tabs
are outside this test.

Root independently inspected the queue-cleared light screenshot ending
`1788964998884-21551.png` and dark editor screenshot ending
`1788964905184-21551.png`. Canonical category/inference and both host-origin
rows are visible, and the editor confirmation row clears the textarea. These
focused visual checks are accepted; the remaining interaction checks are
tracked separately.

## Focused CUA completion

The hidden-window limitation above was resolved with a separate, visible CUA
Chrome tab (tab `461987375`, session `🧪 Alfred QA`) pointed at the same isolated
Vite fixture. No user tabs, native app, database, or provider were touched.
The following are actual AX-discovered interactions and visible states, rather
than source-only checks.

- Both light and dark `edit-failure` forms were opened through the suggestion's
  `Edit` action. The Scope control exposed only `workflow`; the Type control
  exposed `fact`, `decision`, `constraint`, and `note`. No personal type or
  scope choice was offered. The suggestion detail visibly showed
  `Evidence category: Workflow configuration` and the source row
  `Release Instructions · Configured Workflow Instruction · Step Configured-Step-0`.
- Light and dark successful edits accepted changed title/body text, required an
  explicit reconfirm checkbox, and showed `Suggestion updated.` after Save.
  The dark run's visible diagnostics panel recorded
  `update_memory_candidate` with `{id:"evidence-qa-candidate", title:"Dark saved title", body:"Dark saved body", evidenceConfirmed:true}`;
  `errors` was `[]`, the candidate was updated and `evidenceState:"verified"`,
  and canonical memory remained present.
- Light and dark failed Saves showed the precise alert
  `The recorded evidence is unavailable or changed. Reject this suggestion or run the workflow again.`
  The editor stayed open with the edited draft and checked acknowledgement
  still visible, while the candidate remained pending. In the dark run, the
  visible diagnostics panel recorded the attempted payload with
  `{id:"evidence-qa-candidate", body:"Dark failed draft", evidenceConfirmed:true}`;
  `errors` was `[]`, and the candidate title/body plus canonical memory were
  unchanged. This verifies draft retention and rollback at the fixture UI
  boundary.

These CUA interactions complete the focused editor, category/source-label,
successful Save, and failed Save-retention checks. The fixture evidence remains
synthetic and does not prove native transport, Rust handler behavior, or real
database retention.
