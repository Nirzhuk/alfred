import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { SelectControl } from "../../../../components/select-control";
import {
  EDGE_CONDITION_OPERATOR_LABELS,
  EDGE_CONDITION_SOURCE_LABELS,
  MAX_EDGE_CONDITION_VALUE_BYTES,
  defaultEdgeCondition,
  edgeCondition,
  isUnaryEdgeConditionOperator,
  type EdgeCondition,
  type EdgeConditionOperator,
  type EdgeConditionSource,
} from "../../types";
import { useWorkflowStore } from "../../store";
import { validateConditionPattern } from "../../api";
import "./edge-condition-modal.css";

const SOURCES: EdgeConditionSource[] = ["output", "context", "trigger"];

const OPERATORS: EdgeConditionOperator[] = [
  "contains",
  "notContains",
  "equals",
  "notEquals",
  "matches",
  "isEmpty",
  "isNotEmpty",
];

const SOURCE_HINTS: Record<EdgeConditionSource, string> = {
  output: "What the step this connection leaves produced on this run.",
  context: "Everything gathered so far on this run, across earlier steps.",
  trigger: "The payload that started the run. Empty for a manual run.",
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

  const edge = useMemo(
    () => edges.find((candidate) => candidate.id === edgeId) ?? null,
    [edges, edgeId],
  );

  const [draft, setDraft] = useState<EdgeCondition>(
    () => (edge && edgeCondition(edge)) ?? defaultEdgeCondition(),
  );
  // Every hook runs before the missing-edge bail-out below.
  const { error: patternError, checking } = usePatternError(
    draft.value,
    Boolean(draft.caseSensitive),
    draft.operator === "matches",
  );

  if (!edge) return null;

  const nodeLabel = (nodeId: string) => {
    const node = nodes.find((candidate) => candidate.id === nodeId);
    const label = node?.data?.label;
    return typeof label === "string" && label.trim() ? label : "this step";
  };

  const existing = edgeCondition(edge);
  const unary = isUnaryEdgeConditionOperator(draft.operator);
  const valueMissing = !unary && !draft.value.trim();
  const canSave = !patternError && !checking && !valueMissing;

  const update = (patch: Partial<EdgeCondition>) =>
    setDraft((previous) => ({ ...previous, ...patch }));

  const save = () => {
    if (!canSave) return;
    setEdgeCondition(edgeId, {
      ...draft,
      value: unary ? "" : draft.value.slice(0, MAX_EDGE_CONDITION_VALUE_BYTES),
    });
    onClose();
  };

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
          <SelectControl
            value={draft.source}
            onChange={(event) =>
              update({ source: event.target.value as EdgeConditionSource })
            }
          >
            {SOURCES.map((source) => (
              <option key={source} value={source}>
                {source === "output"
                  ? `Output of ${nodeLabel(edge.source)}`
                  : EDGE_CONDITION_SOURCE_LABELS[source]}
              </option>
            ))}
          </SelectControl>
        </label>
        <p className="hint">{SOURCE_HINTS[draft.source]}</p>

        <label className="field">
          <span>Test</span>
          <SelectControl
            value={draft.operator}
            onChange={(event) =>
              update({ operator: event.target.value as EdgeConditionOperator })
            }
          >
            {OPERATORS.map((operator) => (
              <option key={operator} value={operator}>
                {EDGE_CONDITION_OPERATOR_LABELS[operator]}
              </option>
            ))}
          </SelectControl>
        </label>

        {unary ? null : (
          <label className="field">
            <span>Value</span>
            <input
              type="text"
              value={draft.value}
              maxLength={MAX_EDGE_CONDITION_VALUE_BYTES}
              placeholder={draft.operator === "matches" ? "FAIL|error" : "FAIL"}
              aria-invalid={patternError ? true : undefined}
              aria-describedby={patternError ? "edge-condition-error" : undefined}
              onChange={(event) => update({ value: event.target.value })}
            />
          </label>
        )}

        {patternError ? (
          <p className="field-error" id="edge-condition-error" role="alert">
            {patternError}
          </p>
        ) : null}

        {unary ? null : (
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
        )}

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
