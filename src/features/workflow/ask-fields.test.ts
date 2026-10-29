import { describe, expect, test } from "bun:test";
import { askFields, askNames } from "./ask-fields";
import type { WorkflowNode } from "./types";

const node = (type: string, data: Record<string, unknown>) =>
  ({ id: type, type, position: { x: 0, y: 0 }, data }) as WorkflowNode;

describe("askFields", () => {
  test("collects Input and Template placeholders with defaults", () => {
    expect(
      askFields([
        node("input", { prompt: "Write about {{ask:Topic}} in a {{ ask: Tone | calm }} voice" }),
        node("template", { template: "{{ask:Topic|ignored}} {{ask:Audience}} {{output}}" }),
        node("script", { command: "{{ask:Hidden}}" }),
      ]),
    ).toEqual([
      { name: "Topic", defaultValue: "" },
      { name: "Tone", defaultValue: "calm" },
      { name: "Audience", defaultValue: "" },
    ]);
  });

  test("returns nothing for plain workflows", () => {
    expect(askFields([node("input", { prompt: "Hello {{context}}" })])).toEqual([]);
  });
});

test("askNames lists each placeholder once", () => {
  expect(askNames("{{ask:Topic}} {{ ask: Tone | calm }} {{ask:Topic|x}} {{ask Topic}}")).toEqual(["Topic", "Tone"]);
  expect(askNames("Hello")).toEqual([]);
});
