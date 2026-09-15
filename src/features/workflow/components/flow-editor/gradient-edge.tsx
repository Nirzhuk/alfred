import { useId } from "react";
import {
  BaseEdge,
  getBezierPath,
  useStore,
  type ConnectionLineComponentProps,
  type EdgeProps,
} from "@xyflow/react";
import { nodeAccentColor } from "../../node-colors";

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
}: EdgeProps) {
  const gradientId = `edge-gradient-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const sourceType = useStore((state) => state.nodeLookup.get(source)?.type);
  const targetType = useStore((state) => state.nodeLookup.get(target)?.type);
  const [path, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
  });

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
