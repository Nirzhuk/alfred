import { describe, expect, test } from "bun:test";
import {
  composeScheduleCanvas,
  defaultSchedulePosition,
  executableRoots,
  isScheduleCanvasId,
  isUsableScheduleLayout,
  normalizeSchedulePosition,
  scheduleCanvasNodeId,
  stripScheduleCanvasElements,
} from "../src/features/workflow/schedule-canvas";
import type {
  Schedule,
  ScheduleLayout,
  WorkflowEdge,
  WorkflowNode,
} from "../src/features/workflow/types";

function node(id: string, x = 0, y = 0): WorkflowNode {
  return {
    id,
    type: "input",
    position: { x, y },
    data: { label: id, prompt: "" },
  };
}

function edge(source: string, target: string): WorkflowEdge {
  return { id: `${source}->${target}`, source, target };
}

function schedule(overrides: Partial<Schedule> = {}): Schedule {
  return {
    id: "schedule-1",
    workflowId: "workflow-1",
    cron: "0 0 9 * * 1-5",
    enabled: true,
    nextRunAt: "2026-09-10T09:00:00Z",
    onMissed: "run",
    createdAt: "2026-09-01T00:00:00Z",
    updatedAt: "2026-09-01T00:00:00Z",
    ...overrides,
  };
}

describe("executableRoots", () => {
  test("finds the start of a single chain", () => {
    const nodes = [node("a"), node("b"), node("c")];
    expect(executableRoots(nodes, [edge("a", "b"), edge("b", "c")])).toEqual([
      "a",
    ]);
  });

  test("keeps every disconnected starting branch", () => {
    const nodes = [node("a"), node("b"), node("c"), node("d")];
    expect(
      executableRoots(nodes, [edge("a", "b"), edge("c", "d")]),
    ).toEqual(["a", "c"]);
  });

  test("ignores edges whose endpoints are not saved nodes", () => {
    const nodes = [node("a"), node("b")];
    expect(executableRoots(nodes, [edge("missing", "b")])).toEqual(["a", "b"]);
    expect(executableRoots(nodes, [edge("a", "missing")])).toEqual(["a", "b"]);
  });

  test("a self-loop keeps its node out of the roots, like the runner", () => {
    const nodes = [node("a"), node("b")];
    expect(executableRoots(nodes, [edge("a", "a")])).toEqual(["b"]);
  });

  test("an empty graph has no roots", () => {
    expect(executableRoots([], [])).toEqual([]);
  });
});

describe("composeScheduleCanvas", () => {
  test("builds the render-only node and the roots it starts", () => {
    const piece = composeScheduleCanvas({
      nodes: [node("start", 200, 100), node("next", 500, 100)],
      edges: [edge("start", "next")],
      schedule: schedule(),
      layout: null,
    });

    expect(piece.node).not.toBeNull();
    expect(piece.node!.id).toBe(scheduleCanvasNodeId("schedule-1"));
    expect(piece.node!.type).toBe("schedule");
    expect(piece.node!.data).toEqual({
      kind: "schedule",
      workflowId: "workflow-1",
      scheduleId: "schedule-1",
    });
    expect(piece.rootIds).toEqual(["start"]);
    // Nothing connects into a schedule, and it starts no manual branch.
    expect(piece.node!.connectable).toBe(false);
  });

  test("points at every branch a scheduled run would start", () => {
    const piece = composeScheduleCanvas({
      nodes: [node("a"), node("b", 0, 300), node("c")],
      edges: [edge("a", "c")],
      schedule: schedule(),
      layout: null,
    });
    expect(piece.rootIds).toEqual(["a", "b"]);
  });

  test("shows nothing without a schedule", () => {
    expect(
      composeScheduleCanvas({
        nodes: [node("a")],
        edges: [],
        schedule: null,
        layout: null,
      }),
    ).toEqual({ node: null, rootIds: [] });
  });

  test("an empty workflow shows the node without an arrow", () => {
    const piece = composeScheduleCanvas({
      nodes: [],
      edges: [],
      schedule: schedule(),
      layout: null,
    });
    expect(piece.node).not.toBeNull();
    expect(piece.rootIds).toEqual([]);
  });

  test("refuses to compose into a graph that already uses the reserved ids", () => {
    const piece = composeScheduleCanvas({
      nodes: [node(scheduleCanvasNodeId("schedule-1"))],
      edges: [],
      schedule: schedule(),
      layout: null,
    });
    expect(piece).toEqual({ node: null, rootIds: [] });
  });

  test("a saved position wins; an unusable one falls back to the default", () => {
    const nodes = [node("start", 400, 200)];
    const saved: ScheduleLayout = {
      scheduleId: "schedule-1",
      x: -512,
      y: 96,
      updatedAt: "2026-09-02T00:00:00Z",
    };
    expect(
      composeScheduleCanvas({ nodes, edges: [], schedule: schedule(), layout: saved })
        .node!.position,
    ).toEqual({ x: -512, y: 96 });

    const broken: ScheduleLayout = { ...saved, x: Number.NaN };
    expect(
      composeScheduleCanvas({ nodes, edges: [], schedule: schedule(), layout: broken })
        .node!.position,
    ).toEqual(defaultSchedulePosition(nodes, ["start"]));
  });
});

