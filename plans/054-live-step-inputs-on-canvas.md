# Plan 054: Live step inputs and outputs on the workflow canvas

## Status and scope

- Status: IN PROGRESS. Implementation and automated verification completed
  2026-09-16; actual Tauri desktop visual verification remains pending.
- Priority: P1. Effort: L.
- Requested on: 2026-09-16.
- Baseline: desktop commit `4894951` with Plan 053 conditional-connections work in progress in the working tree.
- Depends on: the workflow runner, live run-event bridge, per-workflow run state, canvas node result preview, output modal, and run history.
- Integration dependency: land or stabilize [Plan 053](053-conditional-connections.md) before editing the runner loop. Both plans change `src-tauri/src/runner/mod.rs`, `src/features/workflow/store.ts`, and `src/features/workflow/types.ts`.

Implementation checkpoint, 2026-09-16:

- Added versioned, bounded, display-safe live input snapshots and persisted the
  same values to run History.
- Added per-workflow input state, the canvas Input/Output result card, the full
  step-data viewer, and run-panel input access.
- Added HTTP/header redaction, URL-query redaction, UTF-8 truncation, store
  isolation, History formatting, and focused component/helper tests.
- `bun run check` passed, including the frontend suite/build and Rust suite.
- Pending: inspect real agent and local-action runs in the packaged Tauri UI in
  both themes and reduced motion before changing this plan to DONE.

Show what each workflow step actually received, not only what it produced. During
a run, a node gains one compact result surface with `Input` and `Output` tabs.
Input appears as soon as the runner has finished preparing the value and before
the main provider, command, request, notification, file write, or other node
effect starts. An Input node's optional preparatory script is the one exception:
its output becomes part of the effective input, so the final Input snapshot can
only appear after that script finishes. Output stays in a stable `Working…`
state until output exists.

The selected product decisions are:

1. **Input means effective runtime input**, not only the configured node fields
   and not only the output of the directly connected predecessor.
2. **Input and Output share one compact tabbed result card** on the node.
3. **Input appears live**, once its exact display-safe snapshot is ready.
4. **Every executed node with meaningful runtime input participates.**
5. **Output remains present but disabled as `Working…`** until content arrives.

This plan does not add branch-local context, change execution order, replay old
runs on the canvas, expose credentials, or replace the detailed History screen.

## Why the current UI is insufficient

`NodeOutputPreview` reads `stepOutputs[nodeId]` and renders only after output is
available. Its full-view action writes an output-only object to `selectedOutput`,
which `OutputModal` renders. The live `RunEvent` contract has `output` but no
input field, so the frontend cannot show a running step's effective input.

History already persists an `input_json` value for every recorded run step and
renders it under an `Input` disclosure. That value is not a sufficient live
contract:

- it is available only after `insert_step`, which currently runs after success
  or failure;
- agent and custom-agent inputs are the final composed provider prompt, while
  many local steps persist only the accumulated context summary captured near
  the start of the node;
- it does not identify the input shape or provide a safe compact summary;
- it cannot update the canvas while a provider or command is still running.

The implementation must create one canonical snapshot and use it for both the
live event and persisted History. It must not independently reconstruct input in
React from graph configuration, predecessor outputs, or current store state.

## Terminology and invariants

### Effective runtime input

The display-safe value the runner has fully resolved immediately before the
step's irreversible or externally visible work begins.

Examples:

- for an agent, the exact composed prompt dispatched to the selected harness,
  including eligible context, identities, report instructions, and included
  memory;
- for a script, its stdin plus the Alfred context/output environment values and
  the resolved invocation metadata;
- for an HTTP node, the rendered method, URL, body, and redacted header list;
- for a template, the incoming context/output and the configured transformation
  that is about to be applied;
- for an Output node, the projected body and disposition flags it is about to
  process.

“Exact” applies after template and memory resolution, but before secret-bearing
transport values are attached. The snapshot is exact enough to explain step
behavior without becoming a credential-inspection surface.

