import { openPath } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { Icon } from "../../../../components/icon";
import {
  attachmentAssetUrl,
  attachmentExtension,
  attachmentFileName,
  isImageAttachmentPath,
  shortAttachmentPath,
} from "../../attachments";
import type { InputAttachment } from "../../types";
import { openFileEditor } from "../file-editor-modal";

type Props = {
  attachments: InputAttachment[];
  onRemove: (attachmentId: string) => void;
  /** Show attachments without controls that can remove them. */
  readOnly?: boolean;
  /** Compact list for canvas nodes; richer cards for the settings modal. */
  variant?: "compact" | "modal";
  emptyLabel?: string;
};

function openAttachment(item: InputAttachment) {
  if (item.kind === "file") openFileEditor(item.path);
  else void openPath(item.path).catch((e) => console.error("openPath failed", e));
}

function FolderGlyph() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" className="wf-attach-glyph">
      <path
        fill="currentColor"
        d="M3.5 6.75A2.25 2.25 0 0 1 5.75 4.5h4.1c.4 0 .78.16 1.06.44l1.15 1.15c.28.28.66.44 1.06.44h5.13A2.25 2.25 0 0 1 20.5 8.78v8.47A2.25 2.25 0 0 1 18.25 19.5H5.75A2.25 2.25 0 0 1 3.5 17.25V6.75Z"
        opacity="0.92"
      />
    </svg>
  );
}

function FileGlyph() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" className="wf-attach-glyph">
      <path
        fill="currentColor"
        d="M7.5 3.75h5.38c.4 0 .78.16 1.06.44l4.12 4.12c.28.28.44.66.44 1.06v10.88A1.75 1.75 0 0 1 16.75 22H7.5A1.75 1.75 0 0 1 5.75 20.25V5.5A1.75 1.75 0 0 1 7.5 3.75Zm5.25 1.5v3.38c0 .41.34.75.75.75h3.38L12.75 5.25Z"
        opacity="0.92"
      />
    </svg>
  );
}

function AttachmentThumb({ item }: { item: InputAttachment }) {
  const [imageFailed, setImageFailed] = useState(false);
  const showImage =
    item.kind === "file" &&
    isImageAttachmentPath(item.path) &&
    !imageFailed;

  if (showImage) {
    return (
      <div className="wf-attach-thumb wf-attach-thumb-image">
        <img
          src={attachmentAssetUrl(item.path)}
          alt=""
          loading="lazy"
          onError={() => setImageFailed(true)}
        />
      </div>
    );
  }

  const ext = item.kind === "file" ? attachmentExtension(item.path) : "";
  return (
    <div
      className={`wf-attach-thumb wf-attach-thumb-icon${
        item.kind === "folder" ? " is-folder" : ""
      }`}
    >
      {item.kind === "folder" ? <FolderGlyph /> : <FileGlyph />}
      {ext ? <span className="wf-attach-ext">{ext}</span> : null}
    </div>
  );
}

export function InputAttachmentList({
  attachments,
  onRemove,
  readOnly = false,
  variant = "compact",
  emptyLabel = "Drop files or folders here",
}: Props) {
  if (attachments.length === 0) {
    return (
      <p className={variant === "modal" ? "muted" : "wf-input-attach-empty"}>
        {emptyLabel}
      </p>
    );
  }

  if (variant === "compact") {
    return (
      <ul className="wf-input-attach-list">
        {attachments.map((item) => (
          <li
            key={item.id}
            className="wf-input-attach-chip"
            title={item.path}
          >
            <span className="wf-input-attach-kind">
              {item.kind === "folder" ? "Folder" : "File"}
            </span>
            <button
              type="button"
              className="wf-input-attach-open"
              aria-label={`${item.kind === "folder" ? "Open" : "Edit"} ${item.path}`}
              onClick={() => openAttachment(item)}
            >
              <span className="wf-input-attach-path">
                {shortAttachmentPath(item.path)}
              </span>
              {item.kind === "file" ? <Icon name="pencil-simple" size={12} /> : null}
            </button>
            {readOnly ? null : (
              <button
                type="button"
                className="wf-input-attach-remove"
                aria-label={`Remove ${item.path}`}
                onClick={() => onRemove(item.id)}
              >
                ×
              </button>
            )}
          </li>
        ))}
      </ul>
    );
  }

  return (
    <ul className="wf-attach-cards">
      {attachments.map((item) => {
        const name = attachmentFileName(item.path);
        const kindLabel = item.kind === "folder" ? "Folder" : "File";
        return (
          <li key={item.id} className="wf-attach-card" title={item.path}>
            <AttachmentThumb item={item} />
            <div className="wf-attach-card-copy">
              <span className="wf-attach-card-name">{name}</span>
              <span className="wf-attach-card-meta">
                <span className="wf-attach-card-kind">{kindLabel}</span>
                <span className="wf-attach-card-path">
                  {shortAttachmentPath(item.path)}
                </span>
              </span>
            </div>
            <div className="wf-attach-card-actions">
              <button
                type="button"
                className="ghost wf-attach-card-remove"
                aria-label={`${item.kind === "folder" ? "Open" : "Edit"} ${name}`}
                onClick={() => openAttachment(item)}
              >
                {item.kind === "folder" ? "Open" : "Edit"}
              </button>
              {readOnly ? null : (
                <button
                  type="button"
                  className="ghost wf-attach-card-remove"
                  aria-label={`Remove ${name}`}
                  onClick={() => onRemove(item.id)}
                >
                  Remove
                </button>
              )}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
