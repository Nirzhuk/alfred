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
import { useWorkflowStore } from "../../store";
import {
  defaultAgentNodeData,
  defaultOutputNodeData,
  edgeCondition,
  edgeConditionSummary,
  isPromptNodeData,
  titleForNodeType,
  type AgentProviderId,
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

/** Cap zoom near 1× — scaling the viewport past this softens node text. */
const MIN_ZOOM = 0.6;
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
};

export function FlowEditor({ displayNodes }: Props) {
  const {
    nodes: storeNodes,
    edges: storeEdges,
    onNodesChange,
    onEdgesChange,
    onConnect,
    addNode,
    removeNode,
    duplicateNode,
    disconnectNode,
    updateNodeData,
    setSelectedNodeId,
    providerModels,
  } = useWorkflowStore();
  const colorMode = useThemeStore((s) => s.resolved);

  const { screenToFlowPosition, setViewport, getViewport, fitView } =
    useReactFlow();
  const [menu, setMenu] = useState<FlowContextMenuState | null>(null);
  const [settingsNodeId, setSettingsNodeId] = useState<string | null>(null);
  const [conditionEdgeId, setConditionEdgeId] = useState<string | null>(null);

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

  const onNodeDoubleClick = useCallback(
    (event: React.MouseEvent, node: WorkflowNode) => {
      // Only explicitly marked controls own their double-click sequence. The
      // rest of the card, including the prompt area, still opens the editor.
      if (preventsNodeDoubleClick(event.target)) return;
      openNodeSettings(node.id);
    },
    [openNodeSettings],
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
    (event: React.MouseEvent, node: WorkflowNode) => {
      event.preventDefault();
      event.stopPropagation();
      setSelectedNodeId(node.id);
      const label =
        (typeof node.data?.label === "string" && node.data.label) ||
        titleForNodeType(node.type);
      setMenu({
        kind: "node",
        screen: { x: event.clientX, y: event.clientY },
        nodeId: node.id,
        label,
        blockable: isPromptNodeData(node.data),
        blocked: isPromptNodeData(node.data) && Boolean(node.data.blocked),
      });
    },
    [setSelectedNodeId],
  );

  const onSelectionChange = useCallback(
    ({ nodes: selected }: OnSelectionChangeParams) => {
      setSelectedNodeId(selected[0]?.id ?? null);
      closeMenu();
    },
    [setSelectedNodeId, closeMenu],
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
        nodes={displayNodes}
        edges={edges}
        onNodesChange={(changes) =>
          onNodesChange(changes as NodeChange<WorkflowNode>[])
        }
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onEdgeClick={onEdgeClick}
        onKeyDown={onFlowKeyDown}
        edgesFocusable
        onSelectionChange={onSelectionChange}
        onPaneContextMenu={onPaneContextMenu}
        onNodeContextMenu={onNodeContextMenu}
        onNodeDoubleClick={onNodeDoubleClick}
        onPaneClick={closeMenu}
        onNodeClick={closeMenu}
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
