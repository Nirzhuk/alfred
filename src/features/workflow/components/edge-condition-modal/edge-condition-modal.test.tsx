import { expect, test } from "bun:test";
import {
  defaultEdgeCondition,
  edgeCondition,
  edgeConditionSummary,
  isUnaryEdgeConditionOperator,
  parseEdgeCondition,
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
