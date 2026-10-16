import { useId } from "react";
import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  useStore,
  type ConnectionLineComponentProps,
  type EdgeProps,
} from "@xyflow/react";
import { nodeAccentColor } from "../../node-colors";
import { useWorkflowStore } from "../../store";
import {
  edgeConditionSummary,
  parseEdgeCondition,
  type WorkflowEdgeData,
} from "../../types";
import "./gradient-edge.css";

type GradientProps = {
  id: string;
  sourceX: number;
  sourceY: number;
  targetX: number;
  targetY: number;
  sourceColor: string;
  targetColor: string;
};

function ConnectionGradient({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourceColor,
  targetColor,
}: GradientProps) {
  return (
    <defs>
      <linearGradient
        id={id}
        gradientUnits="userSpaceOnUse"
        x1={sourceX}
        y1={sourceY}
        x2={sourceX === targetX && sourceY === targetY ? targetX + 0.01 : targetX}
        y2={targetY}
      >
        <stop offset="0%" stopColor={sourceColor} />
        <stop offset="100%" stopColor={targetColor} />
      </linearGradient>
    </defs>
  );
}

export function GradientEdge({
  id,
  source,
  target,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style,
  markerStart,
  markerEnd,
  interactionWidth,
  label,
  labelStyle,
  labelShowBg,
  labelBgStyle,
  labelBgPadding,
  labelBgBorderRadius,
  data,
}: EdgeProps) {
  const gradientId = `edge-gradient-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const sourceType = useStore((state) => state.nodeLookup.get(source)?.type);
  const targetType = useStore((state) => state.nodeLookup.get(target)?.type);
  // A connection whose target was skipped did not fire on this run. The muted
  // treatment is paired with the dash pattern and the chip text so the state is
  // never carried by color alone.
  const targetSkipped = useWorkflowStore(
    (state) => state.stepStatuses[target] === "skipped",
  );
  const condition = parseEdgeCondition(
    (data as WorkflowEdgeData | undefined)?.condition,
  );
  const [path, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
  });
  const summary = condition ? edgeConditionSummary(condition) : null;

  return (
    <>
      <ConnectionGradient
        id={gradientId}
        sourceX={sourceX}
        sourceY={sourceY}
        targetX={targetX}
        targetY={targetY}
        sourceColor={nodeAccentColor(sourceType)}
        targetColor={nodeAccentColor(targetType)}
      />
      <BaseEdge
        id={id}
        path={path}
        className={
          [
            condition ? "edge-conditional" : null,
            targetSkipped ? "edge-not-fired" : null,
          ]
            .filter(Boolean)
            .join(" ") || undefined
        }
        style={{ ...style, stroke: `url(#${gradientId})` }}
        markerStart={markerStart}
        markerEnd={markerEnd}
        interactionWidth={interactionWidth}
        label={label}
        labelX={labelX}
        labelY={labelY}
        labelStyle={labelStyle}
        labelShowBg={labelShowBg}
        labelBgStyle={labelBgStyle}
        labelBgPadding={labelBgPadding}
        labelBgBorderRadius={labelBgBorderRadius}
      />
      {summary ? (
        <EdgeLabelRenderer>
          <span
            className={`edge-condition-chip${targetSkipped ? " is-not-fired" : ""}`}
            style={{
              // geometry: anchors the chip on the bezier midpoint React Flow
              // computed above, in canvas coordinates.
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
            }}
            title={summary}
          >
            {targetSkipped ? `did not fire — ${summary}` : summary}
          </span>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
}

export function GradientConnectionLine({
  fromX,
  fromY,
  toX,
  toY,
  fromPosition,
  toPosition,
  fromNode,
  toNode,
  connectionLineStyle,
}: ConnectionLineComponentProps) {
  const gradientId = `connection-gradient-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const fromColor = nodeAccentColor(fromNode.type);
  const [path] = getBezierPath({
    sourceX: fromX,
    sourceY: fromY,
    sourcePosition: fromPosition,
    targetX: toX,
    targetY: toY,
    targetPosition: toPosition,
  });

  return (
    <>
      <ConnectionGradient
        id={gradientId}
        sourceX={fromX}
        sourceY={fromY}
        targetX={toX}
        targetY={toY}
        sourceColor={fromColor}
        targetColor={toNode ? nodeAccentColor(toNode.type) : fromColor}
      />
      <path
        d={path}
        fill="none"
        className="react-flow__connection-path"
        style={{ ...connectionLineStyle, stroke: `url(#${gradientId})` }}
      />
    </>
  );
}
