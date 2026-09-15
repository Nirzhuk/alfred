import { describe, expect, test } from "bun:test";
import {
  filterAndSortMemories,
  memorySearchSnippet,
  memoryFilterCounts,
  memoryUsageStatus,
} from "../src/features/workflow/components/memories-inspector/memory-list-model";
import type { OutputMemory } from "../src/features/workflow/types";

function memory(
  id: string,
  overrides: Partial<OutputMemory> = {},
): OutputMemory {
  return {
    id,
    workflowId: "workflow-1",
    kind: "note",
    source: "manual",
    title: `Memory ${id}`,
    body: `Content for ${id}`,
    pinned: false,
    createdAt: "2026-08-01T10:00:00.000Z",
    updatedAt: "2026-08-01T10:00:00.000Z",
    origin: "owned",
    ...overrides,
  };
}

describe("Memories inspector list model", () => {
  test("sorts all results by recent activity", () => {
    const results = filterAndSortMemories(
      [
        memory("older"),
        memory("newer", { updatedAt: "2026-08-10T10:00:00.000Z" }),
      ],
      "",
      "all",
      "all",
    );

    expect(results.map((item) => item.id)).toEqual(["newer", "older"]);
  });

  test("combines pinned and kind filters and includes owner-pinned linked memories", () => {
    const results = filterAndSortMemories(
      [
        memory("note", { pinned: true }),
        memory("output", { pinned: true, kind: "text" }),
        memory("linked", {
          pinned: true,
          origin: "linked",
          kind: "note",
        }),
      ],
      "",
      "pinned",
      "note",
    );

    expect(results.map((item) => item.id)).toEqual(["linked", "note"]);
  });

  test("searches provenance and returns a matching snippet", () => {
    const linked = memory("linked", {
      origin: "linked",
      sourceWorkflowName: "Research workflow",
      body: "A reusable finding",
    });

    expect(
      filterAndSortMemories([linked], "research", "all", "all"),
    ).toHaveLength(1);
    expect(memorySearchSnippet(linked, "research")).toBe(
      "From Research workflow",
    );
  });

  test("centers content snippets around the matching text", () => {
    const item = memory("long", {
      body: `${"Earlier context ".repeat(8)}important phrase${" later".repeat(
        12,
      )}`,
    });

    const snippet = memorySearchSnippet(item, "important", 90);
    expect(snippet).toContain("important phrase");
    expect(snippet.startsWith("…")).toBe(true);
    expect(snippet.endsWith("…")).toBe(true);
  });

  test("matches search words across content, type, scope and source workflow", () => {
    const item = memory("release", {
      title: "Release facts",
      body: "Cite the primary source.",
      memoryType: "constraint",
      scopeType: "workspace",
      sourceWorkflowName: "Editorial workflow",
    });
    expect(
      filterAndSortMemories(
        [item],
        "editorial workspace primary constraint",
        "all",
        "all",
      ),
    ).toEqual([item]);
    expect(
      filterAndSortMemories([item], "editorial missing", "all", "all"),
    ).toEqual([]);
    expect(memorySearchSnippet(item, "editorial primary")).toContain(
      "primary source",
    );
  });

  test("provides deterministic oldest, name and importance sorts without mutating input", () => {
    const items = [
      memory("z", {
        title: "Zulu",
        salience: 90,
        updatedAt: "2026-08-10T10:00:00Z",
      }),
      memory("a", { title: "Alpha", salience: 70 }),
      memory("b", {
        title: "Bravo",
        salience: 90,
        updatedAt: "2026-08-05T10:00:00Z",
      }),
    ];
    const ids = (sort: "oldest" | "name" | "importance") =>
      filterAndSortMemories(items, "", "all", "all", sort).map(({ id }) => id);
    expect(ids("oldest")).toEqual(["a", "b", "z"]);
    expect(ids("name")).toEqual(["a", "b", "z"]);
    expect(ids("importance")).toEqual(["z", "b", "a"]);
    expect(items.map(({ id }) => id)).toEqual(["z", "a", "b"]);
  });

  test("pinned eligibility excludes expired and inactive memories, including invalid expiry", () => {
    const now = Date.parse("2026-09-08T10:00:00Z");
    const items = [
      memory("active", { pinned: true }),
      memory("expired", { pinned: true, expiresAt: "2026-09-08T10:00:00Z" }),
      memory("retracted", { pinned: true, status: "retracted" }),
      memory("invalid", { pinned: true, expiresAt: "invalid" }),
    ];
    expect(
      filterAndSortMemories(items, "", "pinned", "all", "recent", { now }).map(
        ({ id }) => id,
      ),
    ).toEqual(["active"]);
    expect(
      filterAndSortMemories(items, "", "inactive", "all", "recent", { now }),
    ).toHaveLength(3);
    expect(memoryUsageStatus(items[1], now).label).toBe("Expired");
    expect(memoryUsageStatus(items[2], now).state).toBe("inactive");
    expect(memoryUsageStatus(items[3], now).label).toBe("Invalid expiry");
    expect(
      memoryUsageStatus(
        memory("linked", { pinned: true, origin: "linked" }),
        now,
      ).description,
    ).toContain("source workflow");
  });

  test("navigation counts match the same search, kind, type and scope filters as results", () => {
    const now = Date.parse("2026-09-08T10:00:00Z");
    const items = [
      memory("a", {
        title: "Release",
        pinned: true,
        memoryType: "fact",
        scopeType: "user",
      }),
      memory("b", {
        title: "Release",
        pinned: true,
        memoryType: "fact",
        scopeType: "workspace",
      }),
      memory("c", {
        title: "Release",
        pinned: true,
        memoryType: "fact",
        scopeType: "workflow",
        origin: "linked",
      }),
      memory("d", {
        title: "Release",
        pinned: true,
        memoryType: "fact",
        scopeType: "workspace",
        status: "superseded",
      }),
      memory("e", {
        title: "Other",
        pinned: true,
        memoryType: "fact",
        scopeType: "user",
      }),
    ];
    const options = {
      now,
      scopeFilter: "workspace",
      typeFilter: "fact",
      quickFilter: "pinned",
    } as const;
    const counts = memoryFilterCounts(items, "release", "note", options);
    expect(counts).toEqual({
      all: 2,
      pinned: 1,
      linked: 0,
      inactive: 1,
      user: 1,
      workspace: 1,
      workflow: 1,
    });
    for (const quick of ["all", "pinned", "linked", "inactive"] as const) {
      expect(counts[quick]).toBe(
        filterAndSortMemories(
          items,
          "release",
          quick,
          "note",
          "recent",
          options,
        ).length,
      );
    }
    for (const scope of ["user", "workspace", "workflow"] as const) {
      expect(counts[scope]).toBe(
        filterAndSortMemories(items, "release", "pinned", "note", "recent", {
          ...options,
          scopeFilter: scope,
        }).length,
      );
    }
  });
});
