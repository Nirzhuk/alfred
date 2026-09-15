import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import "../../src/App.css";
import { applyResolvedTheme } from "../../src/features/settings/theme";
import { MemoriesInspector } from "../../src/features/workflow/components/memories-inspector/memories-inspector";
import { useWorkflowStore } from "../../src/features/workflow/store";
import type { MemoryCandidate, MemoryEvidenceItem, OutputMemory, Workflow } from "../../src/features/workflow/types";
import { candidateVersionFixture, memoryVersionFixture } from "./memory-version-fixture";
import { QaEvidencePanel } from "./qa-evidence-panel";
import { QaModalPortal } from "./qa-modal-portal";

// UI fixtures model IPC outcomes. The frozen Rust evaluation exercises real DB policy.
type VersionedCandidate = MemoryCandidate & ReturnType<typeof candidateVersionFixture>;
type VersionedMemory = OutputMemory & ReturnType<typeof memoryVersionFixture>;
const query = new URLSearchParams(location.search);
const scenario = query.get("scenario") ?? "replacement";
applyResolvedTheme(query.get("theme") === "dark" ? "dark" : "light");
const at = "2026-09-10T12:00:00Z";
const workflow: Workflow = {
  id: "correction-qa-workflow", name: "Deployment corrections QA", description: "Synthetic records only",
  workingDirectory: "/tmp/correction-qa", memoryRetrievalEnabled: true,
  memoryLookupEnabled: false,
  graph: { nodes: [], edges: [] }, createdAt: at, updatedAt: at,
};
const sourceText = scenario === "future"
  ? "ALFRED-4821 deploys to production from 2099-01-01T00:00:00Z."
  : scenario === "expired"
    ? "ALFRED-4821 deploys to production until 2026-08-02T00:00:00Z."
    : scenario === "retract"
      ? "The ALFRED-4821 staging routing policy has been withdrawn."
      : "ALFRED-4821 now deploys to production.";
const evidence: MemoryEvidenceItem = {
  ordinal: 0, sourceId: "correction-qa-source", fragmentId: "correction-qa-fragment",
  sourceRunId: "correction-qa-run", sourceStepId: "correction-qa-step",
  sourceNodeId: "deployment-observation", origin: "host_recorded_observation",
  producerIdentity: null, sourceLabel: "Recorded deployment observation", quote: sourceText,
  byteStart: 0, byteEnd: new TextEncoder().encode(sourceText).byteLength,
  normalizationVersion: 1, fragmentDigest: "a".repeat(64), sourceRemoved: false,
};
const target: VersionedMemory = {
  ...memoryVersionFixture(),
  id: "correction-qa-target", workflowId: workflow.id, runId: null, nodeId: null,
  kind: "note", scopeType: "workflow", scopeKey: workflow.id, scopeLabel: workflow.name,
  memoryType: "fact", source: "manual", title: "ALFRED-4821 routing policy",
  body: "ALFRED-4821 deploys to staging.", pinned: false, confidence: 1, salience: 60,
  status: "active", origin: "owned", provenanceState: "manual",
  evidenceSubject: null, evidenceIsInference: false, factKey: "routing:alfred-4821",
  observedAt: "2026-08-01T09:00:00Z", lastConfirmedAt: "2026-08-01T09:00:00Z",
  createdAt: "2026-08-01T09:00:00Z", updatedAt: at,
};
const operation = scenario === "retract" ? "retract" : scenario === "relationship" ? "create" : "supersede";
const candidate: VersionedCandidate = {
  ...candidateVersionFixture(),
  id: "correction-qa-candidate", reviewRunId: "correction-qa-run", workflowId: workflow.id,
  sourceNodeId: evidence.sourceNodeId, operation,
  targetMemoryId: operation === "create" ? null : target.id,
  expectedTargetMemoryId: operation === "create" ? null : target.id,
  expectedTargetSemanticRevision: operation === "create" ? null : target.semanticRevision,
  expectedTargetContentDigest: operation === "create" ? null : target.contentDigest,
  targetComplete: operation !== "create",
  scopeType: "workflow", scopeKey: workflow.id, memoryType: "fact",
  title: scenario === "retract" ? "Withdraw staging routing policy" : "ALFRED-4821 production routing",
  body: scenario === "retract" ? "The staging routing policy has been withdrawn." : "ALFRED-4821 deploys to production.",
  confidence: 0.87, rationale: "The recorded observation supports this routing change.",
  curationContractVersion: 1, evidenceState: "verified", evidenceSubject: "recorded_observation",
  isInference: false, claimDigest: "b".repeat(64), evidence: [evidence],
  factKey: "routing:alfred-4821",
  observedAt: scenario === "expired" ? "2026-08-01T09:00:00Z" : at,
  lastConfirmedAt: scenario === "expired" ? "2026-08-01T09:00:00Z" : at,
  validFrom: scenario === "future" ? "2099-01-01T00:00:00Z" : null,
  expiresAt: scenario === "expired" ? "2026-08-02T00:00:00Z" : null,
  possibleConflicts: scenario === "relationship" ? [{
    memoryId: target.id, title: target.title, relation: "possible_relationship",
  }] : [],
  status: "pending", blockedCode: null,
  createdAt: scenario === "expired" ? "2026-08-01T09:00:00Z" : at, decidedAt: null,
};
let candidates = [candidate];
let memories = [target];
let canonicalMutations = 0;
let candidateEdits = 0;
let mutationApplied = false;
const calls: Array<{ command: string; payload: unknown }> = [];
const errors: string[] = [];
const openedRuns: string[] = [];
window.addEventListener("error", event => errors.push(String(event.error?.stack ?? event.message)));
window.addEventListener("unhandledrejection", event => errors.push(String(event.reason)));

