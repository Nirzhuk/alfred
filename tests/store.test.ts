import { beforeEach, describe, expect, test } from "bun:test";
import { useToastStore } from "../src/components/toast/toast-store";
import {
  normalizeNodes,
  useWorkflowStore,
} from "../src/features/workflow/store";
import {
  defaultAgentNodeData,
  type WorkflowNode,
} from "../src/features/workflow/types";
import type {
  AgentActivity,
  RunEvent,
  Workflow,
} from "../src/features/workflow/types";

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: { hidden: false, hasFocus: () => true },
  });
});

function inputNode(blocked: boolean): WorkflowNode {
  return {
    id: "input-1",
    type: "input",
    position: { x: 24, y: 32 },
    data: {
      label: "Protected input",
      prompt: "Keep this prompt",
      attachments: [
        { id: "attachment-1", kind: "file", path: "/tmp/context.txt" },
      ],
      blocked,
    },
  };
}

describe("agent harness graph migration", () => {
  test("new agent nodes emit an explicit CLI harness", () => {
    expect(defaultAgentNodeData("omp", "default")).toEqual({
      label: "Agent",
      provider: "omp",
      harness: "cli",
      model: "default",
      skillNames: [],
    });
  });

  test("defaults old agent nodes to CLI and removes credential-shaped fields", () => {
    const [node] = normalizeNodes([
      {
        id: "agent-old",
        type: "agent",
        position: { x: 0, y: 0 },
        data: {
          label: "Agent",
          provider: "pi",
          model: "default",
          accessToken: "must-not-persist",
          refreshToken: "must-not-persist",
          apiKey: "must-not-persist",
          credentials: { secret: "must-not-persist" },
        } as never,
      },
    ]);

    expect(node.data).toMatchObject({ provider: "pi", harness: "cli" });
    const serialized = JSON.stringify(node);
    expect(serialized).not.toContain("must-not-persist");
    expect(serialized).not.toContain("accessToken");
    expect(serialized).not.toContain("refreshToken");
    expect(serialized).not.toContain("apiKey");
  });

  test("old, imported, duplicated, and template graphs all stay on CLI", () => {
    for (const source of ["old", "imported", "duplicated", "template"]) {
      const [node] = normalizeNodes([
        {
          id: `agent-${source}`,
          type: "agent",
          position: { x: 0, y: 0 },
          data: {
            label: `${source} agent`,
            provider: "codex",
            model: "gpt-5.6-luna",
          },
        },
      ]);
      expect(node.data).toMatchObject({
        provider: "codex",
        harness: "cli",
      });
    }
  });

  test("preserves an explicit native selection without rewriting it to CLI", () => {
    const [node] = normalizeNodes([
      {
        id: "agent-native",
        type: "agent",
        position: { x: 0, y: 0 },
        data: {
          label: "Native selection",
          provider: "codex",
          harness: "alfred",
          accountRef: "account_opaque",
        },
      },
    ]);
    expect(node.data).toMatchObject({
      provider: "codex",
      harness: "alfred",
      accountRef: "account_opaque",
    });
  });

  test("rejects unknown persisted harness values with a stable error", () => {
    expect(() =>
      normalizeNodes([
        {
          id: "agent-invalid",
          type: "agent",
          position: { x: 0, y: 0 },
          data: {
            label: "Agent",
            provider: "codex",
            harness: "vendor-native",
          } as never,
        },
      ]),
    ).toThrow("invalid_agent_harness");
    expect(() =>
      normalizeNodes([
        {
          id: "agent-null",
          type: "agent",
          position: { x: 0, y: 0 },
          data: {
            label: "Agent",
            provider: "codex",
            harness: null,
          } as never,
        },
      ]),
    ).toThrow("invalid_agent_harness");
  });
});

