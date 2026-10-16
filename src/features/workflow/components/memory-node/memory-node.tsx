import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import { Icon } from "../../../../components/icon";
import type { MemoryNodeData } from "../../types";
import { NodeRunResult } from "../node-run-result";

type MemoryFlowNode = Node<MemoryNodeData, "memory">;

export function MemoryNode({ id, data }: NodeProps<MemoryFlowNode>) {
  const title = data.label || "Memories";
  const count = data.memoryIds?.length ?? 0;

  return (
    <div className="wf-node wf-node-memory">
      <Handle
        className="wf-handle"
        type="target"
        position={Position.Left}
        isConnectable
      />
      <div className="wf-node-header">
        <span className="wf-node-title-icon">
          <Icon name="note" size={16} />
        </span>
        <div className="wf-node-title">{title}</div>
      </div>
      <div className="wf-node-content">
        <p className="wf-node-body">
          {count > 0
            ? `${count} memor${count === 1 ? "y" : "ies"}`
            : "Select memories"}
        </p>
        <p className="wf-node-skill muted">
          {count > 0 ? "Injected into context" : "From this or other workflows"}
        </p>
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
