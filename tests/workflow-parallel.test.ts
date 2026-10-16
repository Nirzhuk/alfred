import { afterEach, beforeEach, describe, expect, spyOn, test } from "bun:test";
import { clearMocks } from "@tauri-apps/api/mocks";
import * as api from "../src/features/workflow/api";
import { useWorkflowStore } from "../src/features/workflow/store";
import type { Workflow } from "../src/features/workflow/types";

function workflow(parallelBranches: boolean | undefined): Workflow {
  return {
    id: "workflow-1",
    name: "Debate",
    description: "",
    workingDirectory: "/projects/alfred",
    memoryLookupEnabled: false,
    ...(parallelBranches === undefined ? {} : { parallelBranches }),
    graph: { nodes: [], edges: [] },
    createdAt: "2026-08-18T10:00:00Z",
    updatedAt: "2026-08-18T10:00:00Z",
  };
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

describe("parallel branches toggle", () => {
  test("saves the workflow toggle through the API", async () => {
    const original = workflow(false);
    const updated = { ...original, parallelBranches: true };
    const update = spyOn(api, "updateWorkflow").mockResolvedValue(updated);
    useWorkflowStore.setState({ workflows: [original], error: null });

    await useWorkflowStore.getState().setParallelBranches(original.id, true);

    expect(update).toHaveBeenCalledWith({
      id: original.id,
      parallelBranches: true,
    });
    expect(
      useWorkflowStore.getState().workflows[0].parallelBranches,
    ).toBe(true);
    update.mockRestore();
  });

  test("toggles a pre-flag workflow that omits the field", async () => {
    const original = workflow(undefined);
    const updated = { ...original, parallelBranches: false };
    const update = spyOn(api, "updateWorkflow").mockResolvedValue(updated);
    useWorkflowStore.setState({ workflows: [original], error: null });

    await useWorkflowStore.getState().setParallelBranches(original.id, false);

    expect(update).toHaveBeenCalledWith({
      id: original.id,
      parallelBranches: false,
    });
    expect(
      useWorkflowStore.getState().workflows[0].parallelBranches,
    ).toBe(false);
    update.mockRestore();
  });

  test("rolls back an optimistic toggle and exposes the store error", async () => {
    const original = workflow(true);
    const update = spyOn(api, "updateWorkflow").mockRejectedValue(
      new Error("parallel_update_failed"),
    );
    useWorkflowStore.setState({ workflows: [original], error: null });

    await useWorkflowStore
      .getState()
      .setParallelBranches(original.id, false);

    expect(
      useWorkflowStore.getState().workflows[0].parallelBranches,
    ).toBe(true);
    expect(useWorkflowStore.getState().error).toContain("parallel_update_failed");
    update.mockRestore();
  });
});
