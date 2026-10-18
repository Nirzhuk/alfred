import { describe, expect, test } from "bun:test";
import {
  describeScheduleUtc,
  formatNextRunLabel,
  formatNextRunWithZone,
  formatScheduleLabel,
  previewNextRunAt,
} from "../src/features/workflow/schedule-label";

describe("formatScheduleLabel", () => {
  test("formats daily and weekday schedules", () => {
    expect(formatScheduleLabel("0 0 9 * * *")).toBe("09:00 daily");
    expect(formatScheduleLabel("0 0 9 * * 1-5")).toBe("09:00 weekdays");
    expect(formatScheduleLabel("0 30 14 * * 1")).toBe("14:30 Mondays");
  });

  test("formats recurring interval schedules", () => {
    expect(formatScheduleLabel("0 0 * * * *")).toBe("hourly");
    expect(formatScheduleLabel("0 */15 * * * *")).toBe("every 15 min");
  });

  test("uses the actual local next-run time instead of the UTC cron hour", () => {
    expect(
      formatScheduleLabel(
        "0 0 9 * * 1-5",
        "2026-08-12T09:00:00Z",
        "Europe/Madrid",
      ),
    ).toBe("11:00 weekdays");
  });

  test("keeps unsupported custom expressions accurate", () => {
    expect(formatScheduleLabel("0 0 9 1 * *")).toBe("0 0 9 1 * *");
  });
});

describe("formatNextRunLabel", () => {
  test("formats a fire time in plain language", () => {
    expect(
      formatNextRunLabel("2026-08-24T09:00:00.000Z", "UTC", "en-GB"),
    ).toMatch(/^Monday,? 24 Aug 2026 at 09:00$/);
  });

  test("hides missing or invalid times", () => {
    expect(formatNextRunLabel(null)).toBeNull();
    expect(formatNextRunLabel("")).toBeNull();
    expect(formatNextRunLabel("not-a-date")).toBeNull();
  });
});

describe("describeScheduleUtc", () => {
  test("describes everyday patterns on their stored UTC basis", () => {
    expect(describeScheduleUtc("0 0 * * * *")).toBe("Every hour");
    expect(describeScheduleUtc("0 */15 * * * *")).toBe("Every 15 minutes");
    expect(describeScheduleUtc("0 0 7 * * *")).toBe("Every day at 07:00 UTC");
    expect(describeScheduleUtc("0 0 7 * * 1-5")).toBe("Weekdays at 07:00 UTC");
    expect(describeScheduleUtc("0 0 7 * * 1")).toBe("Mondays at 07:00 UTC");
  });

  test("returns null rather than guessing at anything else", () => {
    expect(describeScheduleUtc("5 0 9 * * *")).toBeNull();
    expect(describeScheduleUtc("0 0 9 1 * *")).toBeNull();
    expect(describeScheduleUtc("0 0 9 * * 1,3")).toBeNull();
    expect(describeScheduleUtc("0 5 * * * *")).toBeNull();
  });
});

describe("formatNextRunWithZone", () => {
  test("spells out the viewer's zone beside the fire time", () => {
    expect(
      formatNextRunWithZone("2026-08-24T09:00:00.000Z", "UTC", "en-GB"),
    ).toMatch(/^Monday,? 24 Aug 2026 at 09:00 (UTC|GMT)$/);
    expect(
      formatNextRunWithZone(
        "2026-08-24T07:00:00.000Z",
        "Europe/Amsterdam",
        "en-GB",
      ),
    ).toMatch(/^Monday,? 24 Aug 2026 at 09:00 /);
  });

  test("hides missing or invalid times", () => {
    expect(formatNextRunWithZone(null)).toBeNull();
    expect(formatNextRunWithZone("not-a-date")).toBeNull();
  });
});

describe("formatScheduleLabel timezone honesty", () => {
  test("keeps the stored UTC basis when the instant lands on another local day", () => {
    // 09:00 UTC is Monday 09:00 in UTC, but Sunday 21:00 at UTC-12, where the
    // weekday set no longer matches; the stored basis is stated instead.
    expect(
      formatScheduleLabel("0 0 9 * * 1", "2026-08-24T09:00:00Z", "Etc/GMT+12"),
    ).toBe("09:00 Mondays (UTC)");
    expect(
      formatScheduleLabel("0 0 9 * * 1-5", "2026-08-24T09:00:00Z", "Etc/GMT+12"),
    ).toBe("09:00 weekdays (UTC)");
  });

  test("uses the local clock when the instant agrees with the stored day set", () => {
    expect(
      formatScheduleLabel("0 0 9 * * 1-5", "2026-08-24T09:00:00Z", "Europe/Madrid"),
    ).toBe("11:00 weekdays");
  });
});

describe("previewNextRunAt", () => {
  test("finds the next hourly and interval fire times", () => {
    expect(
      previewNextRunAt("0 0 * * * *", new Date("2026-08-19T09:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-19T10:00:00.000Z");
    expect(
      previewNextRunAt("0 */15 * * * *", new Date("2026-08-19T09:07:00.000Z"))?.toISOString(),
    ).toBe("2026-08-19T09:15:00.000Z");
  });

  test("finds the next weekday and Monday morning", () => {
    expect(
      previewNextRunAt("0 0 9 * * *", new Date("2026-08-19T09:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-20T09:00:00.000Z");
    expect(
      previewNextRunAt("0 0 9 * * 1-5", new Date("2026-08-21T09:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-24T09:00:00.000Z");
    expect(
      previewNextRunAt("0 0 9 * * 1", new Date("2026-08-19T10:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-24T09:00:00.000Z");
  });

  test("returns null for unsupported syntax", () => {
    expect(previewNextRunAt("0 0 9 1 JAN *")).toBeNull();
    expect(previewNextRunAt("not cron")).toBeNull();
  });
});
