import { afterEach, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { useWorkflowStore } from "../../store";
import type { RunLogLine } from "../../types";
import { FixWithAlfredButton, RunFailureBanner } from "./run-failure";

// Server rendering reads a zustand store's initial state, not its current one.
const rendered = useWorkflowStore.getInitialState();
const original = {
  activeRun: rendered.activeRun,
  runLogs: rendered.runLogs,
  runPanelOpen: rendered.runPanelOpen,
};
afterEach(() => Object.assign(rendered, original));

const failedStep: RunLogLine = {
  id: "line-1",
  at: "2026-11-02T10:00:00Z",
  kind: "step_failed",
  nodeId: "writer",
  nodeLabel: "Writer",
  message: "claude exited with code 1",
};

function setRun(status: string, runLogs: RunLogLine[] = [failedStep]) {
  Object.assign(rendered, {
    activeRun: {
      id: "run-1",
      workflowId: "wf-1",
      trigger: "manual",
      status,
      createdAt: failedStep.at,
    },
    runLogs,
  });
}

test("a failed run names the step and its error and offers the fix", () => {
  setRun("failed");
  const html = renderToStaticMarkup(<RunFailureBanner />);
  expect(html).toContain('role="alert"');
  expect(html).toContain("Writer");
  expect(html).toContain("claude exited with code 1");
  expect(html).toContain("Fix with Alfred");
  expect(html).toContain("View console");
});

test("the banner clears the Activity rail when it is open", () => {
  setRun("failed");
  rendered.runPanelOpen = true;
  expect(renderToStaticMarkup(<RunFailureBanner />)).toContain("is-panel-open");
});

test("running, cancelled, and finished runs show no banner", () => {
  for (const status of ["running", "cancelled", "completed"]) {
    setRun(status);
    expect(renderToStaticMarkup(<RunFailureBanner />)).toBe("");
  }
});

test("the fix button needs a failure to hand over", () => {
  setRun("failed", []);
  expect(renderToStaticMarkup(<FixWithAlfredButton />)).toBe("");
});
