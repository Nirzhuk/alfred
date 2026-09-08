import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useWorkflowStore } from "../src/features/workflow/store";
import type { Workflow, WorkflowGraph, WorkflowNode } from "../src/features/workflow/types";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function inputNode(prompt = "Original prompt"): WorkflowNode {
  return {
    id: "input-1",
    type: "input",
    position: { x: 0, y: 0 },
    data: { label: "Input", prompt },
  };
}

function workflow(id = "workflow-1", graph?: WorkflowGraph): Workflow {
  return {
    id,
    name: id,
    description: "",
    workingDirectory: "",
    graph: graph ?? { nodes: [inputNode()], edges: [] },
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

type SaveInput = { id: string; graph: WorkflowGraph };
let originalWindow: PropertyDescriptor | undefined;

beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { crypto: globalThis.crypto },
  });
  const current = workflow();
  useWorkflowStore.setState({
    ...useWorkflowStore.getInitialState(),
    activeWorkflowId: current.id,
    workflows: [current, workflow("workflow-2")],
    nodes: current.graph!.nodes,
    edges: current.graph!.edges,
    dirty: true,
  }, true);
});

afterEach(() => {
  clearMocks();
  if (originalWindow) {
    Object.defineProperty(globalThis, "window", originalWindow);
  } else {
    Reflect.deleteProperty(globalThis, "window");
  }
});

describe("workflow persistence", () => {
  test("marks the saved snapshot clean when no newer edits exist", async () => {
    mockIPC((command, args) => {
      expect(command).toBe("update_workflow");
      const input = (args as { input: SaveInput }).input;
      return workflow(input.id, input.graph);
    });

    expect(await useWorkflowStore.getState().saveActiveWorkflow()).toBe(true);
    expect(useWorkflowStore.getState().dirty).toBe(false);
    expect(useWorkflowStore.getState().loading).toBe(false);
  });

  test("keeps node edits made during a pending save dirty", async () => {
    const pending = deferred<Workflow>();
    let submitted!: SaveInput;
    mockIPC((_command, args) => {
      submitted = (args as { input: SaveInput }).input;
      return pending.promise;
    });

    const save = useWorkflowStore.getState().saveActiveWorkflow();
    useWorkflowStore.getState().updateNodeData("input-1", { prompt: "New prompt" });
    pending.resolve(workflow(submitted.id, submitted.graph));
    expect(await save).toBe(true);

    expect(useWorkflowStore.getState().dirty).toBe(true);
    expect(useWorkflowStore.getState().nodes[0].data).toMatchObject({ prompt: "New prompt" });
    expect(useWorkflowStore.getState().workflows[0].graph!.nodes[0].data)
      .toMatchObject({ prompt: "Original prompt" });
  });

  test("keeps edge edits made during a pending save dirty", async () => {
    const pending = deferred<Workflow>();
    mockIPC(() => pending.promise);

    const save = useWorkflowStore.getState().saveActiveWorkflow();
    useWorkflowStore.getState().onConnect({
      source: "input-1",
      target: "output-1",
      sourceHandle: null,
      targetHandle: null,
    });
    pending.resolve(workflow());
    await save;

    expect(useWorkflowStore.getState().dirty).toBe(true);
    expect(useWorkflowStore.getState().edges).toHaveLength(1);
  });

  test("does not clear another workflow's unsaved edits after a tab switch", async () => {
    const pending = deferred<Workflow>();
    mockIPC(() => pending.promise);

    const save = useWorkflowStore.getState().saveActiveWorkflow();
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-2",
      nodes: [inputNode("Unsaved second workflow")],
      edges: [],
      dirty: true,
    });
    pending.resolve(workflow());
    await save;

    expect(useWorkflowStore.getState().activeWorkflowId).toBe("workflow-2");
    expect(useWorkflowStore.getState().dirty).toBe(true);
  });

  test("writes overlapping saves in order and remains loading until the last save", async () => {
    const firstWrite = deferred<Workflow>();
    const secondWrite = deferred<Workflow>();
    const inputs: SaveInput[] = [];
    mockIPC((_command, args) => {
      inputs.push((args as { input: SaveInput }).input);
      return inputs.length === 1 ? firstWrite.promise : secondWrite.promise;
    });

    const firstSave = useWorkflowStore.getState().saveActiveWorkflow();
    useWorkflowStore.getState().updateNodeData("input-1", { prompt: "Newest prompt" });
    const secondSave = useWorkflowStore.getState().saveActiveWorkflow();
    expect(inputs).toHaveLength(1);

    firstWrite.resolve(workflow(inputs[0].id, inputs[0].graph));
    expect(await firstSave).toBe(true);
    expect(inputs).toHaveLength(2);
    expect(useWorkflowStore.getState().loading).toBe(true);
    expect(useWorkflowStore.getState().dirty).toBe(true);

    secondWrite.resolve(workflow(inputs[1].id, inputs[1].graph));
    expect(await secondSave).toBe(true);
    expect(useWorkflowStore.getState().loading).toBe(false);
    expect(useWorkflowStore.getState().dirty).toBe(false);
    expect(useWorkflowStore.getState().workflows[0].graph!.nodes[0].data)
      .toMatchObject({ prompt: "Newest prompt" });
  });

  test("continues a queued save after an earlier save fails", async () => {
    const firstWrite = deferred<Workflow>();
    let calls = 0;
    mockIPC((_command, args) => {
      calls += 1;
      const input = (args as { input: SaveInput }).input;
      return calls === 1 ? firstWrite.promise : workflow(input.id, input.graph);
    });

    const firstSave = useWorkflowStore.getState().saveActiveWorkflow();
    useWorkflowStore.getState().updateNodeData("input-1", { prompt: "Retry prompt" });
    const secondSave = useWorkflowStore.getState().saveActiveWorkflow();
    firstWrite.reject(new Error("Temporary write failure"));

    expect(await firstSave).toBe(false);
    expect(await secondSave).toBe(true);
    expect(useWorkflowStore.getState().error).toBeNull();
    expect(useWorkflowStore.getState().dirty).toBe(false);
  });

  test("does not run the persisted graph when saving the current edits fails", async () => {
    const commands: string[] = [];
    mockIPC((command) => {
      commands.push(command);
      throw new Error("Save failed");
    });

    await useWorkflowStore.getState().runActiveWorkflow();

    expect(commands).toEqual(["update_workflow"]);
    expect(useWorkflowStore.getState().error).toBe("Error: Save failed");
    expect(useWorkflowStore.getState().dirty).toBe(true);
    expect(useWorkflowStore.getState().activeRun).toBeNull();
  });

  test("runs after the current edits are successfully saved", async () => {
    const commands: string[] = [];
    mockIPC((command, args) => {
      commands.push(command);
      if (command === "update_workflow") {
        const input = (args as { input: SaveInput }).input;
        return workflow(input.id, input.graph);
      }
      if (command === "run_workflow") {
        return { id: "run-1", workflowId: "workflow-1", status: "running" };
      }
      throw new Error(`Unexpected command: ${command}`);
    }, { shouldMockEvents: true });

    await useWorkflowStore.getState().runActiveWorkflow();

    expect(commands).toEqual(["update_workflow", "run_workflow"]);
    expect(useWorkflowStore.getState().dirty).toBe(false);
    expect(useWorkflowStore.getState().activeRun?.id).toBe("run-1");
  });
});
