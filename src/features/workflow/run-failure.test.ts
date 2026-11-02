import { expect, test } from "bun:test";
import { fixPrompt, runFailure } from "./run-failure";
import type { RunLogLine } from "./types";

const line = (kind: string, message: string, nodeId?: string): RunLogLine => ({
  id: `${kind}-${nodeId ?? "run"}`,
  at: "2026-11-02T10:00:00Z",
  kind,
  nodeId,
  nodeLabel: nodeId ? `Step ${nodeId}` : null,
  message,
});

test("the first failed step is the cause, ahead of the run-level failure", () => {
  const logs = [
    line("step_started", "go", "a"),
    line("step_failed", "boom", "a"),
    line("step_failed", "second", "b"),
    line("failed", "Automation failed: boom"),
  ];
  expect(runFailure(logs)).toEqual({ nodeId: "a", label: "Step a", message: "boom" });
  expect(runFailure(logs, "b")?.message).toBe("second");
});

test("a run that fails outside any step still reports, and a clean run does not", () => {
  expect(runFailure([line("failed", "Workflow has no nodes")])).toEqual({
    nodeId: null,
    label: null,
    message: "Workflow has no nodes",
  });
  expect(runFailure([line("completed", "done")])).toBeNull();
});

test("the prompt names the ids Alfred needs and caps a huge error", () => {
  const prompt = fixPrompt({
    workflowName: "Digest",
    workflowId: "wf-1",
    runId: "run-1",
    failure: { nodeId: "a", label: "Summarize", message: "x".repeat(5_000) },
  });
  expect(prompt.startsWith('Fix my workflow "Digest": the step "Summarize" failed.')).toBe(true);
  expect(prompt).toContain("Workflow id: wf-1");
  expect(prompt).toContain("Run id: run-1");
  expect(prompt).toContain("Step node id: a");
  expect(prompt).toContain(`${"x".repeat(4_000)}…`);
  expect(prompt).not.toContain("x".repeat(4_001));
});
