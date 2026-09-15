import { SearchInput } from "../../../components/search-input";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { AgentMark } from "../../../components/agent-mark";
import { Icon, type IconProps } from "../../../components/icon/icon";
import {
  ADD_STEP_GROUPS,
  addStepNodeType,
  filterAddStepGroups,
  type AddStepCategory,
  type AddStepItem,
} from "../add-step-items";
import { nodeAccentColor } from "../node-colors";
import type { AgentProviderId, WorkflowNodeData } from "../types";
import "./add-step-picker.css";

export type AddStepMenuHandlers = {
  onAddPrompt: (position: { x: number; y: number }) => void;
  onAddAgent: (
    provider: AgentProviderId,
    position: { x: number; y: number },
  ) => void;
  onAddChoose: (position: { x: number; y: number }) => void;
  onAddMemory: (position: { x: number; y: number }) => void;
  onAddStep: (
    type: string,
    data: WorkflowNodeData,
    position: { x: number; y: number },
  ) => void;
};

type Props = AddStepMenuHandlers & {
  getPosition: () => { x: number; y: number };
  close: () => void;
};

const STEP_ICONS: Record<string, IconProps["name"]> = {
  input: "chat-text",
  identity: "fingerprint",
  memory: "note",
  customAgent: "terminal-window",
  template: "code",
  fileInject: "file",
  gitStatus: "git-branch",
  script: "terminal-window",
  http: "globe",
  chooseOutput: "arrow-square-out",
  appAction: "link",
  notify: "bell",
  writeFile: "file-plus",
  gitHost: "git-pull-request",
};

function selectItem(
  item: AddStepItem,
  handlers: AddStepMenuHandlers,
  position: { x: number; y: number },
) {
  if (item.kind === "prompt") handlers.onAddPrompt(position);
  else if (item.kind === "choose") handlers.onAddChoose(position);
  else if (item.kind === "memory") handlers.onAddMemory(position);
  else if (item.kind === "agent") handlers.onAddAgent(item.provider, position);
  else handlers.onAddStep(item.type, item.data, position);
}

export function AddStepMenuItems({ getPosition, close, ...handlers }: Props) {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<AddStepCategory>("all");
  const [activeId, setActiveId] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const id = useId();
  const listId = `${id}-steps`;
  const groups = filterAddStepGroups(query, category);
  const items = groups.flatMap((group) => group.items);
  const activeItem = items.find((item) => item.id === activeId) ?? items[0];

  useEffect(() => {
    const previousFocus = document.activeElement;
    const root = rootRef.current;
    const frame = requestAnimationFrame(() => searchRef.current?.focus());
    return () => {
      cancelAnimationFrame(frame);
      if (
        previousFocus instanceof HTMLElement &&
        previousFocus.isConnected &&
        (root?.contains(document.activeElement) ||
          document.activeElement === document.body)
      ) {
        previousFocus.focus({ preventScroll: true });
      }
    };
  }, []);

  const addItem = (item: AddStepItem) => {
    selectItem(item, handlers, getPosition());
    close();
  };

  const resetSelection = () => {
    setActiveId("");
    if (resultsRef.current) resultsRef.current.scrollTop = 0;
  };

  const onSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      if (!items.length) return;
      const current = items.findIndex((item) => item.id === activeItem?.id);
      const direction = event.key === "ArrowDown" ? 1 : -1;
      const next = items[(current + direction + items.length) % items.length];
      setActiveId(next.id);
      document
        .getElementById(`${listId}-${next.id}`)
        ?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter" && activeItem) {
      event.preventDefault();
      event.stopPropagation();
      addItem(activeItem);
    }
  };

  return (
    <div
      className="add-step-picker nodrag nowheel"
      ref={rootRef}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          if (event.nativeEvent.isComposing) return;
          event.preventDefault();
          close();
        }
      }}
    >
      <div className="add-step-picker-heading">
        <span>Add step</span>
        <button
          type="button"
          className="add-step-picker-close"
          aria-label="Close add step"
          onClick={close}
        >
          <Icon name="x" size={16} />
        </button>
      </div>
      <SearchInput
        containerClassName="add-step-picker-search"
        onClear={() => {
          setQuery("");
          resetSelection();
        }}
        ref={searchRef}
        type="text"
        role="combobox"
        aria-label="Search steps"
        aria-autocomplete="list"
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={
          activeItem ? `${listId}-${activeItem.id}` : undefined
        }
        aria-describedby={`${id}-help`}
        placeholder="Search steps..."
        autoComplete="off"
        spellCheck={false}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          resetSelection();
        }}
        onKeyDown={onSearchKeyDown}
      />
      <div
        className="add-step-picker-categories"
        role="group"
        aria-label="Step categories"
      >
        {[{ id: "all" as const, label: "All" }, ...ADD_STEP_GROUPS].map(
          (group) => (
            <button
              key={group.id}
              type="button"
              aria-pressed={category === group.id}
              onClick={() => {
                setCategory(group.id);
                resetSelection();
                searchRef.current?.focus();
              }}
            >
              {group.label}
            </button>
          ),
        )}
      </div>
      <div
        ref={resultsRef}
        id={listId}
        className="add-step-picker-results"
        role="listbox"
        aria-label="Available steps"
      >
        {groups.map((group) => (
          <div
            key={group.id}
            role="group"
            aria-labelledby={`${id}-${group.id}`}
            className="add-step-picker-group"
          >
            <div
              id={`${id}-${group.id}`}
              className="add-step-picker-group-label"
            >
              {group.label}
            </div>
            {group.items.map((item) => {
              const nodeType = addStepNodeType(item);
              return (
                <button
                  key={item.id}
                  id={`${listId}-${item.id}`}
                  type="button"
                  role="option"
                  tabIndex={-1}
                  aria-selected={activeItem?.id === item.id}
                  className="add-step-picker-item"
                  onPointerMove={() => setActiveId(item.id)}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => addItem(item)}
                  style={
                    {
                      "--step-color": nodeAccentColor(nodeType),
                    } as CSSProperties
                  }
                >
                  <span className="add-step-picker-item-icon" aria-hidden>
                    {item.kind === "agent" ? (
                      <AgentMark provider={item.provider} size={18} />
                    ) : (
                      <Icon
                        name={
                          item.id === "openIssue"
                            ? "note-pencil"
                            : (STEP_ICONS[nodeType] ?? "plus")
                        }
                        size={18}
                      />
                    )}
                  </span>
                  <span className="add-step-picker-item-copy">
                    <span className="add-step-picker-item-label">
                      {item.label}
                    </span>
                    <span className="add-step-picker-item-description">
                      {item.description}
                    </span>
                  </span>
                  <Icon
                    name="plus"
                    size={14}
                    className="add-step-picker-item-add"
                  />
                </button>
              );
            })}
          </div>
        ))}
        {items.length === 0 ? (
          <div className="add-step-picker-empty" role="presentation">
            <Icon name="magnifying-glass" size={24} />
            <p>No matching steps</p>
            <span>Try another name or choose a different category.</span>
          </div>
        ) : null}
      </div>
      <div className="add-step-picker-footer">
        <span role="status">
          {items.length} {items.length === 1 ? "step" : "steps"}
        </span>
        <span id={`${id}-help`}>
          <kbd>↑↓</kbd> Navigate <kbd>↵</kbd> Add
        </span>
      </div>
    </div>
  );
}
