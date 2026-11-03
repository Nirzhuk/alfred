import { layout as dagreLayout, graphlib } from "@dagrejs/dagre";
import type { WorkflowEdge, WorkflowNode } from "./types";

/** Used until React Flow has measured a node; matches `.wf-node`. */
const FALLBACK_SIZE = { width: 240, height: 120 };
/**
 * geometry: room for the Input/Output panel a run adds under a step, so a tidy
 * column does not overlap after the next run.
 */
const RESULT_PANEL_HEIGHT = 110;
/** geometry: `.gradient-edge-condition` max-width and height, kept clear of steps. */
const CONDITION_LABEL = { width: 220, height: 28 };
const GRID = 8;

const snap = (value: number) => Math.round(value / GRID) * GRID;

/**
 * Left-to-right columns in dependency order. Steps with a fixed height never
 * grow, and steps already showing a result are measured with it.
 */
export function layoutNodes(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
  hasResult: (id: string) => boolean = () => false,
): Map<string, { x: number; y: number }> {
  const graph = new graphlib.Graph({ multigraph: true })
    .setGraph({ rankdir: "LR", nodesep: 40, ranksep: 80 })
    .setDefaultEdgeLabel(() => ({}));

  for (const node of nodes) {
    const size = node.measured ?? FALLBACK_SIZE;
    const grows = !node.height && !hasResult(node.id);
    graph.setNode(node.id, {
      width: size.width ?? FALLBACK_SIZE.width,
      height:
        (size.height ?? FALLBACK_SIZE.height) +
        (grows ? RESULT_PANEL_HEIGHT : 0),
    });
  }
  for (const edge of edges) {
    if (!graph.hasNode(edge.source) || !graph.hasNode(edge.target)) continue;
    graph.setEdge(
      edge.source,
      edge.target,
      edge.data?.condition ? CONDITION_LABEL : {},
      edge.id,
    );
  }

  dagreLayout(graph);

  // Dagre reports centres; React Flow positions are top-left corners.
  return new Map(
    nodes.map((node) => {
      const { x, y, width, height } = graph.node(node.id);
      return [node.id, { x: snap(x - width / 2), y: snap(y - height / 2) }];
    }),
  );
}
