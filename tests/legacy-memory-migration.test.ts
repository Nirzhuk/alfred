import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useWorkflowStore } from "../src/features/workflow/store";
import { withMemoryDefaults } from "../src/features/workflow/memories";
import type { OutputMemory } from "../src/features/workflow/types";

const WORKFLOW_ID = "legacy-workflow";
const STORAGE_KEY = `alfred:memories:${WORKFLOW_ID}`;

function legacyMemory(id: string) {
  return {
    id,
    workflowId: WORKFLOW_ID,
    title: `Saved ${id}`,
    body: `Private body for ${id}`,
    createdAt: "2026-01-01T00:00:00Z",
  };
}

function storedMemory(id: string): OutputMemory {
  const legacy = legacyMemory(id);
  return withMemoryDefaults({
    ...legacy,
    updatedAt: legacy.createdAt,
    source: "import",
  });
}

let originalWindow: PropertyDescriptor | undefined;
let originalStorage: PropertyDescriptor | undefined;
let storage: Map<string, string>;

beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  originalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { crypto: globalThis.crypto },
  });
  storage = new Map();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    },
  });
  useWorkflowStore.setState({
    ...useWorkflowStore.getInitialState(),
    activeWorkflowId: WORKFLOW_ID,
  }, true);
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

describe("legacy memory migration", () => {
  test("retains failed imports and still displays memories already in SQLite", async () => {
    const legacy = legacyMemory("failed");
    storage.set(STORAGE_KEY, JSON.stringify([legacy]));
    mockIPC((command) => {
      if (command === "create_memory") throw new Error(legacy.body);
      if (command === "list_memories") return [storedMemory("existing")];
      throw new Error(`Unexpected command: ${command}`);
    });

    await useWorkflowStore.getState().loadMemories(WORKFLOW_ID);

    expect(JSON.parse(storage.get(STORAGE_KEY)!)).toEqual([legacy]);
    expect(useWorkflowStore.getState().memories.map((memory) => memory.id))
      .toEqual(["existing"]);
    expect(useWorkflowStore.getState().error).toContain("reopen this workflow to retry");
    expect(useWorkflowStore.getState().error).not.toContain(legacy.body);
  });

  test("retries only failed imports even after a partial migration populated SQLite", async () => {
    storage.set(STORAGE_KEY, JSON.stringify([
      legacyMemory("first"), legacyMemory("second"),
    ]));
    const persisted = new Map<string, OutputMemory>();
    const attempts: string[] = [];
    let failSecond = true;
    mockIPC((command, args) => {
      if (command === "create_memory") {
        const { id } = (args as { input: { id: string } }).input;
        attempts.push(id);
        if (id === "second" && failSecond) throw new Error("Write failed");
        const memory = storedMemory(id);
        persisted.set(id, memory);
        return memory;
      }
      if (command === "list_memories") return [...persisted.values()];
      throw new Error(`Unexpected command: ${command}`);
    });

    await useWorkflowStore.getState().loadMemories(WORKFLOW_ID);
    expect(JSON.parse(storage.get(STORAGE_KEY)!)).toEqual([legacyMemory("second")]);
    expect(useWorkflowStore.getState().memories.map((memory) => memory.id)).toEqual(["first"]);

    failSecond = false;
    await useWorkflowStore.getState().loadMemories(WORKFLOW_ID);

    expect(attempts).toEqual(["first", "second", "second"]);
    expect(storage.has(STORAGE_KEY)).toBe(false);
    expect(useWorkflowStore.getState().memories).toHaveLength(2);
    expect(useWorkflowStore.getState().error).toBeNull();
  });

  test("shares pending imports between overlapping loads", async () => {
    storage.set(STORAGE_KEY, JSON.stringify([legacyMemory("pending")]));
    let completeImport!: (memory: OutputMemory) => void;
    const importResult = new Promise<OutputMemory>((resolve) => {
      completeImport = resolve;
    });
    let attempts = 0;
    mockIPC((command) => {
      if (command === "create_memory") {
        attempts += 1;
        return importResult;
      }
      if (command === "list_memories") return [storedMemory("pending")];
      throw new Error(`Unexpected command: ${command}`);
    });

    const first = useWorkflowStore.getState().loadMemories(WORKFLOW_ID);
    const second = useWorkflowStore.getState().loadMemories(WORKFLOW_ID);
    expect(attempts).toBe(1);
    completeImport(storedMemory("pending"));
    await Promise.all([first, second]);

    expect(attempts).toBe(1);
    expect(storage.has(STORAGE_KEY)).toBe(false);
    expect(useWorkflowStore.getState().memories).toHaveLength(1);
  });

  test("keeps the original records if cleanup cannot read localStorage", async () => {
    const original = JSON.stringify([legacyMemory("first"), legacyMemory("second")]);
    storage.set(STORAGE_KEY, original);
    mockIPC((command, args) => {
      if (command === "create_memory") {
        const { id } = (args as { input: { id: string } }).input;
        if (id === "second") {
          localStorage.getItem = () => { throw new Error("Storage unavailable"); };
          throw new Error("Write failed");
        }
        return storedMemory(id);
      }
      if (command === "list_memories") return [storedMemory("first")];
      throw new Error(`Unexpected command: ${command}`);
    });

    await useWorkflowStore.getState().loadMemories(WORKFLOW_ID);

    expect(storage.get(STORAGE_KEY)).toBe(original);
    expect(useWorkflowStore.getState().memories).toHaveLength(1);
    expect(useWorkflowStore.getState().error).toContain("original data has been kept");
  });

  test("keeps the original records when persisting the remaining imports fails", async () => {
    const original = JSON.stringify([legacyMemory("first"), legacyMemory("second")]);
    storage.set(STORAGE_KEY, original);
    localStorage.setItem = () => { throw new Error("Storage full"); };
    mockIPC((command, args) => {
      if (command === "create_memory") {
        const { id } = (args as { input: { id: string } }).input;
        if (id === "second") throw new Error("Write failed");
        return storedMemory(id);
      }
      if (command === "list_memories") return [storedMemory("first")];
      throw new Error(`Unexpected command: ${command}`);
    });

    await useWorkflowStore.getState().loadMemories(WORKFLOW_ID);

    expect(storage.get(STORAGE_KEY)).toBe(original);
    expect(useWorkflowStore.getState().memories).toHaveLength(1);
    expect(useWorkflowStore.getState().error).toContain("original data has been kept");
  });

  test("does not replace another workflow's memories or error after a tab switch", async () => {
    storage.set(STORAGE_KEY, JSON.stringify([legacyMemory("pending")]));
    let rejectImport!: (error: Error) => void;
    const importResult = new Promise<OutputMemory>((_resolve, reject) => {
      rejectImport = reject;
    });
    mockIPC((command) => {
      if (command === "create_memory") return importResult;
      if (command === "list_memories") return [];
      throw new Error(`Unexpected command: ${command}`);
    });

    const load = useWorkflowStore.getState().loadMemories(WORKFLOW_ID);
    useWorkflowStore.setState({
      activeWorkflowId: "another-workflow",
      memories: [storedMemory("another-memory")],
      error: "Another workflow error",
    });
    rejectImport(new Error("Write failed"));
    await load;

    expect(useWorkflowStore.getState().memories[0].id).toBe("another-memory");
    expect(useWorkflowStore.getState().error).toBe("Another workflow error");
    expect(storage.has(STORAGE_KEY)).toBe(true);
  });
});
