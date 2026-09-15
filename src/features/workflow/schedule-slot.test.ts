import { describe, expect, test } from "bun:test";
import {
  FREE_SCHEDULE_COUNT,
  freeScheduleWorkflowId,
  holdsFreeScheduleSlot,
  lockedExtraWorkflowIds,
} from "./schedule-slot";

const OLD = { workflowId: "wf-old", createdAt: "2026-01-01T00:00:00Z" };
const NEW = { workflowId: "wf-new", createdAt: "2026-02-01T00:00:00Z" };

describe("free schedule slot", () => {
  test("one schedule stays editable", () => {
    expect(FREE_SCHEDULE_COUNT).toBe(1);
  });

  test("no saved schedules leaves the slot open for a first creation", () => {
    expect(freeScheduleWorkflowId([])).toBeNull();
    expect(holdsFreeScheduleSlot("wf-any", [])).toBe(true);
  });

  test("a sole schedule holds the slot and a second creation does not", () => {
    expect(holdsFreeScheduleSlot("wf-old", [OLD])).toBe(true);
    expect(holdsFreeScheduleSlot("wf-new", [OLD])).toBe(false);
  });

  test("the oldest schedule keeps the slot when several exist", () => {
    expect(freeScheduleWorkflowId([NEW, OLD])).toBe("wf-old");
    expect(holdsFreeScheduleSlot("wf-old", [OLD, NEW])).toBe(true);
    expect(holdsFreeScheduleSlot("wf-new", [OLD, NEW])).toBe(false);
  });

  test("only rows beyond the free slot are forced off", () => {
    expect(lockedExtraWorkflowIds([])).toEqual([]);
    expect(lockedExtraWorkflowIds([OLD])).toEqual([]);
    expect(lockedExtraWorkflowIds([OLD, NEW])).toEqual(["wf-new"]);
    expect(lockedExtraWorkflowIds([NEW, OLD])).toEqual(["wf-new"]);
  });
});
