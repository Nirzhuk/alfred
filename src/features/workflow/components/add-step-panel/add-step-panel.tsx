import { Panel, useReactFlow } from "@xyflow/react";
import { Icon } from "../../../../components/icon";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  useDropdownMenuClose,
} from "../../../../components/menu";
import {
  AddStepMenuItems,
  type AddStepMenuHandlers,
} from "../add-step-menu-items";

type Props = AddStepMenuHandlers;
type PanelProps = Props & {
  /** Null while there is nothing to arrange. */
  onTidy: (() => void) | null;
};

function AddStepDropdownBody(props: Props & { getPosition: () => { x: number; y: number } }) {
  const close = useDropdownMenuClose();
  return <AddStepMenuItems {...props} close={close} />;
}

export function AddStepPanel({ onTidy, ...props }: PanelProps) {
  const { screenToFlowPosition } = useReactFlow();

  const getPosition = () => {
    const canvas = document.querySelector(".react-flow");
    const rect = canvas?.getBoundingClientRect();
    const x = (rect?.left ?? 0) + (rect?.width ?? 640) * 0.42;
    const y = (rect?.top ?? 0) + (rect?.height ?? 480) * 0.38;
    return screenToFlowPosition({ x, y });
  };

  return (
    <Panel position="top-right" className="add-step-panel">
      <button
        type="button"
        className="add-step-trigger"
        disabled={!onTidy}
        onClick={() => onTidy?.()}
      >
        <Icon name="flow-arrow" size={16} />
        Tidy up
      </button>
      <DropdownMenu className="add-step-panel-inner">
        <DropdownMenuTrigger className="add-step-trigger" aria-haspopup="dialog">
          <span aria-hidden>+</span>
          Add step
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          side="bottom"
          role="dialog"
          aria-label="Add step"
          className="add-step-picker-surface"
        >
          <AddStepDropdownBody {...props} getPosition={getPosition} />
        </DropdownMenuContent>
      </DropdownMenu>
    </Panel>
  );
}
