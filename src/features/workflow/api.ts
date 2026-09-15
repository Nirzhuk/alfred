import { invoke } from "@tauri-apps/api/core";
import type { ProviderModels } from "./models";
import type { AgentCapabilityManifest } from "./agent-capabilities";
import type {
  AgentProvider,
  AgentHarness,
  AgentProviderId,
  AgentUsageSnapshot,
  ActiveRunInfo,
  AppTriggerStatus,
  HistorySearchHit,
  HistorySearchInput,
  MemoryEvidenceItem,
  MemoryCandidate,
  MemoryCandidateStatus,
  MemoryKind,
  MemoryReviewJob,
  MemoryReviewSettings,
  MemoryLookupAvailability,
  MemoryScopeType,
  MemorySource,
  MemoryStatus,
  MemoryType,
  MissedSchedule,
  OnMissed,
  OutputMemory,
  OutputReceipt,
  RetryOutputPersistenceRequest,
  RunHistoryDetail,
  RunHistoryItem,
  RunSummary,
  Schedule,
  ScheduleListItem,
  Skill,
  Trigger,
  TriggerSource,
  Workflow,
  WorkflowFolder,
  WorkflowGraph,
  WorkflowMemoryReview,
} from "./types";

export async function listWorkflows(): Promise<Workflow[]> {
  return invoke("list_workflows");
}

export async function getWorkflow(id: string): Promise<Workflow | null> {
  return invoke("get_workflow", { id });
}

export async function createWorkflow(input: {
  name: string;
  description?: string;
  workingDirectory?: string;
  folderId?: string | null;
  graph?: WorkflowGraph;
}): Promise<Workflow> {
  return invoke("create_workflow", { input });
}

export async function updateWorkflow(input: {
  id: string;
  name?: string;
  description?: string;
  workingDirectory?: string;
  memoryRetrievalEnabled?: boolean;
  memoryLookupEnabled?: boolean;
  graph?: WorkflowGraph;
}): Promise<Workflow> {
  return invoke("update_workflow", { input });
}

export async function getMemoryLookupAvailability(
  workflowId: string,
): Promise<MemoryLookupAvailability> {
  return invoke("get_memory_lookup_availability", { workflowId });
}

export async function deleteWorkflow(id: string): Promise<void> {
  return invoke("delete_workflow", { id });
}

/** Persist sidebar order (top → bottom). */
export async function reorderWorkflows(orderedIds: string[]): Promise<void> {
  return invoke("reorder_workflows", { orderedIds });
}

export async function listWorkflowFolders(): Promise<WorkflowFolder[]> {
  return invoke("list_workflow_folders");
}

export async function createWorkflowFolder(name: string): Promise<WorkflowFolder> {
  return invoke("create_workflow_folder", { name });
}

export async function renameWorkflowFolder(
  id: string,
  name: string,
): Promise<WorkflowFolder> {
  return invoke("rename_workflow_folder", { id, name });
}

export async function deleteWorkflowFolder(id: string): Promise<void> {
  return invoke("delete_workflow_folder", { id });
}

export async function reorderWorkflowFolders(orderedIds: string[]): Promise<void> {
  return invoke("reorder_workflow_folders", { orderedIds });
}

export async function moveWorkflowToFolder(
  workflowId: string,
  folderId: string | null,
): Promise<Workflow> {
  return invoke("move_workflow_to_folder", { workflowId, folderId });
}

export async function listAgentProviders(
  harness?: AgentHarness,
): Promise<AgentProvider[]> {
  return invoke("list_agent_providers", { harness: harness ?? null });
}

export async function listAgentModels(
  harness: AgentHarness = "cli",
): Promise<ProviderModels[]> {
  return invoke<ProviderModels[]>("list_agent_models", {
    harness,
  });
}

/** Explicit combined view for the editor; the compatibility API stays CLI-only. */
export async function listAllAgentModels(): Promise<ProviderModels[]> {
  const [cli, alfred] = await Promise.all([
    listAgentModels("cli"),
    listAgentModels("alfred"),
  ]);
  return [...cli, ...alfred];
}

export async function getAgentCapabilityManifest(): Promise<AgentCapabilityManifest> {
  return invoke("get_agent_capability_manifest");
}

/** Read native subscription windows for providers used by the workflow. */
export async function getAgentUsage(
  providerIds: AgentProviderId[],
): Promise<AgentUsageSnapshot[]> {
  return invoke("get_agent_usage", { providerIds });
}

/** Discover SKILL.md packages from project + user skill directories. */
export async function listSkills(projectRoot?: string): Promise<Skill[]> {
  return invoke("list_skills", { projectRoot: projectRoot ?? null });
}

/** Always available — manually trigger a workflow automation. */
export async function runWorkflow(workflowId: string): Promise<RunSummary> {
  return invoke("run_workflow", { workflowId });
}

