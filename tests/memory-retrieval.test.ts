import { afterEach, beforeEach, describe, expect, spyOn, test } from "bun:test";
import { clearMocks } from "@tauri-apps/api/mocks";
import * as api from "../src/features/workflow/api";
import {
  memoryRecallSummary,
  memoryUseReasonLabel,
  openHistoryMemory,
} from "../src/features/workflow/components/history-page/history-format";
import { useWorkflowStore } from "../src/features/workflow/store";
import type { Workflow } from "../src/features/workflow/types";


function workflow(
  enabled: boolean,
  id = "workflow-1",
): Workflow {
  return {
    id,
    name: "Recall",
    description: "",
    workingDirectory: "/projects/alfred",
    memoryRetrievalEnabled: enabled,
    memoryLookupEnabled: false,
    graph: { nodes: [], edges: [] },
    createdAt: "2026-08-18T10:00:00Z",
    updatedAt: "2026-08-18T10:00:00Z",
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

let originalWindow: PropertyDescriptor | undefined;

beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {},
  });
});


afterEach(() => {
  clearMocks();
  useWorkflowStore.setState({ workflows: [], error: null });
  if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow);
  else Reflect.deleteProperty(globalThis, "window");
});


describe("automatic recall rollout and control", () => {
  test("saves the workflow toggle through the API", async () => {
    const original = workflow(false);
    const updated = { ...original, memoryRetrievalEnabled: true };
    const update = spyOn(api, "updateWorkflow").mockResolvedValue(updated);
    useWorkflowStore.setState({ workflows: [original], error: null });

    await useWorkflowStore
      .getState()
      .setMemoryRetrievalEnabled(original.id, true);

    expect(update).toHaveBeenCalledWith({
      id: original.id,
      memoryRetrievalEnabled: true,
    });
    expect(
      useWorkflowStore.getState().workflows[0].memoryRetrievalEnabled,
    ).toBe(true);
    update.mockRestore();
  });

  test("rolls back an optimistic toggle and exposes the store error", async () => {
    const original = workflow(false);
    const update = spyOn(api, "updateWorkflow").mockRejectedValue(
      new Error("recall_update_failed"),
    );
    useWorkflowStore.setState({ workflows: [original], error: null });

    await useWorkflowStore
      .getState()
      .setMemoryRetrievalEnabled(original.id, true);

    expect(
      useWorkflowStore.getState().workflows[0].memoryRetrievalEnabled,
    ).toBe(false);
    expect(useWorkflowStore.getState().error).toContain("recall_update_failed");
    update.mockRestore();
  });

  test("saves lookup permission independently from automatic recall", async () => {
    const original = workflow(true);
    const updated = { ...original, memoryLookupEnabled: true };
    const update = spyOn(api, "updateWorkflow").mockResolvedValue(updated);
    useWorkflowStore.setState({ workflows: [original], error: null });

    expect(
      await useWorkflowStore
        .getState()
        .setMemoryLookupEnabled(original.id, true),
    ).toBe(true);

    expect(useWorkflowStore.getState().workflows[0]).toMatchObject({
      memoryRetrievalEnabled: true,
      memoryLookupEnabled: true,
    });
    update.mockRestore();
  });

  test("rolls back only lookup permission after a failed save", async () => {
    const original = workflow(false);
    const other = workflow(false, "workflow-2");
    let rejectUpdate!: (error: Error) => void;
    const pending = new Promise<Workflow>((_resolve, reject) => {
      rejectUpdate = reject;
    });
    const update = spyOn(api, "updateWorkflow").mockReturnValue(pending);
    useWorkflowStore.setState({ workflows: [original], error: null });

    const saving = useWorkflowStore
      .getState()
      .setMemoryLookupEnabled(original.id, true);
    useWorkflowStore.setState((state) => ({
      workflows: [...state.workflows, other],
    }));
    rejectUpdate(new Error("lookup_update_failed"));

    expect(await saving).toBe(false);
    expect(
      useWorkflowStore.getState().workflows.map((item) => [
        item.id,
        item.memoryLookupEnabled,
      ]),
    ).toEqual([
      [original.id, false],
      [other.id, false],
    ]);
    expect(useWorkflowStore.getState().error).toContain("lookup_update_failed");
    update.mockRestore();
  });




  test("keeps the newest lookup intent when saves resolve out of order", async () => {
    const original = workflow(false);
    const first = deferred<Workflow>();
    const second = deferred<Workflow>();
    const update = spyOn(api, "updateWorkflow").mockImplementation((input) =>
      input.memoryLookupEnabled === true ? first.promise : second.promise,
    );
    useWorkflowStore.setState({ workflows: [original], error: null });

    const firstSave = useWorkflowStore
      .getState()
      .setMemoryLookupEnabled(original.id, true);
    const secondSave = useWorkflowStore
      .getState()
      .setMemoryLookupEnabled(original.id, false);

    expect(
      useWorkflowStore.getState().workflows[0].memoryLookupEnabled,
    ).toBe(false);
    first.resolve({ ...original, memoryLookupEnabled: true });
    await firstSave;
    expect(update).toHaveBeenCalledTimes(2);
    expect(
      useWorkflowStore.getState().workflows[0].memoryLookupEnabled,
    ).toBe(false);

    second.resolve({ ...original, memoryLookupEnabled: false });
    expect(await firstSave).toBe(true);
    expect(await secondSave).toBe(true);
    expect(
      useWorkflowStore.getState().workflows[0].memoryLookupEnabled,
    ).toBe(false);
    update.mockRestore();
  });

  test("preserves lookup, recall, and graph changes across overlapping saves", async () => {
    const original = workflow(false);
    const updatedGraph = {
      nodes: [],
      edges: [{ id: "edge-1", source: "node-a", target: "node-b" }],
    };
    const lookup = deferred<Workflow>();
    const recall = deferred<Workflow>();
    const graph = deferred<Workflow>();
    const update = spyOn(api, "updateWorkflow").mockImplementation((input) => {
      if (input.memoryLookupEnabled !== undefined) return lookup.promise;
      if (input.memoryRetrievalEnabled !== undefined) return recall.promise;
      if (input.graph !== undefined) return graph.promise;
      throw new Error("unexpected_workflow_update");
    });
    useWorkflowStore.setState({
      workflows: [original],
      activeWorkflowId: original.id,
      nodes: [],
      edges: [],
      dirty: true,
      error: null,
    });

    const lookupSave = useWorkflowStore
      .getState()
      .setMemoryLookupEnabled(original.id, true);
    const recallSave = useWorkflowStore
      .getState()
      .setMemoryRetrievalEnabled(original.id, true);
    const graphSave = useWorkflowStore.getState().saveActiveWorkflow();

    graph.resolve({ ...original, graph: updatedGraph });
    recall.resolve({
      ...original,
      memoryRetrievalEnabled: true,
    });
    lookup.resolve({
      ...original,
      memoryLookupEnabled: true,
    });
    await Promise.all([lookupSave, recallSave, graphSave]);

    expect(useWorkflowStore.getState().workflows[0]).toMatchObject({
      memoryRetrievalEnabled: true,
      memoryLookupEnabled: true,
      graph: updatedGraph,
    });
    update.mockRestore();
  });
});

