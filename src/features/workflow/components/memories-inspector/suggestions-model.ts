import type {
  MemoryCandidate,
  MemoryCandidateStatus,
  MemoryReviewJob,
  MemoryScopeType,
  MemoryType,
} from "../../types";

/**
 * Pure contracts for the Memories inspector's Suggestions queue. The React
 * component owns state; everything decision-shaped lives here so the approval
 * boundary stays testable.
 */

export const CANDIDATE_OPERATION_LABELS: Record<
  MemoryCandidate["operation"],
  string
> = {
  create: "New memory",
  supersede: "Supersede",
  retract: "Retract",
};

export const CANDIDATE_STATUS_LABELS: Record<MemoryCandidateStatus, string> = {
  pending: "Pending",
  approved: "Approved",
  rejected: "Rejected",
  blocked: "Blocked",
};

export const SUGGESTION_SCOPE_TYPES: MemoryScopeType[] = [
  "workflow",
  "workspace",
  "user",
];

const USER_STATEMENT_TYPES: MemoryType[] = [
  "preference",
  "fact",
  "decision",
  "constraint",
  "lesson",
  "note",
];
const WORKFLOW_CONFIGURATION_TYPES: MemoryType[] = [
  "fact",
  "decision",
  "constraint",
  "note",
];
const RECORDED_OBSERVATION_TYPES: MemoryType[] = [
  "fact",
  "constraint",
  "lesson",
  "episode",
  "checkpoint",
  "note",
  "output",
  "artifact",
];

export function candidateScopeChoices(
  candidate: Pick<MemoryCandidate, "evidenceSubject">,
): MemoryScopeType[] {
  return candidate.evidenceSubject === "workflow_configuration"
    ? ["workflow"]
    : SUGGESTION_SCOPE_TYPES;
}

export function candidateMemoryTypeChoices(
  candidate: Pick<MemoryCandidate, "evidenceSubject">,
): MemoryType[] {
  switch (candidate.evidenceSubject) {
    case "workflow_configuration":
      return WORKFLOW_CONFIGURATION_TYPES;
    case "recorded_observation":
      return RECORDED_OBSERVATION_TYPES;
    default:
      return USER_STATEMENT_TYPES;
  }
}

export function candidateConfidencePercent(confidence: number): number {
  return Math.round(Math.min(Math.max(confidence, 0), 1) * 100);
}

/**
 * Approving a user-scope suggestion or any retract touches memory visible
 * beyond this workflow, so the UI must confirm before calling the backend.
 */
export function candidateApprovalRequiresConfirmation(
  candidate: Pick<MemoryCandidate, "scopeType" | "operation">,
): boolean {
  return candidate.scopeType === "user" || candidate.operation === "retract";
}

/** Only pending candidates are editable; decided ones are final. */
export function candidateIsEditable(
  candidate: Pick<MemoryCandidate, "status" | "evidenceState" | "evidence">,
): boolean {
  return (
    candidate.status === "pending" &&
    candidate.evidenceState === "verified" &&
    candidate.evidence.some((item) => !item.sourceRemoved && Boolean(item.quote))
  );
}

export function candidateCanReject(
  candidate: Pick<MemoryCandidate, "status">,
): boolean {
  return candidate.status === "pending" || candidate.status === "blocked";
}

function backendCode(error: unknown): string | null {
  const value = error instanceof Error ? error.message : String(error ?? "");
  const known = [
    "candidate_evidence_unavailable",
    "candidate_not_editable",
    "candidate_not_found",
    "candidate_not_pending",
    "candidate_changed",
    "evidence_reconfirmation_required",
    "evidence_validation_failed",
    "e_claim_category_mismatch",
    "invalid_body",
    "invalid_title",
    "target_changed",
    "target_moved",
    "target_missing",
    "target_retracted",
    "target_superseded",
    "validity_not_started",
  ];
  return known.find((code) => value.includes(code)) ?? null;
}

/** Stable UI copy for command failures. Raw database/provider text is never shown. */
export function candidateActionErrorCopy(
  error: unknown,
  action: "approve" | "reject" | "edit",
): string {
  switch (backendCode(error)) {
    case "candidate_changed":
      return "This suggestion changed. Refresh it before deciding.";
    case "target_changed":
    case "target_moved":
    case "target_missing":
    case "target_retracted":
    case "target_superseded":
      return "The target memory changed after review. Run a new review or make an independent manual correction.";
    case "validity_not_started":
      return "This correction is not effective yet. Approve it on or after its effective date.";
    case "evidence_reconfirmation_required":
      return "Review the evidence and reconfirm it after your latest edit.";
    case "candidate_evidence_unavailable":
    case "evidence_validation_failed":
      return "The recorded evidence is unavailable or changed. Reject this suggestion or run the workflow again.";
    case "candidate_not_editable":
      return "This suggestion can no longer be edited.";
    case "e_claim_category_mismatch":
      return "That scope or type does not match what the recorded evidence can support.";
    case "candidate_not_pending":
      return action === "reject"
        ? "This suggestion was already decided. Refresh the suggestions list."
        : "This suggestion can no longer be approved.";
    case "candidate_not_found":
      return "This suggestion no longer exists. Refresh the suggestions list.";
    case "invalid_title":
    case "invalid_body":
      return "Check the title and content length. Your changes are still here.";
    default:
      return action === "edit"
        ? "Could not save this suggestion. Your changes are still here."
        : "That action could not be completed. Try again.";
  }
}

