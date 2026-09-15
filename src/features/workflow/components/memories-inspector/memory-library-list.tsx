import { SearchInput } from "../../../../components/search-input";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { Icon, type IconProps } from "../../../../components/icon";
import { SelectControl } from "../../../../components/select-control";
import type { MemoryKind, MemoryType, OutputMemory } from "../../types";
import {
  memorySearchSnippet,
  memoryUsageStatus,
  type MemoryKindFilter,
  type MemorySort,
} from "./memory-list-model";
import "./memory-library.css";

export type MemoryLibraryListProps = {
  memories: OutputMemory[];
  totalCount: number;
  selectedId: string | null;
  onSelect: (memory: OutputMemory) => void;
  query: string;
  onQueryChange: (query: string) => void;
  kindFilter: MemoryKindFilter;
  onKindFilterChange: (kind: MemoryKindFilter) => void;
  typeFilter: MemoryType | "all";
  onTypeFilterChange: (type: MemoryType | "all") => void;
  sort: MemorySort;
  onSortChange: (sort: MemorySort) => void;
  onResetFilters: () => void;
  onNewNote: () => void;
  title?: string;
  hasActiveFilters?: boolean;
  searchRef?: RefObject<HTMLInputElement | null>;
  onKeyDown?: (event: KeyboardEvent<HTMLElement>) => void;
};

const KIND_ICONS: Record<MemoryKind, IconProps["name"]> = {
  note: "note",
  text: "chat-text",
  artifact: "file",
};

const TYPE_LABELS: Record<MemoryType, string> = {
  preference: "Preference",
  fact: "Fact",
  decision: "Decision",
  constraint: "Constraint",
  lesson: "Lesson",
  episode: "Episode",
  checkpoint: "Checkpoint",
  note: "Note",
  output: "Output",
  artifact: "Artifact",
};

const SCOPE_LABELS = {
  workflow: "Workflow",
  workspace: "Workspace",
  user: "Personal",
};

function highlightMatch(text: string, query: string): ReactNode {
  const words = [...new Set(query.trim().split(/\s+/).filter(Boolean))];
  if (!words.length) return text;
  const pattern = words
    .sort((a, b) => b.length - a.length)
    .map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join("|");
  return text
    .split(new RegExp(`(${pattern})`, "gi"))
    .map((part, index) =>
      index % 2 === 1 ? <mark key={index}>{part}</mark> : part,
    );
}

function relativeUpdatedAt(value: string) {
  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) return "Unknown date";
  const seconds = Math.round((time - Date.now()) / 1000);
  if (Math.abs(seconds) < 60) return "Just now";
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ["year", 31_536_000],
    ["month", 2_592_000],
    ["week", 604_800],
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  const [unit, duration] = units.find(
    ([, duration]) => Math.abs(seconds) >= duration,
  )!;
  return new Intl.RelativeTimeFormat(undefined, {
    numeric: "auto",
    style: "short",
  }).format(Math.trunc(seconds / duration), unit);
}

