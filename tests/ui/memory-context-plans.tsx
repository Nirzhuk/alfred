import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import "../../src/App.css";
import { applyResolvedTheme } from "../../src/features/settings/theme";
import { HistoryPage } from "../../src/features/workflow/components/history-page/history-page";
import { NodeSettingsModal } from "../../src/features/workflow/components/node-settings-modal/node-settings-modal";
import { MemoriesInspector } from "../../src/features/workflow/components/memories-inspector/memories-inspector";
import { useWorkflowStore } from "../../src/features/workflow/store";
import { isMemoryNodeData } from "../../src/features/workflow/types";
import type { OutputMemory, RunHistoryDetail, RunHistoryItem, Workflow } from "../../src/features/workflow/types";
import { memoryVersionFixture } from "./memory-version-fixture";

const params = new URLSearchParams(location.search);
const screen = params.get("screen") ?? "picker";
const scenario = params.get("scenario") ?? "normal";
applyResolvedTheme(params.get("theme") === "dark" ? "dark" : "light");
const timestamp = "2026-09-09T12:00:00Z";
const workflow: Workflow = {
  id: "context-qa-workflow", name: "Memory context QA", description: "Synthetic fixtures only",
  workingDirectory: "/tmp/context-qa", memoryRetrievalEnabled: true,
  memoryLookupEnabled: false,
  graph: { nodes: [], edges: [] }, createdAt: timestamp, updatedAt: timestamp,
};
const base: OutputMemory = {
  ...memoryVersionFixture(),
  id: "", workflowId: workflow.id, kind: "note", scopeType: "workflow", scopeKey: workflow.id,
  scopeLabel: workflow.name, memoryType: "fact", source: "manual", title: "", body: "",
  pinned: false, confidence: 1, salience: 50, status: "active", createdAt: timestamp,
  updatedAt: timestamp, origin: "owned",
};
const memories: OutputMemory[] = [
  { ...base, id: "context-fact", title: "Release workflow fact", body: "The release job uses the stable branch." },
  { ...base, id: "context-note", memoryType: "note", title: "Handwritten planning note", body: "Use this note when explicitly selected." },
  { ...base, id: "context-report", kind: "artifact", memoryType: "artifact", source: "run", title: "Large saved report", body: "Saved report preview. ".repeat(100), artifactPath: "/tmp/context-qa/synthetic-report.md" },
  { ...base, id: "context-expired", title: "Expired rollout instruction", body: "Historical rollout evidence", expiresAt: "2025-01-01T00:00:00Z" },
  { ...base, id: "context-retracted", title: "Retracted destination", body: "Historical destination", status: "retracted" },
  { ...base, id: "context-pinned", title: "Pinned release constraint", body: "Require manual review before publishing.", pinned: true, memoryType: "constraint" },
];
const linkable: OutputMemory = { ...base, id: "context-linkable", workflowId: "source-qa-workflow", scopeKey: "source-qa-workflow", title: "Linked research note", memoryType: "note", body: "Research to select deliberately.", origin: "linkable", sourceWorkflowName: "Research QA" };
const run: RunHistoryItem = {
  id: "context-qa-run", workflowId: workflow.id, workflowName: workflow.name, trigger: "manual",
  status: "completed", error: null, startedAt: timestamp, finishedAt: timestamp, createdAt: timestamp,
  stepCount: 1, finalOutputPreview: "Synthetic completed result",
};
const noRecall = scenario === "empty" || scenario === "unavailable";
const history: RunHistoryDetail = {
  run,
  evidenceSources: [],
  steps: [{
    id: "context-qa-step", nodeId: "context-agent", agentProvider: "claude_code", skillName: "Release review",
    status: "completed", input: { prompt: "Review the release", memoryRecallStatus: scenario === "empty" ? "abstained" : scenario === "unavailable" ? "unavailable" : "included", memoryRecallOmittedCount: scenario === "large" ? 3 : 0 },
    output: { summary: "Synthetic completed result" }, error: null, startedAt: timestamp, finishedAt: timestamp, createdAt: timestamp,
  }],
  memoryUses: noRecall ? [] : [
    { nodeId: "context-agent", memoryId: memories[0].id, memoryTitle: memories[0].title, scopeType: "workflow", memoryType: "fact", rank: 1, score: 1, reason: "explicit", renderedBytes: 500, rankingVersion: "explicit_v1", createdAt: timestamp },
    { nodeId: "context-agent", memoryId: memories[5].id, memoryTitle: memories[5].title, scopeType: "workflow", memoryType: "constraint", rank: 2, score: 1, reason: "pinned", renderedBytes: 350, rankingVersion: "pinned_v1", createdAt: timestamp },
    { nodeId: "context-agent", memoryId: "context-ranking", memoryTitle: scenario === "legacy" ? "Earlier ranking record" : "Current lexical match", scopeType: "workflow", memoryType: "fact", rank: 3, score: 0.65, reason: "lexical", renderedBytes: 200, rankingVersion: scenario === "legacy" ? "lexical_v1" : "lexical_v2", createdAt: timestamp },
  ],
  memoryContextSummaries: [{
    nodeId: "context-agent", budgetBytes: 12000, memoryBytes: noRecall ? 0 : 1100, includedCount: noRecall ? 0 : 3,
    explicitOmittedCount: scenario === "large" ? 2 : 0, pinnedOmittedCount: scenario === "large" ? 1 : 0,
    automaticOmittedCount: scenario === "large" ? 3 : 0, unavailableCount: scenario === "large" ? 1 : 0,
    repeatedCount: scenario === "large" ? 2 : 0, traceAvailable: scenario !== "unavailable", createdAt: timestamp,
  }],
};

