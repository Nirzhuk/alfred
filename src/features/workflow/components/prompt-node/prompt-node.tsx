import {
  Handle,
  NodeResizeControl,
  Position,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import { useRef } from "react";
import { Icon } from "../../../../components/icon";
import {
  mergeAttachments,
  pickFileAttachments,
  pickFolderAttachments,
  shortAttachmentPath,
} from "../../attachments";
import { useWorkflowStore } from "../../store";
import type { PromptNodeData } from "../../types";
import { InputAttachmentList } from "../input-attachment-list";
import { NodeRunResult } from "../node-run-result";

type InputFlowNode = Node<PromptNodeData, "prompt" | "input">;

const BLOCK_TOGGLE_DEBOUNCE_MS = 300;

function ResizeGripIcon() {
  return (
    <svg
      className="wf-node-resize-icon"
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden
    >
      <path
        d="M5.5 13.5 L13.5 5.5 M8.5 13.5 L13.5 8.5 M11.5 13.5 L13.5 11.5"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}

function BlockIcon({ blocked }: { blocked: boolean }) {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="3.25"
        y="7"
        width="9.5"
        height="6.5"
        rx="1.5"
        stroke="currentColor"
        strokeWidth="1.35"
      />
      <path
        d={
          blocked
            ? "M5.35 7V5.25a2.65 2.65 0 0 1 5.3 0V7"
            : "M5.35 7V5.25a2.65 2.65 0 0 1 5.15-.88"
        }
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function PromptNode({ id, data, selected }: NodeProps<InputFlowNode>) {
  const updateNodeData = useWorkflowStore((s) => s.updateNodeData);
  const title = data.label || "Input";
  const attachments = data.attachments ?? [];
  const blocked = Boolean(data.blocked);
  const blockToggleLockedUntil = useRef(0);

  const toggleBlocked = () => {
    const now = performance.now();
    if (now < blockToggleLockedUntil.current) return;
    blockToggleLockedUntil.current = now + BLOCK_TOGGLE_DEBOUNCE_MS;
    updateNodeData(id, { blocked: !blocked });
  };

  const addFiles = async () => {
    try {
      const picked = await pickFileAttachments();
      if (picked.length === 0) return;
      updateNodeData(id, {
        attachments: mergeAttachments(attachments, picked),
      });
    } catch (e) {
      console.warn("File picker unavailable", e);
    }
  };

  const addFolders = async () => {
    try {
      const picked = await pickFolderAttachments();
      if (picked.length === 0) return;
      updateNodeData(id, {
        attachments: mergeAttachments(attachments, picked),
      });
    } catch (e) {
      console.warn("Folder picker unavailable", e);
    }
  };

  const removeAttachment = (attachmentId: string) => {
    updateNodeData(id, {
      attachments: attachments.filter((a) => a.id !== attachmentId),
    });
  };

  return (
    <div className="wf-node wf-node-prompt">
      <NodeResizeControl
        position="bottom-right"
        minWidth={220}
        minHeight={170}
        maxWidth={560}
        maxHeight={720}
        shouldResize={() => !blocked}
        className={`wf-node-resize-control${
          selected && !blocked ? " is-visible" : ""
        }${blocked ? " is-blocked" : ""}`}
      >
        <ResizeGripIcon />
      </NodeResizeControl>

      <div className="wf-node-header">
        <span className="wf-node-title-icon">
          <Icon name="chat-text" size={16} />
        </span>
        <div className="wf-node-title">{title}</div>
        <button
          type="button"
          data-prevent-node-double-click
          className={`wf-node-block-toggle nodrag nopan${
            blocked ? " is-blocked" : ""
          }${selected ? " is-visible" : ""}`}
          aria-label={blocked ? "Unblock input editing" : "Block input editing"}
          aria-pressed={blocked}
          title={blocked ? "Unblock input editing" : "Block input editing"}
          onClick={(event) => {
            event.stopPropagation();
            toggleBlocked();
          }}
          onDoubleClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
          }}
          onPointerDown={(e) => e.stopPropagation()}
        >
          <BlockIcon blocked={blocked} />
        </button>
      </div>
      <div className="wf-node-content">
        <textarea
        className="wf-node-input nodrag nopan nowheel"
        value={data.prompt}
        placeholder="Describe the task… attach files or folders below"
        rows={4}
        readOnly={blocked}
        aria-readonly={blocked}
        onChange={(e) => updateNodeData(id, { prompt: e.target.value })}
        onPointerDown={(e) => e.stopPropagation()}
      />

      <div
        className="wf-input-attachments nodrag nopan"
        onPointerDown={(e) => e.stopPropagation()}
      >
        {blocked ? (
          <p className="wf-input-blocked-note">Editing blocked</p>
        ) : (
          <div className="wf-input-attach-actions">
            <button
              type="button"
              className="ghost wf-input-attach-btn"
              onClick={() => void addFiles()}
            >
              + File
            </button>
            <button
              type="button"
              className="ghost wf-input-attach-btn"
              onClick={() => void addFolders()}
            >
              + Folder
            </button>
          </div>
        )}
        <InputAttachmentList
          attachments={attachments}
          onRemove={removeAttachment}
          readOnly={blocked}
          variant="compact"
        />
        {data.script ? (
          <ul className="wf-input-attach-list">
            <li
              className="wf-input-attach-chip"
              title={
                data.script.source === "file"
                  ? data.script.path
                  : "Saved inline script"
              }
            >
              <span className="wf-input-attach-kind">
                {data.script.run ? "Script · run" : "Script"}
              </span>
              <span className="wf-input-attach-path">
                {data.script.source === "file"
                  ? shortAttachmentPath(data.script.path) || "No file"
                  : "Inline script"}
              </span>
            </li>
          </ul>
        ) : null}
      </div>

        <NodeRunResult nodeId={id} title={title} />
      </div>
      <Handle
        className="wf-handle"
        type="source"
        position={Position.Right}
        isConnectable
      />
    </div>
  );
}

/** Preferred name for the Input node component. */
export const InputNode = PromptNode;
