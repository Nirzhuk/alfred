import { describe, expect, test } from "bun:test";
import {
  ADD_STEP_ITEMS,
  addStepNodeType,
  filterAddStepGroups,
} from "./add-step-items";
import { AGENT_PROVIDER_IDS } from "./types";

describe("step picker catalog", () => {
  test("keeps every provider available with a distinct step id", () => {
    expect(new Set(ADD_STEP_ITEMS.map((item) => item.id)).size).toBe(
      ADD_STEP_ITEMS.length,
    );
    expect(
      ADD_STEP_ITEMS.flatMap((item) => item.kind === "agent" ? [item.provider] : []).sort(),
    ).toEqual([...AGENT_PROVIDER_IDS].sort());
  });

  test("finds Identity by its purpose and inserts identity data", () => {
    for (const query of ["personality", "persona", "standing instructions"]) {
      const items = filterAddStepGroups(query).flatMap((group) => group.items);
      expect(items.map((item) => item.id)).toEqual(["identity"]);
      expect(addStepNodeType(items[0])).toBe("identity");
      expect(items[0].kind === "step" && items[0].data).toMatchObject({
        kind: "identity",
        role: "",
        personality: "",
        instructions: "",
      });
    }
  });

  test("matches every search word regardless of case or extra spaces", () => {
    expect(
      filterAddStepGroups("  PULL   request ").flatMap((group) => group.items.map((item) => item.id)),
    ).toEqual(["createPr"]);
    expect(filterAddStepGroups("pull notification")).toEqual([]);
  });

  test("searches only the selected category and hides empty sections", () => {
    expect(filterAddStepGroups("github", "agents").map((group) => ({
      id: group.id,
      items: group.items.map((item) => item.id),
    }))).toEqual([{ id: "agents", items: ["github_copilot"] }]);
    expect(filterAddStepGroups("personality", "outputs")).toEqual([]);
    expect(filterAddStepGroups("connected apps", "tools")[0]?.items[0]?.id).toBe("appAction");
  });

  test("restores the complete catalog when search is cleared", () => {
    expect(filterAddStepGroups("   ").flatMap((group) => group.items)).toEqual(ADD_STEP_ITEMS);
  });

  test("preserves separate pull-request and issue actions", () => {
    const items = filterAddStepGroups("github", "outputs").flatMap((group) => group.items);
    expect(items.map((item) => item.id)).toEqual(["createPr", "openIssue"]);
    expect(items.filter((item) => item.kind === "step").map((item) => item.data)).toEqual([
      expect.objectContaining({ kind: "gitHost", action: "pr" }),
      expect.objectContaining({ kind: "gitHost", action: "issue" }),
    ]);
  });
});
