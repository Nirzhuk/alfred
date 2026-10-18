import { Handle, Position, type NodeProps } from "@xyflow/react";
import { Icon } from "../../../../components/icon";
import { useScheduleGate } from "../../capability";
import { describeScheduleUtc, formatNextRunWithZone } from "../../schedule-label";
import { useWorkflowStore } from "../../store";
import type {
  Schedule,
  ScheduleCanvasNode,
  SchedulePreview,
} from "../../types";
import "./schedule-node.css";

/** What the card shows. Kept free of store and React Flow so it can be tested. */
export type ScheduleNodeCardProps = {
  schedule: Schedule | null;
  /** Backend answer for the saved cron; tells an invalid rule from an unarmed one. */
  preview: SchedulePreview | null;
  /** Entitlement: turning a schedule on needs an available slot, off never does. */
  locked: boolean;
  /** A workflow with no steps yet: the schedule runs, but there is nothing to run. */
  emptyGraph: boolean;
  pending: boolean;
  error: string | null;
  onToggle: (enabled: boolean) => void;
  onEdit: () => void;
  onRemove: () => void;
};

function ScheduleMark() {
  return (
    <span className="schedule-node-mark">
      <Icon name="clock" size={16} />
    </span>
  );
}

export function ScheduleNodeCard({
  schedule,
  preview,
  locked,
  emptyGraph,
  pending,
  error,
  onToggle,
  onEdit,
  onRemove,
}: ScheduleNodeCardProps) {
  const enabled = schedule?.enabled ?? false;
  const cron = schedule?.cron ?? null;
  const invalid = Boolean(cron && preview?.cron === cron && preview.error);
  const nextRun =
    enabled && !invalid ? formatNextRunWithZone(schedule?.nextRunAt) : null;

  const state = !cron
    ? "No schedule saved"
    : invalid
      ? `Not a valid schedule: ${cron}`
      : !enabled
        ? locked
          ? "Off — this build’s license covers one schedule"
          : "Off — nothing runs until it is on"
        : nextRun
          ? `Next: ${nextRun}`
          : "Next run is being scheduled";

  return (
    <div className="wf-node wf-node-schedule">
      {/* Generated start arrows leave from here; nothing connects into a
          schedule, and the port never starts a manual branch. */}
      <Handle
        className="wf-handle"
        type="source"
        position={Position.Right}
        isConnectable={false}
      />
      <div className="wf-node-header">
        <ScheduleMark />
        <div className="wf-node-title">Schedule</div>
        <span className={`schedules-status${enabled ? " is-on" : ""}`}>
          {enabled ? "On" : "Off"}
        </span>
      </div>
      <div className="wf-node-content">
        <p className="wf-node-body schedule-node-recurrence">
          {cron ? (describeScheduleUtc(cron) ?? cron) : "Add a schedule"}
        </p>
        <p className="schedule-node-state muted">{state}</p>
        {emptyGraph && !invalid ? (
          <p className="schedule-node-state muted">
            Add a step for this schedule to start
          </p>
        ) : null}
        {error ? (
          <p className="schedule-node-error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="schedule-node-actions">
          <button
            type="button"
            className={`settings-toggle${enabled ? " is-on" : ""}`}
            role="switch"
            aria-checked={enabled}
            aria-label={
              enabled ? "Turn the schedule off" : "Turn the schedule on"
            }
            title={
              locked && !enabled
                ? "Buy a license to run more than one schedule"
                : enabled
                  ? "Turn the schedule off"
                  : "Turn the schedule on"
            }
            disabled={pending || !cron || (locked && !enabled) || invalid}
            onClick={(event) => {
              // The card body opens the schedule; the switch only switches.
              event.stopPropagation();
              onToggle(!enabled);
            }}
          >
            <span className="settings-toggle-knob" aria-hidden />
          </button>
          <span className="schedule-node-hint muted">
            {pending ? "Saving…" : enabled ? "Enabled" : "Disabled"}
          </span>
          <div className="schedule-node-buttons">
            <button
              type="button"
              className="ghost schedule-node-icon-button"
              title="Edit schedule"
              aria-label="Edit schedule"
              onClick={(event) => {
                event.stopPropagation();
                onEdit();
              }}
            >
              <Icon name="pencil-simple" size={14} />
            </button>
            <button
              type="button"
              className="ghost danger schedule-node-icon-button"
              title="Remove schedule"
              aria-label="Remove schedule"
              onClick={(event) => {
                event.stopPropagation();
                onRemove();
              }}
            >
              <Icon name="trash" size={14} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * The workflow's schedule on the canvas. Everything it shows comes from the
 * canonical `schedules` record: the node never keeps its own copy of cron,
 * enabled state, or the next run.
 */
export function ScheduleNode({ data }: NodeProps<ScheduleCanvasNode>) {
  const schedule = useWorkflowStore((s) =>
    s.schedule?.id === data.scheduleId ? s.schedule : null,
  );
  const preview = useWorkflowStore((s) =>
    s.schedulePreview?.cron === schedule?.cron ? s.schedulePreview : null,
  );
  const pending = useWorkflowStore((s) => s.scheduleTogglePending);
  const error = useWorkflowStore((s) => s.scheduleCanvasError);
  const setScheduleEnabled = useWorkflowStore((s) => s.setScheduleEnabled);
  const emptyGraph = useWorkflowStore((s) => s.nodes.length === 0);
  const schedules = useWorkflowStore((s) => s.workflowSchedules);
  const locked = !useScheduleGate(data.workflowId, schedules).available;

  return (
    <ScheduleNodeCard
      schedule={schedule}
      preview={preview}
      locked={locked}
      emptyGraph={emptyGraph}
      pending={pending}
      error={error}
      onToggle={(enabled) => {
        void setScheduleEnabled({
          workflowId: data.workflowId,
          scheduleId: data.scheduleId,
          enabled,
        });
      }}
      // Both actions belong to the canvas, which owns the settings modal and
      // the confirmation; the node only asks for them.
      onEdit={() => window.dispatchEvent(new Event("alfred:open-schedule"))}
      onRemove={() => window.dispatchEvent(new Event("alfred:remove-schedule"))}
    />
  );
}
