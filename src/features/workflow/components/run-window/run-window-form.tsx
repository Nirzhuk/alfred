import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { detectDesktopPlatform } from "../../../../platform";
import { missingRequired, type AskField } from "../../ask-fields";

type Props = {
  workflowName: string;
  description: string;
  stepCount: number;
  workingDirectory: string;
  fields: AskField[];
  answers: Record<string, string>;
  starting: boolean;
  error: string | null;
  onChange: (name: string, value: string) => void;
  onReset: () => void;
  onSubmit: () => void;
  onCancel: () => void;
};

function directoryName(path: string): string {
  const trimmed = path.trim().replace(/[\\/]+$/, "");
  if (!trimmed) return "";
  const parts = trimmed.split(/[\\/]/);
  return parts[parts.length - 1] || trimmed;
}

function sourceSummary(field: AskField): string {
  const labels = [...new Set(field.sources.map((source) => source.label))];
  return labels.join(" · ");
}

/** The run window's form. Kept free of Tauri imports so it can be rendered in tests. */
export function RunWindowForm({
  workflowName,
  description,
  stepCount,
  workingDirectory,
  fields,
  answers,
  starting,
  error,
  onChange,
  onReset,
  onSubmit,
  onCancel,
}: Props) {
  const [attempted, setAttempted] = useState(false);
  const fieldRefs = useRef(new Map<string, HTMLTextAreaElement>());
  const runRef = useRef<HTMLButtonElement | null>(null);
  const missing = useMemo(() => missingRequired(fields, answers), [answers, fields]);
  const dirty = fields.some(
    (field) => (answers[field.name] ?? "") !== field.defaultValue,
  );
  const modifier = detectDesktopPlatform() === "macos" ? "⌘" : "Ctrl";

  useEffect(() => {
    const first =
      fields.find((field) => field.required && !(answers[field.name] ?? "").trim()) ??
      fields[0];
    if (first) fieldRefs.current.get(first.name)?.focus();
    else runRef.current?.focus();
    // Focus once as the form appears; typing must not move the caret.
  }, []);

  const submit = () => {
    setAttempted(true);
    if (missing.length > 0) {
      const target = fieldRefs.current.get(missing[0]);
      target?.scrollIntoView({ block: "nearest" });
      target?.focus();
      return;
    }
    onSubmit();
  };

  const directory = directoryName(workingDirectory);

  return (
    <form
      className="run-window-form"
      aria-labelledby="run-window-title"
      aria-describedby="run-window-description"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
          event.preventDefault();
          submit();
        }
      }}
    >
      <header className="run-window-header">
        <span className="run-window-glyph" aria-hidden="true">
          <Icon name="play" size={20} />
        </span>
        <div className="run-window-heading">
          <h1 id="run-window-title" className="run-window-title">
            {workflowName}
          </h1>
          <p id="run-window-description" className="run-window-description">
            {description.trim() || "Set the values this run will use."}
          </p>
          <div className="run-window-meta">
            <span>
              <Icon name="flow-arrow" size={14} />
              {stepCount} {stepCount === 1 ? "step" : "steps"}
            </span>
            <span>
              <Icon name="sliders" size={14} />
              {fields.length === 0
                ? "No inputs"
                : `${fields.length} ${fields.length === 1 ? "input" : "inputs"}`}
            </span>
            {directory ? (
              <span className="run-window-meta-directory" title={workingDirectory}>
                <Icon name="folder" size={14} />
                {directory}
              </span>
            ) : null}
          </div>
        </div>
      </header>

      <div className="run-window-body">
        {fields.length === 0 ? (
          <div className="run-window-empty">
            <span className="run-window-empty-glyph" aria-hidden="true">
              <Icon name="check" size={20} />
            </span>
            <div>
              <h2>No inputs to fill</h2>
              <p>This workflow can start with the values it already has.</p>
            </div>
          </div>
        ) : (
          <section
            className="run-window-section"
            aria-labelledby="run-window-inputs-title"
          >
            <div className="run-window-section-head">
              <h2 id="run-window-inputs-title">
                {fields.length === 1 ? "Input" : "Inputs"}
              </h2>
              {dirty ? (
                <button
                  type="button"
                  className="ghost run-window-reset"
                  disabled={starting}
                  onClick={() => {
                    setAttempted(false);
                    onReset();
                  }}
                >
                  Reset defaults
                </button>
              ) : null}
            </div>

            {attempted && missing.length > 0 ? (
              <p className="run-window-alert" role="alert">
                Fill in the required inputs before running.
              </p>
            ) : null}

            <div className="run-window-fields">
              {fields.map((field, index) => {
                const invalid =
                  attempted && field.required && !(answers[field.name] ?? "").trim();
                const describedBy = invalid
                  ? `run-window-field-${index}-error`
                  : `run-window-field-${index}-hint`;
                const sources = sourceSummary(field);
                return (
                  <div
                    key={field.name}
                    className={`run-window-field${invalid ? " is-invalid" : ""}`}
                  >
                    <div className="run-window-field-head">
                      <label htmlFor={`run-window-field-${index}`}>{field.name}</label>
                      <span
                        className={`run-window-badge${
                          field.required ? " is-required" : ""
                        }`}
                      >
                        {field.required ? "Required" : "Optional"}
                      </span>
                    </div>
                    <textarea
                      id={`run-window-field-${index}`}
                      ref={(element) => {
                        if (element) fieldRefs.current.set(field.name, element);
                        else fieldRefs.current.delete(field.name);
                      }}
                      rows={field.required ? 3 : 2}
                      value={answers[field.name] ?? ""}
                      disabled={starting}
                      placeholder={
                        field.required
                          ? "Enter a value"
                          : field.defaultValue
                            ? "Leave blank to use the default"
                            : "Optional"
                      }
                      aria-invalid={invalid || undefined}
                      aria-describedby={describedBy}
                      onChange={(event) => onChange(field.name, event.target.value)}
                    />
                    {invalid ? (
                      <p
                        id={`run-window-field-${index}-error`}
                        className="run-window-field-error"
                      >
                        Enter a value to run.
                      </p>
                    ) : (
                      <p
                        id={`run-window-field-${index}-hint`}
                        className="run-window-field-hint"
                      >
                        {field.required
                          ? "No default — this run needs a value."
                          : field.defaultValue
                            ? `Defaults to “${field.defaultValue}” when left blank.`
                            : "Optional — leaving it blank is fine."}
                        {sources ? ` Used in ${sources}.` : ""}
                      </p>
                    )}
                  </div>
                );
              })}
            </div>
          </section>
        )}

        {error ? (
          <p className="run-window-alert" role="alert">
            {error}
          </p>
        ) : null}
      </div>

      <footer className="run-window-footer">
        <span className="run-window-shortcuts" aria-hidden="true">
          <kbd>esc</kbd> to cancel
          <span className="run-window-shortcut-separator">·</span>
          <kbd>{modifier}</kbd>
          <kbd>↵</kbd> to run
        </span>
        <div className="run-window-actions">
          <button type="button" className="ghost" disabled={starting} onClick={onCancel}>
            Cancel
          </button>
          <button
            ref={runRef}
            type="submit"
            className="primary run-window-run"
            disabled={starting}
            aria-busy={starting || undefined}
          >
            <Icon name="play" size={16} />
            {starting ? "Starting…" : "Run workflow"}
          </button>
        </div>
      </footer>
    </form>
  );
}
