# Plan 053: Conditional connections on the workflow canvas

## Status and scope

- Status: IN PROGRESS — steps 1–4 implemented; `bun run check` green
  (571 frontend, 1108 Rust). Desktop visual/run verification in the actual
  Tauri app is still outstanding and is required before this is DONE.
- Priority: P1. Effort: M.
- Requested on: 2026-09-16.
- Baseline: desktop commit `4894951`, clean working tree.
- Dependencies: the existing workflow canvas, workflow store, runner graph walk, and run history. No new backend service, no new database table.
- Independent of: [template sharing](042-share-workflows-by-link.md) and [schedule nodes](043-schedule-node-on-canvas.md), except for the template contract change described below, which sharing must absorb.

Let a connection between two steps carry a condition. When the condition fails,
the connection does not fire and the branch behind it does not run. Connections
with no condition behave exactly as they do today.

This plan does not add a Condition/Branch node type, an expression language, a
loop construct, per-branch context isolation, or failure-tolerant steps. A step
that errors still fails the whole run, unchanged.

## Deviations from this plan as written

- The editor is a shared `Modal` (`components/edge-condition-modal/`), not a
  floating popover. The plan already required building it on the shared
  `Modal`/`SelectControl` components; naming it a popover while rendering a
  modal would have been misleading, and the modal needs no positioning code.
- `SCHEMA_VERSION` is a supported *range* rather than a flat bump. A blanket
  bump would have made every already-shared v1 link unreadable by this build.
  Instead the build reads v1–v2, and emits v2 only when a connection actually
  carries a condition. Older builds still refuse a v2 template, which is the
  property the plan was protecting.
- The runner's node loop was refactored into level-based parallel branch
  execution by concurrent work while this was being implemented. The gate was
  carried into that structure unchanged: skips are evaluated per node against
  the level-entry snapshot, in graph order, before any branch fans out.

## User experience

```text
                        ┌─(output contains "FAIL")──> [Codex] ──> [Slack] ──> [Output]
[Agent: generator] ─────┤
                        ├─(output contains "PASS")──> [Create PR]
                        └─(no condition)───────────> [Write file]
```

1. Draw a connection as today. It is unconditional and shows no chip. Nothing
   about existing workflows changes.
2. Click a connection. A small popover opens at the connection's midpoint with
   three controls: source, operator, value. `Add condition` is the only action
   on an unconditional connection.
3. A conditional connection renders a dashed stroke and a short chip at its
   midpoint, for example `output contains "FAIL"`. The chip truncates with the
   full text in its accessible name and title.
4. `Remove condition` returns the connection to the unconditional dashless
   style. Deleting the connection deletes the condition with it.
5. During a run, a connection that fired renders active; one whose condition
   failed renders muted, and the steps behind it render as skipped rather than
   pending. Run history lists those steps with status `Skipped` and the reason.

The popover is the only new surface. Do not add a condition field to the node
settings modal, and do not add a second place to author the same value.

## Semantics

These three rules are the whole feature. Implement them in one place and test
them directly.

1. **A connection fires** when its source step ran and produced a result, and
   its condition is absent or evaluates true.
2. **A step runs** when it has no incoming connections (a root), or when at
   least one incoming connection fired. Several firing connections run the step
   once, not once per connection.
3. **A step is skipped** when it has incoming connections and none of them
   fired. A skipped step produces no result, so none of its own outgoing
   connections fire. Skipping therefore propagates down the branch on its own —
   no separate subtree walk is required.

Consequences worth stating in tests:

- Fan-out runs every branch whose condition passes, in the existing topological
  order. Two true conditions run both branches.
- A join step downstream of one skipped and one live branch still runs, because
  rule 2 needs only one firing connection.
- A workflow with no conditions produces byte-identical behavior to today.
- A skipped step does not modify `context_prompt` or `last_output`, does not
  emit evidence sources, and does not run output persistence.
- Cycles and disconnected nodes keep their current handling. `topological_order`
  already appends unreached nodes; those remain roots and still run.

## Data model

Conditions live on the edge, in `data`, and are absent by default.

```ts
// src/features/workflow/types.ts
export type EdgeConditionSource = "output" | "context" | "trigger";

export type EdgeConditionOperator =
  | "contains"
  | "notContains"
  | "equals"
  | "notEquals"
  | "matches"
  | "isEmpty"
  | "isNotEmpty";

export type EdgeCondition = {
  source: EdgeConditionSource;
  operator: EdgeConditionOperator;
  /** Ignored by isEmpty / isNotEmpty. */
  value: string;
  /** Text comparison case sensitivity. Default false. */
  caseSensitive?: boolean;
};

export type WorkflowEdgeData = { condition?: EdgeCondition };
export type WorkflowEdge = Edge<WorkflowEdgeData>;
```

