import { describe, expect, test } from "bun:test";
import {
  DOWNLOAD_LATEST_FAILED_MESSAGE,
  DOWNLOAD_LATEST_RELEASES_MESSAGE,
  LATEST_RELEASES_URL,
  UP_TO_DATE_MESSAGE,
  checkForUpdates,
  findUpdate,
  installUpdate,
  openLatestDownload,
  updateAvailableMessage,
  type AvailableUpdate,
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

type HarnessOptions = {
  update?: { version: string; currentVersion: string } | null;
  checkFails?: boolean;
  installFails?: boolean;
  accept?: boolean;
};

function updateHarness({
  update = null,
  checkFails = false,
  installFails = false,
  accept = true,
}: HarnessOptions = {}) {
  const calls: string[] = [];
  const notified: string[] = [];
  const confirmed: string[] = [];
  const available: AvailableUpdate | null = update
    ? {
        ...update,
        downloadAndInstall: async () => {
          calls.push("install");
          if (installFails) throw new Error("bad signature");
        },
      }
    : null;
  const deps = {
    notify: (message: string) => notified.push(message),
    open: async (url: string) => {
      calls.push(`open ${url}`);
    },
    confirm: (message: string) => {
      confirmed.push(message);
      return accept;
    },
    checkUpdate: async (): Promise<AvailableUpdate | null> => {
      if (checkFails) throw new Error("offline");
      return available;
    },
    restart: async () => {
      calls.push("restart");
    },
  };
  return { calls, notified, confirmed, available, deps };
}

const available = { version: "1.2.0", currentVersion: "1.1.0" };

describe("quiet update probe", () => {
  test("reports a newer version without prompting or installing", async () => {
    const ctx = updateHarness({ update: available });

    await expect(findUpdate(ctx.deps.checkUpdate)).resolves.toMatchObject(
      available,
    );
    expect(ctx.confirmed).toEqual([]);
    expect(ctx.calls).toEqual([]);
    expect(ctx.notified).toEqual([]);
  });

  test("returns nothing when the app is current", async () => {
    const ctx = updateHarness();
    await expect(findUpdate(ctx.deps.checkUpdate)).resolves.toBeNull();
    expect(ctx.notified).toEqual([]);
  });

  test("swallows an offline check so the launch stays quiet", async () => {
    const ctx = updateHarness({ checkFails: true });
    await expect(findUpdate(ctx.deps.checkUpdate)).resolves.toBeNull();
    expect(ctx.calls).toEqual([]);
    expect(ctx.notified).toEqual([]);
  });
});

describe("install an available update", () => {
  test("installs and restarts after the user accepts", async () => {
    const ctx = updateHarness({ update: available });
    await expect(installUpdate(ctx.available!, ctx.deps)).resolves.toBe(true);

    expect(ctx.confirmed).toEqual([updateAvailableMessage("1.2.0", "1.1.0")]);
    expect(ctx.calls).toEqual(["install", "restart"]);
  });

  test("does nothing when the user declines", async () => {
    const ctx = updateHarness({ update: available, accept: false });
    await expect(installUpdate(ctx.available!, ctx.deps)).resolves.toBe(false);
    expect(ctx.calls).toEqual([]);
  });

  test("falls back to the releases page without restarting when install fails", async () => {
    const ctx = updateHarness({ update: available, installFails: true });
    await expect(installUpdate(ctx.available!, ctx.deps)).resolves.toBe(false);
    expect(ctx.calls).toEqual(["install", `open ${LATEST_RELEASES_URL}`]);
  });
});

describe("check for updates", () => {
  test("reports up to date from an explicit check", async () => {
    const ctx = updateHarness();
    await expect(checkForUpdates(ctx.deps)).resolves.toBeNull();
    expect(ctx.notified).toEqual([UP_TO_DATE_MESSAGE]);
  });

  test("offers and installs a found update", async () => {
    const ctx = updateHarness({ update: available });
    await expect(checkForUpdates(ctx.deps)).resolves.toMatchObject(available);

    expect(ctx.confirmed).toEqual([updateAvailableMessage("1.2.0", "1.1.0")]);
    expect(ctx.calls).toEqual(["install", "restart"]);
  });

  test("returns the found update when the user declines so the titlebar can keep it", async () => {
    const ctx = updateHarness({ update: available, accept: false });
    await expect(checkForUpdates(ctx.deps)).resolves.toMatchObject(available);
    expect(ctx.calls).toEqual([]);
  });

  test("a failed explicit check falls back to the releases page", async () => {
    const ctx = updateHarness({ checkFails: true });
    await expect(checkForUpdates(ctx.deps)).resolves.toBeNull();
    expect(ctx.calls).toEqual([`open ${LATEST_RELEASES_URL}`]);
  });
});