describe("memory-use explanations", () => {
  test("maps persisted reasons without treating score as confidence", () => {
    expect(memoryUseReasonLabel("explicit")).toBe("Selected in a Memory step");
    expect(memoryUseReasonLabel("pinned")).toBe("Pinned core");
    expect(memoryUseReasonLabel("lexical")).toBe("Matched this step's prompt");
    expect(memoryUseReasonLabel("recent")).toBe("Recent fallback");
  });

  test("links a trace row to the correct workflow-aware Memories event", () => {
    let received: { memoryId?: string; workflowId?: string } | undefined;
    const previousWindow = globalThis.window;
    const previousCustomEvent = globalThis.CustomEvent;
    class TestCustomEvent<T> {
      type: string;
      detail: T;
      constructor(type: string, init: { detail: T }) {
        this.type = type;
        this.detail = init.detail;
      }
    }
    Object.defineProperty(globalThis, "CustomEvent", {
      configurable: true,
      value: TestCustomEvent,
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        dispatchEvent(event: TestCustomEvent<{ memoryId?: string }>) {
          if (event.type === "alfred:open-memories") received = event.detail;
          return true;
        },
      },
    });

    openHistoryMemory("memory-42");

    expect(received).toEqual({ memoryId: "memory-42" });
    openHistoryMemory("memory-43", "workflow-7");
    expect(received).toEqual({
      memoryId: "memory-43",
      workflowId: "workflow-7",
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: previousWindow,
    });
    Object.defineProperty(globalThis, "CustomEvent", {
      configurable: true,
      value: previousCustomEvent,
    });
  });


  test("distinguishes successful abstention, omission, and unavailable recall", () => {
    expect(
      memoryRecallSummary([
        {
          input: {
            memoryRecallStatus: "abstained",
            memoryRecallOmittedCount: 0,
          },
        },
      ]),
    ).toEqual({ status: "abstained", omittedCount: 0 });
    expect(
      memoryRecallSummary([
        {
          input: {
            memoryRecallStatus: "included",
            memoryRecallOmittedCount: 2,
          },
        },
        {
          input: {
            memoryRecallStatus: "unavailable",
            memoryRecallOmittedCount: 1,
          },
        },
      ]),
    ).toEqual({ status: "unavailable", omittedCount: 3 });
  });
});
