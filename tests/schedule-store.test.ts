import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useWorkflowStore } from "../src/features/workflow/store";
import type {
  Schedule,
  ScheduleLayout,
  Workflow,
} from "../src/features/workflow/types";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function workflow(id: string): Workflow {
  return {
    id,
    name: id,
    description: "",
    workingDirectory: "",
    memoryLookupEnabled: false,
    graph: { nodes: [], edges: [] },
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

function schedule(overrides: Partial<Schedule> = {}): Schedule {
  return {
    id: "schedule-1",
    workflowId: "workflow-1",
    cron: "0 0 9 * * 1-5",
    enabled: true,
    nextRunAt: "2026-09-10T09:00:00Z",
    onMissed: "run",
    createdAt: "2026-09-01T00:00:00Z",
    updatedAt: "2026-09-01T00:00:00Z",
    ...overrides,
  };
}

let originalWindow: PropertyDescriptor | undefined;

beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { crypto: globalThis.crypto },
  });
  useWorkflowStore.setState(
    {
      ...useWorkflowStore.getInitialState(),
      activeWorkflowId: "workflow-1",
      workflows: [workflow("workflow-1"), workflow("workflow-2")],
      schedule: schedule(),
      scheduleLayout: null,
    },
    true,
  );
});

afterEach(() => {
  clearMocks();
  if (originalWindow) {
    Object.defineProperty(globalThis, "window", originalWindow);
  } else {
    Reflect.deleteProperty(globalThis, "window");
  }
});

describe("workflow-scoped schedule loading", () => {
  test("a slow read never replaces the schedule of another workflow", async () => {
    const slow = deferred<Schedule | null>();
    mockIPC((command, args) => {
      if (command === "get_workflow_schedule") {
        const requested = (args as { workflowId: string }).workflowId;
        return requested === "workflow-2" ? slow.promise : schedule();
      }
      // workflow-2 has no saved layout.
      return null;
    });

    // The user switches to workflow-2, whose read is still in flight...
    useWorkflowStore.setState({ activeWorkflowId: "workflow-2", schedule: null });
    const pending = useWorkflowStore.getState().loadSchedule("workflow-2");
    // ...switches back to workflow-1, whose read answers first...
    useWorkflowStore.setState({ activeWorkflowId: "workflow-1" });
    await useWorkflowStore.getState().loadSchedule("workflow-1");
    expect(useWorkflowStore.getState().schedule?.workflowId).toBe("workflow-1");

    // ...and only then does the abandoned read answer.
    slow.resolve(schedule({ id: "schedule-2", workflowId: "workflow-2" }));
    await pending;

    expect(useWorkflowStore.getState().schedule?.workflowId).toBe("workflow-1");
    expect(useWorkflowStore.getState().schedule?.id).toBe("schedule-1");
  });

  test("a layout read is scoped to the schedule it was asked for", async () => {
    const layout: ScheduleLayout = {
      scheduleId: "schedule-1",
      x: -320,
      y: 64,
      updatedAt: "2026-09-02T00:00:00Z",
    };
    mockIPC((command) => {
      if (command === "get_workflow_schedule") return schedule();
      if (command === "get_schedule_layout") return layout;
      return null;
    });

    await useWorkflowStore.getState().loadSchedule("workflow-1");
    expect(useWorkflowStore.getState().scheduleLayout).toEqual(layout);
  });

  test("a failed read clears the node for the workflow on screen", async () => {
    mockIPC((command) => {
      if (command === "get_workflow_schedule") throw new Error("database busy");
      return null;
    });

    await useWorkflowStore.getState().loadSchedule("workflow-1");
    expect(useWorkflowStore.getState().schedule).toBeNull();
    expect(useWorkflowStore.getState().scheduleLayout).toBeNull();
  });
});

