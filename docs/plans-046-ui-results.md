# Plan 046 UI QA results

Tested 2026-09-08/09 on the isolated QA frontend with the synthetic IPC fixture in
`tests/ui/memory-plans.html` and `tests/ui/memory-plans.tsx`. The source snapshot at
the start of the bounded run was `85f0d64e23e55e655c9b6939aef802c5eb970ae2`;
the fixture files and this result file are working-tree additions.

## Test boundary

- Target: Argent Chromium CDP device `chromium-cdp-9222`, Chrome 152, isolated tab
  `plan046-qa`, `http://127.0.0.1:1422/tests/ui/memory-plans.html`.
- The fixture supplies existing-memory context, a candidate, a failed review job,
  history detail, supported/unavailable provider metadata, and a retry response.
- No user database, provider process, native packaged app, account mutation, or
  publication flow was used. Browser fixture behavior does not prove native
  transport or live provider activation.

## Scenarios and verdicts

| Scenario | Expected | Observed | Verdict/evidence |
| --- | --- | --- | --- |
| `off` | Review is opt-in; bounded disclosure and fixed instructions are visible. | Global settings showed review off, candidate-only behavior, a bounded digest and existing-memory context, and explicit limits excluding workspace/tools/skills/session data. | **Pass**; dark settings screenshot `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788912892544-21551.png`. |
| `supported` | Supported provider can be selected; acknowledgement gates Save. | `Available: Claude Code 2.1.263` rendered; enabling the global switch exposed the acknowledgement and left Save disabled. | **Partial pass** for disclosure/gating; **fail** for acknowledgement interaction below. Screenshot `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788912843733-21551.png`. |
| `workflow-supported` | Workflow suggestion control is available and can be enabled. | Memories settings showed enabled `Suggest memories after runs`; toggling it on changed the sidebar state to `Suggestions On`. | **Pass**; screenshot `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788919755488-21551.png`. |
| `unavailable` | Unreviewed provider/version is unavailable, gives a precise reason, and prevents provider calls. | Global settings showed `Unavailable: installed cli version not verified. No provider call will be made.` in dark and light themes. Workflow settings showed the same reason and a disabled Suggest memories switch. | **Pass** for fixture-backed UI; dark screenshot `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788913006475-21551.png`, light `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788913021815-21551.png`, workflow `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788919886530-21551.png`. |
| `failure` | Failed review is visible, retry is offered, and history preserves run/output/context. | Suggestions showed `Review failed`, the restricted-review/no-provider-call message, and `Retry review`. After Retry, the queue settled to `No memory suggestions yet`, consistent with the fixture's pending retry response. History detail showed `Review failed`, the same alert, completed run status, synthetic input/output, and one existing pinned-memory context item. | **Pass** for visible failure/retry/history states; before retry `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788920129215-21551.png`, after retry `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788920133988-21551.png`, history detail `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788920030904-21551.png`. The CDP endpoint became unavailable immediately after the retry screenshot, so no post-action IPC call log was collected. |

## Product finding

The acknowledgement checkbox in `src/features/settings/components/settings-page/settings-page.tsx`
crashes the React tree when clicked. Argent showed a blank page and the captured
fixture error was:

```text
TypeError: Cannot read properties of null (reading 'checked')
at http://127.0.0.1:1422/src/features/settings/components/settings-page/settings-page.tsx:1188:57
```

The handler reads `event.currentTarget.checked` inside a functional state updater;
React invokes that updater after the event has been cleared. This blocks the
acknowledgement and Save path, including the `save-failure` rollback scenario. The
finding is in product code, independent of the synthetic IPC fixture.

## Validation and limits

`bunx tsc --noEmit -p tests/ui/tsconfig.json` passed. Both dark and light themes
were inspected for the changed settings states; text and long unavailable/error
messages remained readable without observed clipping at the QA viewport.

The fixture gives bounded UI evidence only. Live activation remains untested and
is intentionally blocked by the installed `Claude Code 2.1.265` review restriction;
native IPC transport, real provider calls, and packaged-app deep-link behavior still
require a separately approved environment.