describe("blocked Input nodes", () => {
  beforeEach(() => {
    useWorkflowStore.setState({
      nodes: [inputNode(true)],
      dirty: false,
    });
  });

  test("ignore content updates while blocked", () => {
    useWorkflowStore.getState().updateNodeData("input-1", {
      label: "Changed label",
      prompt: "Changed prompt",
      attachments: [],
    });

    const node = useWorkflowStore.getState().nodes[0];
    expect(node.data).toMatchObject({
      label: "Protected input",
      prompt: "Keep this prompt",
      blocked: true,
    });
    expect("attachments" in node.data && node.data.attachments).toHaveLength(1);
    expect(useWorkflowStore.getState().dirty).toBe(false);
  });

  test("only applies the unblock field when a mixed patch is received", () => {
    useWorkflowStore.getState().updateNodeData("input-1", {
      blocked: false,
      prompt: "Changed prompt",
    });

    expect(useWorkflowStore.getState().nodes[0].data).toMatchObject({
      prompt: "Keep this prompt",
      blocked: false,
    });
    expect(useWorkflowStore.getState().dirty).toBe(true);
  });

  test("allows edits after the Input is unblocked", () => {
    useWorkflowStore.getState().updateNodeData("input-1", { blocked: false });
    useWorkflowStore.getState().updateNodeData("input-1", {
      prompt: "Changed prompt",
    });

    expect(useWorkflowStore.getState().nodes[0].data).toMatchObject({
      prompt: "Changed prompt",
      blocked: false,
    });
  });
});

function workflow(id: string, nodeId: string): Workflow {
  return {
    id,
    name: id,
    description: "",
    memoryLookupEnabled: false,
    graph: {
      nodes: [
        {
          id: nodeId,
          type: "input",
          position: { x: 0, y: 0 },
          data: { label: nodeId, prompt: "Run" },
        },
      ],
      edges: [],
    },
    createdAt: "2026-08-11T10:00:00.000Z",
    updatedAt: "2026-08-11T10:00:00.000Z",
  };
}

function runEvent(
  workflowId: string,
  runId: string,
  kind: RunEvent["kind"],
  nodeId?: string,
): RunEvent {
  return {
    workflowId,
    runId,
    kind,
    nodeId,
    message: kind,
    at: "2026-08-11T10:00:00.000Z",
  };
}

function activityEvent(
  runId: string,
  nodeId: string,
  activity: AgentActivity,
  at = "2026-08-11T10:00:01.000Z",
): RunEvent {
  return {
    workflowId: "activity-workflow",
    runId,
    kind: "agent_activity",
    nodeId,
    nodeLabel: "Agent",
    status: "running",
    message: activity.label,
    activity,
    at,
  };
}

describe("concurrent workflow runs", () => {
  beforeEach(() => {
    const first = workflow("first", "first-node");
    const second = workflow("second", "second-node");
    useWorkflowStore.setState({
      workflows: [first, second],
      activeWorkflowId: first.id,
      nodes: first.graph.nodes,
      workflowRunStates: {},
      activeRun: null,
      activeNodeId: null,
      inspectedNodeId: null,
      stepStatuses: {},
      stepOutputs: {},
      stepStats: {},
      runLogs: [],
      selectedNodeId: null,
    });
  });

  test("isolates interleaved events without switching the selected workflow", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent("first", "run-first", "started"));
    store.handleRunEvent(runEvent("second", "run-second", "started"));
    store.handleRunEvent(
      runEvent("second", "run-second", "step_started", "second-node"),
    );

    let state = useWorkflowStore.getState();
    expect(state.activeWorkflowId).toBe("first");
    expect(state.activeRun?.id).toBe("run-first");
    expect(state.activeNodeId).toBeNull();
    expect(state.workflowRunStates.first.activeRun?.status).toBe("running");
    expect(state.workflowRunStates.second.activeRun?.status).toBe("running");
    expect(state.workflowRunStates.second.activeNodeId).toBe("second-node");

    state.handleRunEvent(
      runEvent("first", "run-first", "step_started", "first-node"),
    );
    state = useWorkflowStore.getState();
    expect(state.activeNodeId).toBe("first-node");
    expect(state.workflowRunStates.second.activeNodeId).toBe("second-node");
  });

  test("releases completed detail for background workflows that are not open", () => {
    useWorkflowStore.setState({ openWorkflowIds: ["first"] });
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent("second", "run-second", "started"));
    store.handleRunEvent({
      ...runEvent("second", "run-second", "completed"),
      output: "large background result",
    });

    expect(useWorkflowStore.getState().workflowRunStates.second).toBeUndefined();
    expect(useWorkflowStore.getState().activeWorkflowId).toBe("first");
  });

  test("releases completed detail when a non-active workflow tab closes", async () => {
    useWorkflowStore.setState({ openWorkflowIds: ["first", "second"] });
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent("second", "run-second", "started"));
    store.handleRunEvent(runEvent("second", "run-second", "completed"));
    expect(useWorkflowStore.getState().workflowRunStates.second).toBeDefined();

    await useWorkflowStore.getState().closeWorkflowTab("second");

    const state = useWorkflowStore.getState();
    expect(state.openWorkflowIds).toEqual(["first"]);
    expect(state.workflowRunStates.second).toBeUndefined();
  });
});