### Input readiness

Input is ready only when every value represented in the snapshot is final. A
`step_started` event is not sufficient for agent nodes because automatic memory
recall, identity injection, and HTML-report instructions are composed later.
The runner emits the input event after composition and before dispatch. For an
Input node with `script.run`, the preparation script runs first because its
stdout and exit note are incorporated into the value passed downstream. Treat
that script as visible preparation rather than claiming the final snapshot
predated it.

### One snapshot per executed step

- Each executed node emits at most one input-ready event per run.
- Skipped nodes emit no input because they were not executed.
- Failed preparation emits no input when no effective value existed.
- A failure after preparation retains the emitted and persisted input.
- A late event from an older run cannot replace the visible run's input.
- Concurrent workflows keep their inputs in separate `workflowRunStates`.
- An input snapshot is immutable after publication. Output and status may
  continue to change.

### Live and History parity

For a new run, the value opened from the canvas must match the corresponding
History step input after the run finishes. Formatting may differ between the
compact preview and full viewer, but the underlying snapshot must be identical.
Older History rows keep rendering their existing unversioned JSON.

## Runtime input snapshot contract

Add a versioned, provider-neutral shape shared by Rust serialization, TypeScript,
the live store, and History:

```ts
export type StepInputSnapshot = {
  version: 1;
  kind:
    | "prompt"
    | "context"
    | "script"
    | "request"
    | "action"
    | "notification"
    | "file"
    | "git"
    | "disposition";
  summary: string;
  body: unknown;
  redacted: boolean;
};
```

Contract rules:

- `summary` is a runner-produced, single-line description for the compact card.
  React must not infer it from arbitrary JSON.
- `body` contains the full display-safe value used by the full viewer and
  History.
- `redacted` is true when a transport field was removed or masked. The viewer
  states this in text; color is not the only indication.
- `version` allows future shapes to fail soft in older frontends.
- Field names use camelCase at the Tauri boundary.
- Unknown `kind` or version values render as formatted JSON rather than hiding
  the input or failing the run.
- Input capture is observational. Failure to serialize or emit the live event
  must not prevent the underlying workflow step from running. Persistence keeps
  its existing database error behavior.

Extend `RunEvent` with an optional `input: Option<StepInputSnapshot>`. Use a
dedicated `step_input` event kind rather than overloading `step_started` or
`step_log`; event order for a normal step becomes:

```text
step_started
step_input
agent_activity / step_log (zero or more)
step_completed | step_failed
```

The event is optional for compatibility with old emitters and already-running
processes during development.

## Node-by-node capture matrix

Capture must occur in the runner, after resolution and before the operation
listed in the final column.

