# Plan 043: Manage schedules through a connected canvas node

## Status and scope

- Status: IN PROGRESS — steps 1–4 implemented; `bun run check`'s parts are green
  separately (`bun test` 647 frontend, `cargo test --lib` 1139 Rust,
  `bun run build:frontend`; `bun run check` itself was run once and flaked only
  in live-process `agents::managed_runtime` tests, which also fail at HEAD under
  the same load). Canvas behaviour was verified in a browser fixture that renders
  the real canvas with mocked IPC (`tests/ui/schedule-canvas.html`): node state,
  generated arrows, drag persistence, enabled-only switch, delete-key routing,
  overview navigation, empty/invalid/disabled/locked states, both themes, and
  zoom 1× / 0.5× / 0.25×. The packaged Tauri app was verified for the real
  database migration, the real read path (node with UTC recurrence and
  zone-labelled next run, `Edit schedule` toolbar, sidebar badge, persisted
  layout applied) — but driving the packaged window's controls (switch, drag,
  form) is still outstanding, and is required before this is DONE. Documentation:
  [canvas scheduling](../docs/canvas-schedules.md).
- Priority: P1. Effort: M.
- Requested on: 2026-09-08.
- Baseline: desktop commit `85f0d64` with unrelated uncommitted canvas, identity-node, runner, menu, and styling work. Preserve that work and recheck it before implementation.
- Dependencies: the existing scheduler, schedule storage/commands, schedule form, workflow canvas, and capability resolver.
- Independent of: [template sharing](042-share-workflows-by-link.md) and [follow-up tracking](044-follow-up-tracker.md). Integrate with sharing when that feature exists.

## Deviations from this plan as written

- **Start arrows are painted over the viewport, not modelled as React Flow
  edges.** React Flow drops an edge whose target node has no target handle, and
  an Input step deliberately has none (`prompt-node.tsx` renders a single
  source handle), so a real edge could not point at the roots this plan exists
  to explain. The arrows are an SVG layer inside the viewport
  (`components/schedule-node/schedule-arrows.tsx`), positioned from the
  measured nodes: they follow a dragged step, cannot be selected, connected,
  deleted, or persisted, and therefore need no reserved edge ids, no edge
  changes routing, and no copy/paste or template exclusions. The reserved
  `schedule-edge:` namespace remains as a guard.
- **Add/Edit lives in the workflow toolbar, not the step picker.** The plan
  allowed either. A schedule is not a graph node, so the palette would have
  needed a second dispatch path for a node that never enters the graph.
- **No graph undo/redo or clipboard exists in this app**, so the plan's
  undo/copy/paste protections are satisfied by construction: there is no history
  to resurrect a timer from. The delete key routes a schedule-only selection to
  Remove schedule and refuses a mixed selection whole.
- **Stale-write detection uses the guard columns `cron` and `enabled`, not
  `updated_at`.** The ticker advances `next_run_at` and bumps `updated_at` on
  every fire, so a revision check would reject an honest toggle that happened to
  overlap a run.
- **No backend entitlement gate was added.** Licensing is contractually barred
  from reaching schedules (`licensing/acceptance.rs` asserts it) and no Rust gate
  exists today, so enable/edit are gated by the same client-side resolver every
  other schedule surface uses, while disable and remove stay available in every
  build.
- **The Schedules overview and the node state the recurrence's UTC basis**
  (`Weekdays at 07:00 UTC`) and show the next run with its zone. The previous
  compact label paired a local clock time with a UTC weekday name.


Show a workflow's schedule directly on its canvas, connected to its starting steps. Clicking the node opens schedule settings. Existing scheduled execution, manual runs, and the global Schedules overview remain available.

This plan adds a visible way to manage the current schedule. It does not add multiple schedules per workflow, trigger-based branch routing, a mid-flow Wait node, a new scheduling engine, or named-timezone recurrence.

## User experience

