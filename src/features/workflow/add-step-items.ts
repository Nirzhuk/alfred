import type { AgentProviderId, WorkflowNodeData } from "./types";
import {
  defaultCustomAgentNodeData,
  defaultAppActionNodeData,
  defaultFileInjectNodeData,
  defaultGitHostNodeData,
  defaultGitStatusNodeData,
  defaultHttpNodeData,
  defaultIdentityNodeData,
  defaultNotifyNodeData,
  defaultScriptNodeData,
  defaultTemplateNodeData,
  defaultWriteFileNodeData,
} from "./types";

type AddStepDetails = {
  id: string;
  label: string;
  description: string;
  keywords?: string;
};

export type AddStepItem = AddStepDetails & (
  | { kind: "prompt" }
  | { kind: "choose" }
  | { kind: "memory" }
  | { kind: "agent"; provider: AgentProviderId }
  | { kind: "step"; type: string; data: WorkflowNodeData }
);

export type AddStepGroupId = "essentials" | "agents" | "tools" | "outputs";
export type AddStepCategory = "all" | AddStepGroupId;

export type AddStepGroup = {
  id: AddStepGroupId;
  label: string;
  items: AddStepItem[];
};

export const ADD_STEP_GROUPS: AddStepGroup[] = [
  {
    id: "essentials",
    label: "Essentials",
    items: [
      {
        id: "input",
        kind: "prompt",
        label: "Input",
        description: "Give the agent a task and context",
        keywords: "prompt text attachments files",
      },
      {
        id: "identity",
        kind: "step",
        label: "Identity",
        description: "Set a role, personality, and instructions",
        keywords: "persona behavior standing system",
        type: "identity",
        data: defaultIdentityNodeData(),
      },
      {
        id: "memory",
        kind: "memory",
        label: "Memories",
        description: "Bring knowledge from previous runs",
        keywords: "memory context history recall",
      },
    ],
  },
  {
    id: "agents",
    label: "Agents",
    items: [
      { id: "claude_code", kind: "agent", label: "Claude Code", provider: "claude_code", description: "Run a task with Claude Code" },
      { id: "cursor", kind: "agent", label: "Cursor", provider: "cursor", description: "Run a task with Cursor" },
      { id: "codex", kind: "agent", label: "Codex", provider: "codex", description: "Run a task with Codex" },
      { id: "opencode", kind: "agent", label: "OpenCode", provider: "opencode", description: "Run a task with OpenCode" },
      { id: "github_copilot", kind: "agent", label: "GitHub Copilot", provider: "github_copilot", description: "Run a task with GitHub Copilot" },
      { id: "gemini", kind: "agent", label: "Gemini", provider: "gemini", description: "Run a task with Gemini" },
      { id: "grok", kind: "agent", label: "Grok", provider: "grok", description: "Run a task with Grok" },
      { id: "pi", kind: "agent", label: "Pi", provider: "pi", description: "Run a task with Pi" },
      { id: "omp", kind: "agent", label: "OMP", provider: "omp", description: "Run a task with OMP" },
      {
        id: "customAgent",
        kind: "step",
        label: "Custom agent",
        description: "Use your own agent command",
        keywords: "cli bring your own",
        type: "customAgent",
        data: defaultCustomAgentNodeData(),
      },
    ],
  },
  {
    id: "tools",
    label: "Tools",
    items: [
      {
        id: "appAction",
        kind: "step",
        label: "App action",
        description: "Use actions from connected apps",
        keywords: "apps integration telegram slack discord notion",
        type: "appAction",
        data: defaultAppActionNodeData(),
      },
      {
        id: "template",
        kind: "step",
        label: "Template",
        description: "Compose text with workflow values",
        keywords: "variables prompt context",
        type: "template",
        data: defaultTemplateNodeData(),
      },
      {
        id: "fileInject",
        kind: "step",
        label: "File inject",
        description: "Read a file into the agent's context",
        keywords: "attach load input",
        type: "fileInject",
        data: defaultFileInjectNodeData(),
      },
      {
        id: "gitStatus",
        kind: "step",
        label: "Git status",
        description: "Include repository changes and diffs",
        keywords: "context branch working tree",
        type: "gitStatus",
        data: defaultGitStatusNodeData(),
      },
      {
        id: "script",
        kind: "step",
        label: "Script",
        description: "Run a script and use its output",
        keywords: "shell terminal command code bash python",
        type: "script",
        data: defaultScriptNodeData(),
      },
      {
        id: "http",
        kind: "step",
        label: "HTTP",
        description: "Call an API or fetch a URL",
        keywords: "request web network webhook",
        type: "http",
        data: defaultHttpNodeData(),
      },
    ],
  },
  {
    id: "outputs",
    label: "Outputs",
    items: [
      {
        id: "output",
        kind: "choose",
        label: "Output",
        description: "Save results, files, or a report",
        keywords: "sink final html memory",
      },
      {
        id: "notify",
        kind: "step",
        label: "Notify",
        description: "Show a desktop notification",
        keywords: "alert notification",
        type: "notify",
        data: defaultNotifyNodeData(),
      },
      {
        id: "writeFile",
        kind: "step",
        label: "Write file",
        description: "Write workflow output to a file",
        keywords: "save export disk",
        type: "writeFile",
        data: defaultWriteFileNodeData(),
      },
      {
        id: "createPr",
        kind: "step",
        label: "Create PR",
        description: "Open a pull request with your changes",
        keywords: "github gitlab merge git",
        type: "gitHost",
        data: defaultGitHostNodeData("Create PR", "pr"),
      },
      {
        id: "openIssue",
        kind: "step",
        label: "Open issue",
        description: "Create an issue in your repository",
        keywords: "github gitlab ticket git",
        type: "gitHost",
        data: defaultGitHostNodeData("Open issue", "issue"),
      },
    ],
  },
];

export const ADD_STEP_ITEMS: AddStepItem[] = ADD_STEP_GROUPS.flatMap(
  (group) => group.items,
);

export function filterAddStepGroups(
  query: string,
  category: AddStepCategory = "all",
): AddStepGroup[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return ADD_STEP_GROUPS
    .filter((group) => category === "all" || group.id === category)
    .map((group) => ({
      ...group,
      items: group.items.filter((item) => {
        const text = [item.label, item.description, item.keywords, group.label]
          .join(" ")
          .toLowerCase();
        return words.every((word) => text.includes(word));
      }),
    }))
    .filter((group) => group.items.length > 0);
}

export function addStepNodeType(item: AddStepItem): string {
  if (item.kind === "step") return item.type;
  if (item.kind === "prompt") return "input";
  if (item.kind === "choose") return "chooseOutput";
  return item.kind;
}