describe("agent activity run logs", () => {
  const runId = "activity-run";
  const nodeId = "agent-node";

  beforeEach(() => {
    Object.defineProperty(globalThis, "document", {
      configurable: true,
      value: { hidden: false, hasFocus: () => true },
    });
    const currentWorkflow = workflow("activity-workflow", nodeId);
    useWorkflowStore.setState({
      workflows: [currentWorkflow],
      activeWorkflowId: currentWorkflow.id,
      nodes: currentWorkflow.graph.nodes,
      workflowRunStates: {},
      activeRun: null,
      activeNodeId: null,
      inspectedNodeId: null,
      stepStatuses: {},
      stepOutputs: {},
      stepStats: {},
      runLogs: [],
      selectedNodeId: null,
      selectedOutput: null,
    });
  });

  test("replaces a started activity with its completion", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(
      runEvent("activity-workflow", runId, "started"),
    );
    store.handleRunEvent(
      activityEvent(runId, nodeId, {
        id: "tool-1",
        kind: "tool",
        state: "started",
        label: "Read",
      }),
    );
    store.handleRunEvent(
      activityEvent(
        runId,
        nodeId,
        {
          id: "tool-1",
          kind: "tool",
          state: "completed",
          label: "Read",
          detail: "Read src/main.ts",
        },
        "2026-08-11T10:00:02.000Z",
      ),
    );

    const activityLines = useWorkflowStore
      .getState()
      .runLogs.filter((line) => line.activity);
    expect(activityLines).toHaveLength(1);
    expect(activityLines[0].activity).toMatchObject({
      id: "tool-1",
      state: "completed",
      detail: "Read src/main.ts",
    });
    expect(activityLines[0].at).toBe("2026-08-11T10:00:02.000Z");
  });

  test("keeps distinct activities in arrival order and scopes ids by node", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(
      runEvent("activity-workflow", runId, "started"),
    );
    store.handleRunEvent(
      activityEvent(runId, nodeId, {
        id: "shared-id",
        kind: "tool",
        state: "started",
        label: "Search",
      }),
    );
    store.handleRunEvent(
      activityEvent(runId, "other-node", {
        id: "shared-id",
        kind: "assistant",
        state: "completed",
        label: "Agent response",
      }),
    );
    store.handleRunEvent(
      activityEvent(runId, nodeId, {
        id: "second-id",
        kind: "file",
        state: "completed",
        label: "Changed src/main.ts",
      }),
    );

    expect(
      useWorkflowStore
        .getState()
        .runLogs.filter((line) => line.activity)
        .map((line) => [line.nodeId, line.activity?.id]),
    ).toEqual([
      [nodeId, "shared-id"],
      ["other-node", "shared-id"],
      [nodeId, "second-id"],
    ]);
  });

  test("keeps activity detail out of step output", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(
      runEvent("activity-workflow", runId, "started"),
    );
    store.handleRunEvent({
      ...activityEvent(runId, nodeId, {
        id: "command-1",
        kind: "command",
        state: "completed",
        label: "Ran tests",
        detail: "42 tests passed",
      }),
      output: "must not become node output",
    });

    const state = useWorkflowStore.getState();
    expect(state.stepOutputs[nodeId]).toBeUndefined();
    expect(state.runLogs.at(-1)?.output).toBeUndefined();
    expect(state.runLogs.at(-1)?.activity?.detail).toBe("42 tests passed");
  });

  test("retains only the latest 1,000 logical log rows", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(
      runEvent("activity-workflow", runId, "started"),
    );
    for (let index = 0; index < 1_005; index += 1) {
      store.handleRunEvent(
        activityEvent(runId, nodeId, {
          id: `activity-${index}`,
          kind: "status",
          state: "completed",
          label: `Activity ${index}`,
        }),
      );
    }

    const logs = useWorkflowStore.getState().runLogs;
    expect(logs).toHaveLength(1_000);
    expect(logs[0].activity?.id).toBe("activity-5");
    expect(logs.at(-1)?.activity?.id).toBe("activity-1004");
  });

  test("bounds the aggregate console text retained in memory", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(
      runEvent("activity-workflow", runId, "started"),
    );
    for (let index = 0; index < 40; index += 1) {
      store.handleRunEvent(
        activityEvent(runId, nodeId, {
          id: `large-${index}`,
          kind: "command",
          state: "completed",
          label: `Command ${index}`,
          detail: `detail-${index}-`.padEnd(40_000, "x"),
        }),
      );
    }

    const logs = useWorkflowStore.getState().runLogs;
    expect(logs.length).toBeLessThan(40);
    expect(logs.at(-1)?.activity?.id).toBe("large-39");
  });

  test("preserves normal step and run completion behavior", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(
      runEvent("activity-workflow", runId, "started"),
    );
    store.handleRunEvent(
      runEvent("activity-workflow", runId, "step_started", nodeId),
    );
    store.handleRunEvent(
      activityEvent(runId, nodeId, {
        id: "response-1",
        kind: "assistant",
        state: "completed",
        label: "Agent response",
        detail: "Work finished",
      }),
    );
    store.handleRunEvent({
      ...runEvent("activity-workflow", runId, "step_completed", nodeId),
      output: "Step result",
    });
    store.handleRunEvent({
      ...runEvent("activity-workflow", runId, "completed"),
      output: "Final result",
    });

    const state = useWorkflowStore.getState();
    expect(state.activeRun?.status).toBe("completed");
    expect(state.activeNodeId).toBeNull();
    expect(state.stepStatuses[nodeId]).toBe("completed");
    expect(state.stepOutputs[nodeId]).toBe("Step result");
    expect(state.stepOutputs.__final__).toBe("Final result");
    expect(state.runLogs.at(-1)?.kind).toBe("completed");
    expect(state.runLogs.at(-1)?.output).toBeUndefined();
    expect(
      state.runLogs.some((line) => line.activity?.id === "response-1"),
    ).toBe(true);
  });
});

