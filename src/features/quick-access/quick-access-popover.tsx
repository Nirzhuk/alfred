import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { SelectControl } from "../../components/select-control";
import * as api from "../workflow/api";
import { askFields } from "../workflow/ask-fields";
import { formatScheduleLabel } from "../workflow/schedule-label";
import type {
  ActiveRunInfo,
  RunEvent,
  ScheduleListItem,
  Workflow,
} from "../workflow/types";
import { installThemeListeners } from "../settings/theme";
import { formatQuickAccessNextRun } from "./format-next-run";
import {
  readQuickAccessEnabled,
  readQuickAccessMode,
  readQuickAccessPosition,
  saveQuickAccessPosition,
  type QuickAccessMode,
} from "./preferences";
import "./quick-access.css";

// Rust can build this window already expanded (global shortcut or tray on a
// cold start). A `quick-access://open` event would be emitted before this
// webview has registered any listener, so the state travels in the URL instead.
const BOOTED_EXPANDED =
  new URLSearchParams(window.location.search).get("expanded") === "1";

const CLOSE_DELAY_MS = 160;
const EXIT_DURATION_MS = 160;
const COMPACT_WINDOW_WIDTH = 324;
const COMPACT_WINDOW_HEIGHT = 66;

function ClockIcon() {
  return (
    <svg viewBox="0 0 18 18" fill="none" aria-hidden>
      <circle cx="9" cy="9" r="6.25" stroke="currentColor" strokeWidth="1.5" />
      <path
        d="M9 5.4v3.8l2.35 1.45"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function PlayIcon() {
  return (
    <svg viewBox="0 0 18 18" fill="none" aria-hidden>
      <path d="m6.5 4.7 7 4.3-7 4.3V4.7Z" fill="currentColor" />
    </svg>
  );
}

function ArrowIcon() {
  return (
    <svg viewBox="0 0 18 18" fill="none" aria-hidden>
      <path
        d="M5 9h8m-3-3 3 3-3 3"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ExpandIcon() {
  return (
    <svg viewBox="0 0 18 18" fill="none" aria-hidden>
      <path
        d="M7 3.75H3.75V7m7.25-3.25h3.25V7M11 14.25h3.25V11M7 14.25H3.75V11"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function GripIcon() {
  return (
    <svg viewBox="0 0 12 18" fill="currentColor" aria-hidden>
      <circle cx="3" cy="4" r="1" />
      <circle cx="9" cy="4" r="1" />
      <circle cx="3" cy="9" r="1" />
      <circle cx="9" cy="9" r="1" />
      <circle cx="3" cy="14" r="1" />
      <circle cx="9" cy="14" r="1" />
    </svg>
  );
}

export function QuickAccessPopover() {
  const [expanded, setExpanded] = useState(BOOTED_EXPANDED);
  const [mode, setMode] = useState<QuickAccessMode>(readQuickAccessMode);
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [schedules, setSchedules] = useState<ScheduleListItem[]>([]);
  const [activeRuns, setActiveRuns] = useState<ActiveRunInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [selectedWorkflowId, setSelectedWorkflowId] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const closeTimer = useRef<number | null>(null);
  const shrinkTimer = useRef<number | null>(null);
  const positionSaveTimer = useRef<number | null>(null);
  const nativeExpanded = useRef(BOOTED_EXPANDED);
  const hideAfterCollapse = useRef(false);

  const clearTimers = useCallback(() => {
    if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    if (shrinkTimer.current !== null) window.clearTimeout(shrinkTimer.current);
    closeTimer.current = null;
    shrinkTimer.current = null;
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextWorkflows, nextSchedules, nextRuns] = await Promise.all([
        api.listWorkflows(),
        api.listSchedules(),
        api.listActiveRuns(),
      ]);
      setWorkflows(nextWorkflows);
      setSchedules(nextSchedules);
      setActiveRuns(nextRuns);
      setMessage(null);
    } catch (error) {
      setMessage(`Could not refresh: ${String(error)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const expand = useCallback(async () => {
    clearTimers();
    if (nativeExpanded.current) {
      setExpanded(true);
      return;
    }
    nativeExpanded.current = true;
    try {
      await invoke("set_quick_access_expanded", {
        expanded: true,
        mode,
        position: mode === "compact" ? readQuickAccessPosition() : null,
      });
      setExpanded(true);
      void refresh();
    } catch (error) {
      nativeExpanded.current = false;
      setMessage(`Quick access unavailable: ${String(error)}`);
    }
  }, [clearTimers, mode, refresh]);

  const collapse = useCallback(() => {
    clearTimers();
    setExpanded(false);
    shrinkTimer.current = window.setTimeout(() => {
      nativeExpanded.current = false;
      const position = mode === "compact" ? readQuickAccessPosition() : null;
      if (hideAfterCollapse.current) {
        hideAfterCollapse.current = false;
        void invoke("set_quick_access_enabled", {
          enabled: false,
          mode,
          position,
        });
      } else {
        void invoke("set_quick_access_expanded", {
          expanded: false,
          mode,
          position,
        });
      }
      shrinkTimer.current = null;
    }, EXIT_DURATION_MS);
  }, [clearTimers, mode]);

  const scheduleCollapse = useCallback(() => {
    if (!nativeExpanded.current) return;
    if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(collapse, CLOSE_DELAY_MS);
  }, [collapse]);

  useEffect(() => installThemeListeners(), []);

  useEffect(() => {
    if (!BOOTED_EXPANDED) return;
    // Match what the `quick-access://open` listener does for a window that was
    // already running: if the preference is off, this window was opened only
    // for this shortcut press and should be torn down again on collapse.
    hideAfterCollapse.current = !readQuickAccessEnabled();
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (!focused && nativeExpanded.current) collapse();
      })
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      });
    return () => unlisten?.();
  }, [collapse]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const currentWindow = getCurrentWindow();
    void currentWindow
      .onMoved(({ payload }) => {
        if (mode !== "compact" || nativeExpanded.current) return;
        if (positionSaveTimer.current !== null) {
          window.clearTimeout(positionSaveTimer.current);
        }
        positionSaveTimer.current = window.setTimeout(async () => {
          positionSaveTimer.current = null;
          try {
            const [size, scaleFactor] = await Promise.all([
              currentWindow.innerSize(),
              currentWindow.scaleFactor(),
            ]);
            const logicalWidth = size.width / scaleFactor;
            const logicalHeight = size.height / scaleFactor;
            if (
              Math.abs(logicalWidth - COMPACT_WINDOW_WIDTH) > 1 ||
              Math.abs(logicalHeight - COMPACT_WINDOW_HEIGHT) > 1
            ) {
              return;
            }
            saveQuickAccessPosition({ x: payload.x, y: payload.y });
          } catch (error) {
            console.warn("Failed to save compact window position", error);
          }
        }, 120);
      })
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      });
    return () => {
      unlisten?.();
      if (positionSaveTimer.current !== null) {
        window.clearTimeout(positionSaveTimer.current);
        positionSaveTimer.current = null;
      }
    };
  }, [mode]);

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    const refreshIfExpanded = () => {
      if (nativeExpanded.current || mode === "compact") void refresh();
    };
    void listen("schedules://changed", refreshIfExpanded).then((unsub) =>
      unsubs.push(unsub),
    );
    void listen("scheduler://fired", refreshIfExpanded).then((unsub) =>
      unsubs.push(unsub),
    );
    void listen<RunEvent>("run://event", (event) => {
      if (["started", "completed", "failed"].includes(event.payload.kind)) {
        refreshIfExpanded();
      }
    }).then((unsub) => unsubs.push(unsub));
    void listen("quick-access://reset", () => {
      clearTimers();
      nativeExpanded.current = false;
      hideAfterCollapse.current = false;
      setExpanded(false);
    }).then((unsub) => unsubs.push(unsub));
    void listen("quick-access://open", () => {
      clearTimers();
      nativeExpanded.current = true;
      hideAfterCollapse.current = !readQuickAccessEnabled();
      setExpanded(true);
      void refresh();
    }).then((unsub) => unsubs.push(unsub));
    void listen<QuickAccessMode>("quick-access://mode", (event) => {
      clearTimers();
      nativeExpanded.current = false;
      setExpanded(false);
      setMode(event.payload);
      if (event.payload === "compact") void refresh();
    }).then((unsub) => unsubs.push(unsub));
    return () => {
      clearTimers();
      for (const unsub of unsubs) unsub();
    };
  }, [clearTimers, mode, refresh]);

  useEffect(() => {
    if (!expanded && mode !== "compact") return;
    if (mode === "compact") void refresh();
    const interval = window.setInterval(() => void refresh(), 30_000);
    return () => window.clearInterval(interval);
  }, [expanded, mode, refresh]);

  useEffect(() => {
    if (
      workflows.length > 0 &&
      !workflows.some((workflow) => workflow.id === selectedWorkflowId)
    ) {
      setSelectedWorkflowId(workflows[0].id);
    }
  }, [selectedWorkflowId, workflows]);

  const enabledSchedules = useMemo(
    () => schedules.filter((schedule) => schedule.enabled).slice(0, 4),
    [schedules],
  );
  const scheduleByWorkflow = useMemo(
    () =>
      new Map(
        schedules
          .filter((schedule) => schedule.enabled)
          .map((schedule) => [schedule.workflowId, schedule]),
      ),
    [schedules],
  );
  const activeWorkflowIds = useMemo(
    () => new Set(activeRuns.map((run) => run.workflowId)),
    [activeRuns],
  );
  const selectedWorkflow = useMemo(
    () => workflows.find((workflow) => workflow.id === selectedWorkflowId),
    [selectedWorkflowId, workflows],
  );

  const openTarget = async (
    target: "app" | "schedules" | "workflow",
    workflowId?: string,
  ) => {
    try {
      await invoke("open_quick_access_target", {
        target,
        workflowId: workflowId ?? null,
      });
      collapse();
    } catch (error) {
      setMessage(`Could not open Alfred: ${String(error)}`);
    }
  };

  const runWorkflow = async (workflow: Workflow) => {
    if (activeWorkflowIds.has(workflow.id)) return;
    const fields = askFields(workflow.graph?.nodes ?? []);
    if (fields.length > 0) {
      try {
        await api.openRunDialog(workflow.id);
        collapse();
      } catch (error) {
        setMessage(`Could not open the run window: ${String(error)}`);
      }
      return;
    }
    setRunningId(workflow.id);
    setMessage(`Starting ${workflow.name}…`);
    try {
      await api.runWorkflow(workflow.id);
      setActiveRuns((runs) => [
        ...runs,
        {
          runId: `starting:${workflow.id}`,
          workflowId: workflow.id,
          workflowName: workflow.name,
        },
      ]);
      setMessage(`${workflow.name} is running`);
    } catch (error) {
      setMessage(`Could not start ${workflow.name}: ${String(error)}`);
    } finally {
      setRunningId(null);
    }
  };

  return (
    <div
      className="quick-access-shell"
      data-expanded={expanded ? "true" : "false"}
      data-mode={mode}
      onPointerEnter={(event) => {
        if (mode === "hover" && event.pointerType !== "touch") void expand();
      }}
      onPointerLeave={mode === "hover" ? scheduleCollapse : undefined}
      onFocusCapture={() => {
        if (mode === "hover") void expand();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") collapse();
      }}
    >
      {mode === "hover" ? (
        <button
          type="button"
          className="quick-access-edge"
          aria-label={
            expanded ? "Hide Alfred quick access" : "Show Alfred quick access"
          }
          aria-expanded={expanded}
          onClick={() => (expanded ? collapse() : void expand())}
        />
      ) : null}

      {mode === "compact" && !expanded ? (
        <div className="quick-access-compact" aria-label="Alfred compact launcher">
          <div
            className="quick-access-compact-drag"
            title="Drag to move Alfred"
            onPointerDown={(event) => {
              if (event.button === 0) void getCurrentWindow().startDragging();
            }}
          >
            <span className="quick-access-compact-grip">
              <GripIcon />
            </span>
            <span className="quick-access-presence-dot" />
            <span>
              <strong>Alfred</strong>
              <small>Ready</small>
            </span>
          </div>
          <label className="quick-access-compact-picker">
            <span className="quick-access-visually-hidden">Workflow</span>
            <SelectControl
              density="compact"
              aria-label="Workflow to run"
              value={selectedWorkflowId}
              disabled={loading || workflows.length === 0}
              onChange={(event) => setSelectedWorkflowId(event.target.value)}
            >
              {workflows.length === 0 ? (
                <option value="">
                  {loading ? "Loading workflows…" : "No workflows"}
                </option>
              ) : null}
              {workflows.map((workflow) => (
                <option key={workflow.id} value={workflow.id}>
                  {workflow.name}
                </option>
              ))}
            </SelectControl>
          </label>
          <button
            type="button"
            className="quick-access-compact-run"
            aria-label={
              selectedWorkflow
                ? `Run ${selectedWorkflow.name}`
                : "Run selected workflow"
            }
            title="Run selected workflow"
            disabled={
              !selectedWorkflow ||
              runningId === selectedWorkflow.id ||
              activeWorkflowIds.has(selectedWorkflow.id)
            }
            onClick={() => {
              if (selectedWorkflow) void runWorkflow(selectedWorkflow);
            }}
          >
            {selectedWorkflow &&
            (runningId === selectedWorkflow.id ||
              activeWorkflowIds.has(selectedWorkflow.id)) ? (
              <span className="quick-access-running-dot" />
            ) : (
              <PlayIcon />
            )}
          </button>
          <button
            type="button"
            className="quick-access-compact-expand"
            aria-label="Expand quick access"
            aria-controls="quick-access-panel"
            aria-expanded={false}
            title="Expand quick access"
            onClick={() => void expand()}
          >
            <ExpandIcon />
          </button>
        </div>
      ) : null}

      <aside
        id="quick-access-panel"
        className="quick-access-panel"
        aria-label="Alfred quick access"
        aria-hidden={!expanded}
      >
        <header className="quick-access-header">
          <div>
            <p className="quick-access-kicker">Alfred</p>
            <h1>Quick access</h1>
          </div>
          <button
            type="button"
            className="quick-access-open-app"
            onClick={() => void openTarget("app")}
          >
            Open app
            <ArrowIcon />
          </button>
        </header>

        <div className="quick-access-content">
          <section
            className="quick-access-section"
            aria-labelledby="quick-up-next"
          >
            <div className="quick-access-section-heading">
              <h2 id="quick-up-next">Up next</h2>
              <button type="button" onClick={() => void openTarget("schedules")}>
                Manage schedules
              </button>
            </div>
            {loading && schedules.length === 0 ? (
              <p className="quick-access-empty">Loading schedules…</p>
            ) : enabledSchedules.length === 0 ? (
              <p className="quick-access-empty">No active schedules</p>
            ) : (
              <ul className="quick-access-schedule-list">
                {enabledSchedules.map((schedule) => (
                  <li key={schedule.id}>
                    <button
                      type="button"
                      className="quick-access-schedule-row"
                      onClick={() =>
                        void openTarget("workflow", schedule.workflowId)
                      }
                    >
                      <span className="quick-access-schedule-icon">
                        <ClockIcon />
                      </span>
                      <span className="quick-access-row-copy">
                        <strong>{schedule.workflowName}</strong>
                        <span>
                          {formatQuickAccessNextRun(schedule.nextRunAt)}
                          {" · "}
                          {formatScheduleLabel(schedule.cron, schedule.nextRunAt)}
                        </span>
                      </span>
                      <ArrowIcon />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section
            className="quick-access-section quick-access-workflows"
            aria-labelledby="quick-workflows"
          >
            <div className="quick-access-section-heading">
              <h2 id="quick-workflows">Workflows</h2>
              <span>{workflows.length}</span>
            </div>
            {loading && workflows.length === 0 ? (
              <p className="quick-access-empty">Loading workflows…</p>
            ) : workflows.length === 0 ? (
              <p className="quick-access-empty">No workflows yet</p>
            ) : (
              <ul className="quick-access-workflow-list">
                {workflows.map((workflow) => {
                  const schedule = scheduleByWorkflow.get(workflow.id);
                  const isRunning = activeWorkflowIds.has(workflow.id);
                  return (
                    <li key={workflow.id} className="quick-access-workflow-row">
                      <button
                        type="button"
                        className="quick-access-workflow-open"
                        onClick={() => void openTarget("workflow", workflow.id)}
                      >
                        <span className="quick-access-row-copy">
                          <strong>{workflow.name}</strong>
                          <span>
                            {isRunning
                              ? "Running now"
                              : schedule
                                ? formatScheduleLabel(
                                    schedule.cron,
                                    schedule.nextRunAt,
                                  )
                                : "Ready to run"}
                          </span>
                        </span>
                      </button>
                      <button
                        type="button"
                        className="quick-access-run"
                        aria-label={`Run ${workflow.name}`}
                        title={
                          isRunning ? "Workflow is already running" : "Run now"
                        }
                        disabled={isRunning || runningId === workflow.id}
                        onClick={() => void runWorkflow(workflow)}
                      >
                        {isRunning || runningId === workflow.id ? (
                          <span className="quick-access-running-dot" />
                        ) : (
                          <PlayIcon />
                        )}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </section>
        </div>

        <footer className="quick-access-footer">
          <span className="quick-access-presence-dot" />
          <span aria-live="polite">
            {message ?? "Schedules keep running while Alfred is hidden"}
          </span>
        </footer>
      </aside>
    </div>
  );
}