`WorkflowEdge` is currently the bare `Edge` alias at `src/features/workflow/types.ts`.
Narrowing it is the one type change that ripples; fix the call sites it reveals
rather than widening back to `Edge`.

The three sources are the values that already exist at evaluation time:

| Source | Value compared |
| --- | --- |
| `output` | The text output of this connection's own source step |
| `context` | The accumulated `context_prompt` at that moment |
| `trigger` | The run's trigger payload JSON, or empty string when there is none |

Do not offer step status or exit code in this version. A failing step ends the
run before any downstream connection is evaluated, so those options would list
a condition that can never be true. They become meaningful only alongside
failure-tolerant steps, which are out of scope here.

`matches` compiles a regular expression with the `regex` crate, already present
in `src-tauri/Cargo.lock` transitively; promote it to a direct dependency. Cap
the pattern at 512 bytes, compile once per run per edge, and treat a pattern
that fails to compile as a false condition plus a visible step note — never a
panic and never a run failure. `regex` has no catastrophic backtracking, so no
evaluation timeout is needed.

## Runner changes

All of this is in `src-tauri/src/runner/mod.rs`.

`topological_order` (line 2652) stays as-is and keeps producing the execution
order. The node loop (line 1244) gains a gate in front of it.

1. Build `incoming: HashMap<String, Vec<&Value>>` from `edges` alongside the
   existing ordering pass, keyed by target.
2. Add `node_outputs: HashMap<String, String>` and `skipped: HashSet<String>`
   to the run state next to `last_output` (line 1170). The per-node map is
   required: a condition on `S → T` compares `S`'s output, and `last_output` is
   a single shared value that any later step overwrites.
3. At the top of the loop body, after `node_id` is read (line 1268), evaluate
   rule 2. When the step is skipped, record a `skipped` step through
   `insert_step`, emit a `step_skipped` run event carrying the failing
   condition's summary, and `continue` — before the `step_started` event, before
   `output_disposition` is captured, and before any evidence input is built.
4. In the `Ok(...)` arm (line 2441), record `node_outputs.insert(node_id, output)`
   next to the existing `last_output` assignment.
5. Add `fn edge_fires(edge, node_outputs, skipped, context, trigger) -> bool` and
   `fn evaluate_condition(condition, subject) -> bool` as free functions with
   direct unit tests. Keep them pure; no database, no app handle.

`insert_step` accepts a status string already, so `"skipped"` needs no signature
change. Confirm the run-status rollup does not treat a skipped step as a
failure, and that a run whose every leaf is skipped still finishes `completed`.

`nearest_html_report_agents` (line 913) and `with_upstream_identities` (line 689)
walk edges statically to resolve intent before execution. Leave both alone: an
identity or report instruction that reaches a step it can no longer influence is
inert, and making them branch-aware would couple static prompt assembly to
runtime state for no user-visible gain.

## Template contract

`contracts/workflow-template/schema.json` defines `$defs/edge` with
`additionalProperties: false`, and `src-tauri/src/templates/contract.rs:871`
rebuilds each edge into a `TemplateEdge` holding only id, source, target, and
handles. Both drop `data` silently, so a shared workflow would arrive with its
conditions removed and every branch unconditionally live — a wrong workflow that
looks correct.

- Add an optional `data.condition` object to `$defs/edge` with the enumerated
  source and operator values, a bounded `value` string, and `additionalProperties: false`
  on the condition itself.
- Add the field to `TemplateEdge` and to both the export mapping and the import
  reconstruction.
- Reject an unknown source or operator on import rather than coercing it, so a
  template from a newer build fails loudly instead of running the wrong branch.
- Decide `SCHEMA_VERSION` (`contract.rs:8`) deliberately: an optional additive
  field read by older builds as "no condition" is the same silent-wrong-branch
  failure, so bump it and let older builds refuse the template.
- Add a fixture under `contracts/workflow-template/fixtures/` covering a
  conditional edge and one covering an invalid operator.

## UI

Read `docs/design-system.md` before starting. All spacing, radius, font size,
and color values resolve to tokens from `src/styles/tokens.css`; a literal needs
a `geometry:` comment or it fails the token tests.

