import { afterEach, describe, expect, mock, spyOn, test } from "bun:test";
import * as api from "../src/features/workflow/api";
import { withMemoryDefaults } from "../src/features/workflow/memories";
import { useWorkflowStore } from "../src/features/workflow/store";
import type { OutputMemory } from "../src/features/workflow/types";

const memory = (id: string, patch: Partial<OutputMemory> = {}) =>
  withMemoryDefaults({
    id,
    title: id,
    body: id,
    workflowId: "workflow-a",
    createdAt: "2026-09-01T00:00:00Z",
    updatedAt: "2026-09-01T00:00:00Z",
    ...patch,
  });

afterEach(() => {
  mock.restore();
  useWorkflowStore.setState({
    activeWorkflowId: null,
    memories: [],
    error: null,
  });
});

describe("Memory library mutations", () => {
  test("manual creation forwards correction identity and all supported dates", async () => {
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-a",
      memories: [],
    });
    const create = spyOn(api, "createMemory").mockResolvedValue(memory("dated"));

    await useWorkflowStore.getState().addMemory({
      title: "Dated fact",
      body: "The rollout begins tomorrow.",
      factKey: "fact-v1|workflow",
      observedAt: "2026-09-10T08:00:00Z",
      lastConfirmedAt: "2026-09-10T08:00:00Z",
      validFrom: "2026-09-11T00:00:00Z",
      expiresAt: "2026-09-12T00:00:00Z",
    });

    expect(create).toHaveBeenCalledWith(
      expect.objectContaining({
        workflowId: "workflow-a",
        factKey: "fact-v1|workflow",
        observedAt: "2026-09-10T08:00:00Z",
        lastConfirmedAt: "2026-09-10T08:00:00Z",
        validFrom: "2026-09-11T00:00:00Z",
        expiresAt: "2026-09-12T00:00:00Z",
      }),
    );
  });

  test("clearing owned memories reloads linked and inherited survivors", async () => {
    const survivors = [
      memory("linked", { origin: "linked", workflowId: "other" }),
      memory("user", { origin: "inherited", scopeType: "user" }),
      memory("workspace", { origin: "inherited", scopeType: "workspace" }),
    ];
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-a",
      memories: [memory("owned"), ...survivors],
    });
    const clear = spyOn(api, "clearMemories").mockResolvedValue(1);
    const list = spyOn(api, "listMemories").mockResolvedValue(survivors);
    await useWorkflowStore.getState().clearMemories();
    expect(clear).toHaveBeenCalledWith("workflow-a");
    expect(list).toHaveBeenCalledWith("workflow-a", true);
    expect(
      useWorkflowStore
        .getState()
        .memories.map(({ id }) => id)
        .sort(),
    ).toEqual(["linked", "user", "workspace"]);
  });

  test("a clear reload cannot replace another workflow's library", async () => {
    let finish!: (memories: OutputMemory[]) => void;
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-a",
      memories: [memory("owned-a")],
    });
    spyOn(api, "clearMemories").mockResolvedValue(1);
    spyOn(api, "listMemories").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = useWorkflowStore.getState().clearMemories();
    await Promise.resolve();
    const visible = memory("owned-b", { workflowId: "workflow-b" });
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-b",
      memories: [visible],
    });
    finish([]);
    await pending;
    expect(useWorkflowStore.getState().memories).toEqual([visible]);
  });

  test("failed clearing preserves the library and exposes the error", async () => {
    const original = [memory("owned")];
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-a",
      memories: original,
    });
    spyOn(api, "clearMemories").mockRejectedValue(new Error("clear failed"));
    await useWorkflowStore.getState().clearMemories();
    expect(useWorkflowStore.getState().memories).toEqual(original);
    expect(useWorkflowStore.getState().error).toContain("clear failed");
  });

  test("link completion after switching workflows does not insert into the new library", async () => {
    let finish!: (memory: OutputMemory) => void;
    useWorkflowStore.setState({ activeWorkflowId: "workflow-a", memories: [] });
    spyOn(api, "linkMemory").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = useWorkflowStore.getState().linkMemory("linked");
    useWorkflowStore.setState({ activeWorkflowId: "workflow-b", memories: [] });
    finish(memory("linked", { origin: "linked" }));
    expect(await pending).not.toBeNull();
    expect(useWorkflowStore.getState().memories).toEqual([]);
  });

  test("unlink completion after switching workflows does not remove a shared visible ID", async () => {
    let finish!: () => void;
    const linked = memory("linked", { origin: "linked" });
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-a",
      memories: [linked],
    });
    spyOn(api, "unlinkMemory").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = useWorkflowStore.getState().unlinkMemory("linked");
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-b",
      memories: [linked],
    });
    finish();
    await pending;
    expect(useWorkflowStore.getState().memories).toEqual([linked]);
  });

  test("update completion does not replace a shared memory's origin after switching workflows", async () => {
    let finish!: (memory: OutputMemory) => void;
    const original = memory("shared", {
      origin: "owned",
      sourceWorkflowName: "Author workflow",
    });
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-a",
      memories: [original],
    });
    const update = spyOn(api, "updateMemory").mockImplementation(
      () => new Promise((resolve) => { finish = resolve; }),
    );
    const pending = useWorkflowStore.getState().updateMemoryFields({
      id: "shared",
      title: "Edited title",
    });
    const visible = memory("shared", {
      origin: "linked",
      sourceWorkflowName: "Linked author workflow",
    });
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-b",
      memories: [visible],
    });
    finish({ ...original, title: "Edited title" });
    expect((await pending)?.title).toBe("Edited title");
    expect(update).toHaveBeenCalledWith({
      id: "shared",
      expectedSemanticRevision: 1,
      title: "Edited title",
      contextWorkflowId: "workflow-a",
    });
    expect(useWorkflowStore.getState().memories).toEqual([visible]);
  });

  test("a dirty editor keeps its captured revision across a background refresh", async () => {
    const concurrent = memory("shared", {
      body: "Concurrent canary",
      status: "superseded",
      semanticRevision: 2,
    });
    useWorkflowStore.setState({
      activeWorkflowId: "workflow-a",
      memories: [concurrent],
    });
    const update = spyOn(api, "updateMemory").mockRejectedValue(
      new Error("memory_changed"),
    );
    spyOn(api, "listMemories").mockResolvedValue([concurrent]);

    const result = await useWorkflowStore.getState().updateMemoryFields({
      id: "shared",
      expectedSemanticRevision: 1,
      body: "Old dirty draft",
      status: "active",
    });

    expect(result).toBeNull();
    expect(update).toHaveBeenCalledWith({
      id: "shared",
      expectedSemanticRevision: 1,
      body: "Old dirty draft",
      status: "active",
      contextWorkflowId: "workflow-a",
    });
    expect(useWorkflowStore.getState().memories[0]).toMatchObject({
      body: "Concurrent canary",
      status: "superseded",
      semanticRevision: 2,
    });
  });
});
