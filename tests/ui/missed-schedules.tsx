import { useState } from "react";
import { createRoot } from "react-dom/client";
import "../../src/App.css";
import { applyResolvedTheme, useThemeStore } from "../../src/features/settings/theme";
import { MissedSchedulesModal } from "../../src/features/workflow/components/missed-schedules-modal";
import type { MissedSchedule } from "../../src/features/workflow/types";

/**
 * Render-only fixture for the open-time missed-run prompt. No IPC: the
 * component is presentational, and every answer is recorded here instead of
 * starting a real run.
 */

const params = new URLSearchParams(location.search);
const scenario = params.get("scenario") ?? "normal";
const theme = params.get("theme") === "light" ? "light" : "dark";
applyResolvedTheme(theme);
useThemeStore.setState({ preference: theme, resolved: theme });

const rows: MissedSchedule[] = [
  {
    scheduleId: "sched-morning",
    workflowId: "wf-report",
    workflowName: "Morning release digest",
    cron: "0 0 9 * * 1-5",
    missedAt: "2026-09-14T09:00:00Z",
  },
  {
    scheduleId: "sched-hourly",
    workflowId: "wf-inbox",
    workflowName: "Inbox sweep",
    cron: "0 0 * * * *",
    missedAt: "2026-09-14T11:00:00Z",
  },
  {
    scheduleId: "sched-weekly",
    workflowId: "wf-review",
    workflowName: "Weekly dependency review with a deliberately long workflow name",
    cron: "0 0 9 * * 1",
    missedAt: "2026-09-07T09:00:00Z",
  },
];
if (scenario === "single") rows.splice(1);

const decisions: string[] = [];
const errors: string[] = [];
window.addEventListener("error", (event) => errors.push(event.message));
window.addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));

function Fixture() {
  const [pending, setPending] = useState(rows);
  const [error, setError] = useState<string | null>(null);
  const [closed, setClosed] = useState(false);
  const [reopenKey, setReopenKey] = useState(0);

  const open = pending.length > 0 && !closed;

  return (
    <main style={{ padding: "2rem" }}>
      <h1>Missed schedules fixture</h1>
      <p>
        Synthetic rows only. Answers are recorded in <code>window.missedFixture.decisions</code>;
        no workflow runs.
      </p>
      <button type="button" onClick={() => { setClosed(false); setPending(rows); setError(null); setReopenKey((key) => key + 1); }}>
        Reopen prompt ({reopenKey})
      </button>
      <p role="status">
        {closed
          ? "Prompt dismissed — unanswered schedules stay due and are asked again on the next launch."
          : pending.length === 0
            ? "Every missed schedule was answered, so the prompt closed itself."
            : ""}
      </p>
      {open ? (
        <MissedSchedulesModal
          key={reopenKey}
          missed={pending}
          error={error}
          onResolve={async (scheduleId, decision) => {
            decisions.push(`${decision}:${scheduleId}`);
            if (scenario === "error" && scheduleId === "sched-hourly" && decision === "run") {
              setError("This workflow is already running");
              return;
            }
            setError(null);
            setPending((current) => current.filter((item) => item.scheduleId !== scheduleId));
          }}
          onClose={() => setClosed(true)}
        />
      ) : null}
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);

const fixture = { decisions, errors, rows };
(window as typeof window & { missedFixture?: typeof fixture }).missedFixture = fixture;
