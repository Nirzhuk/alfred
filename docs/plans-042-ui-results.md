# Plan 042 sharing UI QA

Tested 2026-09-09 against the implementation working tree based on `85f0d64` in the root-owned isolated Vite fixture. The run used Argent on Chrome 152 (`chromium-cdp-9222`) with `http://127.0.0.1:1422/tests/ui/sharing-plans.html`; the user app at port 1420 and native app state were untouched.

The fixture uses synthetic IPC only. It proves browser behavior and request construction, not native IPC transport, backend event configuration, provider calls, publication service behavior, or sharing security.

## Matrix

- **Share, light, delayed: pass.** `Make reusable` retained the public-example checkbox and showed the Release message editor. Empty editor kept Continue disabled with the precise alert `Enter a public example for Release message.` Typing `Release notes ready` enabled Continue. Review JSON retained that text in message nodes `a` and `b`, removed the recipient question for that value, kept the start disabled, and omitted private publisher values. Create link showed the link; Revoke removed it. The fixture recorded `prepare_workflow_publication`, `publish_workflow_template`, `list_workflow_publications`, `list_pending_workflow_publications`, and `revoke_workflow_publication` with the expected share ID and no errors.
- **Import, light, delayed: pass.** The account picker exposed only compatible `Work (Claude Code)`; expired and other-provider accounts were absent. Team and Partner connection selectors were independent. Team action/event resources were pending and released independently; Partner action resources were then released. Team and Incoming selected `team-channel`, Partner selected `partner-channel`. Browse opened the canonical directory picker payload `{directory:true,multiple:false,title:"Choose a local folder"}` and populated `/tmp/recipient-notes`. Create copy emitted `check_workflow_template_setup` followed by `import_workflow_template`, with the three resource mappings, folder, release message, source link, synthetic digest, UUID attempt ID, and disabled start. UI ended with `Dialog closed.` and no errors.
- **Stale same-connection response: pass.** While Team resources were pending, switching to Alternate left four pending requests. Releasing the stale Team pair left Alternate selected and its resource controls loading; releasing Alternate populated only `alternate channel`. No stale Team value appeared and no errors were recorded.
- **Resource failure, light: pass.** Selecting Team surfaced `Synthetic resource lookup failed` alerts for both Team and Incoming channel controls, kept those controls disabled, and exposed reload actions. No unhandled fixture errors were recorded.
- **Revoked link, dark: pass.** Import displayed the precise alert `This shared workflow has been revoked.` without entering setup. `document.documentElement.dataset.theme` was `dark`; body colors were `rgb(16, 16, 16)` and `rgb(252, 252, 252)`.
- **Existing imported copy, dark: pass.** After selecting Work, Team, Partner, all three resources, `/tmp/existing-notes`, and `Existing copy note`, Create copy displayed `This link is already imported as a local copy. Alfred kept that existing workflow and did not create another one.` The fixture recorded setup validation and import payloads, with no errors.
- **Publish failure, dark: pass.** The full share flow reached Review and share; Create link surfaced the exact `Synthetic sharing service unavailable` alert while leaving the reviewed content visible and the Create link action available. The outgoing payload contained the typed `Failure test message` in both intended message nodes and preserved the disabled start.
- **Keyboard and overflow: pass for exercised controls.** Focus traversal reached account, connection, grouped action/event resource controls, folder/message inputs, modal navigation, and final actions. Both share and import modal bodies were scrollable; no clipping or overflow error appeared in the exercised light and dark states.

## Evidence

Argent screenshots are preserved in the temporary media directory:

- Share light: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788933684672-21551.png` (post-revoke), `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788933641642-21551.png` (review payload), `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788933636167-21551.png` (typed editor).
- Import light: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788934266775-21551.png` (created copy).
- Resource failure light: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788934444342-21551.png`.
- Revoked dark: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788934461910-21551.png`.
- Existing-copy dark: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788934655422-21551.png`.
- Publish-failure dark: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788934860366-21551.png`.

No production files were changed by this QA run. The fixture is intentionally synthetic and must not be cited as proof of native transport or backend correctness.

## Coordinator screenshot review

The behavior checks above passed, but root's independent inspection of the
light-theme editor screenshot found checkbox inputs stretched across the card.
The publish-failure screenshot also left the error outside the visible scroll
area. Root restored the shared compact checkbox width in this feature's checkbox
selector and scrolls command errors into view. These local UI corrections need
a focused browser retest; the preceding screenshots remain evidence of the
pre-correction appearance. Shared search and general modal defaults were not
changed.

## Focused visual retest after sharing.css and error-scroll fixes

Retested 2026-09-09 against the same implementation working tree (`85f0d64` base) with Argent Chrome 152 on `chromium-cdp-9222`. These checks exercised the corrected feature-local checkbox rule and command-error scroll effect; no production files were changed during QA.

