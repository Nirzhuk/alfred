import { confirm as confirmDialog } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { Icon } from "../../../../components/icon";
import {
  memoryReviewFailureCopy,
  useMemoryReviewStore,
  workflowSuggestionGate,
} from "../../../settings/memory-review";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  MenuItem,
  MenuSeparator,
} from "../../../../components/menu";
import { Modal, ModalHeader } from "../../../../components/modal";
import * as api from "../../api";
import {
  createHtmlReportPreview,
} from "../../html-report";
import { useWorkflowStore } from "../../store";
import { workspaceScopeAvailable } from "../../memories";
import type {
  MemoryCandidate,
  MemoryKind,
  MemoryReviewJob,
  MemoryScopeType,
  MemoryStatus,
  MemoryType,
  OutputMemory,
} from "../../types";
import {
  CANDIDATE_OPERATION_LABELS,
  CANDIDATE_STATUS_LABELS,
  SUGGESTION_SCOPE_TYPES,
  applyCandidateUpdate,
  blockedCandidateCopy,
  candidateApprovalRequiresConfirmation,
  candidateConfidencePercent,
  candidateEditPayload,
  candidateIsEditable,
  countPendingSuggestions,
  failedReviewJobs,
  hasCandidateEdits,
  shouldRefreshSuggestions,
  sortSuggestions,
  reviewStatusLabel,
  type CandidateEditDraft,
} from "./suggestions-model";
import {
  filterAndSortMemories,
  memorySearchSnippet,
  type MemoryKindFilter,
  type MemoryQuickFilter,
} from "./memory-list-model";

const KIND_LABELS: Record<MemoryKind, string> = {
  note: "Note",
  text: "Output",
  artifact: "Artifact",
};

type ScopeFilter = "all" | "user" | "workspace" | "workflow" | "inactive";

const MEMORY_TYPES: MemoryType[] = [
  "preference",
  "fact",
  "decision",
  "constraint",
  "lesson",
  "episode",
  "checkpoint",
  "note",
  "output",
  "artifact",
];

