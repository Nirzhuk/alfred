import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "../../src/App.css";
import { applyResolvedTheme } from "../../src/features/settings/theme";
import { HistoryPage } from "../../src/features/workflow/components/history-page/history-page";
import { navigateWorkflowMemory, type WorkflowMemoryNavigationRequest } from "../../src/features/workflow/components/history-page/history-format";
import type { OutputReceipt } from "../../src/features/workflow/components/history-page/output-receipt";
import { MemoriesInspector } from "../../src/features/workflow/components/memories-inspector/memories-inspector";
import { useWorkflowStore } from "../../src/features/workflow/store";
import { defaultOutputNodeData } from "../../src/features/workflow/types";
import type { OutputMemory, RunHistoryDetail, RunHistoryItem, Workflow } from "../../src/features/workflow/types";
import { QaEvidencePanel } from "./qa-evidence-panel";
import { memoryVersionFixture } from "./memory-version-fixture";

const query = new URLSearchParams(location.search);
const scenario = query.get("scenario") ?? "saved";
const inactive = query.get("active") === "false";
applyResolvedTheme(query.get("theme") === "dark" ? "dark" : "light");
const at = "2026-09-09T12:00:00Z";
const storageId = "8ac34c22-58e1-43f0-92ae-8d4f67283598";
const body = scenario === "empty" ? "   " : "Captured release report from the completed step.\n\nThe workflow graph was edited after this run.";
const capturedDisposition = { ...defaultOutputNodeData("Release report"), memoryTitle: "Captured report title", pinMemory: true };
const currentNode = {
  id: "output-qa-node", type: "chooseOutput", position: { x: 0, y: 0 },
  data: { ...capturedDisposition, memoryTitle: "Edited graph title must never be saved", pinMemory: false },
};
const workflow: Workflow = {
  id: "output-qa-workflow", name: "Output persistence QA", description: "Synthetic output fixtures only",
  workingDirectory: "/tmp/output-qa", memoryRetrievalEnabled: false,
  memoryLookupEnabled: false,
  graph: { nodes: [currentNode], edges: [] }, createdAt: at, updatedAt: at,
};
const otherWorkflow: Workflow = { ...workflow, id: "output-qa-other", name: "Other workflow", graph: { nodes: [], edges: [] } };
const run: RunHistoryItem = {
  id: "output-qa-run", workflowId: workflow.id, workflowName: workflow.name, trigger: "schedule",
  status: "completed", error: null, startedAt: at, finishedAt: at, createdAt: at,
  stepCount: ["multiple", "concurrent-retry"].includes(scenario) ? 2 : 1, finalOutputPreview: body,
};
const initialState: OutputReceipt["state"] = scenario === "not-requested" ? "not_requested"
  : scenario === "empty" ? "skipped_empty" : scenario === "deleted" ? "deleted"
  : ["failed", "retry-failure", "retry-refresh-failure", "concurrent-retry"].includes(scenario) ? "failed"
  : scenario === "pending" ? "pending" : scenario === "saving" ? "saving" : "saved";
