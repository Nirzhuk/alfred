import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useWorkflowStore } from "./store";
import { resolveCapability } from "../licensing";

/**
 * Saved-workflow safety for the pro perks (Plan 008 Step 4): a workflow that
 * already uses schedules or triggers must load completely under a locked
 * licence — nothing dropped, nothing silently disabled, and no mutation
 * issued to persisted data while merely loading.
 */

let mutations: string[] = [];
let resolutions: string[] = [];
let resolveFailure: string | null = null;
let savedScheduleInputs: unknown[] = [];

const WORKFLOW = {
  id: "wf-1",
  name: "Nightly report",
  graph: { nodes: [], edges: [] },
};

const SAVED_SCHEDULE = {
  id: "sched-1",
  workflowId: "wf-1",
  cron: "0 0 7 * * *",
  enabled: true,
  nextRunAt: null,
  onMissed: "run",
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

const SAVED_TRIGGERS = [
  {
    id: "trg-file",
    workflowId: "wf-1",
    source: "file",
    label: "Repo saves",
    config: { path: "/tmp/repo", pattern: "", debounceMs: 2000 },
    secret: null,
    enabled: true,
    lastFiredAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
  {
    id: "trg-hook",
    workflowId: "wf-1",
    source: "webhook",
    label: "Local hook",
    config: {},
    secret: "tok",
    enabled: true,
    lastFiredAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
];

const MISSED_RUNS = [
  {
    scheduleId: "sched-morning",
    workflowId: "wf-1",
    workflowName: WORKFLOW.name,
    cron: "0 0 7 * * *",
    missedAt: "2026-09-14T07:00:00Z",
  },
  {
    scheduleId: "sched-hourly",
    workflowId: "wf-2",
    workflowName: "Inbox sweep",
    cron: "0 0 * * * *",
    missedAt: "2026-09-14T11:00:00Z",
  },
];

function scheduleRow() {
  return { ...SAVED_SCHEDULE, workflowName: WORKFLOW.name };
}

let originalWindow: PropertyDescriptor | undefined;
beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { crypto: globalThis.crypto },
  });
  mockIPC((command, args) => {
    const payload = args as Record<string, unknown>;
    switch (command) {
      case "list_workflows": return [WORKFLOW];
      case "list_workflow_folders": return [];
      case "list_schedules": return [scheduleRow()];
      case "list_missed_schedules": return MISSED_RUNS;
      case "resolve_missed_schedule":
        if (resolveFailure) throw new Error(resolveFailure);
        resolutions.push(`${payload.decision}:${payload.scheduleId}`);
        return;
      case "list_active_runs": return [];
      case "get_workflow": return WORKFLOW;
      case "get_workflow_schedule": return SAVED_SCHEDULE;
      case "list_workflow_triggers":
        return payload.workflowId === "wf-1" ? SAVED_TRIGGERS : [];
      case "webhook_base_url": return "http://127.0.0.1:8787";
      case "list_app_trigger_statuses":
      case "list_memories": return [];
      case "upsert_workflow_schedule":
        mutations.push("upsertWorkflowSchedule");
        savedScheduleInputs.push(payload);
        return SAVED_SCHEDULE;
      case "delete_workflow_schedule":
        mutations.push("deleteWorkflowSchedule");
        return;
      case "upsert_workflow_trigger":
        mutations.push("upsertWorkflowTrigger");
        return null;
      case "delete_workflow_trigger":
        mutations.push("deleteWorkflowTrigger");
        return;
      default: throw new Error(`Unexpected command: ${command}`);
    }
  });
});

afterEach(() => {
  clearMocks();
  if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow);
  else Reflect.deleteProperty(globalThis, "window");
});


