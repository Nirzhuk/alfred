import { describe, expect, test } from "bun:test";
import { askFields, askNames, missingRequired } from "./ask-fields";
import type { WorkflowNode } from "./types";

const node = (
  id: string,
  type: string,
  data: Record<string, unknown>,
) => ({ id, type, position: { x: 0, y: 0 }, data }) as WorkflowNode;

describe("askFields", () => {
  test("collects Input and Template placeholders with defaults", () => {
    expect(
      askFields([
        node("input", "input", {
          label: "Brief",
          prompt: "Write about {{ask:Topic}} in a {{ ask: Tone | calm }} voice",
        }),
        node("template", "template", {
          label: "Post",
          template: "{{ask:Topic|ignored}} {{ask:Audience}} {{output}}",
        }),
        node("script", "script", { command: "{{ask:Hidden}}" }),
      ]),
    ).toEqual([
      {
        name: "Topic",
        defaultValue: "",
        required: true,
        sources: [
          { nodeId: "input", label: "Brief", type: "input" },
          { nodeId: "template", label: "Post", type: "template" },
        ],
      },
      {
        name: "Tone",
        defaultValue: "calm",
        required: false,
        sources: [{ nodeId: "input", label: "Brief", type: "input" }],
      },
      {
        name: "Audience",
        defaultValue: "",
        required: true,
        sources: [{ nodeId: "template", label: "Post", type: "template" }],
      },
    ]);
  });

  test("a required occurrence wins over a default elsewhere", () => {
    const [tone] = askFields([
      node("first", "input", { prompt: "{{ask:Tone|calm}}" }),
      node("second", "template", { template: "{{ask:Tone}}" }),
    ]);
    expect(tone.required).toBe(true);
    expect(tone.defaultValue).toBe("");
  });

  test("an explicit empty default makes a placeholder optional", () => {
    const [note] = askFields([
      node("input", "input", { prompt: "{{ask:Note|}}" }),
    ]);
    expect(note).toEqual({
      name: "Note",
      defaultValue: "",
      required: false,
      sources: [{ nodeId: "input", label: "Input", type: "input" }],
    });
  });

  test("returns nothing for plain workflows", () => {
    expect(askFields([node("input", "input", { prompt: "Hello {{context}}" })])).toEqual([]);
  });
});

test("missingRequired mirrors runner/ask.rs", () => {
  const fields = askFields([
    node("input", "input", {
      prompt: "{{ask:Topic}} {{ask:Tone|calm}} {{ask:Note|}}",
    }),
  ]);
  expect(missingRequired(fields, {})).toEqual(["Topic"]);
  expect(missingRequired(fields, { Topic: "   " })).toEqual(["Topic"]);
  expect(missingRequired(fields, { Topic: "Rust" })).toEqual([]);
});

test("askNames lists each placeholder once", () => {
  expect(askNames("{{ask:Topic}} {{ ask: Tone | calm }} {{ask:Topic|x}} {{ask Topic}}")).toEqual(["Topic", "Tone"]);
  expect(askNames("Hello")).toEqual([]);
});