describe("node-scoped live console state", () => {
  const workflowId = "console-workflow";
  const nodeId = "console-node";

  beforeEach(() => {
    const currentWorkflow = workflow(workflowId, nodeId);
    useWorkflowStore.setState({
      workflows: [currentWorkflow],
      activeWorkflowId: workflowId,
      nodes: currentWorkflow.graph.nodes,
      workflowRunStates: {},
      activeRun: null,
      activeNodeId: null,
      inspectedNodeId: null,
      stepStatuses: {},
      stepOutputs: {},
      stepStats: {},
      runLogs: [],
      runPanelOpen: false,
      selectedOutput: null,
    });
  });

  test("clears the active node immediately when a run is cancelled", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent(workflowId, "run-cancel", "started"));
    store.handleRunEvent(
      runEvent(workflowId, "run-cancel", "step_started", nodeId),
    );
    expect(useWorkflowStore.getState().activeNodeId).toBe(nodeId);

    useWorkflowStore
      .getState()
      .handleRunEvent(runEvent(workflowId, "run-cancel", "cancelled"));

    const state = useWorkflowStore.getState();
    expect(state.activeNodeId).toBeNull();
    expect(state.activeRun?.status).toBe("cancelled");
  });

  test("opens at the active node, preserves scope on close, and supports whole-run scope", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent(workflowId, "run-1", "started"));
    store.handleRunEvent(
      runEvent(workflowId, "run-1", "step_started", nodeId),
    );

    expect(useWorkflowStore.getState().runPanelOpen).toBe(false);
    store.openRunPanel();
    let state = useWorkflowStore.getState();
    expect(state.runPanelOpen).toBe(true);
    expect(state.inspectedNodeId).toBe(nodeId);
    expect(state.workflowRunStates[workflowId].inspectedNodeId).toBe(nodeId);

    state.closeRunPanel();
    state = useWorkflowStore.getState();
    expect(state.runPanelOpen).toBe(false);
    expect(state.inspectedNodeId).toBe(nodeId);

    state.handleRunEvent(runEvent(workflowId, "run-1", "completed"));
    state.openRunPanel();
    state = useWorkflowStore.getState();
    expect(state.runPanelOpen).toBe(true);
    expect(state.activeNodeId).toBeNull();
    expect(state.inspectedNodeId).toBe(nodeId);

    state.openRunPanel(null);
    state = useWorkflowStore.getState();
    expect(state.runPanelOpen).toBe(true);
    expect(state.inspectedNodeId).toBeNull();
    expect(state.workflowRunStates[workflowId].inspectedNodeId).toBeNull();
  });

  test("an explicit node filter is not replaced by a no-argument open while visible", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent(workflowId, "run-1", "started"));
    store.openRunPanel("remembered-node");
    store.openRunPanel();

    expect(useWorkflowStore.getState().inspectedNodeId).toBe(
      "remembered-node",
    );
  });

  test("a fresh started event stays closed and preserves the workflow filter", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent(workflowId, "run-1", "started"));
    store.openRunPanel(nodeId);
    store.closeRunPanel();
    store.handleRunEvent(runEvent(workflowId, "run-2", "started"));

    const state = useWorkflowStore.getState();
    expect(state.runPanelOpen).toBe(false);
    expect(state.inspectedNodeId).toBe(nodeId);
    expect(state.workflowRunStates[workflowId].inspectedNodeId).toBe(nodeId);
  });

  test("opening output keeps the panel explicit and scopes it to the output node", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent(runEvent(workflowId, "run-1", "started"));
    store.openOutput({ title: "Result", body: "Done", nodeId });

    const state = useWorkflowStore.getState();
    expect(state.runPanelOpen).toBe(true);
    expect(state.inspectedNodeId).toBe(nodeId);
    expect(state.selectedOutput).toMatchObject({ nodeId, body: "Done" });
  });
});