describe("defaultSchedulePosition", () => {
  test("sits left of the roots and centered on them", () => {
    // Two roots 400 apart: the node is centred on the pair, not on either one.
    expect(
      defaultSchedulePosition([node("a", 320, 96), node("b", 320, 496)], [
        "a",
        "b",
      ]),
    ).toEqual({ x: 8, y: 296 });
  });

  test("uses the origin when there is nothing to sit beside", () => {
    expect(defaultSchedulePosition([], [])).toEqual({ x: 0, y: 0 });
  });
});

describe("schedule canvas ids", () => {
  test("recognizes the node and its arrows, and nothing else", () => {
    expect(isScheduleCanvasId(scheduleCanvasNodeId("s1"))).toBe(true);
    expect(isScheduleCanvasId("schedule-edge:s1:node-1")).toBe(true);
    expect(isScheduleCanvasId("input-1")).toBe(false);
    expect(isScheduleCanvasId(null)).toBe(false);
  });

  test("bounds a dragged position and rejects an unusable saved one", () => {
    expect(normalizeSchedulePosition({ x: 12.4, y: -3.6 })).toEqual({
      x: 16,
      y: 0,
    });
    expect(normalizeSchedulePosition({ x: -1, y: 1 })).toEqual({ x: 0, y: 0 });
    expect(normalizeSchedulePosition({ x: 8.4, y: -12.4 })).toEqual({
      x: 8,
      y: -16,
    });
    expect(
      normalizeSchedulePosition({ x: Number.POSITIVE_INFINITY, y: 4 }),
    ).toEqual({ x: 0, y: 8 });
    expect(
      isUsableScheduleLayout({
        scheduleId: "s1",
        x: 1_000_000,
        y: 0,
        updatedAt: "now",
      }),
    ).toBe(false);
    expect(
      isUsableScheduleLayout({
        scheduleId: "s1",
        x: 24,
        y: -16,
        updatedAt: "now",
      }),
    ).toBe(true);
  });
});

describe("stripScheduleCanvasElements", () => {
  test("keeps a graph without synthetic elements untouched", () => {
    const graph = { nodes: [node("a")], edges: [edge("a", "a")] };
    expect(stripScheduleCanvasElements(graph)).toBe(graph);
  });

  test("drops the schedule node and any edge touching it", () => {
    const scheduleNodeId = scheduleCanvasNodeId("schedule-1");
    const graph = {
      nodes: [node("a"), node(scheduleNodeId)],
      edges: [
        edge("a", "a"),
        { id: "schedule-edge:schedule-1:a", source: scheduleNodeId, target: "a" },
        { id: "stray", source: scheduleNodeId, target: "a" },
      ],
    };
    const stripped = stripScheduleCanvasElements(graph);
    expect(stripped.nodes.map((item) => item.id)).toEqual(["a"]);
    expect(stripped.edges.map((item) => item.id)).toEqual(["a->a"]);
  });
});