```text
[Schedule] ──→ [Input] ──→ [Agent] ──→ [Output]
 Weekdays
 Next: Thursday, 09:00 CEST
 [On]
```

The example time is illustrative. All displayed times must come from the actual persisted schedule and include a timezone label.

1. Choose **Add schedule** in the workflow toolbar or step picker. Reuse the existing repeat/day/time/advanced-cron form inside the workflow editor.
2. Saving creates the schedule through the existing backend command. The canvas inserts and connects its node automatically. Cancelling a new schedule creates nothing.
3. The node shows the recurrence, next run with date/time/zone, and enabled state. Clicking its body opens the same settings. Its switch changes enabled state without changing recurrence.
4. Dragging the node changes only its saved canvas position. It does not change when or what runs.
5. **Remove schedule** removes the schedule and its canvas representation. Turning it off leaves the node visible and the schedule editable.
6. Selecting **Open** or **Edit** on the Schedules page opens the workflow and focuses this node; Edit then opens its settings.

If a workflow already has a schedule, Add schedule becomes Edit schedule and focuses the existing node. Manual workflows need no start node and continue to run through Run now. Opening an existing scheduled workflow shows its node immediately, with no user migration step.

Keep the Schedules page as a cross-workflow overview. Do not add a separate schedule model for that page, the context menu, the tray, or the canvas.

## Connection semantics

The existing scheduler starts the whole workflow. Preserve that behavior and make the connections explain it:

- Generate an arrow from the schedule node to every executable root in the saved graph, determined before adding the visual schedule node.
- A single chain gets one arrow. Several disconnected starting branches get several arrows. Starting steps are not silently omitted from scheduled execution.
- These start arrows are generated, non-selectable, and not individually draggable/deletable in v1. The node has no incoming port and its outgoing port does not permit manual branch routing. A tooltip explains that the schedule starts the entire workflow.
- Adding/removing ordinary graph edges recalculates the displayed roots. It does not edit schedule data.
- An empty workflow can show the schedule node without an arrow and with Add a step guidance. Do not fabricate a hidden Input node or rewrite existing nodes.
- Existing invalid/cyclic graphs keep their current execution behavior and diagnostics. Do not repair or reorder them as a side effect of displaying a schedule.

This resolves an important UI ambiguity: attaching a schedule to one branch must not imply that other branches will be skipped when the backend actually runs them.

## Architecture: one schedule record, a canvas representation

Use `schedules` as the canonical source for cron, enabled state, and next run. Render a schedule node from that record. Keep it separate from executable `WorkflowGraph.nodes` and `.edges`.

Suggested contracts:

```ts
type ScheduleNodeData = {
  kind: "schedule";
  workflowId: string;
  scheduleId: string;
};

type ScheduleNodeLayout = {
  scheduleId: string;
  x: number;
  y: number;
};
```

The component reads the workflow-scoped schedule state for its display. Do not serialize another cron/enabled/next-run copy into node data. Give synthetic node/edge IDs a reserved namespace and reject collisions with executable graph IDs.

Add an optional layout table keyed by schedule ID with an `ON DELETE CASCADE` foreign key. Missing layout means deterministic placement to the left of the roots. Opening a workflow computes this default without updating its execution graph or rescheduling it. Persist finite, bounded coordinates only after an intentional drag.

Compose the rendered canvas through a dedicated helper, for example `composeScheduleCanvas(graph, schedule, layout)`. Route changes according to their source:

- Executable node/edge changes continue through existing graph handlers.
- Schedule selection stays in canvas UI state; dragging saves layout through a narrow command.
- Schedule deletion invokes the existing schedule delete path.
- Synthetic edges never enter graph persistence, execution, graph undo history, copy/paste payloads, or template export.

React Flow's render-node union can include the schedule node without adding it to the persisted executable `WorkflowNodeData` union. Add a guard at graph-save boundaries so a generic canvas handler cannot accidentally save synthetic nodes or edges. This also protects ordinary runs, whose topological ordering currently depends on the executable graph.

