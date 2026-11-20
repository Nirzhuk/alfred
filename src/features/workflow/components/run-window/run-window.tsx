import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { installThemeListeners } from "../../../settings/theme";
import * as api from "../../api";
import { askFields } from "../../ask-fields";
import type { Workflow } from "../../types";
import { RunWindowForm } from "./run-window-form";
import "../../../../App.css";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Dedicated window opened before a manual run that needs `{{ask:…}}` values.
 * It owns only the form; the main window owns starting and tracking the run.
 */
export function RunWindow() {
  const [workflowId, setWorkflowId] = useState(
    () => new URLSearchParams(window.location.search).get("runDialog") ?? "",
  );
  /** Re-emitting the same workflow still reloads a dialog left open after an edit. */
  const [reloadKey, setReloadKey] = useState(0);
  const [workflow, setWorkflow] = useState<Workflow | null>(null);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const loadVersion = useRef(0);
  const startingRef = useRef(false);

  const fields = useMemo(
    () => askFields(workflow?.graph.nodes ?? []),
    [workflow],
  );

  const load = useCallback(async () => {
    const version = ++loadVersion.current;
    if (!workflowId) {
      setLoading(false);
      setLoadError("No workflow was selected.");
      return;
    }
    setLoading(true);
    setLoadError(null);
    setSubmitError(null);
    setWorkflow(null);
    try {
      const next = await api.getWorkflow(workflowId);
      if (version !== loadVersion.current) return;
      if (!next) {
        setLoadError("This workflow no longer exists.");
        return;
      }
      setWorkflow(next);
      setAnswers(
        Object.fromEntries(
          askFields(next.graph.nodes).map((field) => [
            field.name,
            field.defaultValue,
          ]),
        ),
      );
    } catch (error) {
      if (version === loadVersion.current) {
        setLoadError(`Could not load the workflow: ${errorMessage(error)}`);
      }
    } finally {
      if (version === loadVersion.current) setLoading(false);
    }
  }, [workflowId]);

  useEffect(() => {
    void load();
  }, [load, reloadKey]);

  useEffect(() => {
    if (!workflow) return;
    const title = `Run · ${workflow.name}`;
    document.title = title;
    if (isTauri()) {
      void getCurrentWindow()
        .setTitle(title)
        .catch(() => {
          /* the window title is cosmetic */
        });
    }
  }, [workflow]);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<string>("run-dialog://open", (event) => {
      if (!event.payload) return;
      setWorkflowId(event.payload);
      setReloadKey((key) => key + 1);
    }).then((stop) => {
      if (cancelled) stop();
      else unlisten = stop;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => installThemeListeners(), []);

  const cancel = useCallback(() => {
    if (!isTauri()) return;
    void getCurrentWindow()
      .close()
      .catch(() => {
        /* the main window still owns the workflow */
      });
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || starting) return;
      event.preventDefault();
      cancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [cancel, starting]);

  const changeAnswer = useCallback((name: string, value: string) => {
    setAnswers((previous) => ({ ...previous, [name]: value }));
  }, []);

  const resetAnswers = useCallback(() => {
    if (!workflow) return;
    setAnswers(
      Object.fromEntries(
        askFields(workflow.graph.nodes).map((field) => [
          field.name,
          field.defaultValue,
        ]),
      ),
    );
    setSubmitError(null);
  }, [workflow]);

  const submit = useCallback(async () => {
    if (!workflow || startingRef.current) return;
    startingRef.current = true;
    setStarting(true);
    setSubmitError(null);
    try {
      await api.submitRunDialog(workflow.id, answers);
      if (isTauri()) {
        await getCurrentWindow()
          .close()
          .catch(() => {
            /* the run has started; the dialog is no longer needed */
          });
      }
    } catch (error) {
      startingRef.current = false;
      setStarting(false);
      setSubmitError(`Could not start the run: ${errorMessage(error)}`);
    }
  }, [answers, workflow]);

  if (loading || !workflow) {
    return (
      <div className="run-window">
        <header className="run-window-header">
          <span className="run-window-glyph" aria-hidden="true">
            <Icon name="play" size={20} />
          </span>
          <div className="run-window-heading">
            <h1 className="run-window-title">Run workflow</h1>
            <p className="run-window-description">
              {loadError ?? "Loading the inputs for this run…"}
            </p>
          </div>
        </header>
        <div className="run-window-body">
          {loadError ? (
            <div className="run-window-state run-window-state-error" role="alert">
              <span className="run-window-state-glyph" aria-hidden="true">
                <Icon name="warning-circle" size={20} />
              </span>
              <p>{loadError}</p>
            </div>
          ) : (
            <div className="run-window-state" aria-live="polite">
              <span className="run-window-spinner" aria-hidden="true" />
              <p>Loading workflow…</p>
            </div>
          )}
        </div>
        <footer className="run-window-footer">
          <span aria-hidden="true" />
          <div className="run-window-actions">
            <button type="button" className="ghost" onClick={cancel}>
              Cancel
            </button>
            {loadError ? (
              <button type="button" className="primary" onClick={() => void load()}>
                Try again
              </button>
            ) : null}
          </div>
        </footer>
      </div>
    );
  }

  return (
    <div className="run-window">
      <RunWindowForm
        workflowName={workflow.name}
        description={workflow.description}
        stepCount={workflow.graph.nodes.length}
        workingDirectory={workflow.workingDirectory ?? ""}
        fields={fields}
        answers={answers}
        starting={starting}
        error={submitError}
        onChange={changeAnswer}
        onReset={resetAnswers}
        onSubmit={() => void submit()}
        onCancel={cancel}
      />
    </div>
  );
}
