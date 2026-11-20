import { expect, test } from "bun:test";
import {
  defaultEdgeCondition,
  defaultSystemCondition,
  edgeCondition,
  edgeConditionSummary,
  edgeConditionValidationError,
  formatWeekdaySelection,
  isUnaryEdgeConditionOperator,
  operatorNeedsSecondValue,
  parseEdgeCondition,
  parseWeekdaySelection,
  MAX_EDGE_CONDITION_VALUE_BYTES,
  type WorkflowEdge,
} from "../../types";

function edge(condition?: unknown): WorkflowEdge {
  return {
    id: "e1",
    source: "a",
    target: "b",
    data: condition === undefined ? undefined : { condition: condition as never },
  };
}

test("an edge with no data is unconditional", () => {
  expect(edgeCondition(edge())).toBeNull();
  expect(edgeCondition(edge(null))).toBeNull();
});

test("an unknown source or operator reads as unconditional, never throws", () => {
  expect(parseEdgeCondition({ source: "nope", operator: "contains" })).toBeNull();
  expect(parseEdgeCondition({ source: "output", operator: "nope" })).toBeNull();
  expect(parseEdgeCondition("not an object")).toBeNull();
  expect(parseEdgeCondition(undefined)).toBeNull();
});

test("a valid condition round-trips with case sensitivity defaulted off", () => {
  const parsed = parseEdgeCondition({
    source: "trigger",
    operator: "matches",
    value: "FAIL|error",
  });
  expect(parsed).toEqual({
    source: "trigger",
    operator: "matches",
    value: "FAIL|error",
    caseSensitive: false,
  });
});

test("an oversized value is capped at the shared limit", () => {
  const parsed = parseEdgeCondition({
    source: "output",
    operator: "contains",
    value: "x".repeat(MAX_EDGE_CONDITION_VALUE_BYTES * 2),
  });
  expect(parsed?.value.length).toBe(MAX_EDGE_CONDITION_VALUE_BYTES);
});

test("unary operators are identified so the value field can hide", () => {
  expect(isUnaryEdgeConditionOperator("isEmpty")).toBe(true);
  expect(isUnaryEdgeConditionOperator("isNotEmpty")).toBe(true);
  expect(isUnaryEdgeConditionOperator("contains")).toBe(false);
});

test("the chip summary omits the value only for unary operators", () => {
  expect(
    edgeConditionSummary({
      source: "output",
      operator: "contains",
      value: "FAIL",
    }),
  ).toBe('step output contains "FAIL"');
  expect(
    edgeConditionSummary({
      source: "context",
      operator: "isEmpty",
      value: "ignored",
    }),
  ).toBe("run context is empty");
});

test("the default condition is a usable starting point", () => {
  const fresh = defaultEdgeCondition();
  expect(fresh.source).toBe("output");
  expect(fresh.operator).toBe("contains");
  expect(fresh.value).toBe("");
});

test("a machine condition round-trips its field, key, and bounds", () => {
  const parsed = parseEdgeCondition({
    source: "system",
    field: "time",
    operator: "between",
    value: "09:00",
    value2: "17:00",
  });
  expect(parsed).toEqual({
    source: "system",
    field: "time",
    operator: "between",
    value: "09:00",
    value2: "17:00",
    caseSensitive: false,
  });

  const path = parseEdgeCondition({
    source: "system",
    field: "env",
    key: "CI",
    operator: "isOneOf",
    value: "true, 1",
    caseSensitive: true,
  });
  expect(path?.key).toBe("CI");
  expect(path?.caseSensitive).toBe(true);
});

test("a machine condition rejects fields, operators, and keys that do not fit", () => {
  expect(
    parseEdgeCondition({
      source: "system",
      field: "weekday",
      operator: "before",
      value: "2026-11-20",
    }),
  ).toBeNull();
  expect(
    parseEdgeCondition({
      source: "system",
      field: "env",
      operator: "equals",
      value: "true",
    }),
  ).toBeNull();
  expect(
    parseEdgeCondition({
      source: "system",
      field: "path",
      key: "report.md",
      operator: "between",
      value: "a",
      value2: "b",
    }),
  ).toBeNull();
  expect(
    parseEdgeCondition({
      source: "output",
      field: "weekday",
      operator: "contains",
      value: "x",
    }),
  ).toBeNull();
  expect(
    parseEdgeCondition({
      source: "output",
      operator: "isOneOf",
      value: "x",
    }),
  ).toBeNull();
});

test("weekday selections stay in Monday-first order", () => {
  expect(parseWeekdaySelection("Sunday, Monday, Sunday")).toEqual([
    "Monday",
    "Sunday",
  ]);
  expect(formatWeekdaySelection(["Sunday", "Monday"])).toBe(
    "Monday, Sunday",
  );
});

test("machine summaries name the field and both bounds", () => {
  expect(
    edgeConditionSummary({
      source: "system",
      field: "weekday",
      operator: "isOneOf",
      value: "Saturday, Sunday",
    }),
  ).toBe("day of week is one of Sat, Sun");
  expect(
    edgeConditionSummary({
      source: "system",
      field: "time",
      operator: "between",
      value: "09:00",
      value2: "17:00",
    }),
  ).toBe('time of day is between "09:00" and "17:00"');
  expect(
    edgeConditionSummary({
      source: "system",
      field: "path",
      key: "report.md",
      operator: "exists",
      value: "",
    }),
  ).toBe('file "report.md" exists');
  expect(
    edgeConditionSummary({
      source: "system",
      field: "battery",
      operator: "atLeast",
      value: "20",
    }),
  ).toBe("battery level is at least 20%");
});

test("draft validation covers typed inputs the runner would reject", () => {
  expect(
    edgeConditionValidationError({
      source: "system",
      field: "weekday",
      operator: "isOneOf",
      value: "",
    }),
  ).toBe("Choose at least one day.");
  expect(
    edgeConditionValidationError({
      source: "system",
      field: "time",
      operator: "between",
      value: "9:00",
      value2: "17:00",
    }),
  ).toBe("Enter a time.");
  expect(
    edgeConditionValidationError({
      source: "system",
      field: "battery",
      operator: "atLeast",
      value: "120",
    }),
  ).toBe("Enter a level from 0 to 100.");
  expect(
    edgeConditionValidationError({
      source: "system",
      field: "battery",
      operator: "between",
      value: "80",
      value2: "20",
    }),
  ).toBe("The lower bound must not be above the upper bound.");
  expect(
    edgeConditionValidationError({
      source: "system",
      field: "path",
      key: "",
      operator: "exists",
      value: "",
    }),
  ).toBe("Enter a path.");

  const path = defaultSystemCondition("path");
  expect(path.source).toBe("system");
  expect(path.field).toBe("path");
  expect(path.key).toBe("");
  expect(path.operator).toBe("exists");
  expect(
    edgeConditionValidationError({
      ...path,
      key: "report.md",
    }),
  ).toBeNull();
});

test("typed operators expose their value and range needs", () => {
  expect(operatorNeedsSecondValue("between")).toBe(true);
  expect(operatorNeedsSecondValue("atLeast")).toBe(false);
  expect(isUnaryEdgeConditionOperator("exists")).toBe(true);
  expect(isUnaryEdgeConditionOperator("modifiedBefore")).toBe(false);
});
