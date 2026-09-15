import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { create } from "zustand";
import {
  addEdge,
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type EdgeChange,
  type NodeChange,
} from "@xyflow/react";
import * as api from "./api";
import { FALLBACK_PROVIDER_MODELS, type ProviderModels } from "./models";
import type { AgentCapabilityManifest } from "./agent-capabilities";
import {
  emptyGraph,
  agentSkillNames,
  parseAgentHarness,
  isAgentNodeData,
  isIdentityNodeData,
  isOutputNodeData,
  isPromptNodeData,
  normalizeOutputNodeData,
  normalizeIdentityNodeData,
  type AgentNodeData,
  type AgentProviderId,
  type AgentStepStats,
  type AgentUsageSnapshot,
  type AppTriggerStatus,
  type MissedSchedule,
  type OnMissed,
  type OutputMemory,
  type OutputPersistenceEvent,
  type RunEvent,
  type RunLogLine,
  type RunStepStatus,
  type RunSummary,
  type Schedule,
  type ScheduleListItem,
  type Skill,
  type Trigger,
  type TriggerSource,
  type Workflow,
  type WorkflowFolder,
  type WorkflowEdge,
  type WorkflowNode,
  type WorkflowNodeData,
} from "./types";
import {
  clearLegacyMemories,
  loadLegacyMemories,
  removeMigratedLegacyMemories,
  asOwnedMemory,
  canPinMemory,
  sortMemories,
} from "./memories";
import { notifyRunFinished, shouldNotifyAboutRun } from "../../native";
import { useToastStore } from "../../components/toast/toast-store";

type AddMemoryInput = {
  workflowId?: string;
  title: string;
  body: string;
  runId?: string | null;
  nodeId?: string | null;
  kind?: OutputMemory["kind"];
  scopeType?: OutputMemory["scopeType"];
  memoryType?: OutputMemory["memoryType"];
  source?: OutputMemory["source"];
  pinned?: boolean;
  confidence?: number;
  salience?: number;
  status?: OutputMemory["status"];
  supersedesId?: string | null;
  factKey?: string | null;
  observedAt?: string | null;
  lastConfirmedAt?: string | null;
  validFrom?: string | null;
  expiresAt?: string | null;
};

export type WorkflowRunState = {
  activeRun: RunSummary | null;
  activeNodeId: string | null;
  inspectedNodeId: string | null;
  stepStatuses: Record<string, RunStepStatus>;
  stepOutputs: Record<string, string>;
  stepStats: Record<string, AgentStepStats>;
  runLogs: RunLogLine[];
};

function pendingStepStatuses(
  nodes: WorkflowNode[],
): Record<string, RunStepStatus> {
  return Object.fromEntries(nodes.map((node) => [node.id, "pending"]));
}

function emptyWorkflowRunState(
  nodes: WorkflowNode[] = [],
  inspectedNodeId: string | null = null,
): WorkflowRunState {
  return {
    activeRun: null,
    activeNodeId: null,
    inspectedNodeId,
    stepStatuses: pendingStepStatuses(nodes),
    stepOutputs: {},
    stepStats: {},
    runLogs: [],
  };
}

function visibleRunFields(runState: WorkflowRunState) {
  return {
    activeRun: runState.activeRun,
    activeNodeId: runState.activeNodeId,
    inspectedNodeId: runState.inspectedNodeId,
    stepStatuses: runState.stepStatuses,
    stepOutputs: runState.stepOutputs,
    stepStats: runState.stepStats,
    runLogs: runState.runLogs,
  };
}

const OPEN_TABS_KEY = "alfred:open-workflow-tabs";
const MAX_RUN_LOG_LINES = 1_000;
// JavaScript strings are commonly two bytes per code unit. This keeps console
// text near 2 MiB per workflow before object overhead, even when every activity
// detail is individually valid but collectively huge.
const MAX_RUN_LOG_CHARS = 1_000_000;

function runLogLine(event: RunEvent): RunLogLine {
  const activity =
    event.kind === "agent_activity" ? event.activity ?? null : null;
  const id = activity
    ? `agent-activity:${JSON.stringify([
        event.runId,
        event.nodeId ?? null,
        activity.id,
      ])}`
    : `${event.at}-${event.kind}-${event.nodeId ?? "run"}-${Math.random()}`;

  return {
    id,
    at: event.at,
    kind: event.kind,
    nodeId: event.nodeId,
    nodeLabel: event.nodeLabel,
    message: event.message,
    // Step output has one home: stepOutputs. Console rows only need the summary
    // and normalized activity detail rendered by RunActivityPanel.
    output: undefined,
    activity,
    status: event.status,
  };
}

function nextRunLogs(
  current: RunLogLine[],
  line: RunLogLine,
  freshRun: boolean,
): RunLogLine[] {
  let next: RunLogLine[];
  if (freshRun) {
    next = [line];
  } else if (line.activity) {
    const existingIndex = current.findIndex((item) => item.id === line.id);
    if (existingIndex >= 0) {
      next = current.slice();
      next[existingIndex] = line;
    } else {
      next = [...current, line];
    }
  } else {
    next = [...current, line];
  }

  const rowBounded = next.length > MAX_RUN_LOG_LINES
    ? next.slice(-MAX_RUN_LOG_LINES)
    : next;

  let retainedChars = 0;
  let firstRetained = rowBounded.length;
  while (firstRetained > 0) {
    const item = rowBounded[firstRetained - 1];
    const itemChars =
      item.message.length +
      (item.nodeLabel?.length ?? 0) +
      (item.activity?.label.length ?? 0) +
      (item.activity?.detail?.length ?? 0);
    if (retainedChars + itemChars > MAX_RUN_LOG_CHARS) break;
    retainedChars += itemChars;
    firstRetained -= 1;
  }

  return firstRetained === 0 ? rowBounded : rowBounded.slice(firstRetained);
}

