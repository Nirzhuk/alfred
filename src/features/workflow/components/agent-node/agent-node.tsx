import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import { agentHarness, agentSkillNames, type AgentNodeData } from "../../types";
import { useWorkflowStore } from "../../store";
import { AgentMark, agentLabel } from "../../../../components/agent-mark";
import { NodeRunResult } from "../node-run-result";

type AlfredNode = Node<AgentNodeData, "agent">;

export function AgentNode({ id, data }: NodeProps<AlfredNode>) {
  const running = useWorkflowStore(
    (state) =>
      state.activeNodeId === id && state.stepStatuses[id] === "running",
  );
  const title = data.label || agentLabel(data.provider) || "Agent";
  const skills = agentSkillNames(data);

  return (
    <div className="wf-node wf-node-agent">
      <Handle
        className="wf-handle"
        type="target"
        position={Position.Left}
        isConnectable
      />
      <div className="wf-node-header">
        <span className="wf-node-title-icon">
          <AgentMark
            provider={data.provider}
            size={16}
            running={running}
          />
        </span>
        <div className="wf-node-title">{data.label || "Agent"}</div>
      </div>
      <div className="wf-node-content">
        <p className="wf-node-body">
          {agentLabel(data.provider)} ·{" "}
          {agentHarness(data) === "cli" ? "CLI" : "Alfred"}
        </p>
        <p className="wf-node-model">{data.model || "default model"}</p>
        {skills.length > 0 ? (
          <p className="wf-node-skill">
            {skills.map((s) => `/${s}`).join(" ")}
          </p>
        ) : (
          <p className="wf-node-skill muted">No skills</p>
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
