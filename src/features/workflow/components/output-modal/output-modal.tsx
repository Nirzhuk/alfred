import { useEffect, useState } from "react";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useWorkflowStore } from "../../store";
import { formatStatsWithSource } from "../../format-stats";
import { createHtmlReportPreview } from "../../html-report";
import type {
  ChangedFile,
  ChangedFileStatus,
  StepInputSnapshot,
} from "../../types";

export function formatStepInput(snapshot: StepInputSnapshot): string {
  if (typeof snapshot.body === "string") return snapshot.body;
  return JSON.stringify(snapshot.body, null, 2);
}

function basename(path: string) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function dirname(path: string) {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join("/") || "/";
}

// Renders a "summary\n\n{json}" body as prose above the receipt, not one raw JSON dump.
export function splitSummaryAndJson(
  body: string,
): { summary: string; json: string } | null {
  const separator = body.indexOf("\n\n");
  if (separator === -1) return null;
  const summary = body.slice(0, separator).trim();
  const json = body.slice(separator + 2).trim();
  if (!summary || summary.includes("\n")) return null;
  if (!json.startsWith("{") && !json.startsWith("[")) return null;
  try {
    JSON.parse(json);
  } catch {
    return null;
  }
  return { summary, json };
}

const STATUS_LABEL: Record<ChangedFileStatus, string> = {
  created: "New",
  modified: "Modified",
  deleted: "Deleted",
  renamed: "Renamed",
};

function FileRow({ file }: { file: ChangedFile }) {
  return (
    <li className={`output-file-row output-file-${file.status}`}>
      <span className="output-file-badge" aria-hidden />
      <span className="output-file-name" title={file.path}>
        {basename(file.path)}
      </span>
      <span className="output-file-status">{STATUS_LABEL[file.status]}</span>
      <span className="output-file-dir" title={file.path}>
        {dirname(file.path)}
      </span>
      <span className="output-file-actions">
        {file.status !== "deleted" ? (
          <button
            type="button"
            className="ghost"
            onClick={() => void openPath(file.path).catch((e) => console.error("openPath failed", e))}
          >
            Open
          </button>
        ) : null}
        <button
          type="button"
          className="ghost"
          onClick={() =>
            void revealItemInDir(file.path).catch((e) =>
              console.error("revealItemInDir failed", e),
            )
          }
        >
          Reveal
        </button>
      </span>
    </li>
  );
}

