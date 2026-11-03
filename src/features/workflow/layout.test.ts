import { describe, expect, test } from "bun:test";
import { layoutNodes } from "./layout";
import type { WorkflowEdge, WorkflowNode } from "./types";

const node = (id: string, height = 100): WorkflowNode => ({
  id,
  type: "agent",
  position: { x: 0, y: 0 },
  measured: { width: 240, height },
  data: {} as WorkflowNode["data"],
});
const edge = (source: string, target: string, condition = false): WorkflowEdge => ({
  id: `${source}-${target}`,
  source,
  target,
  ...(condition
    ? {
        data: {
          condition: {
            source: "output",
            operator: "contains",
            value: "x",
            caseSensitive: false,
          },
        },
      }
    : {}),
});

const graph = {
  nodes: ["a", "b", "c", "d", "side"].map((id) => node(id)),
  edges: [edge("a", "b"), edge("a", "c"), edge("b", "d"), edge("c", "d"), edge("side", "d")],
};

describe("layoutNodes", () => {
  test("every connection runs left to right", () => {
    const at = layoutNodes(graph.nodes, graph.edges);
    for (const { source, target } of graph.edges) {
      expect(at.get(source)!.x + 240).toBeLessThan(at.get(target)!.x);
    }
  });

  test("steps stay apart even after a run adds a result panel", () => {
    const at = layoutNodes(graph.nodes, graph.edges);
    const grown = 100 + 110;
    for (const a of graph.nodes) {
      for (const b of graph.nodes) {
        if (a.id === b.id) continue;
        const p = at.get(a.id)!;
        const q = at.get(b.id)!;
        const overlap =
          p.x < q.x + 240 && q.x < p.x + 240 && p.y < q.y + grown && q.y < p.y + grown;
        expect(overlap).toBe(false);
      }
    }
  });

  test("a side input sits next to the step it feeds, not at the far left", () => {
    const at = layoutNodes(graph.nodes, graph.edges);
    expect(at.get("side")!.x).toBe(at.get("b")!.x);
  });

  test("a condition label gets room between its columns", () => {
    const plain = layoutNodes([node("a"), node("b")], [edge("a", "b")]);
    const gated = layoutNodes([node("a"), node("b")], [edge("a", "b", true)]);
    const gap = (at: typeof plain) => at.get("b")!.x - at.get("a")!.x - 240;
    expect(gap(gated)).toBeGreaterThanOrEqual(220);
    expect(gap(gated)).toBeGreaterThan(gap(plain));
  });

  test("cycles and edges to removed steps do not throw", () => {
    const at = layoutNodes(
      [node("a"), node("b")],
      [edge("a", "b"), edge("b", "a"), edge("a", "gone")],
    );
    expect(at.size).toBe(2);
  });
});