function mutateBackend() {
  if (mutationApplied) return;
  mutationApplied = true;
  if (["stale-target", "stale-manual", "refresh-before-edit"].includes(scenario)) {
    memories = memories.map(item => item.id !== target.id ? item : {
      ...item, body: "ALFRED-4821 deploys to canary.", semanticRevision: item.semanticRevision + 1,
      contentDigest: "e".repeat(64), // Deliberately preserve updatedAt.
      status: scenario === "stale-manual" ? "superseded" : item.status,
    });
  } else if (scenario === "stale-candidate" || scenario === "stale-candidate-edit") {
    candidates = candidates.map(item => ({
      ...item, title: "Routing correction edited in another window", candidateRevision: item.candidateRevision + 1,
    }));
  } else if (scenario === "source-removed") {
    candidates = candidates.map(item => ({
      ...item, evidenceState: "source_removed", status: "blocked", blockedCode: "source_removed",
      claimDigest: null, rationale: "", candidateRevision: item.candidateRevision + 1,
      evidence: item.evidence.map(source => ({ ...source, sourceId: null, quote: null,
        byteStart: null, byteEnd: null, normalizationVersion: null, fragmentDigest: null, sourceRemoved: true })),
    }));
  }
}
function decideBlocked(existing: VersionedCandidate, blockedCode: string) {
  const decided: VersionedCandidate = { ...existing, status: "blocked", blockedCode, decidedAt: at };
  candidates = candidates.map(item => item.id === decided.id ? decided : item);
  return structuredClone(decided);
}
mockWindows("main");
mockIPC((command, payload: any) => {
  calls.push({ command, payload: structuredClone(payload ?? {}) });
  switch (command) {
    case "list_memories": return structuredClone(memories);
    case "list_memory_candidates": return structuredClone(candidates);
    case "list_memory_evidence": return payload.memoryId === "correction-qa-created" ? [evidence] : [];
    case "update_memory": {
      const input = payload.input;
      const existing = memories.find(item => item.id === input.id);
      if (!existing) throw new Error("memory_not_found");
      if (input.expectedSemanticRevision !== existing.semanticRevision) throw new Error("memory_changed");
      const updated: VersionedMemory = { ...existing,
        title: input.title ?? existing.title, body: input.body ?? existing.body,
        status: input.status ?? existing.status, pinned: input.pinned ?? existing.pinned,
        semanticRevision: existing.semanticRevision + 1, contentDigest: "f".repeat(64),
      };
      memories = memories.map(item => item.id === updated.id ? updated : item);
      canonicalMutations += 1;
      return structuredClone(updated);
    }
    case "approve_memory_candidate": {
      const input = payload.input;
      if (!input || Object.keys(input).sort().join(",") !== "expectedCandidateRevision,id") {
        throw new Error("invalid_approval_request");
      }
      const existing = candidates.find(item => item.id === input.id);
      if (!existing || existing.status !== "pending") throw new Error("candidate_not_pending");
      if (existing.candidateRevision !== input.expectedCandidateRevision) throw new Error("candidate_changed");
      if (scenario === "future") throw new Error("validity_not_started");
      if (scenario === "expired") return decideBlocked(existing, "proposal_expired");
      const liveTarget = memories.find(item => item.id === existing.targetMemoryId);
      if (operation !== "create" && (!liveTarget || liveTarget.semanticRevision !== existing.expectedTargetSemanticRevision
        || liveTarget.contentDigest !== existing.expectedTargetContentDigest)) return decideBlocked(existing, "target_changed");
      if (operation !== "create") {
        memories = memories.map(item => item.id !== existing.targetMemoryId ? item : {
          ...item, status: operation === "retract" ? "retracted" : "superseded",
          semanticRevision: item.semanticRevision + 1,
        });
        canonicalMutations += 1;
      }
      if (operation !== "retract") {
        memories = [...memories, { ...target, id: "correction-qa-created", title: existing.title, body: existing.body,
          source: "review", provenanceState: "evidence_verified", evidenceSubject: existing.evidenceSubject,
          evidenceIsInference: existing.isInference, observedAt: existing.observedAt,
          lastConfirmedAt: existing.lastConfirmedAt, validFrom: existing.validFrom, expiresAt: existing.expiresAt }];
        canonicalMutations += 1;
      }
      const decided: VersionedCandidate = { ...existing, status: "approved", decidedAt: at };
      candidates = candidates.map(item => item.id === decided.id ? decided : item);
      return structuredClone(decided);
    }
    case "reject_memory_candidate": {
      const existing = candidates.find(item => item.id === payload.id);
      if (!existing || !["pending", "blocked"].includes(existing.status)) throw new Error("candidate_not_pending");
      const decided: VersionedCandidate = { ...existing, status: "rejected", decidedAt: at };
      candidates = candidates.map(item => item.id === decided.id ? decided : item);
      return structuredClone(decided);
    }
    case "update_memory_candidate": {
      const input = payload.input;
      const existing = candidates.find(item => item.id === input.id);
      if (!existing || existing.status !== "pending") throw new Error("candidate_not_editable");
      if (input.expectedCandidateRevision !== existing.candidateRevision) throw new Error("candidate_changed");
      if (!input.evidenceConfirmed) throw new Error("evidence_reconfirmation_required");
      const updated = { ...existing, title: input.title ?? existing.title, body: input.body ?? existing.body,
        scopeType: input.scopeType ?? existing.scopeType, memoryType: input.memoryType ?? existing.memoryType,
        validFrom: input.validFrom === undefined ? existing.validFrom : input.validFrom,
        expiresAt: input.expiresAt === undefined ? existing.expiresAt : input.expiresAt,
        candidateRevision: existing.candidateRevision + 1 };
      candidates = candidates.map(item => item.id === updated.id ? updated : item);
      candidateEdits += 1;
      return structuredClone(updated);
    }
    case "get_memory_review_settings": return { enabled: false, provider: "claude_code", model: null, maxCandidates: 5, updatedAt: at };
    case "get_memory_lookup_availability":
      return { available: false, reason: "native_runtime_unavailable" };
    case "get_workflow_memory_review": return { workflowId: workflow.id, enabled: false, updatedAt: at };
    case "list_memory_reviews":
    case "list_linkable_memories":
    case "list_workflow_folders":
    case "list_agent_providers": return [];
    case "clear_decided_memory_candidates": {
      const before = candidates.length;
      candidates = candidates.filter(item => item.status === "pending");
      return before - candidates.length;
    }
    case "plugin:dialog|message":
      return payload.buttons === "OkCancel" && query.get("confirm") === "cancel" ? "Cancel" : "Ok";
    case "plugin:window|set_focus": return null;
    default:
      errors.push(`Unmocked correction QA IPC: ${command}`);
      throw new Error(`Unmocked correction QA IPC: ${command}`);
  }
}, { shouldMockEvents: true });
useWorkflowStore.setState({ workflows: [workflow], activeWorkflowId: workflow.id, openWorkflowIds: [workflow.id],
  nodes: [], edges: [], memories, loading: false, workflowLoading: false, error: null });