| Node type | Snapshot kind and body | Summary | Capture point |
| --- | --- | --- | --- |
| `input` / legacy `prompt` | `context`: resolved authored prompt, attachment context, and optional input-script result | First meaningful line or attachment count | Without `script.run`, after attachment/instruction preparation and before replacing run context. With `script.run`, after the preparation script finishes and before replacing run context |
| `identity` | No separate Input tab when the node consumes no runtime value; its formatted identity remains Output | — | — |
| `memory` | `context`: selected memory references that will be injected, using the same bounded/rendered representation accepted by prompt composition | Number of selected memories | Split resolution from mutation if necessary: resolve the selected value, capture it, then mutate context exactly once |
| `agent` | `prompt`: exact composed prompt passed to the provider-neutral dispatch closure, plus memory recall status/count as metadata | First meaningful prompt line | In `with_composed_agent_prompt`'s `before_dispatch` callback, before CLI/native dispatch |
| `customAgent` | `prompt`: exact composed prompt handed to `run_custom_agent`, plus prompt mode and memory recall metadata | First meaningful prompt line | In the same pre-dispatch callback used to set `persisted_input_prompt` |
| `chooseOutput` / `output` | `disposition`: projected body and normalized disposition flags | `Publish final result`, `Save to memory`, or pass-through summary | After `project_output_value`, before persistence/final-output mutation |
| `template` | `context`: incoming resolved `context`, incoming resolved `output`, template, mode, and cwd | `Append to context` or `Replace context` | After resolving referenced memory values and before `apply_template_transition` |
| `fileInject` | `file`: resolved paths, with no file contents duplicated before the read | Number of paths | After path validation and before the first file read |
| `gitStatus` | `git`: resolved working directory and `includeDiff` | `Status + diff` or `Status only` | After cwd validation and before invoking git |
| legacy `shell` | `script`: command and cwd; it has no stdin/context transport | First command line | After command/cwd validation and before spawning the shell |
| `script` | `script`: resolved source/invocation, cwd, stdin (`last_output`), `$ALFRED_OUTPUT`, and `$ALFRED_CONTEXT` | Script path or first inline line | After script resolution and complete context rendering, before `run_cmd_with_stdin_env` |
| `appAction` | `action`: provider/action ids and the descriptor-prepared non-secret input; include connection label/id only if already display-safe | Provider and action | Inside `with_prepared_action`, before `execute_action` |
| `http` | `request`: method, rendered URL/body, and the configured headers after redaction. Headers are currently passed as configured, not template-rendered | `METHOD host/path` | After URL/body template rendering and before `run_http_request` |
| `notify` | `notification`: rendered title/body and enabled destinations; webhook URL origin only, never credentials/query secrets | Destination summary | After rendering and before desktop notification or webhook POST |
| `writeFile` | `file`: resolved path and rendered content | Filename and byte count | After path/content resolution and before `write_text_file` |
| `gitHost` | `git`: action, repo, title, body, base/head, and draft | `Create PR` or `Create issue` | After rendering and before invoking `gh` |
| unknown type | No input snapshot; retain current unknown-node behavior | — | — |

The matrix is a contract, not illustrative guidance. Add a direct test for each
row that emits an input.

### Avoiding false duplication

Some source/context nodes naturally produce output identical or close to their
input. Preserve that truth rather than inventing different copy. The tabs still
explain direction:

- Input is what the node consumed or prepared for execution.
- Output is what it produced for downstream steps.

Nodes with no meaningful consumed runtime value, currently Identity and unknown
nodes, render only Output. Do not display an empty Input tab.

## Security, privacy, and size limits

The feature makes already-local execution data easier to inspect, but it must
not broaden secret exposure.

1. Never include resolved credentials from Connected Apps, native accounts,
   environment variables, keychains, authorization stores, or provider tokens.
2. For HTTP headers, preserve safe header names and safe values, but replace
   credential-bearing values with `[redacted]`. At minimum treat
   `authorization`, `proxy-authorization`, `cookie`, `set-cookie`, `x-api-key`,
   and names containing `token`, `secret`, `password`, or `credential` as
   sensitive, case-insensitively.
3. Strip userinfo and sensitive query values from displayed webhook URLs.
4. Do not expose a connection's resolved secret; a non-secret opaque
   `connectionId` may remain where it already appears in persisted metadata.
5. Use a shared Rust redaction helper and direct unit tests. Do not duplicate
   secret-name logic in React.
6. Apply a UTF-8 byte cap to live event snapshots so a large context cannot
   freeze Tauri serialization or canvas state. Preserve the full existing
   persisted value only if current History limits allow it; otherwise define
   one shared bounded representation and record truncation explicitly.
7. A truncated snapshot adds `originalBytes`, `capturedBytes`, and
   `truncated: true` metadata in `body` or a small capture-state extension.
   Truncation must happen at UTF-8 boundaries.
8. Compact previews use the runner's bounded summary and never parse or render
   untrusted HTML.
9. The full viewer uses `<pre>`/text rendering. Existing sandboxed HTML preview
   remains Output-only.