describe("enabled-only toggling", () => {
  test("sends the row as rendered and applies the persisted answer", async () => {
    let sent: Record<string, unknown> | null = null;
    mockIPC((command, args) => {
      if (command === "set_schedule_enabled") {
        sent = args as Record<string, unknown>;
        return schedule({ enabled: false, nextRunAt: null });
      }
      return schedule();
    });

    await useWorkflowStore.getState().setScheduleEnabled({
      workflowId: "workflow-1",
      scheduleId: "schedule-1",
      enabled: false,
    });

    expect(sent).toEqual({
      scheduleId: "schedule-1",
      enabled: false,
      guard: { cron: "0 0 9 * * 1-5", enabled: true },
    });
    expect(useWorkflowStore.getState().schedule?.enabled).toBe(false);
    expect(useWorkflowStore.getState().schedule?.nextRunAt).toBeNull();
    expect(useWorkflowStore.getState().scheduleCanvasError).toBeNull();
  });

  test("a rejected toggle reports at the node and re-reads the persisted row", async () => {
    let reads = 0;
    mockIPC((command) => {
      if (command === "set_schedule_enabled") {
        throw "schedule changed while it was being updated — reopen it and try again";
      }
      if (command === "get_workflow_schedule") {
        reads += 1;
        // The database still holds the enabled row the toggle tried to change.
        return schedule();
      }
      return null;
    });

    await useWorkflowStore.getState().setScheduleEnabled({
      workflowId: "workflow-1",
      scheduleId: "schedule-1",
      enabled: false,
    });

    expect(reads).toBe(1);
    expect(useWorkflowStore.getState().schedule?.enabled).toBe(true);
    expect(useWorkflowStore.getState().scheduleCanvasError).toContain(
      "schedule changed while it was being updated",
    );
  });

  test("a second click during the first write is ignored", async () => {
    const write = deferred<Schedule>();
    let writes = 0;
    mockIPC((command) => {
      if (command === "set_schedule_enabled") {
        writes += 1;
        return write.promise;
      }
      return null;
    });

    const first = useWorkflowStore.getState().setScheduleEnabled({
      workflowId: "workflow-1",
      scheduleId: "schedule-1",
      enabled: false,
    });
    await useWorkflowStore.getState().setScheduleEnabled({
      workflowId: "workflow-1",
      scheduleId: "schedule-1",
      enabled: true,
    });
    expect(writes).toBe(1);
    expect(useWorkflowStore.getState().scheduleTogglePending).toBe(true);

    write.resolve(schedule({ enabled: false, nextRunAt: null }));
    await first;
    expect(useWorkflowStore.getState().scheduleTogglePending).toBe(false);
  });

  test("a toggle for a schedule that is no longer on screen writes nothing", async () => {
    let writes = 0;
    mockIPC((command) => {
      if (command === "set_schedule_enabled") writes += 1;
      return null;
    });

    await useWorkflowStore.getState().setScheduleEnabled({
      workflowId: "workflow-1",
      scheduleId: "schedule-removed",
      enabled: false,
    });

    expect(writes).toBe(0);
  });
});

describe("schedule canvas layout", () => {
  test("persists a dragged position and keeps it in state", async () => {
    let sent: Record<string, unknown> | null = null;
    mockIPC((command, args) => {
      if (command === "set_schedule_layout") {
        sent = args as Record<string, unknown>;
        return {
          scheduleId: "schedule-1",
          x: (args as { x: number }).x,
          y: (args as { y: number }).y,
          updatedAt: "2026-09-03T00:00:00Z",
        };
      }
      return null;
    });

    await useWorkflowStore
      .getState()
      .saveScheduleLayout("schedule-1", { x: 12.4, y: -3.6 });

    expect(sent).toEqual({ scheduleId: "schedule-1", x: 16, y: 0 });
    expect(useWorkflowStore.getState().scheduleLayout?.x).toBe(16);
  });

  test("a rejected position returns to the saved one", async () => {
    let reads = 0;
    mockIPC((command) => {
      if (command === "set_schedule_layout") {
        throw "schedule not found: schedule-1";
      }
      if (command === "get_workflow_schedule") {
        reads += 1;
        return schedule();
      }
      if (command === "get_schedule_layout") {
        return {
          scheduleId: "schedule-1",
          x: -320,
          y: 0,
          updatedAt: "2026-09-02T00:00:00Z",
        };
      }
      return null;
    });

    useWorkflowStore.getState().moveScheduleNode("schedule-1", { x: 900, y: 900 });
    await useWorkflowStore
      .getState()
      .saveScheduleLayout("schedule-1", { x: 900, y: 900 });

    expect(reads).toBe(1);
    expect(useWorkflowStore.getState().scheduleLayout?.x).toBe(-320);
    expect(useWorkflowStore.getState().scheduleCanvasError).toContain(
      "schedule not found",
    );
  });

  test("moving the node leaves the schedule itself untouched", () => {
    const before = useWorkflowStore.getState().schedule;
    useWorkflowStore.getState().moveScheduleNode("schedule-1", { x: 40, y: 40 });
    expect(useWorkflowStore.getState().schedule).toBe(before);
    expect(useWorkflowStore.getState().scheduleLayout).toEqual({
      scheduleId: "schedule-1",
      x: 40,
      y: 40,
      updatedAt: "",
    });
  });
});

describe("cron preview", () => {
  test("keeps the backend's answer keyed by the expression it answered", async () => {
    mockIPC((command, args) => {
      expect(command).toBe("preview_schedule_next_run");
      return `${(args as { cron: string }).cron} → live`;
    });

    await useWorkflowStore.getState().previewScheduleCron("0 0 7 * * 1-5");
    expect(useWorkflowStore.getState().schedulePreview).toEqual({
      cron: "0 0 7 * * 1-5",
      nextRunAt: "0 0 7 * * 1-5 → live",
      error: null,
    });

    // The same expression is not asked twice.
    let calls = 0;
    mockIPC(() => {
      calls += 1;
      return "later";
    });
    await useWorkflowStore.getState().previewScheduleCron(" 0 0 7 * * 1-5 ");
    expect(calls).toBe(0);
  });

  test("records the parser's refusal instead of inventing a next run", async () => {
    mockIPC(() => {
      throw "invalid cron `nope`: expected 6 fields";
    });

    await useWorkflowStore.getState().previewScheduleCron("nope");
    const preview = useWorkflowStore.getState().schedulePreview;
    expect(preview?.cron).toBe("nope");
    expect(preview?.nextRunAt).toBeNull();
    expect(preview?.error).toContain("invalid cron");
  });
});
