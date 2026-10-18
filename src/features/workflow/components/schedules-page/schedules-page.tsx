import { useCallback, useEffect, useRef, useState } from "react";
import { ConfirmDialog } from "../../../../components/confirm-dialog";
import { useWorkflowCapability } from "../../capability";
import {
  FREE_SCHEDULE_COUNT,
  freeScheduleWorkflowId,
  lockedExtraWorkflowIds,
} from "../../schedule-slot";
import { useWorkflowStore } from "../../store";
import { openLockedCapabilityCheckout } from "../../../licensing/components/locked-capability/locked-capability";
import * as api from "../../api";
import { describeScheduleUtc, formatNextRunWithZone } from "../../schedule-label";
import { type OnMissed, type ScheduleListItem } from "../../types";

/** Only the choices that differ from the default catch-up are worth a badge. */
function missedPolicyLabel(policy: OnMissed): string | null {
  if (policy === "ask") return "Asks when missed";
  if (policy === "skip") return "Skips missed runs";
  return null;
}

type Props = {
  onClose: () => void;
  /** Open the workflow and focus the schedule node this row describes. */
  onOpenWorkflow: (target: ScheduleTarget) => void;
  /** Same, and open the schedule settings. */
  onEditSchedule: (target: ScheduleTarget) => void;
};

type ScheduleTarget = {
  workflowId: string;
  workflowName: string;
  scheduleId: string;
};