Before implementation, select and document the byte cap beside the existing run
log caps in `store.ts` and add a matching Rust constant. Do not silently rely on
JavaScript string memory or the database's practical maximum.

## Runner implementation

### 1. Add a capture-and-emit helper

In `src-tauri/src/runner/mod.rs` or a focused `runner/step_input.rs` module, add
pure helpers that:

- build a `StepInputSnapshot`;
- normalize a one-line summary;
- redact headers and URLs;
- bound large strings safely;
- serialize the snapshot to the same `Value` persisted by `insert_step`;
- emit `step_input` with run/workflow/node identity.

Keep construction pure where possible. A helper receiving `AppHandle` should
only emit an already-built snapshot.

### 2. Capture before effects

Refactor each match arm only enough to create its final resolved input before
the operation. Do not evaluate templates twice: values shown to the user must be
the same values passed to the operation.

For agent and custom-agent nodes, reuse the final `prompt` supplied to the
existing pre-dispatch callback. Emitting at `step_started` would be incorrect
because recalled memory and upstream identity/report instructions are not final
then.

For concurrent Plan 053 levels, each node builds its snapshot from its
`LevelEntryState`. Do not read a mutable global context after branch execution.
Sibling input snapshots must reflect the same deterministic level prefix that
their actual execution receives.

### 3. Persist the canonical snapshot

Replace the ad hoc `persisted_step_input(prompt, recall...)` path with the
canonical snapshot for newly executed steps. Pass that exact serialized value
to every success, cancellation-after-preparation, and failure-after-preparation
`insert_step` call.

Preparation failures need an explicit rule:

- if enough resolution completed to create an accurate snapshot, emit and
  persist it before returning the failure;
- if the failure is the absence of required configuration or occurs while
  resolving the snapshot itself, persist a small versioned configuration/error
  input only when it is truthful; otherwise retain `{}` and emit no live input.

Do not change old History rows or add a database migration. `input_json` already
stores arbitrary JSON.

### 4. Preserve workflow semantics

Input capture must not change:

- topological/level ordering;
- conditional-edge evaluation and skipped propagation;
- context merge behavior;
- provider selection, account routing, or model resolution;
- memory retrieval and evidence capture;
- command environment/stdin;
- side-effect retries or output persistence;
- cancellation and run-status rollup.

Add ordering assertions proving the input event occurs before a fake provider,
script, request, or action observes execution.

## Frontend state and event handling

Extend `WorkflowRunState` with:

```ts
stepInputs: Record<string, StepInputSnapshot>;
```

Thread it through:

- `emptyWorkflowRunState`;
- `visibleRunFields`;
- store initialization;
- workflow switching and tab cleanup;
- `runActiveWorkflow` and trigger-test resets;
- per-workflow run state;
- `handleRunEvent`.

On `step_input`, write `event.input` only when:

- `nodeId` and the snapshot are present;
- the event belongs to the currently retained run;
- the node has not already published an input snapshot.

The existing stale-run guard must cover input events. A repeated identical event
may be ignored; a conflicting second event should keep the first immutable
snapshot and log a development warning rather than changing what the user saw.

Do not put snapshot bodies into `runLogs`. Inputs have one home in `stepInputs`,
just as step output has one home in `stepOutputs`.

Today, `step_log` events carrying `output` are appended to `stepOutputs`.
Input and Memory nodes use that path for preparatory “loaded into context” data,
which makes a value appear as output before `step_completed`. When dedicated
input snapshots are added:

- change those preparatory events to carry a message without `output`, or route
  them through `step_input`;
- keep genuine streamed output behavior only for event producers that actually
  mean partial step output;
- let `step_completed` remain the authoritative stable Output-tab value;
- keep Plan 053 skip reasons in status/log state rather than presenting them as
  a skipped node's output.

## Canvas result-card UX

Replace `NodeOutputPreview` with a more accurate feature-specific component,
for example `NodeRunResult`, colocated with:

```text
node-run-result/
  node-run-result.tsx
  node-run-result.test.tsx
  index.ts
```

