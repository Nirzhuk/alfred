import { describe, expect, test } from "bun:test";
import {
  DOWNLOAD_LATEST_FAILED_MESSAGE,
  DOWNLOAD_LATEST_RELEASES_MESSAGE,
  LATEST_RELEASES_URL,
  UP_TO_DATE_MESSAGE,
  checkForUpdates,
  openLatestDownload,
  updateAvailableMessage,
} from "./download-latest";

function collect(opener?: (url: string) => Promise<void>) {
  const opened: string[] = [];
  const notified: string[] = [];
  const run = () =>
    openLatestDownload({
      notify: (message) => notified.push(message),
      open: opener ??
        (async (url) => {
          opened.push(url);
        }),
    });
  return { opened, notified, run };
}

describe("download latest version action", () => {
  test("opens only the public GitHub releases page", async () => {
    const ctx = collect();
    await ctx.run();

    expect(ctx.opened).toEqual([LATEST_RELEASES_URL]);
    expect(ctx.notified).toEqual([DOWNLOAD_LATEST_RELEASES_MESSAGE]);
  });

  test("the fallback message explains the manual download path", () => {
    expect(DOWNLOAD_LATEST_RELEASES_MESSAGE).toContain(
      "Alfred could not update itself right now.",
    );
    expect(DOWNLOAD_LATEST_RELEASES_MESSAGE).toContain("unsigned beta");
  });

  test("the releases URL is a GitHub releases page allowed by the opener capability", () => {
    expect(LATEST_RELEASES_URL).toMatch(
      /^https:\/\/github\.com\/Nirzhuk\/alfred\/releases/,
    );
  });
  test("a browser failure explains the manual path and the source rebuild", async () => {
    const ctx = collect(async (url) => {
      ctx.opened.push(url);
      throw new Error("no browser");
    });
    await ctx.run();

    expect(ctx.notified).toEqual([
      DOWNLOAD_LATEST_RELEASES_MESSAGE,
      DOWNLOAD_LATEST_FAILED_MESSAGE,
    ]);
    expect(DOWNLOAD_LATEST_FAILED_MESSAGE).toContain(LATEST_RELEASES_URL);
    expect(DOWNLOAD_LATEST_FAILED_MESSAGE).toContain("bun run build");
  });
});

function updateHarness({
  update = null as { version: string; currentVersion: string } | null,
  checkFails = false,
  installFails = false,
  accept = true,
  silent = false,
} = {}) {
  const calls: string[] = [];
  const notified: string[] = [];
  const confirmed: string[] = [];
  const run = () =>
    checkForUpdates({
      silent,
      notify: (message) => notified.push(message),
      open: async (url) => {
        calls.push(`open ${url}`);
      },
      confirm: (message) => {
        confirmed.push(message);
        return accept;
      },
      checkUpdate: async () => {
        if (checkFails) throw new Error("offline");
        return update
          ? {
              ...update,
              downloadAndInstall: async () => {
                calls.push("install");
                if (installFails) throw new Error("bad signature");
              },
            }
          : null;
      },
      restart: async () => {
        calls.push("restart");
      },
    });
  return { calls, notified, confirmed, run };
}

describe("check for updates", () => {
  const available = { version: "1.2.0", currentVersion: "1.1.0" };

  test("installs and restarts after the user accepts", async () => {
    const ctx = updateHarness({ update: available });
    await ctx.run();
    expect(ctx.confirmed).toEqual([updateAvailableMessage("1.2.0", "1.1.0")]);
    expect(ctx.calls).toEqual(["install", "restart"]);
  });

  test("does nothing when the user declines", async () => {
    const ctx = updateHarness({ update: available, accept: false });
    await ctx.run();
    expect(ctx.calls).toEqual([]);
  });

  test("reports up to date only when asked from the menu", async () => {
    const manual = updateHarness();
    await manual.run();
    expect(manual.notified).toEqual([UP_TO_DATE_MESSAGE]);

    const startup = updateHarness({ silent: true });
    await startup.run();
    expect(startup.notified).toEqual([]);
  });

  test("a failed check falls back to the releases page, but never at startup", async () => {
    const manual = updateHarness({ checkFails: true });
    await manual.run();
    expect(manual.calls).toEqual([`open ${LATEST_RELEASES_URL}`]);

    const startup = updateHarness({ checkFails: true, silent: true });
    await startup.run();
    expect(startup.calls).toEqual([]);
    expect(startup.notified).toEqual([]);
  });

  test("a failed install falls back to the releases page without restarting", async () => {
    const ctx = updateHarness({ update: available, installFails: true });
    await ctx.run();
    expect(ctx.calls).toEqual(["install", `open ${LATEST_RELEASES_URL}`]);
  });
});
