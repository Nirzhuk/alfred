import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { useFileDrop } from "../../use-file-drop";
import type { InputAttachment } from "../../types";
import { InputAttachmentList } from "../input-attachment-list";

type Props = {
  paths: string[];
  onChange: (paths: string[]) => void;
  variant: "compact" | "modal";
};

/**
 * `paths` is a plain string list in the workflow contract, so whether each one
 * is a folder is asked of the disk rather than stored.
 */
function useInjectedAttachments(paths: string[]): InputAttachment[] {
  const [folders, setFolders] = useState<ReadonlySet<string>>(new Set());
  const key = paths.join("\0");

  useEffect(() => {
    if (paths.length === 0) return;
    let stale = false;
    void invoke<boolean[]>("paths_are_folders", { paths }).then((flags) => {
      if (!stale) setFolders(new Set(paths.filter((_, index) => flags[index])));
    });
    return () => {
      stale = true;
    };
    // `key` is the value of `paths`; callers pass a fresh array each render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return paths.map((path) => ({
    id: path,
    path,
    kind: folders.has(path) ? "folder" : "file",
  }));
}

/** What a finished File inject step read, for the run's input viewer. */
export function InjectedPathsList({ paths }: { paths: string[] }) {
  return (
    <InputAttachmentList
      attachments={useInjectedAttachments(paths)}
      onRemove={() => undefined}
      readOnly
      variant="modal"
    />
  );
}

/** The File inject step's path list, shared by its canvas card and settings. */
export function FileInjectPaths({ paths, onChange, variant }: Props) {
  const attachments = useInjectedAttachments(paths);

  const add = (incoming: string[]) => {
    onChange([...new Set([...paths, ...incoming])]);
  };

  const pick = async (directory: boolean) => {
    try {
      const picked = await open({
        multiple: true,
        directory,
        title: directory ? "Inject folders into context" : "Inject files into context",
      });
      if (picked) add(picked);
    } catch (e) {
      console.warn("File picker unavailable", e);
    }
  };

  const { ref, over } = useFileDrop<HTMLDivElement>(add);

  return (
    <div
      ref={ref}
      className={`wf-input-attachments nodrag nopan${over ? " is-drop-target" : ""}`}
      data-prevent-node-double-click
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="wf-attach-field-header">
        {variant === "modal" ? <span>Files and folders</span> : null}
        <div className="wf-input-attach-actions">
          <button
            type="button"
            className="ghost wf-input-attach-btn"
            onClick={() => void pick(false)}
          >
            + File
          </button>
          <button
            type="button"
            className="ghost wf-input-attach-btn"
            onClick={() => void pick(true)}
          >
            + Folder
          </button>
        </div>
      </div>
      <InputAttachmentList
        attachments={attachments}
        onRemove={(path) => onChange(paths.filter((item) => item !== path))}
        variant={variant}
        emptyLabel="Drop files or folders here to inject them"
      />
    </div>
  );
}
