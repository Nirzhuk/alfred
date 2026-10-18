import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { ReactFlowProvider } from "@xyflow/react";
import { FolderPlus } from "lucide-react";
import "@xyflow/react/dist/style.css";
import { AppTitlebar } from "../app-title-bar";
import { ConfirmDialog } from "../../../../components/confirm-dialog";
import { Icon } from "../../../../components/icon";
import { WorkflowResourcesPanel } from "../workflow-resources-panel/workflow-resources-panel";
import { openLatestDownload } from "../../../licensing/download-latest";
import { SettingsPage } from "../../../settings/components/settings-page";
import {
  SettingsSidebar,
  type SettingsSectionId,
} from "../../../settings/components/settings-sidebar";
import { SchedulesPage } from "../schedules-page";
import { SidebarFolderContext } from "../sidebar-folder-context";
import { HistoryPage } from "../history-page";
import { BuilderPage } from "../../../builder/components/builder-page";
import {
  historyNavigation,
  navigateWorkflowMemory,
  type WorkflowMemoryNavigationRequest,
} from "../history-page/history-format";
import { SidebarBottomBar } from "../sidebar-bottom-bar";
import { SidebarNav, type SidebarView } from "../sidebar-nav";
import { AgentUsageBar } from "../agent-usage-bar";
import { WorkflowContextMenu } from "../workflow-context-menu";
import { WorkflowFolderContextMenu } from "../workflow-folder-context-menu";
import { WorkflowFolderModal } from "../workflow-folder-modal";
import { WorkflowList, type ScrolledFolder } from "../workflow-list";
import { FlowEditor, type ScheduleFocusRequest } from "../flow-editor";
import {
  MemoriesInspector,
  type MemoriesInspectorMode,
} from "../memories-inspector";
import { OutputModal } from "../output-modal";
import { RunActivityPanel } from "../run-activity-panel";
import { RenameWorkflowModal } from "../rename-workflow-modal";
import { ScheduleModal } from "../schedule-modal";
import { TriggersModal } from "../triggers-modal";
import { ShareDialog } from "../../../sharing";
import { useWorkflowStore } from "../../store";
import { getWorkflowStatusLabel } from "../../status-label";
import {
  isAgentNodeData,
  isPromptNodeData,
  type AgentProviderId,
} from "../../types";
import { formatScheduleLabel } from "../../schedule-label";

