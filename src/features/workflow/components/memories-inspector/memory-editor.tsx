import { Icon } from "../../../../components/icon";
import { SelectControl } from "../../../../components/select-control";
import type {
  MemoryKind,
  MemoryScopeType,
  MemoryStatus,
  MemoryType,
} from "../../types";

export const MEMORY_TYPE_LABELS: Record<MemoryType, string> = {
  preference: "Preference",
  fact: "Fact",
  decision: "Decision",
  constraint: "Constraint",
  lesson: "Lesson",
  episode: "Episode",
  checkpoint: "Checkpoint",
  note: "Note",
  output: "Output",
  artifact: "Artifact",
};

export const MEMORY_STARTERS: {
  type: MemoryType;
  label: string;
  description: string;
}[] = [
  {
    type: "note",
    label: "Blank note",
    description: "Anything worth remembering",
  },
  {
    type: "preference",
    label: "Preference",
    description: "How you like things done",
  },
  {
    type: "decision",
    label: "Decision",
    description: "A choice and the reasons behind it",
  },
  {
    type: "lesson",
    label: "Lesson",
    description: "What worked and what to do next time",
  },
  {
    type: "constraint",
    label: "Constraint",
    description: "A rule the agent should respect",
  },
];

const CONTENT_PLACEHOLDERS: Partial<Record<MemoryType, string>> = {
  preference:
    "What should the agent know about how you like things done?\n\nFor example: Use short paragraphs and explain technical terms the first time they appear.",
  decision:
    "What did you decide, and why?\n\nInclude the alternatives you considered and when this decision should be revisited.",
  lesson:
    "What did you learn?\n\nDescribe what happened and what the agent should do differently next time.",
  constraint:
    "What rule should the agent follow?\n\nBe specific about when it applies and any exceptions.",
};

export type MemoryDraft = {
  title: string;
  body: string;
  kind: MemoryKind;
  scopeType: MemoryScopeType;
  memoryType: MemoryType;
  status: MemoryStatus;
  salience: number;
  confidence: number;
  observedAt: string;
  lastConfirmedAt: string;
  validFrom: string;
  expiresAt: string;
};