describe("agent authentication run events", () => {
  const workflowId = "auth-workflow";
  const nodeId = "agent-node";

  beforeEach(() => {
    const currentWorkflow = workflow(workflowId, nodeId);
    useWorkflowStore.setState({
      workflows: [{ ...currentWorkflow, name: "Auth workflow" }],
      activeWorkflowId: workflowId,
      nodes: currentWorkflow.graph.nodes,
      workflowRunStates: {},
      activeRun: null,
      activeNodeId: null,
      inspectedNodeId: null,
      stepStatuses: {},
      stepOutputs: {},
      stepStats: {},
      runLogs: [],
    });
    useToastStore.setState({ toasts: [] });
  });

  test("shows an auth toast while retaining the failed step and full log", () => {
    useWorkflowStore.getState().handleRunEvent({
      ...runEvent(workflowId, "run-auth", "step_failed", nodeId),
      nodeType: "agent",
      nodeLabel: "Codex step",
      status: "failed",
      message: "401 Unauthorized: full provider error",
      authRequired: {
        provider: "codex",
        label: "Codex",
        loginCommand: "codex login",
      },
    });

    const workflowState = useWorkflowStore.getState();
    expect(workflowState.stepStatuses[nodeId]).toBe("failed");
    expect(workflowState.stepOutputs[nodeId]).toBe(
      "401 Unauthorized: full provider error",
    );
    expect(workflowState.runLogs.at(-1)).toMatchObject({
      kind: "step_failed",
      message: "401 Unauthorized: full provider error",
    });
    expect(useToastStore.getState().toasts).toEqual([
      {
        id: "agent-auth:codex",
        provider: "codex",
        label: "Codex",
        loginCommand: "codex login",
        workflowName: "Auth workflow",
      },
    ]);
    expect(useToastStore.getState().toasts[0]).not.toHaveProperty("message");
  });

  test("does not show a toast for a generic step failure", () => {
    useWorkflowStore.getState().handleRunEvent({
      ...runEvent(workflowId, "run-generic", "step_failed", nodeId),
      message: "Model unavailable",
    });

    expect(useToastStore.getState().toasts).toEqual([]);
  });

  test("repeating the same provider replaces instead of stacking", () => {
    const store = useWorkflowStore.getState();
    store.handleRunEvent({
      ...runEvent(workflowId, "run-repeat", "step_failed", nodeId),
      message: "Not logged in",
      authRequired: {
        provider: "cursor",
        label: "Cursor",
        loginCommand: "agent login",
      },
    });
    store.handleRunEvent({
      ...runEvent(workflowId, "run-repeat", "step_failed", nodeId),
      message: "Authentication required",
      authRequired: {
        provider: "cursor",
        label: "Latest Cursor",
        loginCommand: "cursor-agent login",
      },
    });

    expect(useToastStore.getState().toasts).toEqual([
      {
        id: "agent-auth:cursor",
        provider: "cursor",
        label: "Latest Cursor",
        loginCommand: "cursor-agent login",
        workflowName: "Auth workflow",
      },
    ]);
  });
});
