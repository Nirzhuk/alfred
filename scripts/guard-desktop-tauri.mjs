#!/usr/bin/env node
/**
 * Desktop-only Tauri wrapper.
 * Alfred ships for macOS, Linux, and Windows — never Android, iOS, or a standalone website.
 */
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const blocked = new Set(["android", "ios"]);
const first = args.find((a) => !a.startsWith("-"));

if (first && blocked.has(first)) {
  console.error(
    "Alfred is desktop-only (macOS, Linux, Windows).\n" +
      "Android/iOS targets are not supported.",
  );
  process.exit(1);
}

if (first === "dev") {
  args.splice(
    args.indexOf(first) + 1,
    0,
    "--config",
    fileURLToPath(new URL("../src-tauri/tauri.dev.conf.json", import.meta.url)),
  );
}

const require = createRequire(import.meta.url);
const tauriCli = path.join(
  path.dirname(require.resolve("@tauri-apps/cli/package.json")),
  "tauri.js",
);

const child = spawn(process.execPath, [tauriCli, ...args], {
  stdio: "inherit",
  env: process.env,
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
