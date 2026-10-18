import { useEffect, useMemo, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { SelectControl } from "../../../../components/select-control";
import { LockedCapability } from "../../../licensing/components";
import {
  formatNextRunWithZone,
  previewNextRunAt,
} from "../../schedule-label";
import {
  cronToPicker,
  DEFAULT_SCHEDULE_PICKER,
  parseTimeInput,
  pickerToCron,
  pickerUsesTime,
  SCHEDULE_REPEAT_OPTIONS,
  SCHEDULE_WEEKDAYS,
  timeInputValue,
  type SchedulePickerValue,
  type ScheduleRepeat,
} from "../../schedule-picker";
import { useScheduleGate, useWorkflowCapability } from "../../capability";
import { useWorkflowStore } from "../../store";
import * as api from "../../api";
import type { CapabilityDecision } from "../../../licensing";
import type { OnMissed, Schedule, ScheduleListItem } from "../../types";

type Props = {
  workflowId: string;
  workflowName: string;
  onClose: () => void;
  /** Test seam: overrides the resolver's answer. Production never passes it;
   * the build kind is compile-time fact either way. */
  decision?: CapabilityDecision;
};

function toggleDay(days: number[], day: number): number[] {
  if (days.includes(day)) {
    const next = days.filter((value) => value !== day);
    return next.length > 0 ? next : days;
  }
  return [...days, day];
}

export function ScheduleModal({ workflowId, workflowName, onClose, decision }: Props) {
  const workflowSchedules = useWorkflowStore((s) => s.workflowSchedules);
  const baseCapability = useWorkflowCapability("schedules");
  // The store list can miss rows another surface wrote, which would wrongly
  // grant the free slot. The database list is the gate's source of truth.
  const [gateRows, setGateRows] = useState<ScheduleListItem[] | null>(null);
  useEffect(() => {
    let live = true;
    setGateRows(null);
    void api.listSchedules().then(
      (list) => {
        if (live) setGateRows(list);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [workflowId]);
  const resolved = useScheduleGate(workflowId, gateRows ?? workflowSchedules);
  const gate = decision ?? resolved;
  // While the truth loads on a locked build, Save stays off so a stale store
  // list can never sneak an extra schedule past the gate.
  const gateListPending = !baseCapability.available && gateRows === null;
  const schedule = useWorkflowStore((s) => s.schedule);
  const loading = useWorkflowStore((s) => s.loading);
  const loadSchedule = useWorkflowStore((s) => s.loadSchedule);
  const saveSchedule = useWorkflowStore((s) => s.saveSchedule);
  const clearSchedule = useWorkflowStore((s) => s.clearSchedule);

  const [enabled, setEnabled] = useState(false);
  const [picker, setPicker] = useState<SchedulePickerValue>(DEFAULT_SCHEDULE_PICKER);
  const [cronMode, setCronMode] = useState(false);
  const [customCron, setCustomCron] = useState("");
  const [onMissed, setOnMissed] = useState<OnMissed>("run");
  // The stored scheduler's parser is the authority on a rule's next time: the
  // editor's own preview only understands the everyday subset.
  const [cronPreview, setCronPreview] = useState<{
    cron: string;
    nextRunAt: string | null;
    error: string | null;
  } | null>(null);

  useEffect(() => {
    void loadSchedule(workflowId);
  }, [workflowId, loadSchedule]);

  useEffect(() => {
    if (!schedule || schedule.workflowId !== workflowId) {
      setEnabled(false);
      setPicker(DEFAULT_SCHEDULE_PICKER);
      setCronMode(false);
      setCustomCron("");
      setOnMissed("run");
      return;
    }

    setEnabled(schedule.enabled);
    setOnMissed(schedule.onMissed ?? "run");
    const parsed = cronToPicker(schedule.cron);
    if (parsed) {
      setPicker(parsed);
      setCronMode(false);
      setCustomCron("");
    } else {
      setCronMode(true);
      setCustomCron(schedule.cron);
    }
  }, [schedule, workflowId]);

  const cron = useMemo(() => {
    if (cronMode) return customCron.trim();
    return pickerToCron(picker);
  }, [cronMode, customCron, picker]);

  const scheduleForWorkflow =
    schedule?.workflowId === workflowId ? schedule : null;

  useEffect(() => {
    if (!isTauri() || !cron) {
      setCronPreview(null);
      return;
    }
    let live = true;
    // Debounced: a cron expression is edited keystroke by keystroke.
    const timeout = window.setTimeout(() => {
      void api.previewScheduleNextRun(cron).then(
        (nextRunAt) => {
          if (live) setCronPreview({ cron, nextRunAt, error: null });
        },
        (error) => {
          if (live) {
            setCronPreview({
              cron,
              nextRunAt: null,
              error: typeof error === "string" ? error : String(error),
            });
          }
        },
      );
    }, 250);
    return () => {
      live = false;
      window.clearTimeout(timeout);
    };
  }, [cron]);

  const savedNextRun =
    scheduleForWorkflow?.cron === cron && scheduleForWorkflow.enabled
      ? formatNextRunWithZone(scheduleForWorkflow.nextRunAt)
      : null;
  const preview =
    isTauri() || !cron
      ? cronPreview?.cron === cron
        ? cronPreview
        : null
      : {
          cron,
          nextRunAt: previewNextRunAt(cron)?.toISOString() ?? null,
          error: null,
        };
  const nextRunLabel =
    savedNextRun ?? formatNextRunWithZone(preview?.nextRunAt ?? null);
  const showTime = !cronMode && pickerUsesTime(picker.repeat);

  return (
    <Modal
      size="md"
      onClose={onClose}
      labelledBy="schedule-modal-title"
      describedBy="schedule-modal-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="clock" size={20} />
          </span>
        }
        title={`Schedule ${workflowName}`}
        titleId="schedule-modal-title"
        description="Choose when this automation should run while Alfred is open."
        descriptionId="schedule-modal-description"
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

      {gate.available ? (
        <div className="schedule-modal-body">
          <label className="field checkbox-field">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
            />
            <span>Enable schedule</span>
          </label>

          {cronMode ? (
            <label className="field">
              <span>Cron expression</span>
              <input
                type="text"
                value={customCron}
                placeholder="0 0 9 * * 1-5"
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
                onChange={(e) => setCustomCron(e.target.value)}
              />
            </label>
          ) : (
            <>
              <label className="field">
                <span>Repeat</span>
                <SelectControl
                  value={picker.repeat}
                  onChange={(e) => {
                    const repeat = e.target.value as ScheduleRepeat;
                    setPicker((current) => ({
                      ...current,
                      repeat,
                      days:
                        repeat === "weekly" && current.days.length === 0
                          ? [1]
                          : current.days,
                    }));
                  }}
                >
                  {SCHEDULE_REPEAT_OPTIONS.map((option) => (
                    <option key={option.id} value={option.id}>
                      {option.label}
                    </option>
                  ))}
                </SelectControl>
              </label>

              {picker.repeat === "weekly" ? (
                <div className="field">
                  <span id="schedule-days-label">Days</span>
                  <div
                    className="schedule-day-row"
                    role="group"
                    aria-labelledby="schedule-days-label"
                  >
                    {SCHEDULE_WEEKDAYS.map((day) => {
                      const selected = picker.days.includes(day.value);
                      return (
                        <button
                          key={day.value}
                          type="button"
                          className={
                            selected ? "schedule-day is-selected" : "schedule-day"
                          }
                          aria-pressed={selected}
                          aria-label={day.name}
                          title={day.name}
                          onClick={() =>
                            setPicker((current) => ({
                              ...current,
                              days: toggleDay(current.days, day.value),
                            }))
                          }
                        >
                          {day.label}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ) : null}

              {showTime ? (
                <label className="field">
                  <span>Time</span>
                  <input
                    type="time"
                    className="schedule-time-input"
                    value={timeInputValue(picker.hour, picker.minute)}
                    step={60}
                    onChange={(e) => {
                      const next = parseTimeInput(e.target.value);
                      if (!next) return;
                      setPicker((current) => ({ ...current, ...next }));
                    }}
                  />
                </label>
              ) : null}
            </>
          )}

          {preview?.error ? (
            <div className="field">
              <span>Next run</span>
              <p className="schedule-next-run error">{preview.error}</p>
            </div>
          ) : nextRunLabel ? (
            <div className="field">
              <span>{savedNextRun ? "Next run" : "Next run (preview)"}</span>
              <p className="schedule-next-run">{nextRunLabel}</p>
            </div>
          ) : null}

          <p className="hint">
            Recurrences are stored in UTC. Times in the canvas and the Schedules
            list are shown in your own zone, so they can shift by an hour when
            daylight saving changes.
          </p>

          <label className="field">
            <span>If Alfred was closed at the scheduled time</span>
            <SelectControl
              value={onMissed}
              onChange={(e) => setOnMissed(e.target.value as OnMissed)}
            >
              <option value="run">Run when Alfred opens</option>
              <option value="skip">Skip the missed run</option>
              <option value="ask">Ask me each time</option>
            </SelectControl>
          </label>
          <p className="hint">
            {onMissed === "run"
              ? "The run starts as soon as Alfred opens."
              : onMissed === "skip"
                ? "The missed run is dropped; the schedule keeps its cadence."
                : "Missed runs wait in a list where you choose per schedule."}
          </p>

          <label className="field checkbox-field">
            <input
              type="checkbox"
              checked={cronMode}
              onChange={(e) => {
                const next = e.target.checked;
                if (next) {
                  setCustomCron(cron || "0 0 9 * * 1-5");
                  setCronMode(true);
                  return;
                }
                const parsed = cronToPicker(customCron);
                if (parsed) setPicker(parsed);
                setCronMode(false);
              }}
            />
            <span>Use a cron expression</span>
          </label>

          <div className="schedule-actions">
            <button
              type="button"
              className="primary"
              disabled={loading || !cron || gateListPending}
              onClick={() => {
                void (async () => {
                  await saveSchedule({ workflowId, cron, enabled, onMissed });
                  if (!useWorkflowStore.getState().error) onClose();
                })();
              }}
            >
              Save schedule
            </button>
            {scheduleForWorkflow ? (
              <button
                type="button"
                className="ghost"
                disabled={loading}
                onClick={() => {
                  void (async () => {
                    await clearSchedule(workflowId);
                    if (!useWorkflowStore.getState().error) onClose();
                  })();
                }}
              >
                Remove
              </button>
            ) : null}
          </div>
        </div>
      ) : (
        <ScheduleLockedContent
          savedSchedule={scheduleForWorkflow}
          workflowId={workflowId}
          onClose={onClose}
        />
      )}
    </Modal>
  );
}

/**
 * The locked treatment for the schedule authoring surface (Plan 008 Step 4):
 * the shared LockedCapability card, plus the saved schedule stated plainly —
 * a lock never hides or drops what is already persisted. Exported so tests
 * can render it statically without the portal-based modal shell.
 */
export function ScheduleLockedContent({
  savedSchedule,
  workflowId,
  onClose,
}: {
  savedSchedule: Schedule | null;
  /** When present with a saved schedule, the lock stays delete-only: the one
   * interaction an over-limit schedule always allows. Omitted in static tests. */
  workflowId?: string;
  onClose?: () => void;
}) {
  const clearSchedule = useWorkflowStore((s) => s.clearSchedule);
  const loading = useWorkflowStore((s) => s.loading);
  const canRemove = Boolean(savedSchedule && workflowId);
  return (
    <div className="schedule-modal-body" data-locked-capability="schedules">
      <LockedCapability
        capabilityName="Schedules"
        description="Run this workflow automatically on a cron cadence while Alfred is open. Manual runs stay free in every build."
      />
      {savedSchedule ? (
        <>
          <p className="muted">
            Saved schedule kept (
            {savedSchedule.enabled ? "enabled" : "paused"}):{" "}
            <code>{savedSchedule.cron}</code>. Nothing was removed.
          </p>
          {canRemove ? (
            <div className="schedule-actions">
              <button
                type="button"
                className="ghost"
                disabled={loading}
                aria-label={`Remove schedule for ${savedSchedule.workflowId}`}
                onClick={() => {
                  void (async () => {
                    await clearSchedule(workflowId as string);
                    if (!useWorkflowStore.getState().error) onClose?.();
                  })();
                }}
              >
                Remove schedule
              </button>
            </div>
          ) : null}
        </>
      ) : null}
    </div>
  );
}
