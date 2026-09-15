import { extractHtmlReport } from "../../html-report";
import { isMemoryPromptEligible, withMemoryDefaults } from "../../memories";
import type {
  MemoryKind,
  MemoryScopeType,
  MemoryType,
  OutputMemory,
} from "../../types";

export type MemoryQuickFilter = "all" | "pinned" | "linked" | "inactive";
export type MemoryKindFilter = "all" | MemoryKind;
export type MemoryScopeFilter = "all" | MemoryScopeType;
export type MemorySort = "recent" | "oldest" | "name" | "importance";
export type MemoryFilterCounts = Record<
  MemoryQuickFilter | MemoryScopeType,
  number
>;

export type MemoryFilterOptions = {
  scopeFilter?: MemoryScopeFilter;
  typeFilter?: "all" | MemoryType;
  now?: Date | number;
};

export type MemoryUsageStatus = {
  state: "pinned" | "available" | "scheduled" | "expired" | "inactive";
  label: string;
  description: string;
};

export function memoryUsageStatus(
  memory: OutputMemory,
  now: Date | number = Date.now(),
): MemoryUsageStatus {
  const normalized = withMemoryDefaults(memory);
  if (normalized.status !== "active") {
    return {
      state: "inactive",
      label: normalized.status === "superseded" ? "Superseded" : "Retracted",
      description:
        "This memory is kept in the library and excluded from agent context.",
    };
  }
  if (!isMemoryPromptEligible(normalized, now)) {
    const nowMs = typeof now === "number" ? now : now.getTime();
    const validFrom = Date.parse(normalized.validFrom ?? "");
    if (Number.isFinite(validFrom) && validFrom > nowMs) {
      return {
        state: "scheduled",
        label: "Not effective yet",
        description: `This memory becomes available ${new Date(validFrom).toLocaleString()}.`,
      };
    }
    const invalidExpiry = !Number.isFinite(
      Date.parse(normalized.expiresAt ?? ""),
    );
    return {
      state: "expired",
      label: invalidExpiry ? "Invalid expiry" : "Expired",
      description: invalidExpiry
        ? "Correct the expiry date before using this memory in agent context."
        : "This memory has expired and is excluded from agent context.",
    };
  }
  if (normalized.pinned) {
    return {
      state: "pinned",
      label: "Pinned",
      description:
        normalized.origin === "linked"
          ? "Pinned by its source workflow. Included in pinned context when space permits; change its pin in the source workflow."
          : "Included in pinned context on each run when space permits.",
    };
  }
  const automaticRecallEligible =
    ["preference", "fact", "decision", "constraint", "lesson"].includes(
      normalized.memoryType,
    ) ||
    (normalized.memoryType === "note" &&
      (normalized.source === "manual" || normalized.source === "review"));
  const selectedContextDescription = normalized.origin === "linked"
    ? "Select it in a Memories node to include it in agent context when space permits."
    : "Select it in a Memories node or pin it to include it in agent context when space permits.";
  return {
    state: "available",
    label: "Available",
    description: automaticRecallEligible
      ? "Select it in a Memories node, or enable automatic recall to use relevant memories."
      : selectedContextDescription,
  };
}

function queryWords(query: string): string[] {
  return query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
}

function matchesFilters(
  memory: OutputMemory,
  words: string[],
  quickFilter: MemoryQuickFilter,
  kindFilter: MemoryKindFilter,
  options: MemoryFilterOptions,
) {
  const normalized = withMemoryDefaults(memory);
  const eligible = isMemoryPromptEligible(normalized, options.now);
  if (quickFilter === "pinned" && (!normalized.pinned || !eligible))
    return false;
  if (quickFilter === "linked" && normalized.origin !== "linked") return false;
  if (quickFilter === "inactive" && eligible) return false;
  if (kindFilter !== "all" && normalized.kind !== kindFilter) return false;
  if (
    options.scopeFilter &&
    options.scopeFilter !== "all" &&
    normalized.scopeType !== options.scopeFilter
  )
    return false;
  if (
    options.typeFilter &&
    options.typeFilter !== "all" &&
    normalized.memoryType !== options.typeFilter
  )
    return false;
  if (words.length === 0) return true;
  const searchable = [
    normalized.title,
    normalized.body,
    normalized.source,
    normalized.sourceWorkflowName,
    normalized.scopeType,
    normalized.scopeLabel,
    normalized.memoryType,
    normalized.origin,
    normalized.kind === "text"
      ? "output outputs"
      : `${normalized.kind} ${normalized.kind}s`,
  ]
    .join(" ")
    .toLocaleLowerCase();
  return words.every((word) => searchable.includes(word));
}

