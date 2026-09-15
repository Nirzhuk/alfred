import type { PhosphorIconName } from "../../components/icon";
import { agentLabel } from "../../components/agent-mark";
import { isMemoryPromptEligible } from "./memories";
import {
  agentSkillNames,
  isAgentNodeData,
  isCustomAgentNodeData,
  isMemoryNodeData,
  isPromptNodeData,
  titleForNodeType,
  type AgentProviderId,
  type OutputMemory,
  type RunHistoryMemoryUse,
  type Skill,
  type WorkflowNode,
} from "./types";

export type ResourceItem = {
  id: string;
  title: string;
  description: string;
  icon?: PhosphorIconName;
  provider?: AgentProviderId;
  nodeIds: string[];
  memoryId?: string;
  badge?: string;
  searchText: string;
};

export type WorkflowResources = Record<
  "memories" | "skills" | "agents" | "tools",
  ResourceItem[]
>;

function item(
  value: Omit<ResourceItem, "searchText">,
  extra = "",
): ResourceItem {
  return {
    ...value,
    searchText: [value.title, value.description, value.badge, extra]
      .filter(Boolean)
      .join(" ")
      .toLocaleLowerCase(),
  };
}

export function getWorkflowResources({
  nodes,
  memories,
  skills,
  memoryUses = [],
}: {
  nodes: WorkflowNode[];
  memories: OutputMemory[];
  skills: Skill[];
  memoryUses?: RunHistoryMemoryUse[];
}): WorkflowResources {
  const result: WorkflowResources = {
    memories: [],
    skills: [],
    agents: [],
    tools: [],
  };
  const currentNodeIds = new Set(nodes.map((node) => node.id));
  const selections = new Map<string, string[]>();
  const uses = new Map<string, RunHistoryMemoryUse[]>();
  for (const use of memoryUses)
    uses.set(use.memoryId, [...(uses.get(use.memoryId) ?? []), use]);
  for (const node of nodes) {
    if (node.type === "memory" && isMemoryNodeData(node.data)) {
      for (const id of node.data.memoryIds)
        selections.set(id, [...(selections.get(id) ?? []), node.id]);
    }
  }
  const memoryIds = new Set(memories.map((memory) => memory.id));
  for (const memory of memories) {
    const recordedUses = uses.get(memory.id) ?? [];
    if (!isMemoryPromptEligible(memory) && !recordedUses.length) continue;
    const selectedBy = selections.get(memory.id) ?? [];
    result.memories.push(
      item(
        {
          id: `memory:${memory.id}`,
          memoryId: memory.id,
          title: memory.title.trim() || "Untitled memory",
          description: memory.scopeLabel.trim() || memory.scopeType,
          icon: "database",
          nodeIds: [
            ...new Set([
              ...selectedBy,
              ...recordedUses.map((use) => use.nodeId),
            ]),
          ].filter((id) => currentNodeIds.has(id)),
          badge: recordedUses.length
            ? "Used in run"
            : selectedBy.length
              ? "Selected"
              : memory.pinned &&
                  memory.origin !== "linked" &&
                  memory.origin !== "linkable"
                ? "Pinned"
                : "Available",
        },
        `${memory.body} ${memory.memoryType}`,
      ),
    );
  }
  for (const [id, recordedUses] of uses) {
    if (memoryIds.has(id)) continue;
    result.memories.push(
      item({
        id: `memory:${id}`,
        title: recordedUses[0].memoryTitle.trim() || "Untitled memory",
        description: "No longer available",
        icon: "database",
        badge: "Used in run",
        nodeIds: [...new Set(recordedUses.map((use) => use.nodeId))].filter(
          (nodeId) => currentNodeIds.has(nodeId),
        ),
      }),
    );
  }

  const assignedSkills = new Map<string, ResourceItem>();
  const unavailableOwners = new Map<string, number>();
  for (const node of nodes) {
    const title = node.data.label.trim() || titleForNodeType(node.type);
    if (node.type === "agent" && isAgentNodeData(node.data)) {
      const provider = node.data.provider;
      const providerName = agentLabel(provider);
      result.agents.push(
        item({
          id: `node:${node.id}`,
          title,
          description: [providerName, node.data.model?.trim()]
            .filter(Boolean)
            .join(" · "),
          provider,
          nodeIds: [node.id],
        }),
      );
      for (const name of new Set(agentSkillNames(node.data))) {
        const skill = skills.find(
          (entry) => entry.name === name && entry.providers.includes(provider),
        );
        if (!skill)
          unavailableOwners.set(name, (unavailableOwners.get(name) ?? 0) + 1);
        const existing = assignedSkills.get(name);
        if (existing) {
          existing.nodeIds.push(node.id);
          existing.searchText += ` ${title.toLocaleLowerCase()}`;
          if (skill && existing.badge === "Unavailable") {
            existing.description =
              skill.description.trim() ||
              `${skill.source === "project" ? "Project" : "User"} skill`;
            existing.badge = undefined;
          }
          continue;
        }
        assignedSkills.set(
          name,
          item(
            {
              id: `skill:${name}`,
              title: name,
              description: skill
                ? skill.description.trim() ||
                  `${skill.source === "project" ? "Project" : "User"} skill`
                : "Not available locally",
              icon: "code",
              nodeIds: [node.id],
              badge: skill ? undefined : "Unavailable",
            },
            `${title} ${skill?.path ?? ""}`,
          ),
        );
      }
    } else if (
      node.type === "customAgent" &&
      isCustomAgentNodeData(node.data)
    ) {
      result.agents.push(
        item({
          id: `node:${node.id}`,
          title,
          description: node.data.command.trim() || "Custom agent",
          icon: "terminal-window",
          nodeIds: [node.id],
        }),
      );
    } else if (node.type !== "memory") {
      const attachmentCount = isPromptNodeData(node.data)
        ? (node.data.attachments?.length ?? 0)
        : 0;
      result.tools.push(
        item(
          {
            id: `node:${node.id}`,
            title,
            description: `${titleForNodeType(node.type)}${attachmentCount ? ` · ${attachmentCount} attachment${attachmentCount === 1 ? "" : "s"}` : ""}`,
            icon: "file",
            nodeIds: [node.id],
          },
          isPromptNodeData(node.data)
            ? node.data.attachments
                ?.map((attachment) => attachment.path)
                .join(" ")
            : "",
        ),
      );
    }
  }
  result.skills = [...assignedSkills.entries()].map(([name, resource]) => {
    const missingCount = unavailableOwners.get(name) ?? 0;
    return {
      ...resource,
      badge:
        missingCount === resource.nodeIds.length
          ? "Unavailable"
          : missingCount
            ? "Partly available"
            : undefined,
    };
  });
  return result;
}
