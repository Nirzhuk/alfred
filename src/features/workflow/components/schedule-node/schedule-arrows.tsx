import {
  getBezierPath,
  Position,
  useInternalNode,
  ViewportPortal,
  type InternalNode,
} from "@xyflow/react";

const ARROW_TOOLTIP =
  "This schedule starts the whole workflow, including every branch shown here.";

/**
 * The generated start arrows: one per executable root, drawn from the schedule
 * node to each root.
 *
 * They are painted over the viewport instead of being React Flow edges for two
 * reasons: a step that accepts no incoming connection (an Input) must still be
 * pointed at, and a generated arrow is not an element anyone can select,
 * disconnect, or delete. Positions come from the measured nodes, so an arrow
 * follows a step as it moves.
 */
export function ScheduleArrows({
  sourceId,
  rootIds,
}: {
  sourceId: string | null;
  rootIds: string[];
}) {
  if (!sourceId) return null;
  return (
    <ViewportPortal>
      <svg className="schedule-arrows" aria-hidden="true" focusable="false">
        {/* A native tooltip carries the one thing the arrows are for. */}
        <title>{ARROW_TOOLTIP}</title>
        {rootIds.map((rootId) => (
          <ScheduleArrow key={rootId} sourceId={sourceId} targetId={rootId} />
        ))}
      </svg>
    </ViewportPortal>
  );
}

function ScheduleArrow({
  sourceId,
  targetId,
}: {
  sourceId: string;
  targetId: string;
}) {
  const source = useInternalNode(sourceId);
  const target = useInternalNode(targetId);
  const from = nodeEdgePoint(source, "right");
  const to = nodeEdgePoint(target, "left");
  // Measurement lands one frame after first paint.
  if (!from || !to) return null;
  const [path] = getBezierPath({
    sourceX: from.x,
    sourceY: from.y,
    sourcePosition: Position.Right,
    targetX: to.x,
    targetY: to.y,
    targetPosition: Position.Left,
  });
  return <path className="schedule-arrow-path" d={path} />;
}

/** Flow-space point on the named side of a measured node, or null before that. */
function nodeEdgePoint(
  node: InternalNode | undefined,
  side: "left" | "right",
): { x: number; y: number } | null {
  const position = node?.internals?.positionAbsolute;
  const { width = 0, height = 0 } = node?.measured ?? {};
  if (!position || width === 0 || height === 0) return null;
  return {
    x: side === "left" ? position.x : position.x + width,
    y: position.y + height / 2,
  };
}