/** Kill the active CLI child for an in-flight run. */
export async function cancelRun(runId: string): Promise<boolean> {
  return invoke("cancel_run", { runId });
}

/** In-flight runs survive window reloads and can include several workflows. */
export async function listActiveRuns(): Promise<ActiveRunInfo[]> {
  return invoke("list_active_runs");
}

export async function listRunHistory(input: {
  workflowId?: string | null;
  limit?: number;
  offset?: number;
} = {}): Promise<RunHistoryItem[]> {
  return invoke("list_run_history", {
    workflowId: input.workflowId ?? null,
    limit: input.limit ?? 25,
    offset: input.offset ?? 0,
  });
}

export async function getRunHistory(
  runId: string,
): Promise<RunHistoryDetail | null> {
  return invoke("get_run_history", { runId });
}

export async function retryOutputPersistence(
  request: RetryOutputPersistenceRequest,
): Promise<OutputReceipt> {
  return invoke("retry_output_persistence", request);
}

export async function searchHistory(
  input: HistorySearchInput,
): Promise<HistorySearchHit[]> {
  return invoke("search_history", {
    input: {
      query: input.query,
      workflowId: input.workflowId ?? null,
      limit: input.limit ?? 25,
    },
  });
}

export async function listSchedules(): Promise<ScheduleListItem[]> {
  return invoke("list_schedules");
}

export async function getWorkflowSchedule(
  workflowId: string,
): Promise<Schedule | null> {
  return invoke("get_workflow_schedule", { workflowId });
}

export async function upsertWorkflowSchedule(input: {
  workflowId: string;
  cron: string;
  enabled: boolean;
  onMissed: OnMissed;
}): Promise<Schedule> {
  return invoke("upsert_workflow_schedule", {
    workflowId: input.workflowId,
    cron: input.cron,
    enabled: input.enabled,
    onMissed: input.onMissed,
  });
}

/** Scheduled runs missed while Alfred was closed, waiting on the user. */
export async function listMissedSchedules(): Promise<MissedSchedule[]> {
  return invoke("list_missed_schedules");
}

/** Answer one missed schedule: run it now, or drop that occurrence. */
export async function resolveMissedSchedule(
  scheduleId: string,
  decision: "run" | "skip",
): Promise<void> {
  return invoke("resolve_missed_schedule", { scheduleId, decision });
}

export async function deleteWorkflowSchedule(
  workflowId: string,
): Promise<void> {
  return invoke("delete_workflow_schedule", { workflowId });
}

export async function listWorkflowTriggers(
  workflowId: string,
): Promise<Trigger[]> {
  return invoke("list_workflow_triggers", { workflowId });
}

export async function listAppTriggerStatuses(
  workflowId: string,
): Promise<AppTriggerStatus[]> {
  return invoke("list_app_trigger_statuses", { workflowId });
}

export async function upsertWorkflowTrigger(input: {
  id?: string;
  workflowId: string;
  source: TriggerSource;
  label?: string;
  config?: Record<string, unknown>;
  enabled?: boolean;
}): Promise<Trigger> {
  return invoke("upsert_workflow_trigger", {
    input: {
      id: input.id ?? null,
      workflowId: input.workflowId,
      source: input.source,
      label: input.label ?? "",
      config: input.config ?? {},
      enabled: input.enabled ?? true,
    },
  });
}

export async function deleteWorkflowTrigger(id: string): Promise<void> {
  return invoke("delete_workflow_trigger", { id });
}

/** Fire a trigger by hand to check the workflow reacts as expected. */
export async function testWorkflowTrigger(id: string): Promise<string> {
  return invoke("test_workflow_trigger", { id });
}

/** `http://127.0.0.1:<port>`, or null when the listener could not bind. */
export async function webhookBaseUrl(): Promise<string | null> {
  return invoke("webhook_base_url");
}

export async function listMemories(
  workflowId: string,
  includeHistory = false,
): Promise<OutputMemory[]> {
  return invoke("list_memories", { workflowId, includeHistory });
}

/** Memories from other workflows that can still be linked in. */
export async function listLinkableMemories(
  workflowId: string,
): Promise<OutputMemory[]> {
  return invoke("list_linkable_memories", { workflowId });
}

export async function linkMemory(
  workflowId: string,
  memoryId: string,
): Promise<OutputMemory> {
  return invoke("link_memory", { workflowId, memoryId });
}

export async function unlinkMemory(
  workflowId: string,
  memoryId: string,
): Promise<void> {
  return invoke("unlink_memory", { workflowId, memoryId });
}