Every current `NodeOutputPreview` consumer migrates to the new component:

- Input/Prompt node;
- Agent node;
- Memory node;
- Output node;
- all `SimpleStepNode` consumers, including App Action and Identity.

Delete the obsolete component only after checking no references remain.

### Card states

| State | Card behavior |
| --- | --- |
| Node has not started | No run-result card |
| Running, input not ready | Existing live activity control; no empty result card |
| Running, input ready, no output | Card appears with Input selected; Output tab remains visible, disabled, and labelled `Working…` |
| Running with streamed output | Keep Output disabled until the step has a stable terminal output; streaming activity remains in the console |
| Completed with both | Input and Output tabs enabled; preserve the user's current selection for that node |
| Completed with output only | Render output-only card without a fake Input tab |
| Failed after input | Input stays available; Output tab shows `Failed` only when no output body exists; the run panel owns detailed errors |
| Skipped | No Input/Output card because the node did not execute; skipped styling comes from Plan 053 |
| Cancelled after input | Input stays available; Output shows `Cancelled` when empty |

The selected tab is component-local UI state. Reset naturally when the node
unmounts or a new run clears both records. Do not persist tab choice in the
workflow graph or Zustand store.

### Interaction and accessibility

- Use a real tablist with two buttons carrying `role="tab"`,
  `aria-selected`, `aria-controls`, and roving or natural tab order.
- Left/Right arrows switch enabled tabs; Home/End are optional only if the
  shared tab pattern already supports them.
- A disabled Output tab cannot receive activation while still exposing the
  `Working…` text to assistive technology.
- The preview body is a separate button that opens the full viewer. Do not nest
  buttons.
- Preserve `nodrag nopan`, pointer propagation guards, and
  `data-prevent-node-double-click`.
- Keep the card height stable when switching tabs. Clamp both previews to the
  same number of lines.
- Input and Output cannot be distinguished by color alone; their visible tab
  labels remain present.
- Keep the existing live-console control available while the node is running.
- Verify keyboard operation while the React Flow node itself is selected.

Use semantic design-system tokens from `src/styles/tokens.css`. Resolve every
font size, gap, padding, margin, radius, control state, and surface to documented
tokens. If unique node-card geometry needs a literal, add the required
`geometry:` comment. Support hover, pressed, focus-visible, disabled, selected,
failed, and reduced-motion states without layout shifts.

## Full step-data viewer

Generalize the output-only selection and modal rather than adding a second
nearly identical modal.

Suggested store shape:

```ts
type SelectedStepData = {
  nodeId: string | null;
  title: string;
  initialTab: "input" | "output";
  input?: StepInputSnapshot;
  output?: string;
};
```

Rename `selectedOutput`, `openOutput`, and `closeOutput` only if the migration
can be completed atomically across all consumers. Otherwise add a new
`selectedStepData` API and remove the old API in the same change; do not leave
two modal state paths.

The viewer:

- opens on the tab selected in the node card;
- allows switching between available Input and Output;
- copies the currently visible body;
- renders input text or formatted JSON safely;
- shows `Some sensitive values were redacted` when `redacted` is true;
- shows truncation metadata when applicable;
- keeps agent statistics and files-changed sections attached to Output only;
- keeps HTML report preview/source behavior attached to Output only;
- updates its title/description to `Step input and output`, not `Agent output`;
- preserves focus trap, Escape close, focus return, and shared Modal structure.

The run activity panel should open this same viewer for a completed step. Change
its step action from output-only clickability to input-or-output availability.
While a step is running, the panel may open Input as soon as it exists even
without output.

## History compatibility

History remains the durable source for past runs.

- Teach `formatHistoryJson` or a new formatter to recognize snapshot version 1
  and display its body clearly.
- Keep the existing `<details><summary>Input</summary>` structure unless a
  broader History redesign is explicitly requested.
- Render old `{ "prompt": ... }`, arbitrary JSON, null, and malformed fallback
  values exactly as today.