function formatWhen(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function highlightText(text: string, query: string): ReactNode {
  const q = query.trim();
  if (!q) return text;
  const index = text.toLowerCase().indexOf(q.toLowerCase());
  if (index < 0) return text;
  return (
    <>
      {text.slice(0, index)}
      <mark>{text.slice(index, index + q.length)}</mark>
      {text.slice(index + q.length)}
    </>
  );
}

function fitHtmlPreview(frame: HTMLIFrameElement | null) {
  const document = frame?.contentDocument;
  if (!frame || !document) return;

  frame.style.height = "1px";
  const height = Math.max(
    document.documentElement.scrollHeight,
    document.body?.scrollHeight ?? 0,
  );
  frame.style.height = `${Math.max(300, height)}px`;
}

export type MemoriesInspectorMode = "memories" | "suggestions";

type Props = {
  open: boolean;
  initialMemoryId?: string | null;
  initialMode?: MemoriesInspectorMode;
  /** Open the Suggestions queue focused on this run's review. */
  focusRunId?: string | null;
  onOpenRunHistory: (runId: string) => void;
  onClose: () => void;
};

export function MemoriesInspector({
  open,
  initialMemoryId,
  initialMode = "memories",
  focusRunId = null,
  onOpenRunHistory,
  onClose,
}: Props) {
  const memories = useWorkflowStore((s) => s.memories);
  const activeWorkflowId = useWorkflowStore((s) => s.activeWorkflowId);
  const workflows = useWorkflowStore((s) => s.workflows);
  const addMemory = useWorkflowStore((s) => s.addMemory);
  const linkMemory = useWorkflowStore((s) => s.linkMemory);
  const unlinkMemory = useWorkflowStore((s) => s.unlinkMemory);
  const updateMemoryFields = useWorkflowStore((s) => s.updateMemoryFields);
  const togglePinMemory = useWorkflowStore((s) => s.togglePinMemory);
  const removeMemory = useWorkflowStore((s) => s.removeMemory);
  const clearMemories = useWorkflowStore((s) => s.clearMemories);
  const setMemoryRetrievalEnabled = useWorkflowStore(
    (s) => s.setMemoryRetrievalEnabled,
  );

  const [query, setQuery] = useState("");
  const [quickFilter, setQuickFilter] = useState<MemoryQuickFilter>("all");
  const [kindFilter, setKindFilter] = useState<MemoryKindFilter>("all");
  const [scopeFilter, setScopeFilter] = useState<ScopeFilter>("all");
  const [typeFilter, setTypeFilter] = useState<MemoryType | "all">("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [kind, setKind] = useState<MemoryKind>("text");
  const [scopeType, setScopeType] = useState<MemoryScopeType>("workflow");
  const [memoryType, setMemoryType] = useState<MemoryType>("note");
  const [status, setStatus] = useState<MemoryStatus>("active");
  const [salience, setSalience] = useState(50);
  const [confidence, setConfidence] = useState(1);
  const [lastConfirmedAt, setLastConfirmedAt] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [editing, setEditing] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [linkingId, setLinkingId] = useState<string | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [showHeaderMenu, setShowHeaderMenu] = useState(false);
  const [showDetailMenu, setShowDetailMenu] = useState(false);
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const [linkerQuery, setLinkerQuery] = useState("");
  const [linkerLoading, setLinkerLoading] = useState(false);
  const [linkerError, setLinkerError] = useState<string | null>(null);
  const [linkable, setLinkable] = useState<OutputMemory[]>([]);
  const [showLinker, setShowLinker] = useState(false);
  const [viewSource, setViewSource] = useState(false);
  const [htmlExpanded, setHtmlExpanded] = useState(false);
  const [recallSaving, setRecallSaving] = useState(false);
  const [mode, setMode] = useState<MemoriesInspectorMode>("memories");
  const [candidates, setCandidates] = useState<MemoryCandidate[]>([]);
  const [reviewJobs, setReviewJobs] = useState<MemoryReviewJob[]>([]);
  const [suggestionsLoading, setSuggestionsLoading] = useState(false);
  const [suggestionsError, setSuggestionsError] = useState(false);
  const [selectedCandidateId, setSelectedCandidateId] = useState<
    string | null
  >(null);
  const [candidateEditing, setCandidateEditing] = useState(false);
  const [candidateDraft, setCandidateDraft] = useState<CandidateEditDraft>({
    title: "",
    body: "",
    scopeType: "workflow",
    memoryType: "note",
  });
  const [candidateBusy, setCandidateBusy] = useState<string | null>(null);
  const [workflowReviewEnabled, setWorkflowReviewEnabled] = useState(false);
  const [workflowReviewSaving, setWorkflowReviewSaving] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const detailRef = useRef<HTMLElement>(null);
  const linkerSearchRef = useRef<HTMLInputElement>(null);
  const htmlPreviewRef = useRef<HTMLIFrameElement>(null);

  const htmlPreview = useMemo(() => createHtmlReportPreview(body), [body]);
  const activeWorkflow = workflows.find(({ id }) => id === activeWorkflowId);
  const automaticRecall = activeWorkflow?.memoryRetrievalEnabled ?? false;
  const hasWorkingDirectory = workspaceScopeAvailable(
    activeWorkflow?.workingDirectory,
  );

  const reviewSettings = useMemoryReviewStore((s) => s.settings);
  const loadReviewSettings = useMemoryReviewStore((s) => s.load);
  const reviewGateReason = workflowSuggestionGate(reviewSettings);
  const pendingCount = countPendingSuggestions(candidates);
  const sortedCandidates = useMemo(
    () => sortSuggestions(candidates),
    [candidates],
  );
  const failedJobs = useMemo(
    () => failedReviewJobs(reviewJobs),
    [reviewJobs],
  );
  const selectedCandidate =
    candidates.find(({ id }) => id === selectedCandidateId) ?? null;

  useEffect(() => {
    if (!htmlPreview || viewSource || editing) return;
    const resize = () => fitHtmlPreview(htmlPreviewRef.current);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [htmlPreview, viewSource, editing]);

  const scopeTypeFiltered = useMemo(
    () =>
      memories.filter((m) => {
        if (scopeFilter === "inactive" && m.status === "active") return false;
        if (
          scopeFilter !== "all" &&
          scopeFilter !== "inactive" &&
          m.scopeType !== scopeFilter
        ) {
          return false;
        }
        if (typeFilter !== "all" && m.memoryType !== typeFilter) return false;
        return true;
      }),
    [memories, scopeFilter, typeFilter],
  );

  const filteredLinkable = useMemo(() => {
    const q = linkerQuery.trim().toLowerCase();
    if (!q) return linkable;
    return linkable.filter(
      (memory) =>
        memory.title.toLowerCase().includes(q) ||
        memory.body.toLowerCase().includes(q) ||
        (memory.sourceWorkflowName ?? "").toLowerCase().includes(q),
    );
  }, [linkable, linkerQuery]);

  const filtered = useMemo(
    () =>
      filterAndSortMemories(scopeTypeFiltered, query, quickFilter, kindFilter),
    [scopeTypeFiltered, query, quickFilter, kindFilter],
  );

  const linkedCount = memories.filter((m) => m.origin === "linked").length;

  const selected =
    filtered.find((m) => m.id === selectedId) ??
    memories.find((m) => m.id === selectedId) ??
    null;
  const isLinkedSelected = selected?.origin === "linked";

  useEffect(() => {
    if (!open) return;
    const preferred =
      initialMemoryId && memories.some((m) => m.id === initialMemoryId)
        ? initialMemoryId
        : (memories[0]?.id ?? null);
    setSelectedId(preferred);
    setQuery("");
    setQuickFilter("all");
    setKindFilter("all");
    setScopeFilter("all");
    setTypeFilter("all");
    setEditing(false);
    setCreating(false);
    setShowLinker(false);
    setViewSource(false);
    setHtmlExpanded(false);
    setDetailOpen(Boolean(initialMemoryId));
    setMode(initialMode);
    setSelectedCandidateId(null);
    setCandidateEditing(false);
    setSuggestionsError(false);
  }, [open, initialMemoryId, activeWorkflowId, initialMode]);

  useEffect(() => {
    if (!open) return;
    void loadReviewSettings();
  }, [open, loadReviewSettings]);

  // Per-workflow review flag lives in SQLite; fetch it for this workflow.
  useEffect(() => {
    if (!open || !activeWorkflowId) {
      setWorkflowReviewEnabled(false);
      return;
    }
    let cancelled = false;
    setWorkflowReviewSaving(true);
    void api
      .getWorkflowMemoryReview(activeWorkflowId)
      .then((row) => {
        if (!cancelled) setWorkflowReviewEnabled(row?.enabled ?? false);
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setWorkflowReviewSaving(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, activeWorkflowId]);

  const loadSuggestions = async (
    workflowId: string,
  ): Promise<{ candidates: MemoryCandidate[]; jobs: MemoryReviewJob[] } | null> => {
    try {
      const [nextCandidates, nextJobs] = await Promise.all([
        api.listMemoryCandidates({ workflowId }),
        api.listMemoryReviews(workflowId),
      ]);
      return { candidates: nextCandidates, jobs: nextJobs };
    } catch {
      return null;
    }
  };

  // Keep keyboard focus on the same suggestion across background refreshes.
  const refreshPreservingFocus = async (workflowId: string) => {
    const activeElement =
      document.activeElement as HTMLElement | null;
    const focusKey = activeElement
      ?.closest("[data-suggestion-id]")
      ?.getAttribute("data-suggestion-id");
    const result = await loadSuggestions(workflowId);
    if (!result) {
      setSuggestionsError(true);
      return;
    }
    setSuggestionsError(false);
    setCandidates(result.candidates);
    setReviewJobs(result.jobs);
    if (focusKey) {
      requestAnimationFrame(() => {
        const restored = document.querySelector<HTMLElement>(
          `[data-suggestion-id="${focusKey}"]`,
        );
        restored?.focus();
      });
    }
  };

  useEffect(() => {
    if (!open || mode !== "suggestions" || !activeWorkflowId) return;
    let cancelled = false;
    setSuggestionsLoading(true);
    void loadSuggestions(activeWorkflowId).then((result) => {
      if (cancelled) return;
      if (result) {
        setSuggestionsError(false);
        setCandidates(result.candidates);
        setReviewJobs(result.jobs);
        if (focusRunId && !selectedCandidateId) {
          const focused = result.candidates.find(
            ({ reviewRunId }) => reviewRunId === focusRunId,
          );
          if (focused) setSelectedCandidateId(focused.id);
        }
      } else {
        setSuggestionsError(true);
      }
      setSuggestionsLoading(false);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, mode, activeWorkflowId, focusRunId]);

  // Backend-driven refresh: only the affected active workflow reloads.
  useEffect(() => {
    if (!open || !activeWorkflowId || mode !== "suggestions") return;
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    void listen<{ workflowId: string; pendingCount: number }>(
      "memory://candidates-changed",
      (event) => {
        if (!shouldRefreshSuggestions(event.payload, activeWorkflowId, open))
          return;
        void refreshPreservingFocus(activeWorkflowId).then(() => {
          setAnnouncement(
            `${event.payload.pendingCount} pending memory suggestion${
              event.payload.pendingCount === 1 ? "" : "s"
            }.`,
          );
        });
      },
    ).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, activeWorkflowId, mode]);

  const selectCandidate = (candidate: MemoryCandidate) => {
    setSelectedCandidateId(candidate.id);
    setCandidateEditing(false);
    setCandidateDraft({
      title: candidate.title,
      body: candidate.body,
      scopeType: candidate.scopeType,
      memoryType: candidate.memoryType,
    });
  };

  const decideCandidate = async (
    candidate: MemoryCandidate,
    decision: "approve" | "reject",
  ) => {
    if (
      decision === "approve" &&
      candidateApprovalRequiresConfirmation(candidate)
    ) {
      const scopeNote =
        candidate.scopeType === "user"
          ? "This memory will be visible to every workflow on this installation."
          : "The target memory will be permanently retracted.";
      const confirmed = await confirmDialog(
        `${scopeNote} Approve this suggestion?`,
        { title: "Approve suggestion", kind: "warning" },
      );
      if (!confirmed) return;
    }
    setCandidateBusy(candidate.id);
    try {
      const updated =
        decision === "approve"
          ? await api.approveMemoryCandidate(candidate.id)
          : await api.rejectMemoryCandidate(candidate.id);
      setCandidates((list) => applyCandidateUpdate(list, updated));
      setAnnouncement(
        updated.status === "blocked"
          ? `Suggestion blocked. ${blockedCandidateCopy(updated.blockedCode)}`
          : decision === "approve"
            ? "Suggestion approved."
            : "Suggestion rejected.",
      );
    } catch {
      setAnnouncement("That action could not be completed. Try again.");
    } finally {
      setCandidateBusy(null);
    }
  };

  const saveCandidateEdits = async (candidate: MemoryCandidate) => {
    if (!hasCandidateEdits(candidate, candidateDraft)) {
      setCandidateEditing(false);
      return;
    }
    setCandidateBusy(candidate.id);
    try {
      const updated = await api.updateMemoryCandidate(
        candidateEditPayload(candidate, candidateDraft),
      );
      setCandidates((list) => applyCandidateUpdate(list, updated));
      setCandidateEditing(false);
      setAnnouncement("Suggestion updated.");
    } catch {
      setAnnouncement(
        "This edit was not accepted. Titles and bodies have length limits and cannot contain credentials.",
      );
    } finally {
      setCandidateBusy(null);
    }
  };

  const retryFailedReview = async (job: MemoryReviewJob) => {
    setCandidateBusy(job.runId);
    try {
      const retried = await api.retryMemoryReview(job.runId);
      setReviewJobs((jobs) =>
        jobs.map((item) => (item.runId === job.runId ? retried : item)),
      );
      setAnnouncement("Review queued again.");
    } catch {
      setAnnouncement(
        reviewGateReason
          ? reviewGateReason
          : "The review could not be retried. Check Memory review settings, then try again.",
      );
    } finally {
      setCandidateBusy(null);
    }
  };

  const toggleWorkflowReview = () => {
    if (!activeWorkflowId) return;
    setWorkflowReviewSaving(true);
    void api
      .setWorkflowMemoryReview(
        activeWorkflowId,
        !workflowReviewEnabled,
      )
      .then((row) => setWorkflowReviewEnabled(row.enabled))
      .catch(() => {})
      .finally(() => setWorkflowReviewSaving(false));
  };

  useEffect(() => {
    if (!open || !activeWorkflowId || !showLinker) return;
    let cancelled = false;
    setLinkerLoading(true);
    setLinkerError(null);
    void api
      .listLinkableMemories(activeWorkflowId)
      .then((rows) => {
        if (!cancelled) setLinkable(rows);
      })
      .catch(() => {
        if (!cancelled) setLinkerError("Could not load memories. Try again.");
      })
      .finally(() => {
        if (!cancelled) setLinkerLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, activeWorkflowId, showLinker, memories.length]);

  useEffect(() => {
    if (!open) return;
    if (selectedId && !memories.some((m) => m.id === selectedId)) {
      setSelectedId(memories[0]?.id ?? null);
    }
  }, [open, memories, selectedId]);

  useEffect(() => {
    if (!selected || creating || editing) return;
    setTitle(selected.title);
    setBody(selected.body);
    setKind(selected.kind);
    setScopeType(selected.scopeType);
    setMemoryType(selected.memoryType);
    setStatus(selected.status);
    setSalience(selected.salience);
    setConfidence(selected.confidence);
    setLastConfirmedAt(selected.lastConfirmedAt ?? "");
    setExpiresAt(selected.expiresAt ?? "");
    setDirty(false);
    setViewSource(false);
    setHtmlExpanded(false);
  }, [selected?.id, selected?.updatedAt, creating, editing]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const isTyping =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.tagName === "SELECT" ||
        target?.isContentEditable;
      if (event.key === "/" && !isTyping) {
        event.preventDefault();
        searchRef.current?.focus();
      } else if (event.key === "Escape" && showLinker) {
        event.stopPropagation();
        setShowLinker(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, showLinker]);

  useEffect(() => {
    if (showLinker) {
      requestAnimationFrame(() => linkerSearchRef.current?.focus());
    }
  }, [showLinker]);

  const confirmDiscard = async () => {
    if (!dirty || (!editing && !creating)) return true;
    return confirmDialog("Discard your unsaved changes?", {
      title: "Unsaved changes",
      kind: "warning",
    });
  };

  const requestClose = async () => {
    if (!(await confirmDiscard())) return;
    onClose();
  };

  const loadMemory = async (memory: OutputMemory) => {
    if (!(await confirmDiscard())) return;
    setEditing(false);
    setCreating(false);
    setSelectedId(memory.id);
    setViewSource(false);
    setHtmlExpanded(false);
    setDetailOpen(true);
  };

  const resetDraftFields = () => {
    setTitle("");
    setBody("");
    setKind("note");
    setScopeType("workflow");
    setMemoryType("note");
    setStatus("active");
    setSalience(50);
    setConfidence(1);
    setLastConfirmedAt("");
    setExpiresAt("");
  };

  const startNewNote = async () => {
    if (!(await confirmDiscard())) return;
    setEditing(true);
    setCreating(true);
    setSelectedId(null);
    resetDraftFields();
    setDirty(false);
    setViewSource(true);
    setHtmlExpanded(false);
    setDetailOpen(true);
  };

  const saveSelected = async () => {
    if (!title.trim() && !creating) return;
    setSaving(true);
    try {
      if (creating) {
        const previousIds = new Set(memories.map((memory) => memory.id));
        await addMemory({
          title: title.trim() || "Note",
          body,
          kind,
          scopeType,
          memoryType,
          source: "manual",
          salience,
          confidence,
          status,
          lastConfirmedAt: lastConfirmedAt || null,
          expiresAt: expiresAt || null,
        });
        setCreating(false);
        const created = useWorkflowStore
          .getState()
          .memories.find((memory) => !previousIds.has(memory.id));
        if (created) setSelectedId(created.id);
      } else if (selected) {
        if (
          selected.scopeType !== scopeType &&
          !window.confirm(
            `Move this memory from ${selected.scopeLabel} scope to ${scopeType} scope?`,
          )
        ) {
          return;
        }
        await updateMemoryFields({
          id: selected.id,
          title: title.trim() || selected.title,
          ...(body !== selected.body ? { body } : {}),
          kind,
          scopeType,
          memoryType,
          salience,
          confidence,
          status,
          lastConfirmedAt: lastConfirmedAt || null,
          expiresAt: expiresAt || null,
        });
      }
      setEditing(false);
      setDirty(false);
      setViewSource(false);
      setHtmlExpanded(false);
    } finally {
      setSaving(false);
    }
  };

  const cancelEditing = async () => {
    if (!(await confirmDiscard())) return;
    setEditing(false);
    setCreating(false);
    setDirty(false);
    if (selected) {
      setTitle(selected.title);
      setBody(selected.body);
    } else {
      setDetailOpen(false);
    }
  };

  const deleteSelected = async () => {
    if (!selected) return;
    const confirmed = await confirmDialog(
      `Delete “${selected.title}”? This cannot be undone.`,
      { title: "Delete memory", kind: "warning" },
    );
    if (!confirmed) return;
    const id = selected.id;
    await removeMemory(id);
  };

  const unlinkSelected = async () => {
    if (!selected) return;
    const confirmed = await confirmDialog(
      `Unlink “${selected.title}” from this workflow? The original memory will remain available.`,
      { title: "Unlink memory", kind: "warning" },
    );
    if (!confirmed) return;
    await unlinkMemory(selected.id);
  };

  const clearOwned = async () => {
    if (!(await confirmDiscard())) return;
    const ownedCount = memories.filter(
      (memory) => memory.origin !== "linked",
    ).length;
    const confirmed = await confirmDialog(
      `Delete ${ownedCount} owned ${
        ownedCount === 1 ? "memory" : "memories"
      }? Linked memories will remain.`,
      { title: "Clear owned memories", kind: "warning" },
    );
    if (!confirmed) return;
    await clearMemories();
  };

  const openLinker = () => {
    setLinkerQuery("");
    setShowLinker(true);
  };

  const linkSelectedMemory = async (memory: OutputMemory) => {
    setLinkingId(memory.id);
    try {
      const linked = await linkMemory(memory.id);
      if (linked) {
        setSelectedId(linked.id);
        setShowLinker(false);
        setCreating(false);
        setEditing(false);
        setDetailOpen(true);
      }
    } finally {
      setLinkingId(null);
    }
  };

  const handleListKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (!filtered.length) return;
    const currentIndex = filtered.findIndex(
      (memory) => memory.id === selectedId,
    );
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex =
        currentIndex < 0
          ? 0
          : (currentIndex + direction + filtered.length) % filtered.length;
      void loadMemory(filtered[nextIndex]);
    } else if (event.key === "Enter" && selected) {
      event.preventDefault();
      setDetailOpen(true);
      detailRef.current?.focus();
    }
  };

  const pinnedCount = memories.filter(
    (m) => m.pinned && m.origin !== "linked",
  ).length;
  const visiblePinnedBytes = memories
    .filter((memory) => memory.pinned && memory.status === "active")
    .reduce(
      (total, memory) =>
        total +
        new TextEncoder().encode(`${memory.title}\n${memory.body}`).length,
      0,
    );
  const supersedes = selected?.supersedesId
    ? memories.find(({ id }) => id === selected.supersedesId)
    : null;
  const supersededBy = selected
    ? memories.find(({ supersedesId }) => supersedesId === selected.id)
    : null;

  return (
    <Modal
      open={open}
      size="xl"
      className={`memories-inspector-modal${
        htmlExpanded ? " is-html-expanded" : ""
      }`}
      onClose={() => void requestClose()}
      labelledBy="memories-inspector-title"
      describedBy="memories-inspector-description"
      closeOnEscape={!showLinker}
    >
      <ModalHeader
        strong
        titleAs="h2"
        title="Memories"
        titleId="memories-inspector-title"
        description={
          memories.length === 0 ? (
            "No memories yet for this workflow."
          ) : (
            <span className="memories-header-stats">
              <span>{memories.length} memories</span>
              {pinnedCount ? <span>{pinnedCount} in next run</span> : null}
              {linkedCount ? <span>{linkedCount} linked</span> : null}
            </span>
          )
        }
        descriptionId="memories-inspector-description"
        actions={
          <>
            <button type="button" className="ghost" onClick={openLinker}>
              <Icon name="link" size={15} />
              Link memory
            </button>
            <button
              type="button"
              className="ghost"
              onClick={() => void startNewNote()}
            >
              <Icon name="plus" size={15} />
              New note
            </button>
            <DropdownMenu
              open={showHeaderMenu}
              onOpenChange={setShowHeaderMenu}
            >
              <DropdownMenuTrigger className="ghost memories-more-button">
                More
                <Icon name="caret-down" size={12} />
              </DropdownMenuTrigger>
              <DropdownMenuContent aria-label="Memory library actions">
                <MenuItem
                  danger
                  disabled={!memories.some((m) => m.origin !== "linked")}
                  onSelect={() => setShowHeaderMenu(false)}
                  onClick={() => void clearOwned()}
                >
                  Clear owned memories
                </MenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <button
              type="button"
              className="ghost modal-close-button"
              aria-label="Close"
              onClick={() => void requestClose()}
            >
              <Icon name="x" size={16} />
            </button>
          </>
        }
      />

      <div className="memory-recall-control">
        <div>
          <strong>Automatic recall</strong>
          <p>
            Relevant local memories may be added to agent prompts for this workflow. Uses local exact FTS5 search + recency, not an embedding service.
          </p>
          <span>
            Fixed limit: 8 items / 6,000 bytes per agent or custom-agent step.
          </span>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={automaticRecall}
          aria-label="Automatic recall"
          className={`settings-toggle${automaticRecall ? " is-on" : ""}`}
          disabled={!activeWorkflowId || recallSaving}
          onClick={() => {
            if (!activeWorkflowId) return;
            setRecallSaving(true);
            void setMemoryRetrievalEnabled(activeWorkflowId, !automaticRecall).finally(
              () => setRecallSaving(false),
            );
          }}
        >
          <span className="settings-toggle-knob" />
        </button>
      </div>

      <p className="sr-only" role="status" aria-live="polite">
        {announcement}
      </p>

      <div className="memory-recall-control">
        <div>
          <strong>Suggest memories after runs</strong>
          <p>
            After a completed run, the reviewer provider chosen in Settings may
            propose memory changes. Nothing is saved without your approval.
          </p>
          {reviewGateReason ? <span>{reviewGateReason}</span> : null}
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={workflowReviewEnabled}
          aria-label="Suggest memories after runs"
          className={`settings-toggle${
            workflowReviewEnabled ? " is-on" : ""
          }`}
          disabled={
            Boolean(reviewGateReason) ||
            !activeWorkflowId ||
            workflowReviewSaving
          }
          onClick={toggleWorkflowReview}
        >
          <span className="settings-toggle-knob" />
        </button>
      </div>

      <div
        className={`memories-inspector-body${
          detailOpen ? " is-detail-open" : ""
        }`}
      >
        <aside
          className="memories-inspector-list"
          aria-label="Memory library"
          onKeyDown={handleListKeyDown}
        >
          <div className="memories-mode-tabs" role="tablist" aria-label="Memories modes">
            <button
              type="button"
              role="tab"
              aria-selected={mode === "memories"}
              className={`ghost memories-filter${mode === "memories" ? " is-active" : ""}`}
              onClick={() => setMode("memories")}
            >
              All memories
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={mode === "suggestions"}
              className={`ghost memories-filter${mode === "suggestions" ? " is-active" : ""}`}
              onClick={() => setMode("suggestions")}
            >
              Suggestions
              {pendingCount > 0 ? (
                <span className="suggestion-pending-badge">{pendingCount}</span>
              ) : null}
            </button>
          </div>
          {mode === "memories" ? (
          <>
          <div className="memories-search-wrap">
            <Icon name="magnifying-glass" size={15} />
            <input
              ref={searchRef}
              type="search"
              className="memories-inspector-search"
              aria-label="Search memories"
              placeholder="Search memories"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <kbd>/</kbd>
          </div>
          <div className="memories-filter-row" role="tablist">
            {(
              [
                ["all", "All"],
                ["pinned", "Next run"],
                ["linked", "Linked"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={quickFilter === id}
                className={`ghost memories-filter${
                  quickFilter === id ? " is-active" : ""
                }`}
                onClick={() => setQuickFilter(id)}
              >
                {label}
              </button>
            ))}
            <select
              className="memories-kind-filter"
              aria-label="Filter by memory kind"
              value={kindFilter}
              onChange={(event) =>
                setKindFilter(event.target.value as MemoryKindFilter)
              }
            >
              <option value="all">All kinds</option>
              <option value="note">Notes</option>
              <option value="text">Outputs</option>
              <option value="artifact">Artifacts</option>
            </select>
          </div>
          <div className="memories-filter-selects">
            <label className="field">
              <span>Scope</span>
              <select
                value={scopeFilter}
                onChange={(event) =>
                  setScopeFilter(event.target.value as ScopeFilter)
                }
              >
                <option value="all">All scopes</option>
                <option value="user">User</option>
                <option value="workspace">Workspace</option>
                <option value="workflow">Workflow</option>
                <option value="inactive">Inactive</option>
              </select>
            </label>
            <label className="field">
              <span>Type</span>
              <select
                value={typeFilter}
                onChange={(event) =>
                  setTypeFilter(event.target.value as MemoryType | "all")
                }
              >
                <option value="all">All types</option>
                {MEMORY_TYPES.map((value) => (
                  <option key={value} value={value}>
                    {value}
                  </option>
                ))}
              </select>
            </label>
          </div>

          {filtered.length === 0 ? (
            <div className="memories-list-empty">
              <Icon name="note" size={22} />
              <p>
                {memories.length === 0
                  ? "Run a workflow, link a memory, or create a note."
                  : "No memories match your search and filters."}
              </p>
              {memories.length === 0 ? (
                <button
                  type="button"
                  className="ghost"
                  onClick={() => void startNewNote()}
                >
                  New note
                </button>
              ) : null}
            </div>
          ) : (
            <ul className="memories-results">
              {filtered.map((memory) => (
                <li key={memory.id}>
                  <button
                    type="button"
                    className={`memories-list-item${
                      memory.id === selectedId && !creating ? " is-active" : ""
                    }`}
                    onClick={() => void loadMemory(memory)}
                  >
                    <span className="memories-list-top">
                      <span className="memories-list-title">
                        {memory.pinned && memory.origin !== "linked" ? (
                          <Icon
                            name="push-pin"
                            size={12}
                            className="memories-pin-icon"
                          />
                        ) : null}
                        <span className="memories-list-title-text">
                          {highlightText(memory.title, query)}
                        </span>
                      </span>
                      <span className="memories-list-kind">
                        {KIND_LABELS[memory.kind]}
                      </span>
                    </span>
                    <span className="memories-list-preview">
                      {highlightText(memorySearchSnippet(memory, query), query)}
                    </span>
                    <span className="memories-list-meta-row">
                      <span>
                        {memory.origin === "linked"
                          ? `From ${memory.sourceWorkflowName ?? "workflow"}`
                          : `${memory.scopeType} · ${memory.memoryType}`}
                        {memory.status !== "active"
                          ? ` · ${memory.status}`
                          : ""}
                      </span>
                      <time>
                        {formatWhen(memory.updatedAt || memory.createdAt)}
                      </time>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          </>
          ) : (
            <div className="suggestions-list-wrap">
              {failedJobs.map((job) => (
                <div
                  key={job.runId}
                  className="suggestion-failed"
                  data-suggestion-id={`job-${job.runId}`}
                >
                  <div>
                    <strong>{reviewStatusLabel(job.status)}</strong>
                    <span>{memoryReviewFailureCopy(job.errorCode)}</span>
                  </div>
                  <button
                    type="button"
                    className="ghost"
                    disabled={candidateBusy === job.runId}
                    onClick={() => void retryFailedReview(job)}
                  >
                    {candidateBusy === job.runId ? "Retrying…" : "Retry review"}
                  </button>
                </div>
              ))}
              {suggestionsLoading && sortedCandidates.length === 0 ? (
                <div className="memories-list-empty">
                  <p>Loading suggestions…</p>
                </div>
              ) : suggestionsError && sortedCandidates.length === 0 ? (
                <div className="memories-list-empty" role="alert">
                  <p>Suggestions could not be loaded. Try again.</p>
                </div>
              ) : sortedCandidates.length === 0 && failedJobs.length === 0 ? (
                <div className="memories-list-empty">
                  <Icon name="note" size={22} />
                  <p>
                    No memory suggestions yet. They appear here after reviewed
                    runs when Memory review is on.
                  </p>
                </div>
              ) : (
                <ul className="memories-results suggestions-results">
                  {sortedCandidates.map((candidate) => (
                    <li key={candidate.id}>
                      <button
                        type="button"
                        data-suggestion-id={candidate.id}
                        className={`memories-list-item suggestion-item${
                          candidate.id === selectedCandidateId ? " is-active" : ""
                        }`}
                        onClick={() => selectCandidate(candidate)}
                      >
                        <span className="memories-list-top">
                          <span className="memories-list-title-text">
                            {candidate.title}
                          </span>
                          <span className={`suggestion-status is-${candidate.status}`}>
                            {CANDIDATE_STATUS_LABELS[candidate.status]}
                          </span>
                        </span>
                        <span className="suggestion-item-meta">
                          {CANDIDATE_OPERATION_LABELS[candidate.operation]} ·{" "}
                          {candidate.scopeType} · {candidate.memoryType} ·{" "}
                          {candidateConfidencePercent(candidate.confidence)}%
                          confident
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </aside>

        <section
          ref={detailRef}
          className="memories-inspector-detail"
          tabIndex={-1}
        >
          {mode === "suggestions" ? (
            selectedCandidate ? (
              <article
                className="suggestion-detail"
                data-suggestion-id={`detail-${selectedCandidate.id}`}
                aria-label="Memory suggestion"
              >
                <header className="memories-detail-header">
                  <div className="memories-detail-heading">
                    <div className="suggestion-chip-row">
                      <span className="suggestion-op-chip">
                        {CANDIDATE_OPERATION_LABELS[selectedCandidate.operation]}
                      </span>
                      <span className={`suggestion-status is-${selectedCandidate.status}`}>
                        {CANDIDATE_STATUS_LABELS[selectedCandidate.status]}
                      </span>
                      <span className="muted">
                        {candidateConfidencePercent(selectedCandidate.confidence)}%
                        confident · {formatWhen(selectedCandidate.createdAt)}
                      </span>
                    </div>
                    {!candidateEditing ? (
                      <h3>{selectedCandidate.title}</h3>
                    ) : null}
                  </div>
                </header>

                {selectedCandidate.status === "blocked" &&
                selectedCandidate.blockedCode ? (
                  <p className="suggestion-blocked" role="alert">
                    {blockedCandidateCopy(selectedCandidate.blockedCode)}
                  </p>
                ) : null}

                {candidateEditing && candidateIsEditable(selectedCandidate) ? (
                  <div className="memories-edit-form">
                    <label className="field">
                      <span>Title</span>
                      <input
                        type="text"
                        value={candidateDraft.title}
                        onChange={(event) =>
                          setCandidateDraft((draft) => ({
                            ...draft,
                            title: event.target.value,
                          }))
                        }
                      />
                    </label>
                    <div className="memories-detail-toolbar">
                      <label className="field memories-kind-field">
                        <span>Scope</span>
                        <select
                          value={candidateDraft.scopeType}
                          onChange={(event) =>
                            setCandidateDraft((draft) => ({
                              ...draft,
                              scopeType: event.target.value as MemoryScopeType,
                            }))
                          }
                        >
                          {SUGGESTION_SCOPE_TYPES.map((value) => (
                            <option key={value} value={value}>
                              {value}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="field memories-kind-field">
                        <span>Type</span>
                        <select
                          value={candidateDraft.memoryType}
                          onChange={(event) =>
                            setCandidateDraft((draft) => ({
                              ...draft,
                              memoryType: event.target.value as MemoryType,
                            }))
                          }
                        >
                          {MEMORY_TYPES.map((value) => (
                            <option key={value} value={value}>
                              {value}
                            </option>
                          ))}
                        </select>
                      </label>
                    </div>
                    <label className="field memories-body-field">
                      <span>Content</span>
                      <textarea
                        aria-label="Suggestion content"
                        value={candidateDraft.body}
                        rows={10}
                        onChange={(event) =>
                          setCandidateDraft((draft) => ({
                            ...draft,
                            body: event.target.value,
                          }))
                        }
                      />
                    </label>
                    <div className="memories-edit-actions">
                      <button
                        type="button"
                        className="ghost"
                        disabled={candidateBusy === selectedCandidate.id}
                        onClick={() => setCandidateEditing(false)}
                      >
                        Cancel
                      </button>
                      <button
                        type="button"
                        className="primary"
                        disabled={
                          candidateBusy === selectedCandidate.id ||
                          !candidateDraft.title.trim() ||
                          !candidateDraft.body.trim()
                        }
                        onClick={() => void saveCandidateEdits(selectedCandidate)}
                      >
                        {candidateBusy === selectedCandidate.id
                          ? "Saving…"
                          : "Save changes"}
                      </button>
                    </div>
                  </div>
                ) : (
                  <>
                    <div className="memories-content-surface">
                      <pre>{selectedCandidate.body}</pre>
                    </div>
                    <div className="suggestion-rationale">
                      <strong>Why it was proposed</strong>
                      <p>{selectedCandidate.rationale}</p>
                    </div>
                  </>
                )}

                <div className="muted memories-detail-meta">
                  <p>
                    Proposed scope: {selectedCandidate.scopeType} · Type:{" "}
                    {selectedCandidate.memoryType} ·{" "}
                    {CANDIDATE_OPERATION_LABELS[selectedCandidate.operation]}
                  </p>
                  <button
                    type="button"
                    className="ghost memories-history-link"
                    onClick={() => onOpenRunHistory(selectedCandidate.reviewRunId)}
                  >
                    Open source run in History
                  </button>
                  {selectedCandidate.targetMemoryId &&
                  memories.some(
                    ({ id }) => id === selectedCandidate.targetMemoryId,
                  ) ? (
                    <button
                      type="button"
                      className="ghost memories-history-link"
                      onClick={() => {
                        setMode("memories");
                        setSelectedId(selectedCandidate.targetMemoryId);
                        setDetailOpen(true);
                      }}
                    >
                      Open target memory
                    </button>
                  ) : null}
                </div>

                {candidateIsEditable(selectedCandidate) ? (
                  <div className="suggestion-actions">
                    {candidateEditing ? null : (
                      <button
                        type="button"
                        className="ghost"
                        onClick={() => {
                          setCandidateDraft({
                            title: selectedCandidate.title,
                            body: selectedCandidate.body,
                            scopeType: selectedCandidate.scopeType,
                            memoryType: selectedCandidate.memoryType,
                          });
                          setCandidateEditing(true);
                        }}
                      >
                        <Icon name="pencil-simple" size={15} />
                        Edit
                      </button>
                    )}
                    <button
                      type="button"
                      className="ghost context-action"
                      disabled={candidateBusy === selectedCandidate.id}
                      onClick={() => void decideCandidate(selectedCandidate, "reject")}
                    >
                      Reject
                    </button>
                    <button
                      type="button"
                      className="primary"
                      disabled={candidateBusy === selectedCandidate.id}
                      onClick={() => void decideCandidate(selectedCandidate, "approve")}
                    >
                      {candidateBusy === selectedCandidate.id
                        ? "Working…"
                        : "Approve"}
                    </button>
                  </div>
                ) : null}
              </article>
            ) : (
              <div className="memories-inspector-placeholder">
                <Icon name="note" size={28} />
                <h3>Select a suggestion</h3>
                <p className="muted">
                  Review model-proposed memory changes before anything is saved.
                </p>
              </div>
            )
          ) : (
          creating || selected ? (
            <>
              <button
                type="button"
                className="ghost memories-detail-back"
                onClick={() => setDetailOpen(false)}
              >
                <Icon name="arrow-left" size={15} />
                Memories
              </button>

              <header className="memories-detail-header">
                <div className="memories-detail-heading">
                  {selected?.origin === "linked" ? (
                    <div className="memories-detail-provenance">
                      <span className="memories-linked-source">
                        <Icon name="link" size={13} />
                        From {selected.sourceWorkflowName ?? "another workflow"}
                      </span>
                    </div>
                  ) : null}
                  {!editing && selected ? (
                    <h3>{selected.title}</h3>
                  ) : (
                    <h3>{creating ? "New note" : "Edit memory"}</h3>
                  )}
                  {selected ? (
                    <p className="muted">
                      Updated{" "}
                      {formatWhen(selected.updatedAt || selected.createdAt)}
                      {selected.artifactPath ? ". Artifact stored on disk." : ""}
                    </p>
                  ) : (
                    <p className="muted">
                      Notes remain available as workflow context.
                    </p>
                  )}
                </div>

                <div className="memories-detail-actions">
                  {!editing && htmlPreview ? (
                    <>
                      <button
                        type="button"
                        className="ghost memories-expand-button"
                        aria-pressed={htmlExpanded}
                        aria-label={
                          htmlExpanded
                            ? "Exit expanded HTML preview"
                            : "Expand HTML preview"
                        }
                        title={
                          htmlExpanded ? "Exit expanded preview" : "Expand preview"
                        }
                        onClick={() => setHtmlExpanded((current) => !current)}
                      >
                        <Icon name="corners-out" size={17} />
                      </button>
                      {selected ? (
                        <button
                          type="button"
                          className="ghost"
                          onClick={() => setViewSource((current) => !current)}
                        >
                          {viewSource ? "Show rendered preview" : "View source"}
                        </button>
                      ) : null}
                    </>
                  ) : null}
                  {selected && !creating && !editing && !isLinkedSelected ? (
                    <button
                      type="button"
                      className={
                        selected.pinned ? "primary" : "ghost context-action"
                      }
                      onClick={() => void togglePinMemory(selected.id)}
                    >
                      <Icon name="push-pin" size={15} />
                      {selected.pinned
                        ? "Remove from next run"
                        : "Add to next run"}
                    </button>
                  ) : null}
                  {selected && !editing && !isLinkedSelected ? (
                    <button
                      type="button"
                      className="ghost"
                      onClick={() => {
                        setEditing(true);
                        setDirty(false);
                      }}
                    >
                      <Icon name="pencil-simple" size={15} />
                      Edit
                    </button>
                  ) : null}
                  {selected && !creating && !editing ? (
                    <DropdownMenu
                      open={showDetailMenu}
                      onOpenChange={setShowDetailMenu}
                    >
                      <DropdownMenuTrigger className="ghost memories-more-button">
                        More
                        <Icon name="caret-down" size={12} />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent aria-label="Selected memory actions">
                        {htmlPreview ? (
                          <>
                            <MenuItem
                              onSelect={() => setShowDetailMenu(false)}
                              onClick={() =>
                                setViewSource((current) => !current)
                              }
                            >
                              {viewSource
                                ? "Show rendered preview"
                                : "View source"}
                            </MenuItem>
                            <MenuSeparator />
                          </>
                        ) : null}
                        <MenuItem
                          danger
                          onSelect={() => setShowDetailMenu(false)}
                          onClick={() =>
                            void (isLinkedSelected
                              ? unlinkSelected()
                              : deleteSelected())
                          }
                        >
                          {isLinkedSelected ? "Unlink memory" : "Delete memory"}
                        </MenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  ) : null}
                </div>
              </header>

              {editing ? (
                <div className="memories-edit-form">
                  <label className="field">
                    <span>Title</span>
                    <input
                      type="text"
                      value={title}
                      placeholder="Memory title"
                      autoFocus
                      onChange={(event) => {
                        setTitle(event.target.value);
                        setDirty(true);
                      }}
                    />
                  </label>
                  <div className="memories-detail-toolbar">
                    <label className="field memories-kind-field">
                      <span>Kind</span>
                      <select
                        value={kind}
                        onChange={(event) => {
                          setKind(event.target.value as MemoryKind);
                          setDirty(true);
                        }}
                      >
                        <option value="note">Note</option>
                        <option value="text">Output</option>
                        <option value="artifact">Artifact</option>
                      </select>
                    </label>
                    <label className="field memories-kind-field">
                      <span>Scope</span>
                      <select
                        value={scopeType}
                        onChange={(event) => {
                          setScopeType(event.target.value as MemoryScopeType);
                          setDirty(true);
                        }}
                      >
                        <option value="workflow">Workflow</option>
                        <option value="workspace" disabled={!hasWorkingDirectory}>
                          Workspace
                        </option>
                        <option value="user">User</option>
                      </select>
                    </label>
                    <label className="field memories-kind-field">
                      <span>Type</span>
                      <select
                        value={memoryType}
                        onChange={(event) => {
                          setMemoryType(event.target.value as MemoryType);
                          setDirty(true);
                        }}
                      >
                        {MEMORY_TYPES.map((value) => (
                          <option key={value} value={value}>
                            {value}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="field memories-kind-field">
                      <span>Status</span>
                      <select
                        value={status}
                        onChange={(event) => {
                          setStatus(event.target.value as MemoryStatus);
                          setDirty(true);
                        }}
                      >
                        <option value="active">Active</option>
                        <option value="superseded">Superseded</option>
                        <option value="retracted">Retracted</option>
                      </select>
                    </label>
                  </div>
                  <div className="memories-metadata-grid">
                    <label className="field">
                      <span>Salience · {salience}</span>
                      <input
                        type="range"
                        min="0"
                        max="100"
                        value={salience}
                        onChange={(event) => {
                          setSalience(Number(event.target.value));
                          setDirty(true);
                        }}
                      />
                    </label>
                    <label className="field">
                      <span>Confidence · {Math.round(confidence * 100)}%</span>
                      <input
                        type="range"
                        min="0"
                        max="100"
                        value={Math.round(confidence * 100)}
                        onChange={(event) => {
                          setConfidence(Number(event.target.value) / 100);
                          setDirty(true);
                        }}
                      />
                    </label>
                    <label className="field">
                      <span>Last confirmed (RFC3339)</span>
                      <input
                        type="text"
                        placeholder="2026-08-18T10:00:00Z"
                        value={lastConfirmedAt}
                        onChange={(event) => {
                          setLastConfirmedAt(event.target.value);
                          setDirty(true);
                        }}
                      />
                    </label>
                    <label className="field">
                      <span>Expiry (RFC3339)</span>
                      <input
                        type="text"
                        placeholder="No expiry"
                        value={expiresAt}
                        onChange={(event) => {
                          setExpiresAt(event.target.value);
                          setDirty(true);
                        }}
                      />
                    </label>
                  </div>
                  <label className="field memories-body-field">
                    <span>Content</span>
                    <textarea
                      aria-label="Content"
                      value={body}
                      rows={16}
                      placeholder="Write a durable note"
                      onChange={(event) => {
                        setBody(event.target.value);
                        setDirty(true);
                      }}
                    />
                  </label>
                  <div className="memories-edit-actions">
                    <button
                      type="button"
                      className="ghost"
                      disabled={saving}
                      onClick={() => void cancelEditing()}
                    >
                      Cancel
                    </button>
                    <button
                      type="button"
                      className="primary"
                      disabled={
                        saving || (!dirty && !creating) || !body.trim()
                      }
                      onClick={() => void saveSelected()}
                    >
                      {saving
                        ? "Saving…"
                        : creating
                          ? "Create note"
                          : "Save changes"}
                    </button>
                  </div>
                </div>
              ) : htmlPreview && !viewSource ? (
                <div className="memories-content-surface is-html">
                  <iframe
                    ref={htmlPreviewRef}
                    className="memories-html-preview"
                    title={`${selected?.title || "Memory"} preview`}
                    sandbox="allow-same-origin"
                    referrerPolicy="no-referrer"
                    scrolling="no"
                    srcDoc={htmlPreview}
                    onLoad={(event) => fitHtmlPreview(event.currentTarget)}
                  />
                </div>
              ) : (
                <div className="memories-content-surface">
                  <pre>{body}</pre>
                </div>
              )}

              {selected && !editing ? (
                <div className="muted memories-detail-meta">
                  <p>
                    Scope: {selected.scopeLabel} · Type: {selected.memoryType} ·
                    Source: {selected.source}
                    {selected.sourceWorkflowName
                      ? ` from ${selected.sourceWorkflowName}`
                      : ""}
                    {selected.nodeId ? ` · Node ${selected.nodeId}` : ""}
                    {selected.artifactPath ? " · Artifact on disk" : ""}
                  </p>
                  {selected.runId ? (
                    <button
                      type="button"
                      className="ghost memories-history-link"
                      onClick={() => onOpenRunHistory(selected.runId!)}
                    >
                      Open run {selected.runId} in History
                    </button>
                  ) : null}
                  {supersedes ? <p>Supersedes {supersedes.title}</p> : null}
                  {supersededBy ? (
                    <p>Superseded by {supersededBy.title}</p>
                  ) : null}
                </div>
              ) : null}

              {!editing ? (
                <div className="memory-budget-note" role="status">
                  Pinned context is selected deterministically within a
                  6,000-byte budget (User 1,500 · Workspace 2,000 ·
                  Workflow/linked 2,500).
                  {visiblePinnedBytes > 6_000 ? (
                    <strong>
                      {" "}
                      Visible pins exceed the budget; overflow remains in the
                      library and will be omitted from the run prompt.
                    </strong>
                  ) : null}
                </div>
              ) : null}
            </>
          ) : (
            <div className="memories-inspector-placeholder">
              <Icon name="note" size={28} />
              <h3>Select a memory</h3>
              <p className="muted">
                Choose an item to read it, add it to the next run, or edit its
                content.
              </p>
              <button
                type="button"
                className="primary"
                onClick={() => void startNewNote()}
              >
                New note
              </button>
            </div>
          )
          )}
        </section>
      </div>

      {showLinker ? (
        <Modal
          size="lg"
          className="memories-link-picker-modal"
          onClose={() => setShowLinker(false)}
          labelledBy="link-memory-title"
          describedBy="link-memory-description"
        >
          <ModalHeader
            leading={
              <span className="modal-identity-icon">
                <Icon name="database" size={20} />
              </span>
            }
            title="Link a memory"
            titleId="link-memory-title"
            description="Reuse context from another workflow without copying it."
            descriptionId="link-memory-description"
            actions={
              <button
                type="button"
                className="ghost modal-close-button"
                aria-label="Close link picker"
                onClick={() => setShowLinker(false)}
              >
                <Icon name="x" size={16} />
              </button>
            }
          />
          <div className="memories-link-picker">
            <div className="memories-search-wrap">
              <Icon name="magnifying-glass" size={15} />
              <input
                ref={linkerSearchRef}
                type="search"
                aria-label="Search linkable memories"
                placeholder="Search other workflows"
                value={linkerQuery}
                onChange={(event) => setLinkerQuery(event.target.value)}
              />
            </div>
            <div className="memories-link-picker-results">
              {linkerLoading ? (
                <div className="memories-link-picker-loading" aria-label="Loading">
                  <span />
                  <span />
                  <span />
                </div>
              ) : linkerError ? (
                <p className="memories-link-picker-error" role="alert">
                  {linkerError}
                </p>
              ) : filteredLinkable.length === 0 ? (
                <div className="memories-list-empty">
                  <Icon name="link" size={22} />
                  <p>
                    {linkable.length === 0
                      ? "No memories from other workflows are available."
                      : "No linkable memories match your search."}
                  </p>
                </div>
              ) : (
                <ul>
                  {filteredLinkable.map((memory) => (
                    <li key={memory.id}>
                      <button
                        type="button"
                        className="memories-link-picker-item"
                        disabled={linkingId !== null}
                        onClick={() => void linkSelectedMemory(memory)}
                      >
                        <span>
                          <strong>
                            {highlightText(memory.title, linkerQuery)}
                          </strong>
                          <small>
                            {memory.sourceWorkflowName ?? "Another workflow"}
                          </small>
                        </span>
                        <span className="memories-list-preview">
                          {highlightText(
                            memorySearchSnippet(memory, linkerQuery),
                            linkerQuery,
                          )}
                        </span>
                        <span className="memories-link-picker-action">
                          {linkingId === memory.id ? "Linking…" : "Link"}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        </Modal>
      ) : null}
    </Modal>
  );
}
