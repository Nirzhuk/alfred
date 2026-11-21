import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

/**
 * Official Alfred builds are published as public GitHub Release assets with a
 * signed `latest.json`. Update checks never install on their own: `findUpdate`
 * quietly reports an available version, and `installUpdate` asks before
 * downloading. The releases page stays the fallback when the check or install
 * cannot run (offline, dev build, or an install that predates the updater).
 */
export const LATEST_RELEASES_URL =
  "https://github.com/Nirzhuk/alfred/releases/latest";

/** Re-probe cadence for a long-running window. */
export const UPDATE_CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000;

export const DOWNLOAD_LATEST_RELEASES_MESSAGE = [
  "Download the latest Alfred",
  "",
  "Alfred could not update itself right now.",
  "",
  "Your browser will open the GitHub releases page, where you can grab the",
  "latest installer for your platform.",
  "",
  "The macOS disk images are signed and notarized. The Windows installer is an",
  "unsigned beta, so Windows reports an unknown publisher and may show a",
  "SmartScreen warning.",
].join("\n");

export const DOWNLOAD_LATEST_FAILED_MESSAGE = [
  "Download the latest Alfred",
  "",
  "Alfred could not open your browser. Visit this page yourself to reach the",
  "latest downloads:",
  "",
  `    ${LATEST_RELEASES_URL}`,
  "",
  "Alternatively, rebuild from source:",
  "",
  "    git pull",
  "    bun install --frozen-lockfile",
  "    bun run build",
  "",
  "See docs/building-from-source.md for the platform prerequisites.",
].join("\n");

export const UP_TO_DATE_MESSAGE = "You're on the latest version of Alfred.";

export function updateAvailableMessage(next: string, current: string): string {
  return [
    `Alfred ${next} is available (you have ${current}).`,
    "",
    "Install it and restart Alfred now?",
  ].join("\n");
}

type ExternalOpener = (url: string) => Promise<void>;

export type DownloadLatestDeps = {
  notify?: (message: string) => void;
  open?: ExternalOpener;
};

export async function openLatestDownload({
  notify = (message) => window.alert(message),
  open = openUrl,
}: DownloadLatestDeps = {}): Promise<void> {
  notify(DOWNLOAD_LATEST_RELEASES_MESSAGE);
  try {
    await open(LATEST_RELEASES_URL);
  } catch {
    notify(DOWNLOAD_LATEST_FAILED_MESSAGE);
  }
}

export type AvailableUpdate = {
  version: string;
  currentVersion: string;
  downloadAndInstall: () => Promise<void>;
};

export type UpdateCheck = () => Promise<AvailableUpdate | null>;

/**
 * Quiet availability probe used at launch and on a timer. It never prompts,
 * installs, or opens the releases page, so a failed check looks like "no
 * update"; `checkForUpdates` is the explicit path that reports failures.
 */
export async function findUpdate(
  checkUpdate: UpdateCheck = check,
): Promise<AvailableUpdate | null> {
  try {
    return await checkUpdate();
  } catch {
    return null;
  }
}

export type InstallUpdateDeps = DownloadLatestDeps & {
  confirm?: (message: string) => boolean;
  restart?: () => Promise<void>;
};

/**
 * Ask, then download the update and restart. Returns true when Alfred
 * restarted into the new version; a failed download falls back to the
 * releases page and returns false.
 */
export async function installUpdate(
  update: AvailableUpdate,
  {
    confirm = (message) => window.confirm(message),
    restart = relaunch,
    ...download
  }: InstallUpdateDeps = {},
): Promise<boolean> {
  if (!confirm(updateAvailableMessage(update.version, update.currentVersion))) {
    return false;
  }
  try {
    // Windows quits into the installer here; macOS and Linux need the restart.
    await update.downloadAndInstall();
    await restart();
    return true;
  } catch {
    await openLatestDownload(download);
    return false;
  }
}

export type CheckForUpdatesDeps = InstallUpdateDeps & {
  checkUpdate?: UpdateCheck;
};

/**
 * Explicit check from the menu, tray, or titlebar. Reports "up to date" or
 * falls back to the releases page, then offers to install a found update.
 * Returns the update either way so the titlebar can keep offering it after a
 * declined install.
 */
export async function checkForUpdates(
  deps: CheckForUpdatesDeps = {},
): Promise<AvailableUpdate | null> {
  const { checkUpdate = check } = deps;
  let update: AvailableUpdate | null;
  try {
    update = await checkUpdate();
  } catch {
    await openLatestDownload(deps);
    return null;
  }
  if (!update) {
    (deps.notify ?? ((message) => window.alert(message)))(UP_TO_DATE_MESSAGE);
    return null;
  }
  await installUpdate(update, deps);
  return update;
}