/** Stable explanation for a blocked candidate; never provider or DB detail. */
export function blockedCandidateCopy(code: string | null | undefined): string {
  switch (code) {
    case "target_missing":
      return "The memory this suggestion wanted to change no longer exists.";
    case "target_inactive":
      return "The target memory is no longer active, so this suggestion cannot be applied.";
    case "target_changed":
      return "The target memory changed after this suggestion was reviewed.";
    case "target_moved":
      return "The target memory moved to another scope after this suggestion was reviewed.";
    case "target_superseded":
      return "The target was already superseded. This suggestion was not redirected to its replacement.";
    case "target_retracted":
      return "The target was already retracted.";
    case "target_snapshot_unavailable":
    case "target_snapshot_incomplete":
      return "The reviewed target snapshot was unavailable or incomplete, so this change cannot be applied.";
    case "proposal_expired":
      return "This suggestion expired before approval.";
    case "target_scope_mismatch":
      return "The scope of the target memory changed after this review ran.";
    case "duplicate_content":
      return "An equivalent active memory already exists, so approving would duplicate it.";
    case "target_forbidden":
    case "target_required":
      return "This suggestion does not match its operation, so it cannot be applied as-is. Edit or reject it.";
    case "legacy_evidence_unavailable":
      return "This older suggestion has no recorded evidence and cannot be approved. Run the workflow again to create a source-backed suggestion.";
    case "source_removed":
      return "Its source history was deleted, so this suggestion can no longer be approved.";
    case "evidence_validation_failed":
      return "Its recorded evidence no longer matches this claim. Reject it or run the workflow again.";
    default:
      return "This suggestion no longer matches your current memories. Review it and try again, or reject it.";
  }
}

/** Queue order: actionable pending first, then blocked, then decided. */
const STATUS_ORDER: Record<MemoryCandidateStatus, number> = {
  pending: 0,
  blocked: 1,
  rejected: 2,
  approved: 3,
};

export function sortSuggestions(
  candidates: MemoryCandidate[],
): MemoryCandidate[] {
  return [...candidates].sort((a, b) => {
    const byStatus = STATUS_ORDER[a.status] - STATUS_ORDER[b.status];
    if (byStatus !== 0) return byStatus;
    return (
      new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
    );
  });
}

export function countPendingSuggestions(
  candidates: Pick<MemoryCandidate, "status">[],
): number {
  return candidates.filter((candidate) => candidate.status === "pending")
    .length;
}

/**
 * Replace a candidate in place after a backend decision succeeds. Rows leave
 * Pending only through this transition — never optimistically.
 */
export function applyCandidateUpdate(
  candidates: MemoryCandidate[],
  updated: MemoryCandidate,
): MemoryCandidate[] {
  return candidates.map((candidate) =>
    candidate.id === updated.id ? updated : candidate,
  );
}

/** Edit payload for a pending suggestion; empty edits are omitted. */
export type CandidateEditDraft = {
  expectedCandidateRevision: number;
  title: string;
  body: string;
  scopeType: MemoryScopeType;
  memoryType: MemoryType;
  validFrom: string;
  expiresAt: string;
};

export function candidateEditPayload(
  candidate: MemoryCandidate,
  draft: CandidateEditDraft,
): {
  id: string;
  expectedCandidateRevision: number;
  title?: string;
  body?: string;
  scopeType?: MemoryScopeType;
  memoryType?: MemoryType;
  validFrom?: string | null;
  expiresAt?: string | null;
} {
  const title = draft.title.trim();
  const body = draft.body.trim();
  return {
    id: candidate.id,
    expectedCandidateRevision: draft.expectedCandidateRevision,
    ...(title && title !== candidate.title ? { title } : {}),
    ...(body && body !== candidate.body ? { body } : {}),
    ...(draft.scopeType !== candidate.scopeType
      ? { scopeType: draft.scopeType }
      : {}),
    ...(draft.memoryType !== candidate.memoryType
      ? { memoryType: draft.memoryType }
      : {}),
    ...(draft.validFrom !== (candidate.validFrom ?? "")
      ? { validFrom: draft.validFrom || null }
      : {}),
    ...(draft.expiresAt !== (candidate.expiresAt ?? "")
      ? { expiresAt: draft.expiresAt || null }
      : {}),
  };
}

export function hasCandidateEdits(
  candidate: MemoryCandidate,
  draft: CandidateEditDraft,
): boolean {
  return Object.keys(candidateEditPayload(candidate, draft)).length > 2;
}

/**
 * Event-scoped refresh: `memory://candidates-changed` refreshes only when the
 * inspector is open AND the event belongs to the active workflow.
 */
export function shouldRefreshSuggestions(
  event: { workflowId?: string; pendingCount?: number },
  activeWorkflowId: string | null,
  open: boolean,
): boolean {
  return Boolean(open && activeWorkflowId && event.workflowId === activeWorkflowId);
}

export function suggestionAnnouncement(
  previousPending: number,
  nextPending: number,
): string | null {
  if (previousPending === nextPending) return null;
  if (nextPending === 0) return "No pending memory suggestions.";
  if (nextPending < previousPending)
    return `${nextPending} pending memory suggestion${
      nextPending === 1 ? "" : "s"
    } remaining.`;
  return `${nextPending} new pending memory suggestion${
    nextPending === 1 ? "" : "s"
  }.`;
}

/** Failed reviews surfaced at the top of the Suggestions queue. */
export function failedReviewJobs(
  jobs: MemoryReviewJob[],
): MemoryReviewJob[] {
  return jobs.filter((job) => job.status === "failed");
}

export function reviewStatusLabel(
  status: MemoryReviewJob["status"],
): string {
  switch (status) {
    case "pending":
      return "Queued";
    case "running":
      return "Reviewing";
    case "completed":
      return "Reviewed";
    case "failed":
      return "Review failed";
    case "skipped":
      return "Skipped";
    default:
      return status;
  }
}
