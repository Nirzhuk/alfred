const WEEKDAYS: Record<string, string> = {
  "0": "Sundays",
  "1": "Mondays",
  "2": "Tuesdays",
  "3": "Wednesdays",
  "4": "Thursdays",
  "5": "Fridays",
  "6": "Saturdays",
  "7": "Sundays",
};

const WEEKDAY_INDEX: Record<string, number> = {
  Sun: 0,
  Mon: 1,
  Tue: 2,
  Wed: 3,
  Thu: 4,
  Fri: 5,
  Sat: 6,
};

const SIMPLE_FIELD = /^(?:\*|\*\/\d+|\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)$/;

function clockTime(hour: string, minute: string): string | null {
  if (!/^\d{1,2}$/.test(hour) || !/^\d{1,2}$/.test(minute)) return null;
  const hours = Number(hour);
  const minutes = Number(minute);
  if (hours > 23 || minutes > 59) return null;
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
}

/**
 * Local clock time and weekday of a fire instant, or null when the value is
 * missing or unreadable.
 */
function instantInZone(
  value: string | null | undefined,
  timeZone?: string,
): { time: string; weekday: number } | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  const parts = new Intl.DateTimeFormat("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    weekday: "short",
    timeZone,
  }).formatToParts(date);
  const hour = parts.find((part) => part.type === "hour")?.value;
  const minute = parts.find((part) => part.type === "minute")?.value;
  const weekday = parts.find((part) => part.type === "weekday")?.value;
  if (!hour || !minute || weekday == null) return null;
  return { time: `${hour}:${minute}`, weekday: WEEKDAY_INDEX[weekday] ?? 0 };
}

/** Whether a UTC day-of-week field covers the given weekday. */
function dayOfWeekMatches(pattern: string, weekday: number): boolean {
  if (pattern === "*") return true;
  if (pattern === "1-5") return weekday >= 1 && weekday <= 5;
  if (!/^\d+$/.test(pattern)) return false;
  return Number(pattern) % 7 === weekday;
}

/** IANA zone of the viewer, or `local` when the environment cannot name it. */
function localTimeZoneLabel(timeZone?: string): string {
  try {
    return new Intl.DateTimeFormat(undefined, { timeZone }).resolvedOptions()
      .timeZone;
  } catch {
    return "local";
  }
}

/** Compact, human-readable text for a six-field Alfred cron expression. */
export function formatScheduleLabel(
  cron: string,
  nextRunAt?: string | null,
  timeZone?: string,
): string {
  const fields = cron.trim().split(/\s+/);
  if (fields.length !== 6) return cron;

  const [, minute, hour, dayOfMonth, month, dayOfWeek] = fields;
  if (dayOfMonth === "*" && month === "*" && dayOfWeek === "*") {
    if (hour === "*" && minute === "0") return "hourly";
    const interval = minute.match(/^\*\/(\d+)$/)?.[1];
    if (hour === "*" && interval) return `every ${interval} min`;
  }

  if (dayOfMonth !== "*" || month !== "*") return cron;
  const instant = instantInZone(nextRunAt, timeZone);
  // The day set is stored in UTC. Pairing it with a clock read from the fire
  // instant is only honest while that instant also lands on one of those days
  // locally; otherwise describe the stored UTC basis and say so.
  const localClockIsValid = Boolean(
    instant && dayOfWeekMatches(dayOfWeek, instant.weekday),
  );
  const time = localClockIsValid ? instant!.time : clockTime(hour, minute);
  if (!time) return cron;
  // Without a fire instant the compact label already describes the stored UTC
  // basis; with one, it says so whenever the local day disagrees with it.
  const zone = instant && !localClockIsValid ? " (UTC)" : "";
  if (dayOfWeek === "*") return `${time} daily${zone}`;
  if (dayOfWeek === "1-5") return `${time} weekdays${zone}`;
  if (WEEKDAYS[dayOfWeek]) return `${time} ${WEEKDAYS[dayOfWeek]}${zone}`;
  return cron;
}

/**
 * Recurrence of a six-field Alfred cron on its stored UTC basis, e.g.
 * `Weekdays at 07:00 UTC`. Null when the expression is not an everyday
 * pattern, so callers show the cron itself rather than a guess. Sub-daily
 * intervals carry no day claim and are described without a zone.
 */