export function MemoryLibraryList({
  memories,
  totalCount,
  selectedId,
  onSelect,
  query,
  onQueryChange,
  kindFilter,
  onKindFilterChange,
  typeFilter,
  onTypeFilterChange,
  sort,
  onSortChange,
  onResetFilters,
  onNewNote,
  title = "All memories",
  hasActiveFilters,
  searchRef,
  onKeyDown,
}: MemoryLibraryListProps) {
  const [filtersOpen, setFiltersOpen] = useState(false);
  const ownSearchRef = useRef<HTMLInputElement>(null);
  const resolvedSearchRef = searchRef ?? ownSearchRef;
  const resultsRef = useRef<HTMLDivElement>(null);
  const id = useId();
  const advancedFilterCount =
    Number(kindFilter !== "all") + Number(typeFilter !== "all");
  const canReset =
    hasActiveFilters ?? (Boolean(query.trim()) || advancedFilterCount > 0);
  const selectedIsVisible = memories.some((memory) => memory.id === selectedId);

  useEffect(() => {
    const selected = resultsRef.current?.querySelector<HTMLButtonElement>(
      '[aria-selected="true"]',
    );
    if (!selected) return;
    if (resultsRef.current?.contains(document.activeElement)) {
      selected.focus({ preventScroll: true });
    }
    selected.scrollIntoView({ block: "nearest" });
  }, [selectedId, memories]);

  const handleResultKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (onKeyDown) {
      onKeyDown(event);
      return;
    }
    if (
      !memories.length ||
      (event.key !== "ArrowDown" && event.key !== "ArrowUp")
    )
      return;
    event.preventDefault();
    const current = memories.findIndex((memory) => memory.id === selectedId);
    const direction = event.key === "ArrowDown" ? 1 : -1;
    const next =
      current < 0
        ? 0
        : (current + direction + memories.length) % memories.length;
    onSelect(memories[next]);
  };

  return (
    <section className="memory-library-list" aria-label={title}>
      <div className="memory-library-list-header">
        <div className="memory-library-list-heading">
          <h3>{title}</h3>
          <span role="status">{memories.length}</span>
        </div>
        <SearchInput
          shortcut="/"
          clearLabel="Clear memory search"
          onClear={() => onQueryChange("")}
          ref={resolvedSearchRef}
          type="search"
          aria-label="Search memories"
          placeholder="Search memories..."
          value={query}
          autoComplete="off"
          onChange={(event) => onQueryChange(event.target.value)}
          onKeyDown={(event) => {
            if (
              event.key === "ArrowDown" &&
              memories.length &&
              !event.nativeEvent.isComposing
            ) {
              event.preventDefault();
              resultsRef.current
                ?.querySelector<HTMLButtonElement>('[tabindex="0"]')
                ?.focus();
            }
          }}
        />
        <div className="memory-library-list-toolbar">
          <button
            type="button"
            className="memory-library-filter-toggle"
            aria-expanded={filtersOpen}
            aria-controls={`${id}-filters`}
            onClick={() => setFiltersOpen((current) => !current)}
          >
            <Icon name="sliders" size={16} />
            Filters
            {advancedFilterCount > 0 ? (
              <span>{advancedFilterCount}</span>
            ) : null}
            <Icon name="caret-down" size={14} />
          </button>
          <SelectControl
            density="compact"
            containerClassName="memory-library-sort"
            aria-label="Sort memories"
            value={sort}
            onChange={(event) => onSortChange(event.target.value as MemorySort)}
          >
            <option value="recent">Recent</option>
            <option value="oldest">Oldest</option>
            <option value="name">Name</option>
            <option value="importance">Importance</option>
          </SelectControl>
        </div>
        {filtersOpen ? (
          <div
            id={`${id}-filters`}
            className="memory-library-filter-fields"
            role="group"
            aria-label="Memory filters"
          >
            <label>
              <span>Kind</span>
              <SelectControl
                density="compact"
                value={kindFilter}
                onChange={(event) =>
                  onKindFilterChange(event.target.value as MemoryKindFilter)
                }
              >
                <option value="all">All kinds</option>
                <option value="note">Notes</option>
                <option value="text">Outputs</option>
                <option value="artifact">Artifacts</option>
              </SelectControl>
            </label>
            <label>
              <span>Type</span>
              <SelectControl
                density="compact"
                value={typeFilter}
                onChange={(event) =>
                  onTypeFilterChange(event.target.value as MemoryType | "all")
                }
              >
                <option value="all">All types</option>
                {(
                  Object.entries(TYPE_LABELS) as Array<[MemoryType, string]>
                ).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </SelectControl>
            </label>
          </div>
        ) : null}
        {canReset ? (
          <button
            type="button"
            className="memory-library-reset"
            onClick={onResetFilters}
          >
            <Icon name="x" size={14} />
            Reset filters
          </button>
        ) : null}
      </div>

      <div
        className="memory-library-results"
        ref={resultsRef}
        onKeyDown={handleResultKeyDown}
      >
        {memories.length ? (
          <ul role="listbox" aria-label="Memories">
            {memories.map((memory, index) => {
              const updatedAt = memory.updatedAt || memory.createdAt;
              const usage = memoryUsageStatus(memory);
              const scope = SCOPE_LABELS[memory.scopeType] ?? "Workflow";
              return (
                <li key={memory.id} role="presentation">
                  <button
                    type="button"
                    className="memory-library-row"
                    data-memory-id={memory.id}
                    role="option"
                    aria-selected={memory.id === selectedId}
                    tabIndex={
                      memory.id === selectedId ||
                      (!selectedIsVisible && index === 0)
                        ? 0
                        : -1
                    }
                    onClick={() => onSelect(memory)}
                  >
                    <span className="memory-library-row-icon" aria-hidden>
                      <Icon
                        name={KIND_ICONS[memory.kind] ?? "note"}
                        size={18}
                      />
                    </span>
                    <span className="memory-library-row-content">
                      <span className="memory-library-row-title">
                        <span>
                          {highlightMatch(
                            memory.title || "Untitled memory",
                            query,
                          )}
                        </span>
                        {usage.state === "pinned" ? (
                          <span
                            className="memory-library-pin"
                            title={usage.description}
                          >
                            <Icon name="push-pin" size={14} label="Pinned" />
                          </span>
                        ) : null}
                      </span>
                      <span className="memory-library-row-preview">
                        {highlightMatch(
                          memorySearchSnippet(memory, query, 180) ||
                            "No content",
                          query,
                        )}
                      </span>
                      <span className="memory-library-row-meta">
                        <span title={memory.scopeLabel || scope}>{scope}</span>
                        <span>{TYPE_LABELS[memory.memoryType] ?? "Note"}</span>
                        {memory.origin === "linked" ? (
                          <span
                            title={`From ${memory.sourceWorkflowName || "another workflow"}`}
                          >
                            <Icon name="link" size={12} label="Linked memory" />
                          </span>
                        ) : null}
                        {usage.state === "expired" ||
                        usage.state === "inactive" ? (
                          <span
                            className="memory-library-usage"
                            title={usage.description}
                          >
                            {usage.label}
                          </span>
                        ) : null}
                        <time
                          dateTime={updatedAt}
                          title={new Date(updatedAt).toLocaleString()}
                        >
                          {relativeUpdatedAt(updatedAt)}
                        </time>
                      </span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <div className="memory-library-empty">
            <Icon name={totalCount ? "magnifying-glass" : "note"} size={24} />
            <h3>{totalCount ? "No matching memories" : "No memories yet"}</h3>
            <p>
              {totalCount
                ? "Try another search or reset your filters."
                : "Create a note or save a workflow result."}
            </p>
            <button
              type="button"
              onClick={totalCount ? onResetFilters : onNewNote}
            >
              <Icon name={totalCount ? "arrow-clockwise" : "plus"} size={16} />
              {totalCount ? "Reset filters" : "New note"}
            </button>
          </div>
        )}
      </div>
    </section>
  );
}
