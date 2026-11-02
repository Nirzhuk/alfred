import { useState } from "react";
import { Icon } from "../../../../components/icon";
import { fixPrompt, requestFixWithAlfred, runFailure } from "../../run-failure";
import { useWorkflowStore } from "../../store";

/** Hands the active run's failure to a new chat with Alfred. */
export function FixWithAlfredButton({ nodeId }: { nodeId?: string }) {
  const activeRun = useWorkflowStore((s) => s.activeRun);
  const runLogs = useWorkflowStore((s) => s.runLogs);
  const workflows = useWorkflowStore((s) => s.workflows);
  const failure = activeRun ? runFailure(runLogs, nodeId) : null;
  if (!activeRun || !failure) return null;
  return (
    <button
      type="button"
      className="primary run-failure-fix"
      onClick={(event) => {
        event.stopPropagation();
        requestFixWithAlfred(
          fixPrompt({
            workflowName:
              workflows.find((w) => w.id === activeRun.workflowId)?.name ??
              "Workflow",
            workflowId: activeRun.workflowId,
            runId: activeRun.id,
            failure,
          }),
        );
      }}
    >
      <Icon name="chat-text" size={14} />
      Fix with Alfred
    </button>
  );
}

/** Stays over the canvas until dismissed, so a failed run cannot go unnoticed. */
export function RunFailureBanner() {
  const activeRun = useWorkflowStore((s) => s.activeRun);
  const runLogs = useWorkflowStore((s) => s.runLogs);
  const openRunPanel = useWorkflowStore((s) => s.openRunPanel);
  const runPanelOpen = useWorkflowStore((s) => s.runPanelOpen);
  const [dismissedRunId, setDismissedRunId] = useState<string | null>(null);

  if (activeRun?.status !== "failed" || dismissedRunId === activeRun.id) {
    return null;
  }
  const failure = runFailure(runLogs);
  if (!failure) return null;

  return (
    <div
      className={`run-failure-banner${runPanelOpen ? " is-panel-open" : ""}`}
      role="alert"
    >
      <Icon name="warning-circle" size={20} className="run-failure-icon" />
      <div className="run-failure-copy">
        <p className="run-failure-title">
          {failure.label ? `“${failure.label}” failed` : "Run failed"}
        </p>
        <p className="run-failure-message" title={failure.message}>
          {failure.message}
        </p>
      </div>
      <div className="run-failure-actions">
        <FixWithAlfredButton />
        <button
          type="button"
          className="ghost"
          onClick={() => openRunPanel(failure.nodeId)}
        >
          View console
        </button>
        <button
          type="button"
          className="ghost run-failure-dismiss"
          aria-label="Dismiss error"
          onClick={() => setDismissedRunId(activeRun.id)}
        >
          ×
        </button>
      </div>
    </div>
  );
}
