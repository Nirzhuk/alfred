import { describe, expect, test } from "bun:test";
import { withMemoryDefaults } from "./memories";
import { getWorkflowResources } from "./resources";
import type {
  RunHistoryMemoryUse,
  WorkflowNode,
  WorkflowNodeData,
} from "./types";

const node = (
  id: string,
  type: string,
  data: WorkflowNodeData,
): WorkflowNode => ({ id, type, data, position: { x: 0, y: 0 } });
const memory = (id: string, overrides = {}) =>
  withMemoryDefaults({
    id,
    title: id,
    body: "Context",
    createdAt: "2026-01-01",
    updatedAt: "2026-01-01",
    ...overrides,
  });
const use = (memoryId: string, nodeId = "agent"): RunHistoryMemoryUse => ({
  memoryId,
  nodeId,
  memoryTitle: "Historical title",
  scopeType: "workflow",
  memoryType: "note",
  rank: 1,
  score: 1,
  reason: "pinned",
  renderedBytes: 20,
  createdAt: "2026-01-01",
});

describe("workflow resources", () => {
  test("keeps available, selected, pinned, and recorded usage distinct", () => {
    const nodes = [
      node("memory", "memory", { label: "Context", memoryIds: ["selected"] }),
      node("agent", "agent", { label: "Agent", provider: "codex" }),
    ];
    const result = getWorkflowResources({
      nodes,
      skills: [],
      memories: [
        memory("available"),
        memory("selected"),
        memory("pinned", { pinned: true }),
        memory("expired", { expiresAt: "2000-01-01" }),
        memory("invalid", { expiresAt: "invalid" }),
        memory("retracted", { status: "retracted" }),
        memory("used", { status: "superseded" }),
      ],
      memoryUses: [
        use("used"),
        use("missing"),
        use("missing", "deleted-agent"),
      ],
    });
    expect(result.memories.map((entry) => [entry.id, entry.badge])).toEqual([
      ["memory:available", "Available"],
      ["memory:selected", "Selected"],
      ["memory:pinned", "Pinned"],
      ["memory:used", "Used in run"],
      ["memory:missing", "Used in run"],
    ]);
    expect(result.memories[1].nodeIds).toEqual(["memory"]);
    expect(result.memories[4].title).toBe("Historical title");
    expect(result.memories[4].memoryId).toBeUndefined();
    expect(result.memories[4].nodeIds).toEqual(["agent"]);
  });

  test("does not advertise linked memories as pinned context", () => {
    const result = getWorkflowResources({
      nodes: [],
      skills: [],
      memories: [
        memory("linked", { pinned: true, origin: "linked" }),
        memory("linkable", { pinned: true, origin: "linkable" }),
      ],
    });
    expect(result.memories.map((entry) => entry.badge)).toEqual([
      "Available",
      "Available",
    ]);
  });

  test("deduplicates assigned skills across legacy and current agents and keeps missing skills", () => {
    const result = getWorkflowResources({
      memories: [],
      nodes: [
        node("a", "agent", {
          label: "Writer",
          provider: "codex",
          skillNames: [" review ", "review", "missing"],
        }),
        node("b", "agent", {
          label: "Reviewer",
          provider: "claude_code",
          skillName: "review",
        }),
      ],
      skills: [
        {
          name: "review",
          description: "Review changes",
          path: "/skills/review",
          source: "user",
          providers: ["codex"],
        },
      ],
    });
    expect(result.skills).toHaveLength(2);
    expect(result.skills[0].nodeIds).toEqual(["a", "b"]);
    expect(result.skills[0].description).toBe("Review changes");
    expect(result.skills[0].badge).toBe("Partly available");
    expect(result.skills[0].searchText).toContain("reviewer");
    expect(result.skills[1].badge).toBe("Unavailable");
  });

  test("classifies custom agents and tools using the current graph with useful fallbacks", () => {
    const result = getWorkflowResources({
      memories: [],
      skills: [],
      nodes: [
        node("custom", "customAgent", {
          kind: "customAgent",
          label: " ",
          command: "custom-cli",
          promptMode: "stdin",
        }),
        node("input", "input", {
          label: " ",
          prompt: "Read",
          attachments: [
            { id: "attachment", path: "/docs/brief.md", kind: "file" },
          ],
        }),
        node("context", "memory", { label: "Context", memoryIds: [] }),
        node("identity", "identity", {
          kind: "identity",
          label: "Editor",
          role: "Editor",
          personality: "",
          instructions: "",
        }),
      ],
    });
    expect(result.agents.map((entry) => entry.title)).toEqual(["Custom agent"]);
    expect(result.tools.map((entry) => entry.title)).toEqual([
      "Input",
      "Editor",
    ]);
    expect(result.tools[0].description).toBe("Input · 1 attachment");
    expect(result.tools[0].searchText).toContain("/docs/brief.md");
    expect(
      getWorkflowResources({ nodes: [], memories: [], skills: [] }).agents,
    ).toEqual([]);
  });
});
