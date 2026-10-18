import { getIdentifier } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { detectDesktopPlatform } from "../../platform";

const SHORTCUTS_KEY = "alfred:keyboard-shortcuts";

export const SHORTCUT_DEFINITIONS = [
  {
    id: "quickAccess",
    label: "Open Quick Access",
    description: "Show Quick Access from anywhere on your desktop.",
    scope: "System-wide",
  },
  {
    id: "newWorkflow",
    label: "New workflow",
    description: "Create a workflow in the main Alfred window.",
    scope: "Application",
  },
  {
    id: "saveWorkflow",
    label: "Save workflow",
    description: "Save changes to the current workflow.",
    scope: "Application",
  },
  {
    id: "runWorkflow",
    label: "Run workflow",
    description: "Run the currently selected workflow.",
    scope: "Application",
  },
  {
    id: "deleteWorkflow",
    label: "Delete workflow",
    description: "Delete the currently selected workflow after confirmation.",
    scope: "Application",
  },
  {
    id: "openSettings",
    label: "Open Settings",
    description: "Open Alfred’s Settings page.",
    scope: "Application",
  },
] as const;

export type ShortcutId = (typeof SHORTCUT_DEFINITIONS)[number]["id"];
export type ShortcutMap = Record<ShortcutId, string>;

const INITIAL_SHORTCUTS: ShortcutMap = {
  quickAccess: "",
  newWorkflow: "CmdOrCtrl+N",
  saveWorkflow: "CmdOrCtrl+S",
  runWorkflow: "CmdOrCtrl+R",
  deleteWorkflow: "CmdOrCtrl+Backspace",
  openSettings: "CmdOrCtrl+Comma",
};

async function resolveDefaultShortcuts(): Promise<ShortcutMap> {
  const identifier = await getIdentifier();
  return {
    ...INITIAL_SHORTCUTS,
    quickAccess:
      identifier === "com.nirzhuk.alfred.dev"
        ? "CmdOrCtrl+Alt+Shift+Space"
        : "CmdOrCtrl+Shift+Space",
  };
}

type ShortcutPreferences = {
  shortcuts: ShortcutMap;
  defaults: ShortcutMap | null;
  busy: boolean;
  setShortcut: (id: ShortcutId, accelerator: string) => Promise<void>;
  resetShortcuts: () => Promise<void>;
};

function isMacPlatform(): boolean {
  return detectDesktopPlatform() === "macos";
}

function readShortcuts(defaults: ShortcutMap): ShortcutMap {
  try {
    const saved = JSON.parse(localStorage.getItem(SHORTCUTS_KEY) ?? "{}") as
      | Partial<Record<ShortcutId, unknown>>
      | null;
    const shortcuts = { ...defaults };
    if (!saved || typeof saved !== "object") return shortcuts;

    for (const { id } of SHORTCUT_DEFINITIONS) {
      if (typeof saved[id] === "string" && saved[id].trim()) {
        shortcuts[id] = saved[id];
      }
    }
    return shortcuts;
  } catch {
    return { ...defaults };
  }
}

function persistShortcuts(shortcuts: ShortcutMap): void {
  try {
    localStorage.setItem(SHORTCUTS_KEY, JSON.stringify(shortcuts));
  } catch {
    /* Preferences remain available for the current session. */
  }
}

export function shortcutFromKeyboardEvent(
  event: Pick<
    KeyboardEvent,
    "altKey" | "code" | "ctrlKey" | "key" | "metaKey" | "shiftKey"
  >,
  mac = isMacPlatform(),
): string | null {
  if (["Alt", "Control", "Meta", "Shift"].includes(event.key)) return null;

  const modifiers: string[] = [];
  if ((mac && event.metaKey) || (!mac && event.ctrlKey)) {
    modifiers.push("CmdOrCtrl");
  }
  if (mac && event.ctrlKey) modifiers.push("Ctrl");
  if (!mac && event.metaKey) modifiers.push("Super");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");

  const key = normalizeEventCode(event.code, event.key);
  if (!key) return null;

  const isFunctionKey = /^F(?:[1-9]|1\d|2[0-4])$/.test(key);
  if (modifiers.length === 0 && !isFunctionKey) return null;
  return [...modifiers, key].join("+");
}

function normalizeEventCode(code: string, key: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^F(?:[1-9]|1\d|2[0-4])$/.test(code)) return code;

  const supportedCodes = new Set([
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "Backquote",
    "Backslash",
    "Backspace",
    "BracketLeft",
    "BracketRight",
    "Comma",
    "Delete",
    "End",
    "Enter",
    "Equal",
    "Escape",
    "Home",
    "Insert",
    "Minus",
    "PageDown",
    "PageUp",
    "Period",
    "Quote",
    "Semicolon",
    "Slash",
    "Space",
    "Tab",
  ]);
  if (supportedCodes.has(code)) return code;

  // Some webviews omit `code` for less common keyboard layouts. Tauri's
  // accelerator parser also accepts a single literal character.
  return key.length === 1 ? key.toUpperCase() : null;
}

