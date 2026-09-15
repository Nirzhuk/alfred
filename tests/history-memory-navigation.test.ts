import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { navigateWorkflowMemory } from "../src/features/workflow/components/history-page/history-format";
import { useWorkflowStore } from "../src/features/workflow/store";
import type { Workflow } from "../src/features/workflow/types";

function workflow(id: string): Workflow {
  return {
    id,
    name: id,
    description: "",
    workingDirectory: "/tmp/alfred-navigation-test",
    memoryRetrievalEnabled: false,
    memoryLookupEnabled: false,
    graph: { nodes: [], edges: [] },
    createdAt: "2026-09-10T12:00:00Z",
    updatedAt: "2026-09-10T12:00:00Z",
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

let originalWindow: PropertyDescriptor | undefined;
let originalStorage: PropertyDescriptor | undefined;

beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  originalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { crypto: globalThis.crypto },
  });
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: () => null,
      setItem: () => {},
      removeItem: () => {},
    },
  });
  useWorkflowStore.setState(useWorkflowStore.getInitialState(), true);
});

afterEach(() => {
  clearMocks();
  for (const [key, descriptor] of [
    ["window", originalWindow],
    ["localStorage", originalStorage],
  ] as const) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor);
    else Reflect.deleteProperty(globalThis, key);
  }
});

describe("workflow-aware memory navigation", () => {
  test("loads the target workflow before opening its canonical memory", async () => {
    let activeWorkflowId = "workflow-current";
    const calls: string[] = [];

    const opened = await navigateWorkflowMemory(
      { memoryId: "memory-1", workflowId: "workflow-saved" },
      {
        activeWorkflowId: () => activeWorkflowId,
        loadWorkflowContext: async (workflowId) => {
          calls.push(`load:${workflowId}`);
          activeWorkflowId = workflowId;
          return true;
        },
        isCurrent: () => true,
        openMemory: (memoryId) => calls.push(`open:${memoryId}`),
      },
    );

    expect(opened).toBe(true);
    expect(calls).toEqual([
      "load:workflow-saved",
      "open:memory-1",
    ]);
  });

  test("a newer navigation prevents a delayed workflow selection from opening", async () => {
    let activeWorkflowId = "workflow-current";
    let current = true;
    let finishSelection: (() => void) | undefined;
    const opened: Array<string | null> = [];
    const navigation = navigateWorkflowMemory(
      { memoryId: "memory-old", workflowId: "workflow-old" },
      {
        activeWorkflowId: () => activeWorkflowId,
        loadWorkflowContext: (workflowId) =>
          new Promise<boolean>((resolve) => {
            finishSelection = () => {
              activeWorkflowId = workflowId;
              resolve(true);
            };
          }),
        isCurrent: () => current,
        openMemory: (memoryId) => opened.push(memoryId),
      },
    );

    await Promise.resolve();
    current = false;
    finishSelection?.();

    expect(await navigation).toBe(false);
    expect(opened).toEqual([]);
  });

  test("does not open when workflow loading fails or activates another workflow", async () => {
    const opened: Array<string | null> = [];
    expect(
      await navigateWorkflowMemory(
        { memoryId: "memory-1", workflowId: "workflow-missing" },
        {
          activeWorkflowId: () => "workflow-current",
          loadWorkflowContext: async () => {
            throw new Error("load_failed");
          },
          isCurrent: () => true,
          openMemory: (memoryId) => opened.push(memoryId),
        },
      ),
    ).toBe(false);
    expect(
      await navigateWorkflowMemory(
        { memoryId: "memory-2", workflowId: "workflow-missing" },
        {
          activeWorkflowId: () => "workflow-current",
          loadWorkflowContext: async () => true,
          isCurrent: () => true,
          openMemory: (memoryId) => opened.push(memoryId),
        },
      ),
    ).toBe(false);
    expect(opened).toEqual([]);
  });

  test("keeps existing no-workflow events on the active context", async () => {
    let selected = false;
    const opened: Array<string | null> = [];
    const result = await navigateWorkflowMemory(
      { memoryId: "memory-current" },
      {
        activeWorkflowId: () => "workflow-current",
        loadWorkflowContext: async () => {
          selected = true;
          return true;
        },
        isCurrent: () => true,
        openMemory: (memoryId) => opened.push(memoryId),
      },
    );

    expect(result).toBe(true);
    expect(selected).toBe(false);
    expect(opened).toEqual(["memory-current"]);
  });

  test("waits for the real active-workflow memory load before focus", async () => {
    const target = workflow("workflow-target");
    useWorkflowStore.setState({
      workflows: [target],
      activeWorkflowId: target.id,
    });
    const pendingMemories = deferred<[]>();
    mockIPC((command) => {
      if (command === "list_memories") return pendingMemories.promise;
      throw new Error(`Unexpected command: ${command}`);
    });
    const opened: Array<string | null> = [];
    const navigation = navigateWorkflowMemory(
      { memoryId: "memory-target", workflowId: target.id },
      {
        activeWorkflowId: () =>
          useWorkflowStore.getState().activeWorkflowId,
        loadWorkflowContext: (workflowId) =>
          useWorkflowStore
            .getState()
            .ensureWorkflowMemoryContext(workflowId),
        isCurrent: () => true,
        openMemory: (memoryId) => opened.push(memoryId),
      },
    );

    await Promise.resolve();
    expect(opened).toEqual([]);
    pendingMemories.resolve([]);
    expect(await navigation).toBe(true);
    expect(opened).toEqual(["memory-target"]);
  });

  test("does not focus after real workflow selection when memory loading fails", async () => {
    const current = workflow("workflow-current");
    const target = workflow("workflow-target");
    useWorkflowStore.setState({
      workflows: [current, target],
      activeWorkflowId: current.id,
    });
    mockIPC((command) => {
      if (command === "get_workflow") return target;
      if (command === "get_workflow_schedule") return null;
      if (command === "list_workflow" ) return [];
      if (command === "list_workflow_triggers") return [];
      if (command === "webhook_base_url") return null;
      if (command === "list_app_trigger_statuses") return [];
      if (command === "list_memories") {
        throw new Error("memory_context_unavailable");
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    const opened: Array<string | null> = [];
    const result = await navigateWorkflowMemory(
      { memoryId: "memory-target", workflowId: target.id },
      {
        activeWorkflowId: () =>
          useWorkflowStore.getState().activeWorkflowId,
        loadWorkflowContext: (workflowId) =>
          useWorkflowStore
            .getState()
            .ensureWorkflowMemoryContext(workflowId),
        isCurrent: () => true,
        openMemory: (memoryId) => opened.push(memoryId),
      },
    );

    expect(useWorkflowStore.getState().activeWorkflowId).toBe(target.id);
    expect(result).toBe(false);
    expect(opened).toEqual([]);
    expect(useWorkflowStore.getState().error).toContain(
      "memory_context_unavailable",
    );
  });
});
