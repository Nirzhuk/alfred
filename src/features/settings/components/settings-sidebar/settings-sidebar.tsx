import { useMemo, useState, type ReactNode } from "react";
import { SearchInput } from "../../../../components/search-input";
import {
  SETTINGS_SECTION_LABELS,
  type SettingsSectionId,
} from "./settings-sections";

type SettingsNavItem = {
  id: SettingsSectionId;
  label: string;
  description: string;
  icon: ReactNode;
};

type SettingsNavGroup = {
  label: string;
  items: SettingsNavItem[];
};

type Props = {
  activeSection: SettingsSectionId;
  onChange: (section: SettingsSectionId) => void;
  onBack: () => void;
};

function BackIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="m9.75 3.5-4.5 4.5 4.5 4.5M5.5 8h5.75"
        stroke="currentColor"
        strokeWidth="1.45"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function GeneralIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M6.48 1.75h3.04l.32 1.72c.44.15.84.38 1.2.66l1.66-.6 1.52 2.64-1.34 1.12c.07.38.11.78.11 1.19 0 .41-.04.81-.11 1.19l1.34 1.12-1.52 2.64-1.66-.6a4.8 4.8 0 0 1-1.2.66l-.32 1.72H6.48l-.32-1.72a4.8 4.8 0 0 1-1.2-.66l-1.66.6-1.52-2.64 1.34-1.12A5 5 0 0 1 2.9 8.48c0-.41.04-.81.11-1.19L1.67 6.17l1.52-2.64 1.66.6c.36-.28.76-.51 1.2-.66l.32-1.72Z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
      <circle cx="8" cy="8" r="2" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  );
}

function QuickAccessIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="1.75"
        y="3"
        width="12.5"
        height="9.75"
        rx="2.25"
        stroke="currentColor"
        strokeWidth="1.3"
      />
      <path
        d="M10.2 5.2h2M11.2 4.2v2M4 9.9h4.25"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinecap="round"
      />
    </svg>
  );
}

function ShortcutsIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="1.75"
        y="3.25"
        width="12.5"
        height="9.5"
        rx="1.5"
        stroke="currentColor"
        strokeWidth="1.3"
      />
      <path
        d="M4.25 6.25h.5M7.75 6.25h.5M11.25 6.25h.5M4.25 9.25h.5M7 9.25h2M11.25 9.25h.5"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}

function NotificationIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M3.5 11.25h9l-1.15-1.4V6.8A3.35 3.35 0 0 0 8 3.45 3.35 3.35 0 0 0 4.65 6.8v3.05L3.5 11.25Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
      <path
        d="M6.55 12.45a1.55 1.55 0 0 0 2.9 0"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}

function LicenseIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="1.75"
        y="3"
        width="12.5"
        height="10"
        rx="2"
        stroke="currentColor"
        strokeWidth="1.3"
      />
      <path
        d="M1.75 6.25h12.5M4.25 10h3"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinecap="round"
      />
    </svg>
  );
}

function ConnectedAppsIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M6.3 9.7 9.7 6.3M5.15 11.85l-1 .98a2.1 2.1 0 0 1-2.98-2.96l2.05-2.04a2.1 2.1 0 0 1 2.97 0M10.85 4.15l1-.98a2.1 2.1 0 0 1 2.98 2.96l-2.05 2.04a2.1 2.1 0 0 1-2.97 0"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}

function NativeAgentsIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="2.25"
        y="4"
        width="11.5"
        height="8.75"
        rx="2.25"
        stroke="currentColor"
        strokeWidth="1.3"
      />
      <path
        d="M8 1.75V4M5.25 8h.5M10.25 8h.5M5.75 10.5h4.5"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinecap="round"
      />
    </svg>
  );
}

function MemoryReviewIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M8 2.5 9.6 5.7l3.4.5-2.5 2.4.6 3.4L8 10.4l-3.1 1.6.6-3.4L3 6.2l3.4-.5L8 2.5Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function DataIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden>
      <ellipse
        cx="8"
        cy="4"
        rx="4.75"
        ry="2.25"
        stroke="currentColor"
        strokeWidth="1.35"
      />
      <path
        d="M3.25 4v4c0 1.24 2.13 2.25 4.75 2.25S12.75 9.24 12.75 8V4M3.25 8v4c0 1.24 2.13 2.25 4.75 2.25s4.75-1.01 4.75-2.25V8"
        stroke="currentColor"
        strokeWidth="1.35"
      />
    </svg>
  );
}

