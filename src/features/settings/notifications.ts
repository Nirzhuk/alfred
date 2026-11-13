import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { create } from "zustand";
import { detectDesktopPlatform } from "../../platform";
import {
  CONDITION_SKIP_SOUND_INTERVAL_MS,
  playSoundPreset,
} from "./feedback-sounds";

const ENABLED_KEY = "alfred:notifications-enabled";
const SUCCESS_SOUND_KEY = "alfred:notification-success-sound";
const ERROR_SOUND_KEY = "alfred:notification-error-sound";
/** Pre-split installs stored one sound at this key. */
const LEGACY_SOUND_KEY = "alfred:notification-sound";

export const NOTIFICATION_SOUND_OPTIONS = [
  { value: "system", label: "System default" },
  { value: "chime", label: "Chime" },
  { value: "ping", label: "Ping" },
  { value: "pop", label: "Pop" },
  { value: "none", label: "None" },
] as const;

export type NotificationSound =
  (typeof NOTIFICATION_SOUND_OPTIONS)[number]["value"];

/** Which outcome the configured sound belongs to. */
export type NotificationSoundKind = "success" | "error";

export type NotificationSounds = {
  success: NotificationSound;
  error: NotificationSound;
};

export type NotificationPermissionStatus = "unknown" | "granted" | "denied";

export type OpenRunOutputPayload = {
  workflowId: string;
  title: string;
  body: string;
  ok: boolean;
};

type NotificationsState = {
  enabled: boolean;
  successSound: NotificationSound;
  errorSound: NotificationSound;
  permission: NotificationPermissionStatus;
  busy: boolean;
  refreshPermission: () => Promise<NotificationPermissionStatus>;
  setEnabled: (enabled: boolean) => Promise<void>;
  setSound: (
    kind: NotificationSoundKind,
    sound: NotificationSound,
  ) => Promise<void>;
  openSystemSettings: () => Promise<void>;
  sendTest: () => Promise<boolean>;
};

export function readNotificationsEnabled(): boolean {
  try {
    const raw = localStorage.getItem(ENABLED_KEY);
    if (raw === null) return true;
    return raw === "1";
  } catch {
    return true;
  }
}

function persistNotificationsEnabled(enabled: boolean) {
  try {
    localStorage.setItem(ENABLED_KEY, enabled ? "1" : "0");
  } catch {
    /* ignore */
  }
}

function isNotificationSound(
  value: string | null,
): value is NotificationSound {
  return NOTIFICATION_SOUND_OPTIONS.some((option) => option.value === value);
}

/**
 * Resolve the two stored sounds, falling back to the pre-split setting.
 *
 * Exported for tests; callers normally use `readNotificationSounds`.
 */
export function resolveNotificationSounds(stored: {
  legacy: string | null;
  success: string | null;
  error: string | null;
}): NotificationSounds {
  const legacy = isNotificationSound(stored.legacy)
    ? stored.legacy
    : ("system" as const);
  return {
    success: isNotificationSound(stored.success) ? stored.success : legacy,
    error: isNotificationSound(stored.error) ? stored.error : legacy,
  };
}

export function readNotificationSounds(): NotificationSounds {
  try {
    return resolveNotificationSounds({
      legacy: localStorage.getItem(LEGACY_SOUND_KEY),
      success: localStorage.getItem(SUCCESS_SOUND_KEY),
      error: localStorage.getItem(ERROR_SOUND_KEY),
    });
  } catch {
    return { success: "system", error: "system" };
  }
}

function persistNotificationSound(
  kind: NotificationSoundKind,
  sound: NotificationSound,
) {
  try {
    localStorage.setItem(
      kind === "success" ? SUCCESS_SOUND_KEY : ERROR_SOUND_KEY,
      sound,
    );
  } catch {
    /* ignore */
  }
}

export function isMacPlatform(): boolean {
  return detectDesktopPlatform() === "macos";
}

async function readPermission(): Promise<NotificationPermissionStatus> {
  try {
    return (await isPermissionGranted()) ? "granted" : "denied";
  } catch {
    return "unknown";
  }
}

async function ensureGranted(): Promise<NotificationPermissionStatus> {
  try {
    if (await isPermissionGranted()) return "granted";
    const result = await requestPermission();
    return result === "granted" ? "granted" : "denied";
  } catch {
    return "unknown";
  }
}

async function syncNativeSounds(sounds: NotificationSounds): Promise<void> {
  try {
    await invoke("set_notification_sounds_cmd", {
      successSound: sounds.success,
      errorSound: sounds.error,
    });
  } catch (err) {
    console.warn("Failed to sync notification sounds", err);
  }
}