describe("loading a workflow that uses pro capabilities under a locked licence", () => {
  test("the distribution-no-license cell is actually reachable", () => {
    const decision = resolveCapability(
      {
        buildKind: "distribution",
        licenseState: "unlicensed",
        inWindow: false,
      },
      "schedules",
    );
    expect(decision).toEqual({ available: false, reason: "noLicense" });
    expect(
      resolveCapability(
        { buildKind: "distribution", licenseState: "unlicensed", inWindow: false },
        "triggers",
      ).available,
    ).toBe(false);
  });

  test("load keeps every saved schedule and trigger intact, mutating nothing", async () => {
    await useWorkflowStore.getState().loadWorkflows();

    const state = useWorkflowStore.getState();
    expect(state.error).toBeNull();
    expect(state.workflows.map((workflow) => workflow.id)).toEqual(["wf-1"]);

    // The full schedule row survived load, still enabled, untouched.
    expect(state.workflowSchedules).toHaveLength(1);
    expect(state.workflowSchedules[0]).toMatchObject({
      workflowId: "wf-1",
      cron: "0 0 7 * * *",
      enabled: true,
    });

    // selectWorkflow ran inside loadWorkflows, so triggers loaded too.
    expect(state.activeWorkflowId).toBe("wf-1");
    expect(state.triggers.map((trigger) => trigger.id)).toEqual([
      "trg-file",
      "trg-hook",
    ]);
    expect(state.triggers.every((trigger) => trigger.enabled)).toBe(true);

    // Loading only ever read; no persisted schedule or trigger was rewritten,
    // cleared, or toggled behind our back.
    expect(mutations).toEqual([]);
  });

  test("re-loading under an explicitly locked decision changes nothing stored", async () => {
    // The resolver says both capabilities are locked on this hypothetical
    // build; the store has no licence input at all and must behave the same.
    const locked = resolveCapability(
      { buildKind: "distribution", licenseState: "unlicensed", inWindow: false },
      "schedules",
    );
    expect(locked.available).toBe(false);

    await useWorkflowStore.getState().loadSchedule("wf-1");
    await useWorkflowStore.getState().loadTriggers("wf-1");

    const state = useWorkflowStore.getState();
    expect(state.schedule).toMatchObject({ cron: "0 0 7 * * *", enabled: true });
    expect(state.triggers).toHaveLength(2);
    expect(mutations).toEqual([]);
  });
});

/**
 * Missed scheduled runs: Alfred was closed when the schedule was due and the
 * schedule is set to ask. Each answer must reach the backend for its own
 * schedule, and a rejected answer must leave that schedule waiting.
 */
describe("missed scheduled runs waiting on the user", () => {
  beforeEach(() => {
    resolutions = [];
    resolveFailure = null;
    savedScheduleInputs = [];
    useWorkflowStore.setState({ missedSchedules: [], error: null });
  });

  test("the open-time prompt lists every run waiting on an answer", async () => {
    await useWorkflowStore.getState().loadMissedSchedules();
    expect(useWorkflowStore.getState().missedSchedules).toEqual(MISSED_RUNS);
  });

  test("answering one schedule decides only that schedule", async () => {
    await useWorkflowStore.getState().loadMissedSchedules();
    await useWorkflowStore
      .getState()
      .resolveMissedSchedule("sched-hourly", "skip");

    expect(resolutions).toEqual(["skip:sched-hourly"]);
    expect(
      useWorkflowStore.getState().missedSchedules.map((item) => item.scheduleId),
    ).toEqual(["sched-morning"]);
  });

  test("a rejected answer keeps the schedule listed and reports the error", async () => {
    await useWorkflowStore.getState().loadMissedSchedules();
    resolveFailure = "This workflow is already running";
    const error = await useWorkflowStore
      .getState()
      .resolveMissedSchedule("sched-morning", "run");

    expect(error).toContain("already running");
    expect(
      useWorkflowStore.getState().missedSchedules.map((item) => item.scheduleId),
    ).toEqual(["sched-morning", "sched-hourly"]);
  });

  test("saving a schedule stores the missed-run choice the user picked", async () => {
    await useWorkflowStore.getState().saveSchedule({
      workflowId: "wf-1",
      cron: "0 0 7 * * *",
      enabled: true,
      onMissed: "ask",
    });

    expect(savedScheduleInputs).toEqual([
      {
        workflowId: "wf-1",
        cron: "0 0 7 * * *",
        enabled: true,
        onMissed: "ask",
      },
    ]);
  });
});
