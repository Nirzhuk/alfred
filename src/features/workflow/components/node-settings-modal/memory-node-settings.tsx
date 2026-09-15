import { SearchInput } from "../../../../components/search-input";
import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../../../components/icon";
import * as api from "../../api";
import { withMemoryDefaults } from "../../memories";
import type { OutputMemory } from "../../types";
import {
  filterAndSortMemories,
  memorySearchSnippet,
  memoryUsageStatus,
} from "../memories-inspector/memory-list-model";
import { addMemorySelection, memoryPickerItems } from "./memory-node-model";
import "./memory-node-settings.css";

type Props = {
  label: string;
  memoryIds: string[];
  memories: OutputMemory[];
  activeWorkflowId: string | null;
  onUpdate: (patch: { label?: string; memoryIds?: string[] }) => void;
  onLinkMemory: (memoryId: string) => Promise<OutputMemory | null>;
  onOpenLibrary: () => void;
};

function provenance(memory: OutputMemory) {
  const normalized = withMemoryDefaults(memory);
  if (normalized.origin === "linked") {
    return `Linked from ${normalized.sourceWorkflowName || "another workflow"} · Read-only`;
  }
  if (normalized.origin === "linkable") {
    return `From ${normalized.sourceWorkflowName || "another workflow"} · Links when selected`;
  }
  return `${normalized.scopeLabel} · ${normalized.memoryType}`;
}