export function describeScheduleUtc(cron: string): string | null {
  const fields = cron.trim().split(/\s+/);
  if (fields.length !== 6) return null;
  const [seconds, minute, hour, dayOfMonth, month, dayOfWeek] = fields;
  if (seconds !== "0" && seconds !== "00") return null;
  if (dayOfMonth !== "*" || month !== "*") return null;

  if (hour === "*") {
    if (dayOfWeek !== "*") return null;
    if (minute === "0") return "Every hour";
    const interval = minute.match(/^\*\/(\d+)$/)?.[1];
    return interval ? `Every ${interval} minutes` : null;
  }

  const time = clockTime(hour, minute);
  if (!time) return null;
  if (dayOfWeek === "*") return `Every day at ${time} UTC`;
  if (dayOfWeek === "1-5") return `Weekdays at ${time} UTC`;
  if (WEEKDAYS[dayOfWeek]) return `${WEEKDAYS[dayOfWeek]} at ${time} UTC`;
  return null;
}

/** Locale date for a schedule fire time, or null when there is nothing to show. */
export function formatNextRunLabel(
  value: string | Date | null | undefined,
  timeZone?: string,
  locale?: string,
): string | null {
  if (value == null || value === "") return null;
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  const day = date.toLocaleDateString(locale, {
    weekday: "long",
    day: "numeric",
    month: "short",
    year: "numeric",
    timeZone,
  });
  const time = date.toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone,
  });
  return `${day} at ${time}`;
}

/**
 * Fire time in the viewer's zone with the zone spelled out after it, e.g.
 * `Thursday, 24 Sep 2026 at 09:00 CEST`. The zone label is short where the
 * environment provides one and the IANA name otherwise.
 */
export function formatNextRunWithZone(
  value: string | Date | null | undefined,
  timeZone?: string,
  locale?: string,
): string | null {
  const label = formatNextRunLabel(value, timeZone, locale);
  if (!label) return null;
  const date = value instanceof Date ? value : new Date(value as string);
  const parts = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    hourCycle: "h23",
    timeZone,
    timeZoneName: "short",
  }).formatToParts(date);
  const name = parts.find((part) => part.type === "timeZoneName")?.value;
  return `${label} ${name ?? localTimeZoneLabel(timeZone)}`;
}

function fieldMatches(raw: string, value: number, sundayWrap = false): boolean {
  if (raw === "*") return true;
  const step = raw.match(/^\*\/(\d+)$/);
  if (step) {
    const n = Number(step[1]);
    return n > 0 && value % n === 0;
  }
  const allowed = new Set<number>();
  for (const part of raw.split(",")) {
    const range = part.match(/^(\d+)-(\d+)$/);
    if (range) {
      const from = Number(range[1]);
      const to = Number(range[2]);
      if (from > to) return false;
      for (let i = from; i <= to; i++) {
        allowed.add(sundayWrap && i === 7 ? 0 : i);
      }
      continue;
    }
    if (!/^\d+$/.test(part)) return false;
    const n = Number(part);
    allowed.add(sundayWrap && n === 7 ? 0 : n);
  }
  return allowed.has(value);
}

/**
 * Next fire time for a simple six-field Alfred cron, in UTC like the desktop
 * scheduler. Returns null for names, aliases, or other unsupported syntax.
 */
export function previewNextRunAt(
  cron: string,
  after: Date = new Date(),
): Date | null {
  const fields = cron.trim().split(/\s+/);
  if (fields.length !== 6 || fields.some((field) => !SIMPLE_FIELD.test(field))) {
    return null;
  }

  const [seconds, minutes, hours, dayOfMonth, month, dayOfWeek] = fields;
  const cursor = new Date(after.getTime());
  cursor.setUTCMilliseconds(0);
  cursor.setUTCSeconds(cursor.getUTCSeconds() + 1);

  const exactSecond = /^\d+$/.test(seconds) ? Number(seconds) : null;
  if (exactSecond != null) {
    if (cursor.getUTCSeconds() > exactSecond) {
      cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
    }
    cursor.setUTCSeconds(exactSecond);
  }

  const limit = after.getTime() + 366 * 24 * 60 * 60 * 1000;
  for (let i = 0; i < 366 * 24 * 60 && cursor.getTime() <= limit; i++) {
    const domStar = dayOfMonth === "*";
    const dowStar = dayOfWeek === "*";
    const domOk = fieldMatches(dayOfMonth, cursor.getUTCDate());
    const dowOk = fieldMatches(dayOfWeek, cursor.getUTCDay(), true);
    const dayOk = domStar || dowStar ? domOk && dowOk : domOk || dowOk;

    if (
      fieldMatches(seconds, cursor.getUTCSeconds()) &&
      fieldMatches(minutes, cursor.getUTCMinutes()) &&
      fieldMatches(hours, cursor.getUTCHours()) &&
      fieldMatches(month, cursor.getUTCMonth() + 1) &&
      dayOk
    ) {
      return new Date(cursor.getTime());
    }

    cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
    if (exactSecond != null) cursor.setUTCSeconds(exactSecond);
    else cursor.setUTCSeconds(0);
  }

  return null;
}
