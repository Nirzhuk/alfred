import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { SearchInput } from "../../../../components/search-input";
import { useWorkflowStore } from "../../store";
import { isAgentNodeData, type OutputMemory } from "../../types";
import { formatStats } from "../../format-stats";
import { isMemoryPromptEligible } from "../../memories";
import { AgentMark } from "../../../../components/agent-mark";

const FOLLOW_THRESHOLD_PX = 24;

function formatConsoleTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

function activityKindLabel(kind: string) {
  return kind.replace(/_/g, " ");
}

function previewText(text: string, max = 120) {
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max)}…`;
}

function formatWhen(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function MemoryCard({
  memory,
  onTogglePin,
  onRemove,
}: {
  memory: OutputMemory;
  onTogglePin: (id: string) => void;
  onRemove: (id: string) => void;
}) {
  return (
    <li
      className={`run-memory-item${memory.pinned ? " is-pinned" : ""}`}
    >
      <div className="run-memory-card">
        <div className="run-memory-controls">
          {memory.origin !== "linked" ? (
            <button
              type="button"
              className={`ghost run-memory-pin${memory.pinned ? " is-on" : ""}`}
              title={memory.pinned ? "Unpin memory" : "Pin for next run"}
              aria-label={
                memory.pinned
                  ? `Unpin ${memory.title}`
                  : `Pin ${memory.title}`
              }
              onClick={() => onTogglePin(memory.id)}
            >
              <Icon name="push-pin" size={14} className="run-memory-pin-icon" />
            </button>
          ) : null}
          <button
            type="button"
            className="ghost danger run-memory-remove"
            title={
              memory.origin === "linked" ? "Unlink memory" : "Remove memory"
            }
            aria-label={
              memory.origin === "linked"
                ? `Unlink ${memory.title}`
                : `Remove memory ${memory.title}`
            }
            onClick={() => onRemove(memory.id)}
          >
            ×
          </button>
        </div>
        <button
          type="button"
          className="run-memory-card-body"
          onClick={() =>
            window.dispatchEvent(
              new CustomEvent("alfred:open-memories", {
                detail: { memoryId: memory.id },
              }),
            )
          }
        >
          <span className="run-memory-title-row">
            <span className="run-memory-title">{memory.title}</span>
            {memory.origin === "linked" ? (
              <span className="memory-origin-badge">
                From {memory.sourceWorkflowName ?? "workflow"}
              </span>
            ) : memory.pinned ? (
              <span className="run-memory-badge">Pinned</span>
            ) : null}
          </span>
          <p className="run-memory-preview">
            {previewText(memory.body, 120)}
          </p>
          <span className="run-memory-meta">
            {memory.scopeLabel} · {memory.memoryType} ·{" "}
            {formatWhen(memory.updatedAt || memory.createdAt)}
          </span>
        </button>
      </div>
    </li>
  );
}

export function RunActivityPanel() {
  const runPanelOpen = useWorkflowStore((s) => s.runPanelOpen);
  const activeRun = useWorkflowStore((s) => s.activeRun);
  const runLogs = useWorkflowStore((s) => s.runLogs);
  const activeNodeId = useWorkflowStore((s) => s.activeNodeId);
  const inspectedNodeId = useWorkflowStore((s) => s.inspectedNodeId);
  const stepStatuses = useWorkflowStore((s) => s.stepStatuses);
  const stepInputs = useWorkflowStore((s) => s.stepInputs);
  const stepOutputs = useWorkflowStore((s) => s.stepOutputs);
  const stepStats = useWorkflowStore((s) => s.stepStats);
  const memories = useWorkflowStore((s) => s.memories);
  const closeRunPanel = useWorkflowStore((s) => s.closeRunPanel);
  const openRunPanel = useWorkflowStore((s) => s.openRunPanel);
  const cancelActiveRun = useWorkflowStore((s) => s.cancelActiveRun);
  const openOutput = useWorkflowStore((s) => s.openOutput);
  const openStepData = useWorkflowStore((s) => s.openStepData);
  const removeMemory = useWorkflowStore((s) => s.removeMemory);
  const togglePinMemory = useWorkflowStore((s) => s.togglePinMemory);
  const addNote = useWorkflowStore((s) => s.addNote);
  const nodes = useWorkflowStore((s) => s.nodes);
  const consoleRef = useRef<HTMLDivElement | null>(null);
  const followingRef = useRef(true);
  const [query, setQuery] = useState("");
  const [noteOpen, setNoteOpen] = useState(false);
  const [noteTitle, setNoteTitle] = useState("");
  const [noteBody, setNoteBody] = useState("");
  const [savingNote, setSavingNote] = useState(false);
  const [showJumpToLatest, setShowJumpToLatest] = useState(false);

  const status = activeRun?.status ?? "idle";
  const isRunning = status === "running";

  const promptMemories = memories.filter((memory) =>
    isMemoryPromptEligible(memory),
  );
  const filteredMemories = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return promptMemories;
    return promptMemories.filter(
      (m) =>
        m.title.toLowerCase().includes(q) ||
        m.body.toLowerCase().includes(q) ||
        m.kind.toLowerCase().includes(q),
    );
  }, [promptMemories, query]);

  const pinnedCount = promptMemories.filter((m) => m.pinned).length;

  const activeNode = nodes.find((n) => n.id === activeNodeId);
  const activeProvider =
    activeNode?.type === "agent" && isAgentNodeData(activeNode.data)
      ? activeNode.data.provider
      : null;
  const activeLabel =
    activeNode && "label" in activeNode.data
      ? String(activeNode.data.label)
      : activeNodeId
        ? "Working…"
        : null;

  const nodeFilterId = inspectedNodeId ?? activeNodeId;
  const inspectedNode = inspectedNodeId
    ? nodes.find((node) => node.id === inspectedNodeId)
    : null;
  const inspectedLabel = inspectedNode
    ? "label" in inspectedNode.data
      ? String(inspectedNode.data.label)
      : inspectedNode.type ?? inspectedNode.id
    : inspectedNodeId;
  const filteredRunLogs = useMemo(
    () =>
      inspectedNodeId
        ? runLogs.filter((line) => line.nodeId === inspectedNodeId)
        : runLogs,
    [runLogs, inspectedNodeId],
  );
  const latestConsoleRow = filteredRunLogs[filteredRunLogs.length - 1];
  const latestConsoleKey = [
    filteredRunLogs.length,
    latestConsoleRow?.id ?? "",
    latestConsoleRow?.at ?? "",
  ].join(":");

  useEffect(() => {
    if (!runPanelOpen) return;
    const element = consoleRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
    followingRef.current = true;
    setShowJumpToLatest(false);
  }, [inspectedNodeId, runPanelOpen]);

  useEffect(() => {
    if (!runPanelOpen || !followingRef.current) return;
    const element = consoleRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
  }, [latestConsoleKey, runPanelOpen]);

  const handleConsoleScroll = () => {
    const element = consoleRef.current;
    if (!element) return;
    const distanceFromBottom =
      element.scrollHeight - element.scrollTop - element.clientHeight;
    const following = distanceFromBottom <= FOLLOW_THRESHOLD_PX;
    if (following === followingRef.current) return;
    followingRef.current = following;
    setShowJumpToLatest(!following);
  };

  const jumpToLatest = () => {
    const element = consoleRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
    followingRef.current = true;
    setShowJumpToLatest(false);
  };

  const finalOutput = stepOutputs.__final__;
  const hasSteps = nodes.length > 0;
  const hasRunContext =
    Boolean(activeRun) || runLogs.length > 0 || Object.keys(stepStatuses).length > 0;

  const showOutput = (title: string, body: string, nodeId?: string | null) => {
    openOutput({ title, body, nodeId });
  };

  const submitNote = async () => {
    if (!noteBody.trim()) return;
    setSavingNote(true);
    try {
      await addNote(noteTitle, noteBody);
      setNoteTitle("");
      setNoteBody("");
      setNoteOpen(false);
    } finally {
      setSavingNote(false);
    }
  };

  const statusLabel =
    status === "running"
      ? "Running"
      : status === "completed"
        ? "Finished"
        : status === "failed"
          ? "Failed"
          : status === "cancelled"
            ? "Stopped"
            : hasRunContext
              ? "Last run"
              : "Idle";

  return (
    <div
      className="run-rail"
      aria-hidden={!runPanelOpen}
      inert={!runPanelOpen}
    >
      <aside
        className="run-panel"
        aria-hidden={!runPanelOpen}
        inert={!runPanelOpen}
      >
        <header className="run-panel-header">
          <div>
            <h2>Activity</h2>
            <p className={`run-status run-status-${status}`}>{statusLabel}</p>
          </div>
          <div className="run-panel-actions">
            {isRunning ? (
              <button
                type="button"
                className="ghost danger"
                onClick={() => void cancelActiveRun()}
              >
                Stop
              </button>
            ) : null}
            <button
              type="button"
              className="ghost"
              onClick={() => closeRunPanel()}
            >
              Close
            </button>
          </div>
        </header>

        <div className="run-panel-scroll">
          {activeLabel ? (
            <div
              className="run-active-card"
              aria-live="polite"
              aria-atomic="true"
            >
              {isRunning && activeProvider ? (
                <AgentMark provider={activeProvider} size={16} running />
              ) : (
                <span className="running-status-dot" aria-hidden />
              )}
              <div>
                <p className="run-section-kicker">Now running</p>
                <p className="run-active-name">{activeLabel}</p>
              </div>
            </div>
          ) : null}

          {(runLogs.length > 0 || isRunning) && (
            <section className="run-section run-section-console">
              <header className="run-console-header">
                <div>
                  <h3>
                    Console · {inspectedLabel || "Whole run"}
                  </h3>
                  <p className="run-section-desc">
                    Observable agent actions and safe output.
                  </p>
                </div>
                <div className="run-console-filters" aria-label="Console scope">
                  <button
                    type="button"
                    className={inspectedNodeId ? "is-active" : ""}
                    disabled={!nodeFilterId}
                    aria-pressed={Boolean(inspectedNodeId)}
                    onClick={() => {
                      if (nodeFilterId) openRunPanel(nodeFilterId);
                    }}
                  >
                    This node
                  </button>
                  <button
                    type="button"
                    className={inspectedNodeId ? "" : "is-active"}
                    aria-pressed={!inspectedNodeId}
                    onClick={() => openRunPanel(null)}
                  >
                    Whole run
                  </button>
                </div>
              </header>

              <div className="run-console-shell">
                <div
                  ref={consoleRef}
                  className="run-console"
                  role="log"
                  aria-live="off"
                  aria-label="Live agent activity"
                  onScroll={handleConsoleScroll}
                >
                  {filteredRunLogs.length === 0 ? (
                    <p className="muted run-console-empty">
                      {isRunning
                        ? "Waiting for activity…"
                        : "No activity for this scope."}
                    </p>
                  ) : (
                    filteredRunLogs.map((line) => {
                      const kind = line.activity?.kind ?? line.kind;
                      const summary = line.activity?.label || line.message;
                      const detail = line.activity?.detail?.trim();
                      return (
                        <article key={line.id} className="run-console-row">
                          <div className="run-console-row-main">
                            <time dateTime={line.at}>
                              {formatConsoleTime(line.at)}
                            </time>
                            <span className="run-console-kind">
                              {activityKindLabel(kind)}
                            </span>
                            <span className="run-console-summary">
                              {summary}
                            </span>
                          </div>
                          {detail ? (
                            <details className="run-console-detail">
                              <summary>Details</summary>
                              <pre>{detail}</pre>
                            </details>
                          ) : null}
                        </article>
                      );
                    })
                  )}
                  {showJumpToLatest ? (
                    <button
                      type="button"
                      className="run-console-jump"
                      onClick={jumpToLatest}
                    >
                      Jump to latest
                    </button>
                  ) : null}
                </div>
              </div>
            </section>
          )}

          {/* 1. This run — step progress only */}
          {hasSteps && hasRunContext ? (
            <section className="run-section">
              <header className="run-section-header">
                <div>
                  <h3>This run</h3>
                  <p className="run-section-desc">
                    Steps in order. Click one with output to open it.
                  </p>
                </div>
              </header>
              <div className="run-steps">
                {nodes.map((node) => {
                  const st = stepStatuses[node.id] ?? "pending";
                  const label =
                    "label" in node.data
                      ? String(node.data.label)
                      : node.type ?? node.id;
                  const output = stepOutputs[node.id];
                  const input = stepInputs[node.id];
                  const clickable = Boolean(input || output?.trim());
                  const stats = stepStats[node.id];
                  const statsLine = stats ? formatStats(stats) : null;

                  return (
                    <div key={node.id} className="run-step-block">
                      <button
                        type="button"
                        className={`run-step run-step-${st}${
                          node.id === activeNodeId ? " is-active" : ""
                        }${clickable ? " is-clickable" : ""}`}
                        disabled={!clickable}
                        onClick={() => {
                          if (!input && !output) return;
                          openStepData({
                            title: label,
                            nodeId: node.id,
                            input,
                            output,
                            initialTab: input ? "input" : "output",
                          });
                        }}
                        title={
                          clickable ? "Open step input and output" : "No run data yet"
                        }
                      >
                        <span className="run-step-dot" />
                        <span className="run-step-label">{label}</span>
                        <span className="run-step-status">
                          {clickable ? "Open" : st}
                        </span>
                      </button>
                      {statsLine ? (
                        <span className="run-step-stats">{statsLine}</span>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            </section>
          ) : null}

          {/* 2. Result — one place for the final answer */}
          {finalOutput ? (
            <section className="run-section">
              <header className="run-section-header">
                <div>
                  <h3>Result</h3>
                  <p className="run-section-desc">
                    Final answer from this run.
                  </p>
                </div>
              </header>
              <button
                type="button"
                className="run-final-output"
                onClick={() => showOutput("Final output", finalOutput)}
              >
                <span className="run-output-preview">
                  {previewText(finalOutput, 180)}
                </span>
                <span className="run-output-action">View full output →</span>
              </button>
            </section>
          ) : null}

          {/* 3. Library — durable context for future runs */}
          <section className="run-section run-section-memories">
            <header className="run-section-header">
              <div>
                <h3>Library</h3>
                <p className="run-section-desc">
                  Saved notes for later. Pin to inject on the next run
                  {pinnedCount > 0 ? ` · ${pinnedCount} pinned` : ""}.
                </p>
              </div>
              <div className="run-memories-actions">
                <button
                  type="button"
                  className="ghost run-memories-clear"
                  onClick={() =>
                    window.dispatchEvent(
                      new CustomEvent("alfred:open-memories"),
                    )
                  }
                >
                  Manage
                </button>
                <button
                  type="button"
                  className="ghost run-memories-clear"
                  onClick={() => setNoteOpen((v) => !v)}
                >
                  {noteOpen ? "Cancel" : "Add note"}
                </button>
              </div>
            </header>

            {noteOpen ? (
              <div className="run-memory-note">
                <input
                  type="text"
                  placeholder="Note title"
                  value={noteTitle}
                  onChange={(e) => setNoteTitle(e.target.value)}
                />
                <textarea
                  placeholder="Write a durable note for this workflow…"
                  value={noteBody}
                  rows={3}
                  onChange={(e) => setNoteBody(e.target.value)}
                />
                <button
                  type="button"
                  className="primary"
                  disabled={savingNote || !noteBody.trim()}
                  onClick={() => void submitNote()}
                >
                  Save note
                </button>
              </div>
            ) : null}

            {promptMemories.length > 4 ? (
              <SearchInput
                aria-label="Search library"
                containerClassName="run-memories-search"
                placeholder="Search library…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onClear={() => setQuery("")}
              />
            ) : null}

            {promptMemories.length === 0 ? (
              <p className="muted run-memories-empty">
                Nothing saved yet. Add an Output step with “Save to memories”,
                or write a note, to keep results here.
              </p>
            ) : filteredMemories.length === 0 ? (
              <p className="muted run-memories-empty">
                No memories match “{query}”.
              </p>
            ) : (
              <ul className="run-memories-list">
                {filteredMemories.map((memory) => (
                  <MemoryCard
                    key={memory.id}
                    memory={memory}
                    onTogglePin={(id) => void togglePinMemory(id)}
                    onRemove={(id) => void removeMemory(id)}
                  />
                ))}
              </ul>
            )}
          </section>

          {!hasRunContext && promptMemories.length === 0 && !isRunning ? (
            <p className="muted run-panel-empty">
              Run a workflow to see steps and results here. Pinned library
              items are injected into the next run.
            </p>
          ) : null}
        </div>
      </aside>
    </div>
  );
}
