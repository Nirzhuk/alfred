import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { SelectControl } from "../../../../components/select-control";
import {
  MAX_EDGE_CONDITION_VALUE_BYTES,
  RUN_EDGE_CONDITION_OPERATORS,
  SYSTEM_FIELD_GROUPS,
  SYSTEM_FIELD_HINTS,
  SYSTEM_FIELD_KEY_LABELS,
  SYSTEM_FIELD_LABELS,
  SYSTEM_FIELD_OPERATORS,
  SYSTEM_FIELD_OPTIONS,
  WEEKDAY_OPTIONS,
  defaultSystemCondition,
  edgeCondition,
  edgeConditionOperatorLabel,
  edgeConditionSubjectValue,
  edgeConditionValidationError,
  fieldRequiresKey,
  fieldSupportsCaseSensitivity,
  formatWeekdaySelection,
  isUnaryEdgeConditionOperator,
  operatorNeedsSecondValue,
  parseWeekdaySelection,
  type EdgeCondition,
  type EdgeConditionOperator,
  type RunEdgeConditionSource,
  type SystemField,
} from "../../types";
import { useWorkflowStore } from "../../store";
import {
  previewConditionSubject,
  validateConditionPattern,
  type ConditionSubjectPreview,
} from "../../api";
import "./edge-condition-modal.css";

const SOURCE_HINTS: Record<RunEdgeConditionSource, string> = {
  output: "What the step this connection leaves produced on this run.",
  context: "Everything gathered so far on this run, across earlier steps.",
  trigger: "The payload that started the run. Empty for a manual run.",
};

const TEXT_VALUE_PLACEHOLDERS: Partial<Record<SystemField, string>> = {
  timezone: "America/New_York",
  hostname: "macbook",
  username: "niri",
  env: "true",
  frontmostApp: "Safari",
  clipboard: "text that must appear",
};

/**
 * Ask the runner's own engine whether a pattern compiles.
 *
 * JavaScript's `RegExp` cannot answer this: it accepts look-around
 * (`FAIL(?!ED)`) that the runner rejects — which would save a condition that
 * can never fire — and rejects inline flags (`(?i)fail`) the runner accepts.
 * While a check is in flight the previous verdict stands, so the Save button
 * never flickers between keystrokes.
 */
function usePatternError(pattern: string, caseSensitive: boolean, active: boolean) {
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  // Only the newest request may write a verdict; earlier ones are stale.
  const latest = useRef(0);

  useEffect(() => {
    if (!active || !pattern) {
      setError(null);
      setChecking(false);
      return;
    }
    const request = ++latest.current;
    setChecking(true);
    const timer = setTimeout(() => {
      void validateConditionPattern(pattern, caseSensitive).then((message) => {
        if (request !== latest.current) return;
        setError(message);
        setChecking(false);
      });
    }, 200);
    return () => clearTimeout(timer);
  }, [pattern, caseSensitive, active]);

  return { error, checking };
}

type Props = {
  edgeId: string;
  onClose: () => void;
};