export async function createMemory(input: {
  workflowId: string;
  title: string;
  body: string;
  runId?: string | null;
  nodeId?: string | null;
  kind?: MemoryKind;
  scopeType?: MemoryScopeType;
  memoryType?: MemoryType;
  source?: MemorySource;
  pinned?: boolean;
  confidence?: number;
  salience?: number;
  status?: MemoryStatus;
  supersedesId?: string | null;
  factKey?: string | null;
  observedAt?: string | null;
  lastConfirmedAt?: string | null;
  validFrom?: string | null;
  expiresAt?: string | null;
  id?: string;
}): Promise<OutputMemory> {
  return invoke("create_memory", { input });
}

export async function updateMemory(input: {
  id: string;
  expectedSemanticRevision: number;
  contextWorkflowId?: string;
  title?: string;
  body?: string;
  pinned?: boolean;
  kind?: MemoryKind;
  scopeType?: MemoryScopeType;
  memoryType?: MemoryType;
  confidence?: number;
  salience?: number;
  status?: MemoryStatus;
  supersedesId?: string | null;
  factKey?: string | null;
  observedAt?: string | null;
  lastConfirmedAt?: string | null;
  validFrom?: string | null;
  expiresAt?: string | null;
}): Promise<OutputMemory> {
  return invoke("update_memory", { input });
}

export async function deleteMemory(
  id: string,
  contextWorkflowId?: string,
): Promise<void> {
  return invoke("delete_memory", { id, contextWorkflowId });
}

export async function clearMemories(workflowId: string): Promise<number> {
  return invoke("clear_memories", { workflowId });
}

// ---------------------------------------------------------------------------
// Post-run memory review (Plan 028)
// ---------------------------------------------------------------------------

export async function getMemoryReviewSettings(): Promise<MemoryReviewSettings> {
  return invoke("get_memory_review_settings");
}

export async function updateMemoryReviewSettings(input: {
  enabled: boolean;
  provider: string | null;
  model?: string | null;
  maxCandidates?: number;
}): Promise<MemoryReviewSettings> {
  return invoke("update_memory_review_settings", { input });
}

export async function setWorkflowMemoryReview(
  workflowId: string,
  enabled: boolean,
): Promise<WorkflowMemoryReview> {
  return invoke("set_workflow_memory_review", { workflowId, enabled });
}

export async function listMemoryCandidates(input: {
  workflowId: string;
  status?: MemoryCandidateStatus;
}): Promise<MemoryCandidate[]> {
  return invoke("list_memory_candidates", { input });
}

/** Pending candidates only may be edited; decided ones are final. */
export async function updateMemoryCandidate(input: {
  id: string;
  expectedCandidateRevision: number;
  title?: string;
  body?: string;
  scopeType?: MemoryScopeType;
  memoryType?: MemoryType;
  validFrom?: string | null;
  expiresAt?: string | null;
  evidenceConfirmed?: boolean;
}): Promise<MemoryCandidate> {
  return invoke("update_memory_candidate", { input });
}

export async function listMemoryEvidence(
  memoryId: string,
): Promise<MemoryEvidenceItem[]> {
  return invoke("list_memory_evidence", { memoryId });
}

export async function approveMemoryCandidate(
  id: string,
  expectedCandidateRevision: number,
): Promise<MemoryCandidate> {
  return invoke("approve_memory_candidate", {
    input: { id, expectedCandidateRevision },
  });
}

export async function rejectMemoryCandidate(id: string): Promise<MemoryCandidate> {
  return invoke("reject_memory_candidate", { id });
}

export async function retryMemoryReview(runId: string): Promise<MemoryReviewJob> {
  return invoke("retry_memory_review", { runId });
}

export async function getMemoryReviewJob(
  runId: string,
): Promise<MemoryReviewJob | null> {
  return invoke("get_memory_review_job", { runId });
}

export async function listMemoryReviews(
  workflowId: string,
): Promise<MemoryReviewJob[]> {
  return invoke("list_memory_reviews", { workflowId });
}

export async function getWorkflowMemoryReview(
  workflowId: string,
): Promise<WorkflowMemoryReview | null> {
  return invoke("get_workflow_memory_review", { workflowId });
}

/** Data settings: physically delete decided candidates for a workflow. */
export async function clearDecidedMemoryCandidates(
  workflowId: string,
): Promise<number> {
  return invoke("clear_decided_memory_candidates", { workflowId });
}

/** `gh`-backed pickers for the git-host node. Read-only; no token leaves gh. */
export type GithubCliRepos = {
  current: string | null;
  repos: string[];
  error: string | null;
};

export async function githubCliRepos(cwd?: string): Promise<GithubCliRepos> {
  return invoke("github_cli_repos", { cwd: cwd?.trim() || null });
}

export async function githubCliBranches(
  repo: string,
): Promise<string[]> {
  return invoke("github_cli_branches", {
    repo: repo.trim(),
  });
}
