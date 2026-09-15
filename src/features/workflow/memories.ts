import type { OutputMemory } from "./types";

const KEY = (workflowId: string) => `alfred:memories:${workflowId}`;

/** Legacy localStorage shape (pre-SQLite memories). */
type LegacyMemory = {
  id: string;
  workflowId: string;
  runId?: string | null;
  nodeId?: string | null;
  title: string;
  body: string;
  createdAt: string;
};

export function loadLegacyMemories(workflowId: string): LegacyMemory[] {
  try {
    const raw = localStorage.getItem(KEY(workflowId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as LegacyMemory[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function clearLegacyMemories(workflowId: string) {
  try {
    localStorage.removeItem(KEY(workflowId));
  } catch {
    /* ignore */
  }
}

export function removeMigratedLegacyMemories(
  workflowId: string,
  migratedIds: ReadonlySet<string>,
) {
  if (migratedIds.size === 0) return;
  const raw = localStorage.getItem(KEY(workflowId));
  if (!raw) return;
  const memories = JSON.parse(raw) as LegacyMemory[];
  if (!Array.isArray(memories)) {
    throw new Error("Saved memories could not be read.");
  }
  const remaining = memories.filter(
    (memory) => !migratedIds.has(memory.id),
  );
  if (remaining.length === 0) {
    localStorage.removeItem(KEY(workflowId));
  } else {
    localStorage.setItem(KEY(workflowId), JSON.stringify(remaining));
  }
}

export function sortMemories(memories: OutputMemory[]): OutputMemory[] {
  return [...memories].sort((a, b) => {
    const aActive = a.status === "active";
    const bActive = b.status === "active";
    if (aActive !== bActive) return aActive ? -1 : 1;
    const aPinned = aActive && a.pinned;
    const bPinned = bActive && b.pinned;
    if (aPinned !== bPinned) return aPinned ? -1 : 1;
    if (aActive && bActive) {
      const specificity = { workflow: 0, workspace: 1, user: 2 } as const;
      const scopeOrder = specificity[a.scopeType] - specificity[b.scopeType];
      if (scopeOrder !== 0) return scopeOrder;
    }
    return (b.updatedAt || b.createdAt).localeCompare(
      a.updatedAt || a.createdAt,
    );
  });
}

export function withMemoryDefaults(
  memory: Partial<OutputMemory> &
    Pick<OutputMemory, "id" | "title" | "body" | "createdAt" | "updatedAt">,
): OutputMemory {
  const workflowId = memory.workflowId ?? null;
  const scopeType = memory.scopeType ?? "workflow";
  return {
    workflowId,
    runId: null,
    nodeId: null,
    kind: "text",
    scopeType,
    scopeKey:
      memory.scopeKey ??
      (scopeType === "user" ? "local-user" : workflowId ?? ""),
    scopeLabel:
      memory.scopeLabel ??
      (scopeType === "user"
        ? "User"
        : scopeType === "workspace"
          ? "Workspace"
          : "Workflow"),
    memoryType:
      memory.memoryType ??
      (memory.kind === "artifact"
        ? "artifact"
        : memory.source === "manual"
          ? "note"
          : "output"),
    source: "run",
    artifactPath: null,
    artifactContentDigest: null,
    pinned: false,
    confidence: 1,
    salience: 50,
    status: "active",
    semanticRevision: 1,
    contentDigest: "",
    supersedesId: null,
    factKey: null,
    observedAt: null,
    lastConfirmedAt: null,
    validFrom: null,
    expiresAt: null,
    ...memory,
  };
}

export function asOwnedMemory(memory: OutputMemory): OutputMemory {
  return withMemoryDefaults({ ...memory, origin: memory.origin ?? "owned" });
}

export function canPinMemory(memory: OutputMemory): boolean {
  return memory.status === "active" && memory.origin !== "linked";
}

export function isMemoryPromptEligible(
  memory: OutputMemory,
  now: Date | number = Date.now(),
): boolean {
  if (memory.status !== "active") return false;
  const nowMs = typeof now === "number" ? now : now.getTime();
  if (memory.validFrom) {
    const validFrom = Date.parse(memory.validFrom);
    if (!Number.isFinite(validFrom) || validFrom > nowMs) return false;
  }
  if (!memory.expiresAt) return true;
  const expiresAt = Date.parse(memory.expiresAt);
  return Number.isFinite(expiresAt) && expiresAt > nowMs;
}

export function workspaceScopeAvailable(workingDirectory?: string): boolean {
  return Boolean(workingDirectory?.trim());
}
