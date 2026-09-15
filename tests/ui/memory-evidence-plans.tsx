import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import "../../src/App.css";
import { applyResolvedTheme } from "../../src/features/settings/theme";
import { MemoriesInspector } from "../../src/features/workflow/components/memories-inspector/memories-inspector";
import { useWorkflowStore } from "../../src/features/workflow/store";
import type { MemoryCandidate, MemoryEvidenceItem, OutputMemory, Workflow } from "../../src/features/workflow/types";
import { QaEvidencePanel } from "./qa-evidence-panel";
import { memoryVersionFixture, candidateVersionFixture } from "./memory-version-fixture";

const query = new URLSearchParams(location.search);
const scenario = query.get("scenario") ?? "normal";
const screen = query.get("screen") ?? "suggestions";
applyResolvedTheme(query.get("theme") === "dark" ? "dark" : "light");
const at = "2026-09-09T12:00:00Z";
const workflow: Workflow = {
  id: "evidence-qa-workflow", name: "Release evidence QA", description: "Synthetic evidence only",
  workingDirectory: "/tmp/evidence-qa", memoryRetrievalEnabled: true,
  memoryLookupEnabled: false,
  graph: { nodes: [], edges: [] }, createdAt: at, updatedAt: at,
};
function citation(ordinal: number, quote: string): MemoryEvidenceItem {
  return {
    ordinal, sourceId: `evidence-qa-source-${ordinal}`, fragmentId: `evidence-qa-fragment-${ordinal}`,
    sourceRunId: "evidence-qa-run", sourceStepId: `evidence-qa-step-${ordinal}`,
    sourceNodeId: `configured-step-${ordinal}`, origin: "configured_workflow_instruction",
    producerIdentity: null, sourceLabel: ordinal === 0 ? "Release instructions" : "Notification instructions",
    quote, byteStart: 0, byteEnd: new TextEncoder().encode(quote).byteLength,
    normalizationVersion: 1, fragmentDigest: "a".repeat(64), sourceRemoved: false,
  };
}
const primary = citation(0, scenario === "malicious"
  ? '<img src="https://invalid.example/evidence-probe" onerror="window.evidenceInjected=true">'
  : "Require human approval before publishing the release.");