- Search indexing continues indexing the canonical persisted snapshot. Confirm
  wrapper keys such as `version`, `kind`, and `summary` do not overwhelm useful
  prompt/body matches; adjust `index_run_step` extraction only if a focused
  search test demonstrates regression.
- A skipped step has no runtime input. Its History Input section should state
  `Not executed` or stay absent, consistent with Plan 053.

## Files expected to change

Backend:

- `src-tauri/src/runner/mod.rs`
- optionally `src-tauri/src/runner/step_input.rs`
- runner unit/integration tests colocated in the existing test module
- `src-tauri/src/db/history.rs` only if versioned formatting/search extraction
  requires backend changes

Frontend contracts and state:

- `src/features/workflow/types.ts`
- `src/features/workflow/store.ts`
- `tests/store.test.ts` for run-event and per-workflow-state behavior
- keep `src/features/workflow/store.test.ts` focused on its existing
  workflow/schedule persistence concerns

Canvas and viewer:

- new `src/features/workflow/components/node-run-result/`
- every current `NodeOutputPreview` consumer
- `src/features/workflow/components/output-modal/` or a renamed
  `step-data-modal/`
- `src/features/workflow/components/run-activity-panel/`
- `src/App.css` or colocated feature CSS according to the established component
  ownership at implementation time

History:

- `src/features/workflow/components/history-page/`

Documentation:

- this plan's status and checkpoints
- user-facing workflow documentation describing Input versus Output
- `plans/README.md` only after the plan is accepted for indexing

## Delivery tracks and STOP conditions

### Track 1: Contract, pure snapshot builders, and redaction

1. Add Rust and TypeScript snapshot types.
2. Add pure summary, redaction, URL sanitization, and byte-bound helpers.
3. Unit-test Unicode boundaries and every sensitive-field rule.
4. Add optional `input` to `RunEvent` without emitting it.

**STOP:** Type/build checks pass, old events deserialize unchanged, and no
execution behavior has changed.

### Track 2: Runner capture and History parity

1. Implement capture points from the node matrix.
2. Emit one `step_input` before each effect.
3. Persist the same snapshot through every terminal outcome.
4. Prove Plan 053 level snapshots and skip behavior remain unchanged.
5. Confirm old History rows still load.

**STOP:** Direct tests cover every emitting node type, event-before-effect
ordering, success/failure/cancellation, fan-out siblings, and live/persisted
identity.

### Track 3: Store and full viewer

1. Add per-workflow `stepInputs`.
2. Handle resets, stale events, concurrent workflows, and immutable duplicates.
3. Generalize the output modal into one step-data viewer.
4. Update the run activity panel to open available input or output.

**STOP:** Store tests prove run isolation and event ordering; viewer tests prove
copy/format/tab behavior without canvas integration.

### Track 4: Canvas result card

1. Build the Input/Output tabbed card.
2. Migrate every node consumer.
3. Remove the obsolete output-only component and CSS.
4. Add running, complete, failed, cancelled, skipped, and output-only tests.

**STOP:** No `NodeOutputPreview` references remain, keyboard behavior passes,
and card geometry is stable at supported canvas zoom levels.

### Track 5: History, docs, and desktop verification

1. Add version-aware History display and search regression tests.
2. Document runtime Input semantics and redaction.
3. Exercise real workflows in the Tauri app.
4. Update this plan's status with exact test and visual evidence.

**STOP:** All acceptance cases below pass in both themes and the full check is
green.

## Acceptance tests