const MAC_KEY_LABELS: Record<string, string> = {
  Alt: "⌥",
  Cmd: "⌘",
  CmdOrCtrl: "⌘",
  Ctrl: "⌃",
  Shift: "⇧",
  Super: "⌘",
};

const KEY_LABELS: Record<string, string> = {
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  Backquote: "`",
  Backslash: "\\",
  Backspace: "⌫",
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Delete: "Delete",
  Enter: "↩",
  Equal: "=",
  Escape: "Esc",
  Minus: "−",
  Period: ".",
  Quote: "'",
  Semicolon: ";",
  Slash: "/",
  Space: "Space",
  Tab: "Tab",
};

export function formatShortcut(
  accelerator: string,
  mac = isMacPlatform(),
): string {
  const tokens = accelerator.split("+");
  const labels = tokens.map((token) => {
    if (mac && MAC_KEY_LABELS[token]) return MAC_KEY_LABELS[token];
    if (!mac && token === "CmdOrCtrl") return "Ctrl";
    if (!mac && token === "Super") return "Win";
    return KEY_LABELS[token] ?? token;
  });
  return mac ? labels.join("") : labels.join("+");
}

export function findShortcutConflict(
  shortcuts: ShortcutMap,
  accelerator: string,
  exceptId: ShortcutId,
  mac = isMacPlatform(),
): ShortcutId | null {
  const candidate = shortcutCollisionKey(accelerator, mac);
  for (const { id } of SHORTCUT_DEFINITIONS) {
    if (
      id !== exceptId &&
      shortcutCollisionKey(shortcuts[id], mac) === candidate
    ) {
      return id;
    }
  }
  return null;
}

const RESERVED_SHORTCUTS = [
  { accelerator: "CmdOrCtrl+A", label: "Select All" },
  { accelerator: "CmdOrCtrl+C", label: "Copy" },
  { accelerator: "CmdOrCtrl+Q", label: "Quit" },
  { accelerator: "CmdOrCtrl+V", label: "Paste" },
  { accelerator: "CmdOrCtrl+X", label: "Cut" },
  { accelerator: "CmdOrCtrl+Z", label: "Undo" },
  { accelerator: "CmdOrCtrl+Shift+Z", label: "Redo" },
] as const;

export function findReservedShortcutConflict(
  accelerator: string,
  mac = isMacPlatform(),
): string | null {
  const candidate = shortcutCollisionKey(accelerator, mac);
  return (
    RESERVED_SHORTCUTS.find(
      (shortcut) =>
        shortcutCollisionKey(shortcut.accelerator, mac) === candidate,
    )?.label ?? null
  );
}

function shortcutCollisionKey(accelerator: string, mac: boolean): string {
  return accelerator
    .split("+")
    .map((token) => {
      if (token === "CmdOrCtrl") return mac ? "Cmd" : "Ctrl";
      if (token === "Super") return mac ? "Cmd" : "Super";
      return token;
    })
    .sort()
    .join("+")
    .toLocaleLowerCase();
}

export function getShortcutHelpLines(shortcuts: ShortcutMap): string[] {
  return SHORTCUT_DEFINITIONS.map(
    ({ id, label }) => `${formatShortcut(shortcuts[id])} — ${label}`,
  );
}

export async function syncShortcutPreferences(): Promise<void> {
  let defaults: ShortcutMap;
  try {
    defaults = await resolveDefaultShortcuts();
  } catch (error) {
    console.warn("Failed to resolve shortcut defaults", error);
    return;
  }
  const shortcuts = readShortcuts(defaults);
  useShortcutPreferences.setState({ shortcuts, defaults });
  try {
    await invoke("set_global_shortcut", {
      shortcut: shortcuts.quickAccess,
    });
  } catch (error) {
    const fallback = { ...shortcuts, quickAccess: defaults.quickAccess };
    useShortcutPreferences.setState({ shortcuts: fallback });
    console.warn("Failed to restore the global shortcut", error);
  }
}

export const useShortcutPreferences = create<ShortcutPreferences>((set, get) => ({
  shortcuts: readShortcuts(INITIAL_SHORTCUTS),
  defaults: null,
  busy: false,
  setShortcut: async (id, accelerator) => {
    const current = get().shortcuts;
    if (current[id] === accelerator) return;
    if (
      findShortcutConflict(current, accelerator, id) ||
      findReservedShortcutConflict(accelerator)
    ) {
      throw new Error("That shortcut is already in use.");
    }
    set({ busy: true });
    try {
      if (id === "quickAccess") {
        await invoke("set_global_shortcut", { shortcut: accelerator });
      }
      const shortcuts = { ...current, [id]: accelerator };
      persistShortcuts(shortcuts);
      set({ shortcuts });
    } finally {
      set({ busy: false });
    }
  },
  resetShortcuts: async () => {
    set({ busy: true });
    try {
      const defaults = await resolveDefaultShortcuts();
      if (get().shortcuts.quickAccess !== defaults.quickAccess) {
        await invoke("set_global_shortcut", {
          shortcut: defaults.quickAccess,
        });
      }
      const shortcuts = { ...defaults };
      persistShortcuts(shortcuts);
      set({ shortcuts, defaults });
    } finally {
      set({ busy: false });
    }
  },
}));