export function MemoryNodeSettings({
  label,
  memoryIds,
  memories,
  activeWorkflowId,
  onUpdate,
  onLinkMemory,
  onOpenLibrary,
}: Props) {
  const [query, setQuery] = useState("");
  const [linkable, setLinkable] = useState<OutputMemory[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [selectionError, setSelectionError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const mounted = useRef(true);
  const busy = useRef(false);
  const currentIds = useRef(memoryIds);
  currentIds.current = memoryIds;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!activeWorkflowId) return;
    let cancelled = false;
    setLoading(true);
    setLoadError(false);
    void api
      .listLinkableMemories(activeWorkflowId)
      .then((rows) => {
        if (!cancelled) setLinkable(rows);
      })
      .catch(() => {
        if (!cancelled) setLoadError(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeWorkflowId, retry]);

  const { selected, available } = useMemo(
    () => memoryPickerItems(memories, linkable, memoryIds),
    [memories, linkable, memoryIds],
  );
  const results = useMemo(
    () => filterAndSortMemories(available, query, "all", "all"),
    [available, query],
  );

  const remove = (id: string) => {
    const next = currentIds.current.filter((memoryId) => memoryId !== id);
    currentIds.current = next;
    onUpdate({ memoryIds: next });
    setSelectionError(null);
  };

  const select = async (memory: OutputMemory) => {
    if (busy.current) return;
    const usage = memoryUsageStatus(memory);
    if (usage.state === "expired" || usage.state === "inactive") {
      setSelectionError(
        "This memory is no longer active. Choose another memory.",
      );
      return;
    }
    busy.current = true;
    setPendingId(memory.id);
    setSelectionError(null);
    try {
      const ids = await addMemorySelection(memory.id, {
        requiresLink: !memories.some(({ id }) => id === memory.id),
        linkMemory: onLinkMemory,
        currentSelection: () => currentIds.current,
        isCurrent: () => mounted.current,
      });
      if (!mounted.current) return;
      if (ids) {
        currentIds.current = ids;
        onUpdate({ memoryIds: ids });
      } else {
        setSelectionError(
          "Could not link this memory. Your selection has not changed.",
        );
      }
    } catch {
      if (mounted.current)
        setSelectionError("Could not select this memory. Try again.");
    } finally {
      busy.current = false;
      if (mounted.current) setPendingId(null);
    }
  };

  return (
    <div className="memory-node-settings">
      <div className="memory-node-intro">
        <span className="memory-node-intro-icon">
          <Icon name="note" size={20} />
        </span>
        <div>
          <strong>Choose what this step remembers</strong>
          <p>
            Selected memories are added to the agent context when this step
            runs. Large selections may be shortened or omitted by the shared
            memory context limit; the selection stays editable here.
          </p>
        </div>
      </div>
      <label className="field">
        <span>Label</span>
        <input
          value={label}
          onChange={(event) => onUpdate({ label: event.target.value })}
        />
      </label>

      <section className="memory-node-selection" aria-label="Selected memories">
        <div className="memory-node-section-heading">
          <h3>
            Selected <span>{selected.length}</span>
          </h3>
          <span>Used by this step</span>
        </div>
        {selected.length === 0 ? (
          <p className="memory-node-empty">
            Select a memory below to give this step context.
          </p>
        ) : (
          <ul className="memory-node-rows">
            {selected.map(({ id, memory }) => {
              const usage = memory ? memoryUsageStatus(memory) : null;
              const unavailable =
                !usage ||
                usage.state === "inactive" ||
                usage.state === "expired";
              return (
                <li
                  key={id}
                  className={`memory-node-selected-row${unavailable ? " is-unavailable" : ""}`}
                >
                  <Icon name={unavailable ? "clock" : "check"} size={16} />
                  <div className="memory-node-copy">
                    <strong>{memory?.title || "Unavailable memory"}</strong>
                    {memory ? <span>{provenance(memory)}</span> : null}
                    {unavailable ? (
                      <span className="memory-node-unavailable">
                        {usage
                          ? `${usage.label}. This memory will be skipped.`
                          : "This memory is no longer in the library. Remove it or link it again."}
                      </span>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    className="ghost memory-node-remove"
                    aria-label={`Remove ${memory?.title || "unavailable memory"}`}
                    disabled={pendingId !== null}
                    onClick={() => remove(id)}
                  >
                    <Icon name="x" size={14} />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section
        className="memory-node-available"
        aria-label="Available memories"
      >
        <div className="memory-node-section-heading">
          <h3>
            Available <span>{results.length}</span>
          </h3>
        </div>
        <SearchInput
          containerClassName="memory-node-search-layout"
          onClear={() => setQuery("")}
          type="search"
          aria-label="Search available memories"
          placeholder="Search content, type, or workflow"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        {selectionError ? (
          <p className="memory-node-error" role="alert">
            {selectionError}
          </p>
        ) : null}
        {loadError ? (
          <div className="memory-node-error" role="alert">
            <span>Could not load other workflows.</span>
            <button
              type="button"
              className="ghost"
              onClick={() => setRetry((value) => value + 1)}
            >
              Retry
            </button>
          </div>
        ) : null}
        {loading ? (
          <p className="memory-node-loading" role="status">
            Loading memories from other workflows…
          </p>
        ) : null}
        {results.length > 0 ? (
          <ul
            className="memory-node-rows memory-node-results"
            aria-busy={pendingId !== null}
          >
            {results.map((memory) => (
              <li key={memory.id}>
                <label className="memory-node-choice">
                  <input
                    type="checkbox"
                    checked={false}
                    disabled={pendingId !== null}
                    onChange={() => void select(memory)}
                  />
                  <span className="memory-node-copy">
                    <strong>{memory.title}</strong>
                    <span className="memory-node-snippet">
                      {memorySearchSnippet(memory, query, 110)}
                    </span>
                    <span>
                      {pendingId === memory.id
                        ? "Linking memory…"
                        : provenance(memory)}
                    </span>
                  </span>
                </label>
              </li>
            ))}
          </ul>
        ) : !loading ? (
          <p className="memory-node-empty">
            {query.trim()
              ? "No available memories match your search."
              : "No more memories to select. Create a note in the library to add context."}
          </p>
        ) : null}
      </section>
      <footer className="memory-node-footer">
        <span>Manage notes and shared context in your library.</span>
        <button type="button" className="ghost" onClick={onOpenLibrary}>
          <Icon name="arrow-square-out" size={15} />
          Open memory library
        </button>
      </footer>
    </div>
  );
}