## Bounded retest after acknowledgement/provider event fixes

Retested 2026-09-09 against the same isolated Chromium target (`chromium-cdp-9222`, Chrome 152, `http://127.0.0.1:1422/tests/ui/memory-plans.html`) with the working tree at `85f0d64` plus the current fixes in `settings-page.tsx`. The original acknowledgement crash finding above is retained; this retest did not reproduce a blank page.

- **Supported dark Save:** enabling the global switch, selecting the available `claude_code` provider, checking the acknowledgement, and activating Save kept the page rendered and returned the control to its saved/off state. The synthetic call log contained `update_memory_review_settings` with `{input:{enabled:true,provider:"claude_code",model:null}}`. The acknowledgement click itself also completed without a React error. The available-provider fixture contains one option, so keyboard navigation left the selected provider at `claude_code`.
- **Save failure, dark:** the live Save action completed without a crash and rendered `Memory review settings could not be saved.`. Argent screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788929344947-21551.png`. Debugger state after the failed call was `scenario=save-failure`, `theme=dark`, `provider=claude_code`, global switch `aria-checked=true`, acknowledgement `checked=true`; the call log recorded `update_memory_review_settings` with the same payload and `errors=[]`.
- **Save failure, light:** switched the fixture theme with the keyboard and repeated the live Save action. The light page stayed rendered and showed the same precise error; screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788929426468-21551.png`. The light-theme state likewise kept the global switch and acknowledgement checked after failure. Theme-switch evidence: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788929407428-21551.png`.

The requested failed-save rollback is therefore **not observed**: the draft remains visibly enabled and acknowledged after the rejected update. This matches the current store catch path, which clears `saving` and sets the error but does not restore the prior draft. Browser fixtures prove the rendered behavior and synthetic command payload only; they do not prove native Tauri transport or persistence.

## Bounded retest after failed-save rollback fix

Retested 2026-09-09 on the fresh isolated Chromium target (`chromium-cdp-9222`, Chrome 152, `http://127.0.0.1:1422/tests/ui/memory-plans.html`) at source snapshot `85f0d64e23e55e655c9b6939aef802c5eb970ae2` plus the current `settings-page.tsx` fix and the scoped fixture additions. This run exercised the live rendered UI through Argent discovery before each tap.

- **Save failure, dark — pass after fix:** In the `save-failure` scenario, enabling review, acknowledging the disclosure, and pressing Save left the page rendered, kept `Memory review settings could not be saved.`, and restored the persisted off state: the global switch was off and the acknowledgement was gone. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788930455619-21551.png`.
- **Save failure, light — pass after fix:** Repeated the same flow in the light theme. The precise error remained visible while the global switch returned off and the acknowledgement disappeared. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788930573196-21551.png`.
- **Supported Save, light — pass:** In the `supported` scenario, the acknowledgement checkbox no longer crashed the React tree. With `claude_code` selected, enabling the switch, checking the disclosure, and pressing Save kept the page rendered and returned the UI to its saved state. The debugger call log recorded `update_memory_review_settings` with `{input:{enabled:true,provider:"claude_code",model:null}}`; there were no alerts or fixture errors. Screenshot after Save: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788930705485-21551.png`.
- **Already-enabled model edit, light — pass:** With the saved enabled state visible, editing the model to `claude-sonnet-4` and pressing Save kept the page rendered and produced the expected latest synthetic command payload `{input:{enabled:true,provider:"claude_code",model:"claude-sonnet-4"}}`; `aria-checked` remained `true`, Save was enabled, and alerts/errors were empty. Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788930760176-21551.png`.

The fixture's `get_memory_review_settings` response remains scenario-based and static, so this bounded run verifies same-page Save state and the exact synthetic IPC payloads; it does not claim reload persistence, native Tauri transport, or real provider activation. `bunx tsc --noEmit -p tests/ui/tsconfig.json` and `git diff --check` passed for the scoped fixture/report changes.
