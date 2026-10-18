import type {
  Schedule,
  ScheduleCanvasNode,
  ScheduleLayout,
  WorkflowEdge,
  WorkflowGraph,
  WorkflowNode,
} from "./types";

/**
 * Composition for the schedule node shown on the workflow canvas.
 *
 * The schedule itself lives once, in `schedules`. This module only turns that
 * record into a render-only node plus one generated arrow per executable root,
 * so the timer is visible and connected without ever entering the graph the
 * runner executes, the graph that gets saved, or a shared template.
 */

/** Reserved id namespace: no executable node or edge may use it. */
const SCHEDULE_NODE_PREFIX = "schedule-node:";
/** Arrow ids are never created; the prefix stays reserved for stray elements. */
const SCHEDULE_EDGE_PREFIX = "schedule-edge:";

/**
 * geometry: the schedule card is a fixed-width node, so the default position to
 * its left clears it without a measurement pass. Keep in step with
 * `.wf-node-schedule` in `schedule-node.css`.
 */
const SCHEDULE_NODE_WIDTH = 240;
/** Gap between the schedule node and the leftmost root it starts. */
const SCHEDULE_NODE_GAP = 72;
/** Canvas grid, matching React Flow's `snapGrid`. */
const GRID = 8;
/** Farthest a dragged node may be stored from the origin. */
export const MAX_SCHEDULE_LAYOUT_COORDINATE = 50_000;

export type ScheduleCanvasPiece = {
  /** Render-only schedule node, or null when the workflow has no schedule. */
  node: ScheduleCanvasNode | null;
  /**
   * Executable roots the schedule starts, in saved-graph order. The canvas
   * draws one generated arrow to each; the arrows are painted outside React
   * Flow's edge model, so roots that accept no incoming connection (an Input
   * step) can still be pointed at.
   */
  rootIds: string[];
};

export function scheduleCanvasNodeId(scheduleId: string): string {
  return `${SCHEDULE_NODE_PREFIX}${scheduleId}`;
}

/** Whether an id belongs to the schedule node or one of its start arrows. */
export function isScheduleCanvasId(id: string | null | undefined): boolean {
  if (!id) return false;
  return (
    id.startsWith(SCHEDULE_NODE_PREFIX) || id.startsWith(SCHEDULE_EDGE_PREFIX)
  );
}

/** Node ids the runner starts from: no incoming connection in the saved graph. */
export function executableRoots(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): string[] {
  const known = new Set(nodes.map((node) => node.id));
  const targeted = new Set<string>();
  for (const edge of edges) {
    if (known.has(edge.source) && known.has(edge.target)) {
      targeted.add(edge.target);
    }
  }
  return nodes
    .map((node) => node.id)
    .filter((id) => !targeted.has(id) && !isScheduleCanvasId(id));
}

/**
 * Deterministic default placement, used until the user drags the node: to the
 * left of the roots it starts, vertically centred on them. An empty workflow
 * places it at the origin and shows no arrow.
 */
export function defaultSchedulePosition(
  nodes: WorkflowNode[],
  roots: string[],
): { x: number; y: number } {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const positions = roots
    .map((id) => byId.get(id)?.position)
    .filter(
      (position): position is { x: number; y: number } =>
        Boolean(position) &&
        Number.isFinite(position!.x) &&
        Number.isFinite(position!.y),
    );
  if (positions.length === 0) return { x: 0, y: 0 };

  const left = Math.min(...positions.map((position) => position.x));
  const top = Math.min(...positions.map((position) => position.y));
  const bottom = Math.max(...positions.map((position) => position.y));
  return {
    x: snap(left - SCHEDULE_NODE_WIDTH - SCHEDULE_NODE_GAP),
    y: snap((top + bottom) / 2),
  };
}

/** Finite and inside the stored bounds, so a persisted position is sane. */
export function isUsableScheduleLayout(
  layout: ScheduleLayout | null | undefined,
): layout is ScheduleLayout {
  return Boolean(
    layout &&
      Number.isFinite(layout.x) &&
      Number.isFinite(layout.y) &&
      Math.abs(layout.x) <= MAX_SCHEDULE_LAYOUT_COORDINATE &&
      Math.abs(layout.y) <= MAX_SCHEDULE_LAYOUT_COORDINATE,
  );
}

/** Bounded, grid-aligned coordinates for a drag that is about to be saved. */
export function normalizeSchedulePosition(position: { x: number; y: number }): {
  x: number;
  y: number;
} {
  return { x: snap(position.x), y: snap(position.y) };
}

/**
 * Build the schedule node and the list of roots it starts.
 *
 * Returns nothing when the graph already uses a reserved id: a collision would
 * make the runner and the canvas disagree about which element is which.
 */
export function composeScheduleCanvas(input: {
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
  schedule: Schedule | null | undefined;
  layout: ScheduleLayout | null | undefined;
}): ScheduleCanvasPiece {
  const { nodes, edges, schedule } = input;
  if (!schedule) return { node: null, rootIds: [] };
  const reservedIdInUse =
    nodes.some((node) => isScheduleCanvasId(node.id)) ||
    edges.some(
      (edge) =>
        isScheduleCanvasId(edge.id) ||
        isScheduleCanvasId(edge.source) ||
        isScheduleCanvasId(edge.target),
    );
  if (reservedIdInUse) return { node: null, rootIds: [] };

  const roots = executableRoots(nodes, edges);
  return {
    node: {
      id: scheduleCanvasNodeId(schedule.id),
      type: "schedule",
      position: isUsableScheduleLayout(input.layout)
        ? { x: input.layout.x, y: input.layout.y }
        : defaultSchedulePosition(nodes, roots),
      // geometry: the card has a fixed width; React Flow measures the height.
      width: SCHEDULE_NODE_WIDTH,
      data: {
        kind: "schedule",
        workflowId: schedule.workflowId,
        scheduleId: schedule.id,
      },
      connectable: false,
    },
    rootIds: roots,
  };
}

/**
 * Drop synthetic elements from a graph on its way to disk. Defensive: the
 * composed node never enters graph state, and this keeps a future handler from
 * persisting one through a path nobody remembered.
 */
export function stripScheduleCanvasElements(graph: WorkflowGraph): WorkflowGraph {
  const nodes = graph.nodes.filter((node) => !isScheduleCanvasId(node.id));
  const edges = graph.edges.filter(
    (edge) =>
      !isScheduleCanvasId(edge.id) &&
      !isScheduleCanvasId(edge.source) &&
      !isScheduleCanvasId(edge.target),
  );
  if (nodes.length === graph.nodes.length && edges.length === graph.edges.length) {
    return graph;
  }
  return { nodes, edges };
}

function snap(value: number): number {
  const finite = Number.isFinite(value) ? value : 0;
  const bounded = Math.max(
    -MAX_SCHEDULE_LAYOUT_COORDINATE,
    Math.min(MAX_SCHEDULE_LAYOUT_COORDINATE, finite),
  );
  const aligned = Math.round(bounded / GRID) * GRID;
  // Never store a negative zero: it survives a round trip as `-0`.
  return aligned === 0 ? 0 : aligned;
}
