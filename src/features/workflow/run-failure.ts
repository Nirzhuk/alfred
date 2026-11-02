import type { RunLogLine } from "./types";

/** The event Alfred's canvas turns into a new "Speak with Alfred" chat. */
export const FIX_WITH_ALFRED_EVENT = "alfred:fix-with-alfred";

const MAX_ERROR_CHARS = 4_000;

export type RunFailure = {
  nodeId: string | null;
  label: string | null;
  message: string;
};

/**
 * What went wrong, read from the run's log. Fail-fast cancels sibling branches,
 * so the first failed step is the cause; a run-level failure has no step.
 */
export function runFailure(
  runLogs: RunLogLine[],
  nodeId?: string | null,
): RunFailure | null {
  const line =
    runLogs.find(
      (row) =>
        row.kind === "step_failed" && (!nodeId || row.nodeId === nodeId),
    ) ?? runLogs.find((row) => row.kind === "failed");
  if (!line) return null;
  return {
    nodeId: line.nodeId ?? null,
    label: line.nodeLabel ?? null,
    message: line.message.trim() || "The run failed without an error message.",
  };
}

export function fixPrompt(input: {
  workflowName: string;
  workflowId: string;
  runId: string;
  failure: RunFailure;
}): string {
  const { workflowName, workflowId, runId, failure } = input;
  const error =
    failure.message.length > MAX_ERROR_CHARS
      ? `${failure.message.slice(0, MAX_ERROR_CHARS)}…`
      : failure.message;
  return [
    failure.label
      ? `Fix my workflow "${workflowName}": the step "${failure.label}" failed.`
      : `Fix my workflow "${workflowName}": the run failed.`,
    "",
    `- Workflow id: ${workflowId}`,
    `- Run id: ${runId}`,
    ...(failure.nodeId ? [`- Step node id: ${failure.nodeId}`] : []),
    "",
    "Error:",
    "```",
    error,
    "```",
    "",
    "Read the run with get_run and the workflow, tell me the cause in a sentence or two, and change the workflow if that fixes it. If the fix needs something only I can do, such as signing in, tell me exactly what to do.",
  ].join("\n");
}

export function requestFixWithAlfred(prompt: string) {
  window.dispatchEvent(
    new CustomEvent(FIX_WITH_ALFRED_EVENT, { detail: { prompt } }),
  );
}