const calls: Array<{ command: string; payload: unknown }> = [];
const errors: string[] = [];
const openedMemories: string[] = [];
window.addEventListener("error", event => errors.push(String(event.error?.stack ?? event.message)));
window.addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
window.addEventListener("alfred:open-memories", event => openedMemories.push((event as CustomEvent).detail.memoryId));
mockWindows("main");
mockIPC((command, payload: any) => {
  calls.push({ command, payload: structuredClone(payload ?? {}) });
  switch (command) {
    case "list_memories": return structuredClone(memories);
    case "list_linkable_memories":
      if (scenario === "lookup-failure") throw new Error("Synthetic linkable-memory lookup failed");
      return [structuredClone(linkable)];
    case "link_memory": return { ...linkable, origin: "linked" };
    case "plugin:opener|reveal_item_in_dir": throw new Error("Synthetic artifact unavailable");
    case "list_run_history": return [structuredClone(run)];
    case "get_run_history": return structuredClone(history);
    case "get_memory_review_job": return null;
    case "get_memory_review_settings": return { enabled: false, provider: null, model: null, maxCandidates: 5, updatedAt: timestamp };
    case "get_memory_lookup_availability":
      return { available: false, reason: "native_runtime_unavailable" };
    case "get_workflow_memory_review": return { workflowId: workflow.id, enabled: false, updatedAt: timestamp };
    case "list_agent_providers":
    case "list_memory_candidates":
    case "list_memory_evidence":
    case "list_memory_reviews":
    case "search_history":
    case "list_workflow_folders": return [];
    case "update_workflow": return { ...workflow, ...payload?.input };
    case "plugin:window|set_focus": return null;
    case "plugin:dialog|message": return "Ok";
    default:
      errors.push(`Unmocked context QA IPC: ${command}`);
      throw new Error(`Unmocked context QA IPC: ${command}`);
  }
}, { shouldMockEvents: true });
useWorkflowStore.setState({
  workflows: [workflow], activeWorkflowId: workflow.id, openWorkflowIds: [workflow.id],
  nodes: [{ id: "context-memory-node", type: "memory", position: { x: 0, y: 0 }, data: {
    label: "Release context", memoryIds: scenario === "empty" ? [] : ["context-fact", "context-report", "context-expired", "context-retracted", "context-deleted"],
  } }], edges: [], memories, loading: false, workflowLoading: false, error: null,
});
Object.assign(window, { contextFixture: {
  calls, errors, openedMemories, scenario,
  selection: () => {
    const data = useWorkflowStore.getState().nodes[0]?.data;
    return data && isMemoryNodeData(data) ? data.memoryIds : [];
  },
} });

function Fixture() {
  const [closed, setClosed] = useState(false);
  if (closed) return <p role="status">Dialog closed.</p>;
  if (screen === "history") return <HistoryPage activeWorkflowId={workflow.id} initialRunId={run.id} onClose={() => setClosed(true)} />;
  if (screen === "library") return <MemoriesInspector open initialMemoryId={params.get("memory") ?? "context-report"} onOpenRunHistory={() => {}} onClose={() => setClosed(true)} />;
  return <NodeSettingsModal nodeId="context-memory-node" onClose={() => setClosed(true)} />;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
