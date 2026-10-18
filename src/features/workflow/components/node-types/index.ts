import type { NodeTypes } from "@xyflow/react";
import { AgentNode } from "../agent-node";
import { AppActionNode } from "../app-action-node";
import { ChooseOutputNode } from "../choose-output-node";
import { IdentityNode } from "../identity-node/identity-node";
import { MemoryNode } from "../memory-node";
import { InputNode } from "../prompt-node";
import { ScheduleNode } from "../schedule-node";
import {
  FileInjectNode,
  CustomAgentNode,
  GitHostNode,
  GitStatusNode,
  HttpNode,
  NotifyNode,
  ScriptNode,
  ShellNode,
  TemplateNode,
  WriteFileNode,
} from "../utility-nodes/utility-nodes";

export const nodeTypes: NodeTypes = {
  /** Render-only: the workflow's schedule. Never saved into a graph. */
  schedule: ScheduleNode,
  /** Legacy type id — still used by existing graphs. */
  prompt: InputNode,
  input: InputNode,
  agent: AgentNode,
  appAction: AppActionNode,
  customAgent: CustomAgentNode,
  chooseOutput: ChooseOutputNode,
  memory: MemoryNode,
  identity: IdentityNode,
  template: TemplateNode,
  fileInject: FileInjectNode,
  gitStatus: GitStatusNode,
  /** Superseded by `script` in the palette; kept for existing graphs. */
  shell: ShellNode,
  script: ScriptNode,
  http: HttpNode,
  notify: NotifyNode,
  writeFile: WriteFileNode,
  gitHost: GitHostNode,
};
