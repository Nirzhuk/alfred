import type { ReactNode } from "react";
import {
  ContextMenu,
  MenuDescription,
  MenuItem,
  MenuLabel,
  MenuSeparator,
  MenuSub,
  MenuSubContent,
  MenuSubTrigger,
  useContextMenuClose,
} from "../../../../components/menu";
import type { WorkflowFolder } from "../../types";

type Props = {
  x: number;
  y: number;
  workflowName: string;
  workflowFolderId?: string | null;
  folders: WorkflowFolder[];
  running: boolean;
  onClose: () => void;
  onRun: () => void;
  onStop: () => void;
  onRename: () => void;
  onEditFolder: () => void;
  onMoveToFolder: (folderId: string | null) => void;
  onSchedule: () => void;
  onTriggers: () => void;
  onShare: () => void;
  onDelete: () => void;
};

function WorkflowMenuBody({
  workflowName,
  workflowFolderId,
  folders,
  running,
  onRun,
  onStop,
  onRename,
  onEditFolder,
  onMoveToFolder,
  onSchedule,
  onTriggers,
  onShare,
  onDelete,
}: Omit<Props, "x" | "y" | "onClose">) {
  const close = useContextMenuClose();
  const run = (action: () => void) => {
    action();
    close();
  };

  return (
    <>
      <MenuLabel>Workflow</MenuLabel>
      <MenuDescription title={workflowName}>{workflowName}</MenuDescription>

      {running ? (
        <MenuItem danger icon={<StopIcon />} onSelect={() => run(onStop)}>
          Stop
        </MenuItem>
      ) : (
        <MenuItem icon={<PlayIcon />} onSelect={() => run(onRun)}>
          Run
        </MenuItem>
      )}

      <MenuSeparator />

      <MenuItem
        icon={<PencilIcon />}
        onSelect={() => run(onRename)}
      >
        Rename…
      </MenuItem>
      <MenuItem
        icon={<FolderIcon />}
        onSelect={() => run(onEditFolder)}
      >
        Working directory…
      </MenuItem>
      <MenuSub>
        <MenuSubTrigger>Move to folder</MenuSubTrigger>
        <MenuSubContent>
          <MenuItem
            disabled={!workflowFolderId}
            onSelect={() => run(() => onMoveToFolder(null))}
          >
            <span className="workflow-menu-folder-choice">
              <span>Unfiled</span>
              {!workflowFolderId ? <span aria-hidden>✓</span> : null}
            </span>
          </MenuItem>
          {folders.map((folder) => {
            const current = folder.id === workflowFolderId;
            return (
              <MenuItem
                key={folder.id}
                disabled={current}
                onSelect={() => run(() => onMoveToFolder(folder.id))}
              >
                <span className="workflow-menu-folder-choice">
                  <span>{folder.name}</span>
                  {current ? <span aria-hidden>✓</span> : null}
                </span>
              </MenuItem>
            );
          })}
          {folders.length === 0 ? (
            <MenuDescription>No folders yet</MenuDescription>
          ) : null}
        </MenuSubContent>
      </MenuSub>

      <MenuSeparator />

      <MenuItem icon={<ClockIcon />} onSelect={() => run(onSchedule)}>
        Schedule…
      </MenuItem>
      <MenuItem icon={<FiltersIcon />} onSelect={() => run(onTriggers)}>
        Triggers…
      </MenuItem>
      <MenuItem onSelect={() => run(onShare)}>
        Share workflow…
      </MenuItem>

      <MenuSeparator />

      <MenuItem danger icon={<TrashIcon />} onSelect={() => run(onDelete)}>
        Delete
      </MenuItem>
    </>
  );
}

function Icon({ children }: { children: ReactNode }) {
  return children;
}

function PencilIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path
          d="M9.5 3.5 12.5 6.5M3 13l.7-3.2L10.8 2.7a1.4 1.4 0 0 1 2 2L5.8 11.7 3 13Z"
          stroke="currentColor"
          strokeWidth="1.35"
          strokeLinejoin="round"
        />
      </svg>
    </Icon>
  );
}

function PlayIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path
          d="m5.5 3.75 6.25 4.25-6.25 4.25v-8.5Z"
          stroke="currentColor"
          strokeWidth="1.35"
          strokeLinejoin="round"
        />
      </svg>
    </Icon>
  );
}

function StopIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <rect
          x="4.25"
          y="4.25"
          width="7.5"
          height="7.5"
          rx="1"
          stroke="currentColor"
          strokeWidth="1.35"
        />
      </svg>
    </Icon>
  );
}

function FolderIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path
          d="M2.5 4.5h4l1.2 1.2H13.5v6.8a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1V4.5Z"
          stroke="currentColor"
          strokeWidth="1.35"
          strokeLinejoin="round"
        />
      </svg>
    </Icon>
  );
}

function ClockIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <circle cx="8" cy="8" r="5.25" stroke="currentColor" strokeWidth="1.35" />
        <path
          d="M8 5.2V8l1.8 1.2"
          stroke="currentColor"
          strokeWidth="1.35"
          strokeLinecap="round"
        />
      </svg>
    </Icon>
  );
}

function FiltersIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path
          d="M3 4.5h10M5 8h6M6.5 11.5h3"
          stroke="currentColor"
          strokeWidth="1.35"
          strokeLinecap="round"
        />
      </svg>
    </Icon>
  );
}

function TrashIcon() {
  return (
    <Icon>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <path
          d="M3.5 4.5h9M6 4.5V3.2h4v1.3M5.2 4.5l.5 8.3h4.6l.5-8.3"
          stroke="currentColor"
          strokeWidth="1.35"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    </Icon>
  );
}

export function WorkflowContextMenu({
  x,
  y,
  workflowName,
  workflowFolderId,
  folders,
  running,
  onClose,
  onRun,
  onStop,
  onRename,
  onEditFolder,
  onMoveToFolder,
  onSchedule,
  onTriggers,
  onShare,
  onDelete,
}: Props) {
  return (
    <ContextMenu x={x} y={y} onClose={onClose} animated zIndex={80}>
      <WorkflowMenuBody
        workflowName={workflowName}
        workflowFolderId={workflowFolderId}
        folders={folders}
        running={running}
        onRun={onRun}
        onStop={onStop}
        onRename={onRename}
        onEditFolder={onEditFolder}
        onMoveToFolder={onMoveToFolder}
        onSchedule={onSchedule}
        onTriggers={onTriggers}
        onShare={onShare}
        onDelete={onDelete}
      />
    </ContextMenu>
  );
}