function FilesChangedSection({ files }: { files: ChangedFile[] }) {
  const [open, setOpen] = useState(files.length <= 6);

  const counts = files.reduce<Partial<Record<ChangedFileStatus, number>>>((acc, f) => {
    acc[f.status] = (acc[f.status] ?? 0) + 1;
    return acc;
  }, {});
  const summary = (Object.keys(counts) as ChangedFileStatus[])
    .map((status) => `${counts[status]} ${STATUS_LABEL[status].toLowerCase()}`)
    .join(" · ");

  return (
    <div className="output-files-section">
      <button
        type="button"
        className="output-files-toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span>
          {files.length} file{files.length === 1 ? "" : "s"} changed
          <span className="output-files-summary"> · {summary}</span>
        </span>
        <span className="output-files-chevron" aria-hidden>
          {open ? "▾" : "▸"}
        </span>
      </button>
      {open ? (
        <ul className="output-files-list">
          {files.map((f) => (
            <FileRow key={f.path} file={f} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}

export function OutputModal() {
  const selectedOutput = useWorkflowStore((s) => s.selectedOutput);
  const closeOutput = useWorkflowStore((s) => s.closeOutput);
  const stepStats = useWorkflowStore((s) => s.stepStats);
  const [copied, setCopied] = useState(false);
  const [viewSource, setViewSource] = useState(false);
  const [activeTab, setActiveTab] = useState<"input" | "output">("output");

  useEffect(() => {
    setCopied(false);
    setViewSource(false);
    setActiveTab(selectedOutput?.initialTab ?? "output");
  }, [
    selectedOutput?.body,
    selectedOutput?.initialTab,
    selectedOutput?.title,
  ]);

  if (!selectedOutput) return null;

  const hasInput = Boolean(selectedOutput.input);
  const output = selectedOutput.output ?? selectedOutput.body;
  const hasOutput = Boolean(output.trim());
  const visibleBody =
    activeTab === "input" && selectedOutput.input
      ? formatStepInput(selectedOutput.input)
      : output;
  const htmlPreview =
    activeTab === "output" ? createHtmlReportPreview(visibleBody) : null;
  const structured = htmlPreview ? null : splitSummaryAndJson(visibleBody);
  const stats =
    activeTab === "output" && selectedOutput.nodeId
      ? stepStats[selectedOutput.nodeId]
      : undefined;
  const statsLine = stats ? formatStatsWithSource(stats) : null;
  const filesChanged = stats?.filesChanged ?? [];

  const copySelected = async () => {
    try {
      await navigator.clipboard.writeText(visibleBody);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };

  return (
    <Modal
      size={htmlPreview ? "xl" : "lg"}
      className={htmlPreview ? "output-modal--html" : undefined}
      onClose={closeOutput}
      labelledBy="output-modal-title"
      describedBy="output-modal-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name={htmlPreview ? "monitor" : "terminal-window"} size={20} />
          </span>
        }
        title={selectedOutput.title}
        titleId="output-modal-title"
        description={
          hasInput && hasOutput
            ? "Step input and output"
            : activeTab === "input"
              ? "Step input"
              : htmlPreview
                ? "HTML report"
                : "Step output"
        }
        descriptionId="output-modal-description"
        actions={
          <>
            {htmlPreview ? (
              <button
                type="button"
                className="ghost"
                aria-pressed={viewSource}
                onClick={() => setViewSource((current) => !current)}
              >
                {viewSource ? "Preview" : "View source"}
              </button>
            ) : null}
            <button type="button" className="ghost" onClick={() => void copySelected()}>
              {copied ? "Copied" : "Copy"}
            </button>
            <button
              type="button"
              className="ghost modal-close-button"
              aria-label="Close"
              onClick={() => closeOutput()}
            >
              <Icon name="x" size={16} />
            </button>
          </>
        }
      >
        {statsLine ? <p className="output-modal-stats">{statsLine}</p> : null}
      </ModalHeader>
      {hasInput && hasOutput ? (
        <div
          className="output-modal-tabs"
          role="tablist"
          aria-label={`${selectedOutput.title} data`}
        >
          <button
            type="button"
            role="tab"
            aria-selected={activeTab === "input"}
            className={activeTab === "input" ? "is-active" : ""}
            onClick={() => setActiveTab("input")}
          >
            Input
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={activeTab === "output"}
            className={activeTab === "output" ? "is-active" : ""}
            onClick={() => setActiveTab("output")}
          >
            Output
          </button>
        </div>
      ) : null}
      {activeTab === "input" && selectedOutput.input?.redacted ? (
        <p className="output-modal-notice">
          Some sensitive values were redacted.
        </p>
      ) : null}
      {activeTab === "input" && selectedOutput.input?.truncated ? (
        <p className="output-modal-notice">
          Input preview was truncated to{" "}
          {selectedOutput.input.capturedBytes?.toLocaleString()} of{" "}
          {selectedOutput.input.originalBytes?.toLocaleString()} bytes.
        </p>
      ) : null}
      {filesChanged.length > 0 ? <FilesChangedSection files={filesChanged} /> : null}
      {htmlPreview && !viewSource ? (
        <iframe
          className="output-modal-html-preview"
          title={`${selectedOutput.title} preview`}
          sandbox=""
          referrerPolicy="no-referrer"
          srcDoc={htmlPreview}
        />
      ) : structured ? (
        <>
          <p className="output-modal-summary">{structured.summary}</p>
          <pre className="output-modal-body">{structured.json}</pre>
        </>
      ) : (
        <pre className="output-modal-body">{visibleBody}</pre>
      )}
    </Modal>
  );
}
