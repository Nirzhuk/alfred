import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { useFileDrop } from "../../use-file-drop";
import { InputAttachmentList } from "../input-attachment-list";

type Props = {
  paths: string[];
  onChange: (paths: string[]) => void;
  variant: "compact" | "modal";
};

/** The File inject step's file list, shared by its canvas card and settings. */
export function FileInjectPaths({ paths, onChange, variant }: Props) {
  const [notice, setNotice] = useState<string | null>(null);

  const add = (incoming: string[]) => {
    onChange([...new Set([...paths, ...incoming])]);
  };

  const pick = async () => {
    try {
      const picked = await open({
        multiple: true,
        directory: false,
        title: "Inject files into context",
      });
      if (picked) add(picked);
    } catch (e) {
      console.warn("File picker unavailable", e);
    }
  };

  const { ref, over } = useFileDrop<HTMLDivElement>((dropped) => {
    void invoke<boolean[]>("paths_are_folders", { paths: dropped }).then((folders) => {
      const files = dropped.filter((_, index) => !folders[index]);
      if (files.length > 0) add(files);
      setNotice(
        files.length < dropped.length
          ? "Folders are skipped here. Attach them to an Input step instead."
          : null,
      );
    });
  });

  return (
    <div
      ref={ref}
      className={`wf-input-attachments nodrag nopan${over ? " is-drop-target" : ""}`}
      data-prevent-node-double-click
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="wf-attach-field-header">
        {variant === "modal" ? <span>Files</span> : null}
        <button
          type="button"
          className="ghost wf-input-attach-btn"
          onClick={() => void pick()}
        >
          + File
        </button>
      </div>
      {notice ? <p className="wf-input-attach-empty">{notice}</p> : null}
      <InputAttachmentList
        attachments={paths.map((path) => ({ id: path, path, kind: "file" as const }))}
        onRemove={(path) => onChange(paths.filter((item) => item !== path))}
        variant={variant}
        emptyLabel="Drop files here to inject them"
      />
    </div>
  );
}
