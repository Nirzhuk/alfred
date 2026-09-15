import type { Node, NodeProps } from "@xyflow/react";
import { Icon } from "../../../../components/icon";
import type { IdentityNodeData } from "../../types";
import { previewLine, SimpleStepNode } from "../simple-step-node/simple-step-node";

export function IdentityNode({
  id,
  data,
}: NodeProps<Node<IdentityNodeData, "identity">>) {
  const detail = data.personality.trim() || data.instructions.trim();
  return (
    <SimpleStepNode
      id={id}
      className="wf-node-identity"
      icon={<Icon name="fingerprint" size={16} />}
      title={data.label || "Identity"}
      body={previewLine(data.role, "Define who the agent is")}
      meta={previewLine(detail, "Role, personality & instructions")}
    />
  );
}
