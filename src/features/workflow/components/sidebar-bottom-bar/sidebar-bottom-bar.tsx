import { useState } from "react";
import {
  getShortcutHelpLines,
  useShortcutPreferences,
} from "../../../settings/shortcuts";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  MenuItem,
  MenuLabel,
  MenuSeparator,
  useDropdownMenuClose,
} from "../../../../components/menu";
import { version as appVersion } from "../../../../../package.json";

function HelpIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden>
      <circle cx="8" cy="8" r="5.75" stroke="currentColor" strokeWidth="1.35" />
      <path
        d="M6.4 6.2a1.7 1.7 0 0 1 3.25.7c0 1.15-1.65 1.45-1.65 2.4"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
      <circle cx="8" cy="11.35" r="0.65" fill="currentColor" />
    </svg>
  );
}

function HelpMenuItems() {
  const close = useDropdownMenuClose();
  const shortcuts = useShortcutPreferences((state) => state.shortcuts);

  return (
    <>
      <MenuLabel>Help</MenuLabel>
      <MenuItem
        onSelect={() => {
          close();
          window.alert(
            [
              "Keyboard shortcuts",
              "",
              ...getShortcutHelpLines(shortcuts),
              "Right-click workflow — Actions",
              "Add step (canvas) — New node menu",
              "Right-click canvas — Add step",
            ].join("\n"),
          );
        }}
      >
        Keyboard shortcuts
      </MenuItem>
      <MenuItem
        onSelect={() => {
          close();
          window.alert(
            [
              `Version ${appVersion}`,
              "",
              "Alfred runs local coding agents as visual workflow automations. Workflows, memories, and runs stay on this machine.",
            ].join("\n"),
          );
        }}
      >
        About Alfred
      </MenuItem>
      <MenuSeparator />
      <MenuItem
        onSelect={() => {
          close();
          window.dispatchEvent(new Event("alfred:open-schedule"));
        }}
      >
        Schedule current workflow…
      </MenuItem>
    </>
  );
}

export function SidebarBottomBar() {
  const [helpOpen, setHelpOpen] = useState(false);

  return (
    <div className="sidebar-bottom-bar">
      <div className="sidebar-bottom-actions sidebar-bottom-actions-left">
        <DropdownMenu
          className="sidebar-help"
          open={helpOpen}
          onOpenChange={setHelpOpen}
        >
          <DropdownMenuTrigger
            className={[
              "sidebar-bottom-icon",
              helpOpen ? "is-active" : "",
            ]
              .filter(Boolean)
              .join(" ")}
            title="Help"
            aria-label="Help"
          >
            <HelpIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" side="top" aria-label="Help">
            <HelpMenuItems />
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}
