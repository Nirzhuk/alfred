import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

/**
 * Official Alfred builds are published as public GitHub Release assets with a
 * signed `latest.json`. `checkForUpdates` installs from that manifest; the
 * releases page stays the fallback when the check or install cannot run
 * (offline, dev build, or an install that predates the updater).
 */
export const LATEST_RELEASES_URL =
  "https://github.com/Nirzhuk/alfred/releases/latest";

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

type AvailableUpdate = {
  version: string;
  currentVersion: string;
  downloadAndInstall: () => Promise<void>;
};

export type CheckForUpdatesDeps = DownloadLatestDeps & {
  /** Startup check: stay quiet unless an update is actually available. */
  silent?: boolean;
  confirm?: (message: string) => boolean;
  checkUpdate?: () => Promise<AvailableUpdate | null>;
  restart?: () => Promise<void>;
};

export async function checkForUpdates({
  silent = false,
  confirm = (message) => window.confirm(message),
  checkUpdate = check,
  restart = relaunch,
  ...download
}: CheckForUpdatesDeps = {}): Promise<void> {
  const notify = download.notify ?? ((message) => window.alert(message));
  let update: AvailableUpdate | null;
  try {
    update = await checkUpdate();
  } catch {
    if (!silent) await openLatestDownload(download);
    return;
  }
  if (!update) {
    if (!silent) notify(UP_TO_DATE_MESSAGE);
    return;
  }
  if (!confirm(updateAvailableMessage(update.version, update.currentVersion))) {
    return;
  }
  try {
    // Windows quits into the installer here; macOS and Linux need the restart.
    await update.downloadAndInstall();
    await restart();
  } catch {
    await openLatestDownload(download);
  }
}