The same graph must reach the runner before and after adding, moving, disabling, or removing the schedule representation. No new runner match arm or additional run step is required. Existing run history identifies scheduled runs through its trigger kind.

## Keep mutations and loading reliable

The current frontend has both an active `schedule` field and cross-workflow `workflowSchedules` state. `loadSchedule`, `saveSchedule`, and `clearSchedule` can complete after the user has switched workflows. Scope responses by workflow ID and guard active-view assignment so a slow response cannot replace another workflow's node.

Reuse `schedules://changed` for invalidation, and refresh the affected schedule after scheduled runs update its next time. If the existing ticker does not emit a suitable refresh event, add a lightweight post-commit notification or a visible-view refresh on completion/focus. Do not calculate an authoritative next-run time in React.

There is a toggle-specific issue to address: `scheduler::upsert_schedule` currently recomputes `next_run_at` whenever it saves. Add an enabled-only backend mutation so clicking the node switch cannot round-trip cron through the local-time picker. Changing off to on computes the next occurrence from now; changing on to off clears the next occurrence; setting the already-persisted enabled value is a no-op. Cron remains unchanged. Route enabled-only changes from the existing controls through the same behavior.

For each operation, show a pending state, reject duplicate submission, return backend errors at the node/form, and restore the persisted visual state on failure. Recheck existence on write; a delayed toggle/layout write must not recreate a schedule another control removed. Use the schedule ID and expected revision/update value for stale-write detection where necessary.

Keep destructive schedule actions outside generic graph undo/redo. Deleting a schedule-only selection routes to Remove schedule. Reject deletion of a mixed schedule/step selection with guidance to remove the schedule separately, then delete the steps. Do not partially delete that selection. Graph undo must never resurrect or enable a timer.

## Time display and compatibility

The current Rust scheduler evaluates a six-field cron in UTC. The frontend picker converts local wall-clock selections to UTC using a reference date; the stored row does not retain an IANA timezone. A recurrence therefore cannot promise to stay at the same local hour across daylight-saving changes.

For this plan:

- Preserve every existing cron, enabled value, and next-run instant on upgrade/open. Do not infer and save a missing timezone.
- Reuse the picker conversion for deliberate recurrence edits only. Opening, moving, focusing, or toggling the node must not re-encode cron.
- Show the next occurrence in the user's current timezone, with a date and explicit zone. For a recurring rule, show its canonical UTC basis or a correctly converted full local day/time description; do not combine a local hour with an untranslated UTC weekday.
- Add a backend preview command using the same parser as execution for edited/disabled rules. The current JavaScript preview accepts only a subset of cron syntax and must not be treated as authoritative for advanced expressions.
- In schedule settings, explain the stored UTC recurrence and that local time can change with daylight saving. A future named-timezone recurrence migration can improve this without being hidden inside this UI change.
- For disabled, invalid, unavailable, or entitlement-blocked schedules, show the actual state rather than inventing an upcoming run.

Keep existing capability presentation and official/source-build behavior. UI disablement is not an execution gate: validate schedule enable/edit mutations and scheduled execution against the same entitlement policy used by the app at implementation time. Ensure disabling/removing an existing schedule remains available when an entitlement is unavailable. Manual runs remain governed by their existing policy.

## Files and integration points

| Area | Existing location / proposed addition |
| --- | --- |
| Schedule data and layout | `src-tauri/src/db/schedules.rs`, `schema.sql`, `migrate.rs` |
| Schedule mutations/preview/events | `src-tauri/src/scheduler/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` |
| Frontend APIs and workflow-scoped state | `src/features/workflow/api.ts`, `store.ts` |
| Rendering and event routing | `components/flow-editor/flow-editor.tsx`, `components/workflow-canvas/workflow-canvas.tsx`, `components/node-types/index.ts` |
| New UI/helper | `components/schedule-node/`, `schedule-canvas.ts` |
| Form and navigation | `components/schedule-modal/`, `components/schedules-page/`, `components/workflow-context-menu/`, `add-step-items.ts` |
| Time labels | `schedule-label.ts`, `schedule-picker.ts` and their existing tests |

