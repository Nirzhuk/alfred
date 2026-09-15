import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { createRoot } from "react-dom/client";
import { useEffect, useState } from "react";
import "../../src/App.css";
import { applyResolvedTheme, useThemeStore } from "../../src/features/settings/theme";
import { SettingsPage } from "../../src/features/settings/components/settings-page/settings-page";
import { useMemoryReviewStore } from "../../src/features/settings/memory-review";
import { MemoriesInspector } from "../../src/features/workflow/components/memories-inspector/memories-inspector";
import { HistoryPage } from "../../src/features/workflow/components/history-page/history-page";
import { useWorkflowStore } from "../../src/features/workflow/store";
import { memoryVersionFixture, candidateVersionFixture } from "./memory-version-fixture";
import type {
  MemoryCandidate,
  MemoryReviewJob,
  MemoryReviewSettings,
  OutputMemory,
  RunHistoryDetail,
  RunHistoryItem,
  Workflow,
} from "../../src/features/workflow/types";

type Scenario =
  | "off"
  | "supported"
  | "workflow-supported"
  | "unavailable"
  | "failure"
  | "save-failure"
  | "lookup-probe-failure"
  | "lookup-unavailable"
  | "lookup-supported"
  | "lookup-save-failure"
  | "lookup-active-unavailable"
  | "lookup-switch";
type Theme = "light" | "dark";
type Screen = "settings" | "workflow" | "suggestions" | "history";

const scenarioParam = new URLSearchParams(location.search).get("scenario");
const scenarioValues: readonly Scenario[] = [
  "off",
  "supported",
  "workflow-supported",
  "unavailable",
  "failure",
  "save-failure",
  "lookup-unavailable",
  "lookup-supported",
  "lookup-probe-failure",
  "lookup-save-failure",
  "lookup-active-unavailable",
  "lookup-switch",
];
const initialScenario: Scenario = scenarioValues.includes(
  scenarioParam as Scenario,
)
  ? (scenarioParam as Scenario)
  : "off";

const screenParam = new URLSearchParams(location.search).get("screen");
const screenValues: readonly Screen[] = [
  "settings",
  "workflow",
  "suggestions",
  "history",
];
const initialScreen: Screen = screenValues.includes(screenParam as Screen)
  ? (screenParam as Screen)
  : "settings";

const timestamp = "2026-09-08T16:00:00Z";
const workflow: Workflow = {
  id: "memory-ui-workflow",
  name: "Memory review fixture",
  description: "Synthetic plan046 UI data",
  workingDirectory: "/tmp/memory-review-fixture",
  memoryRetrievalEnabled: true,
  memoryLookupEnabled: initialScenario === "lookup-active-unavailable",
  graph: { nodes: [], edges: [] },
  createdAt: timestamp,
  updatedAt: timestamp,
};

const alternateWorkflow: Workflow = {
  ...workflow,
  id: "memory-ui-workflow-secondary",
  name: "Memory lookup secondary workflow",
  memoryLookupEnabled: false,
};

function workflowsForScenario(current: Scenario): Workflow[] {
  const primary = {
    ...workflow,
    memoryLookupEnabled: current === "lookup-active-unavailable",
  };
  return current === "lookup-switch"
    ? [primary, { ...alternateWorkflow }]
    : [primary];
}

type PendingValue<T> = {
  token: number;
  promise: Promise<T>;
  resolve: (value: T) => void;
  value: T;
};

type PendingMap<T> = Map<string, PendingValue<T>[]>;

let nextPendingToken = 0;