export function SchedulesPage({
  onClose,
  onOpenWorkflow,
  onEditSchedule,
}: Props) {
  const [rows, setRows] = useState<ScheduleListItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [pendingRemove, setPendingRemove] = useState<ScheduleListItem | null>(null);
  const gate = useWorkflowCapability("schedules");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await api.listSchedules();
      setRows(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const enabledCount = rows.filter((r) => r.enabled).length;
  const freeId = freeScheduleWorkflowId(rows);
  const overLimit =
    !gate.available && rows.length > FREE_SCHEDULE_COUNT && freeId !== null;

  // Over-limit extras are forced off once the license check lands: without a
  // license only the free slot may run. The user still chooses to keep the
  // paused rows or remove them.
  const forcingRef = useRef(false);
  useEffect(() => {
    if (loading || !overLimit || forcingRef.current) return;
    const extraIds = new Set(lockedExtraWorkflowIds(rows));
    const targets = rows.filter((r) => r.enabled && extraIds.has(r.workflowId));
    if (targets.length === 0) return;
    forcingRef.current = true;
    void (async () => {
      try {
        const updated = await Promise.all(
          targets.map((row) =>
            api.upsertWorkflowSchedule({
              workflowId: row.workflowId,
              cron: row.cron,
              enabled: false,
              onMissed: row.onMissed,
            }),
          ),
        );
        const fresh = new Map(updated.map((s) => [s.workflowId, s]));
        setRows((prev) =>
          prev.map((row) => {
            const next = fresh.get(row.workflowId);
            return next
              ? {
                  ...row,
                  cron: next.cron,
                  enabled: next.enabled,
                  nextRunAt: next.nextRunAt,
                  onMissed: next.onMissed,
                }
              : row;
          }),
        );
        await useWorkflowStore.getState().refreshWorkflowSchedules();
      } catch (e) {
        setError(String(e));
      } finally {
        forcingRef.current = false;
      }
    })();
  }, [loading, overLimit, rows, freeId]);

  const removeRow = async (row: ScheduleListItem) => {
    setRemovingId(row.id);
    setError(null);
    try {
      await api.deleteWorkflowSchedule(row.workflowId);
      await refresh();
      // The sidebar badges read the workflow store, not this page's rows.
      await useWorkflowStore.getState().refreshWorkflowSchedules();
    } catch (e) {
      setError(String(e));
    } finally {
      setRemovingId(null);
    }
  };
  return (
    <section className="settings-page schedules-page" aria-label="Schedules">
      <header className="settings-page-header">
        <div>
          <p className="settings-kicker">Automations</p>
          <h1>Schedules</h1>
          <p className="schedules-page-lead">
            Cron-based runs across your workflows
            {rows.length > 0
              ? ` · ${enabledCount} active of ${rows.length}`
              : ""}
            .
          </p>
        </div>
        <button type="button" className="ghost" onClick={onClose}>
          Back to canvas
        </button>
      </header>

      <div className="settings-page-body">
        {error ? <p className="error">{error}</p> : null}

        {overLimit ? (
          <div className="settings-card schedules-limit" role="note">
            <div className="schedules-limit-main">
              <p className="settings-label">1 schedule included free</p>
              <p className="settings-value">
                Without a license only your first schedule stays editable. Remove
                the extras below, or buy a license to unlock every schedule.
              </p>
            </div>
            <button
              type="button"
              className="primary"
              onClick={() => void openLockedCapabilityCheckout()}
            >
              Buy Alfred License
            </button>
          </div>
        ) : null}

        {loading ? (
          <p className="muted">Loading schedules…</p>
        ) : rows.length === 0 ? (
          <div className="settings-card schedules-empty">
            <p className="settings-label">No schedules yet</p>
            <p className="settings-value">
              Open a workflow, right-click it in the sidebar, and choose
              Schedule… to run it on a cadence.
            </p>
          </div>
        ) : (
          <ul className="schedules-list">
            {rows.map((row) => {
              const locked = overLimit && row.workflowId !== freeId;
              const removing = removingId === row.id;
              return (
              <li key={row.id} className="schedules-card">
                <div className="schedules-card-main">
                  <div className="schedules-card-top">
                    <button
                      type="button"
                      className="schedules-card-name"
                      onClick={() =>
                        onOpenWorkflow({
                          workflowId: row.workflowId,
                          workflowName: row.workflowName,
                          scheduleId: row.id,
                        })
                      }
                    >
                      {row.workflowName}
                    </button>
                    <span
                      className={`schedules-status${
                        row.enabled ? " is-on" : ""
                      }`}
                    >
                      {row.enabled ? "On" : "Off"}
                    </span>
                  </div>
                  <p className="schedules-card-meta">
                    {describeScheduleUtc(row.cron) ?? row.cron}
                    <span aria-hidden> · </span>
                    {!row.enabled
                      ? "Off"
                      : (formatNextRunWithZone(row.nextRunAt) ??
                        "Next run is being scheduled")}
                    {missedPolicyLabel(row.onMissed) ? (
                      <>
                        <span aria-hidden> · </span>
                        {missedPolicyLabel(row.onMissed)}
                      </>
                    ) : null}
                  </p>
                </div>
                <div className="schedules-card-actions">
                  <button
                    type="button"
                    className="ghost"
                    disabled={locked || loading}
                    title={
                      locked
                        ? "Buy a license to edit this schedule — or remove it to stay on the free schedule"
                        : undefined
                    }
                    aria-label={
                      locked
                        ? `Edit ${row.workflowName} (locked, buy a license)`
                        : `Edit ${row.workflowName}`
                    }
                    onClick={() =>
                      onEditSchedule({
                        workflowId: row.workflowId,
                        workflowName: row.workflowName,
                        scheduleId: row.id,
                      })
                    }
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    className="ghost"
                    onClick={() =>
                      onOpenWorkflow({
                        workflowId: row.workflowId,
                        workflowName: row.workflowName,
                        scheduleId: row.id,
                      })
                    }
                  >
                    Open
                  </button>
                  <button
                    type="button"
                    className="ghost"
                    disabled={loading || removing}
                    aria-label={`Remove schedule for ${row.workflowName}`}
                    onClick={() => setPendingRemove(row)}
                  >
                    {removing ? "Removing…" : "Remove"}
                  </button>
                </div>
              </li>
              );
            })}
          </ul>
          )}

        <button
          type="button"
          className="ghost schedules-refresh"
          onClick={() => void refresh()}
          disabled={loading}
        >
          Refresh
        </button>
      </div>
      {pendingRemove ? (
        <ConfirmDialog
          title={`Remove schedule for “${pendingRemove.workflowName}”?`}
          message="This removes the schedule only. The workflow itself stays exactly as it is. This cannot be undone."
          confirmLabel="Remove schedule"
          danger
          onCancel={() => setPendingRemove(null)}
          onConfirm={() => {
            const row = pendingRemove;
            setPendingRemove(null);
            void removeRow(row);
          }}
        />
      ) : null}
    </section>
  );
}
