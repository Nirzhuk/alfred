import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { createElement } from "react";
import { IdentitySettings } from "../src/features/workflow/components/node-settings-modal/identity-settings";
import { normalizeNodes } from "../src/features/workflow/store";
import {
  defaultIdentityNodeData,
  isIdentityNodeData,
  isPromptNodeData,
  normalizeIdentityNodeData,
  titleForNodeType,
  type WorkflowNode,
} from "../src/features/workflow/types";

describe("Identity persistence", () => {
  test("retains authored fields and placement across a graph round trip", () => {
    const original: WorkflowNode = {
      id: "identity-editor",
      type: "identity",
      position: { x: 120, y: 240 },
      data: {
        ...defaultIdentityNodeData("Editor"),
        role: "Senior editor",
        personality: "Direct and warm.",
        instructions: "Check sources.\nKeep our voice.",
      },
    };
    const [restored] = normalizeNodes(JSON.parse(JSON.stringify([original])));
    expect(restored).toEqual(original);
    expect(isIdentityNodeData(restored.data)).toBe(true);
    expect(isPromptNodeData(restored.data)).toBe(false);
    expect(titleForNodeType(restored.type)).toBe("Identity");
  });

  test("fills missing or malformed fields in imported nodes", () => {
    const [restored] = normalizeNodes([
      {
        id: "identity-imported",
        type: "identity",
        position: { x: 0, y: 0 },
        data: { label: " ", role: 42, personality: null, instructions: ["bad"] },
      } as unknown as WorkflowNode,
    ]);
    expect(restored.data).toEqual(defaultIdentityNodeData());
    expect(normalizeIdentityNodeData({ instructions: "Keep this.\nExactly." }))
      .toEqual({ ...defaultIdentityNodeData(), instructions: "Keep this.\nExactly." });
  });

  test("recognizes the identity kind when a graph omits its renderer type", () => {
    const [restored] = normalizeNodes([
      {
        id: "identity-kind",
        position: { x: 0, y: 0 },
        data: defaultIdentityNodeData(),
      },
    ]);
    expect(restored.type).toBe("identity");
  });
});

test("Identity settings display saved instructions as editable plain text", () => {
  const html = renderToStaticMarkup(createElement(IdentitySettings, {
    data: {
      ...defaultIdentityNodeData("Research & writing"),
      role: "Editor",
      personality: "Direct and warm.",
      instructions: "Keep <examples> literal.\nCheck facts.",
    },
    onUpdate: () => {},
  }));
  expect(html).toContain("Role</span><input");
  expect(html).toContain("Personality</span><textarea");
  expect(html).toContain("Standing instructions</span><textarea");
  expect(html).toContain("Keep &lt;examples&gt; literal.\nCheck facts.");
  expect(html).toContain("connected downstream agent");
});
