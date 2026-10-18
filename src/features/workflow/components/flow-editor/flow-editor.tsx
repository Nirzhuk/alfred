import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Background,
  BackgroundVariant,
  ConnectionMode,
  ReactFlow,
  useReactFlow,
  type DefaultEdgeOptions,
  type NodeChange,
  type OnSelectionChangeParams,
} from "@xyflow/react";
import { useThemeStore } from "../../../settings/theme";
import { GradientConnectionLine, GradientEdge } from "./gradient-edge";
import { AddStepPanel } from "../add-step-panel";
import { NodeSettingsModal } from "../node-settings-modal";
import { EdgeConditionModal } from "../edge-condition-modal";
import {
  FlowContextMenu,
  type FlowContextMenuState,
} from "../flow-context-menu";
import { nodeTypes } from "../node-types";
import { ScheduleArrows } from "../schedule-node";
import { useWorkflowStore } from "../../store";
import {
  composeScheduleCanvas,
  isScheduleCanvasId,
  scheduleCanvasNodeId,
} from "../../schedule-canvas";
import {
  defaultAgentNodeData,
  defaultOutputNodeData,
  edgeCondition,
  edgeConditionSummary,
  isPromptNodeData,
  titleForNodeType,
  type AgentProviderId,
  type ScheduleCanvasNode,
  type WorkflowEdge,
  type WorkflowNode,
  type WorkflowNodeData,
} from "../../types";

function nodeLabelById(nodes: WorkflowNode[], nodeId: string): string {
  const label = nodes.find((node) => node.id === nodeId)?.data?.label;
  return typeof label === "string" && label.trim() ? label : nodeId;
}

function newId() {
  return crypto.randomUUID();
}

/** Everything the canvas renders: saved steps plus the schedule node. */
type CanvasNode = WorkflowNode | ScheduleCanvasNode;

/**
 * Id of a node change. An "add" change carries its node under `item`, so the
 * generic union cannot be read through `id` alone.
 */
function changeNodeId(change: NodeChange<CanvasNode>): string {
  return "id" in change ? change.id : change.item.id;
}

// Leave room for large workflows while keeping zoom capped at 1× for sharp text.
const MIN_ZOOM = 0.25;
const MAX_ZOOM = 1;

function preventsNodeDoubleClick(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    Boolean(target.closest("[data-prevent-node-double-click]"))
  );
}

const edgeTypes = { gradient: GradientEdge };

const defaultEdgeOptions: DefaultEdgeOptions = {
  type: "gradient",
  animated: false,
};

type Props = {
  displayNodes: WorkflowNode[];
  /**
   * Cross-surface navigation: select and centre the schedule node, optionally
   * opening its settings. `token` changes on every request so a repeat focus of
   * the same node still runs.
   */
  focusScheduleRequest?: ScheduleFocusRequest | null;
};

/** A focus request raised by the Schedules overview or the workflow toolbar. */
export type ScheduleFocusRequest = {
  scheduleId: string;
  openSettings: boolean;
  token: number;
};

