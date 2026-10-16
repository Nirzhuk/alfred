import { SearchInput } from "../../../../components/search-input";
import { confirm as confirmDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
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
import { SelectControl } from "../../../../components/select-control";
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
import { createHtmlReportPreview } from "../../html-report";
import { useWorkflowStore } from "../../store";
import {
  canPinMemory,
  isMemoryPromptEligible,
  workspaceScopeAvailable,
} from "../../memories";
import { MemoryLibrarySidebar } from "./memory-library-sidebar";
import { MemoryLibraryList } from "./memory-library-list";
import {
  MemoryEditor,
  MEMORY_STARTERS,
  MEMORY_TYPE_LABELS,
  localDateTime,
  storedDateTime,
  type MemoryDraft,
} from "./memory-editor";
import { MemoryAutomationSettings } from "./memory-automation-settings";
import "./memory-workspace.css";
import type {
  MemoryCandidate,
  MemoryKind,
  MemoryEvidenceItem,
  MemoryLookupAvailability,
  MemoryReviewJob,
  MemoryScopeType,
  MemoryStatus,
  MemoryType,
  OutputMemory,
} from "../../types";
import {
  CANDIDATE_OPERATION_LABELS,
  CANDIDATE_STATUS_LABELS,
  applyCandidateUpdate,
  blockedCandidateCopy,
  candidateActionErrorCopy,
  candidateApprovalRequiresConfirmation,
  candidateCanReject,
  candidateConfidencePercent,
  candidateEditPayload,
  candidateIsEditable,
  candidateMemoryTypeChoices,
  candidateScopeChoices,
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
  memoryFilterCounts,
  memoryUsageStatus,
  type MemoryKindFilter,
  type MemoryQuickFilter,
  type MemoryScopeFilter,
  type MemorySort,
} from "./memory-list-model";

function formatWhen(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function memoryLookupAvailabilityCopy(reason: string | null): string {
  switch (reason) {
    case "workflow_not_found":
      return "This workflow could not be found.";
    case "native_capability_manifest_invalid":
      return "The native agent capability manifest could not be verified.";
    case "native_route_descriptor_mismatch":
      return "The configured native runtime does not match the verified capability manifest.";
    case "claude_package_target_unsupported":
    case "claude_release_manifest_invalid":
    case "claude_release_manifest_mismatch":
    case "claude_release_signature_missing":
    case "claude_publisher_verification_rejected":
    case "claude_sealed_verification_mismatch":
    case "claude_package_store_rejected":
      return "The native provider release could not be verified.";
    case "claude_commercial_terms_unconfirmed":
      return "The native provider release is blocked pending terms verification.";
    case "claude_managed_package_integration_missing":
      return "The native provider release package integration is not verified.";
    case "claude_publisher_verification_integration_missing":
      return "The native provider publisher verification is not integrated.";
    case "claude_packaged_no_cli_smoke_missing":
      return "The packaged native provider runtime has not passed verification.";
    case "claude_native_workflow_renderer_approval_missing":
      return "The native provider workflow renderer is not approved.";
    case "codex_cross_platform_signing_and_packaged_smoke_missing":
    case "codex_app_server_production_unsupported":
    case "codex_runtime_signature_verification_missing":
    case "codex_runtime_license_notice_packaging_missing":
    case "codex_packaged_no_cli_smoke_missing":
    case "codex_python_sdk_public_capability_audit_blocked":
    case "codex_python_sdk_host_approval_unavailable":
    case "codex_python_sdk_known_client_enterprise_clearance_missing":
    case "codex_python_sdk_sealed_package_unverified":
    case "codex_python_sdk_packaged_smoke_missing":
    case "claude_api_key_account_intake_unavailable":
    case "claude_live_api_key_smoke_missing":
    case "cursor_account_repository_consent_and_e2e_gates_missing":
    case "cursor_native_contract_blocked":
    case "opencode_package_account_and_tool_bridge_unverified":
    case "opencode_native_commercial_approval_missing":
    case "opencode_native_package_unverified":
    case "opencode_native_supervisor_http_capability_unavailable":
    case "opencode_native_secret_entry_unavailable":
    case "opencode_native_host_approval_bridge_unavailable":
    case "opencode_native_packaged_live_smoke_missing":
    case "copilot_sdk_package_license_and_packaged_smoke_missing":
    case "gemini_api_key_account_intake_unavailable":
    case "gemini_live_api_key_smoke_missing":
    case "grok_live_api_key_smoke_missing":
    case "native_provider_not_implemented":
      return "This native provider is not release-ready for memory lookup.";
    case "native_provider_unconfigured":
      return "No native agent provider is configured for this workflow.";
    case "native_account_unconfigured":
      return "The native agent account is not configured.";
    case "native_provider_unavailable":
    case "provider_unavailable":
      return "The configured native agent provider is unavailable.";
    case "native_provider_blocked":
      return "The configured native agent provider is blocked by its release policy.";
    case "native_runtime_unavailable":
      return "No supported native agent runtime is available for this workflow.";
    case "native_runtime_context_capacity_unavailable":
    case "context_capacity_unknown":
    case "context_capacity_unavailable":
      return "The runtime's remaining context capacity could not be verified.";
    case "memory_lookup_transport_unavailable":
    case "lookup_transport_unavailable":
    case "native_lookup_transport_unavailable":
      return "The configured runtime cannot receive memory lookup results.";
    case "capability_unsupported":
    case "memory_lookup_unsupported":
      return "The configured runtime does not support memory lookup.";
    case "workflow_has_no_configured_alfred_agent":
      return "This workflow has no configured Alfred agent.";
    case "runtime_not_configured":
    case "no_release_eligible_runtime":
      return "No release-eligible native runtime is configured for this workflow.";
    case "memory_lookup_unavailable":
      return "Memory lookup is unavailable for this workflow right now.";
    default: {
      const dynamicReason = reason?.trim() ?? "";
      if (
        dynamicReason.length > 0 &&
        /\s/.test(dynamicReason) &&
        !dynamicReason.includes("_")
      ) {
        return dynamicReason.length > 160
          ? `${dynamicReason.slice(0, 157)}…`
          : dynamicReason;
      }
      return "Memory lookup is unavailable for this workflow right now.";
    }
  }
}

const MEMORY_LOOKUP_PROBE_ERROR =
  "Could not check memory lookup availability. Try again.";

function evidenceOriginLabel(origin: MemoryEvidenceItem["origin"]): string {
  return origin
    .split("_")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function evidenceSubjectLabel(subject: NonNullable<OutputMemory["evidenceSubject"]>): string {
  switch (subject) {
    case "user_statement":
      return "User statement";
    case "workflow_configuration":
      return "Workflow configuration";
    case "recorded_observation":
      return "Recorded observation";
  }
}

function memoryProvenanceCopy(memory: OutputMemory): string {
  const state =
    memory.provenanceState ??
    (memory.source === "manual" ? "manual" : "legacy_unknown");
  switch (state) {
    case "manual":
      return "Written manually. No run citation is expected.";
    case "evidence_verified":
      return "Approved from a source-backed suggestion.";
    case "source_removed":
      return "The source history was deleted. Copied quotes and digests were removed.";
    default:
      return "No verified curation citation is available for this memory.";
  }
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
  const setMemoryLookupEnabled = useWorkflowStore(
    (s) => s.setMemoryLookupEnabled,
  );

  const [query, setQuery] = useState("");
  const [quickFilter, setQuickFilter] = useState<MemoryQuickFilter>("all");
  const [kindFilter, setKindFilter] = useState<MemoryKindFilter>("all");
  const [scopeFilter, setScopeFilter] = useState<MemoryScopeFilter>("all");
  const [sort, setSort] = useState<MemorySort>("recent");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pinSaving, setPinSaving] = useState(false);
  const [showCreateMenu, setShowCreateMenu] = useState(false);
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
  const [draftSemanticRevision, setDraftSemanticRevision] = useState(1);
  const [observedAt, setObservedAt] = useState("");
  const [lastConfirmedAt, setLastConfirmedAt] = useState("");
  const [validFrom, setValidFrom] = useState("");
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
  const [linkerRetry, setLinkerRetry] = useState(0);
  const [linkable, setLinkable] = useState<OutputMemory[]>([]);
  const [showLinker, setShowLinker] = useState(false);
  const [viewSource, setViewSource] = useState(false);
  const [htmlExpanded, setHtmlExpanded] = useState(false);
  const [recallSaving, setRecallSaving] = useState(false);
  const [memoryLookupSaving, setMemoryLookupSaving] = useState(false);
  const [memoryLookupAvailability, setMemoryLookupAvailability] =
    useState<MemoryLookupAvailability | null>(null);
  const [memoryLookupAvailabilityWorkflowId, setMemoryLookupAvailabilityWorkflowId] =
    useState<string | null>(null);
  const [memoryLookupAvailabilityLoading, setMemoryLookupAvailabilityLoading] =
    useState(false);
  const [memoryLookupAvailabilityError, setMemoryLookupAvailabilityError] =
    useState<string | null>(null);
  const [memoryLookupAvailabilityRetry, setMemoryLookupAvailabilityRetry] =
    useState(0);
  const [mode, setMode] = useState<MemoriesInspectorMode>("memories");
  const [candidates, setCandidates] = useState<MemoryCandidate[]>([]);
  const [reviewJobs, setReviewJobs] = useState<MemoryReviewJob[]>([]);
  const [suggestionsLoading, setSuggestionsLoading] = useState(false);
  const [suggestionsError, setSuggestionsError] = useState(false);
  const [suggestionsRetry, setSuggestionsRetry] = useState(0);
  const [selectedCandidateId, setSelectedCandidateId] = useState<string | null>(
    null,
  );
  const [candidateEditing, setCandidateEditing] = useState(false);
  const [candidateEvidenceConfirmed, setCandidateEvidenceConfirmed] =
    useState(false);
  const [candidateDraft, setCandidateDraft] = useState<CandidateEditDraft>({
    expectedCandidateRevision: 1,
    title: "",
    body: "",
    scopeType: "workflow",
    memoryType: "note",
    validFrom: "",
    expiresAt: "",
  });
  const [candidateBusy, setCandidateBusy] = useState<string | null>(null);
  const [memoryEvidence, setMemoryEvidence] = useState<MemoryEvidenceItem[]>([]);
  const [memoryEvidenceLoading, setMemoryEvidenceLoading] = useState(false);
  const [memoryEvidenceError, setMemoryEvidenceError] = useState(false);
  const [memoryEvidenceRetry, setMemoryEvidenceRetry] = useState(0);
  const [workflowReviewEnabled, setWorkflowReviewEnabled] = useState(false);
  const [workflowReviewSaving, setWorkflowReviewSaving] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const detailRef = useRef<HTMLElement>(null);
  const linkerSearchRef = useRef<HTMLInputElement>(null);
  const htmlPreviewRef = useRef<HTMLIFrameElement>(null);
  const currentView = useRef({ open, workflowId: activeWorkflowId, settingsOpen });
  currentView.current = { open, workflowId: activeWorkflowId, settingsOpen };

  const lookupViewKey = `${open ? "open" : "closed"}:${
    activeWorkflowId ?? ""
  }`;
  const lookupGeneration = useRef(0);
  const previousLookupViewKey = useRef<string | null>(null);
  if (previousLookupViewKey.current !== lookupViewKey) {
    previousLookupViewKey.current = lookupViewKey;
    lookupGeneration.current += 1;
  }
  const lookupSaveViewKey = `${lookupViewKey}:${settingsOpen ? "settings" : "list"}`;
  const lookupSaveGeneration = useRef(0);
  const previousLookupSaveViewKey = useRef<string | null>(null);
  if (previousLookupSaveViewKey.current !== lookupSaveViewKey) {
    previousLookupSaveViewKey.current = lookupSaveViewKey;
    lookupSaveGeneration.current += 1;
  }
  const htmlPreview = useMemo(() => createHtmlReportPreview(body), [body]);
  const activeWorkflow = workflows.find(({ id }) => id === activeWorkflowId);
  const automaticRecall = activeWorkflow?.memoryRetrievalEnabled ?? false;
  const memoryLookupEnabled = activeWorkflow?.memoryLookupEnabled ?? false;
  const currentMemoryLookupAvailability =
    memoryLookupAvailabilityWorkflowId === activeWorkflowId
      ? memoryLookupAvailability
      : null;
  const currentMemoryLookupAvailabilityError =
    memoryLookupAvailabilityWorkflowId === activeWorkflowId
      ? memoryLookupAvailabilityError
      : null;
  const currentMemoryLookupAvailabilityLoading =
    Boolean(open && activeWorkflowId) &&
    (memoryLookupAvailabilityWorkflowId !== activeWorkflowId
      ? true
      : memoryLookupAvailabilityLoading);
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
  const failedJobs = useMemo(() => failedReviewJobs(reviewJobs), [reviewJobs]);
  const selectedCandidate =
    candidates.find(({ id }) => id === selectedCandidateId) ?? null;
  const selectedCandidateTarget = selectedCandidate?.targetMemoryId
    ? memories.find(({ id }) => id === selectedCandidate.targetMemoryId) ?? null
    : null;

  useEffect(() => {
    if (!htmlPreview || viewSource || editing) return;
    const resize = () => fitHtmlPreview(htmlPreviewRef.current);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [htmlPreview, viewSource, editing]);

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
      filterAndSortMemories(memories, query, quickFilter, kindFilter, sort, {
        scopeFilter,
        typeFilter,
      }),
    [memories, query, quickFilter, kindFilter, sort, scopeFilter, typeFilter],
  );

  const libraryCounts = useMemo(
    () => memoryFilterCounts(memories, query, kindFilter, { typeFilter }),
    [memories, query, kindFilter, typeFilter],
  );

  const selected =
    filtered.find((m) => m.id === selectedId) ??
    memories.find((m) => m.id === selectedId) ??
    null;
  const isLinkedSelected = selected?.origin === "linked";
  const selectedUsage = selected ? memoryUsageStatus(selected) : null;
  const libraryTitle =
    scopeFilter !== "all"
      ? { workflow: "This workflow", workspace: "Workspace", user: "Personal" }[
          scopeFilter
        ]
      : {
          all: "All memories",
          pinned: "Pinned",
          linked: "Linked",
          inactive: "Inactive",
        }[quickFilter];

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
    setSort("recent");
    setSettingsOpen(false);
    setActionError(null);
    setDirty(false);
    setEditing(false);
    setCreating(false);
    setShowLinker(false);
    setViewSource(false);
    setHtmlExpanded(false);
    setDetailOpen(Boolean(initialMemoryId));
    setMode(initialMode);
    setSelectedCandidateId(null);
    setCandidates([]);
    setReviewJobs([]);
    setCandidateEditing(false);
    setCandidateEvidenceConfirmed(false);
    setSuggestionsError(false);
  }, [open, initialMemoryId, activeWorkflowId, initialMode]);

  useEffect(() => {
    if (!settingsOpen) {
      setMemoryLookupSaving(false);
    }
  }, [settingsOpen]);

  useEffect(() => {
    if (!open) return;
    void loadReviewSettings();
  }, [open, loadReviewSettings]);

  useEffect(() => {
    const generation = lookupGeneration.current;
    setMemoryLookupSaving(false);
    setMemoryLookupAvailability(null);
    setMemoryLookupAvailabilityWorkflowId(null);
    setMemoryLookupAvailabilityError(null);
    if (!open || !activeWorkflowId) {
      setMemoryLookupAvailabilityLoading(false);
      return;
    }

    let cancelled = false;
    setMemoryLookupAvailabilityLoading(true);
    void api
      .getMemoryLookupAvailability(activeWorkflowId)
      .then((result) => {
        if (
          cancelled ||
          lookupGeneration.current !== generation ||
          !currentView.current.open ||
          currentView.current.workflowId !== activeWorkflowId
        ) {
          return;
        }
        const reason = result?.reason;
        if (
          typeof result?.available !== "boolean" ||
          (reason !== undefined &&
            reason !== null &&
            typeof reason !== "string")
        ) {
          throw new Error("invalid_memory_lookup_availability");
        }
        setMemoryLookupAvailability({
          available: result.available,
          reason: reason ?? null,
        });
        setMemoryLookupAvailabilityWorkflowId(activeWorkflowId);
      })
      .catch(() => {
        if (
          cancelled ||
          lookupGeneration.current !== generation ||
          !currentView.current.open ||
          currentView.current.workflowId !== activeWorkflowId
        ) {
          return;
        }
        setMemoryLookupAvailability({ available: false, reason: null });
        setMemoryLookupAvailabilityWorkflowId(activeWorkflowId);
        setMemoryLookupAvailabilityError(MEMORY_LOOKUP_PROBE_ERROR);
      })
      .finally(() => {
        if (
          !cancelled &&
          lookupGeneration.current === generation &&
          currentView.current.open &&
          currentView.current.workflowId === activeWorkflowId
        ) {
          setMemoryLookupAvailabilityLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [open, activeWorkflowId, memoryLookupAvailabilityRetry]);

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
  ): Promise<{
    candidates: MemoryCandidate[];
    jobs: MemoryReviewJob[];
  } | null> => {
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
    const activeElement = document.activeElement as HTMLElement | null;
    const focusKey = activeElement
      ?.closest("[data-suggestion-id]")
      ?.getAttribute("data-suggestion-id");
    const result = await loadSuggestions(workflowId);
    if (
      !currentView.current.open ||
      currentView.current.workflowId !== workflowId
    )
      return;
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
    if (!open || !activeWorkflowId) return;
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
  }, [open, activeWorkflowId, focusRunId, suggestionsRetry]);

  // Backend-driven refresh: only the affected active workflow reloads.
  useEffect(() => {
    if (!open || !activeWorkflowId) return;
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    void listen<{ workflowId: string; pendingCount: number }>(
      "memory://candidates-changed",
      (event) => {
        if (!shouldRefreshSuggestions(event.payload, activeWorkflowId, open))
          return;
        void refreshPreservingFocus(activeWorkflowId).then(() => {
          if (cancelled) return;
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
  }, [open, activeWorkflowId]);

  const selectCandidate = async (candidate: MemoryCandidate) => {
    if (!(await confirmDiscard())) return;
    setSelectedCandidateId(candidate.id);
    setSettingsOpen(false);
    setDetailOpen(true);
    setActionError(null);
    setCandidateEditing(false);
    setCandidateEvidenceConfirmed(false);
    setCandidateDraft({
      expectedCandidateRevision: candidate.candidateRevision,
      title: candidate.title,
      body: candidate.body,
      scopeType: candidate.scopeType,
      memoryType: candidate.memoryType,
      validFrom: candidate.validFrom ?? "",
      expiresAt: candidate.expiresAt ?? "",
    });
  };

  const decideCandidate = async (
    candidate: MemoryCandidate,
    decision: "approve" | "reject",
  ) => {
    if (candidateBusy || candidateEditing) return;
    setActionError(null);
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
          ? await api.approveMemoryCandidate(
              candidate.id,
              candidate.candidateRevision,
            )
          : await api.rejectMemoryCandidate(candidate.id);
      setCandidates((list) => applyCandidateUpdate(list, updated));
      setAnnouncement(
        updated.status === "blocked"
          ? `Suggestion blocked. ${blockedCandidateCopy(updated.blockedCode)}`
          : decision === "approve"
            ? "Suggestion approved."
            : "Suggestion rejected.",
      );
      if (
        activeWorkflowId &&
        (updated.status === "approved" ||
          (updated.status === "blocked" &&
            updated.blockedCode?.startsWith("target_")))
      ) {
        await useWorkflowStore.getState().loadMemories(activeWorkflowId);
      }
    } catch (error) {
      const copy = candidateActionErrorCopy(error, decision);
      setActionError(copy);
      setAnnouncement(copy);
      if (
        [
          "candidate_changed",
          "candidate_not_found",
          "candidate_not_pending",
          "candidate_evidence_unavailable",
          "evidence_validation_failed",
        ].some((code) => String(error).includes(code)) &&
        activeWorkflowId
      ) {
        await refreshPreservingFocus(activeWorkflowId);
      }
    } finally {
      setCandidateBusy(null);
    }
  };

  const saveCandidateEdits = async (candidate: MemoryCandidate) => {
    if (candidateBusy) return;
    setActionError(null);
    if (!hasCandidateEdits(candidate, candidateDraft)) {
      setCandidateEditing(false);
      return;
    }
    setCandidateBusy(candidate.id);
    try {
      const updated = await api.updateMemoryCandidate({
        ...candidateEditPayload(candidate, candidateDraft),
        evidenceConfirmed: candidateEvidenceConfirmed,
      });
      setCandidates((list) => applyCandidateUpdate(list, updated));
      setCandidateEditing(false);
      setCandidateEvidenceConfirmed(false);
      setAnnouncement("Suggestion updated.");
    } catch (error) {
      const copy = candidateActionErrorCopy(error, "edit");
      setActionError(copy);
      setAnnouncement(copy);
      if (
        [
          "candidate_changed",
          "candidate_not_found",
          "candidate_not_pending",
          "candidate_evidence_unavailable",
          "evidence_validation_failed",
        ].some((code) => String(error).includes(code)) &&
        activeWorkflowId
      ) {
        await refreshPreservingFocus(activeWorkflowId);
      }
    } finally {
      setCandidateBusy(null);
    }
  };

  const retryFailedReview = async (job: MemoryReviewJob) => {
    if (candidateBusy) return;
    setActionError(null);
    setCandidateBusy(job.runId);
    try {
      const retried = await api.retryMemoryReview(job.runId);
      setReviewJobs((jobs) =>
        jobs.map((item) => (item.runId === job.runId ? retried : item)),
      );
      setAnnouncement("Review queued again.");
    } catch {
      setActionError(
        reviewGateReason ??
          "The review could not be retried. Check memory settings, then try again.",
      );
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
    useWorkflowStore.getState().keepWorkflow(activeWorkflowId);
    setWorkflowReviewSaving(true);
    void api
      .setWorkflowMemoryReview(activeWorkflowId, !workflowReviewEnabled)
      .then((row) => setWorkflowReviewEnabled(row.enabled))
      .catch(() =>
        setActionError("Could not update memory suggestions. Try again."),
      )
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
  }, [open, activeWorkflowId, showLinker, memories.length, linkerRetry]);

  useEffect(() => {
    if (!open) return;
    if (!editing && selectedId && !memories.some((m) => m.id === selectedId)) {
      setSelectedId(memories[0]?.id ?? null);
    }
  }, [open, memories, selectedId, editing]);

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
    setDraftSemanticRevision(selected.semanticRevision);
    setObservedAt(selected.observedAt ?? "");
    setLastConfirmedAt(selected.lastConfirmedAt ?? "");
    setValidFrom(selected.validFrom ?? "");
    setExpiresAt(selected.expiresAt ?? "");
    setDirty(false);
    setViewSource(false);
    setHtmlExpanded(false);
  }, [
    selected?.id,
    selected?.updatedAt,
    selected?.semanticRevision,
    creating,
    editing,
  ]);

  useEffect(() => {
    let cancelled = false;
    if (!open || !selected?.id) {
      setMemoryEvidence([]);
      setMemoryEvidenceLoading(false);
      setMemoryEvidenceError(false);
      return;
    }
    setMemoryEvidenceLoading(true);
    setMemoryEvidenceError(false);
    api.listMemoryEvidence(selected.id)
      .then((items) => {
        if (!cancelled) setMemoryEvidence(items);
      })
      .catch(() => {
        if (!cancelled) {
          setMemoryEvidence([]);
          setMemoryEvidenceError(true);
        }
      })
      .finally(() => {
        if (!cancelled) setMemoryEvidenceLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, selected?.id, memoryEvidenceRetry]);

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
    if (saving || candidateBusy) return false;
    const candidateDirty =
      candidateEditing &&
      selectedCandidate &&
      hasCandidateEdits(selectedCandidate, candidateDraft);
    if ((!dirty || (!editing && !creating)) && !candidateDirty) return true;
    return confirmDialog("Discard your unsaved changes?", {
      title: "Unsaved changes",
      kind: "warning",
    });
  };

  const requestClose = async () => {
    if (!(await confirmDiscard())) return;
    onClose();
  };

  const cancelCandidateEditing = async () => {
    if (!(await confirmDiscard())) return;
    setCandidateEditing(false);
    setCandidateEvidenceConfirmed(false);
    setActionError(null);
  };

  const openRunHistory = async (runId: string) => {
    if (!(await confirmDiscard())) return;
    onOpenRunHistory(runId);
  };

  const openTargetMemory = async (id: string) => {
    const memory = memories.find((item) => item.id === id);
    if (memory) await loadMemory(memory);
  };

  const resetFilters = () => {
    setQuery("");
    setQuickFilter("all");
    setScopeFilter("all");
    setKindFilter("all");
    setTypeFilter("all");
  };

  const changeMode = async (next: MemoriesInspectorMode) => {
    if (!(await confirmDiscard())) return false;
    setEditing(false);
    setCreating(false);
    setCandidateEditing(false);
    setCandidateEvidenceConfirmed(false);
    setDirty(false);
    setDetailOpen(false);
    setMode(next);
    setSettingsOpen(false);
    setActionError(null);
    return true;
  };

  const openSettings = async () => {
    if (!(await confirmDiscard())) return;
    setEditing(false);
    setCreating(false);
    setCandidateEditing(false);
    setDirty(false);
    setSettingsOpen((current) => !current);
    setDetailOpen(true);
    setHtmlExpanded(false);
    setActionError(null);
  };

  const toggleRecall = () => {
    if (!activeWorkflowId || recallSaving) return;
    const requested = !automaticRecall;
    setRecallSaving(true);
    setActionError(null);
    void setMemoryRetrievalEnabled(activeWorkflowId, requested)
      .then(() => {
        const workflow = useWorkflowStore
          .getState()
          .workflows.find((item) => item.id === activeWorkflowId);
        if (workflow?.memoryRetrievalEnabled !== requested)
          setActionError("Could not update automatic recall. Try again.");
      })
      .catch(() =>
        setActionError("Could not update automatic recall. Try again."),
      )
      .finally(() => setRecallSaving(false));
  };

  const retryMemoryLookupAvailability = () => {
    if (
      !open ||
      !activeWorkflowId ||
      currentMemoryLookupAvailabilityLoading
    ) {
      return;
    }
    setMemoryLookupAvailabilityRetry((retry) => retry + 1);
  };

  const toggleMemoryLookup = () => {
    const workflowId = activeWorkflowId;
    if (!workflowId || memoryLookupSaving) return;
    const requested = !memoryLookupEnabled;
    if (
      requested &&
      (currentMemoryLookupAvailabilityLoading ||
        currentMemoryLookupAvailability?.available !== true ||
        Boolean(currentMemoryLookupAvailabilityError)
    )) {
      return;
    }

    const generation = lookupGeneration.current;
    const saveGeneration = lookupSaveGeneration.current;
    const isCurrentView = () =>
      lookupGeneration.current === generation &&
      lookupSaveGeneration.current === saveGeneration &&
      currentView.current.open &&
      currentView.current.workflowId === workflowId &&
      currentView.current.settingsOpen;
    setMemoryLookupSaving(true);
    setActionError(null);
    void setMemoryLookupEnabled(workflowId, requested)
      .then((saved) => {
        if (!isCurrentView()) return;
        const workflow = useWorkflowStore
          .getState()
          .workflows.find((item) => item.id === workflowId);
        if (!saved || workflow?.memoryLookupEnabled !== requested) {
          setActionError("Could not update memory lookup permission. Try again.");
        }
      })
      .catch(() => {
        if (isCurrentView()) {
          setActionError("Could not update memory lookup permission. Try again.");
        }
      })
      .finally(() => {
        if (isCurrentView()) setMemoryLookupSaving(false);
      });
  };

  const updateDraft = (patch: Partial<MemoryDraft>) => {
    if (patch.title !== undefined) setTitle(patch.title);
    if (patch.body !== undefined) setBody(patch.body);
    if (patch.kind !== undefined) setKind(patch.kind);
    if (patch.scopeType !== undefined) setScopeType(patch.scopeType);
    if (patch.memoryType !== undefined) setMemoryType(patch.memoryType);
    if (patch.status !== undefined) setStatus(patch.status);
    if (patch.salience !== undefined) setSalience(patch.salience);
    if (patch.confidence !== undefined) setConfidence(patch.confidence);
    if (patch.observedAt !== undefined) setObservedAt(patch.observedAt);
    if (patch.lastConfirmedAt !== undefined)
      setLastConfirmedAt(patch.lastConfirmedAt);
    if (patch.validFrom !== undefined) setValidFrom(patch.validFrom);
    if (patch.expiresAt !== undefined) setExpiresAt(patch.expiresAt);
    setDirty(true);
    setActionError(null);
  };

  const toggleSelectedPin = async () => {
    if (!selected || pinSaving) return;
    setPinSaving(true);
    setActionError(null);
    try {
      await togglePinMemory(selected.id);
      const updated = useWorkflowStore
        .getState()
        .memories.find((memory) => memory.id === selected.id);
      if (updated?.pinned === selected.pinned)
        setActionError("Could not change this pin. Try again.");
      else
        setAnnouncement(
          updated?.pinned ? "Memory pinned." : "Memory unpinned.",
        );
    } finally {
      setPinSaving(false);
    }
  };

  const loadMemory = async (memory: OutputMemory) => {
    if (!(await confirmDiscard())) return false;
    setMode("memories");
    setCandidateEditing(false);
    setEditing(false);
    setCreating(false);
    setSelectedId(memory.id);
    setSettingsOpen(false);
    setDirty(false);
    setActionError(null);
    setViewSource(false);
    setHtmlExpanded(false);
    setDetailOpen(true);
    return true;
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
    setObservedAt("");
    setLastConfirmedAt("");
    setValidFrom("");
    setExpiresAt("");
  };

  const startNewNote = async (
    type: MemoryType = "note",
    copy?: OutputMemory,
  ) => {
    if (!(await confirmDiscard())) return;
    setMode("memories");
    setSettingsOpen(false);
    setActionError(null);
    setCandidateEditing(false);
    setEditing(true);
    setCreating(true);
    setSelectedId(null);
    resetDraftFields();
    setMemoryType(type);
    if (copy) {
      setTitle(`${copy.title} (copy)`);
      setBody(copy.body);
    }
    setDirty(Boolean(copy));
    setViewSource(true);
    setHtmlExpanded(false);
    setDetailOpen(true);
  };

  const saveSelected = async () => {
    if (saving) return;
    if (!creating && !selected) {
      setActionError(
        "This memory is no longer in the library. Your draft is still here; copy its contents before closing.",
      );
      return;
    }
    if (!body.trim() && (creating || !selected?.artifactPath)) return;
    setActionError(null);
    setSaving(true);
    try {
      if (creating) {
        const created = await addMemory({
          title: title.trim() || MEMORY_TYPE_LABELS[memoryType],
          body,
          kind,
          scopeType,
          memoryType,
          source: "manual",
          salience,
          confidence,
          status,
          observedAt: observedAt || null,
          lastConfirmedAt: lastConfirmedAt || null,
          validFrom: validFrom || null,
          expiresAt: expiresAt || null,
        });
        if (!created) {
          setActionError(
            "Could not create this memory. Your draft is still here; try saving again.",
          );
          return;
        }
        setCreating(false);
        setSelectedId(created.id);
      } else if (selected) {
        if (
          selected.scopeType !== scopeType &&
          !(await confirmDialog(
            `Move this memory to ${scopeType === "user" ? "all your workflows" : scopeType === "workspace" ? "this workspace" : "this workflow"}? Its availability to other workflows will change.`,
            { title: "Change memory scope", kind: "warning" },
          ))
        ) {
          return;
        }
        const updated = await updateMemoryFields({
          id: selected.id,
          expectedSemanticRevision: draftSemanticRevision,
          title: title.trim() || selected.title,
          ...(!selected.artifactPath && body !== selected.body ? { body } : {}),
          kind,
          ...(scopeType !== selected.scopeType ? { scopeType } : {}),
          memoryType,
          salience,
          confidence,
          status,
          ...(status !== "active" && selected.pinned ? { pinned: false } : {}),
          observedAt: observedAt || null,
          lastConfirmedAt: lastConfirmedAt || null,
          validFrom: validFrom || null,
          expiresAt: expiresAt || null,
        });
        if (!updated) {
          setActionError(
            "Could not save these changes. Your draft is still here; try saving again.",
          );
          return;
        }
      }
      setEditing(false);
      setDirty(false);
      setViewSource(false);
      setHtmlExpanded(false);
      setAnnouncement(creating ? "Memory created." : "Memory saved.");
    } catch {
      setActionError(
        "Could not save this memory. Your draft is still here; try again.",
      );
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
    if (!selected || saving) return;
    const confirmed = await confirmDialog(
      `Delete “${selected.title}”? This cannot be undone.`,
      { title: "Delete memory", kind: "warning" },
    );
    if (!confirmed) return;
    const id = selected.id;
    setActionError(null);
    setSaving(true);
    try {
      await removeMemory(id);
      if (
        useWorkflowStore.getState().memories.some((memory) => memory.id === id)
      ) {
        setActionError("Could not delete this memory. Try again.");
      } else {
        setAnnouncement("Memory deleted.");
      }
    } finally {
      setSaving(false);
    }
  };

  const unlinkSelected = async () => {
    if (!selected || saving) return;
    const confirmed = await confirmDialog(
      `Unlink “${selected.title}” from this workflow? The original memory will remain available.`,
      { title: "Unlink memory", kind: "warning" },
    );
    if (!confirmed) return;
    setActionError(null);
    setSaving(true);
    try {
      await unlinkMemory(selected.id);
      if (
        useWorkflowStore
          .getState()
          .memories.some((memory) => memory.id === selected.id)
      ) {
        setActionError("Could not unlink this memory. Try again.");
      } else {
        setAnnouncement("Memory unlinked. The original is unchanged.");
      }
    } finally {
      setSaving(false);
    }
  };

  const clearOwned = async () => {
    if (!(await confirmDiscard())) return;
    const ownedCount = memories.filter(
      (memory) =>
        memory.origin !== "linked" &&
        memory.scopeType === "workflow" &&
        memory.workflowId === activeWorkflowId,
    ).length;
    if (!ownedCount) {
      setActionError(
        "This workflow has no memories to clear. Linked, workspace, and personal memories are managed at their source.",
      );
      return;
    }
    const confirmed = await confirmDialog(
      `Delete ${ownedCount} ${
        ownedCount === 1 ? "memory" : "memories"
      } from this workflow? Linked, workspace, and personal memories will remain.`,
      { title: "Clear workflow memories", kind: "warning" },
    );
    if (!confirmed) return;
    setEditing(false);
    setCreating(false);
    setCandidateEditing(false);
    setDirty(false);
    setSelectedId(null);
    setActionError(null);
    setSaving(true);
    try {
      await clearMemories();
      const remaining = useWorkflowStore.getState().memories;
      if (
        remaining.some(
          (memory) =>
            memory.origin !== "linked" &&
            memory.scopeType === "workflow" &&
            memory.workflowId === activeWorkflowId,
        )
      ) {
        setActionError("Could not clear workflow memories. Try again.");
      } else {
        setAnnouncement(
          "Workflow memories cleared. Shared and linked memories remain.",
        );
      }
    } finally {
      setSaving(false);
    }
  };

  const openLinker = () => {
    setLinkerQuery("");
    setShowLinker(true);
  };

  const linkSelectedMemory = async (memory: OutputMemory) => {
    if (linkingId || !(await confirmDiscard())) return;
    setLinkerError(null);
    setLinkingId(memory.id);
    try {
      const linked = await linkMemory(memory.id);
      if (linked) {
        setSelectedId(linked.id);
        setShowLinker(false);
        setCreating(false);
        setEditing(false);
        setDetailOpen(true);
        setSettingsOpen(false);
        setMode("memories");
        setDirty(false);
        setActionError(null);
      } else {
        setLinkerError("Could not link this memory. Try again.");
      }
    } finally {
      setLinkingId(null);
    }
  };

  const handleListKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    if (target.closest("input, textarea, select, [contenteditable='true']"))
      return;
    if (mode !== "memories") return;
    if (!filtered.length) return;
    const focusedId =
      target.closest<HTMLElement>("[data-memory-id]")?.dataset.memoryId;
    const currentIndex = filtered.findIndex(
      (memory) => memory.id === (focusedId ?? selectedId),
    );
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex =
        currentIndex < 0
          ? 0
          : (currentIndex + direction + filtered.length) % filtered.length;
      void loadMemory(filtered[nextIndex]);
    } else if (event.key === "Enter" && focusedId) {
      const memory = filtered.find((item) => item.id === focusedId);
      if (!memory) return;
      event.preventDefault();
      void loadMemory(memory).then((loaded) => {
        if (loaded) requestAnimationFrame(() => detailRef.current?.focus());
      });
    }
  };

  const visiblePinnedBytes = memories
    .filter((memory) => memory.pinned && isMemoryPromptEligible(memory))
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
      className={`memories-inspector-modal memory-workspace${
        htmlExpanded ? " is-html-expanded" : ""
      }`}
      onClose={() => void requestClose()}
      labelledBy="memories-inspector-title"
      describedBy="memories-inspector-description"
      closeOnEscape={
        !showLinker && !showHeaderMenu && !showDetailMenu && !showCreateMenu
      }
    >
      <ModalHeader
        titleAs="h2"
        leading={
          <span className="memory-workspace-mark">
            <Icon name="note" size={22} />
          </span>
        }
        title="Memories"
        titleId="memories-inspector-title"
        description="Knowledge your agents can use again."
        descriptionId="memories-inspector-description"
        actions={
          <>
            <button
              type="button"
              className="ghost memory-link-button"
              disabled={!activeWorkflowId}
              onClick={openLinker}
            >
              <Icon name="link" size={16} /> Link memory
            </button>
            <DropdownMenu
              open={showCreateMenu}
              onOpenChange={setShowCreateMenu}
            >
              <DropdownMenuTrigger
                className="primary memory-create-button"
                disabled={!activeWorkflowId}
              >
                <Icon name="plus" size={16} /> New memory{" "}
                <Icon name="caret-down" size={14} />
              </DropdownMenuTrigger>
              <DropdownMenuContent
                aria-label="Create a memory"
                className="memory-starters-menu"
              >
                {MEMORY_STARTERS.map((starter) => (
                  <MenuItem
                    key={starter.type}
                    onSelect={() => {
                      setShowCreateMenu(false);
                      void startNewNote(starter.type);
                    }}
                  >
                    <span>
                      <strong>{starter.label}</strong>
                      <small>{starter.description}</small>
                    </span>
                  </MenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu
              open={showHeaderMenu}
              onOpenChange={setShowHeaderMenu}
            >
              <DropdownMenuTrigger
                className="ghost memory-header-more"
                aria-label="Memory library actions"
              >
                <Icon name="sliders" size={18} />
              </DropdownMenuTrigger>
              <DropdownMenuContent aria-label="Memory library actions">
                <MenuItem
                  onSelect={() => {
                    setShowHeaderMenu(false);
                    void openSettings();
                  }}
                >
                  Memory settings
                </MenuItem>
                <MenuSeparator />
                <MenuItem
                  danger
                  disabled={
                    !memories.some(
                      (memory) =>
                        memory.origin !== "linked" &&
                        memory.scopeType === "workflow" &&
                        memory.workflowId === activeWorkflowId,
                    )
                  }
                  onSelect={() => {
                    setShowHeaderMenu(false);
                    void clearOwned();
                  }}
                >
                  Clear workflow memories
                </MenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <button
              type="button"
              className="ghost modal-close-button"
              aria-label="Close memories"
              onClick={() => void requestClose()}
            >
              <Icon name="x" size={18} />
            </button>
          </>
        }
      />
      <p className="sr-only" role="status" aria-live="polite">
        {announcement}
      </p>
      {actionError ? (
        <div className="memory-workspace-error" role="alert">
          <span>{actionError}</span>
          <button
            type="button"
            className="ghost"
            aria-label="Dismiss memory error"
            onClick={() => setActionError(null)}
          >
            <Icon name="x" size={16} />
          </button>
        </div>
      ) : null}

      <div
        className={`memories-inspector-body${
          detailOpen ? " is-detail-open" : ""
        }`}
      >
        <MemoryLibrarySidebar
          workflowName={activeWorkflow?.name ?? "No workflow selected"}
          mode={mode}
          onModeChange={changeMode}
          quickFilter={quickFilter}
          onQuickFilterChange={setQuickFilter}
          scopeFilter={scopeFilter}
          onScopeFilterChange={setScopeFilter}
          counts={libraryCounts}
          pendingCount={pendingCount}
          automaticRecall={automaticRecall}
          automaticSuggestions={workflowReviewEnabled}
          suggestionsUnavailableReason={reviewGateReason}
          settingsOpen={settingsOpen}
          onOpenSettings={() => void openSettings()}
        />
        {mode === "memories" ? (
          <MemoryLibraryList
            memories={filtered}
            totalCount={memories.length}
            selectedId={creating ? null : selectedId}
            onSelect={(memory) => void loadMemory(memory)}
            query={query}
            onQueryChange={setQuery}
            kindFilter={kindFilter}
            onKindFilterChange={setKindFilter}
            typeFilter={typeFilter}
            onTypeFilterChange={setTypeFilter}
            sort={sort}
            onSortChange={setSort}
            onResetFilters={resetFilters}
            onNewNote={() => void startNewNote()}
            title={libraryTitle}
            hasActiveFilters={Boolean(
              query ||
              quickFilter !== "all" ||
              scopeFilter !== "all" ||
              kindFilter !== "all" ||
              typeFilter !== "all",
            )}
            searchRef={searchRef}
            onKeyDown={handleListKeyDown}
          />
        ) : (
          <aside
            className="memories-inspector-list memory-suggestions-list"
            aria-label="Memory suggestions"
          >
            <div className="memory-suggestions-heading">
              <h3>Suggestions</h3>
              <p>Review what your agents learned before adding it to memory.</p>
            </div>
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
                  <button
                    type="button"
                    className="ghost"
                    onClick={() =>
                      setSuggestionsRetry((current) => current + 1)
                    }
                  >
                    Retry
                  </button>
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
                          candidate.id === selectedCandidateId
                            ? " is-active"
                            : ""
                        }`}
                        onClick={() => selectCandidate(candidate)}
                      >
                        <span className="memories-list-top">
                          <span className="memories-list-title-text">
                            {candidate.title}
                          </span>
                          <span
                            className={`suggestion-status is-${candidate.status}`}
                          >
                            {CANDIDATE_STATUS_LABELS[candidate.status]}
                          </span>
                        </span>
                        <span className="suggestion-item-meta">
                          {CANDIDATE_OPERATION_LABELS[candidate.operation]} ·{" "}
                          {candidate.scopeType} · {candidate.memoryType} ·{" "}
                          Model assessment: {candidateConfidencePercent(candidate.confidence)}%
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </aside>
        )}

        <section
          ref={detailRef}
          className="memories-inspector-detail"
          tabIndex={-1}
          aria-label="Memory details"
        >
          {settingsOpen || mode === "suggestions" ? (
            <button
              type="button"
              className="ghost memories-detail-back"
              onClick={() => {
                if (settingsOpen) setSettingsOpen(false);
                setDetailOpen(false);
              }}
            >
              <Icon name="arrow-left" size={16} /> Back to list
            </button>
          ) : null}
          {settingsOpen ? (
            <MemoryAutomationSettings
              automaticRecall={automaticRecall}
              recallSaving={recallSaving}
              onToggleRecall={toggleRecall}
              memoryLookupEnabled={memoryLookupEnabled}
              memoryLookupAvailable={
                currentMemoryLookupAvailability?.available === true
              }
              memoryLookupAvailabilityLoading={
                currentMemoryLookupAvailabilityLoading
              }
              memoryLookupAvailabilityError={
                currentMemoryLookupAvailabilityError
              }
              onRetryMemoryLookupAvailability={retryMemoryLookupAvailability}
              memoryLookupSaving={memoryLookupSaving}
              memoryLookupAvailabilityReason={
                currentMemoryLookupAvailability?.available === false
                  ? memoryLookupAvailabilityCopy(
                      currentMemoryLookupAvailability.reason,
                    )
                  : null
              }
              onToggleMemoryLookup={toggleMemoryLookup}
              automaticSuggestions={workflowReviewEnabled}
              reviewSaving={workflowReviewSaving}
              reviewGateReason={reviewGateReason}
              onToggleReview={toggleWorkflowReview}
              disabled={!activeWorkflowId}
            />
          ) : mode === "suggestions" ? (
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
                        {
                          CANDIDATE_OPERATION_LABELS[
                            selectedCandidate.operation
                          ]
                        }
                      </span>
                      <span
                        className={`suggestion-status is-${selectedCandidate.status}`}
                      >
                        {CANDIDATE_STATUS_LABELS[selectedCandidate.status]}
                      </span>
                      <span className="muted">
                        {candidateConfidencePercent(
                          selectedCandidate.confidence,
                        )}
                        % model assessment · {formatWhen(selectedCandidate.createdAt)}
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
                  <fieldset
                    className="memories-edit-form memory-suggestion-editor"
                    disabled={Boolean(candidateBusy)}
                  >
                    <label className="field">
                      <span>Title</span>
                      <input
                        type="text"
                        value={candidateDraft.title}
                        onChange={(event) => {
                          setCandidateEvidenceConfirmed(false);
                          setCandidateDraft((draft) => ({
                            ...draft,
                            title: event.target.value,
                          }));
                        }}
                      />
                    </label>
                    <div className="memories-detail-toolbar">
                      <label className="field memories-kind-field">
                        <span>Scope</span>
                        <SelectControl
                          value={candidateDraft.scopeType}
                          onChange={(event) => {
                            setCandidateEvidenceConfirmed(false);
                            setCandidateDraft((draft) => ({
                              ...draft,
                              scopeType: event.target.value as MemoryScopeType,
                            }));
                          }}
                        >
                          {candidateScopeChoices(selectedCandidate).map((value) => (
                            <option key={value} value={value}>
                              {value}
                            </option>
                          ))}
                        </SelectControl>
                      </label>
                      <label className="field memories-kind-field">
                        <span>Type</span>
                        <SelectControl
                          value={candidateDraft.memoryType}
                          onChange={(event) => {
                            setCandidateEvidenceConfirmed(false);
                            setCandidateDraft((draft) => ({
                              ...draft,
                              memoryType: event.target.value as MemoryType,
                            }));
                          }}
                        >
                          {candidateMemoryTypeChoices(selectedCandidate).map((value) => (
                            <option key={value} value={value}>
                              {value}
                            </option>
                          ))}
                        </SelectControl>
                      </label>
                    </div>
                    <label className="field memories-body-field">
                      <span>Content</span>
                      <textarea
                        aria-label="Suggestion content"
                        value={candidateDraft.body}
                        rows={10}
                        onChange={(event) => {
                          setCandidateEvidenceConfirmed(false);
                          setCandidateDraft((draft) => ({
                            ...draft,
                            body: event.target.value,
                          }));
                        }}
                      />
                    </label>
                    <div className="memories-detail-toolbar">
                      <label className="field memories-kind-field">
                        <span>Valid from</span>
                        <input
                          type="datetime-local"
                          value={localDateTime(candidateDraft.validFrom)}
                          onChange={(event) => {
                            setCandidateEvidenceConfirmed(false);
                            setCandidateDraft((draft) => ({
                              ...draft,
                              validFrom: storedDateTime(event.target.value),
                            }));
                          }}
                        />
                      </label>
                      <label className="field memories-kind-field">
                        <span>Expires</span>
                        <input
                          type="datetime-local"
                          value={localDateTime(candidateDraft.expiresAt)}
                          onChange={(event) => {
                            setCandidateEvidenceConfirmed(false);
                            setCandidateDraft((draft) => ({
                              ...draft,
                              expiresAt: storedDateTime(event.target.value),
                            }));
                          }}
                        />
                      </label>
                    </div>
                    <label className="suggestion-evidence-confirmation">
                      <input
                        type="checkbox"
                        checked={candidateEvidenceConfirmed}
                        onChange={(event) =>
                          setCandidateEvidenceConfirmed(event.target.checked)
                        }
                      />
                      <span>
                        I reconfirm that the evidence below supports this edited claim.
                      </span>
                    </label>
                    <div className="memories-edit-actions">
                      <button
                        type="button"
                        className="ghost"
                        disabled={candidateBusy === selectedCandidate.id}
                        onClick={() => void cancelCandidateEditing()}
                      >
                        Cancel
                      </button>
                      <button
                        type="button"
                        className="primary"
                        disabled={
                          candidateBusy === selectedCandidate.id ||
                          !candidateDraft.title.trim() ||
                          !candidateDraft.body.trim() ||
                          !candidateEvidenceConfirmed
                        }
                        onClick={() =>
                          void saveCandidateEdits(selectedCandidate)
                        }
                      >
                        {candidateBusy === selectedCandidate.id
                          ? "Saving…"
                          : "Save changes"}
                      </button>
                    </div>
                  </fieldset>
                ) : (
                  <>
                    <div className="memories-content-surface">
                      <pre>{selectedCandidate.body}</pre>
                    </div>
                    <div className="suggestion-rationale">
                      <strong>Why it was proposed</strong>
                      <p>{selectedCandidate.rationale}</p>
                    </div>
                    {selectedCandidateTarget ? (
                      <section className="suggestion-current-target" aria-label="Current target memory">
                        <strong>Current target</strong>
                        <h4>{selectedCandidateTarget.title}</h4>
                        <pre>{selectedCandidateTarget.body}</pre>
                        <p className="muted">
                          Reviewed revision {selectedCandidate.expectedTargetSemanticRevision ?? "unknown"}; current revision {selectedCandidateTarget.semanticRevision}.
                        </p>
                      </section>
                    ) : selectedCandidate.targetMemoryId ? (
                      <p className="suggestion-blocked">
                        The reviewed target is no longer available in this memory list.
                      </p>
                    ) : null}
                    {selectedCandidate.validFrom || selectedCandidate.expiresAt ? (
                      <p className="muted suggestion-validity">
                        {selectedCandidate.validFrom
                          ? `Effective ${formatWhen(selectedCandidate.validFrom)}`
                          : "Effective immediately"}
                        {selectedCandidate.expiresAt
                          ? ` · Expires ${formatWhen(selectedCandidate.expiresAt)}`
                          : ""}
                      </p>
                    ) : null}
                    {selectedCandidate.possibleConflicts.length > 0 ? (
                      <section className="suggestion-possible-conflicts" aria-label="Possible relationships">
                        <strong>Possible relationships</strong>
                        <p className="muted">These matches may be related. They were not merged or used as replacement targets.</p>
                        <ul>
                          {selectedCandidate.possibleConflicts.map((conflict) => (
                            <li key={conflict.memoryId}>
                              {conflict.title} · {conflict.relation.replace(/_/g, " ")}
                            </li>
                          ))}
                        </ul>
                      </section>
                    ) : null}
                  </>
                )}

                <section className="suggestion-evidence" aria-label="Suggestion evidence">
                  <h4>Evidence</h4>
                  {selectedCandidate.evidenceSubject ? (
                    <p className="muted">
                      Evidence category: {evidenceSubjectLabel(selectedCandidate.evidenceSubject)}
                    </p>
                  ) : null}
                  {selectedCandidate.isInference ? (
                    <p className="suggestion-inference-label">
                      Inference — review whether the cited text supports this conclusion.
                    </p>
                  ) : null}
                  {selectedCandidate.evidenceState !== "verified" ? (
                    <p className="suggestion-evidence-unavailable">
                      {selectedCandidate.evidenceState === "source_removed"
                        ? "The source history was deleted. Copied quotes and digests were removed."
                        : "This legacy suggestion has no verifiable recorded evidence."}
                    </p>
                  ) : selectedCandidate.evidence.length === 0 ? (
                    <p className="suggestion-evidence-unavailable">
                      Recorded evidence is unavailable. This suggestion cannot be approved.
                    </p>
                  ) : (
                    <ul>
                      {selectedCandidate.evidence.map((item) => (
                        <li key={`${item.fragmentId}-${item.ordinal}`}>
                          <div className="suggestion-evidence-source">
                            {item.sourceLabel ?? "Recorded source"}
                            {` · ${evidenceOriginLabel(item.origin)}`}
                            {item.sourceNodeId ? ` · Step ${item.sourceNodeId}` : ""}
                          </div>
                          {item.sourceRemoved || !item.quote ? (
                            <p>Source removed.</p>
                          ) : (
                            <blockquote>{item.quote}</blockquote>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                  <p className="muted">
                    Exact quotes prove source linkage. You decide whether they support the claim.
                  </p>
                </section>

                <div className="muted memories-detail-meta">
                  <p>
                    Proposed scope: {selectedCandidate.scopeType} · Type:{" "}
                    {selectedCandidate.memoryType} ·{" "}
                    {CANDIDATE_OPERATION_LABELS[selectedCandidate.operation]}
                  </p>
                  <button
                    type="button"
                    className="ghost memories-history-link"
                    onClick={() =>
                      void openRunHistory(selectedCandidate.reviewRunId)
                    }
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
                      onClick={() =>
                        void openTargetMemory(selectedCandidate.targetMemoryId!)
                      }
                    >
                      Open target memory
                    </button>
                  ) : null}
                </div>

                {candidateCanReject(selectedCandidate) ? (
                  <div className="suggestion-actions">
                    {!candidateEditing && candidateIsEditable(selectedCandidate) ? (
                      <button
                        type="button"
                        className="ghost"
                        onClick={() => {
                          setCandidateDraft({
                            expectedCandidateRevision:
                              selectedCandidate.candidateRevision,
                            title: selectedCandidate.title,
                            body: selectedCandidate.body,
                            scopeType: selectedCandidate.scopeType,
                            memoryType: selectedCandidate.memoryType,
                            validFrom: selectedCandidate.validFrom ?? "",
                            expiresAt: selectedCandidate.expiresAt ?? "",
                          });
                          setCandidateEvidenceConfirmed(false);
                          setCandidateEditing(true);
                        }}
                      >
                        <Icon name="pencil-simple" size={15} />
                        Edit
                      </button>
                    ) : null}
                    <button
                      type="button"
                      className="ghost context-action"
                      disabled={Boolean(candidateBusy) || candidateEditing}
                      onClick={() =>
                        void decideCandidate(selectedCandidate, "reject")
                      }
                    >
                      Reject
                    </button>
                    {candidateIsEditable(selectedCandidate) ? (
                      <button
                        type="button"
                        className="primary"
                        disabled={Boolean(candidateBusy) || candidateEditing}
                        onClick={() =>
                          void decideCandidate(selectedCandidate, "approve")
                        }
                      >
                        {candidateBusy === selectedCandidate.id
                          ? "Working…"
                          : "Approve"}
                      </button>
                    ) : null}
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
          ) : editing || creating || selected ? (
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
                  <div className="memory-detail-eyebrow">
                    <Icon name="note" size={16} />{" "}
                    {
                      MEMORY_TYPE_LABELS[
                        creating ? memoryType : (selected?.memoryType ?? "note")
                      ]
                    }
                  </div>
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
                    <h3>{creating ? "New memory" : "Edit memory"}</h3>
                  )}
                  {selected ? (
                    <p className="muted">
                      Updated{" "}
                      {formatWhen(selected.updatedAt || selected.createdAt)}
                      {selected.artifactPath
                        ? ". Artifact stored on disk."
                        : ""}
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
                          htmlExpanded
                            ? "Exit expanded preview"
                            : "Expand preview"
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
                      aria-pressed={selected.pinned}
                      disabled={
                        pinSaving ||
                        !canPinMemory(selected) ||
                        (!selected.pinned && !isMemoryPromptEligible(selected))
                      }
                      onClick={() => void toggleSelectedPin()}
                    >
                      <Icon name="push-pin" size={15} />
                      {pinSaving
                        ? "Updating…"
                        : selected.pinned
                          ? "Unpin"
                          : "Pin memory"}
                    </button>
                  ) : null}
                  {selected && !editing && !isLinkedSelected ? (
                    <button
                      type="button"
                      className="ghost"
                      onClick={() => {
                        setDraftSemanticRevision(selected.semanticRevision);
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
                        {!selected.artifactPath ? (
                          <MenuItem
                            onSelect={() => {
                              setShowDetailMenu(false);
                              void startNewNote(selected.memoryType, selected);
                            }}
                          >
                            Make an editable copy
                          </MenuItem>
                        ) : null}
                        {selected.artifactPath ? (
                          <MenuItem
                            onSelect={() => {
                              setShowDetailMenu(false);
                              void revealItemInDir(
                                selected.artifactPath!,
                              ).catch(() =>
                                setActionError(
                                  "Could not locate the memory file.",
                                ),
                              );
                            }}
                          >
                            Show original file
                          </MenuItem>
                        ) : null}
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

              {!editing && selectedUsage ? (
                <div
                  className={`memory-usage-summary is-${selectedUsage.state}`}
                >
                  <Icon
                    name={
                      selectedUsage.state === "pinned"
                        ? "push-pin"
                        : selectedUsage.state === "available"
                          ? "note"
                          : "clock"
                    }
                    size={18}
                  />
                  <div>
                    <strong>{selectedUsage.label}</strong>
                    <p>{selectedUsage.description}</p>
                  </div>
                </div>
              ) : null}
              {editing ? (
                <MemoryEditor
                  draft={{
                    title,
                    body,
                    kind,
                    scopeType,
                    memoryType,
                    status,
                    salience,
                    confidence,
                    observedAt,
                    lastConfirmedAt,
                    validFrom,
                    expiresAt,
                  }}
                  onChange={updateDraft}
                  onSave={() => void saveSelected()}
                  onCancel={() => void cancelEditing()}
                  creating={creating}
                  saving={saving}
                  dirty={dirty}
                  hasWorkingDirectory={hasWorkingDirectory}
                  fileBacked={!creating && Boolean(selected?.artifactPath)}
                />
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
                <section className="suggestion-evidence memory-evidence" aria-label="Memory evidence">
                  <h4>Provenance</h4>
                  {selected.evidenceSubject ? (
                    <p className="suggestion-inference-label">
                      Evidence category: {evidenceSubjectLabel(selected.evidenceSubject)}
                      {selected.evidenceIsInference
                        ? " · Inference — the approved claim goes beyond its exact quotes."
                        : ""}
                    </p>
                  ) : null}
                  {memoryEvidenceLoading ? (
                    <p className="muted">Loading evidence…</p>
                  ) : memoryEvidenceError ? (
                    <div className="suggestion-evidence-unavailable" role="alert">
                      <p>Evidence could not be loaded.</p>
                      <button
                        type="button"
                        className="ghost"
                        onClick={() => setMemoryEvidenceRetry((value) => value + 1)}
                      >
                        Try again
                      </button>
                    </div>
                  ) : memoryEvidence.length > 0 ? (
                    <>
                      <p className="muted">Approved from recorded run evidence.</p>
                      <ul>
                        {memoryEvidence.map((item) => (
                          <li key={`${item.fragmentId}-${item.ordinal}`}>
                            <div className="suggestion-evidence-source">
                              {item.sourceLabel ?? "Recorded source"}
                              {` · ${evidenceOriginLabel(item.origin)}`}
                              {item.sourceNodeId ? ` · Step ${item.sourceNodeId}` : ""}
                            </div>
                            {item.sourceRemoved || !item.quote ? (
                              <p>Source history removed.</p>
                            ) : (
                              <blockquote>{item.quote}</blockquote>
                            )}
                            <button
                              type="button"
                              className="ghost memories-history-link"
                              onClick={() => void openRunHistory(item.sourceRunId)}
                            >
                              Open source run in History
                            </button>
                          </li>
                        ))}
                      </ul>
                      <p className="muted">
                        Deleting source history removes copied quotes while keeping the approved memory.
                      </p>
                    </>
                  ) : (
                    <p className={
                      selected.provenanceState === "manual" || selected.source === "manual"
                        ? "muted"
                        : "suggestion-evidence-unavailable"
                    }>
                      {memoryProvenanceCopy(selected)}
                    </p>
                  )}
                </section>
              ) : null}

              {selected && !editing ? (
                <div className="memory-details-footer">
                  <dl className="memory-facts">
                    <div>
                      <dt>Available to</dt>
                      <dd>
                        {selected.scopeType === "user"
                          ? "All my workflows"
                          : selected.scopeType === "workspace"
                            ? "This workspace"
                            : "This workflow"}
                      </dd>
                    </div>
                    <div>
                      <dt>Source</dt>
                      <dd>
                        {selected.sourceWorkflowName ??
                          (selected.source === "manual"
                            ? "Written by you"
                            : selected.source === "run"
                              ? "Workflow output"
                              : selected.source)}
                      </dd>
                    </div>
                    <div>
                      <dt>Importance</dt>
                      <dd>{selected.salience ?? 50} / 100</dd>
                    </div>
                  </dl>
                  {selected.artifactPath ? (
                    <p className="memory-file-note">
                      <Icon name="file" size={16} /> Showing the saved file
                      preview.{" "}
                      <button
                        type="button"
                        className="ghost"
                        onClick={() =>
                          void revealItemInDir(selected.artifactPath!).catch(
                            () =>
                              setActionError(
                                "Could not locate the memory file.",
                              ),
                          )
                        }
                      >
                        Show original file
                      </button>
                    </p>
                  ) : null}
                  {selected.runId ? (
                    <button
                      type="button"
                      className="ghost memories-history-link"
                      onClick={() => onOpenRunHistory(selected.runId!)}
                    >
                      <Icon name="arrow-square-out" size={16} /> View source run
                    </button>
                  ) : null}
                  {supersedes ? (
                    <p className="muted">Replaces {supersedes.title}</p>
                  ) : null}
                  {supersededBy ? (
                    <p className="muted">Replaced by {supersededBy.title}</p>
                  ) : null}
                  {visiblePinnedBytes > 6_000 ? (
                    <p className="memory-budget-note">
                      Your pinned memories exceed the context limit. Some will
                      stay in the library without being included in a run.
                    </p>
                  ) : null}
                </div>
              ) : null}
            </>
          ) : (
            <div className="memories-inspector-placeholder">
              <Icon name="note" size={28} />
              <h3>Select a memory</h3>
              <p className="muted">
                Keep useful context, decisions, and preferences here. Select a
                memory to read it or create one for your agents.
              </p>
              <button
                type="button"
                className="primary"
                onClick={() => void startNewNote()}
              >
                Create a memory
              </button>
            </div>
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
            <SearchInput
              onClear={() => setLinkerQuery("")}
              ref={linkerSearchRef}
              type="search"
              aria-label="Search linkable memories"
              placeholder="Search other workflows"
              value={linkerQuery}
              onChange={(event) => setLinkerQuery(event.target.value)}
            />
            <div className="memories-link-picker-results">
              {linkerError ? (
                <div className="memories-link-picker-error" role="alert">
                  <p>{linkerError}</p>
                  <button
                    type="button"
                    className="ghost"
                    onClick={() => setLinkerRetry((current) => current + 1)}
                  >
                    Retry
                  </button>
                </div>
              ) : null}
              {linkerLoading ? (
                <div
                  className="memories-link-picker-loading"
                  aria-label="Loading"
                >
                  <span />
                  <span />
                  <span />
                </div>
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
