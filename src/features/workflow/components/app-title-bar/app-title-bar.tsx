import { useEffect, useRef, useState } from "react";
import { getName } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "../../../../components/icon";
import { LicenseBadge } from "../../../licensing";

type Tab = {
  id: string;
  name: string;
  dirty?: boolean;
  scheduleLabel?: string;
};

type Props = {
  tabs: Tab[];
  activeWorkflowId: string | null;
  sidebarCollapsed: boolean;
  activityOpen: boolean;
  activityEnabled: boolean;
  activityRunning: boolean;
  activityEventCount?: number;
  onToggleSidebar: () => void;
  onToggleActivity: () => void;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onNewTab: () => void;
  onRenameTab: (id: string, name: string) => void;
};

function SidebarIcon({ collapsed }: { collapsed: boolean }) {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden
      className={collapsed ? "is-collapsed-icon" : undefined}
    >
      <rect
        x="1.5"
        y="2.5"
        width="13"
        height="11"
        rx="2"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <path d="M5.5 2.5v11" stroke="currentColor" strokeWidth="1.4" />
    </svg>
  );
}

function ActivityPanelIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect
        x="1.5"
        y="2.5"
        width="13"
        height="11"
        rx="2"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <path d="M10.5 2.5v11" stroke="currentColor" strokeWidth="1.4" />
    </svg>
  );
}

export function AppTitlebar({
  tabs,
  activeWorkflowId,
  sidebarCollapsed,
  activityOpen,
  activityEnabled,
  activityRunning,
  activityEventCount = 0,
  onToggleSidebar,
  onToggleActivity,
  onSelectTab,
  onCloseTab,
  onNewTab,
  onRenameTab,
}: Props) {
  const tabsRef = useRef<HTMLDivElement | null>(null);
  const tabRefs = useRef(new Map<string, HTMLDivElement>());
  const [fullscreen, setFullscreen] = useState(false);
  const [productName, setProductName] = useState("Alfred");

  useEffect(() => {
    if (!isTauri()) return;
    void getName().then(setProductName).catch((error) => {
      console.warn("Failed to resolve application name", error);
    });
    const win = getCurrentWindow();
    const unsubs: Array<() => void> = [];

    const sync = () => {
      void win
        .isFullscreen()
        .then(setFullscreen)
        .catch(() => setFullscreen(false));
      void invoke("sync_macos_traffic_lights").catch(() => {});
    };

    sync();
    void win.onResized(sync).then((u) => unsubs.push(u));
    void win.onFocusChanged(sync).then((u) => unsubs.push(u));

    return () => {
      for (const u of unsubs) u();
    };
  }, []);

  useEffect(() => {
    if (!activeWorkflowId) return;
    const container = tabsRef.current;
    const el = tabRefs.current.get(activeWorkflowId);
    if (!container || !el) return;

    const id = window.requestAnimationFrame(() => {
      const left = el.offsetLeft;
      const right = left + el.offsetWidth;
      const viewLeft = container.scrollLeft;
      const viewRight = viewLeft + container.clientWidth;
      const pad = 12;

      if (left < viewLeft + pad) {
        container.scrollTo({ left: Math.max(0, left - pad), behavior: "smooth" });
      } else if (right > viewRight - pad) {
        container.scrollTo({
          left: right - container.clientWidth + pad,
          behavior: "smooth",
        });
      }
    });
    return () => window.cancelAnimationFrame(id);
  }, [activeWorkflowId, tabs.length]);

  return (
    <header
      className={`app-titlebar${fullscreen ? " is-fullscreen" : ""}`}
      data-tauri-drag-region
    >
      <div className="titlebar-leading" data-tauri-drag-region>
        <p className="titlebar-brand">{productName}</p>
        <button
          type="button"
          className="ghost titlebar-icon-btn"
          title={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
          aria-label={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
          aria-pressed={!sidebarCollapsed}
          onClick={onToggleSidebar}
        >
          <SidebarIcon collapsed={sidebarCollapsed} />
        </button>
      </div>

      <div className="titlebar-tabs-wrap" data-tauri-drag-region>
        <div
          ref={tabsRef}
          className="titlebar-tabs"
          role="tablist"
          aria-label="Open workflows"
          data-tauri-drag-region
        >
          {tabs.map((tab) => {
            const active = tab.id === activeWorkflowId;
            return (
              <div
                key={tab.id}
                ref={(node) => {
                  if (node) tabRefs.current.set(tab.id, node);
                  else tabRefs.current.delete(tab.id);
                }}
                className={`workflow-tab${active ? " is-active" : ""}`}
                role="tab"
                aria-selected={active}
                data-workflow-tab={tab.id}
              >
                <button
                  type="button"
                  className="workflow-tab-main"
                  onClick={() => onSelectTab(tab.id)}
                  onDoubleClick={() => onRenameTab(tab.id, tab.name)}
                  title={tab.name}
                >
                  <span className="workflow-tab-dot" aria-hidden />
                  <span className="workflow-tab-name">{tab.name}</span>
                  {tab.dirty ? <span className="workflow-tab-dirty" /> : null}
                  {tab.scheduleLabel ? (
                    <span
                      className="workflow-tab-schedule"
                      title={`Runs ${tab.scheduleLabel}`}
                    >
                      ◷
                    </span>
                  ) : null}
                </button>
                <button
                  type="button"
                  className="ghost workflow-tab-close"
                  title={`Close ${tab.name}`}
                  aria-label={`Close ${tab.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    onCloseTab(tab.id);
                  }}
                >
                  <Icon name="x" size={14} />
                </button>
              </div>
            );
          })}
        </div>
        <button
          type="button"
          className="ghost titlebar-icon-btn titlebar-new-tab"
          title="New workflow"
          aria-label="New workflow"
          onClick={onNewTab}
        >
          +
        </button>
      </div>

      <div className="titlebar-trailing" data-tauri-drag-region>
        <button
          type="button"
          className={`ghost titlebar-activity${activityOpen ? " is-active" : ""}`}
          disabled={!activityEnabled && !activityOpen}
          title={
            activityRunning
              ? "Open live console"
              : activityOpen
                ? "Hide activity panel"
                : activityEnabled
                  ? "Open activity panel"
                  : "Run an automation to see activity"
          }
          aria-pressed={activityOpen}
          onClick={onToggleActivity}
        >
          <ActivityPanelIcon />
          <span>{activityRunning ? "Live" : "Activity"}</span>
          {activityEventCount > 0 ? (
            <span className="titlebar-count">{activityEventCount}</span>
          ) : null}
        </button>
        <LicenseBadge />
      </div>
    </header>
  );
}