export function localDateTime(value: string): string {
  if (!value) return "";
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  const pad = (part: number) => String(part).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function storedDateTime(value: string): string {
  if (!value) return "";
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toISOString() : "";
}

export function MemoryEditor({
  draft,
  onChange,
  onSave,
  onCancel,
  creating,
  saving,
  dirty,
  hasWorkingDirectory,
  fileBacked,
}: {
  draft: MemoryDraft;
  onChange: (patch: Partial<MemoryDraft>) => void;
  onSave: () => void;
  onCancel: () => void;
  creating: boolean;
  saving: boolean;
  dirty: boolean;
  hasWorkingDirectory: boolean;
  fileBacked: boolean;
}) {
  const canSave =
    !saving &&
    (dirty || creating) &&
    (fileBacked || Boolean(draft.body.trim()));
  return (
    <form
      className="memory-composer"
      onSubmit={(event) => {
        event.preventDefault();
        if (canSave) onSave();
      }}
      onKeyDown={(event) => {
        if (
          (event.metaKey || event.ctrlKey) &&
          event.key.toLowerCase() === "s"
        ) {
          event.preventDefault();
          event.stopPropagation();
          if (canSave) onSave();
        }
      }}
    >
      <fieldset disabled={saving}>
        <label className="field memory-composer-title">
          <span>Title</span>
          <input
            autoFocus
            value={draft.title}
            placeholder="Give this memory a useful name"
            onChange={(event) => onChange({ title: event.target.value })}
          />
        </label>
        <div className="memory-composer-classification">
          <label className="field">
            <span>Memory type</span>
            <SelectControl
              value={draft.memoryType}
              onChange={(event) =>
                onChange({ memoryType: event.target.value as MemoryType })
              }
            >
              {Object.entries(MEMORY_TYPE_LABELS).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </SelectControl>
          </label>
          <label className="field">
            <span>Available to</span>
            <SelectControl
              value={draft.scopeType}
              onChange={(event) =>
                onChange({ scopeType: event.target.value as MemoryScopeType })
              }
            >
              <option value="workflow">This workflow</option>
              <option value="workspace" disabled={!hasWorkingDirectory}>
                This workspace
              </option>
              <option value="user">All my workflows</option>
            </SelectControl>
          </label>
        </div>
        <p className="memory-composer-scope-hint">
          {draft.scopeType === "user"
            ? "Personal memory is available across all your workflows."
            : draft.scopeType === "workspace"
              ? "Shared with workflows that use this working directory."
              : "Available only in this workflow unless you link it elsewhere."}
        </p>
        <label className="field memory-composer-content">
          <span>Content</span>
          {fileBacked ? (
            <small>
              This is a file preview. You can edit its details here; edit the
              original file to change its contents.
            </small>
          ) : null}
          <textarea
            aria-label="Memory content"
            rows={10}
            value={draft.body}
            readOnly={fileBacked}
            placeholder={
              CONTENT_PLACEHOLDERS[draft.memoryType] ??
              "Write something the agent can use again: context, a useful fact, or an instruction. Be specific."
            }
            onChange={(event) => onChange({ body: event.target.value })}
          />
        </label>
        <details className="memory-advanced-settings">
          <summary>
            <Icon name="sliders" size={16} /> More details{" "}
            <Icon name="caret-down" size={14} />
          </summary>
          <p>Adjust how this memory is organized and considered for recall.</p>
          <div className="memory-composer-classification">
            <label className="field">
              <span>Content kind</span>
              <SelectControl
                value={draft.kind}
                onChange={(event) =>
                  onChange({ kind: event.target.value as MemoryKind })
                }
              >
                <option value="note">Note</option>
                <option value="text">Output</option>
                <option value="artifact">Artifact</option>
              </SelectControl>
            </label>
            <label className="field">
              <span>Status</span>
              <SelectControl
                value={draft.status}
                onChange={(event) =>
                  onChange({ status: event.target.value as MemoryStatus })
                }
              >
                <option value="active">Active</option>
                <option value="superseded">Superseded</option>
                <option value="retracted">Retracted</option>
              </SelectControl>
            </label>
            <label className="field">
              <span>Importance · {draft.salience}</span>
              <input
                type="range"
                min={0}
                max={100}
                value={draft.salience}
                onChange={(event) =>
                  onChange({ salience: Number(event.target.value) })
                }
              />
            </label>
            <label className="field">
              <span>Confidence · {Math.round(draft.confidence * 100)}%</span>
              <input
                type="range"
                min={0}
                max={100}
                value={Math.round(draft.confidence * 100)}
                onChange={(event) =>
                  onChange({ confidence: Number(event.target.value) / 100 })
                }
              />
            </label>
            <label className="field">
              <span>Observed</span>
              <input
                type="datetime-local"
                value={localDateTime(draft.observedAt)}
                onChange={(event) =>
                  onChange({ observedAt: storedDateTime(event.target.value) })
                }
              />
            </label>
            <label className="field">
              <span>Last confirmed</span>
              <input
                type="datetime-local"
                value={localDateTime(draft.lastConfirmedAt)}
                onChange={(event) =>
                  onChange({
                    lastConfirmedAt: storedDateTime(event.target.value),
                  })
                }
              />
            </label>
            <label className="field">
              <span>Valid from</span>
              <input
                type="datetime-local"
                value={localDateTime(draft.validFrom)}
                onChange={(event) =>
                  onChange({ validFrom: storedDateTime(event.target.value) })
                }
              />
              <small>Leave empty to make it effective immediately.</small>
            </label>
            <label className="field">
              <span>Expires</span>
              <input
                type="datetime-local"
                value={localDateTime(draft.expiresAt)}
                onChange={(event) =>
                  onChange({ expiresAt: storedDateTime(event.target.value) })
                }
              />
              <small>Leave empty to keep it available.</small>
            </label>
          </div>
        </details>
      </fieldset>
      <footer className="memory-composer-footer">
        <span>
          {dirty
            ? "Unsaved changes"
            : creating
              ? "Ready when you are"
              : "All changes saved"}
        </span>
        <button
          type="button"
          className="ghost"
          disabled={saving}
          onClick={onCancel}
        >
          Cancel
        </button>
        <button type="submit" className="primary" disabled={!canSave}>
          {saving ? "Saving…" : creating ? "Create memory" : "Save changes"}
        </button>
      </footer>
    </form>
  );
}