export function EdgeConditionModal({ edgeId, onClose }: Props) {
  const edges = useWorkflowStore((state) => state.edges);
  const setEdgeCondition = useWorkflowStore((state) => state.setEdgeCondition);
  const clearEdgeCondition = useWorkflowStore(
    (state) => state.clearEdgeCondition,
  );
  const nodes = useWorkflowStore((state) => state.nodes);
  const workflowId = useWorkflowStore((state) => state.activeWorkflowId);

  const edge = useMemo(
    () => edges.find((candidate) => candidate.id === edgeId) ?? null,
    [edges, edgeId],
  );

  const [draft, setDraft] = useState<EdgeCondition>(
    () => (edge && edgeCondition(edge)) ?? { source: "output", operator: "contains", value: "" },
  );
  const [preview, setPreview] = useState<ConditionSubjectPreview | null>(null);
  const [attempted, setAttempted] = useState(false);

  const field = draft.source === "system" ? draft.field : undefined;
  const canPreview =
    !field || !fieldRequiresKey(field) || Boolean(draft.key?.trim());
  const unary = isUnaryEdgeConditionOperator(draft.operator);
  const needsSecondValue = operatorNeedsSecondValue(draft.operator);
  const showCaseSensitivity =
    !unary &&
    (field ? fieldSupportsCaseSensitivity(field) : true) &&
    (draft.operator === "contains" ||
      draft.operator === "notContains" ||
      draft.operator === "equals" ||
      draft.operator === "notEquals" ||
      draft.operator === "matches" ||
      draft.operator === "isOneOf" ||
      draft.operator === "isNotOneOf");

  // Every hook runs before the missing-edge bail-out below.
  const { error: patternError, checking } = usePatternError(
    draft.value,
    Boolean(draft.caseSensitive),
    draft.operator === "matches",
  );
  const draftError = edgeConditionValidationError(draft);

  useEffect(() => {
    if (!field || !canPreview) {
      setPreview(null);
      return;
    }
    let cancelled = false;
    setPreview(null);
    const timer = setTimeout(
      () => {
        void previewConditionSubject(workflowId ?? "", field, draft.key)
          .then((value) => {
            if (!cancelled) setPreview(value);
          })
          .catch(() => {
            if (!cancelled) {
              setPreview({
                value: null,
                detail: null,
                unavailable: "Could not read this value.",
              });
            }
          });
      },
      fieldRequiresKey(field) ? 300 : 0,
    );
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [field, canPreview, draft.key, workflowId]);

  if (!edge) return null;

  const nodeLabel = (nodeId: string) => {
    const node = nodes.find((candidate) => candidate.id === nodeId);
    const label = node?.data?.label;
    return typeof label === "string" && label.trim() ? label : "this step";
  };

  const existing = edgeCondition(edge);
  const subjectValue = edgeConditionSubjectValue(draft);
  const operators = field
    ? SYSTEM_FIELD_OPERATORS[field]
    : RUN_EDGE_CONDITION_OPERATORS;
  const hint = field
    ? SYSTEM_FIELD_HINTS[field]
    : SOURCE_HINTS[draft.source as RunEdgeConditionSource];
  const shownError = patternError ?? (attempted ? draftError : null);

  const update = (patch: Partial<EdgeCondition>) =>
    setDraft((previous) => ({ ...previous, ...patch }));

  const changeSubject = (value: string) => {
    setAttempted(false);
    if (value.startsWith("system.")) {
      setDraft(defaultSystemCondition(value.slice("system.".length) as SystemField));
      return;
    }
    setDraft({
      source: value as RunEdgeConditionSource,
      operator: "contains",
      value: "",
      caseSensitive: false,
    });
  };

  const changeOperator = (operator: EdgeConditionOperator) => {
    setAttempted(false);
    setDraft((previous) => ({
      ...previous,
      operator,
      value2: operatorNeedsSecondValue(operator)
        ? (previous.value2 ?? (previous.field === "battery" ? "80" : "17:00"))
        : undefined,
    }));
  };

  const toggleWeekday = (day: string, checked: boolean) => {
    const days = parseWeekdaySelection(draft.value);
    update({
      value: formatWeekdaySelection(
        checked ? [...days, day] : days.filter((value) => value !== day),
      ),
    });
  };

  const canSave = !patternError && !checking && !draftError;

  const save = () => {
    if (draftError) {
      setAttempted(true);
      return;
    }
    if (!canSave) return;
    const condition: EdgeCondition = {
      source: draft.source,
      operator: draft.operator,
      value: unary
        ? ""
        : draft.value.slice(0, MAX_EDGE_CONDITION_VALUE_BYTES),
      caseSensitive: showCaseSensitivity ? Boolean(draft.caseSensitive) : false,
    };
    if (field) condition.field = field;
    if (field && fieldRequiresKey(field) && draft.key?.trim()) {
      condition.key = draft.key
        .trim()
        .slice(0, MAX_EDGE_CONDITION_VALUE_BYTES);
    }
    if (needsSecondValue && draft.value2) {
      condition.value2 = draft.value2.slice(0, MAX_EDGE_CONDITION_VALUE_BYTES);
    }
    setEdgeCondition(edgeId, condition);
    onClose();
  };

  const renderValueControl = () => {
    if (unary) return null;
    if (field === "weekday") {
      const selected = new Set(parseWeekdaySelection(draft.value));
      return (
        <div className="field">
          <span>Days</span>
          <div className="edge-condition-weekdays">
            {WEEKDAY_OPTIONS.map((option) => (
              <label key={option.value} className="edge-condition-weekday">
                <input
                  type="checkbox"
                  checked={selected.has(option.value)}
                  onChange={(event) =>
                    toggleWeekday(option.value, event.target.checked)
                  }
                />
                <span>{option.short}</span>
              </label>
            ))}
          </div>
        </div>
      );
    }
    if (field === "date") {
      return (
        <label className="field">
          <span>Date</span>
          <input
            type="date"
            value={draft.value}
            onChange={(event) => update({ value: event.target.value })}
          />
        </label>
      );
    }
    if (field === "time" || field === "battery") {
      const numeric = field === "battery";
      return (
        <div className="edge-condition-range">
          <label className="field">
            <span>{needsSecondValue ? "From" : "Value"}</span>
            <input
              type={numeric ? "number" : "time"}
              min={numeric ? 0 : undefined}
              max={numeric ? 100 : undefined}
              step={numeric ? 1 : undefined}
              value={draft.value}
              onChange={(event) => update({ value: event.target.value })}
            />
          </label>
          {needsSecondValue ? (
            <label className="field">
              <span>To</span>
              <input
                type={numeric ? "number" : "time"}
                min={numeric ? 0 : undefined}
                max={numeric ? 100 : undefined}
                step={numeric ? 1 : undefined}
                value={draft.value2 ?? ""}
                onChange={(event) => update({ value2: event.target.value })}
              />
            </label>
          ) : null}
        </div>
      );
    }
    if (field && SYSTEM_FIELD_OPTIONS[field]) {
      return (
        <label className="field">
          <span>Value</span>
          <SelectControl
            value={draft.value}
            onChange={(event) => update({ value: event.target.value })}
          >
            {SYSTEM_FIELD_OPTIONS[field]?.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </SelectControl>
        </label>
      );
    }
    if (field === "path") {
      return (
        <label className="field">
          <span>Modified</span>
          <input
            type="datetime-local"
            value={draft.value}
            onChange={(event) => update({ value: event.target.value })}
          />
        </label>
      );
    }
    return (
      <label className="field">
        <span>Value</span>
        <input
          type="text"
          value={draft.value}
          maxLength={MAX_EDGE_CONDITION_VALUE_BYTES}
          placeholder={
            (field && TEXT_VALUE_PLACEHOLDERS[field]) ??
            (draft.operator === "matches" ? "FAIL|error" : "FAIL")
          }
          aria-invalid={patternError ? true : undefined}
          aria-describedby={shownError ? "edge-condition-error" : undefined}
          onChange={(event) => update({ value: event.target.value })}
        />
      </label>
    );
  };

  const previewText =
    preview === null
      ? "Reading the current value…"
      : preview.unavailable
        ? `Unavailable: ${preview.unavailable}`
        : preview.value
          ? `Current: ${preview.value}${preview.detail ? ` · ${preview.detail}` : ""}`
          : `Current: ${preview.value === "" ? "empty" : (preview.detail ?? "not set")}`;

  return (
    <Modal
      size="sm"
      onClose={onClose}
      labelledBy="edge-condition-title"
      describedBy="edge-condition-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="sliders" size={20} />
          </span>
        }
        title="Connection condition"
        titleId="edge-condition-title"
        description={`Run ${nodeLabel(edge.target)} only when this is true. Without a condition the connection always fires.`}
        descriptionId="edge-condition-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={onClose}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />

      <div className="edge-condition-body">
        <label className="field">
          <span>Compare</span>
          <SelectControl value={subjectValue} onChange={(event) => changeSubject(event.target.value)}>
            <optgroup label="Run">
              <option value="output">Output of {nodeLabel(edge.source)}</option>
              <option value="context">Run context</option>
              <option value="trigger">Trigger payload</option>
            </optgroup>
            {SYSTEM_FIELD_GROUPS.map((group) => (
              <optgroup key={group.label} label={group.label}>
                {group.fields.map((subject) => (
                  <option key={subject} value={`system.${subject}`}>
                    {SYSTEM_FIELD_LABELS[subject]}
                  </option>
                ))}
              </optgroup>
            ))}
          </SelectControl>
        </label>
        <p className="hint">{hint}</p>

        {field && fieldRequiresKey(field) ? (
          <label className="field">
            <span>{SYSTEM_FIELD_KEY_LABELS[field]}</span>
            <input
              type="text"
              value={draft.key ?? ""}
              maxLength={MAX_EDGE_CONDITION_VALUE_BYTES}
              placeholder={field === "env" ? "CI" : "reports/latest.md"}
              aria-invalid={attempted && draftError ? true : undefined}
              onChange={(event) => update({ key: event.target.value })}
            />
          </label>
        ) : null}

        <label className="field">
          <span>Test</span>
          <SelectControl
            value={draft.operator}
            onChange={(event) =>
              changeOperator(event.target.value as EdgeConditionOperator)
            }
          >
            {operators.map((operator) => (
              <option key={operator} value={operator}>
                {edgeConditionOperatorLabel(operator, field)}
              </option>
            ))}
          </SelectControl>
        </label>

        {renderValueControl()}

        {field && canPreview ? (
          <p className="hint edge-condition-current" aria-live="polite">
            {previewText}
          </p>
        ) : null}

        {shownError ? (
          <p className="field-error" id="edge-condition-error" role="alert">
            {shownError}
          </p>
        ) : null}

        {showCaseSensitivity ? (
          <label className="edge-condition-toggle">
            <input
              type="checkbox"
              checked={Boolean(draft.caseSensitive)}
              onChange={(event) =>
                update({ caseSensitive: event.target.checked })
              }
            />
            <span>Match upper and lower case exactly</span>
          </label>
        ) : null}

        <div className="edge-condition-actions">
          {existing ? (
            <button
              type="button"
              className="ghost"
              onClick={() => {
                clearEdgeCondition(edgeId);
                onClose();
              }}
            >
              Remove condition
            </button>
          ) : (
            <span />
          )}
          <div className="edge-condition-confirm">
            <button type="button" className="ghost" onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              className="primary"
              disabled={!canSave}
              onClick={save}
            >
              {existing ? "Save condition" : "Add condition"}
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