const receipt: OutputReceipt = {
  runId: run.id, nodeId: currentNode.id, stepId: "output-qa-step",
  disposition: { ...capturedDisposition, saveToMemory: scenario !== "not-requested" },
  resolvedMemoryTitle: capturedDisposition.memoryTitle, state: initialState,
  storageIdentity: initialState === "not_requested" || initialState === "skipped_empty" ? null : storageId,
  outputSha256: initialState === "not_requested" || initialState === "skipped_empty" ? null : "a".repeat(64),
  artifactRequired: false, memoryId: initialState === "saved" ? storageId : null,
  errorCode: initialState === "failed" ? "database_write" : null,
  savingStartedAt: initialState === "saving" ? at : null,
  deletedAt: initialState === "deleted" ? at : null, createdAt: at, updatedAt: at,
};
const memory: OutputMemory = {
  ...memoryVersionFixture(),
  id: storageId, workflowId: workflow.id, runId: run.id, nodeId: currentNode.id,
  kind: "text", scopeType: "workflow", scopeKey: workflow.id, scopeLabel: workflow.name,
  memoryType: "output", source: "run", title: capturedDisposition.memoryTitle, body,
  pinned: capturedDisposition.pinMemory, confidence: 1, salience: 50, status: "active",
  provenanceState: "legacy_unknown", origin: "owned", createdAt: at, updatedAt: at,
};
let receipts = scenario === "legacy" ? [] : [receipt];
if (["multiple", "concurrent-retry"].includes(scenario)) receipts.push({ ...receipt,
  nodeId: "output-qa-secondary", stepId: "output-qa-step-secondary",
  storageIdentity: "95bcd97a-f7e0-428b-8818-63c81671e68e",
  disposition: { ...capturedDisposition, memoryTitle: "Secondary report" },
  resolvedMemoryTitle: "Secondary report", state: "failed", memoryId: null, errorCode: "database_write",
});
let memories = initialState === "saved" ? [memory] : [];
const history: RunHistoryDetail & { outputReceipts?: OutputReceipt[] } = {
  run, memoryUses: [], memoryContextSummaries: [], evidenceSources: [],
  steps: receipts.length > 1 ? receipts.map(item => makeStep(item.nodeId, item.stepId)) : [makeStep(currentNode.id, receipt.stepId)],
};
function makeStep(nodeId: string, id: string): RunHistoryDetail["steps"][number] {
  return { id, nodeId, agentProvider: null, skillName: "Release report", status: "completed",
    input: { nodeType: "chooseOutput" }, output: { text: body }, error: null,
    startedAt: at, finishedAt: at, createdAt: at };
}
const calls: Array<{ command: string; payload: unknown }> = [];
const errors: string[] = [];
const openedMemories: string[] = [];
const requestedMemories: WorkflowMemoryNavigationRequest[] = [];
let retried = false;
let retryCount = 0;
let releaseStaleHistory: (() => void) | null = null;
window.addEventListener("error", event => errors.push(String(event.error?.stack ?? event.message)));
window.addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
mockWindows("main");
mockIPC((command, payload: any) => {
  calls.push({ command, payload: structuredClone(payload ?? {}) });
  switch (command) {
    case "get_workflow": return structuredClone([workflow, otherWorkflow].find(item => item.id === payload.id) ?? null);
    case "get_workflow_schedule": return null;
    case "webhook_base_url": return "http://127.0.0.1:8787";
    case "list_run_history": return [structuredClone(run)];
    case "get_run_history":
      if (retried && scenario === "retry-refresh-failure") throw new Error("Synthetic history refresh unavailable");
      if (scenario === "concurrent-retry" && retryCount === 1) {
        const captured = structuredClone({ ...history, outputReceipts: receipts });
        return new Promise(resolve => { releaseStaleHistory = () => { releaseStaleHistory = null; resolve(captured); }; });
      }
      if (scenario === "concurrent-retry" && retryCount > 1) throw new Error("Synthetic second retry history refresh unavailable");
      return structuredClone({ ...history, ...(scenario === "legacy" ? {} : { outputReceipts: receipts }) });
    case "retry_output_persistence": {
      if (Object.keys(payload).sort().join(",") !== "nodeId,runId") throw new Error("Retry supplied replacement output settings");
      const target = receipts.find(item => item.runId === payload.runId && item.nodeId === payload.nodeId);
      if (!target || target.state !== "failed") throw new Error("Receipt is not retryable");
      if (scenario === "retry-failure") throw new Error("database_write");
      retried = true;
      retryCount += 1;
      const savedMemory = { ...memory, id: target.storageIdentity!, nodeId: target.nodeId,
        title: target.resolvedMemoryTitle, pinned: target.disposition.pinMemory };
      const saved = { ...target, state: "saved" as const, memoryId: savedMemory.id, errorCode: null };
      receipts = receipts.map(item => item.nodeId === target.nodeId ? saved : item);
      memories = [...memories.filter(item => item.id !== savedMemory.id), savedMemory];
      return structuredClone(saved);
    }
    case "list_memories":
      if (scenario === "refresh-failure") throw new Error("Synthetic library refresh unavailable");
      return structuredClone(memories.filter(item => item.workflowId === payload.workflowId));
    case "delete_memory":
      memories = memories.filter(item => item.id !== payload.id);
      receipts = receipts.map(item => item.memoryId === payload.id ? { ...item, state: "deleted", memoryId: null, deletedAt: at } : item);
      return null;
    case "get_memory_review_job": return null;
    case "get_memory_review_settings": return { enabled: false, provider: null, model: null, maxCandidates: 5, updatedAt: at };
    case "get_workflow_memory_review": return { workflowId: workflow.id, enabled: false, updatedAt: at };
    case "get_memory_lookup_availability":
      return { available: false, reason: "native_runtime_unavailable" };
    case "list_memory_candidates":
    case "list_memory_evidence":
    case "list_memory_reviews":
    case "list_workflow_triggers":
    case "list_app_trigger_statuses":
    case "list_linkable_memories":
    case "list_agent_providers":
    case "list_workflow_folders":
    case "search_history": return [];
    case "plugin:dialog|message": return "Ok";
    case "plugin:window|set_focus": return null;
    case "create_memory":
    case "start_workflow":
    default:
      errors.push(`Unexpected output QA IPC: ${command}`);
      throw new Error(`Unexpected output QA IPC: ${command}`);
  }
}, { shouldMockEvents: true });
useWorkflowStore.setState({
  workflows: [workflow, otherWorkflow], activeWorkflowId: inactive ? otherWorkflow.id : workflow.id,
  openWorkflowIds: [workflow.id, otherWorkflow.id], nodes: inactive ? [] : [currentNode], edges: [],
  memories: inactive ? [] : memories, loading: false, workflowLoading: false, error: null,
});
function replayCompletion() {
  useWorkflowStore.getState().handleRunEvent({
    runId: run.id, workflowId: workflow.id, kind: "step_completed", nodeId: currentNode.id,
    nodeType: "chooseOutput", nodeLabel: "Release report", status: "completed",
    message: "Synthetic completion replay", output: body, at,
  });
}
Object.assign(window, { outputFixture: {
  calls, errors, openedMemories, requestedMemories, scenario, inactive, replayCompletion,
  receipts: () => structuredClone(receipts), memories: () => structuredClone(memories),
} });
function Fixture() {
  const [memoryId, setMemoryId] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    let navigationGeneration = 0;
    const open = (event: Event) => {
      const request = (event as CustomEvent<WorkflowMemoryNavigationRequest>).detail ?? {};
      requestedMemories.push(structuredClone(request));
      const generation = ++navigationGeneration;
      void navigateWorkflowMemory(request, {
        activeWorkflowId: () => useWorkflowStore.getState().activeWorkflowId,
        loadWorkflowContext: id => useWorkflowStore.getState().ensureWorkflowMemoryContext(id),
        isCurrent: () => generation === navigationGeneration,
        openMemory: id => { if (id) openedMemories.push(id); setMemoryId(id); },
      });
    };
    window.addEventListener("alfred:open-memories", open);
    return () => { navigationGeneration += 1; window.removeEventListener("alfred:open-memories", open); };
  }, []);
  return <>
    <HistoryPage key={generation} activeWorkflowId={inactive ? null : workflow.id} initialRunId={run.id} onClose={() => {}} />
    <aside aria-label="Synthetic QA controls" style={{ position: "fixed", bottom: query.get("diagnostics") === "1" ? 58 : 8, left: 8, display: "flex", gap: 8, zIndex: 10 }}>
      <button type="button" className="ghost" onClick={replayCompletion}>Replay completion event</button>
      <button type="button" className="ghost" onClick={() => setGeneration(value => value + 1)}>Reload history</button>
      {scenario === "concurrent-retry" && <button type="button" className="ghost" onClick={() => releaseStaleHistory?.()}>Release stale history refresh</button>}
    </aside>
    {memoryId && <MemoriesInspector open initialMemoryId={memoryId} onOpenRunHistory={() => setMemoryId(null)} onClose={() => setMemoryId(null)} />}
    <QaEvidencePanel snapshot={() => ({ calls, errors, receipts, memories, openedMemories, requestedMemories,
      retryCount, staleHistoryHeld: releaseStaleHistory !== null,
      activeWorkflowId: useWorkflowStore.getState().activeWorkflowId,
      libraryMemoryIds: useWorkflowStore.getState().memories.map(item => item.id),
      storeError: useWorkflowStore.getState().error })} />
  </>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