| Area | Location |
| --- | --- |
| Types and defaults | `src/features/workflow/types.ts` |
| `onConnect`, `updateEdgeCondition`, `removeEdgeCondition` | `src/features/workflow/store.ts:1910` |
| Chip, dashed stroke, run-state styling | `components/flow-editor/gradient-edge.tsx` |
| Edge click routing, popover mount | `components/flow-editor/flow-editor.tsx` |
| New popover component | `components/edge-condition-popover/` |
| Skipped status label | `components/run-activity-panel/run-activity-panel.tsx:260` |
| Skipped step in history | `components/history-page/` |
| Run event type | `src/features/workflow/types.ts`, `api.ts` |

- Render the chip through React Flow's `EdgeLabelRenderer` inside `GradientEdge`
  so it stays positioned at the bezier midpoint across zoom levels. The existing
  gradient stroke is unchanged; the condition adds a dash pattern only.
- Build the popover on the shared `Modal`/`SelectControl` components. Do not add
  feature-local select chrome. The value field is a plain input, not a
  `SearchInput`.
- `isEmpty` and `isNotEmpty` hide the value field rather than disabling it.
- The chip is not an interactive control on its own; the edge's existing
  interaction area opens the popover, and the popover is keyboard reachable from
  a selected edge via Enter.
- Verify both themes and `prefers-reduced-motion`. The muted skipped state must
  not be color-only — pair it with the dash pattern and the chip's text.

`store.ts:1910` currently writes `type: "default"` on new edges while
`flow-editor.tsx:80` rewrites every edge to `type: "gradient"` at render. Leave
that as is; it works. Just carry `data` through both paths.

## Delivery steps

### 1. Types, store, and the pure evaluator

- Add `EdgeCondition`, narrow `WorkflowEdge`, and fix the call sites that surfaces.
- Add store actions to set and clear a condition, marking the workflow dirty.
- Add `evaluate_condition` and `edge_fires` in Rust with unit tests for every
  operator, both case modes, empty subjects, and an uncompilable pattern.
- Exit when the evaluator is fully tested and no execution path uses it yet.

### 2. Runner gating and skipped steps

- Add the incoming map, `node_outputs`, `skipped`, the loop gate, and the
  `skipped` step record plus `step_skipped` event.
- Prove the no-conditions path is unchanged against the existing runner tests.
- Exit when fan-out, join, propagation, and all-skipped runs behave per the
  three semantic rules.

### 3. Canvas authoring

- Add the popover, chip, dashed stroke, and edge-click routing.
- Add the skipped label to the activity panel and run history.
- Exit when a user can add, edit, and remove a condition entirely on the canvas
  and watch a branch skip during a live run.

### 4. Sharing, docs, and verification

- Extend the schema, `TemplateEdge`, export, import, fixtures, and the version
  decision.
- Document conditional connections in `docs/` alongside the workflow docs.
- Run the matrix below in the actual Tauri desktop app, not from source reading.

## Acceptance tests

| Scenario | Expected result |
| --- | --- |
| Existing workflow, no conditions | Identical run steps, outputs, and history to the pre-change build |
| Fan-out, two of three conditions true | Both true branches run; the third branch's steps are recorded `skipped` |
| Condition false on a 3-step branch | All three steps skipped; no partial execution, no output persistence |
| Join with one live and one skipped input | Join step runs exactly once |
| Root step with no incoming edges | Always runs, regardless of any condition elsewhere |
| `matches` with an invalid pattern | Condition false, note on the skipped step, run completes |
| `trigger` source with no trigger payload | Compared against an empty string; `isEmpty` is true |
| Condition referencing a step that failed | Run already ended at the failure; no skipped rows are invented |
| Every leaf skipped | Run status `completed`, not `failed` |
| Save, reload, reopen workflow | Conditions round-trip exactly, including case sensitivity |
| Delete an edge / undo | Condition disappears and returns with the edge; no orphan condition state |
| Export and import a shared template | Conditions arrive intact; an unknown operator is rejected with a clear message |
| Older build opens a bumped template | Refused explicitly, never imported with conditions stripped |
| Both themes, zoomed canvas, keyboard only | Chip legible, popover reachable, focus visible, no layout shift |

Run `bun run check` and `git diff --check` before handoff. Runner behavior must
be demonstrated by an actual run in the desktop app; static source assertions do
not count as verification.

The plan is complete when a connection can gate its branch, unconditioned
workflows are provably unchanged, and a shared template carries its conditions
or refuses to import.
