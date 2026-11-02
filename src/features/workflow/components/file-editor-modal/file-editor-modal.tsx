import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import { create } from "zustand";
import { ConfirmDialog } from "../../../../components/confirm-dialog";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { attachmentFileName } from "../../attachments";
import { fromDiskText, toDiskText } from "./file-text";

const useFileEditorStore = create<{ path: string | null }>(() => ({ path: null }));

export function openFileEditor(path: string) {
  useFileEditorStore.setState({ path });
}

const CHANGED_ON_DISK = "FILE_CHANGED_ON_DISK";

type Loaded = { saved: string; crlf: boolean; modifiedMs: number };

function Editor({ path, onClose }: { path: string; onClose: () => void }) {
  const [file, setFile] = useState<Loaded | null>(null);
  const [draft, setDraft] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [saving, setSaving] = useState(false);
  const [confirmingClose, setConfirmingClose] = useState(false);

  const dirty = file !== null && draft !== file.saved;
  const name = attachmentFileName(path);

  const load = useCallback(async () => {
    setConflict(false);
    setNotice(null);
    try {
      const loaded = await invoke<{ content: string; modifiedMs: number }>(
        "read_text_file",
        { path },
      );
      const { text, crlf } = fromDiskText(loaded.content);
      setFile({ saved: text, crlf, modifiedMs: loaded.modifiedMs });
      setDraft(text);
    } catch (error) {
      setLoadError(String(error));
    }
  }, [path]);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async (overwrite = false) => {
    if (!file || saving || (!dirty && !overwrite)) return;
    setSaving(true);
    setNotice(null);
    try {
      const modifiedMs = await invoke<number>("write_text_file", {
        path,
        content: toDiskText(draft, file.crlf),
        expectedModifiedMs: overwrite ? null : file.modifiedMs,
      });
      setFile({ ...file, saved: draft, modifiedMs });
      setConflict(false);
      setNotice("Saved. The next run uses this version.");
    } catch (error) {
      if (String(error) === CHANGED_ON_DISK) setConflict(true);
      else setNotice(String(error));
    } finally {
      setSaving(false);
    }
  };

  const openExternally = () => {
    openPath(path).catch((error) => setNotice(`Could not open this file: ${error}`));
  };

  const requestClose = () => {
    if (dirty) setConfirmingClose(true);
    else onClose();
  };

  return (
    <Modal
      size="xl"
      closeOnBackdrop={false}
      onClose={requestClose}
      labelledBy="file-editor-title"
      describedBy="file-editor-path"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="file" size={20} />
          </span>
        }
        title={dirty ? `${name} (unsaved)` : name}
        titleId="file-editor-title"
        description={path}
        descriptionId="file-editor-path"
        actions={
          <>
            <button type="button" className="ghost" onClick={openExternally}>
              Open in default app
            </button>
            {loadError ? null : (
              <button
                type="button"
                className="primary"
                disabled={!dirty || saving}
                onClick={() => void save()}
              >
                {saving ? "Saving…" : "Save"}
              </button>
            )}
            <button
              type="button"
              className="ghost modal-close-button"
              aria-label="Close"
              onClick={requestClose}
            >
              <Icon name="x" size={16} />
            </button>
          </>
        }
      />
      {conflict ? (
        <div className="output-modal-notice file-editor-conflict" role="alert">
          <span>This file changed on disk after you opened it.</span>
          <button type="button" className="ghost" onClick={() => void load()}>
            Reload from disk
          </button>
          <button type="button" className="ghost" onClick={() => void save(true)}>
            Overwrite
          </button>
        </div>
      ) : null}
      {notice ? (
        <p className="output-modal-notice" role="status">
          {notice}
        </p>
      ) : null}
      {loadError ? (
        <p className="output-modal-body">{loadError}</p>
      ) : (
        <textarea
          className="file-editor-text"
          aria-label={`Contents of ${name}`}
          autoFocus
          readOnly={!file}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          wrap="off"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
              event.preventDefault();
              void save();
            }
          }}
        />
      )}
      {confirmingClose ? (
        <ConfirmDialog
          title="Discard unsaved changes?"
          message={`Your edits to ${name} will be lost.`}
          confirmLabel="Discard"
          cancelLabel="Keep editing"
          danger
          onConfirm={onClose}
          onCancel={() => setConfirmingClose(false)}
        />
      ) : null}
    </Modal>
  );
}

export function FileEditorModal() {
  const path = useFileEditorStore((state) => state.path);
  if (!path) return null;
  return (
    <Editor
      key={path}
      path={path}
      onClose={() => useFileEditorStore.setState({ path: null })}
    />
  );
}