Read `docs/design-system.md` before UI work. Reuse the clock icon, semantic tokens, existing Modal/SelectControl components, node sizing, selection treatment, and keyboard patterns. Keep new CSS with the schedule node. Do not rearrange existing workflow steps to make room for it; adjust initial placement or the viewport instead.

## Delivery steps

### 1. Define canvas composition and preserve the scheduler contract

- Add the render-only node type, deterministic root-arrow composition, layout persistence, and persistence-boundary guards.
- Add backend preview and safe enabled-only mutation semantics, including stale-write rejection and capability checks.
- Verify existing schedules need no cron or execution-graph migration; test old databases and repeated migrations.
- Exit when canvas composition can produce the new node without changing the graph consumed by the runner.

### 2. Build node interaction and reuse the schedule form

- Add the node with settings access, enabled switch, state labels, drag persistence, and explicit removal.
- Add Add/Edit schedule entry points; preserve current custom-cron editing.
- Partition graph/schedule change handling, copy/paste, keyboard deletion, and undo/redo.
- Exit when a user can create, edit, toggle, move, and remove a schedule entirely from the workflow editor.

### 3. Connect overview navigation and runtime refresh

- Make the Schedules page focus the corresponding node and open its settings when requested.
- Keep context menu, node, overview, tray, and next-run display synchronized through workflow-scoped state.
- Handle slow loads, workflow switching, schedule deletion from another control, and run completion refresh.
- Exit when every entry point edits the same canonical record and no stale response changes another workflow's display.

### 4. Verify compatibility and integrate sharing

- Execute the matrix below in the actual Tauri desktop UI as well as focused automated tests.
- If Plan 042 exists, export the canonical schedule recipe separately, omit the synthetic node/edges, and reconstruct the node from the recipient's disabled schedule. Transfer optional layout only through the template's reviewed presentation contract.
- Update user documentation to describe canvas scheduling, UTC recurrence, and the cross-workflow overview.
- Run final checks and record the tested desktop build.

## Acceptance tests

| Scenario | Expected result |
| --- | --- |
| Existing enabled schedule | Same cron, enabled value, and next-run instant; exactly one visible node |
| Existing manual workflow | Same graph and run behavior; no schedule node |
| Add schedule twice | One canonical schedule and one node; second action edits it |
| One chain / several roots | Correct generated arrows; every original executable node remains in the runner's graph |
| Empty or invalid graph | No hidden graph edits or fabricated steps |
| Move or focus node | Only layout/UI changes; no next-run reset |
| Toggle around a DST/date boundary | Same canonical cron; no picker round-trip or silent weekday shift |
| Advanced cron | Backend preview and stored schedule agree; unsupported frontend shorthand is not substituted |
| Remove / graph undo / duplicate / paste | Timer cannot be duplicated or resurrected by ordinary graph history |
| Switch workflows during requests | Node/form always belongs to the displayed workflow |
| Failed save / removed schedule during save | Persisted state wins; no silent recreation or false enabled indicator |
| Entitlement unavailable | Existing schedule visible; prohibited activation blocked; disable/remove and existing manual-run policy preserved |
| Scheduled run completes | Next-run display refreshes; no synthetic node appears in run steps |
| Shared template import | Recipient gets a disabled canonical schedule and one reconstructed canvas node |

Run relevant schedule, store, canvas-composition, and migration tests, then `bun run check` and `git diff --check`. Use clock-controlled fixtures for UTC/local-day changes and DST boundaries. Visual verification must include the canvas at different zoom levels, the form, the Schedules overview, keyboard interaction, and both themes. Do not claim runtime or visual verification from static source assertions alone.

The plan is complete when schedules are visible and manageable on the canvas, all existing execution behavior is preserved, and the overview consistently leads back to the same node.