function activityTime(memory: OutputMemory) {
  const updated = Date.parse(memory.updatedAt);
  if (Number.isFinite(updated)) return updated;
  const created = Date.parse(memory.createdAt);
  return Number.isFinite(created) ? created : 0;
}

function previewText(text: string, max: number) {
  if (extractHtmlReport(text)) return "HTML report. Open to preview";
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max)}…`;
}

export function memorySearchSnippet(
  memory: OutputMemory,
  query: string,
  max = 130,
) {
  const q = query.trim().toLocaleLowerCase();
  if (!q) return previewText(memory.body, max);
  if (extractHtmlReport(memory.body)) return "HTML report. Open to preview";

  const flat = memory.body.replace(/\s+/g, " ").trim();
  const loweredBody = flat.toLocaleLowerCase();
  const phraseIndex = loweredBody.indexOf(q);
  const bodyIndex =
    phraseIndex >= 0
      ? phraseIndex
      : (queryWords(query)
          .map((word) => loweredBody.indexOf(word))
          .find((index) => index >= 0) ?? -1);
  if (bodyIndex >= 0) {
    const start = Math.max(0, bodyIndex - 36);
    const end = Math.min(flat.length, start + max);
    return `${start > 0 ? "…" : ""}${flat.slice(start, end)}${
      end < flat.length ? "…" : ""
    }`;
  }

  const provenance = memory.sourceWorkflowName
    ? `From ${memory.sourceWorkflowName}`
    : `Source: ${memory.source}`;
  if (
    queryWords(query).some((word) =>
      provenance.toLocaleLowerCase().includes(word),
    )
  )
    return provenance;
  return previewText(memory.body, max);
}

export function filterAndSortMemories(
  memories: OutputMemory[],
  query: string,
  quickFilter: MemoryQuickFilter,
  kindFilter: MemoryKindFilter,
  sort: MemorySort = "recent",
  options: MemoryFilterOptions = {},
) {
  const words = queryWords(query);
  return memories
    .filter((memory) =>
      matchesFilters(memory, words, quickFilter, kindFilter, options),
    )
    .sort((a, b) => {
      const byName = a.title.localeCompare(b.title, undefined, {
        sensitivity: "base",
      });
      const recent = activityTime(b) - activityTime(a);
      if (sort === "name") return byName || recent || a.id.localeCompare(b.id);
      if (sort === "oldest")
        return -recent || byName || a.id.localeCompare(b.id);
      if (sort === "importance") {
        const importance = (b.salience ?? 50) - (a.salience ?? 50);
        if (importance) return importance;
      }
      return recent || byName || a.id.localeCompare(b.id);
    });
}

export function memoryFilterCounts(
  memories: OutputMemory[],
  query = "",
  kindFilter: MemoryKindFilter = "all",
  options: MemoryFilterOptions & { quickFilter?: MemoryQuickFilter } = {},
): MemoryFilterCounts {
  const words = queryWords(query);
  const countQuick = (quick: MemoryQuickFilter) =>
    memories.filter((memory) =>
      matchesFilters(memory, words, quick, kindFilter, options),
    ).length;
  const countScope = (scope: MemoryScopeType) =>
    memories.filter((memory) =>
      matchesFilters(memory, words, options.quickFilter ?? "all", kindFilter, {
        ...options,
        scopeFilter: scope,
      }),
    ).length;
  return {
    all: countQuick("all"),
    pinned: countQuick("pinned"),
    linked: countQuick("linked"),
    inactive: countQuick("inactive"),
    user: countScope("user"),
    workspace: countScope("workspace"),
    workflow: countScope("workflow"),
  };
}
