import { afterEach, beforeEach, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useWorkflowStore } from "./store";
import type { Workflow } from "./types";

const store = () => useWorkflowStore.getState();
let records: Map<string, Workflow>;
let nextId: number;
let deleteFailure: boolean;
let pendingUpdate: Promise<void> | null;
let originalWindow: PropertyDescriptor | undefined;

function workflow(id: string, name = id): Workflow {
  return {
    id, name, description: "", workingDirectory: "", memoryLookupEnabled: false,
    graph: { nodes: [], edges: [] },
    createdAt: "2026-10-16T00:00:00Z", updatedAt: "2026-10-16T00:00:00Z",
  };
}

beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true, value: { crypto: globalThis.crypto },
  });
  records = new Map([["saved", workflow("saved")]]);
  nextId = 0;
  deleteFailure = false;
  pendingUpdate = null;
  useWorkflowStore.setState({
    ...useWorkflowStore.getInitialState(),
    workflows: [...records.values()], openWorkflowIds: ["saved"],
    activeWorkflowId: "saved",
  }, true);
  mockIPC(async (command, args) => {
    const { id, input } = (args ?? {}) as {
      id: string; input: Partial<Workflow> & { id: string };
    };
    switch (command) {
      case "create_workflow": {
        const created = { ...workflow(`new-${++nextId}`), ...input };
        records.set(created.id, created);
        return created;
      }
      case "get_workflow": return records.get(id) ?? null;
      case "update_workflow": {
        if (pendingUpdate) await pendingUpdate;
        const saved = { ...records.get(input.id)!, ...input };
        records.set(saved.id, saved);
        return saved;
      }
      case "delete_workflow":
        if (deleteFailure) throw new Error("Database unavailable");
        records.delete(id);
        return;
      case "delete_workflow_schedule": return;
      case "get_workflow_schedule": return null;
      case "list_workflow_triggers":
      case "list_app_trigger_statuses":
      case "list_memories": return [];
      case "webhook_base_url": return null;
      default: throw new Error(`Unexpected command: ${command}`);
    }
  });
});

afterEach(() => {
  clearMocks();
  useWorkflowStore.setState(useWorkflowStore.getInitialState(), true);
  if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow);
  else Reflect.deleteProperty(globalThis, "window");
});

test("switching workflows removes a new untouched workflow from storage and tabs", async () => {
  await store().createWorkflow();
  const id = store().activeWorkflowId!;
  expect(await store().selectWorkflow("saved")).toBe(true);
  expect(records.has(id)).toBe(false);
  expect(store().workflows.map((item) => item.id)).toEqual(["saved"]);
  expect(store().openWorkflowIds).toEqual(["saved"]);
  expect(store().activeWorkflowId).toBe("saved");
});

test("creating another workflow discards the previous untouched one", async () => {
  await store().createWorkflow();
  const first = store().activeWorkflowId!;
  await store().createWorkflow();
  expect(records.has(first)).toBe(false);
  expect(store().workflows.map((item) => item.id)).toEqual(["new-2", "saved"]);
  expect(store().activeWorkflowId).toBe("new-2");
});

test("closing the only untouched tab leaves no workflow behind", async () => {
  records.clear();
  useWorkflowStore.setState({ workflows: [], openWorkflowIds: [], activeWorkflowId: null });
  await store().createWorkflow();
  await store().closeWorkflowTab(store().activeWorkflowId!);
  expect([...records.keys()]).toEqual([]);
  expect(store().activeWorkflowId).toBeNull();
  expect(store().workflows).toEqual([]);
  expect(store().openWorkflowIds).toEqual([]);
});

test("leaving for another app section discards without selecting a different workflow", async () => {
  await store().createWorkflow();
  expect(await store().discardUntouchedWorkflow(store().activeWorkflowId!)).toBe(true);
  expect(store().activeWorkflowId).toBeNull();
  expect([...records.keys()]).toEqual(["saved"]);
});

test("reselecting a new workflow is not abandonment, and existing empty workflows stay", async () => {
  await store().createWorkflow();
  const id = store().activeWorkflowId!;
  await store().selectWorkflow(id);
  expect(records.has(id)).toBe(true);
  expect(await store().discardUntouchedWorkflow("saved")).toBe(false);
  expect(records.has("saved")).toBe(true);
  await store().selectWorkflow("saved");
  expect(records.has(id)).toBe(false);
});

test("editing then removing a step still keeps the workflow after autosave", async () => {
  await store().createWorkflow();
  const id = store().activeWorkflowId!;
  store().addNode({ id: "input", type: "input", position: { x: 0, y: 0 }, data: { label: "Input", prompt: "Hello" } });
  store().removeNode("input");
  expect(await store().saveActiveWorkflow()).toBe(true);
  await store().selectWorkflow("saved");
  expect(records.has(id)).toBe(true);
  expect(store().openWorkflowIds).toContain(id);
});

test("a rename in flight protects a new workflow from navigation cleanup", async () => {
  await store().createWorkflow();
  const id = store().activeWorkflowId!;
  let finish!: () => void;
  pendingUpdate = new Promise<void>((resolve) => { finish = resolve; });
  const rename = store().renameWorkflow(id, "Keep this workflow");
  await store().selectWorkflow("saved");
  expect(records.has(id)).toBe(true);
  finish();
  await rename;
  expect(records.get(id)?.name).toBe("Keep this workflow");
});

test("failed cleanup leaves the workflow available and reports the error", async () => {
  await store().createWorkflow();
  const id = store().activeWorkflowId!;
  deleteFailure = true;
  expect(await store().discardUntouchedWorkflow(id)).toBe(false);
  expect(records.has(id)).toBe(true);
  expect(store().workflows.some((item) => item.id === id)).toBe(true);
  expect(store().error).toContain("Database unavailable");
});
