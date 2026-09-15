import { describe, expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { LOCKED_CAPABILITY_EXPLANATION } from "../../../licensing/components/locked-capability/locked-capability";
import { ScheduleLockedContent } from "./schedule-modal";
import type { Schedule } from "../../types";

const SAVED: Schedule = {
  id: "sched-1",
  workflowId: "wf-1",
  cron: "0 0 9 * * 1-5",
  enabled: true,
  nextRunAt: null,
  onMissed: "run",
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

function render(
  savedSchedule: Schedule | null,
  workflowId?: string,
): string {
  return renderToStaticMarkup(
    <ScheduleLockedContent savedSchedule={savedSchedule} workflowId={workflowId} />,
  );
}

describe("schedule locked content", () => {
  test("uses the shared locked-capability treatment and names the perk", () => {
    const markup = render(null);
    expect(markup).toContain(">Schedules</h3>");
    expect(markup).toContain(LOCKED_CAPABILITY_EXPLANATION);
    expect(markup).toContain('data-locked-capability="schedules"');
  });

  test("states the saved schedule plainly instead of hiding it", () => {
    const markup = render(SAVED);
    expect(markup).toContain("Saved schedule kept");
    expect(markup).toContain("<code>0 0 9 * * 1-5</code>");
    expect(markup).toContain("(enabled)");
    expect(markup).toContain("Nothing was removed.");
  });

  test("a paused saved schedule says so without being dropped", () => {
    const markup = render({ ...SAVED, enabled: false });
    expect(markup).toContain("(paused)");
    expect(markup).toContain("<code>0 0 9 * * 1-5</code>");
  });

  test("with no saved schedule there is nothing extra to disclose", () => {
    const markup = render(null);
    expect(markup).not.toContain("Saved schedule kept");
  });

  test("an over-limit schedule stays delete-only instead of fully locked", () => {
    const markup = render(SAVED, "wf-1");
    expect(markup).toContain("Remove schedule");
    expect(markup).toContain('aria-label="Remove schedule for wf-1"');
  });

  test("without a workflow target there is no remove action to offer", () => {
    expect(render(SAVED)).not.toContain("Remove schedule");
    expect(render(null, "wf-1")).not.toContain("Remove schedule");
  });
});
