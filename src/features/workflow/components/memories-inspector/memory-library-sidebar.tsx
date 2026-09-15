import { Icon, type IconProps } from "../../../../components/icon";
import type {
  MemoryFilterCounts,
  MemoryQuickFilter,
  MemoryScopeFilter,
} from "./memory-list-model";
import "./memory-library.css";

export type MemoryLibrarySidebarProps = {
  workflowName: string;
  mode: "memories" | "suggestions";
  onModeChange: (
    mode: "memories" | "suggestions",
  ) => boolean | void | Promise<boolean | void>;
  quickFilter: MemoryQuickFilter;
  onQuickFilterChange: (filter: MemoryQuickFilter) => void;
  scopeFilter: MemoryScopeFilter;
  onScopeFilterChange: (filter: MemoryScopeFilter) => void;
  counts: MemoryFilterCounts;
  pendingCount: number;
  automaticRecall: boolean;
  automaticSuggestions: boolean;
  suggestionsUnavailableReason?: string | null;
  settingsOpen?: boolean;
  onOpenSettings: () => void;
};

const LIBRARY_VIEWS: Array<{
  id: MemoryQuickFilter;
  label: string;
  icon: IconProps["name"];
}> = [
  { id: "all", label: "All memories", icon: "database" },
  { id: "pinned", label: "Pinned", icon: "push-pin" },
  { id: "linked", label: "Linked", icon: "link" },
  { id: "inactive", label: "Inactive", icon: "clock" },
];

const SCOPE_VIEWS: Array<{
  id: Exclude<MemoryScopeFilter, "all">;
  label: string;
  icon: IconProps["name"];
}> = [
  { id: "workflow", label: "This workflow", icon: "git-branch" },
  { id: "workspace", label: "Workspace", icon: "folder" },
  { id: "user", label: "Personal", icon: "fingerprint" },
];

export function MemoryLibrarySidebar({
  workflowName,
  mode,
  onModeChange,
  quickFilter,
  onQuickFilterChange,
  scopeFilter,
  onScopeFilterChange,
  counts,
  pendingCount,
  automaticRecall,
  automaticSuggestions,
  suggestionsUnavailableReason,
  settingsOpen = false,
  onOpenSettings,
}: MemoryLibrarySidebarProps) {
  const suggestionsEnabled =
    automaticSuggestions && !suggestionsUnavailableReason;

  return (
    <aside
      className="memory-library-sidebar"
      aria-label="Memory library navigation"
    >
      <div className="memory-library-context">
        <span className="memory-library-context-icon" aria-hidden>
          <Icon name="note" size={20} />
        </span>
        <div>
          <span>Memory library</span>
          <strong title={workflowName}>
            {workflowName || "No workflow selected"}
          </strong>
        </div>
      </div>

      <nav className="memory-library-navigation" aria-label="Memory views">
        <div className="memory-library-nav-section">
          {LIBRARY_VIEWS.map((view) => (
            <button
              key={view.id}
              type="button"
              className="memory-library-nav-item"
              aria-pressed={
                !settingsOpen &&
                mode === "memories" &&
                scopeFilter === "all" &&
                quickFilter === view.id
              }
              onClick={async () => {
                if ((await onModeChange("memories")) === false) return;
                onScopeFilterChange("all");
                onQuickFilterChange(view.id);
              }}
            >
              <Icon name={view.icon} size={18} />
              <span>{view.label}</span>
              <span className="memory-library-nav-count">
                {counts[view.id]}
              </span>
            </button>
          ))}
          <button
            type="button"
            className="memory-library-nav-item memory-library-suggestions-nav"
            aria-pressed={!settingsOpen && mode === "suggestions"}
            onClick={() => void onModeChange("suggestions")}
          >
            <Icon name="note-pencil" size={18} />
            <span>Suggestions</span>
            {pendingCount > 0 ? (
              <span
                className="memory-library-nav-count is-pending"
                aria-label={`${pendingCount} pending`}
              >
                {pendingCount}
              </span>
            ) : null}
          </button>
        </div>

        <div className="memory-library-nav-section">
          <p className="memory-library-nav-label">Scope</p>
          {SCOPE_VIEWS.map((view) => (
            <button
              key={view.id}
              type="button"
              className="memory-library-nav-item"
              aria-pressed={
                !settingsOpen && mode === "memories" && scopeFilter === view.id
              }
              onClick={async () => {
                if ((await onModeChange("memories")) === false) return;
                onQuickFilterChange("all");
                onScopeFilterChange(view.id);
              }}
            >
              <Icon name={view.icon} size={18} />
              <span>{view.label}</span>
              <span className="memory-library-nav-count">
                {counts[view.id]}
              </span>
            </button>
          ))}
        </div>
      </nav>

      <div className="memory-library-sidebar-footer">
        <div className="memory-library-status">
          <span>Automatic recall</span>
          <span className={automaticRecall ? "is-enabled" : undefined}>
            {automaticRecall ? "On" : "Off"}
          </span>
        </div>
        <div
          className="memory-library-status"
          title={suggestionsUnavailableReason ?? undefined}
        >
          <span>Suggestions</span>
          <span className={suggestionsEnabled ? "is-enabled" : undefined}>
            {suggestionsUnavailableReason
              ? "Set up"
              : suggestionsEnabled
                ? "On"
                : "Off"}
          </span>
        </div>
        <button
          type="button"
          className="memory-library-settings-button"
          aria-expanded={settingsOpen}
          onClick={onOpenSettings}
        >
          <Icon name="gear-six" size={16} />
          Memory settings
          <Icon name="caret-down" size={14} />
        </button>
      </div>
    </aside>
  );
}
