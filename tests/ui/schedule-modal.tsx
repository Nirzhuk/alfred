import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { createRoot } from "react-dom/client";
import { useState } from "react";
import "../../src/App.css";
import { applyResolvedTheme, useThemeStore } from "../../src/features/settings/theme";
import { ScheduleModal } from "../../src/features/workflow/components/schedule-modal";
import type { OnMissed, Schedule } from "../../src/features/workflow/types";
import { QaModalPortal } from "./qa-modal-portal";

/**
 * Real schedule modal with mocked IPC: proves the missed-run choice is a
 * modal-scoped control that loads the saved value and saves the picked one.
 * No workflow, schedule, or run leaves this browser.
 */

const params = new URLSearchParams(location.search);
const policy = (params.get("policy") ?? "ask") as OnMissed;
const theme = params.get("theme") === "light" ? "light" : "dark";
applyResolvedTheme(theme);
useThemeStore.setState({ preference: theme, resolved: theme });

const calls: { command: string; payload: unknown }[] = [];
const errors: string[] = [];
window.addEventListener("error", (event) => errors.push(event.message));
window.addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));

const saved: Schedule = {
  id: "sched-morning",
  workflowId: "wf-report",
  cron: "0 0 9 * * 1-5",
  enabled: true,
  nextRunAt: null,
  onMissed: policy,
  createdAt: "2026-09-01T08:00:00Z",
  updatedAt: "2026-09-01T08:00:00Z",
};

mockWindows("main");
mockIPC(
  async (command, payload) => {
    calls.push({ command, payload: structuredClone(payload ?? {}) });
    switch (command) {
      case "get_workflow_schedule":
        return params.get("policy") === "none" ? null : saved;
      case "upsert_workflow_schedule": {
        const input = payload as { cron: string; enabled: boolean; onMissed: OnMissed };
        return { ...saved, ...input };
      }
      case "delete_workflow_schedule":
        return null;
      default:
        if (command.startsWith("plugin:")) return null;
        throw new Error(`Unmocked schedule fixture IPC: ${command}`);
    }
  },
  { shouldMockEvents: true },
);

function Fixture() {
  const [open, setOpen] = useState(true);
  return (
    <main style={{ padding: "2rem" }}>
      <h1>Schedule modal fixture</h1>
      <p>
        Saved policy: <code>{policy}</code>. Calls are recorded in{" "}
        <code>window.scheduleFixture.calls</code>.
      </p>
      <button type="button" onClick={() => setOpen(true)}>
        Reopen schedule modal
      </button>
      <p role="status">{open ? "" : "Modal closed by its own action."}</p>
      {open ? (
        <ScheduleModal
          workflowId="wf-report"
          workflowName="Morning release digest"
          decision={{ available: true, reason: null }}
          onClose={() => setOpen(false)}
        />
      ) : null}
      <QaModalPortal>
        <p data-qa="last-call">
          {calls.length === 0 ? "no calls yet" : JSON.stringify(calls[calls.length - 1])}
        </p>
      </QaModalPortal>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);

const fixture = { calls, errors, saved };
(window as typeof window & { scheduleFixture?: typeof fixture }).scheduleFixture = fixture;
