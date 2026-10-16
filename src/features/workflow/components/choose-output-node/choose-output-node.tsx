import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import { Icon } from "../../../../components/icon";
import type { OutputNodeData } from "../../types";
import { NodeRunResult } from "../node-run-result";

type OutputFlowNode = Node<OutputNodeData, "chooseOutput">;

export function ChooseOutputNode({ id, data }: NodeProps<OutputFlowNode>) {
  const title = data.label || "Output";
  const chips: string[] = [];
  if (data.htmlReport) chips.push("HTML");
  if (data.saveToMemory) chips.push(data.pinMemory ? "Pinned memory" : "Memory");
  if (data.asFinalResult) chips.push("Final");
  if (data.includeFilesChanged) chips.push("Files");

  return (
    <div className="wf-node wf-node-choose">
      <Handle
        className="wf-handle"
        type="target"
        position={Position.Left}
        isConnectable
      />
      <div className="wf-node-header">
        <span className="wf-node-title-icon">
          <Icon name="arrow-square-out" size={16} />
        </span>
        <div className="wf-node-title">{title}</div>
      </div>
      <div className="wf-node-content">
        <p className="wf-node-body">
          {chips.length > 0
            ? "Dispose agent result"
            : "Pass through — no save"}
        </p>
        {chips.length > 0 ? (
          <div className="wf-node-disposition" aria-label="Disposition">
            {chips.map((chip) => (
              <span key={chip} className="wf-node-disposition-chip">
                {chip}
              </span>
            ))}
          </div>
        ) : (
          <p className="wf-node-skill muted">Preview only</p>
        )}
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
