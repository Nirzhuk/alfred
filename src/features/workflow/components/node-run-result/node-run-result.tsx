import { useEffect, useState } from "react";
import { useWorkflowStore } from "../../store";

export function previewText(text: string, max = 140) {
  const [firstParagraph = ""] = text.trim().split(/\n\s*\n/, 1);
  const flat = firstParagraph.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max)}…`;
}

type Props = {
  nodeId: string;
  title: string;
};

function TerminalGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="1.75"
        y="2.25"
        width="12.5"
        height="11.5"
        rx="2"
        stroke="currentColor"
        strokeWidth="1.25"
      />
      <path
        d="m4.25 6 2 2-2 2M8.25 10h3"
        stroke="currentColor"
        strokeWidth="1.25"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Live, tabbed view of the values a workflow node consumed and produced. */
export function NodeRunResult({ nodeId, title }: Props) {
  const input = useWorkflowStore((state) => state.stepInputs[nodeId]);
  const output = useWorkflowStore((state) => state.stepOutputs[nodeId]);
  const status = useWorkflowStore((state) => state.stepStatuses[nodeId]);
  const openRunPanel = useWorkflowStore((state) => state.openRunPanel);
  const openStepData = useWorkflowStore((state) => state.openStepData);
  const [tab, setTab] = useState<"input" | "output">("input");

  const running = status === "running";
  const hasInput = Boolean(input);
  const hasOutput = Boolean(output?.trim());

  useEffect(() => {
    if (!hasInput && hasOutput) setTab("output");
    if (hasInput && !hasOutput) setTab("input");
  }, [hasInput, hasOutput]);

  if (status === "skipped" || (!running && !hasInput && !hasOutput)) return null;

  if (!hasInput && !hasOutput) {
    return (
      <button
        type="button"
        className="wf-node-activity nodrag nopan"
        aria-label={`Open live activity for ${title}`}
        data-prevent-node-double-click
        onClick={(event) => {
          event.stopPropagation();
          openRunPanel(nodeId);
        }}
        onPointerDown={(event) => event.stopPropagation()}
      >
        <TerminalGlyph />
        <span>Preparing input ·</span>
        <span className="wf-node-activity-action" aria-hidden>
          Open console →
        </span>
      </button>
    );
  }

  const activeTab = tab === "input" && hasInput ? "input" : "output";
  const preview =
    activeTab === "input"
      ? input?.summary ?? "Input ready"
      : hasOutput
        ? previewText(output)
        : running
          ? "Working…"
          : status === "failed"
            ? "Failed before producing output"
            : "No output";

  const openDetail = () => {
    if (activeTab === "input" && !input) return;
    if (activeTab === "output" && !hasOutput) return;
    openStepData({
      title,
      nodeId,
      input,
      output,
      initialTab: activeTab,
    });
  };

  const onTabKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (!hasInput || !hasOutput) return;
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    setTab(activeTab === "input" ? "output" : "input");
  };

  return (
    <div
      className="wf-node-result nodrag nopan"
      data-prevent-node-double-click
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="wf-node-result-tabs" role="tablist" aria-label={`${title} run data`}>
        {hasInput ? (
          <button
            type="button"
            role="tab"
            id={`node-${nodeId}-input-tab`}
            aria-selected={activeTab === "input"}
            aria-controls={`node-${nodeId}-result-panel`}
            className={activeTab === "input" ? "is-active" : ""}
            onKeyDown={onTabKeyDown}
            onClick={(event) => {
              event.stopPropagation();
              setTab("input");
            }}
          >
            Input
          </button>
        ) : null}
        <button
          type="button"
          role="tab"
          id={`node-${nodeId}-output-tab`}
          aria-selected={activeTab === "output"}
          aria-controls={`node-${nodeId}-result-panel`}
          aria-disabled={!hasOutput}
          className={activeTab === "output" ? "is-active" : ""}
          onKeyDown={onTabKeyDown}
          onClick={(event) => {
            event.stopPropagation();
            if (hasOutput) setTab("output");
          }}
        >
          Output{!hasOutput && running ? " · Working…" : ""}
        </button>
      </div>
      <div
        role="tabpanel"
        id={`node-${nodeId}-result-panel`}
        aria-labelledby={`node-${nodeId}-${activeTab}-tab`}
      >
        <button
          type="button"
          className="wf-node-result-preview"
          disabled={activeTab === "output" && !hasOutput}
          onClick={(event) => {
            event.stopPropagation();
            openDetail();
          }}
        >
          <span>{preview}</span>
          {(activeTab === "input" && hasInput) ||
          (activeTab === "output" && hasOutput) ? (
            <span className="run-output-action">View full {activeTab} →</span>
          ) : null}
        </button>
      </div>
      {running ? (
        <button
          type="button"
          className="wf-node-result-console"
          onClick={(event) => {
            event.stopPropagation();
            openRunPanel(nodeId);
          }}
        >
          <TerminalGlyph />
          Open console
        </button>
      ) : null}
    </div>
  );
}
