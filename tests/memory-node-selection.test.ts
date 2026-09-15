import { describe, expect, test } from "bun:test";
import {
  addMemorySelection,
  memoryPickerItems,
} from "../src/features/workflow/components/node-settings-modal/memory-node-model";
import { withMemoryDefaults } from "../src/features/workflow/memories";
import type { OutputMemory } from "../src/features/workflow/types";

const memory = (id: string, patch: Partial<OutputMemory> = {}) =>
  withMemoryDefaults({
    id,
    title: id,
    body: id,
    createdAt: "2026-09-01T00:00:00Z",
    updatedAt: "2026-09-01T00:00:00Z",
    ...patch,
  });

describe("Memory node selection", () => {
  test("keeps expired, inactive and missing selections removable and deduplicates fetched rows", () => {
    const linked = memory("linked", { origin: "linked" });
    const items = memoryPickerItems(
      [
        linked,
        memory("expired", { expiresAt: "2026-08-01T00:00:00Z" }),
        memory("inactive", { status: "retracted" }),
        memory("available"),
      ],
      [memory("linked", { origin: "linkable" })],
      ["linked", "expired", "inactive", "missing", "linked"],
    );
    expect(items.selected.map(({ id }) => id)).toEqual([
      "linked",
      "expired",
      "inactive",
      "missing",
    ]);
    expect(items.selected[0].memory).toBe(linked);
    expect(items.selected[3].memory).toBeNull();
    expect(items.available.map(({ id }) => id)).toEqual(["available"]);
  });

  test("failed linking never adds a reference to the node", async () => {
    const result = await addMemorySelection("other-workflow", {
      requiresLink: true,
      linkMemory: async () => null,
      currentSelection: () => ["existing"],
      isCurrent: () => true,
    });
    expect(result).toBeNull();
  });

  test("a completed link preserves selections changed while it was pending", async () => {
    let finish!: (memory: OutputMemory) => void;
    let selected = ["original"];
    const pending = addMemorySelection("linked", {
      requiresLink: true,
      linkMemory: () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
      currentSelection: () => selected,
      isCurrent: () => true,
    });
    selected = ["newer-choice"];
    finish(memory("linked"));
    expect(await pending).toEqual(["newer-choice", "linked"]);
  });

  test("closing or switching the picker during a link prevents a stale node update", async () => {
    const result = await addMemorySelection("linked", {
      requiresLink: true,
      linkMemory: async () => memory("linked"),
      currentSelection: () => ["existing"],
      isCurrent: () => false,
    });
    expect(result).toBeNull();
  });

  test("existing library memories do not create links or duplicate IDs", async () => {
    const result = await addMemorySelection("existing", {
      requiresLink: false,
      linkMemory: async () => {
        throw new Error("should not link");
      },
      currentSelection: () => ["existing"],
      isCurrent: () => true,
    });
    expect(result).toEqual(["existing"]);
  });
});
