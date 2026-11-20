import { describe, expect, test } from "bun:test";
import { isSelectableTarget } from "../src/selection";

function target(nodeType: number, matched: boolean) {
  const closest = (selector: string) => {
    expect(selector).toContain(".output-modal-body");
    expect(selector).toContain(".user-select-text");
    return matched ? ({} as Element) : null;
  };
  return {
    nodeType,
    parentElement: { closest },
    closest,
  } as unknown as EventTarget;
}

describe("text selection targets", () => {
  test("resolves selectstart text nodes to their parent element", () => {
    expect(isSelectableTarget(target(3, true))).toBe(true);
  });

  test("rejects text outside selectable surfaces", () => {
    expect(isSelectableTarget(target(3, false))).toBe(false);
  });

  test("accepts element targets directly", () => {
    expect(isSelectableTarget(target(1, true))).toBe(true);
  });

  test("ignores document-like targets", () => {
    expect(
      isSelectableTarget({
        nodeType: 9,
        parentElement: null,
      } as unknown as EventTarget),
    ).toBe(false);
    expect(isSelectableTarget(null)).toBe(false);
  });
});