const secondary = citation(1, "Post the Slack notification only after release approval.");
const inferred = scenario === "multi" || scenario === "queue-cleared";
const evidence = inferred ? [primary, secondary] : [primary];
const removedEvidence = evidence.map(item => ({
  ...item, sourceId: null, quote: null, byteStart: null, byteEnd: null,
  normalizationVersion: null, fragmentDigest: null, sourceRemoved: true,
}));
type VersionedCandidate = MemoryCandidate & ReturnType<typeof candidateVersionFixture>;
const candidate: VersionedCandidate = {
  ...candidateVersionFixture(),
  id: "evidence-qa-candidate", reviewRunId: "evidence-qa-run", workflowId: workflow.id,
  sourceNodeId: scenario === "multi" ? null : primary.sourceNodeId,
  operation: "create", targetMemoryId: null, scopeType: "workflow", scopeKey: workflow.id,
  memoryType: "constraint", title: "Require release approval",
  body: "This workflow requires human approval before publishing.", confidence: 0.82,
  rationale: "The saved workflow instructions explicitly require approval.",
  curationContractVersion: scenario === "legacy" ? 0 : 1,
  evidenceState: scenario === "legacy" ? "legacy_unavailable" : scenario === "removed" ? "source_removed" : scenario === "needs-reconfirmation" ? "needs_reconfirmation" : "verified",
  evidenceSubject: scenario === "legacy" ? null : "workflow_configuration",
  isInference: inferred, claimDigest: scenario === "legacy" || scenario === "removed" ? null : "b".repeat(64),
  evidence: scenario === "legacy" || scenario === "empty-evidence" ? [] : scenario === "removed" ? removedEvidence : evidence,
  status: scenario === "legacy" || scenario === "removed" ? "blocked" : "pending",
  blockedCode: scenario === "legacy" ? "legacy_evidence_unavailable" : scenario === "removed" ? "source_removed" : null,
  createdAt: at, decidedAt: null,
};
const memory: OutputMemory & { provenanceState: string } = {
  ...memoryVersionFixture(),
  id: "evidence-qa-approved", workflowId: workflow.id, runId: "evidence-qa-run", nodeId: primary.sourceNodeId,
  kind: "note", scopeType: "workflow", scopeKey: workflow.id, scopeLabel: workflow.name,
  memoryType: "constraint", source: scenario === "manual" ? "manual" : "review",
  title: "Approved release policy", body: "Human approval is required before release publication.",
  pinned: false, confidence: 0.82, salience: 70, status: "active", origin: "owned",
  provenanceState: scenario === "manual" ? "manual" : scenario === "legacy" ? "legacy_unknown" : scenario === "removed" ? "source_removed" : "evidence_verified",
  evidenceSubject: scenario === "manual" || scenario === "legacy" ? null : "workflow_configuration",
  evidenceIsInference: inferred,
  createdAt: at, updatedAt: at,
};
let candidates = scenario === "queue-cleared" ? [] : [candidate];
let memories: OutputMemory[] = [memory];
const canonicalEvidence = new Map<string, MemoryEvidenceItem[]>([[memory.id,
  scenario === "manual" || scenario === "legacy" ? [] : scenario === "removed" ? removedEvidence : evidence,
]]);
const calls: Array<{ command: string; payload: unknown }> = [];
const errors: string[] = [];
const openedRuns: string[] = [];
window.addEventListener("error", event => errors.push(String(event.error?.stack ?? event.message)));
window.addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
mockWindows("main");
mockIPC((command, payload: any) => {
  calls.push({ command, payload: structuredClone(payload ?? {}) });
  switch (command) {
    case "list_memories": return structuredClone(memories);
    case "list_memory_candidates": return structuredClone(candidates);
    case "list_memory_evidence":
      if (scenario === "evidence-failure") throw new Error("evidence_unavailable");
      return structuredClone(canonicalEvidence.get(payload.memoryId) ?? []);
    case "update_memory_candidate": {
      if (scenario === "edit-failure") throw new Error("evidence_validation_failed");
      const input = payload.input;
      if (!input.evidenceConfirmed) throw new Error("evidence_reconfirmation_required");
      const existing = candidates.find(item => item.id === input.id);
      if (!existing || existing.status !== "pending") throw new Error("candidate_not_editable");
      if (input.expectedCandidateRevision !== existing.candidateRevision) throw new Error("candidate_changed");
      const updated: VersionedCandidate = { ...existing, title: input.title ?? existing.title,
        body: input.body ?? existing.body, scopeType: input.scopeType ?? existing.scopeType,
        memoryType: input.memoryType ?? existing.memoryType, evidenceState: "verified",
        claimDigest: "c".repeat(64), candidateRevision: existing.candidateRevision + 1 };
      candidates = candidates.map(item => item.id === updated.id ? updated : item);
      return structuredClone(updated);
    }
    case "approve_memory_candidate": {
      const input = payload.input;
      const existing = candidates.find(item => item.id === input?.id);
      if (!existing || existing.status !== "pending") throw new Error("candidate_not_pending");
      if (input.expectedCandidateRevision !== existing.candidateRevision) throw new Error("candidate_changed");
      if (scenario === "approval-failure") throw new Error("evidence_validation_failed");
      if (existing.evidenceState !== "verified" || existing.evidence.length === 0) throw new Error("candidate_evidence_missing");
      const updated: VersionedCandidate = { ...existing, status: "approved", decidedAt: at };
      candidates = candidates.map(item => item.id === updated.id ? updated : item);
      const saved = { ...memory, id: "evidence-qa-new-approved", title: existing.title, body: existing.body,
        evidenceSubject: existing.evidenceSubject, evidenceIsInference: existing.isInference };
      memories = [...memories, saved];
      canonicalEvidence.set(saved.id, structuredClone(existing.evidence));
      return structuredClone(updated);
    }
    case "reject_memory_candidate": {
      const existing = candidates.find(item => item.id === payload.id);
      if (!existing || !["pending", "blocked"].includes(existing.status)) throw new Error("candidate_not_pending");
      const updated: VersionedCandidate = { ...existing, status: "rejected", decidedAt: at };
      candidates = candidates.map(item => item.id === updated.id ? updated : item);
      return structuredClone(updated);
    }
    case "clear_decided_memory_candidates": {
      const oldCount = candidates.length;
      candidates = candidates.filter(item => item.status === "pending");
      return oldCount - candidates.length;
    }
    case "get_memory_lookup_availability":
      return { available: false, reason: "native_runtime_unavailable" };
    case "get_memory_review_settings": return { enabled: false, provider: "claude_code", model: null, maxCandidates: 5, updatedAt: at };
    case "get_workflow_memory_review": return { workflowId: workflow.id, enabled: false, updatedAt: at };
    case "list_memory_reviews":
    case "list_linkable_memories":
    case "list_workflow_folders":
    case "list_agent_providers": return [];
    case "plugin:dialog|message": return "Ok";
    case "plugin:window|set_focus": return null;
    default:
      errors.push(`Unmocked evidence QA IPC: ${command}`);
      throw new Error(`Unmocked evidence QA IPC: ${command}`);
  }
}, { shouldMockEvents: true });
useWorkflowStore.setState({
  workflows: [workflow], activeWorkflowId: workflow.id, openWorkflowIds: [workflow.id],
  nodes: [], edges: [], memories, loading: false, workflowLoading: false, error: null,
});
Object.assign(window, { evidenceFixture: {
  calls, errors, openedRuns, scenario,
  candidates: () => structuredClone(candidates),
  memories: () => structuredClone(memories),
} });
function Fixture() {
  const [closed, setClosed] = useState(false);
  const [openedRun, setOpenedRun] = useState<string | null>(null);
  if (openedRun) return <p role="status">Source run requested: {openedRun}</p>;
  if (closed) return <p role="status">Dialog closed.</p>;
  return <><MemoriesInspector open initialMode={screen === "library" ? "memories" : "suggestions"}
    initialMemoryId={screen === "library" ? memory.id : undefined}
    onOpenRunHistory={runId => { openedRuns.push(runId); setOpenedRun(runId); }}
    onClose={() => setClosed(true)} />
    <QaEvidencePanel snapshot={() => ({ calls, errors, candidates, memories, openedRuns })} />
  </>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