function pendingValue<T>(
  pending: PendingMap<T>,
  id: string,
  value: T,
): PendingValue<T> {
  let resolve!: (next: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  const created = {
    token: ++nextPendingToken,
    promise,
    resolve,
    value,
  };
  const queue = pending.get(id) ?? [];
  queue.push(created);
  pending.set(id, queue);
  return created;
}

const memories: OutputMemory[] = [
  {
    ...memoryVersionFixture(),
    id: "memory-ui-existing",
    workflowId: workflow.id,
    kind: "note",
    scopeType: "workflow",
    scopeKey: workflow.id,
    scopeLabel: workflow.name,
    memoryType: "note",
    source: "manual",
    title: "Existing memory context",
    body: "Existing memory selected for the bounded review context.",
    pinned: true,
    confidence: 1,
    salience: 80,
    status: "active",
    createdAt: timestamp,
    updatedAt: timestamp,
    origin: "owned",
  },
  {
    ...memoryVersionFixture(),
    id: "memory-ui-second",
    workflowId: workflow.id,
    kind: "text",
    scopeType: "workflow",
    scopeKey: workflow.id,
    scopeLabel: workflow.name,
    memoryType: "decision",
    source: "run",
    title: "Current workflow decision",
    body: "Keep review suggestions candidate-only until explicitly approved.",
    pinned: false,
    confidence: 0.9,
    salience: 60,
    status: "active",
    createdAt: timestamp,
    updatedAt: timestamp,
    origin: "owned",
  },
];

const candidate: MemoryCandidate = {
  ...candidateVersionFixture(),
  id: "memory-ui-candidate",
  reviewRunId: "memory-ui-run",
  workflowId: workflow.id,
  sourceNodeId: "memory-ui-node",
  operation: "create",
  targetMemoryId: null,
  scopeType: "workflow",
  scopeKey: workflow.id,
  memoryType: "lesson",
  title: "Candidate suggestion",
  body: "A synthetic candidate that still requires approval.",
  confidence: 0.82,
  rationale: "The completed run contained a durable workflow decision.",
  curationContractVersion: 0,
  evidenceState: "legacy_unavailable",
  evidenceSubject: null,
  isInference: false,
  claimDigest: null,
  evidence: [],
  status: "blocked",
  blockedCode: "legacy_evidence_unavailable",
  createdAt: timestamp,
  decidedAt: null,
};

const failedJob: MemoryReviewJob = {
  runId: "memory-ui-run",
  workflowId: workflow.id,
  status: "failed",
  provider: "claude_code",
  providerProduct: "Claude Code",
  executionMode: "restricted_review",
  capabilityVersion: "2.1.265",
  restrictionContractVersion: "1",
  model: null,
  errorCode: "restricted_review_unavailable",
  candidateCount: 0,
  startedAt: timestamp,
  finishedAt: timestamp,
  createdAt: timestamp,
};

const run: RunHistoryItem = {
  id: "memory-ui-run",
  workflowId: workflow.id,
  workflowName: workflow.name,
  trigger: "manual",
  status: "completed",
  error: null,
  startedAt: timestamp,
  finishedAt: timestamp,
  createdAt: timestamp,
  stepCount: 1,
  finalOutputPreview: "Completed run with bounded memory context.",
};

const detail: RunHistoryDetail = {
  run,
  evidenceSources: [],
  steps: [
    {
      id: "memory-ui-step",
      nodeId: "memory-ui-node",
      agentProvider: "claude_code",
      skillName: "Review fixture",
      status: "completed",
      input: { prompt: "Synthetic run input" },
      output: { summary: "Synthetic run output" },
      error: null,
      startedAt: timestamp,
      finishedAt: timestamp,
      createdAt: timestamp,
    },
  ],
  memoryUses: [
    {
      nodeId: "memory-ui-node",
      memoryId: memories[0].id,
      memoryTitle: memories[0].title,
      scopeType: "workflow",
      memoryType: "note",
      rank: 1,
      score: 1,
      reason: "pinned",
      renderedBytes: 96,
      rankingVersion: "pinned_v1",
      createdAt: timestamp,
    },
  ],
  memoryContextSummaries: [
    {
      nodeId: "memory-ui-node",
      budgetBytes: 32 * 1024,
      memoryBytes: 96,
      includedCount: 1,
      explicitOmittedCount: 0,
      pinnedOmittedCount: 0,
      automaticOmittedCount: 0,
      unavailableCount: 0,
      repeatedCount: 0,
      traceAvailable: true,
      createdAt: timestamp,
    },
  ],
};
const calls: { command: string; payload: unknown }[] = [];
let scenario: Scenario = initialScenario;
let fixtureWorkflows = workflowsForScenario(initialScenario);
const lookupAvailabilityPending: PendingMap<{
  available: boolean;
  reason: string | null;
}> = new Map();
const lookupSavePending: PendingMap<Workflow> = new Map();
let workflowReviewEnabled = false;
let retried = false;
let lookupProbeAttempt = 0;

const fixtureErrors: string[] = [];
const originalConsoleError = console.error;
console.error = (...args: unknown[]) => {
  fixtureErrors.push(args.map((arg) => String(arg)).join(" "));
  originalConsoleError(...args);
};
window.addEventListener("error", (event) => {
  fixtureErrors.push(String(event.error?.stack ?? event.message));
});
window.addEventListener("unhandledrejection", (event) => {
  fixtureErrors.push(String(event.reason?.stack ?? event.reason));
});

function settingsFor(current: Scenario): MemoryReviewSettings {
  const unavailable = current === "unavailable" || current === "failure";
  const enabled = current === "workflow-supported" || current === "unavailable" || current === "failure";
  return {
    enabled,
    provider: "claude_code",
    model: null,
    maxCandidates: 5,
    updatedAt: timestamp,
    availability: {
      supported: !unavailable,
      reason: unavailable ? "installed_cli_version_not_verified" : null,
      product: "Claude Code",
      runtimeVersion: unavailable ? "2.1.265" : "2.1.263",
      executionMode: "restricted_review",
      restrictionContractVersion: "1",
    },
  };
}

function jobsFor(current: Scenario): MemoryReviewJob[] {
  if (current === "failure" || current === "unavailable") {
    return [retried ? { ...failedJob, status: "pending", errorCode: null } : failedJob];
  }
  return [];
}

function lookupAvailabilityFor(
  workflowId: string,
): { available: boolean; reason: string | null } | Promise<{
  available: boolean;
  reason: string | null;
}> {
  if (scenario === "lookup-switch") {
    return pendingValue(lookupAvailabilityPending, workflowId, {
      available:
        workflowId === fixtureWorkflows[0]?.id,
      reason: workflowId === fixtureWorkflows[0]?.id
        ? null
        : "native_runtime_unavailable",
    }).promise;
  }
  if (scenario === "lookup-probe-failure") {
    lookupProbeAttempt += 1;
    if (lookupProbeAttempt === 1) {
      throw new Error("synthetic lookup availability failure");
    }
    return { available: true, reason: null };
  }
  if (
    scenario === "lookup-supported" ||
    scenario === "lookup-save-failure"
  ) {
    return { available: true, reason: null };
  }
  if (scenario === "lookup-active-unavailable") {
    return { available: false, reason: "native_runtime_unavailable" };
  }
  return { available: false, reason: "native_runtime_unavailable" };
}

function reply(command: string, payload: any): unknown {
  calls.push({ command, payload: structuredClone(payload ?? {}) });
  switch (command) {
    case "get_memory_review_settings":
      return settingsFor(scenario);
    case "update_memory_review_settings":
      if (scenario === "save-failure") throw new Error("synthetic save failure");
      return {
        ...settingsFor(scenario),
        enabled: Boolean(payload?.input?.enabled),
        provider: payload?.input?.provider ?? null,
        model: payload?.input?.model ?? null,
      };
    case "list_agent_providers":
      return [{ id: "claude_code", label: "Claude Code", defaultModel: null }];
    case "list_agent_models":
      return [];
    case "get_agent_capability_manifest":
      return null;
    case "get_workflow_memory_review":
      return {
        workflowId: payload?.workflowId ?? workflow.id,
        enabled: workflowReviewEnabled,
        updatedAt: timestamp,
      };
    case "set_workflow_memory_review":
      workflowReviewEnabled = Boolean(payload?.enabled);
      return {
        workflowId: payload?.workflowId ?? workflow.id,
        enabled: workflowReviewEnabled,
        updatedAt: timestamp,
      };
    case "get_memory_lookup_availability":
      return lookupAvailabilityFor(payload?.workflowId ?? workflow.id);
    case "list_memories":
      return memories;
    case "list_memory_candidates":
      return scenario === "workflow-supported" ? [candidate] : [];
    case "list_memory_evidence":
      return [];
    case "list_memory_reviews":
      return jobsFor(scenario);
    case "get_memory_review_job":
      return scenario === "failure" || scenario === "unavailable" ? jobsFor(scenario)[0] : null;
    case "retry_memory_review":
      retried = true;
      return { ...failedJob, status: "pending", errorCode: null };
    case "list_run_history":
      return [run];
    case "get_run_history":
      return detail;
    case "update_workflow": {
      const input = (payload?.input ?? {}) as {
        id?: string;
        memoryLookupEnabled?: boolean;
        [key: string]: unknown;
      };
      const id = input.id ?? workflow.id;
      const current =
        fixtureWorkflows.find((item) => item.id === id) ?? workflow;
      if (
        scenario === "lookup-save-failure" &&
        input.memoryLookupEnabled !== undefined
      ) {
        throw new Error("synthetic lookup save failure");
      }
      const updated = { ...current, ...input } as Workflow;
      if (
        scenario === "lookup-switch" &&
        input.memoryLookupEnabled !== undefined
      ) {
        return pendingValue(
          lookupSavePending,
          id,
          structuredClone(updated),
        ).promise;
      }
      fixtureWorkflows = fixtureWorkflows.map((item) =>
        item.id === id ? updated : item,
      );
      return structuredClone(updated);
    }
    case "list_workflow_folders":
    case "list_schedules":
    case "list_active_runs":
      return [];
    case "notification_permission":
      return "granted";
    default:
      if (command.startsWith("plugin:")) return null;
      throw new Error(`Unmocked memory UI fixture IPC: ${command}`);
  }
}
mockWindows("main");
mockIPC(reply, { shouldMockEvents: true });

useWorkflowStore.setState({
  workflows: fixtureWorkflows,
  activeWorkflowId: workflow.id,
  openWorkflowIds: [workflow.id],
  nodes: [],
  edges: [],
  memories,
  providerModels: [],
  loading: false,
  workflowLoading: false,
  error: null,
});

function FixtureNavigation({
  screen,
  scenarioValue,
  theme,
  onScreen,
  onScenario,
  onTheme,
}: {
  screen: Screen;
  scenarioValue: Scenario;
  theme: Theme;
  onScreen: (value: Screen) => void;
  onScenario: (value: Scenario) => void;
  onTheme: (value: Theme) => void;
}) {
  return (
    <nav className="memory-ui-fixture-nav" aria-label="Memory review fixture controls">
      <label>
        Scenario
        <select
          aria-label="Fixture scenario"
          value={scenarioValue}
          onChange={(event) => onScenario(event.currentTarget.value as Scenario)}
        >
          <option value="off">Off</option>
          <option value="supported">Supported setup</option>
          <option value="workflow-supported">Workflow supported</option>
          <option value="unavailable">Unavailable version</option>
          <option value="failure">Failed review</option>
          <option value="save-failure">Settings save failure</option>
          <option value="lookup-unavailable">
            Lookup unavailable (default off)
          </option>
          <option value="lookup-supported">Lookup fake-supported</option>
          <option value="lookup-probe-failure">
            Lookup availability probe failure
          </option>
          <option value="lookup-save-failure">Lookup save failure</option>
          <option value="lookup-active-unavailable">
            Lookup active, then unavailable
          </option>
          <option value="lookup-switch">Lookup workflow switch</option>
        </select>
      </label>
      <label>
        Theme
        <select
          aria-label="Fixture theme"
          value={theme}
          onChange={(event) => onTheme(event.currentTarget.value as Theme)}
        >
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
      </label>
      <button type="button" aria-pressed={screen === "settings"} onClick={() => onScreen("settings")}>
        Global settings
      </button>
      {scenarioValue === "lookup-switch" ? (
        <button
          type="button"
          data-testid="switch-lookup-workflow"
          onClick={() => {
            switchLookupWorkflow();
          }}
        >
          Switch lookup workflow
        </button>
      ) : null}
      <button type="button" aria-pressed={screen === "workflow"} onClick={() => onScreen("workflow")}>
        Workflow memory UI
      </button>
      <button type="button" aria-pressed={screen === "suggestions"} onClick={() => onScreen("suggestions")}>
        Suggestions queue
      </button>
      <button type="button" aria-pressed={screen === "history"} onClick={() => onScreen("history")}>
        History
      </button>
    </nav>
  );
}

function applyScenarioState(next: Scenario): void {
  scenario = next;
  fixtureWorkflows = workflowsForScenario(next);
  lookupAvailabilityPending.clear();
  lookupSavePending.clear();
  useWorkflowStore.setState({
    workflows: fixtureWorkflows,
    activeWorkflowId: fixtureWorkflows[0]?.id ?? null,
    error: null,
  });
  workflowReviewEnabled = false;
  retried = false;
  lookupProbeAttempt = 0;
}

function MemoryUiFixture() {
  const [screen, setScreen] = useState<Screen>(initialScreen);
  const [currentScenario, setCurrentScenario] =
    useState<Scenario>(initialScenario);
  const [theme, setTheme] = useState<Theme>("dark");


  useEffect(() => {
    applyResolvedTheme(theme);
    useThemeStore.setState({ preference: theme, resolved: theme });
  }, [theme]);

  const changeScenario = (next: Scenario) => {
    applyScenarioState(next);
    setCurrentScenario(next);
    useMemoryReviewStore.setState({
      settings: settingsFor(next),
      providers: [{ id: "claude_code", label: "Claude Code" }],
      loaded: false,
      loading: false,
      saving: false,
      error: null,
    });
  };

  useEffect(() => {
    const handleScenarioChange = () => {
      const next = scenario;
      applyScenarioState(next);
      setCurrentScenario(next);
      useMemoryReviewStore.setState({
        settings: settingsFor(next),
        providers: [{ id: "claude_code", label: "Claude Code" }],
        loaded: false,
        loading: false,
        saving: false,
        error: null,
      });
    };
    window.addEventListener("memory-fixture-scenario", handleScenarioChange);
    return () =>
      window.removeEventListener(
        "memory-fixture-scenario",
        handleScenarioChange,
      );
  }, []);

  return (
    <>
      <FixtureNavigation
        screen={screen}
        scenarioValue={currentScenario}
        theme={theme}
        onScreen={setScreen}
        onScenario={changeScenario}
        onTheme={setTheme}
      />
      <main className="memory-ui-fixture-content">
        {screen === "settings" ? (
          <SettingsPage key={`settings-${currentScenario}`} activeSection="memory-review" />
        ) : null}
        {screen === "workflow" || screen === "suggestions" ? (
          <MemoriesInspector
            key={`${screen}-${currentScenario}`}
            open
            initialMode={screen === "suggestions" ? "suggestions" : "memories"}
            onOpenRunHistory={() => setScreen("history")}
            onClose={() => setScreen("settings")}
          />
        ) : null}
        {screen === "history" ? (
          <HistoryPage
            activeWorkflowId={workflow.id}
            onSelectedRunIdChange={() => {}}
            onClose={() => setScreen("settings")}
          />
        ) : null}
      </main>
    </>
  );
}

function takePending<T>(
  pending: PendingMap<T>,
  workflowId: string,
  token?: number,
): PendingValue<T> | null {
  const queue = pending.get(workflowId);
  if (!queue) return null;
  const index =
    token === undefined
      ? 0
      : queue.findIndex((request) => request.token === token);
  if (index < 0) return null;
  const [request] = queue.splice(index, 1);
  if (queue.length === 0) pending.delete(workflowId);
  return request ?? null;
}

function resolveLookupAvailability(
  workflowId: string,
  result: { available: boolean; reason: string | null },
  token?: number,
): boolean {
  const pending = takePending(lookupAvailabilityPending, workflowId, token);
  if (!pending) return false;
  pending.resolve(result);
  return true;
}

function resolveLookupSave(workflowId: string, token?: number): boolean {
  const pending = takePending(lookupSavePending, workflowId, token);
  if (!pending) return false;
  fixtureWorkflows = fixtureWorkflows.map((item) =>
    item.id === workflowId ? pending.value : item,
  );
  pending.resolve(structuredClone(pending.value));
  return true;
}

function switchLookupWorkflow(): string | null {
  const current = useWorkflowStore.getState().activeWorkflowId;
  const next =
    current === fixtureWorkflows[0]?.id
      ? fixtureWorkflows[1]?.id
      : fixtureWorkflows[0]?.id;
  if (next) useWorkflowStore.setState({ activeWorkflowId: next });
  return next ?? null;
}

const root = createRoot(document.getElementById("root")!);
root.render(<MemoryUiFixture />);

export const memoryFixture = {
  scenarios: scenarioValues,
  calls,
  errors: fixtureErrors,
  snapshot: () => ({
    scenario,
    activeWorkflowId: useWorkflowStore.getState().activeWorkflowId,
    workflows: useWorkflowStore.getState().workflows.map((item) => ({
      id: item.id,
      memoryLookupEnabled: item.memoryLookupEnabled,
    })),
    lookupAvailabilityPending: [...lookupAvailabilityPending.keys()],
    lookupAvailabilityRequests: [...lookupAvailabilityPending.entries()].flatMap(
      ([workflowId, queue]) =>
        queue.map(({ token }) => ({ workflowId, token })),
    ),
    lookupSavePending: [...lookupSavePending.keys()],
    lookupSaveRequests: [...lookupSavePending.entries()].flatMap(
      ([workflowId, queue]) =>
        queue.map(({ token }) => ({ workflowId, token })),
    ),
  }),
  switchLookupWorkflow,
  resolveLookupAvailability,
  resolveLookupSave,
  setScenario: (next: Scenario) => {
    scenario = next;
    window.dispatchEvent(new Event("memory-fixture-scenario"));
  },
};
(window as typeof window & { memoryFixture?: typeof memoryFixture }).memoryFixture = memoryFixture;
