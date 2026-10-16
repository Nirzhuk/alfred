import type {
  HistorySearchHit,
  RunHistoryMemoryUse,
  RunHistoryStep,
} from "../../types";

export type HistoryMode = "browse" | "search";
export type HistoryScope = "current" | "all";
export type HistoryNavigationAction =
  | { type: "open-run"; runId: string }
  | { type: "open-history" }
  | { type: "close-history" };

export function historyNavigation(action: HistoryNavigationAction): {
  view: "canvas" | "history";
  runId: string | null;
} {
  if (action.type === "open-run") {
    return { view: "history", runId: action.runId };
  }
  if (action.type === "open-history") {
    return { view: "history", runId: null };
  }
  return { view: "canvas", runId: null };
}

export function historyMode(query: string): HistoryMode {
  return query.trim() ? "search" : "browse";
}

export function historyWorkflowId(
  scope: HistoryScope,
  activeWorkflowId: string | null,
): string | null {
  return scope === "current" ? activeWorkflowId : null;
}

export function historyHitLabel(kind: HistorySearchHit["kind"]): string {
  return kind === "run_step" ? "Run step" : "Memory";
}

export function memoryUseReasonLabel(
  reason: RunHistoryMemoryUse["reason"],
): string {
  if (reason === "explicit") return "Selected in a Memory step";
  if (reason === "pinned") return "Pinned core";
  if (reason === "lexical") return "Matched this step's prompt";
  return "Recent fallback";
}

export type MemoryRecallSummary = {
  status: "included" | "abstained" | "unavailable" | null;
  omittedCount: number;
};

export function memoryRecallSummary(
  steps: Pick<RunHistoryStep, "input">[],
): MemoryRecallSummary {
  let status: MemoryRecallSummary["status"] = null;
  let omittedCount = 0;
  for (const step of steps) {
    if (!step.input || typeof step.input !== "object") continue;
    const input = step.input as Record<string, unknown>;
    const nextStatus = input.memoryRecallStatus;
    if (
      nextStatus === "included" ||
      nextStatus === "abstained" ||
      nextStatus === "unavailable"
    ) {
      if (
        nextStatus === "unavailable" ||
        (nextStatus === "abstained" && status !== "unavailable") ||
        status === null
      ) {
        status = nextStatus;
      }
    }
    const omitted = input.memoryRecallOmittedCount;
    if (
      typeof omitted === "number" &&
      Number.isFinite(omitted) &&
      omitted > 0
    ) {
      omittedCount += Math.floor(omitted);
    }
  }
  return { status, omittedCount };
}

export type WorkflowMemoryNavigationRequest = {
  memoryId?: string;
  workflowId?: string;
};

export function openHistoryMemory(
  memoryId: string,
  workflowId?: string,
): void {
  const detail: WorkflowMemoryNavigationRequest = workflowId
    ? { memoryId, workflowId }
    : { memoryId };
  window.dispatchEvent(
    new CustomEvent("alfred:open-memories", { detail }),
  );
}

export async function navigateWorkflowMemory(
  request: WorkflowMemoryNavigationRequest,
  adapter: {
    activeWorkflowId: () => string | null;
    loadWorkflowContext: (workflowId: string) => Promise<boolean>;
    isCurrent: () => boolean;
    openMemory: (memoryId: string | null) => void;
  },
): Promise<boolean> {
  if (!adapter.isCurrent()) return false;
  const workflowId = request.workflowId?.trim();
  if (workflowId) {
    try {
      if (!(await adapter.loadWorkflowContext(workflowId))) return false;
    } catch {
      return false;
    }
  }
  if (
    !adapter.isCurrent() ||
    (workflowId && adapter.activeWorkflowId() !== workflowId)
  ) {
    return false;
  }
  adapter.openMemory(request.memoryId ?? null);
  return true;
}

/** Deep-link from a History run to its Suggestions in the Memories inspector. */
export function openHistorySuggestions(runId: string): void {
  window.dispatchEvent(
    new CustomEvent("alfred:open-suggestions", { detail: { runId } }),
  );
}

export function isCurrentHistoryGeneration(
  requestGeneration: number,
  currentGeneration: number,
): boolean {
  return requestGeneration === currentGeneration;
}

export function literalHistorySnippet(snippet: string): string {
  return snippet;
}

export function formatHistoryJson(value: unknown): string {
  try {
    if (
      value &&
      typeof value === "object" &&
      (value as Record<string, unknown>).version === 1 &&
      "body" in value
    ) {
      const snapshot = value as Record<string, unknown>;
      const body =
        typeof snapshot.body === "string"
          ? snapshot.body
          : JSON.stringify(snapshot.body, null, 2) ?? "null";
      const notices = [
        snapshot.redacted === true ? "Some sensitive values were redacted." : null,
        snapshot.truncated === true
          ? `Input was truncated to ${String(snapshot.capturedBytes ?? "?")} of ${String(snapshot.originalBytes ?? "?")} bytes.`
          : null,
      ].filter(Boolean);
      return notices.length > 0 ? `${notices.join("\n")}\n\n${body}` : body;
    }
    return JSON.stringify(value, null, 2) ?? "null";
  } catch {
    return "Unable to display persisted value.";
  }
}

export function formatHistoryWhen(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
