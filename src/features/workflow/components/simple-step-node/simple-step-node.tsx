import { Handle, Position, type NodeProps } from "@xyflow/react";
import type { ReactNode } from "react";
import { NodeRunResult } from "../node-run-result";

type Props = {
  id: string;
  className: string;
  title: string;
  body: string;
  meta?: string;
  icon?: ReactNode;
};

/** Compact display-only step card used by Context / Action / Sink utilities. */
export function SimpleStepNode({ id, className, title, body, meta, icon }: Props) {
  return (
    <div className={`wf-node ${className}`}>
      <Handle
        className="wf-handle"
        type="target"
        position={Position.Left}
        isConnectable
      />
      <div className="wf-node-header">
        {icon ? <span className="wf-node-title-icon">{icon}</span> : null}
        <div className="wf-node-title">{title}</div>
      </div>
      <div className="wf-node-content">
        <p className="wf-node-body">{body}</p>
        {meta ? <p className="wf-node-skill muted">{meta}</p> : null}
        <NodeRunResult nodeId={id} title={title} />
      </div>
      <Handle
        className="wf-handle"
        type="source"
        position={Position.Right}
        isConnectable
      />
    </div>
  );
}

/** Truncate a one-line preview for node cards. */
export function previewLine(text: string, empty = "Not configured"): string {
  const t = text.trim();
  if (!t) return empty;
  return t.length > 72 ? `${t.slice(0, 69)}…` : t;
}

/** Satisfy React Flow NodeProps typing for simple cards. */
export type AnyNodeProps = NodeProps;