| Scenario | Expected result |
| --- | --- |
| Agent with authored input only | Input tab appears before provider activity and exactly matches the dispatched prompt |
| Agent with selected, pinned, and recalled memory | Input includes only memory that composition actually included; History matches canvas |
| Agent with upstream Identity and HTML report request | Final injected instructions appear once in Input |
| Custom agent using template mode | Input shows the final prompt, while command transport details remain display-safe |
| Script after an agent | Input shows exact stdin/`ALFRED_OUTPUT` and context values before the process starts |
| HTTP body using `{{context}}` and `{{output}}` | Input shows rendered values; authorization and secret-like headers are redacted |
| Connected App action | Input shows descriptor-prepared non-secret fields and never resolved credentials |
| Notify webhook with sensitive URL query | Input identifies the destination without exposing sensitive query values |
| Write file | Input path/content matches the bytes passed to the write operation |
| GitHub PR/issue | Input shows rendered title/body/base/head/draft before `gh` executes |
| Running step with input and no output | Input is selected; Output is visible, disabled, and says `Working…` |
| Streamed script/agent activity | Output tab does not oscillate with partial console lines; console still streams |
| Failure after preparation | Input remains inspectable; History persists the same snapshot |
| Failure before preparation | No misleading Input appears |
| Conditional branch skipped | No Input event/card is created for the skipped node |
| Two parallel Plan 053 branches | Each input reflects the same deterministic level prefix and its own prepared values |
| Join after one skipped branch | Join input reflects the actual merged live branch state and the node runs once |
| Two workflows running concurrently | Inputs and selected full-view data never cross workflows |
| Late event from previous run | Current canvas input is unchanged |
| Large Unicode prompt | Snapshot truncates at a valid UTF-8 boundary, reports truncation, and the app remains responsive |
| Old History row | Existing unversioned input JSON still renders |
| Keyboard-only canvas | Tabs, preview opener, full viewer, copy, and close are reachable with visible focus |
| Light/dark and reduced motion | Text remains legible, states are not color-only, and no layout motion is required |

## Verification matrix

Focused frontend:

```sh
bun test src/features/workflow/components/node-run-result
bun test src/features/workflow/components/output-modal
bun test src/features/workflow/components/run-activity-panel
bun test src/features/workflow/components/history-page
bun test tests/store.test.ts
```

Focused Rust:

```sh
cd src-tauri
cargo test runner::tests::step_input
cargo test runner::tests::conditional
cargo test db::history
```

Repository gates:

```sh
bun test
bun run build:frontend
bun run check
git diff --check
```

Desktop verification must use `bun run dev` and the actual Tauri application.
At minimum run one agent workflow and one local/script/action workflow. Inspect
Input before Output exists, compare the final canvas input with History, and
repeat in both themes. Static source inspection and standalone component
previews do not satisfy this gate.

## Risks and mitigations

### Capturing too early

An early snapshot omits memory, identity, templates, or rendered values and
misleads the user. Capture at the last point before dispatch/effect, and assert
event ordering with fakes.

### Capturing too late

Emitting only after `insert_step` defeats the live requirement and loses input
on long-running or failed steps. Emit before the effect and persist the same
immutable value later.

### Secret exposure

Node-specific transport data can contain tokens even when prompts do not. Build
allowlisted display snapshots and shared backend redaction; never serialize raw
credential-bearing request objects.

### Memory and UI pressure

Exact prompts can be large, and `stepInputs` is retained per open workflow.
Bound event payloads, clear them with run state, avoid duplicating them in logs,
and render only a short summary on the canvas.

### Divergence between live and durable views

Separate live and History builders will drift. Build once in Rust, emit that
snapshot, and pass the same serialized value to `insert_step`.

### Plan 053 merge conflicts

Conditional execution is currently restructuring the runner into deterministic
levels and adding skipped events. Do not implement this plan against the old
sequential loop and then manually merge both designs. Stabilize Plan 053 first,
then capture from each node's `LevelEntryState` and rerun all conditional
acceptance cases.

## Completion definition

The plan is complete when a user can inspect the exact display-safe runtime
input of every meaningful executed step while it is still running, switch
between Input and Output from one compact canvas card, open the same data in a
full viewer, and later see the identical snapshot in History—without exposing
credentials, changing workflow semantics, or mixing concurrent runs.
