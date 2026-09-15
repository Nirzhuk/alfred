import { useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { formatNextRunLabel, formatScheduleLabel } from "../../schedule-label";
import type { MissedSchedule } from "../../types";

export type MissedDecision = "run" | "skip";

type ListProps = {
  /** One row per schedule that was due while Alfred was closed. */
  missed: MissedSchedule[];
  error: string | null;
  onResolve: (scheduleId: string, decision: MissedDecision) => Promise<void>;
  onClose: () => void;
};

/**
 * The open-time prompt for schedules set to “Ask me each time”: every missed
 * run is listed with its own Run now / Skip choice, so one schedule’s answer
 * never decides another’s. Exported without the modal shell so tests can
 * render the list statically.
 */
export function MissedSchedulesList({
  missed,
  error,
  onResolve,
  onClose,
}: ListProps) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [skippingAll, setSkippingAll] = useState(false);
  const busy = busyId !== null || skippingAll;

  const resolve = async (scheduleId: string, decision: MissedDecision) => {
    setBusyId(scheduleId);
    try {
      await onResolve(scheduleId, decision);
    } finally {
      setBusyId(null);
    }
  };

  // Best effort: a row that fails stays listed and keeps its own answer.
  const skipAll = async () => {
    setSkippingAll(true);
    try {
      for (const item of missed) {
        await onResolve(item.scheduleId, "skip");
      }
    } finally {
      setSkippingAll(false);
    }
  };

  return (
    <div className="missed-schedules-body">
      {error ? (
        <p className="error" role="alert">
          {error}
        </p>
      ) : null}

      <ul className="missed-schedules-list">
        {missed.map((item) => (
          <li key={item.scheduleId} className="missed-schedules-row">
            <div className="missed-schedules-main">
              <p className="missed-schedules-name">{item.workflowName}</p>
              <p className="missed-schedules-meta">
                {formatScheduleLabel(item.cron, item.missedAt)}
                <span aria-hidden> · </span>
                due {formatNextRunLabel(item.missedAt) ?? item.missedAt}
              </p>
            </div>
            <div className="missed-schedules-actions">
              <button
                type="button"
                className="primary"
                disabled={busy}
                aria-label={`Run ${item.workflowName} now`}
                onClick={() => void resolve(item.scheduleId, "run")}
              >
                {busyId === item.scheduleId ? "Running…" : "Run now"}
              </button>
              <button
                type="button"
                className="ghost"
                disabled={busy}
                aria-label={`Skip ${item.workflowName}`}
                onClick={() => void resolve(item.scheduleId, "skip")}
              >
                Skip
              </button>
            </div>
          </li>
        ))}
      </ul>

      <div className="missed-schedules-footer">
        <button
          type="button"
          className="ghost"
          disabled={busy}
          onClick={() => void skipAll()}
        >
          Skip all
        </button>
        <button type="button" className="ghost" onClick={onClose}>
          Decide later
        </button>
      </div>
    </div>
  );
}

export function MissedSchedulesModal(props: ListProps) {
  return (
    <Modal
      size="md"
      onClose={props.onClose}
      labelledBy="missed-schedules-title"
      describedBy="missed-schedules-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="clock" size={20} />
          </span>
        }
        title="Scheduled runs were missed"
        titleId="missed-schedules-title"
        description="Alfred was closed when these schedules were due. Choose what to do for each."
        descriptionId="missed-schedules-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={props.onClose}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />
      <MissedSchedulesList {...props} />
    </Modal>
  );
}
