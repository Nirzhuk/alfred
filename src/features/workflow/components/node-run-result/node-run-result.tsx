import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useEffect, useMemo, useState } from "react";
import { Icon } from "../../../../components/icon";
import { attachmentFileName } from "../../attachments";
import { isSafeToOpen, outputFilePaths } from "../../output-files";
import { useWorkflowStore } from "../../store";
import { absolutePath, formatStepInput } from "../output-modal/output-modal";
import { FixWithAlfredButton } from "../run-failure";

export function previewText(text: string, max = 140) {
  const [firstParagraph = ""] = text.trim().split(/\n\s*\n/, 1);
  const flat = firstParagraph.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max)}…`;
}

type Tab = "input" | "output";

type Props = {
  nodeId: string;
  title: string;
  /** For steps that supply data, where what went in is what you came to see. */
  inputFirst?: boolean;
};

/** Output once there is one, unless the step is a source of input. */
export function defaultResultTab(
  hasInput: boolean,
  hasOutput: boolean,
  inputFirst: boolean,
): Tab {
  return hasInput && (inputFirst || !hasOutput) ? "input" : "output";
}

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

const MAX_NAMED_FILES = 2;

/** Named when few, counted when not. */
export function ResultFileChips({
  paths,
  onShowAll,
}: {
  paths: string[];
  onShowAll: () => void;
}) {
  if (paths.length === 0) return null;

  const open = (path: string) => {
    // Opening an executable runs it, so a path read from text only gets revealed.
    void absolutePath(path)
      .then(isSafeToOpen(path) ? openPath : revealItemInDir)
      .catch((error) => {
        console.warn("Could not open file", error);
        onShowAll();
      });
  };

  return (
    <div className="wf-node-result-files">
      {paths.length > MAX_NAMED_FILES ? (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onShowAll();
          }}
        >
          <Icon name="file" size={12} />
          <span>{paths.length} files</span>
        </button>
      ) : (
        paths.map((path) => (
          <button
            key={path}
            type="button"
            title={path}
            aria-label={`Open ${attachmentFileName(path)}`}
            onClick={(event) => {
              event.stopPropagation();
              open(path);
            }}
          >
            <Icon name="file" size={12} />
            <span>{attachmentFileName(path)}</span>
          </button>
        ))
      )}
    </div>
  );
}

/** The files behind the visible tab. */
function ResultFiles({
  nodeId,
  tab,
  inputFirst,
  onShowAll,
}: {
  nodeId: string;
  tab: Tab;
  inputFirst: boolean;
  onShowAll: () => void;
}) {
  const input = useWorkflowStore((state) => state.stepInputs[nodeId]);
  const output = useWorkflowStore((state) => state.stepOutputs[nodeId]);
  const changed = useWorkflowStore((state) => state.stepStats[nodeId]?.filesChanged);

  const paths = useMemo(() => {
    if (tab === "input") {
      // Input and File inject cards already list their files above the result.
      return input && !inputFirst ? outputFilePaths(formatStepInput(input)) : [];
    }
    const existing = (changed ?? [])
      .filter((file) => file.status !== "deleted")
      .map((file) => file.path);
    return [...new Set([...existing, ...outputFilePaths(output ?? "")])];
  }, [tab, input, inputFirst, output, changed]);

  return <ResultFileChips paths={paths} onShowAll={onShowAll} />;
}

/** Live, tabbed view of the values a workflow node consumed and produced. */
export function NodeRunResult({ nodeId, title, inputFirst = false }: Props) {
  const input = useWorkflowStore((state) => state.stepInputs[nodeId]);
  const output = useWorkflowStore((state) => state.stepOutputs[nodeId]);
  const status = useWorkflowStore((state) => state.stepStatuses[nodeId]);
  const openRunPanel = useWorkflowStore((state) => state.openRunPanel);
  const openStepData = useWorkflowStore((state) => state.openStepData);
  const [pickedTab, setPickedTab] = useState<Tab | null>(null);

  const running = status === "running";
  const hasInput = Boolean(input);
  const hasOutput = Boolean(output?.trim());

  useEffect(() => {
    if (!hasOutput) setPickedTab(null); // A new run starts from the default again.
  }, [hasOutput]);

  if (status === "failed") {
    return (
      <div
        className="wf-node-error nodrag nopan"
        role="group"
        aria-label={`${title} failed`}
        data-prevent-node-double-click
        onPointerDown={(event) => event.stopPropagation()}
      >
        <p className="wf-node-error-title">
          <Icon name="warning-circle" size={14} />
          Failed
        </p>
        <button
          type="button"
          className="wf-node-error-message"
          onClick={(event) => {
            event.stopPropagation();
            if (!hasInput && !hasOutput) {
              openRunPanel(nodeId);
              return;
            }
            openStepData({
              title,
              nodeId,
              input,
              output,
              initialTab: hasOutput ? "output" : "input",
            });
          }}
        >
          {previewText(output ?? "") || "Failed before producing output"}
        </button>
        <FixWithAlfredButton nodeId={nodeId} />
      </div>
    );
  }

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

  const activeTab =
    (pickedTab === "input" && hasInput) || (pickedTab === "output" && hasOutput)
      ? pickedTab
      : defaultResultTab(hasInput, hasOutput, inputFirst);
  const preview =
    activeTab === "input"
      ? input?.summary ?? "Input ready"
      : hasOutput
        ? previewText(output)
        : running
          ? "Working…"
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
    setPickedTab(activeTab === "input" ? "output" : "input");
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
              setPickedTab("input");
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
            if (hasOutput) setPickedTab("output");
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
        <ResultFiles
          nodeId={nodeId}
          tab={activeTab}
          inputFirst={inputFirst}
          onShowAll={openDetail}
        />
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