export function FlowEditor({ displayNodes, focusScheduleRequest }: Props) {
  const {
    nodes: storeNodes,
    edges: storeEdges,
    schedule,
    scheduleLayout,
    activeWorkflowId,
    onNodesChange,
    onEdgesChange,
    onConnect,
    addNode,
    removeNode,
    duplicateNode,
    disconnectNode,
    updateNodeData,
    moveScheduleNode,
    saveScheduleLayout,
    setSelectedNodeId,
    providerModels,
  } = useWorkflowStore();
  const colorMode = useThemeStore((s) => s.resolved);

  const { screenToFlowPosition, setViewport, getViewport, fitView } =
    useReactFlow();
  const [menu, setMenu] = useState<FlowContextMenuState | null>(null);
  const [settingsNodeId, setSettingsNodeId] = useState<string | null>(null);
  const [conditionEdgeId, setConditionEdgeId] = useState<string | null>(null);
  const [scheduleSelected, setScheduleSelected] = useState(false);
  const [showScheduleHint, setShowScheduleHint] = useState(false);

  const activeSchedule =
    schedule && schedule.workflowId === activeWorkflowId ? schedule : null;
  const schedulePiece = useMemo(
    () =>
      composeScheduleCanvas({
        nodes: storeNodes,
        edges: storeEdges,
        schedule: activeSchedule,
        layout: scheduleLayout,
      }),
    [storeNodes, storeEdges, activeSchedule, scheduleLayout],
  );
  const scheduleNode = useMemo(
    () =>
      schedulePiece.node
        ? {
            ...schedulePiece.node,
            selected: scheduleSelected,
            className: showScheduleHint ? "is-hinted" : undefined,
          }
        : null,
    [schedulePiece, scheduleSelected, showScheduleHint],
  );
  const scheduleId = activeSchedule?.id ?? null;
  const renderNodes = useMemo(
    () => [...displayNodes, ...(scheduleNode ? [scheduleNode] : [])],
    [displayNodes, scheduleNode],
  );

  const edges = useMemo(
    () =>
      storeEdges.map((edge) => {
        const condition = edgeCondition(edge);
        const from = nodeLabelById(storeNodes, edge.source);
        const to = nodeLabelById(storeNodes, edge.target);
        return {
          ...edge,
          type: "gradient" as const,
          ariaLabel: condition
            ? `Connection from ${from} to ${to}, runs when ${edgeConditionSummary(condition)}`
            : `Connection from ${from} to ${to}, always runs`,
        };
      }),
    [storeEdges, storeNodes],
  );

  /** Round pan offsets and clamp zoom so nodes stay sharp. */
  const sharpenViewport = useCallback(() => {
    const { x, y, zoom } = getViewport();
    const clampedZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
    const next = {
      x: Math.round(x),
      y: Math.round(y),
      zoom: Math.round(clampedZoom * 100) / 100,
    };
    if (next.x !== x || next.y !== y || next.zoom !== zoom) {
      void setViewport(next, { duration: 0 });
    }
  }, [getViewport, setViewport]);

  const closeMenu = useCallback(() => setMenu(null), []);

  const openNodeSettings = useCallback(
    (nodeId: string) => {
      setSelectedNodeId(nodeId);
      setSettingsNodeId(nodeId);
      closeMenu();
    },
    [setSelectedNodeId, closeMenu],
  );

  // A connection is the only place a condition is authored, so a plain click on
  // it opens the editor. Keyboard users reach the same editor with Enter on a
  // selected edge.
  const openEdgeCondition = useCallback(
    (edgeId: string) => {
      setConditionEdgeId(edgeId);
      closeMenu();
    },
    [closeMenu],
  );

  const onEdgeClick = useCallback(
    (_event: React.MouseEvent, edge: WorkflowEdge) => openEdgeCondition(edge.id),
    [openEdgeCondition],
  );

  // React Flow's own edge handler selects on Enter but has no "open" hook and
  // exposes no onEdgeKeyDown prop, so the container resolves the focused edge
  // from the DOM. Selection still happens first; this only adds the open.
  const onFlowKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (event.key !== "Enter") return;
      const target = event.target as Element | null;
      const edgeId = target
        ?.closest?.(".react-flow__edge")
        ?.getAttribute("data-id");
      if (!edgeId) return;
      event.preventDefault();
      openEdgeCondition(edgeId);
    },
    [openEdgeCondition],
  );

  /** Open the schedule settings owned by the workflow canvas. */
  const openScheduleSettings = useCallback(() => {
    window.dispatchEvent(new Event("alfred:open-schedule"));
  }, []);

  const onNodeDoubleClick = useCallback(
    (event: React.MouseEvent, node: CanvasNode) => {
      if (isScheduleCanvasId(node.id)) {
        openScheduleSettings();
        return;
      }
      // Only explicitly marked controls own their double-click sequence. The
      // rest of the card, including the prompt area, still opens the editor.
      if (preventsNodeDoubleClick(event.target)) return;
      openNodeSettings(node.id);
    },
    [openNodeSettings, openScheduleSettings],
  );

  const onNodeClick = useCallback(
    (_event: React.MouseEvent, node: CanvasNode) => {
      closeMenu();
      // The schedule node has no settings modal of its own: its body opens the
      // one schedule form the rest of the app uses.
      if (isScheduleCanvasId(node.id)) openScheduleSettings();
    },
    [closeMenu, openScheduleSettings],
  );

  useEffect(() => {
    const editNode = (event: Event) => {
      const detail = (event as CustomEvent<{ workflowId: string; nodeId: string }>).detail;
      const state = useWorkflowStore.getState();
      if (!detail || detail.workflowId !== state.activeWorkflowId) return;
      if (!state.nodes.some((node) => node.id === detail.nodeId)) return;
      openNodeSettings(detail.nodeId);
      void fitView({
        nodes: [{ id: detail.nodeId }],
        padding: 0.4,
        minZoom: MIN_ZOOM,
        maxZoom: MAX_ZOOM,
        duration: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 180,
      });
    };
    window.addEventListener("alfred:edit-node", editNode);
    return () => window.removeEventListener("alfred:edit-node", editNode);
  }, [fitView, openNodeSettings]);

  // A focus request can arrive before the node is on screen (the workflow was
  // still loading), so it is applied from an effect keyed on the node id.
  useEffect(() => {
    if (!focusScheduleRequest) return;
    if (focusScheduleRequest.scheduleId !== scheduleId) return;
    setScheduleSelected(true);
    setShowScheduleHint(true);
    const timeout = window.setTimeout(() => setShowScheduleHint(false), 2400);
    void fitView({
      nodes: [{ id: scheduleCanvasNodeId(focusScheduleRequest.scheduleId) }],
      padding: 0.4,
      minZoom: MIN_ZOOM,
      maxZoom: MAX_ZOOM,
      duration: window.matchMedia("(prefers-reduced-motion: reduce)").matches
        ? 0
        : 180,
    });
    if (focusScheduleRequest.openSettings) openScheduleSettings();
    return () => window.clearTimeout(timeout);
  }, [focusScheduleRequest, scheduleId, fitView, openScheduleSettings]);

  useEffect(() => {
    if (!menu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeMenu();
    };
    const onPointer = () => closeMenu();
    window.addEventListener("keydown", onKey);
    // Close on next click anywhere (menu buttons stopPropagation).
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  }, [menu, closeMenu]);

  useEffect(() => {
    const onFitCanvas = () => {
      void fitView({
        padding: 0.2,
        minZoom: MIN_ZOOM,
        maxZoom: MAX_ZOOM,
        duration: 180,
      });
    };
    window.addEventListener("alfred:fit-canvas", onFitCanvas);
    return () =>
      window.removeEventListener("alfred:fit-canvas", onFitCanvas);
  }, [fitView]);

  const onPaneContextMenu = useCallback(
    (event: MouseEvent | React.MouseEvent) => {
      event.preventDefault();
      const screen = { x: event.clientX, y: event.clientY };
      const flow = screenToFlowPosition(screen);
      setMenu({ kind: "pane", screen, flow });
    },
    [screenToFlowPosition],
  );

  const onNodeContextMenu = useCallback(
    (event: React.MouseEvent, node: CanvasNode) => {
      event.preventDefault();
      event.stopPropagation();
      // The step menu (duplicate, disconnect, delete) has nothing to offer a
      // schedule; its own controls carry edit and remove.
      if (isScheduleCanvasId(node.id)) return;
      const step = node as WorkflowNode;
      setSelectedNodeId(step.id);
      const label =
        (typeof step.data?.label === "string" && step.data.label) ||
        titleForNodeType(step.type);
      setMenu({
        kind: "node",
        screen: { x: event.clientX, y: event.clientY },
        nodeId: step.id,
        label,
        blockable: isPromptNodeData(step.data),
        blocked: isPromptNodeData(step.data) && Boolean(step.data.blocked),
      });
    },
    [setSelectedNodeId],
  );

  const onSelectionChange = useCallback(
    ({ nodes: selected }: OnSelectionChangeParams) => {
      // Schedule selection stays in canvas UI state: it must not become the
      // step selection the settings modal and activity panel follow.
      const scheduleNodeSelected = selected.some((node) =>
        isScheduleCanvasId(node.id),
      );
      setScheduleSelected(scheduleNodeSelected);
      setSelectedNodeId(
        scheduleNodeSelected ? null : (selected[0]?.id ?? null),
      );
      closeMenu();
    },
    [setSelectedNodeId, closeMenu],
  );

  /**
   * Canvas changes, split by what they belong to. Synthetic schedule elements
   * never reach the graph handlers: a drag moves only the saved layout, a
   * delete request routes to removing the schedule, and ordinary graph edits
   * keep flowing through the same store actions as before.
   */
  const onFlowNodesChange = useCallback(
    (changes: NodeChange<CanvasNode>[]) => {
      const executable = changes.filter(
        (change) => !isScheduleCanvasId(changeNodeId(change)),
      );
      const synthetic = changes.filter((change) =>
        isScheduleCanvasId(changeNodeId(change)),
      );

      if (synthetic.length > 0) {
        const scheduleRemoved = synthetic.some(
          (change) => change.type === "remove",
        );
        const stepRemoved = executable.some(
          (change) => change.type === "remove",
        );
        if (scheduleRemoved) {
          window.dispatchEvent(
            new Event(
              stepRemoved
                ? "alfred:schedule-delete-blocked"
                : "alfred:remove-schedule",
            ),
          );
          // A mixed selection is rejected whole: neither half is deleted.
          if (stepRemoved) return;
        }
        // A drag only moves the saved layout; it never touches graph state.
        if (scheduleId) {
          for (const change of synthetic) {
            if (change.type === "position" && change.position) {
              moveScheduleNode(scheduleId, change.position);
            }
          }
        }
      }

      // Synthetic ids are gone by now, so what is left is the executable graph.
      if (executable.length > 0) {
        onNodesChange(executable as NodeChange<WorkflowNode>[]);
      }
    },
    [onNodesChange, moveScheduleNode, scheduleId],
  );

  const onNodeDragStop = useCallback(
    (_event: MouseEvent | TouchEvent, node: CanvasNode) => {
      if (!isScheduleCanvasId(node.id) || !scheduleId) return;
      // Only an intentional drag persists a position; focusing or rendering
      // the node never writes one.
      void saveScheduleLayout(scheduleId, node.position);
    },
    [saveScheduleLayout, scheduleId],
  );

  const onAddPrompt = useCallback(
    (position: { x: number; y: number }) => {
      addNode({
        id: newId(),
        type: "input",
        position,
        width: 280,
        height: 280,
        data: { label: "Input", prompt: "", attachments: [] },
      });
    },
    [addNode],
  );

  const onAddAgent = useCallback(
    (provider: AgentProviderId, position: { x: number; y: number }) => {
      const catalog = providerModels.find(
        (candidate) =>
          candidate.provider === provider && candidate.harness === "cli",
      );
      addNode({
        id: newId(),
        type: "agent",
        position,
        data: defaultAgentNodeData(provider, catalog?.defaultModel ?? null),
      });
    },
    [addNode, providerModels],
  );

  const onAddChoose = useCallback(
    (position: { x: number; y: number }) => {
      addNode({
        id: newId(),
        type: "chooseOutput",
        position,
        data: defaultOutputNodeData("Output"),
      });
    },
    [addNode],
  );

  const onAddMemory = useCallback(
    (position: { x: number; y: number }) => {
      addNode({
        id: newId(),
        type: "memory",
        position,
        data: { label: "Memories", memoryIds: [] },
      });
    },
    [addNode],
  );

  const onAddStep = useCallback(
    (
      type: string,
      data: WorkflowNodeData,
      position: { x: number; y: number },
    ) => {
      addNode({
        id: newId(),
        type,
        position,
        data: structuredClone(data),
      });
    },
    [addNode],
  );

  return (
    <>
      <ReactFlow
        nodes={renderNodes}
        edges={edges}
        onNodesChange={onFlowNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onEdgeClick={onEdgeClick}
        onKeyDown={onFlowKeyDown}
        edgesFocusable
        onSelectionChange={onSelectionChange}
        onPaneContextMenu={onPaneContextMenu}
        onNodeContextMenu={onNodeContextMenu}
        onNodeDoubleClick={onNodeDoubleClick}
        onNodeClick={onNodeClick}
        onNodeDragStop={onNodeDragStop}
        onPaneClick={closeMenu}
        onMoveStart={closeMenu}
        onMoveEnd={sharpenViewport}
        onInit={() => {
          // fitView applies after init; sharpen once the viewport settles.
          requestAnimationFrame(() => sharpenViewport());
        }}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        colorMode={colorMode}
        defaultEdgeOptions={defaultEdgeOptions}
        connectionLineComponent={GradientConnectionLine}
        connectionMode={ConnectionMode.Strict}
        fitView
        fitViewOptions={{ padding: 0.2, minZoom: MIN_ZOOM, maxZoom: MAX_ZOOM }}
        minZoom={MIN_ZOOM}
        maxZoom={MAX_ZOOM}
        snapToGrid
        snapGrid={[8, 8]}
        elevateNodesOnSelect={false}
        proOptions={{ hideAttribution: true }}
      >
        <Background
          id="canvas-dots-even"
          variant={BackgroundVariant.Dots}
          gap={[24, 26]}
          offset={[6, 6]}
          size={1.5}
          color="var(--canvas-dot)"
          bgColor="transparent"
        />
        <Background
          id="canvas-dots-odd"
          variant={BackgroundVariant.Dots}
          gap={[24, 26]}
          offset={[18, 19]}
          size={1.5}
          color="var(--canvas-dot)"
          bgColor="transparent"
        />
        {/* One generated arrow per root the schedule starts. */}
        <ScheduleArrows
          sourceId={schedulePiece.node?.id ?? null}
          rootIds={schedulePiece.rootIds}
        />
        <AddStepPanel
          onAddPrompt={onAddPrompt}
          onAddAgent={onAddAgent}
          onAddChoose={onAddChoose}
          onAddMemory={onAddMemory}
          onAddStep={onAddStep}
        />
      </ReactFlow>

      {menu ? (
        <div
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => e.stopPropagation()}
        >
          <FlowContextMenu
            menu={menu}
            onClose={closeMenu}
            onAddPrompt={onAddPrompt}
            onAddAgent={onAddAgent}
            onAddChoose={onAddChoose}
            onAddMemory={onAddMemory}
            onAddStep={onAddStep}
            onDuplicateNode={duplicateNode}
            onDisconnectNode={disconnectNode}
            onRemoveNode={removeNode}
            onEditNode={openNodeSettings}
            onToggleNodeBlocked={(nodeId, blocked) =>
              updateNodeData(nodeId, { blocked })
            }
          />
        </div>
      ) : null}

      {settingsNodeId ? (
        <NodeSettingsModal
          nodeId={settingsNodeId}
          onClose={() => setSettingsNodeId(null)}
        />
      ) : null}

      {conditionEdgeId ? (
        <EdgeConditionModal
          edgeId={conditionEdgeId}
          onClose={() => setConditionEdgeId(null)}
        />
      ) : null}
    </>
  );
}
