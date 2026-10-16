import type { ReactNode } from "react";
import { Icon } from "../../../../components/icon/icon";

export type SidebarView = "canvas" | "history" | "schedules" | "settings" | "builder";

type Props = {
  view: SidebarView;
  activityOpen: boolean;
  activityEnabled: boolean;
  memoriesOpen: boolean;
  memoriesEnabled: boolean;
  onChange: (view: SidebarView) => void;
  onNewWorkflow: () => void;
  onToggleActivity: () => void;
  onOpenMemories: () => void;
  onOpenConnectedApps: () => void;
  onOpenSettings: () => void;
};

function NewWorkflowIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M11.75 3.25H5.5A2.25 2.25 0 0 0 3.25 5.5v9A2.25 2.25 0 0 0 5.5 16.75h9a2.25 2.25 0 0 0 2.25-2.25V8.25"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="m9 11 1.05-3.05 4.7-4.7a1.42 1.42 0 0 1 2 2l-4.7 4.7L9 11Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ActivityIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M3.25 15.75v-4.5M7.75 15.75v-9M12.25 15.75v-6.5M16.75 15.75v-12"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}

function MemoriesIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M4.25 3.25h11.5v13.5L13 14.9l-3 1.85-3-1.85-2.75 1.85V3.25Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
      <path
        d="M7 7h6M7 10h4"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}

function ClockIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <circle cx="10" cy="10" r="7" stroke="currentColor" strokeWidth="1.35" />
      <path
        d="M10 6.25V10l2.65 1.75"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}

function HistoryIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M4.1 6.2A7 7 0 1 1 3 10"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
      <path
        d="M4.1 2.9v3.3H.8M10 6.1v4.2l2.8 1.7"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ConnectedAppsIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M7.4 12.6 5.8 14.2a2.83 2.83 0 0 1-4-4l2.45-2.45a2.83 2.83 0 0 1 4 0M12.6 7.4l1.6-1.6a2.83 2.83 0 1 1 4 4l-2.45 2.45a2.83 2.83 0 0 1-4 0M7.25 12.75l5.5-5.5"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M8.05 2.3h3.9l.4 2.15c.55.2 1.06.47 1.5.83l2.08-.75 1.9 3.3-1.68 1.4c.1.48.14.98.14 1.49s-.05 1.01-.14 1.49l1.68 1.4-1.9 3.3-2.08-.75c-.44.36-.95.63-1.5.83l-.4 2.15h-3.9l-.4-2.15a6 6 0 0 1-1.5-.83l-2.08.75-1.9-3.3 1.68-1.4a6.4 6.4 0 0 1 0-2.98l-1.68-1.4 1.9-3.3 2.08.75c.44-.36.95-.63 1.5-.83l.4-2.15Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
      <circle
        cx="10"
        cy="10.72"
        r="2.45"
        stroke="currentColor"
        strokeWidth="1.35"
      />
    </svg>
  );
}

function NavButton({
  label,
  icon,
  active = false,
  disabled = false,
  title,
  onClick,
}: {
  label: string;
  icon: ReactNode;
  active?: boolean;
  disabled?: boolean;
  title?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={["sidebar-nav-item", active ? "is-active" : ""]
        .filter(Boolean)
        .join(" ")}
      aria-current={active ? "page" : undefined}
      aria-pressed={active || undefined}
      disabled={disabled}
      title={title}
      onClick={onClick}
    >
      <span className="sidebar-nav-icon">{icon}</span>
      <span>{label}</span>
    </button>
  );
}

export function SidebarNav({
  view,
  activityOpen,
  activityEnabled,
  memoriesOpen,
  memoriesEnabled,
  onChange,
  onNewWorkflow,
  onToggleActivity,
  onOpenMemories,
  onOpenConnectedApps,
  onOpenSettings,
}: Props) {
  return (
    <nav className="sidebar-nav" aria-label="App sections">
      <NavButton
        label="New workflow"
        icon={<NewWorkflowIcon />}
        onClick={onNewWorkflow}
      />
      <NavButton
        label="Workflow Builder"
        icon={<Icon name="sparkle" size={18} />}
        active={view === "builder"}
        onClick={() => onChange("builder")}
      />
      <NavButton
        label="Activity"
        icon={<ActivityIcon />}
        active={activityOpen}
        disabled={!activityEnabled && !activityOpen}
        title={!activityEnabled ? "Run a workflow to see activity" : undefined}
        onClick={onToggleActivity}
      />
      <NavButton
        label="Memories"
        icon={<MemoriesIcon />}
        active={memoriesOpen}
        disabled={!memoriesEnabled}
        title={!memoriesEnabled ? "Open a workflow to view its memories" : undefined}
        onClick={onOpenMemories}
      />
      <NavButton
        label="History"
        icon={<HistoryIcon />}
        active={view === "history"}
        onClick={() => onChange("history")}
      />
      <NavButton
        label="Schedules"
        icon={<ClockIcon />}
        active={view === "schedules"}
        onClick={() => onChange("schedules")}
      />
      <NavButton
        label="Connected apps"
        icon={<ConnectedAppsIcon />}
        onClick={onOpenConnectedApps}
      />
      <NavButton
        label="Settings"
        icon={<SettingsIcon />}
        active={view === "settings"}
        onClick={onOpenSettings}
      />
    </nav>
  );
}
