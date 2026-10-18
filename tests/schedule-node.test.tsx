import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ReactFlowProvider } from "@xyflow/react";
import { ScheduleNodeCard } from "../src/features/workflow/components/schedule-node";
import type { Schedule } from "../src/features/workflow/types";

const schedule: Schedule = {
  id: "schedule-1",
  workflowId: "workflow-1",
  cron: "0 0 7 * * 1-5",
  enabled: true,
  nextRunAt: "2026-09-10T07:00:00Z",
  onMissed: "run",
  createdAt: "2026-09-01T00:00:00Z",
  updatedAt: "2026-09-01T00:00:00Z",
};

type CardProps = Parameters<typeof ScheduleNodeCard>[0];

function renderCard(overrides: Partial<CardProps> = {}) {
  const props: CardProps = {
    schedule,
    preview: { cron: schedule.cron, nextRunAt: schedule.nextRunAt, error: null },
    locked: false,
    emptyGraph: false,
    pending: false,
    error: null,
    onToggle: () => {},
    onEdit: () => {},
    onRemove: () => {},
    ...overrides,
  };
  return renderToStaticMarkup(
    <ReactFlowProvider>
      <ScheduleNodeCard {...props} />
    </ReactFlowProvider>,
  );
}

describe("ScheduleNodeCard state", () => {
  test("states the recurrence on its stored UTC basis beside the zone-labelled next run", () => {
    const markup = renderCard();
    expect(markup).toContain("Weekdays at 07:00 UTC");
    expect(markup).toMatch(/Next: [^<]*\d{2}:\d{2} \S/);
    expect(markup).toContain('aria-checked="true"');
  });

  test("shows a disabled schedule as off instead of inventing a next run", () => {
    const markup = renderCard({
      schedule: { ...schedule, enabled: false, nextRunAt: null },
    });
    expect(markup).toContain("Off — nothing runs until it is on");
    expect(markup).not.toContain("Next: ");
  });

  test("an unarmed schedule says so rather than showing a stale time", () => {
    const markup = renderCard({ schedule: { ...schedule, nextRunAt: null } });
    expect(markup).toContain("Next run is being scheduled");
  });

  test("an invalid expression is reported instead of being described as a rule", () => {
    const markup = renderCard({
      schedule: { ...schedule, cron: "nope" },
      preview: { cron: "nope", nextRunAt: null, error: "invalid cron `nope`" },
    });
    expect(markup).toContain("Not a valid schedule: nope");
    expect(markup).toContain('disabled=""');
    expect(markup).not.toContain("Next: ");
  });

  test("under a license that cannot enable it, the switch only offers turning off", () => {
    const enabledMarkup = renderCard({ locked: true });
    expect(enabledMarkup).toContain('aria-checked="true"');
    expect(enabledMarkup).not.toContain('disabled=""');

    const pausedMarkup = renderCard({
      locked: true,
      schedule: { ...schedule, enabled: false, nextRunAt: null },
    });
    expect(pausedMarkup).toContain('aria-checked="false"');
    expect(pausedMarkup).toContain('disabled=""');
    expect(pausedMarkup).toContain("Buy a license");
    expect(pausedMarkup).toContain(
      "Off — this build’s license covers one schedule",
    );
  });

  test("reports a pending write and a refused one without hiding the saved rule", () => {
    const pending = renderCard({ pending: true });
    expect(pending).toContain("Saving…");
    expect(pending).toContain('disabled=""');
    expect(pending).toContain("Weekdays at 07:00 UTC");

    const failed = renderCard({ error: "schedule not found: schedule-1" });
    expect(failed).toContain('role="alert"');
    expect(failed).toContain("schedule not found: schedule-1");
  });

  test("says what is missing when the workflow has no steps yet", () => {
    expect(renderCard({ emptyGraph: true })).toContain(
      "Add a step for this schedule to start",
    );
    expect(renderCard()).not.toContain("Add a step for this schedule to start");
  });

  test("asks to turn the schedule off when it is on, and on when it is off", () => {
    expect(renderCard()).toContain('aria-label="Turn the schedule off"');
    expect(
      renderCard({ schedule: { ...schedule, enabled: false, nextRunAt: null } }),
    ).toContain('aria-label="Turn the schedule on"');
  });
});