async function pushSimpleNotification(title: string, body: string) {
  try {
    await invoke("notify_message_cmd", { title, body });
  } catch (err) {
    console.warn("Failed to send notification", err);
  }
}

/**
 * True when the Alfred window is not the frontmost visible window.
 * Prefer Tauri window state — `document.hasFocus()` is unreliable in WKWebView.
 */
export async function shouldNotifyAboutRun(): Promise<boolean> {
  try {
    const win = getCurrentWindow();
    const [focused, visible] = await Promise.all([
      win.isFocused(),
      win.isVisible(),
    ]);
    return !focused || !visible;
  } catch {
    return document.hidden || !document.hasFocus();
  }
}

const storedSounds = readNotificationSounds();

export const useNotificationsStore = create<NotificationsState>((set, get) => ({
  enabled: readNotificationsEnabled(),
  successSound: storedSounds.success,
  errorSound: storedSounds.error,
  permission: "unknown",
  busy: false,

  refreshPermission: async () => {
    const permission = await readPermission();
    set({ permission });
    return permission;
  },

  setEnabled: async (enabled) => {
    set({ busy: true });
    try {
      persistNotificationsEnabled(enabled);
      set({ enabled });

      if (!enabled) return;

      const permission = await ensureGranted();
      set({ permission });
      if (permission === "granted") {
        await pushSimpleNotification(
          "Notifications on",
          "Alfred will notify you when a background run finishes.",
        );
      }
    } finally {
      set({ busy: false });
    }
  },

  setSound: async (kind, sound) => {
    persistNotificationSound(kind, sound);
    if (kind === "success") set({ successSound: sound });
    else set({ errorSound: sound });
    const { successSound, errorSound } = get();
    await syncNativeSounds({ success: successSound, error: errorSound });
  },

  openSystemSettings: async () => {
    if (!isMacPlatform()) return;
    try {
      await openUrl(
        "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
      );
    } catch {
      try {
        await openUrl(
          "x-apple.systempreferences:com.apple.preference.notifications",
        );
      } catch {
        /* ignore */
      }
    }
  },

  sendTest: async () => {
    const { enabled } = get();
    if (!enabled) return false;
    const permission = await ensureGranted();
    set({ permission });
    if (permission !== "granted") return false;
    await pushSimpleNotification(
      "Test notification",
      "Alfred notifications are working.",
    );
    return true;
  },
}));

/**
 * Play the configured cue inside Alfred.
 *
 * Used when the window is focused, where desktop notifications are withheld
 * (they are only sent for background runs).
 */
export function playNotificationSound(kind: NotificationSoundKind): void {
  const { successSound, errorSound } = useNotificationsStore.getState();
  playSoundPreset(kind === "success" ? successSound : errorSound);
}

let lastConditionSkipSoundAt = Number.NEGATIVE_INFINITY;

/**
 * Alert once per burst when a conditional connection does not fire.
 *
 * A skipped branch normally skips several steps in a row; the throttle keeps
 * one branch from sounding like a slot machine. Returns whether a cue played.
 */
export function playConditionSkippedSound(now = Date.now()): boolean {
  if (now - lastConditionSkipSoundAt < CONDITION_SKIP_SOUND_INTERVAL_MS) {
    return false;
  }
  lastConditionSkipSoundAt = now;
  playNotificationSound("error");
  return true;
}

/** Startup: refresh permission; only prompt if the user wants notifications. */
export async function prepareNotifications(): Promise<void> {
  const enabled = readNotificationsEnabled();
  const sounds = readNotificationSounds();
  useNotificationsStore.setState({
    enabled,
    successSound: sounds.success,
    errorSound: sounds.error,
  });
  await syncNativeSounds(sounds);
  if (!enabled) {
    const permission = await readPermission();
    useNotificationsStore.setState({ permission });
    return;
  }
  const permission = await ensureGranted();
  useNotificationsStore.setState({ permission });
}

/** Clickable run notification (Rust) — opens output when the user clicks it. */
export async function notifyRunFinished(input: {
  workflowId: string;
  workflowName: string;
  ok: boolean;
  title: string;
  body: string;
}): Promise<void> {
  if (!readNotificationsEnabled()) return;

  const permission = await ensureGranted();
  useNotificationsStore.setState({ permission });
  if (permission !== "granted") {
    console.warn("Skipping run notification — permission not granted");
    return;
  }

  try {
    await invoke("notify_run_finished_cmd", {
      workflowId: input.workflowId,
      workflowName: input.workflowName,
      ok: input.ok,
      title: input.title,
      body: input.body,
    });
  } catch (err) {
    console.warn("Failed to send run notification", err);
    await pushSimpleNotification(
      input.ok ? "Automation finished" : "Automation failed",
      `${input.workflowName}: ${input.title}`,
    );
  }
}