- **Share checkbox, light: pass.** From the bottom of the Recipient setup scroll area, the retained-public-example checkbox rendered as a compact square and stayed inline with its label. Computed checkbox bounds were `16×16px`; its label row was `1044px` wide, with `display:grid` and `appearance:none`. Tapping it showed the Release message editor, the empty editor displayed `Enter a public example for Release message.`, and Continue was disabled. Keyboard entry of `Light visual retest` enabled Continue.
- **Share checkbox, dark: pass.** The same actual flow passed in dark theme. Computed bounds were again `16×16px` with a `1044px` label row. Empty retained editor showed the precise alert and disabled Continue; `Dark visual retest` enabled Continue.
- **Publish failure from bottom, light: pass.** Starting with the modal body at its maximum scroll position, Create link returned `Synthetic sharing service unavailable` and the effect moved the alert into the viewport. Computed alert rect was `{x:185,y:203.390625,w:1070,h:45.59375}` in a `1440×913` viewport; modal scroll was `{top:24,h:559,sh:745}`. The reviewed content and Create link action remained available.
- **Publish failure from bottom, dark: pass.** The same flow produced the same precise alert in dark theme with computed rect `{x:185,y:203.390625,w:1070,h:45.59375}` and modal scroll `{top:24,h:559,sh:745}` in the same viewport.
- **Missing required import setup from bottom, light: pass.** With the import body at maximum scroll and all required values empty, Create workflow copy returned the complete required-field alert beginning `Choose Review account...` and scrolled it into view. Computed alert rect was `{x:335,y:160.90625,w:770,h:65.1875}`; modal scroll was `{top:24,h:594,sh:1216}`.
- **Missing required import setup from bottom, dark: pass.** The same required-field alert and viewport placement passed in dark theme with the same computed alert and scroll bounds.

Focused retest screenshots:

- Light retained editor empty: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935264411-21551.png`; typed/enabled: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935283675-21551.png`.
- Dark retained editor empty: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935328388-21551.png`; typed/enabled: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935344577-21551.png`.
- Publish failure light after auto-scroll: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935435085-21551.png`.
- Publish failure dark after auto-scroll: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935530545-21551.png`.
- Missing-required import light after auto-scroll: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935585177-21551.png`.
- Missing-required import dark after auto-scroll: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935624247-21551.png`.

## Public preview browser QA

Retested the root-owned service fixture at `http://127.0.0.1:1423` on 2026-09-09 with Argent Chrome 152. The fixture status endpoint supplied normal share `WCKW24rg63W9OKZRgvZr_Pe-wjRXpHn6`, malicious share `KmHb53iorM_zUs9ceCDLSw0LjQS9bUAp`, and revoked share `KSwg8rCQ10oAB5RZMAWhU2QX6bUSNPou`.

- **Normal preview, dark: pass.** Actual page rendered the title, description, steps, connections, required apps/tools, guidance, and action row. Desktop computed layout was `1440×913` viewport with `main` at `{x:360,y:64,w:720,h:686.875}`. The Open in Alfred, Download Alfred, and Report a problem hrefs were inspected without activation; the custom scheme was not clicked. Copy link was tapped through Argent and visibly changed to `Copied`. The page's `data-copy` value matched the current preview URL; browser clipboard read was denied by permission, so the copied OS clipboard value was not independently read.
- **Normal preview, light: pass.** The same service-rendered page was captured with the CDP emulated `prefers-color-scheme: light` media setting; runtime reported `media:"light"` and black text, and the screenshot showed the complete light layout with no clipping. Argent remained the interaction/discovery path; the raw CDP media override was used only because Argent has no color-scheme emulation control and its attach resets the override.
- **Mobile guidance at 390px: pass.** The page was rendered at a real `390×844` CDP viewport. Argent described the desktop-app guidance `Alfred is a desktop app. Copy this link and open it on your computer.` as visible (`display:block`, `{x:16,y:814.875,w:343,h:48}`). Main content was `{x:16,y:32,w:343,h:942.875}`; body `scrollWidth` was `375` and `clientWidth` was `375`, so there was no horizontal overflow. The expected vertical scroll remained available.
- **Malicious long strings: pass.** The actual malicious preview rendered script, image, and SVG payload strings as literal text nodes. `innerHTML` was escaped, all content nodes were text-node type 3, `document.images` was empty, no `IMG/SCRIPT/SVG/IFRAME/OBJECT` appeared under the preview main content, and `window.previewInjected` remained false. The fixture status endpoint reported `probes:[]`, so the probe URL was never requested.
- **Revoked preview: pass.** The actual response rendered `Template unavailable` and `This publication was revoked or is no longer available.`. Navigation timing reported HTTP status `410`; it contained no scripts or images.

Public-preview screenshots:

- Normal dark desktop after Argent Copy link interaction: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935772485-21551.png`.
- Normal light desktop captured after CDP media emulation: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/alfred-preview-light.png`.
- Normal dark 390px mobile guidance: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935839250-21551.png`.
- Malicious literal payload preview: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788935993183-21551.png`.
- Revoked HTTP 410 preview: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788936023767-21551.png`.
