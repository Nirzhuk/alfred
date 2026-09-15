import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { formatNextRunLabel, formatScheduleLabel } from "../../schedule-label";
import { MissedSchedulesList } from "./missed-schedules-modal";
import type { MissedSchedule } from "../../types";

const MISSED: MissedSchedule[] = [
  {
    scheduleId: "sched-morning",
    workflowId: "wf-report",
    workflowName: "Morning release digest",
    cron: "0 0 9 * * 1-5",
    missedAt: "2026-09-14T09:00:00Z",
  },
  {
    scheduleId: "sched-weekly",
    workflowId: "wf-review",
    workflowName: "Weekly dependency review",
    cron: "0 0 9 * * 1",
    missedAt: "2026-09-07T09:00:00Z",
  },
];

function render(missed: MissedSchedule[] = MISSED, error: string | null = null): string {
  return renderToStaticMarkup(
    <MissedSchedulesList
      missed={missed}
      error={error}
      onResolve={async () => {}}
      onClose={() => {}}
    />,
  );
}

describe("missed schedule prompt", () => {
  test("lists one Run now / Skip pair per missed schedule", () => {
    const markup = render();

    expect(markup.match(/>Run now</g)).toHaveLength(MISSED.length);
    expect(markup.match(/>Skip</g)).toHaveLength(MISSED.length);
    for (const item of MISSED) {
      expect(markup).toContain(item.workflowName);
      // Each answer names its own workflow, so repeated buttons stay distinct.
      expect(markup).toContain(`aria-label="Run ${item.workflowName} now"`);
      expect(markup).toContain(`aria-label="Skip ${item.workflowName}"`);
      expect(markup).toContain(formatScheduleLabel(item.cron, item.missedAt));
      expect(markup).toContain(formatNextRunLabel(item.missedAt)!);
    }
  });

  test("shows only the schedules still waiting on an answer", () => {
    const markup = render([MISSED[0]]);
    expect(markup).toContain("Morning release digest");
    expect(markup).not.toContain("Weekly dependency review");
  });

  test("announces a failed answer instead of dropping the row", () => {
    const markup = render(MISSED, "This workflow is already running");
    expect(markup).toContain('role="alert"');
    expect(markup).toContain("This workflow is already running");
    expect(markup).toContain("Morning release digest");
  });

  test("offers a bulk answer and a way to decide later", () => {
    const markup = render();
    expect(markup).toContain(">Skip all</button>");
    expect(markup).toContain(">Decide later</button>");
  });
});