const snapshot = () => ({ scenario, calls, errors, candidates, memories, canonicalMutations, candidateEdits, mutationApplied, openedRuns });
Object.assign(window, { correctionFixture: { snapshot, mutateBackend } });
function Fixture() {
  const [closed, setClosed] = useState(false);
  const [mutated, setMutated] = useState(false);
  const manualScenario = ["stale-manual", "refresh-before-edit"].includes(scenario);
  const hasMutation = ["stale-target", "stale-candidate", "stale-candidate-edit", "source-removed", "stale-manual", "refresh-before-edit"].includes(scenario);
  return <>
    {closed ? <p role="status">Dialog closed.</p> : <MemoriesInspector open
      initialMode={manualScenario ? "memories" : "suggestions"}
      initialMemoryId={manualScenario ? target.id : undefined}
      onOpenRunHistory={id => { openedRuns.push(id); }} onClose={() => setClosed(true)} />}
    {hasMutation && <QaModalPortal><aside aria-label="Synthetic concurrent change" style={{ position: "fixed", right: 8,
      bottom: 58, zIndex: 10000, background: "#fff", color: "#171717", padding: 8, border: "1px solid #aaa" }}>
      <button type="button" disabled={mutated} onClick={async () => {
        mutateBackend();
        if (manualScenario) await useWorkflowStore.getState().loadMemories(workflow.id);
        if (scenario === "stale-candidate-edit") await emit("memory://candidates-changed", {
          workflowId: workflow.id, pendingCount: 1,
        });
        setMutated(true);
      }}>
        {mutated ? "Concurrent change applied" : "Apply concurrent change"}
      </button>
    </aside></QaModalPortal>}
    <QaEvidencePanel snapshot={snapshot} />
  </>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