const NAV_GROUPS: SettingsNavGroup[] = [
  {
    label: "Application",
    items: [
      {
        id: "general",
        label: SETTINGS_SECTION_LABELS.general,
        description: "Behavior and app details",
        icon: <GeneralIcon />,
      },
      {
        id: "quick-access",
        label: SETTINGS_SECTION_LABELS["quick-access"],
        description: "Floating window, pin, and visibility",
        icon: <QuickAccessIcon />,
      },
      {
        id: "shortcuts",
        label: SETTINGS_SECTION_LABELS.shortcuts,
        description: "Customize global and application keys",
        icon: <ShortcutsIcon />,
      },
      {
        id: "notifications",
        label: SETTINGS_SECTION_LABELS.notifications,
        description: "Run alerts and sounds",
        icon: <NotificationIcon />,
      },
    ],
  },
  {
    label: "Account",
    items: [
      {
        id: "license-billing",
        label: SETTINGS_SECTION_LABELS["license-billing"],
        description: "License, devices, and Polar billing",
        icon: <LicenseIcon />,
      },
    ],
  },
  {
    label: "Integrations",
    items: [
      {
        id: "native-agents",
        label: SETTINGS_SECTION_LABELS["native-agents"],
        description: "Accounts for Alfred's native agent harness",
        icon: <NativeAgentsIcon />,
      },
      {
        id: "connected-apps",
        label: SETTINGS_SECTION_LABELS["connected-apps"],
        description: "Accounts and external services",
        icon: <ConnectedAppsIcon />,
      },
      {
        id: "sharing",
        label: SETTINGS_SECTION_LABELS.sharing,
        description: "Review and revoke published workflow links",
        icon: <ConnectedAppsIcon />,
      },
    ],
  },
  {
    label: "Local",
    items: [
      {
        id: "memory-review",
        label: SETTINGS_SECTION_LABELS["memory-review"],
        description: "Post-run memory suggestions and consent",
        icon: <MemoryReviewIcon />,
      },
      {
        id: "data",
        label: SETTINGS_SECTION_LABELS.data,
        description: "Storage and linked memories",
        icon: <DataIcon />,
      },
    ],
  },
];

export function SettingsSidebar({ activeSection, onChange, onBack }: Props) {
  const [query, setQuery] = useState("");
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleGroups = useMemo(
    () =>
      NAV_GROUPS.map((group) => ({
        ...group,
        items: normalizedQuery
          ? group.items.filter((item) =>
              `${item.label} ${item.description}`
                .toLocaleLowerCase()
                .includes(normalizedQuery),
            )
          : group.items,
      })).filter((group) => group.items.length > 0),
    [normalizedQuery],
  );

  return (
    <div className="sidebar-scroll settings-sidebar-scroll">
      <div className="settings-sidebar-heading">
        <button
          type="button"
          className="settings-sidebar-back"
          title="Back to workflows"
          aria-label="Back to workflows"
          onClick={onBack}
        >
          <BackIcon />
        </button>
        <div>
          <p>Alfred</p>
          <h2>Settings</h2>
        </div>
      </div>

      <SearchInput
        aria-label="Search settings"
        value={query}
        placeholder="Search settings…"
        onChange={(event) => setQuery(event.currentTarget.value)}
        onClear={() => setQuery("")}
      />

      <nav className="settings-sidebar-nav" aria-label="Settings sections">
        {visibleGroups.map((group) => (
          <div className="settings-sidebar-group" key={group.label}>
            <h3>{group.label}</h3>
            <div className="settings-sidebar-group-items">
              {group.items.map((item) => {
                const active = item.id === activeSection;
                return (
                  <button
                    key={item.id}
                    type="button"
                    className={[
                      "settings-sidebar-item",
                      active ? "is-active" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    aria-current={active ? "page" : undefined}
                    onClick={() => onChange(item.id)}
                  >
                    <span className="settings-sidebar-item-icon">
                      {item.icon}
                    </span>
                    <span className="settings-sidebar-item-copy">
                      <span>{item.label}</span>
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        ))}

        {visibleGroups.length === 0 ? (
          <p className="settings-sidebar-empty">
            No settings match “{query.trim()}”.
          </p>
        ) : null}
      </nav>
    </div>
  );
}
