import {
  ContextMenu,
  MenuItem,
  MenuLabel,
  MenuSeparator,
  useContextMenuClose,
} from "../../../../components/menu";
import {
  AddStepMenuItems,
  type AddStepMenuHandlers,
} from "../add-step-menu-items";

export type FlowContextMenuState =
  | {
      kind: "pane";
      /** Screen coordinates for placing the menu */
      screen: { x: number; y: number };
      /** Flow coordinates where the new node should appear */
      flow: { x: number; y: number };
    }
  | {
      kind: "node";
      screen: { x: number; y: number };
      nodeId: string;
      label: string;
      blockable: boolean;
      blocked: boolean;
    };

type Props = AddStepMenuHandlers & {
  menu: FlowContextMenuState;
  onClose: () => void;
  onDuplicateNode: (nodeId: string) => void;
  onDisconnectNode: (nodeId: string) => void;
  onRemoveNode: (nodeId: string) => void;
  onEditNode: (nodeId: string) => void;
  onToggleNodeBlocked: (nodeId: string, blocked: boolean) => void;
};

function NodeMenu({
  nodeId,
  label,
  blockable,
  blocked,
  onDuplicateNode,
  onDisconnectNode,
  onRemoveNode,
  onEditNode,
  onToggleNodeBlocked,
}: {
  nodeId: string;
  label: string;
  blockable: boolean;
  blocked: boolean;
  onDuplicateNode: (nodeId: string) => void;
  onDisconnectNode: (nodeId: string) => void;
  onRemoveNode: (nodeId: string) => void;
  onEditNode: (nodeId: string) => void;
  onToggleNodeBlocked: (nodeId: string, blocked: boolean) => void;
}) {
  const close = useContextMenuClose();
  return (
    <>
      <MenuLabel>{label || "Step"}</MenuLabel>
      <MenuItem
        onSelect={() => {
          onEditNode(nodeId);
          close();
        }}
      >
        Edit…
      </MenuItem>
      {blockable ? (
        <MenuItem
          onSelect={() => {
            onToggleNodeBlocked(nodeId, !blocked);
            close();
          }}
        >
          {blocked ? "Unblock editing" : "Block editing"}
        </MenuItem>
      ) : null}
      <MenuItem
        onSelect={() => {
          onDuplicateNode(nodeId);
          close();
        }}
      >
        Duplicate
      </MenuItem>
      <MenuItem
        onSelect={() => {
          onDisconnectNode(nodeId);
          close();
        }}
      >
        Disconnect edges
      </MenuItem>
      <MenuSeparator />
      <MenuItem
        danger
        onSelect={() => {
          onRemoveNode(nodeId);
          close();
        }}
      >
        Remove
      </MenuItem>
    </>
  );
}

function PaneMenu({
  flow,
  onAddPrompt,
  onAddAgent,
  onAddChoose,
  onAddMemory,
  onAddStep,
}: AddStepMenuHandlers & {
  flow: { x: number; y: number };
}) {
  const close = useContextMenuClose();
  return (
    <AddStepMenuItems
      onAddPrompt={onAddPrompt}
      onAddAgent={onAddAgent}
      onAddChoose={onAddChoose}
      onAddMemory={onAddMemory}
      onAddStep={onAddStep}
      getPosition={() => flow}
      close={close}
    />
  );
}

export function FlowContextMenu({
  menu,
  onClose,
  onAddPrompt,
  onAddAgent,
  onAddChoose,
  onAddMemory,
  onAddStep,
  onDuplicateNode,
  onDisconnectNode,
  onRemoveNode,
  onEditNode,
  onToggleNodeBlocked,
}: Props) {
  if (menu.kind === "node") {
    return (
      <ContextMenu x={menu.screen.x} y={menu.screen.y} onClose={onClose}>
        <NodeMenu
          nodeId={menu.nodeId}
          label={menu.label}
          blockable={menu.blockable}
          blocked={menu.blocked}
          onDuplicateNode={onDuplicateNode}
          onDisconnectNode={onDisconnectNode}
          onRemoveNode={onRemoveNode}
          onEditNode={onEditNode}
          onToggleNodeBlocked={onToggleNodeBlocked}
        />
      </ContextMenu>
    );
  }

  return (
    <ContextMenu
      x={menu.screen.x}
      y={menu.screen.y}
      onClose={onClose}
      role="dialog"
      aria-label="Add step"
      className="add-step-picker-surface"
    >
      <PaneMenu
        flow={menu.flow}
        onAddPrompt={onAddPrompt}
        onAddAgent={onAddAgent}
        onAddChoose={onAddChoose}
        onAddMemory={onAddMemory}
        onAddStep={onAddStep}
      />
    </ContextMenu>
  );
}