function loadOpenTabs(): string[] {
  try {
    const raw = localStorage.getItem(OPEN_TABS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as string[];
    return Array.isArray(parsed) ? parsed.filter((id) => typeof id === "string") : [];
  } catch {
    return [];
  }
}

function persistOpenTabs(ids: string[]) {
  try {
    localStorage.setItem(OPEN_TABS_KEY, JSON.stringify(ids));
  } catch {
    /* ignore */
  }
}

function withOpenTab(ids: string[], id: string): string[] {
  if (ids.includes(id)) return ids;
  return [...ids, id];
}

/** Restore node defaults and migrate legacy fields on load. */
export function normalizeNodes(nodes: WorkflowNode[]): WorkflowNode[] {
  return nodes.map((node) => {
    if (node.type === "identity" || isIdentityNodeData(node.data)) {
      return {
        ...node,
        type: "identity",
        data: normalizeIdentityNodeData(
          node.data as unknown as Record<string, unknown>,
        ),
      };
    }
    if (node.type === "chooseOutput" || isOutputNodeData(node.data)) {
      return {
        ...node,
        type: "chooseOutput",
        data: normalizeOutputNodeData(
          node.data as unknown as Record<string, unknown>,
        ),
      };
    }
    if (!isAgentNodeData(node.data)) return node;
    const raw = node.data as AgentNodeData & Record<string, unknown>;
    const skillNames = agentSkillNames(raw);
    const next: AgentNodeData = {
      label: raw.label,
      provider: raw.provider,
      harness: parseAgentHarness(raw.harness),
      accountRef:
        typeof raw.accountRef === "string" && raw.accountRef.trim()
          ? raw.accountRef.trim()
          : undefined,
      model:
        typeof raw.model === "string" || raw.model === null
          ? raw.model
          : undefined,
      skillNames,
    };
    return { ...node, data: next };
  });
}

type WorkflowStore = {
  workflows: Workflow[];
  workflowFolders: WorkflowFolder[];
  activeWorkflowId: string | null;
  /** Workflow ids currently open as titlebar tabs. */
  openWorkflowIds: string[];
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
  skills: Skill[];
  providerModels: ProviderModels[];
  agentCapabilityManifest: AgentCapabilityManifest | null;
  schedule: Schedule | null;
  /** Saved schedules for labels across the entire workflow sidebar. */
  workflowSchedules: ScheduleListItem[];
  /** Runs missed while Alfred was closed, waiting on the user's decision. */
  missedSchedules: MissedSchedule[];
  triggers: Trigger[];
  appTriggerStatuses: AppTriggerStatus[];
  /** `http://127.0.0.1:<port>` for webhook triggers; null if the listener is down. */
  webhookBaseUrl: string | null;
  selectedNodeId: string | null;
  dirty: boolean;
  loading: boolean;
  /** Loading state owned by the visible workflow handoff. */
  workflowLoading: boolean;
  error: string | null;
  runPanelOpen: boolean;
  /** Run activity is isolated per workflow so several workflows can run together. */
  workflowRunStates: Record<string, WorkflowRunState>;
  activeRun: RunSummary | null;
  activeNodeId: string | null;
  inspectedNodeId: string | null;
  stepStatuses: Record<string, RunStepStatus>;
  stepOutputs: Record<string, string>;
  stepStats: Record<string, AgentStepStats>;
  agentUsage: AgentUsageSnapshot[];
  usageLoading: boolean;
  runLogs: RunLogLine[];
  memories: OutputMemory[];
  selectedOutput: {
    title: string;
    body: string;
    nodeId?: string | null;
  } | null;
  loadWorkflows: () => Promise<void>;
  loadSkills: (projectRoot?: string) => Promise<void>;
  loadProviderModels: () => Promise<void>;
  loadAgentUsage: (providers: AgentProviderId[]) => Promise<void>;
  loadSchedule: (workflowId: string) => Promise<void>;
  /** Re-read every schedule so sidebar labels match the database after the
   * schedules page mutates outside this store (delete, force-off). */
  refreshWorkflowSchedules: () => Promise<void>;
  saveSchedule: (input: {
    workflowId: string;
    cron: string;
    enabled: boolean;
    onMissed: OnMissed;
  }) => Promise<void>;
  clearSchedule: (workflowId: string) => Promise<void>;
  /** Load the missed-run list the user still has to answer. */
  loadMissedSchedules: () => Promise<void>;
  /**
   * Answer one missed schedule. Resolves with the error message when the
   * backend refused the answer, so the prompt can attribute it to that row.
   */
  resolveMissedSchedule: (
    scheduleId: string,
    decision: "run" | "skip",
  ) => Promise<string | null>;
  loadTriggers: (workflowId: string) => Promise<void>;
  saveTrigger: (input: {
    id?: string;
    workflowId: string;
    source: TriggerSource;
    label?: string;
    config?: Record<string, unknown>;
    enabled?: boolean;
  }) => Promise<Trigger | null>;
  removeTrigger: (id: string) => Promise<void>;
  testTrigger: (id: string) => Promise<void>;
  selectWorkflow: (id: string) => Promise<boolean>;
  ensureWorkflowMemoryContext: (id: string) => Promise<boolean>;
  closeWorkflowTab: (id: string) => Promise<void>;
  createWorkflow: (name?: string, folderId?: string | null) => Promise<void>;
  createWorkflowFolder: (name: string) => Promise<WorkflowFolder | null>;
  renameWorkflowFolder: (id: string, name: string) => Promise<void>;
  deleteWorkflowFolder: (id: string) => Promise<void>;
  moveWorkflowToFolder: (
    workflowId: string,
    folderId: string | null,
    beforeWorkflowId?: string,
  ) => Promise<void>;
  renameWorkflow: (id: string, name: string) => Promise<void>;
  setWorkingDirectory: (id: string, workingDirectory: string) => Promise<void>;
  setMemoryRetrievalEnabled: (id: string, enabled: boolean) => Promise<void>;
  setMemoryLookupEnabled: (id: string, enabled: boolean) => Promise<boolean>;
  reorderWorkflows: (orderedIds: string[]) => Promise<void>;
  deleteWorkflow: (id: string) => Promise<void>;
  saveActiveWorkflow: () => Promise<boolean>;
  runActiveWorkflow: (workflowId?: string) => Promise<void>;
  cancelActiveRun: (workflowId?: string) => Promise<void>;
  openRunPanel: (nodeId?: string | null) => void;
  closeRunPanel: () => void;
  openOutput: (output: {
    title: string;
    body: string;
    nodeId?: string | null;
  }) => void;
  closeOutput: () => void;
  loadMemories: (workflowId: string) => Promise<boolean>;
  addMemory: (memory: AddMemoryInput) => Promise<OutputMemory | null>;
  addNote: (title: string, body: string) => Promise<void>;
  linkMemory: (memoryId: string) => Promise<OutputMemory | null>;
  unlinkMemory: (memoryId: string) => Promise<void>;
  updateMemoryFields: (input: {
    id: string;
    expectedSemanticRevision?: number;
    title?: string;
    body?: string;
    pinned?: boolean;
    kind?: OutputMemory["kind"];
    scopeType?: OutputMemory["scopeType"];
    memoryType?: OutputMemory["memoryType"];
    confidence?: number;
    salience?: number;
    status?: OutputMemory["status"];
    supersedesId?: string | null;
    factKey?: string | null;
    observedAt?: string | null;
    lastConfirmedAt?: string | null;
    validFrom?: string | null;
    expiresAt?: string | null;
  }) => Promise<OutputMemory | null>;
  togglePinMemory: (id: string) => Promise<void>;
  removeMemory: (id: string) => Promise<void>;
  clearMemories: () => Promise<void>;
  handleRunEvent: (event: RunEvent) => void;
  handleOutputPersistenceEvent: (event: OutputPersistenceEvent) => void;
  setSelectedNodeId: (id: string | null) => void;
  updateNodeData: (nodeId: string, data: Partial<WorkflowNodeData>) => void;
  onNodesChange: (changes: NodeChange<WorkflowNode>[]) => void;
  onEdgesChange: (changes: EdgeChange<WorkflowEdge>[]) => void;
  onConnect: (connection: Connection) => void;
  addNode: (node: WorkflowNode) => void;
  removeNode: (nodeId: string) => void;
  duplicateNode: (nodeId: string) => void;
  disconnectNode: (nodeId: string) => void;
};

const LEGACY_MEMORY_MIGRATION_ERROR =
  "Some saved memories could not finish importing. Your original data has been kept; reopen this workflow to retry.";
const legacyMemoryMigrations = new Map<string, Promise<boolean>>();

async function migrateLegacyMemories(workflowId: string): Promise<boolean> {
  const pending = legacyMemoryMigrations.get(workflowId);
  if (pending) return pending;
  const legacy = loadLegacyMemories(workflowId);
  if (legacy.length === 0) return true;
  const migration = (async () => {
    const migratedIds = new Set<string>();
    let complete = true;
    for (const item of legacy) {
      try {
        await api.createMemory({
          id: item.id,
          workflowId,
          title: item.title || "Output",
          body: item.body,
          runId: item.runId,
          nodeId: item.nodeId,
          source: "import",
          kind: "text",
        });
        migratedIds.add(item.id);
      } catch {
        complete = false;
      }
    }
    try {
      removeMigratedLegacyMemories(workflowId, migratedIds);
    } catch {
      complete = false;
    }
    return complete;
  })();
  legacyMemoryMigrations.set(workflowId, migration);
  try {
    return await migration;
  } finally {
    if (legacyMemoryMigrations.get(workflowId) === migration) {
      legacyMemoryMigrations.delete(workflowId);
    }
  }
}

let runUnlisten: UnlistenFn | null = null;
let outputPersistenceUnlisten: UnlistenFn | null = null;
let usageRequestSequence = 0;
let workflowSelectionSequence = 0;
const workflowSaves = new Map<string, Promise<boolean>>();

type WorkflowPermissionField =
  | "memoryRetrievalEnabled"
  | "memoryLookupEnabled";
type WorkflowStoreSet = (
  partial:
    | Partial<WorkflowStore>
    | ((state: WorkflowStore) => Partial<WorkflowStore>),
) => void;
type WorkflowStoreGet = () => WorkflowStore;

const workflowPermissionSaves = new Map<string, Promise<boolean>>();
const workflowPermissionSequences = new Map<string, number>();
const workflowPermissionConfirmed = new Map<string, boolean>();

async function persistWorkflowPermission(
  set: WorkflowStoreSet,
  get: WorkflowStoreGet,
  workflowId: string,
  field: WorkflowPermissionField,
  enabled: boolean,
): Promise<boolean> {
  const previous = get().workflows.find(
    (workflow) => workflow.id === workflowId,
  );
  if (!previous) return false;

  const key = `${workflowId}:${field}`;
  const sequence = (workflowPermissionSequences.get(key) ?? 0) + 1;
  workflowPermissionSequences.set(key, sequence);
  const previousValue =
    field === "memoryLookupEnabled"
      ? previous.memoryLookupEnabled
      : (previous.memoryRetrievalEnabled ?? false);
  if (!workflowPermissionConfirmed.has(key)) {
    workflowPermissionConfirmed.set(key, previousValue);
  }

  set((state) => ({
    error: null,
    workflows: state.workflows.map((workflow) =>
      workflow.id === workflowId
        ? { ...workflow, [field]: enabled }
        : workflow,
    ),
  }));

  const previousSave = workflowPermissionSaves.get(key);
  let save!: Promise<boolean>;
  save = (async () => {
    if (previousSave) {
      try {
        await previousSave;
      } catch {
        // The previous request has already surfaced its own error state.
      }
    }

    try {
      const workflow = await api.updateWorkflow(
        field === "memoryLookupEnabled"
          ? { id: workflowId, memoryLookupEnabled: enabled }
          : { id: workflowId, memoryRetrievalEnabled: enabled },
      );
      const persistedValue =
        field === "memoryLookupEnabled"
          ? workflow.memoryLookupEnabled
          : (workflow.memoryRetrievalEnabled ?? enabled);
      if (typeof persistedValue !== "boolean") {
        throw new Error("invalid_workflow_permission_response");
      }
      workflowPermissionConfirmed.set(key, persistedValue);
      if (workflowPermissionSequences.get(key) === sequence) {
        set((state) => ({
          workflows: state.workflows.map((item) =>
            item.id === workflowId
              ? { ...item, [field]: persistedValue }
              : item,
          ),
        }));
      }
      return true;
    } catch (error) {
      if (workflowPermissionSequences.get(key) === sequence) {
        const rollback =
          workflowPermissionConfirmed.get(key) ?? previousValue;
        set((state) => ({
          workflows: state.workflows.map((item) =>
            item.id === workflowId
              ? { ...item, [field]: rollback }
              : item,
          ),
          error: String(error),
        }));
      }
      return false;
    } finally {
      if (workflowPermissionSaves.get(key) === save) {
        workflowPermissionSaves.delete(key);
        workflowPermissionSequences.delete(key);
        workflowPermissionConfirmed.delete(key);
      }
    }
  })();
  workflowPermissionSaves.set(key, save);
  return save;
}

async function ensureRunListener(handle: (event: RunEvent) => void) {
  if (runUnlisten) return;
  runUnlisten = await listen<RunEvent>("run://event", (event) => {
    handle(event.payload);
  });
}

/** Install the live run listener once (manual + scheduled + trigger runs). */
export async function installRunEventBridge() {
  await ensureRunListener((event) => {
    useWorkflowStore.getState().handleRunEvent(event);
  });
}

export async function installOutputPersistenceEventBridge() {
  if (outputPersistenceUnlisten) return;
  outputPersistenceUnlisten = await listen<OutputPersistenceEvent>(
    "output-persistence://receipt",
    (event) => {
      useWorkflowStore
        .getState()
        .handleOutputPersistenceEvent(event.payload);
      window.dispatchEvent(
        new CustomEvent("alfred:output-persistence-receipt", {
          detail: event.payload,
        }),
      );
    },
  );
}

export const useWorkflowStore = create<WorkflowStore>((set, get) => ({
  workflows: [],
  workflowFolders: [],
  activeWorkflowId: null,
  openWorkflowIds: loadOpenTabs(),
  nodes: [],
  edges: [],
  skills: [],
  providerModels: FALLBACK_PROVIDER_MODELS,
  agentCapabilityManifest: null,
  schedule: null,
  workflowSchedules: [],
  missedSchedules: [],
  triggers: [],
  appTriggerStatuses: [],
  webhookBaseUrl: null,
  selectedNodeId: null,
  dirty: false,
  loading: false,
  workflowLoading: false,
  error: null,
  runPanelOpen: false,
  workflowRunStates: {},
  activeRun: null,
  activeNodeId: null,
  inspectedNodeId: null,
  stepStatuses: {},
  stepOutputs: {},
  stepStats: {},
  agentUsage: [],
  usageLoading: false,
  runLogs: [],
  memories: [],
  selectedOutput: null,

  loadWorkflows: async () => {
    set({ loading: true, error: null });
    try {
      const [workflows, workflowFolders, workflowSchedules, activeRuns] = await Promise.all([
        api.listWorkflows(),
        api.listWorkflowFolders(),
        api.listSchedules(),
        api.listActiveRuns(),
      ]);
      const known = new Set(workflows.map((w) => w.id));
      let openWorkflowIds = get().openWorkflowIds.filter((id) => known.has(id));
      if (openWorkflowIds.length === 0 && workflows[0]) {
        openWorkflowIds = [workflows[0].id];
      }
      persistOpenTabs(openWorkflowIds);
      set((state) => {
        const workflowRunStates = { ...state.workflowRunStates };
        for (const run of activeRuns) {
          const workflow = workflows.find((item) => item.id === run.workflowId);
          if (!workflow) continue;
          const current = workflowRunStates[run.workflowId];
          if (current?.activeRun?.id === run.runId) continue;
          workflowRunStates[run.workflowId] = {
            ...emptyWorkflowRunState(workflow.graph?.nodes ?? []),
            activeRun: {
              id: run.runId,
              workflowId: run.workflowId,
              trigger: "active",
              status: "running",
              createdAt: new Date().toISOString(),
            },
          };
        }
        return {
          workflows,
          workflowFolders,
          workflowSchedules,
          workflowRunStates,
          openWorkflowIds,
          loading: false,
        };
      });
      const preferred =
        (get().activeWorkflowId && known.has(get().activeWorkflowId!)
          ? get().activeWorkflowId
          : null) ??
        openWorkflowIds[0] ??
        workflows[0]?.id ??
        null;
      if (preferred) {
        await get().selectWorkflow(preferred);
      }
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  loadSkills: async (projectRoot) => {
    try {
      const skills = await api.listSkills(projectRoot);
      set({ skills });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadProviderModels: async () => {
    set({ loading: true, error: null });
    try {
      const [providerModels, agentCapabilityManifest] = await Promise.all([
        api.listAllAgentModels(),
        api.getAgentCapabilityManifest().catch(() => null),
      ]);
      if (providerModels.length > 0) {
        set({ providerModels, agentCapabilityManifest, loading: false });
      } else {
        set({ agentCapabilityManifest, loading: false });
      }
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  loadAgentUsage: async (providers) => {
    const request = ++usageRequestSequence;
    if (providers.length === 0) {
      set({ agentUsage: [], usageLoading: false });
      return;
    }
    set({ usageLoading: true });
    try {
      const agentUsage = await api.getAgentUsage(providers);
      if (request !== usageRequestSequence) return;
      set({ agentUsage, usageLoading: false });
    } catch (error) {
      if (request !== usageRequestSequence) return;
      console.warn("Agent subscription usage unavailable", error);
      set({ usageLoading: false });
    }
  },
  refreshWorkflowSchedules: async () => {
    try {
      set({ workflowSchedules: await api.listSchedules() });
    } catch (e) {
      console.warn("Could not refresh schedules", e);
    }
  },

  loadSchedule: async (workflowId) => {
    try {
      const schedule = await api.getWorkflowSchedule(workflowId);
      set((state) => ({
        schedule,
        workflowSchedules: schedule
          ? [
              ...state.workflowSchedules.filter(
                (item) => item.workflowId !== workflowId,
              ),
              {
                ...schedule,
                workflowName:
                  state.workflows.find((workflow) => workflow.id === workflowId)
                    ?.name ?? "Workflow",
              },
            ]
          : state.workflowSchedules.filter(
              (item) => item.workflowId !== workflowId,
            ),
      }));
    } catch (e) {
      set({ schedule: null, error: String(e) });
    }
  },

  saveSchedule: async (input) => {
    set({ loading: true, error: null });
    try {
      const schedule = await api.upsertWorkflowSchedule(input);
      set((state) => ({
        schedule,
        workflowSchedules: [
          ...state.workflowSchedules.filter(
            (item) => item.workflowId !== input.workflowId,
          ),
          {
            ...schedule,
            workflowName:
              state.workflows.find(
                (workflow) => workflow.id === input.workflowId,
              )?.name ?? "Workflow",
          },
        ],
        loading: false,
      }));
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  clearSchedule: async (workflowId) => {
    set({ loading: true, error: null });
    try {
      await api.deleteWorkflowSchedule(workflowId);
      set((state) => ({
        schedule: null,
        workflowSchedules: state.workflowSchedules.filter(
          (item) => item.workflowId !== workflowId,
        ),
        loading: false,
      }));
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  loadMissedSchedules: async () => {
    try {
      set({ missedSchedules: await api.listMissedSchedules() });
    } catch (e) {
      // A failed read must never invent an answer prompt; the schedules stay
      // due for the next launch.
      console.warn("Could not load missed schedules", e);
    }
  },

  resolveMissedSchedule: async (scheduleId, decision) => {
    try {
      await api.resolveMissedSchedule(scheduleId, decision);
      set((state) => ({
        missedSchedules: state.missedSchedules.filter(
          (item) => item.scheduleId !== scheduleId,
        ),
      }));
      return null;
    } catch (e) {
      // The schedule stays listed and due; the prompt shows this message.
      return String(e);
    }
  },

  loadTriggers: async (workflowId) => {
    try {
      const [triggers, baseUrl, appTriggerStatuses] = await Promise.all([
        api.listWorkflowTriggers(workflowId),
        // Cheap, and the port can change between launches when 8787 is taken.
        api.webhookBaseUrl(),
        api.listAppTriggerStatuses(workflowId),
      ]);
      if (get().activeWorkflowId === workflowId) {
        set({ triggers, webhookBaseUrl: baseUrl, appTriggerStatuses });
      }
    } catch (e) {
      set({ triggers: [], appTriggerStatuses: [], error: String(e) });
    }
  },

  saveTrigger: async (input) => {
    set({ loading: true, error: null });
    try {
      const trigger = await api.upsertWorkflowTrigger(input);
      set((state) => ({
        triggers: [
          ...state.triggers.filter((t) => t.id !== trigger.id),
          trigger,
        ].sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
        loading: false,
      }));
      if (input.source === "app") {
        const appTriggerStatuses = await api.listAppTriggerStatuses(
          input.workflowId,
        );
        set({ appTriggerStatuses });
      }
      return trigger;
    } catch (e) {
      set({ loading: false, error: String(e) });
      return null;
    }
  },

  removeTrigger: async (id) => {
    try {
      await api.deleteWorkflowTrigger(id);
      set((state) => ({
        triggers: state.triggers.filter((t) => t.id !== id),
        appTriggerStatuses: state.appTriggerStatuses.filter(
          (status) => status.triggerId !== id,
        ),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  testTrigger: async (id) => {
    await ensureRunListener((event) => get().handleRunEvent(event));
    const { activeWorkflowId, nodes, workflowRunStates } = get();
    if (!activeWorkflowId) return;
    if (workflowRunStates[activeWorkflowId]?.activeRun?.status === "running") {
      set({ error: "This workflow is already running." });
      return;
    }
    const nextRunState = emptyWorkflowRunState(nodes);
    set((state) => ({
      error: null,
      runPanelOpen: true,
      workflowRunStates: {
        ...state.workflowRunStates,
        [activeWorkflowId]: nextRunState,
      },
      ...visibleRunFields(nextRunState),
    }));
    try {
      await api.testWorkflowTrigger(id);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  selectWorkflow: async (id) => {
    const request = ++workflowSelectionSequence;
    set({
      loading: true,
      workflowLoading: true,
      error: null,
      selectedNodeId: null,
      schedule: null,
      triggers: [],
      appTriggerStatuses: [],
    });
    try {
      const workflow = await api.getWorkflow(id);
      if (request !== workflowSelectionSequence) return false;
      if (!workflow) {
        set({
          loading: false,
          workflowLoading: false,
          error: `Workflow not found: ${id}`,
        });
        return false;
      }
      const graph = workflow.graph ?? emptyGraph();
      const nodes = normalizeNodes(graph.nodes ?? []);
      const runState =
        get().workflowRunStates[workflow.id] ?? emptyWorkflowRunState(nodes);
      const openWorkflowIds = withOpenTab(get().openWorkflowIds, workflow.id);
      persistOpenTabs(openWorkflowIds);
      set({
        activeWorkflowId: workflow.id,
        openWorkflowIds,
        nodes,
        edges: graph.edges ?? [],
        memories: [],
        dirty: false,
        loading: false,
        workflowLoading: true,
        selectedOutput: null,
        ...visibleRunFields(runState),
      });
      const [, , memoriesLoaded] = await Promise.all([
        get().loadSchedule(workflow.id),
        get().loadTriggers(workflow.id),
        get().loadMemories(workflow.id),
      ]);
      if (request === workflowSelectionSequence) {
        set({ workflowLoading: false });
        return memoriesLoaded && get().activeWorkflowId === workflow.id;
      }
      return false;
    } catch (e) {
      if (request !== workflowSelectionSequence) return false;
      set({ loading: false, workflowLoading: false, error: String(e) });
      return false;
    }
  },

  ensureWorkflowMemoryContext: async (id) => {
    if (get().activeWorkflowId !== id) {
      return get().selectWorkflow(id);
    }
    return get().loadMemories(id);
  },

  closeWorkflowTab: async (id) => {
    const { openWorkflowIds, activeWorkflowId, workflows } = get();
    if (!openWorkflowIds.includes(id)) return;

    const nextOpen = openWorkflowIds.filter((tabId) => tabId !== id);
    persistOpenTabs(nextOpen);
    set((state) => {
      const runState = state.workflowRunStates[id];
      if (runState?.activeRun?.status === "running") {
        return { openWorkflowIds: nextOpen };
      }
      const workflowRunStates = { ...state.workflowRunStates };
      delete workflowRunStates[id];
      return { openWorkflowIds: nextOpen, workflowRunStates };
    });

    if (activeWorkflowId !== id) return;

    const fallback =
      nextOpen[Math.max(0, openWorkflowIds.indexOf(id) - 1)] ??
      nextOpen[0] ??
      workflows.find((w) => w.id !== id)?.id ??
      null;

    if (fallback) {
      await get().selectWorkflow(fallback);
    } else {
      set({
        activeWorkflowId: null,
        nodes: [],
        edges: [],
        memories: [],
        schedule: null,
        selectedNodeId: null,
        dirty: false,
        workflowLoading: false,
        selectedOutput: null,
        ...visibleRunFields(emptyWorkflowRunState()),
      });
    }
  },

  createWorkflow: async (name = "Untitled workflow", folderId = null) => {
    set({
      loading: true,
      workflowLoading: true,
      error: null,
      selectedNodeId: null,
      schedule: null,
    });
    try {
      const workflow = await api.createWorkflow({
        name,
        folderId,
        graph: emptyGraph(),
      });
      const openWorkflowIds = withOpenTab(get().openWorkflowIds, workflow.id);
      persistOpenTabs(openWorkflowIds);
      set((state) => ({
        workflows: [workflow, ...state.workflows],
        activeWorkflowId: workflow.id,
        openWorkflowIds,
        nodes: [],
        edges: [],
        memories: [],
        dirty: false,
        loading: false,
        workflowLoading: false,
        schedule: null,
        selectedOutput: null,
        ...visibleRunFields(emptyWorkflowRunState()),
      }));
    } catch (e) {
      set({ loading: false, workflowLoading: false, error: String(e) });
    }
  },

  createWorkflowFolder: async (name) => {
    const trimmed = name.trim();
    if (!trimmed) {
      set({ error: "Folder name cannot be empty." });
      return null;
    }
    set({ loading: true, error: null });
    try {
      const folder = await api.createWorkflowFolder(trimmed);
      set((state) => ({
        workflowFolders: [...state.workflowFolders, folder],
        loading: false,
      }));
      return folder;
    } catch (e) {
      set({ loading: false, error: String(e) });
      return null;
    }
  },

  renameWorkflowFolder: async (id, name) => {
    const trimmed = name.trim();
    if (!trimmed) {
      set({ error: "Folder name cannot be empty." });
      return;
    }
    set({ loading: true, error: null });
    try {
      const folder = await api.renameWorkflowFolder(id, trimmed);
      set((state) => ({
        workflowFolders: state.workflowFolders.map((item) =>
          item.id === id ? folder : item,
        ),
        loading: false,
      }));
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  deleteWorkflowFolder: async (id) => {
    set({ loading: true, error: null });
    try {
      await api.deleteWorkflowFolder(id);
      set((state) => ({
        workflowFolders: state.workflowFolders.filter((folder) => folder.id !== id),
        workflows: state.workflows.map((workflow) =>
          workflow.folderId === id ? { ...workflow, folderId: null } : workflow,
        ),
        loading: false,
      }));
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  moveWorkflowToFolder: async (workflowId, folderId, beforeWorkflowId) => {
    const previous = get().workflows;
    const moving = previous.find((workflow) => workflow.id === workflowId);
    if (!moving) return;

    const without = previous.filter((workflow) => workflow.id !== workflowId);
    const moved = { ...moving, folderId };
    let insertAt = -1;
    if (beforeWorkflowId) {
      insertAt = without.findIndex((workflow) => workflow.id === beforeWorkflowId);
    }
    if (insertAt < 0) {
      for (let index = without.length - 1; index >= 0; index -= 1) {
        if ((without[index].folderId ?? null) === folderId) {
          insertAt = index + 1;
          break;
        }
      }
    }
    if (insertAt < 0) insertAt = without.length;
    const next = [...without];
    next.splice(insertAt, 0, moved);
    set({ workflows: next, error: null });

    try {
      const saved = await api.moveWorkflowToFolder(workflowId, folderId);
      await api.reorderWorkflows(next.map((workflow) => workflow.id));
      set((state) => ({
        workflows: state.workflows.map((workflow) =>
          workflow.id === workflowId ? { ...workflow, ...saved } : workflow,
        ),
      }));
    } catch (e) {
      set({ workflows: previous, error: String(e) });
    }
  },

  setWorkingDirectory: async (id, workingDirectory) => {
    try {
      const workflow = await api.updateWorkflow({
        id,
        workingDirectory: workingDirectory.trim(),
      });
      set((state) => ({
        workflows: state.workflows.map((w) =>
          w.id === workflow.id ? workflow : w,
        ),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setMemoryRetrievalEnabled: async (id, enabled) => {
    await persistWorkflowPermission(
      set,
      get,
      id,
      "memoryRetrievalEnabled",
      enabled,
    );
  },

  setMemoryLookupEnabled: (id, enabled) =>
    persistWorkflowPermission(set, get, id, "memoryLookupEnabled", enabled),

  reorderWorkflows: async (orderedIds) => {
    const previous = get().workflows;
    const byId = new Map(previous.map((w) => [w.id, w]));
    const next = orderedIds
      .map((id) => byId.get(id))
      .filter((w): w is NonNullable<typeof w> => Boolean(w));
    // Keep any ids missing from the payload at the end.
    for (const w of previous) {
      if (!orderedIds.includes(w.id)) next.push(w);
    }
    set({ workflows: next });
    try {
      await api.reorderWorkflows(next.map((w) => w.id));
    } catch (e) {
      set({ workflows: previous, error: String(e) });
    }
  },

  renameWorkflow: async (id, name) => {
    const trimmed = name.trim();
    if (!trimmed) {
      set({ error: "Workflow name cannot be empty." });
      return;
    }

    set({ loading: true, error: null });
    try {
      const workflow = await api.updateWorkflow({ id, name: trimmed });
      set((state) => ({
        workflows: state.workflows.map((w) =>
          w.id === workflow.id ? { ...w, name: workflow.name } : w,
        ),
        workflowSchedules: state.workflowSchedules.map((item) =>
          item.workflowId === workflow.id
            ? { ...item, workflowName: workflow.name }
            : item,
        ),
        loading: false,
      }));
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  deleteWorkflow: async (id) => {
    set({ loading: true, error: null });
    try {
      // Best-effort: clear schedule first so delete can't fail on older DBs
      // without ON DELETE CASCADE.
      try {
        await api.deleteWorkflowSchedule(id);
      } catch {
        /* no schedule */
      }

      await api.deleteWorkflow(id);
      clearLegacyMemories(id);
      const remaining = get().workflows.filter((w) => w.id !== id);
      const openWorkflowIds = get().openWorkflowIds.filter((tabId) => tabId !== id);
      persistOpenTabs(openWorkflowIds);
      const wasActive = get().activeWorkflowId === id;

      set((state) => {
        const workflowRunStates = { ...state.workflowRunStates };
        delete workflowRunStates[id];
        return {
          workflows: remaining,
          workflowSchedules: state.workflowSchedules.filter(
            (item) => item.workflowId !== id,
          ),
          workflowRunStates,
          openWorkflowIds,
          loading: false,
          schedule: wasActive ? null : state.schedule,
        };
      });

      if (wasActive) {
        const fallback = openWorkflowIds[0] ?? remaining[0]?.id ?? null;
        if (fallback) {
          await get().selectWorkflow(fallback);
        } else {
          set({
            activeWorkflowId: null,
            nodes: [],
            edges: [],
            memories: [],
            schedule: null,
            selectedNodeId: null,
            dirty: false,
            workflowLoading: false,
            selectedOutput: null,
            ...visibleRunFields(emptyWorkflowRunState()),
          });
        }
      }
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },

  saveActiveWorkflow: async () => {
    const { activeWorkflowId, nodes, edges } = get();
    if (!activeWorkflowId) return false;

    const previousSave = workflowSaves.get(activeWorkflowId);
    set({ loading: true, error: null });
    const save = (async () => {
      if (previousSave) {
        await previousSave;
        set({ error: null });
      }
      try {
        const workflow = await api.updateWorkflow({
          id: activeWorkflowId,
          graph: { nodes: normalizeNodes(nodes), edges },
        });
        set((state) => ({
          workflows: state.workflows.map((item) =>
            item.id === activeWorkflowId
              ? {
                  ...item,
                  graph: workflow.graph,
                  updatedAt: workflow.updatedAt,
                }
              : item,
          ),
          dirty:
            state.activeWorkflowId === activeWorkflowId &&
            state.nodes === nodes &&
            state.edges === edges
              ? false
              : state.dirty,
        }));
        return true;
      } catch (e) {
        set({ error: String(e) });
        return false;
      }
    })();
    workflowSaves.set(activeWorkflowId, save);
    try {
      return await save;
    } finally {
      if (workflowSaves.get(activeWorkflowId) === save) {
        workflowSaves.delete(activeWorkflowId);
      }
      if (workflowSaves.size === 0) set({ loading: false });
    }
  },

  runActiveWorkflow: async (workflowId) => {
    const {
      activeWorkflowId,
      dirty,
      saveActiveWorkflow,
      nodes,
      workflows,
      workflowRunStates,
    } = get();
    const targetWorkflowId = workflowId ?? activeWorkflowId;
    if (!targetWorkflowId) return;
    const targetIsVisible = targetWorkflowId === activeWorkflowId;
    if (
      workflowRunStates[targetWorkflowId]?.activeRun?.status === "running"
    ) {
      if (targetIsVisible) get().openRunPanel();
      return;
    }
    if (targetIsVisible && dirty && !(await saveActiveWorkflow())) return;

    await installRunEventBridge();

    const targetNodes = targetIsVisible
      ? nodes
      : workflows.find((workflow) => workflow.id === targetWorkflowId)?.graph
          ?.nodes ?? [];
    const nextRunState = emptyWorkflowRunState(
      targetNodes,
      workflowRunStates[targetWorkflowId]?.inspectedNodeId ?? null,
    );
    set((state) => ({
      loading: true,
      error: null,
      workflowRunStates: {
        ...state.workflowRunStates,
        [targetWorkflowId]: nextRunState,
      },
      ...(state.activeWorkflowId === targetWorkflowId
        ? {
            selectedOutput: null,
            ...visibleRunFields(nextRunState),
          }
        : {}),
    }));

    try {
      const summary = await api.runWorkflow(targetWorkflowId);
      set((state) => {
        const current =
          state.workflowRunStates[targetWorkflowId] ?? nextRunState;
        const eventAlreadyArrived = current.activeRun?.id === summary.id;
        const activeRun = eventAlreadyArrived
          ? {
              ...summary,
              trigger: summary.trigger,
              status: current.activeRun!.status,
            }
          : { ...summary, status: "running" };
        const updated = { ...current, activeRun };
        return {
          workflowRunStates: {
            ...state.workflowRunStates,
            [targetWorkflowId]: updated,
          },
          loading: false,
          ...(state.activeWorkflowId === targetWorkflowId
            ? visibleRunFields(updated)
            : {}),
        };
      });
    } catch (e) {
      set((state) => ({
        loading: false,
        error: String(e),
        ...(state.activeWorkflowId === targetWorkflowId
          ? { runPanelOpen: true }
          : {}),
      }));
    }
  },

  cancelActiveRun: async (workflowId) => {
    const { activeWorkflowId, workflowRunStates } = get();
    const targetWorkflowId = workflowId ?? activeWorkflowId;
    if (!targetWorkflowId) return;
    const run = workflowRunStates[targetWorkflowId]?.activeRun;
    if (!run || run.status !== "running") return;
    try {
      await api.cancelRun(run.id);
      set((state) => {
        const current = state.workflowRunStates[targetWorkflowId];
        if (!current || current.activeRun?.id !== run.id) return {};
        const updated: WorkflowRunState = {
          ...current,
          activeRun: { ...current.activeRun, status: "cancelled" },
          activeNodeId: null,
        };
        return {
          workflowRunStates: {
            ...state.workflowRunStates,
            [targetWorkflowId]: updated,
          },
          ...(state.activeWorkflowId === targetWorkflowId
            ? visibleRunFields(updated)
            : {}),
        };
      });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  openRunPanel: (nodeId) =>
    set((state) => {
      const inspectedNodeId =
        nodeId !== undefined
          ? nodeId
          : state.runPanelOpen
            ? state.inspectedNodeId
            : state.inspectedNodeId ?? state.activeNodeId;
      const workflowId = state.activeWorkflowId;
      const current = workflowId
        ? state.workflowRunStates[workflowId]
        : undefined;

      return {
        runPanelOpen: true,
        inspectedNodeId,
        ...(workflowId && current
          ? {
              workflowRunStates: {
                ...state.workflowRunStates,
                [workflowId]: { ...current, inspectedNodeId },
              },
            }
          : {}),
      };
    }),

  closeRunPanel: () =>
    set({
      runPanelOpen: false,
      selectedOutput: null,
    }),

  openOutput: (output) =>
    set((state) => {
      const inspectedNodeId = output.nodeId ?? null;
      const workflowId = state.activeWorkflowId;
      const current = workflowId
        ? state.workflowRunStates[workflowId]
        : undefined;
      return {
        selectedOutput: output,
        runPanelOpen: true,
        inspectedNodeId,
        ...(workflowId && current
          ? {
              workflowRunStates: {
                ...state.workflowRunStates,
                [workflowId]: { ...current, inspectedNodeId },
              },
            }
          : {}),
      };
    }),

  closeOutput: () => set({ selectedOutput: null }),

  loadMemories: async (workflowId) => {
    try {
      const migrationComplete = await migrateLegacyMemories(workflowId);
      const memories = await api.listMemories(workflowId, true);
      if (get().activeWorkflowId === workflowId) {
        set((state) => ({
          memories: sortMemories(memories),
          error: !migrationComplete
            ? LEGACY_MEMORY_MIGRATION_ERROR
            : state.error === LEGACY_MEMORY_MIGRATION_ERROR
              ? null
              : state.error,
        }));
        return true;
      }
      return false;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  },

  addMemory: async (input) => {
    const workflowId = input.workflowId ?? get().activeWorkflowId;
    if (!workflowId || !input.body.trim()) return null;

    try {
      const created = await api.createMemory({
        workflowId,
        title: input.title.trim() || "Output",
        body: input.body,
        runId: input.runId ?? null,
        nodeId: input.nodeId ?? null,
        kind: input.kind,
        scopeType: input.scopeType,
        memoryType: input.memoryType,
        source: input.source ?? "run",
        pinned: input.pinned,
        confidence: input.confidence,
        salience: input.salience,
        status: input.status,
        supersedesId: input.supersedesId,
        factKey: input.factKey,
        observedAt: input.observedAt,
        lastConfirmedAt: input.lastConfirmedAt,
        validFrom: input.validFrom,
        expiresAt: input.expiresAt,
      });
      const memory = asOwnedMemory(created);
      if (get().activeWorkflowId === workflowId) {
        set((state) => ({
          memories: sortMemories([
            memory,
            ...state.memories.filter((m) => m.id !== memory.id),
          ]),
        }));
      }
      return memory;
    } catch (e) {
      set({ error: String(e) });
      return null;
    }
  },

  addNote: async (title, body) => {
    await get().addMemory({
      title: title.trim() || "Note",
      body,
      source: "manual",
      kind: "note",
    });
  },

  linkMemory: async (memoryId) => {
    const workflowId = get().activeWorkflowId;
    if (!workflowId) return null;
    try {
      const linked = await api.linkMemory(workflowId, memoryId);
      if (get().activeWorkflowId === workflowId) {
        set((state) => ({
          memories: sortMemories([
            linked,
            ...state.memories.filter((m) => m.id !== linked.id),
          ]),
        }));
      }
      return linked;
    } catch (e) {
      set({ error: String(e) });
      return null;
    }
  },

  unlinkMemory: async (memoryId) => {
    const workflowId = get().activeWorkflowId;
    if (!workflowId) return;
    try {
      await api.unlinkMemory(workflowId, memoryId);
      if (get().activeWorkflowId === workflowId) {
        set((state) => ({
          memories: state.memories.filter((m) => m.id !== memoryId),
        }));
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  updateMemoryFields: async (input) => {
    const workflowId = get().activeWorkflowId;
    try {
      const existing = get().memories.find((m) => m.id === input.id);
      if (existing?.origin === "linked") {
        set({ error: "Linked memories are read-only here. Unlink to remove." });
        return null;
      }
      if (!existing) {
        set({ error: "This memory changed or was removed. Refresh and try again." });
        return null;
      }
      const updated = asOwnedMemory({
        ...(await api.updateMemory({
          ...input,
          expectedSemanticRevision:
            input.expectedSemanticRevision ?? existing.semanticRevision,
          contextWorkflowId: workflowId ?? undefined,
        })),
        origin: existing?.origin ?? "owned",
        sourceWorkflowName: existing?.sourceWorkflowName,
      });
      if (get().activeWorkflowId === workflowId) {
        set((state) => ({
          memories: sortMemories(
            state.memories.map((m) => (m.id === updated.id ? updated : m)),
          ),
        }));
      }
      return updated;
    } catch (e) {
      if (String(e).includes("memory_changed") && workflowId) {
        await get().loadMemories(workflowId);
        set({ error: "This memory changed. Review the latest version and try again." });
        return null;
      }
      set({ error: String(e) });
      return null;
    }
  },

  togglePinMemory: async (id) => {
    const current = get().memories.find((m) => m.id === id);
    if (!current) return;
    if (current.origin === "linked") {
      set({
        error: "Pinning applies to owned memories. Use a Memories node for linked ones.",
      });
      return;
    }
    if (!canPinMemory(current)) {
      set({ error: "Only active memories can be pinned." });
      return;
    }
    await get().updateMemoryFields({ id, pinned: !current.pinned });
  },

  removeMemory: async (id) => {
    const current = get().memories.find((m) => m.id === id);
    if (!current) return;
    if (current.origin === "linked") {
      await get().unlinkMemory(id);
      return;
    }
    try {
      await api.deleteMemory(id, get().activeWorkflowId ?? undefined);
      set((state) => ({
        memories: state.memories.filter((m) => m.id !== id),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  clearMemories: async () => {
    const workflowId = get().activeWorkflowId;
    if (!workflowId) return;
    try {
      await api.clearMemories(workflowId);
      clearLegacyMemories(workflowId);
      const memories = await api.listMemories(workflowId, true);
      if (get().activeWorkflowId === workflowId) {
        set({ memories: sortMemories(memories) });
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  handleRunEvent: (event) => {
    if (event.kind === "step_failed" && event.authRequired) {
      const workflowName =
        get().workflows.find((workflow) => workflow.id === event.workflowId)
          ?.name ?? "Workflow";
      useToastStore
        .getState()
        .showAgentAuthToast(event.authRequired, workflowName);
    }

    const line = runLogLine(event);

    set((state) => {
      const workflow = state.workflows.find(
        (item) => item.id === event.workflowId,
      );
      const workflowNodes =
        state.activeWorkflowId === event.workflowId
          ? state.nodes
          : workflow?.graph?.nodes ?? [];
      const existing = state.workflowRunStates[event.workflowId];

      // A late event from an older run must never overwrite the current run.
      if (
        event.kind !== "started" &&
        existing?.activeRun?.id &&
        existing.activeRun.id !== event.runId
      ) {
        return {};
      }

      const freshRun =
        event.kind === "started" && existing?.activeRun?.id !== event.runId;
      let current = freshRun
        ? emptyWorkflowRunState(
            workflowNodes,
            existing?.inspectedNodeId ?? null,
          )
        : existing ?? emptyWorkflowRunState(workflowNodes);
      const stepStatuses = { ...current.stepStatuses };
      const stepOutputs = { ...current.stepOutputs };
      const stepStats = { ...current.stepStats };
      let activeNodeId = current.activeNodeId;
      let activeRun = current.activeRun;

      // Manual, scheduled, and triggered runs all join the same per-workflow slot.
      if (event.kind === "started" || !activeRun) {
        activeRun = {
          id: event.runId,
          workflowId: event.workflowId,
          trigger:
            current.activeRun?.id === event.runId
              ? current.activeRun.trigger
              : "schedule",
          status: "running",
          createdAt: event.at,
        };
      }

      if (event.kind === "step_started" && event.nodeId) {
        stepStatuses[event.nodeId] = "running";
        activeNodeId = event.nodeId;
      }
      if (event.kind === "step_completed" && event.nodeId) {
        stepStatuses[event.nodeId] = "completed";
        if (event.stats) stepStats[event.nodeId] = event.stats;
        if (event.output) {
          stepOutputs[event.nodeId] = event.output;
        }
      }
      if (event.kind === "step_log" && event.nodeId && event.output) {
        const previous = stepOutputs[event.nodeId] ?? "";
        stepOutputs[event.nodeId] = previous
          ? `${previous}\n${event.output}`
          : event.output;
      }
      if (event.kind === "step_failed" && event.nodeId) {
        stepStatuses[event.nodeId] = "failed";
        if (event.message) {
          stepOutputs[event.nodeId] = event.output || event.message;
        }
      }

      if (event.kind === "completed") {
        activeNodeId = null;
        if (event.output) stepOutputs.__final__ = event.output;
        activeRun = { ...activeRun, status: "completed" };
        const workflowName = workflow?.name ?? "Workflow";
        const outputBody = event.output?.trim() || "Run completed";
        queueMicrotask(() => {
          void (async () => {
            if (!(await shouldNotifyAboutRun())) return;
            await notifyRunFinished({
              workflowId: event.workflowId,
              workflowName,
              ok: true,
              title: "Final output",
              body: outputBody,
            });
          })();
        });
      }
      if (event.kind === "failed") {
        activeNodeId = null;
        activeRun = { ...activeRun, status: "failed" };
        const workflowName = workflow?.name ?? "Workflow";
        const failBody =
          event.message?.trim() || event.output?.trim() || "Run failed";
        queueMicrotask(() => {
          void (async () => {
            if (!(await shouldNotifyAboutRun())) return;
            await notifyRunFinished({
              workflowId: event.workflowId,
              workflowName,
              ok: false,
              title: "Run failed",
              body: failBody,
            });
          })();
        });
      }
      if (event.kind === "cancelled") {
        activeNodeId = null;
        activeRun = { ...activeRun, status: "cancelled" };
      }

      current = {
        activeRun,
        activeNodeId,
        inspectedNodeId: current.inspectedNodeId,
        stepStatuses,
        stepOutputs,
        stepStats,
        runLogs: nextRunLogs(current.runLogs, line, freshRun),
      };
      const visible = state.activeWorkflowId === event.workflowId;

      const workflowRunStates = { ...state.workflowRunStates };
      const terminal = ["completed", "failed", "cancelled"].includes(event.kind);
      const keepRunDetail =
        visible || state.openWorkflowIds.includes(event.workflowId) || !terminal;
      if (keepRunDetail) workflowRunStates[event.workflowId] = current;
      else delete workflowRunStates[event.workflowId];

      return {
        workflowRunStates,
        ...(visible ? visibleRunFields(current) : {}),
        ...(visible && event.kind === "step_started" && event.nodeId
          ? { selectedNodeId: event.nodeId }
          : {}),
        ...(visible && event.kind === "completed" && event.output
          ? {
              selectedOutput: {
                title: "Final output",
                body: event.output,
                nodeId: null,
              },
            }
          : {}),
      };
    });
  },

  handleOutputPersistenceEvent: (event) => {
    if (get().activeWorkflowId === event.workflowId) {
      void get().loadMemories(event.workflowId);
    }
  },

  setSelectedNodeId: (id) => set({ selectedNodeId: id }),

  updateNodeData: (nodeId, data) => {
    set((state) => {
      let changed = false;
      const nodes = state.nodes.map((node) => {
        if (node.id !== nodeId) return node;

        // A blocked Input can only be unblocked. Keeping this guard in the
        // store protects every editing surface, not only disabled controls.
        if (isPromptNodeData(node.data) && node.data.blocked) {
          if (!("blocked" in data) || data.blocked !== false) return node;
          changed = true;
          return {
            ...node,
            data: { ...node.data, blocked: false },
          };
        }

        changed = true;
        return {
          ...node,
          data: { ...node.data, ...data } as WorkflowNodeData,
        };
      });

      return changed ? { nodes, dirty: true } : state;
    });
  },

  onNodesChange: (changes) => {
    set((state) => ({
      nodes: applyNodeChanges(changes, state.nodes),
      dirty: true,
    }));
  },

  onEdgesChange: (changes) => {
    set((state) => ({
      edges: applyEdgeChanges(changes, state.edges),
      dirty: true,
    }));
  },

  onConnect: (connection) => {
    set((state) => ({
      edges: addEdge({ ...connection, type: "default" }, state.edges),
      dirty: true,
    }));
  },

  addNode: (node) => {
    set((state) => ({
      nodes: [...state.nodes, node],
      dirty: true,
      selectedNodeId: node.id,
    }));
  },

  removeNode: (nodeId) => {
    set((state) => ({
      nodes: state.nodes.filter((n) => n.id !== nodeId),
      edges: state.edges.filter(
        (e) => e.source !== nodeId && e.target !== nodeId,
      ),
      dirty: true,
      selectedNodeId:
        state.selectedNodeId === nodeId ? null : state.selectedNodeId,
    }));
  },

  duplicateNode: (nodeId) => {
    const source = get().nodes.find((n) => n.id === nodeId);
    if (!source) return;

    const copy: WorkflowNode = {
      ...source,
      id: crypto.randomUUID(),
      position: {
        x: source.position.x + 40,
        y: source.position.y + 40,
      },
      selected: false,
      data: structuredClone(source.data),
    };

    set((state) => ({
      nodes: [...state.nodes, copy],
      dirty: true,
      selectedNodeId: copy.id,
    }));
  },

  disconnectNode: (nodeId) => {
    set((state) => {
      const nextEdges = state.edges.filter(
        (e) => e.source !== nodeId && e.target !== nodeId,
      );
      if (nextEdges.length === state.edges.length) return state;
      return { edges: nextEdges, dirty: true };
    });
  },
}));