function FolderGlyph() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M2.5 4.25h4.1l1.15 1.25H13.5v6.25a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1V4.25Z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function WorkflowCanvas() {
  const {
    nodes,
    dirty,
    loading,
    workflowLoading,
    error,
    activeWorkflowId,
    workflows,
    workflowFolders,
    workflowSchedules,
    schedule,
    openWorkflowIds,
    createWorkflow,
    deleteWorkflowFolder,
    moveWorkflowToFolder,
    selectWorkflow,
    closeWorkflowTab,
    deleteWorkflow,
    discardUntouchedWorkflow,
    saveActiveWorkflow,
    runActiveWorkflow,
    cancelActiveRun,
    setWorkingDirectory,
    setParallelBranches,
    loadSkills,
    loadProviderModels,
    loadAgentUsage,
    runPanelOpen,
    workflowRunStates,
    openRunPanel,
    closeRunPanel,
    openOutput,
    activeNodeId,
    stepStatuses,
    stepStats,
    agentUsage,
    usageLoading,
    activeRun,
    memories,
    runLogs,
    previewScheduleCron,
  } = useWorkflowStore();

  const activeWorkflow = workflows.find(
    (workflow) => workflow.id === activeWorkflowId,
  );
  const activeWorkingDirectory =
    activeWorkflow?.workingDirectory?.trim() ?? "";
  const parallelBranches = activeWorkflow?.parallelBranches ?? true;
  const [showWorkflowLoading, setShowWorkflowLoading] = useState(false);
  const memoryNavigationGeneration = useRef(0);
  const cancelPendingMemoryNavigation = useCallback(() => {
    memoryNavigationGeneration.current += 1;
  }, []);
  const selectWorkflowFromUi = useCallback(
    (workflowId: string) => {
      cancelPendingMemoryNavigation();
      return selectWorkflow(workflowId);
    },
    [cancelPendingMemoryNavigation, selectWorkflow],
  );
  const workflowStatusLabel = getWorkflowStatusLabel({
    activeWorkflowId,
    activeRunStatus: activeRun?.status,
    dirty,
    loading,
    workflowLoading,
    showWorkflowLoading,
  });

  const canOpenActivity =
    Boolean(activeRun) || memories.length > 0 || runLogs.length > 0;
  const activityRunning = activeRun?.status === "running";
  const activityEventCount = useMemo(
    () => runLogs.filter((line) => Boolean(line.activity)).length,
    [runLogs],
  );
  const runningProviderByWorkflowId = useMemo<
    Record<string, AgentProviderId | null>
  >(() => {
    const workflowsById = new Map(
      workflows.map((workflow) => [workflow.id, workflow]),
    );
    const providers: Record<string, AgentProviderId | null> = {};

    for (const runState of Object.values(workflowRunStates)) {
      if (runState.activeRun?.status !== "running") continue;
      const workflowId = runState.activeRun.workflowId;
      const workflow = workflowsById.get(workflowId);
      const graphNodes =
        workflowId === activeWorkflowId
          ? nodes
          : (workflow?.graph?.nodes ?? []);
      const activeNode = graphNodes.find(
        (node) => node.id === runState.activeNodeId,
      );
      providers[workflowId] =
        activeNode?.type === "agent" && isAgentNodeData(activeNode.data)
          ? activeNode.data.provider
          : null;
    }

    return providers;
  }, [activeWorkflowId, nodes, workflowRunStates, workflows]);

  const usedProviders = useMemo(() => {
    const seen = new Set<string>();
    return nodes.flatMap((node) => {
      if (!isAgentNodeData(node.data) || seen.has(node.data.provider)) return [];
      seen.add(node.data.provider);
      return [node.data.provider];
    });
  }, [nodes]);
  const usedProvidersKey = usedProviders.join(",");
  const openCodeUsageRefreshKey = useMemo(
    () =>
      Object.entries(stepStats)
        .filter(([, stats]) => stats.provider === "opencode")
        .map(
          ([nodeId, stats]) =>
            `${nodeId}:${stats.durationMs ?? ""}:${stats.totalCostUsd ?? ""}`,
        )
        .sort()
        .join("|"),
    [stepStats],
  );

  const [workflowMenu, setWorkflowMenu] = useState<{
    id: string;
    name: string;
    x: number;
    y: number;
  } | null>(null);
  const [folderMenu, setFolderMenu] = useState<{
    id: string;
    name: string;
    x: number;
    y: number;
  } | null>(null);
  const [folderModal, setFolderModal] = useState<
    | { mode: "create" }
    | { mode: "rename"; id: string; name: string }
    | null
  >(null);
  const [deleteFolderTarget, setDeleteFolderTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [renameTarget, setRenameTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [scheduleTarget, setScheduleTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  /** Focus request handed to the canvas: select the node, maybe open settings. */
  const [scheduleFocus, setScheduleFocus] = useState<ScheduleFocusRequest | null>(
    null,
  );
  const [removeScheduleTarget, setRemoveScheduleTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  /** Guidance shown next to the toolbar when a mixed deletion is refused. */
  const [canvasNotice, setCanvasNotice] = useState<string | null>(null);
  const [triggersTarget, setTriggersTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [shareWorkflowId, setShareWorkflowId] = useState<string | null>(null);
  const saveBeforeShare = useCallback(async () => {
    if (shareWorkflowId && useWorkflowStore.getState().activeWorkflowId === shareWorkflowId) {
      await saveActiveWorkflow();
    }
  }, [saveActiveWorkflow, shareWorkflowId]);
  const [memoriesOpen, setMemoriesOpen] = useState(false);
  const [memoriesFocusId, setMemoriesFocusId] = useState<string | null>(null);
  const [memoriesMode, setMemoriesMode] =
    useState<MemoriesInspectorMode>("memories");
  const [suggestionsFocusRunId, setSuggestionsFocusRunId] = useState<
    string | null
  >(null);
  const [historyRunId, setHistoryRunId] = useState<string | null>(null);
  const [runPanelMounted, setRunPanelMounted] = useState(runPanelOpen);
  const [view, setSidebarView] = useState<SidebarView>("canvas");
  const setView = useCallback((nextView: SidebarView) => {
    if (nextView !== "canvas") {
      const id = useWorkflowStore.getState().activeWorkflowId;
      if (id) void discardUntouchedWorkflow(id);
    }
    setSidebarView(nextView);
  }, [discardUntouchedWorkflow]);
  const [settingsSection, setSettingsSection] =
    useState<SettingsSectionId>("general");
  const [schedulesTick, setSchedulesTick] = useState(0);
  const [scrolledFolder, setScrolledFolder] = useState<ScrolledFolder | null>(
    null,
  );
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    try {
      return localStorage.getItem("alfred:sidebar-collapsed") === "1";
    } catch {
      return false;
    }
  });
  const [resourcesOpen, setResourcesOpen] = useState(() => {
    try {
      return localStorage.getItem("alfred:resources-open") === "1";
    } catch {
      return false;
    }
  });
  const toggleResources = (open: boolean) => {
    setResourcesOpen(open);
    try {
      localStorage.setItem("alfred:resources-open", open ? "1" : "0");
    } catch {
      // The panel still works when preferences cannot be saved.
    }
  };

  const navigateHistory = (
    action: Parameters<typeof historyNavigation>[0],
  ) => {
    cancelPendingMemoryNavigation();
    const next = historyNavigation(action);
    setHistoryRunId(next.runId);
    setView(next.view);
  };

  const setSidebarOpen = (open: boolean) => {
    const collapsed = !open;
    setSidebarCollapsed(collapsed);
    try {
      localStorage.setItem("alfred:sidebar-collapsed", collapsed ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  useEffect(() => {
    if (runPanelOpen) {
      setRunPanelMounted(true);
      return;
    }
    // Keep the DOM only for the existing close transition, then release up to
    // 1,000 console rows, memory cards, and the compositor layer.
    const timeout = window.setTimeout(() => setRunPanelMounted(false), 200);
    return () => window.clearTimeout(timeout);
  }, [runPanelOpen]);

  useEffect(() => {
    if (!workflowLoading) {
      setShowWorkflowLoading(false);
      return;
    }
    const timeout = window.setTimeout(() => setShowWorkflowLoading(true), 180);
    return () => window.clearTimeout(timeout);
  }, [workflowLoading]);

  const chooseWorkingDirectory = async (workflowId: string) => {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "Choose working directory",
    });
    if (typeof picked !== "string" || !picked) return;
    await setWorkingDirectory(workflowId, picked);
  };

  const requestDelete = (id: string, name?: string) => {
    const label =
      name ?? workflows.find((w) => w.id === id)?.name ?? "this workflow";
    setDeleteTarget({ id, name: label });
  };

  useEffect(() => {
    if (!isTauri()) return;
    void loadProviderModels();
  }, [loadProviderModels]);

  useEffect(() => {
    if (!isTauri()) return;
    void loadSkills(activeWorkingDirectory || undefined);
  }, [activeWorkingDirectory, loadSkills]);

  useEffect(() => {
    if (!isTauri()) return;
    void loadAgentUsage(usedProviders);
    if (usedProviders.length === 0) return;
    const refresh = window.setInterval(
      () => void loadAgentUsage(usedProviders),
      5 * 60_000,
    );
    return () => window.clearInterval(refresh);
    // The stable key prevents a refresh loop when node objects are recreated.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [usedProvidersKey, loadAgentUsage]);

  useEffect(() => {
    if (!isTauri()) return;
    if (!openCodeUsageRefreshKey || !usedProviders.includes("opencode")) return;
    void loadAgentUsage(usedProviders);
    // Refresh local OpenCode Go history immediately after a completed step.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openCodeUsageRefreshKey, loadAgentUsage]);

  // The backend parser is the authority on a saved cron, so the node can tell
  // an invalid rule from one that simply has no next time yet.
  const activeScheduleCron =
    schedule && schedule.workflowId === activeWorkflowId ? schedule.cron : null;
  useEffect(() => {
    if (!isTauri() || !activeScheduleCron) return;
    void previewScheduleCron(activeScheduleCron);
  }, [activeScheduleCron, previewScheduleCron]);

  useEffect(() => {
    if (!canvasNotice) return;
    const timeout = window.setTimeout(() => setCanvasNotice(null), 6000);
    return () => window.clearTimeout(timeout);
  }, [canvasNotice]);

  useEffect(() => {
    const openSchedule = () => {
      cancelPendingMemoryNavigation();
      const { activeWorkflowId: id, workflows: list } =
        useWorkflowStore.getState();
      if (!id) {
        window.alert("Select a workflow first.");
        return;
      }
      const name = list.find((w) => w.id === id)?.name ?? "Workflow";
      setScheduleTarget({ id, name });
    };
    const removeSchedule = () => {
      const { activeWorkflowId: id, workflows: list, schedule: active } =
        useWorkflowStore.getState();
      if (!id || !active) return;
      const name = list.find((w) => w.id === id)?.name ?? "this workflow";
      setRemoveScheduleTarget({ id, name });
    };
    const scheduleDeleteBlocked = () => {
      setCanvasNotice(
        "Remove the schedule on its own first, then delete the selected steps.",
      );
    };
    const openDelete = () => {
      cancelPendingMemoryNavigation();
      const { activeWorkflowId: id, workflows: list } =
        useWorkflowStore.getState();
      if (!id) {
        window.alert("Select a workflow first.");
        return;
      }
      const name = list.find((w) => w.id === id)?.name ?? "this workflow";
      setDeleteTarget({ id, name });
    };
    const openMemories = (event: Event) => {
      const detail =
        (event as CustomEvent<WorkflowMemoryNavigationRequest>).detail ?? {};
      const generation = ++memoryNavigationGeneration.current;
      void navigateWorkflowMemory(detail, {
        activeWorkflowId: () =>
          useWorkflowStore.getState().activeWorkflowId,
        loadWorkflowContext: (workflowId) =>
          useWorkflowStore
            .getState()
            .ensureWorkflowMemoryContext(workflowId),
        isCurrent: () => generation === memoryNavigationGeneration.current,
        openMemory: (memoryId) => {
          setMemoriesFocusId(memoryId);
          setMemoriesMode("memories");
          setSuggestionsFocusRunId(null);
          setMemoriesOpen(true);
        },
      });
    };
    const openSuggestions = (event: Event) => {
      cancelPendingMemoryNavigation();
      const runId =
        (event as CustomEvent<{ runId?: string }>).detail?.runId ?? null;
      setMemoriesFocusId(null);
      setMemoriesMode("suggestions");
      setSuggestionsFocusRunId(runId);
      setMemoriesOpen(true);
    };
    const openSettings = (event: Event) => {
      cancelPendingMemoryNavigation();
      const section = (
        event as CustomEvent<{ section?: SettingsSectionId }>
      ).detail?.section;
      if (section) setSettingsSection(section);
      setView("settings");
      setMemoriesOpen(false);
    };
    const openSchedules = () => {
      cancelPendingMemoryNavigation();
      setView("schedules");
      setSchedulesTick((tick) => tick + 1);
    };
    const renameWorkflow = () => {
      cancelPendingMemoryNavigation();
      const { activeWorkflowId: id, workflows: list } =
        useWorkflowStore.getState();
      if (!id) {
        window.alert("Select a workflow first.");
        return;
      }
      const name =
        list.find((workflow) => workflow.id === id)?.name ?? "Workflow";
      setRenameTarget({ id, name });
    };
    const toggleSidebar = () => {
      setSidebarCollapsed((collapsed) => {
        const next = !collapsed;
        try {
          localStorage.setItem(
            "alfred:sidebar-collapsed",
            next ? "1" : "0",
          );
        } catch {
          /* ignore */
        }
        return next;
      });
    };
    const toggleActivity = () => {
      setView("canvas");
      const state = useWorkflowStore.getState();
      if (state.runPanelOpen) {
        state.closeRunPanel();
      } else if (
        state.activeRun ||
        state.memories.length > 0 ||
        state.runLogs.length > 0
      ) {
        state.openRunPanel();
      } else {
        window.alert("Run a workflow first to open the activity panel.");
      }
    };
    const downloadLatest = () => {
      void openLatestDownload();
    };
    const openWorkflow = (event: Event) => {
      const workflowId = (event as CustomEvent<{ workflowId?: string }>).detail
        ?.workflowId;
      if (!workflowId) return;
      setView("canvas");
      void selectWorkflowFromUi(workflowId);
    };
    const openActivity = () => {
      cancelPendingMemoryNavigation();
      setView("canvas");
      openRunPanel(null);
    };
    const openRunOutput = (event: Event) => {
      const detail = (
        event as CustomEvent<{
          workflowId?: string;
          title?: string;
          body?: string;
        }>
      ).detail;
      if (!detail?.workflowId || !detail.body) return;
      setView("canvas");
      setMemoriesOpen(false);
      void (async () => {
        await selectWorkflowFromUi(detail.workflowId!);
        openOutput({
          title: detail.title || "Final output",
          body: detail.body!,
          nodeId: null,
        });
      })();
    };
    const openShare = (event: Event) => {
      const requestedId = (event as CustomEvent<{ workflowId?: string }>).detail?.workflowId;
      const id = requestedId ?? useWorkflowStore.getState().activeWorkflowId;
      if (!id) {
        window.alert("Select a workflow first.");
        return;
      }
      setView("canvas");
      if (requestedId && requestedId !== useWorkflowStore.getState().activeWorkflowId) {
        void selectWorkflowFromUi(requestedId).then(() =>
          setShareWorkflowId(requestedId),
        );
      } else {
        cancelPendingMemoryNavigation();
        setShareWorkflowId(id);
      }
    };
    window.addEventListener("alfred:open-schedule", openSchedule);
    window.addEventListener("alfred:remove-schedule", removeSchedule);
    window.addEventListener(
      "alfred:schedule-delete-blocked",
      scheduleDeleteBlocked,
    );
    window.addEventListener("alfred:delete-workflow", openDelete);
    window.addEventListener("alfred:open-memories", openMemories);
    window.addEventListener("alfred:open-suggestions", openSuggestions);
    window.addEventListener("alfred:open-settings", openSettings);
    window.addEventListener("alfred:open-schedules", openSchedules);
    window.addEventListener("alfred:rename-workflow", renameWorkflow);
    window.addEventListener("alfred:toggle-sidebar", toggleSidebar);
    window.addEventListener("alfred:toggle-activity", toggleActivity);
    window.addEventListener("alfred:download-latest", downloadLatest);
    window.addEventListener("alfred:open-workflow", openWorkflow);
    window.addEventListener("alfred:open-activity", openActivity);
    window.addEventListener("alfred:open-run-output", openRunOutput);
    window.addEventListener("alfred:share-workflow", openShare);
    return () => {
      memoryNavigationGeneration.current += 1;
      window.removeEventListener("alfred:open-schedule", openSchedule);
      window.removeEventListener("alfred:remove-schedule", removeSchedule);
      window.removeEventListener(
        "alfred:schedule-delete-blocked",
        scheduleDeleteBlocked,
      );
      window.removeEventListener("alfred:delete-workflow", openDelete);
      window.removeEventListener("alfred:open-memories", openMemories);
      window.removeEventListener("alfred:open-suggestions", openSuggestions);
      window.removeEventListener("alfred:open-settings", openSettings);
      window.removeEventListener("alfred:open-schedules", openSchedules);
      window.removeEventListener("alfred:rename-workflow", renameWorkflow);
      window.removeEventListener("alfred:toggle-sidebar", toggleSidebar);
      window.removeEventListener("alfred:toggle-activity", toggleActivity);
      window.removeEventListener("alfred:download-latest", downloadLatest);
      window.removeEventListener("alfred:open-workflow", openWorkflow);
      window.removeEventListener("alfred:open-activity", openActivity);
      window.removeEventListener("alfred:open-run-output", openRunOutput);
      window.removeEventListener("alfred:share-workflow", openShare);
    };
  }, []);

  const openTabs = useMemo(() => {
    const byId = new Map(workflows.map((w) => [w.id, w]));
    const scheduleByWorkflow = new Map(
      workflowSchedules
        .filter((item) => item.enabled)
        .map((item) => [
          item.workflowId,
          formatScheduleLabel(item.cron, item.nextRunAt),
        ]),
    );
    return openWorkflowIds
      .map((id) => byId.get(id))
      .filter((w): w is NonNullable<typeof w> => Boolean(w))
      .map((w) => ({
        id: w.id,
        name: w.name,
        dirty: w.id === activeWorkflowId ? dirty : false,
        scheduleLabel: scheduleByWorkflow.get(w.id),
      }));
  }, [openWorkflowIds, workflows, workflowSchedules, activeWorkflowId, dirty]);

  const displayNodes = useMemo(() => {
    return nodes.map((node) => {
      const status = stepStatuses[node.id];
      const isActive = node.id === activeNodeId;
      const isBlocked =
        isPromptNodeData(node.data) && Boolean(node.data.blocked);
      const className = [
        node.className,
        status && (status !== "running" || isActive)
          ? `rf-node-${status}`
          : "",
        isActive ? "rf-node-active" : "",
        isBlocked ? "rf-node-blocked" : "",
      ]
        .filter(Boolean)
        .join(" ");
      return {
        ...node,
        className,
        draggable: isBlocked ? false : node.draggable,
      };
    });
  }, [nodes, stepStatuses, activeNodeId]);

  return (
    <div className="app-frame">
      <AppTitlebar
        tabs={openTabs}
        activeWorkflowId={activeWorkflowId}
        sidebarCollapsed={sidebarCollapsed}
        activityOpen={runPanelOpen}
        activityEnabled={canOpenActivity}
        activityRunning={activityRunning}
        activityEventCount={activityEventCount}
        onToggleSidebar={() => setSidebarOpen(sidebarCollapsed)}
        onToggleActivity={() => {
          if (runPanelOpen) closeRunPanel();
          else openRunPanel(null);
        }}
        onSelectTab={(id) => void selectWorkflowFromUi(id)}
        onCloseTab={(id) => {
          cancelPendingMemoryNavigation();
          void closeWorkflowTab(id);
        }}
        onNewTab={() => {
          cancelPendingMemoryNavigation();
          setView("canvas");
          void createWorkflow();
        }}
        onRenameTab={(id, name) => setRenameTarget({ id, name })}
      />

      <div
        className={[
          "app-shell",
          sidebarCollapsed ? "sidebar-collapsed" : "",
          runPanelOpen ? "" : "run-collapsed",
        ]
          .filter(Boolean)
          .join(" ")}
      >
        <div className="sidebar-rail">
          <aside className="sidebar" aria-hidden={sidebarCollapsed}>
            {view === "settings" ? (
              <SettingsSidebar
                activeSection={settingsSection}
                onChange={setSettingsSection}
                onBack={() => setView("canvas")}
              />
            ) : (
              <>
                <SidebarNav
                  view={view}
                  activityOpen={runPanelOpen && view === "canvas"}
                  activityEnabled={canOpenActivity}
                  memoriesOpen={memoriesOpen}
                  memoriesEnabled={Boolean(activeWorkflowId)}
                  onChange={(nextView) => {
                    if (nextView === "history") {
                      navigateHistory({ type: "open-history" });
                    } else {
                      setView(nextView);
                    }
                  }}
                  onNewWorkflow={() => {
                    setView("canvas");
                    void createWorkflow();
                  }}
                  onToggleActivity={() => {
                    setView("canvas");
                    if (runPanelOpen) closeRunPanel();
                    else openRunPanel();
                  }}
                  onOpenMemories={() => {
                    setView("canvas");
                    setMemoriesFocusId(null);
                    setMemoriesOpen(true);
                  }}
                  onOpenConnectedApps={() => {
                    setSettingsSection("connected-apps");
                    setView("settings");
                  }}
                  onOpenSettings={() => {
                    setSettingsSection("general");
                    setView("settings");
                  }}
                />

                <div className="sidebar-header">
                  <div className="sidebar-header-title">
                    <h2>Workflows</h2>
                    <SidebarFolderContext folder={scrolledFolder} />
                  </div>
                  <div className="sidebar-header-actions">
                    <button
                      type="button"
                      className="ghost sidebar-header-icon sidebar-folder-add"
                      title="New folder"
                      aria-label="New folder"
                      onClick={() => setFolderModal({ mode: "create" })}
                    >
                      <FolderPlus aria-hidden="true" strokeWidth={1.75} />
                    </button>
                  </div>
                </div>

                <div className="sidebar-scroll">
                  <WorkflowList
                    workflows={workflows}
                    folders={workflowFolders}
                    activeWorkflowId={activeWorkflowId}
                    activeLiveNodes={nodes}
                    activeIsDirty={dirty}
                    schedules={workflowSchedules}
                    runningProviderByWorkflowId={runningProviderByWorkflowId}
                    onSelect={(id) => {
                      setView("canvas");
                      void selectWorkflowFromUi(id);
                    }}
                    onOpenMenu={({ id, name, x, y }) => {
                      setWorkflowMenu({ id, name, x, y });
                    }}
                    onOpenFolderMenu={({ id, name, x, y }) => {
                      setFolderMenu({ id, name, x, y });
                    }}
                    onMoveToFolder={(workflowId, folderId, beforeWorkflowId) =>
                      void moveWorkflowToFolder(
                        workflowId,
                        folderId,
                        beforeWorkflowId,
                      )
                    }
                    onScrolledFolderChange={setScrolledFolder}
                  />
                </div>
              </>
            )}

            <SidebarBottomBar />
          </aside>
        </div>

        {view === "settings" ? (
          <SettingsPage activeSection={settingsSection} />
        ) : view === "history" ? (
          <HistoryPage
            activeWorkflowId={activeWorkflowId}
            initialRunId={historyRunId}
            onSelectedRunIdChange={setHistoryRunId}
            onClose={() => navigateHistory({ type: "close-history" })}
          />
        ) : view === "builder" ? (
          <BuilderPage
            onOpenWorkflow={(id) => {
              setView("canvas");
              void selectWorkflowFromUi(id);
            }}
          />
        ) : view === "schedules" ? (
          <SchedulesPage
            key={schedulesTick}
            onClose={() => setView("canvas")}
            onOpenWorkflow={({ workflowId, scheduleId }) => {
              setView("canvas");
              void selectWorkflowFromUi(workflowId).then(() => {
                setScheduleFocus({
                  scheduleId,
                  openSettings: false,
                  token: Date.now(),
                });
              });
            }}
            onEditSchedule={({ workflowId, workflowName, scheduleId }) => {
              setView("canvas");
              void selectWorkflowFromUi(workflowId).then(() => {
                setScheduleFocus({
                  scheduleId,
                  openSettings: true,
                  token: Date.now(),
                });
                // The node may not exist for this workflow (the schedule was
                // already dealt with); the form still opens on the fallback.
                setScheduleTarget({ id: workflowId, name: workflowName });
              });
            }}
          />
        ) : (
          <>
            <div className="workflow-workspace">
              {resourcesOpen && activeWorkflowId && !workflowLoading ? (
                <WorkflowResourcesPanel
                  key={activeWorkflowId}
                  workflowId={activeWorkflowId}
                  workflowName={activeWorkflow?.name ?? "Workflow"}
                  onClose={() => {
                    toggleResources(false);
                    document.getElementById("workflow-resources-toggle")?.focus();
                  }}
                  onOpenMemories={(memoryId) => {
                    setMemoriesFocusId(memoryId ?? null);
                    setMemoriesMode("memories");
                    setSuggestionsFocusRunId(null);
                    setMemoriesOpen(true);
                  }}
                  onOpenRunHistory={(runId) => navigateHistory({ type: "open-run", runId })}
                />
              ) : null}
              <section className="canvas-pane">
                <header className="canvas-toolbar">
                  <div className="toolbar-left">
                    <button
                      id="workflow-resources-toggle"
                      type="button"
                      className="ghost resources-toggle"
                      disabled={!activeWorkflowId || workflowLoading}
                      aria-expanded={resourcesOpen && Boolean(activeWorkflowId)}
                      aria-controls="workflow-resources"
                      title={resourcesOpen ? "Close workflow resources" : "Open workflow resources"}
                      onClick={() => toggleResources(!resourcesOpen)}
                    >
                      <Icon name="sidebar-simple" size={14} />
                      Resources
                    </button>
                    <div>
                      <p className="status">
                        {workflowStatusLabel}
                      </p>
                    </div>
                    {error ? <span className="error">{error}</span> : null}
                    {canvasNotice ? (
                      <span className="canvas-notice" role="status">
                        {canvasNotice}
                      </span>
                    ) : null}
                  </div>

                  {activeWorkflowId ? (
                    <div className="toolbar-cwd">
                      <label
                        htmlFor="workflow-cwd-input"
                        className="toolbar-cwd-label"
                        title="Working directory — agent CLIs run in this folder"
                      >
                        <FolderGlyph />
                        <span>Folder</span>
                      </label>
                      <input
                        id="workflow-cwd-input"
                        type="text"
                        className="toolbar-cwd-input user-select-text"
                        spellCheck={false}
                        placeholder="/path/to/project"
                        defaultValue={activeWorkingDirectory}
                        key={`${activeWorkflowId}:${activeWorkingDirectory}`}
                        title={
                          activeWorkingDirectory || "Set a working directory"
                        }
                        onBlur={(e) => {
                          const next = e.target.value.trim();
                          if (next !== activeWorkingDirectory) {
                            void setWorkingDirectory(activeWorkflowId, next);
                          }
                        }}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            (e.target as HTMLInputElement).blur();
                          }
                        }}
                      />
                      <button
                        type="button"
                        className="ghost toolbar-cwd-browse"
                        title="Choose folder"
                        aria-label="Choose working directory"
                        onClick={() => {
                          void chooseWorkingDirectory(activeWorkflowId);
                        }}
                      >
                        …
                      </button>
                    </div>
                  ) : (
                    <div className="toolbar-cwd is-empty" aria-hidden />
                  )}

                  <div className="toolbar-right">
                    {activeWorkflowId ? (
                      <button
                        type="button"
                        className="ghost"
                        disabled={loading || workflowLoading}
                        title={
                          schedule
                            ? "Show the schedule on the canvas and edit it"
                            : "Run this workflow automatically on a cadence"
                        }
                        onClick={() => {
                          const workflowId = activeWorkflowId;
                          if (!workflowId) return;
                          if (schedule) {
                            setScheduleFocus({
                              scheduleId: schedule.id,
                              openSettings: true,
                              token: Date.now(),
                            });
                            return;
                          }
                          setScheduleTarget({
                            id: workflowId,
                            name: activeWorkflow?.name ?? "Workflow",
                          });
                        }}
                      >
                        {schedule ? "Edit schedule" : "Add schedule"}
                      </button>
                    ) : null}
                    <button
                      type="button"
                      className="ghost"
                      title={
                        parallelBranches
                          ? "Independent branches run in parallel. Switch to sequential."
                          : "Independent branches run one at a time. Switch to parallel."
                      }
                      aria-pressed={parallelBranches}
                      disabled={!activeWorkflowId || loading || workflowLoading}
                      onClick={() => {
                        if (activeWorkflowId)
                          void setParallelBranches(activeWorkflowId, !parallelBranches);
                      }}
                    >
                      {parallelBranches ? "Parallel" : "Sequential"}
                    </button>
                    <button
                      type="button"
                      className="ghost danger"
                      disabled={!activeWorkflowId || loading || workflowLoading}
                      onClick={() => {
                        if (activeWorkflowId) requestDelete(activeWorkflowId);
                      }}
                    >
                      Delete
                    </button>
                    <button
                      type="button"
                      className="ghost"
                      disabled={!activeWorkflowId || loading || workflowLoading}
                      onClick={() => void saveActiveWorkflow()}
                    >
                      Save
                    </button>
                    <button
                      type="button"
                      className="ghost"
                      disabled={!activeWorkflowId || loading || workflowLoading}
                      onClick={() => {
                        if (activeWorkflowId) setShareWorkflowId(activeWorkflowId);
                      }}
                    >
                      Share
                    </button>
                    {activeRun?.status === "running" ? (
                      <button
                        type="button"
                        className="ghost danger"
                        onClick={() => void cancelActiveRun()}
                      >
                        Stop
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="primary"
                        disabled={!activeWorkflowId || loading || workflowLoading}
                        onClick={() => void runActiveWorkflow()}
                      >
                        Run
                      </button>
                    )}
                  </div>
                </header>

                <div className="canvas">
                  <ReactFlowProvider>
                    <FlowEditor
                      displayNodes={displayNodes}
                      focusScheduleRequest={scheduleFocus}
                    />
                  </ReactFlowProvider>
                  <div className="canvas-usage-dock">
                    <AgentUsageBar
                      workflowKey={activeWorkflowId ?? "no-workflow"}
                      providers={usedProviders}
                      usage={agentUsage}
                      refreshing={usageLoading}
                      onRefresh={() => void loadAgentUsage(usedProviders)}
                    />
                  </div>
                </div>
              </section>
            </div>

            {runPanelMounted ? <RunActivityPanel /> : null}

          </>
        )}
      </div>

      <OutputModal />
      <MemoriesInspector
        open={memoriesOpen}
        initialMemoryId={memoriesFocusId}
        initialMode={memoriesMode}
        focusRunId={suggestionsFocusRunId}
        onOpenRunHistory={(runId) => {
          setMemoriesOpen(false);
          setMemoriesFocusId(null);
          navigateHistory({ type: "open-run", runId });
        }}
        onClose={() => {
          cancelPendingMemoryNavigation();
          setMemoriesOpen(false);
          setMemoriesFocusId(null);
        }}
      />

      {workflowMenu ? (
        <WorkflowContextMenu
          key={`${workflowMenu.id}-${workflowMenu.x}-${workflowMenu.y}`}
          x={workflowMenu.x}
          y={workflowMenu.y}
          workflowName={workflowMenu.name}
          workflowFolderId={
            workflows.find((workflow) => workflow.id === workflowMenu.id)
              ?.folderId ?? null
          }
          folders={workflowFolders}
          running={
            workflowRunStates[workflowMenu.id]?.activeRun?.status === "running"
          }
          onClose={() => setWorkflowMenu(null)}
          onRun={() => {
            void runActiveWorkflow(workflowMenu.id);
          }}
          onStop={() => {
            void cancelActiveRun(workflowMenu.id);
          }}
          onRename={() => {
            setRenameTarget({
              id: workflowMenu.id,
              name: workflowMenu.name,
            });
          }}
          onEditFolder={() => {
            void chooseWorkingDirectory(workflowMenu.id);
          }}
          onMoveToFolder={(folderId) => {
            void moveWorkflowToFolder(workflowMenu.id, folderId);
          }}
          onSchedule={() => {
            void selectWorkflowFromUi(workflowMenu.id);
            setScheduleTarget({
              id: workflowMenu.id,
              name: workflowMenu.name,
            });
          }}
          onTriggers={() => {
            void selectWorkflowFromUi(workflowMenu.id);
            setTriggersTarget({
              id: workflowMenu.id,
              name: workflowMenu.name,
            });
          }}
          onShare={() => {
            const id = workflowMenu.id;
            setWorkflowMenu(null);
            void selectWorkflowFromUi(id).then(() => setShareWorkflowId(id));
          }}
          onDelete={() => {
            requestDelete(workflowMenu.id, workflowMenu.name);
          }}
        />
      ) : null}

      {folderMenu ? (
        <WorkflowFolderContextMenu
          key={`${folderMenu.id}-${folderMenu.x}-${folderMenu.y}`}
          x={folderMenu.x}
          y={folderMenu.y}
          folderName={folderMenu.name}
          onClose={() => setFolderMenu(null)}
          onCreateWorkflow={() => {
            setView("canvas");
            void createWorkflow("Untitled workflow", folderMenu.id);
          }}
          onRename={() => {
            setFolderModal({
              mode: "rename",
              id: folderMenu.id,
              name: folderMenu.name,
            });
          }}
          onDelete={() => {
            setDeleteFolderTarget({ id: folderMenu.id, name: folderMenu.name });
          }}
        />
      ) : null}

      {folderModal ? (
        <WorkflowFolderModal
          folder={
            folderModal.mode === "rename"
              ? { id: folderModal.id, name: folderModal.name }
              : null
          }
          onClose={() => setFolderModal(null)}
        />
      ) : null}

      {renameTarget ? (
        <RenameWorkflowModal
          workflowId={renameTarget.id}
          workflowName={renameTarget.name}
          onClose={() => setRenameTarget(null)}
        />
      ) : null}

      {triggersTarget ? (
        <TriggersModal
          workflowId={triggersTarget.id}
          workflowName={triggersTarget.name}
          onClose={() => setTriggersTarget(null)}
        />
      ) : null}

      {scheduleTarget ? (
        <ScheduleModal
          workflowId={scheduleTarget.id}
          workflowName={scheduleTarget.name}
          onClose={() => {
            setScheduleTarget(null);
            if (view === "schedules") {
              setSchedulesTick((n) => n + 1);
            }
          }}
        />
      ) : null}

      {removeScheduleTarget ? (
        <ConfirmDialog
          title={`Remove the schedule for “${removeScheduleTarget.name}”?`}
          message="This removes the schedule only. The workflow, its steps, and its manual runs stay exactly as they are."
          confirmLabel="Remove schedule"
          danger
          onCancel={() => setRemoveScheduleTarget(null)}
          onConfirm={() => {
            const id = removeScheduleTarget.id;
            setRemoveScheduleTarget(null);
            void useWorkflowStore.getState().clearSchedule(id);
          }}
        />
      ) : null}

      {deleteTarget ? (
        <ConfirmDialog
          title={`Delete “${deleteTarget.name}”?`}
          message="This permanently removes the workflow and its schedule. This cannot be undone."
          confirmLabel="Delete workflow"
          danger
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const id = deleteTarget.id;
            setDeleteTarget(null);
            void deleteWorkflow(id);
          }}
        />
      ) : null}

      {deleteFolderTarget ? (
        <ConfirmDialog
          title={`Delete “${deleteFolderTarget.name}”?`}
          message="The folder will be removed. Its workflows will stay available under Unfiled."
          confirmLabel="Delete folder"
          danger
          onCancel={() => setDeleteFolderTarget(null)}
          onConfirm={() => {
            const id = deleteFolderTarget.id;
            setDeleteFolderTarget(null);
            void deleteWorkflowFolder(id);
          }}
        />
      ) : null}
      {shareWorkflowId ? (
        <ShareDialog
          workflowId={shareWorkflowId}
          saveBeforeShare={saveBeforeShare}
          onClose={() => setShareWorkflowId(null)}
        />
      ) : null}
    </div>
  );
}
