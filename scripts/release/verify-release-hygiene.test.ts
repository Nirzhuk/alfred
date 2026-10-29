import { describe, expect, test } from "bun:test";

import {
  collect,
  scanArchitecture,
  scanSecrets,
} from "./verify-release-hygiene";

const file = (path: string, text: string) => ({ path, text });

describe("architecture scan", () => {
  test("flags every obsolete commerce and update reference with its line", () => {
    const violations = scanArchitecture([
      file("docs/releasing.md", "intro\npaid through Stripe today\nend"),
      file("README.md", "assets live in CrabNebula\n"),
      file("docs/install.md", "Alfred ships manual Polar downloads\n"),
    ]);
    expect(violations.map((violation) => `${violation.file}:${violation.line}`)).toEqual([
      "docs/releasing.md:2",
      "README.md:1",
    ]);
  });

  test("passes a clean set of surfaces", () => {
    expect(scanArchitecture([file("README.md", "Polar owns checkout.\n")])).toEqual([]);
  });
});

describe("secret scan", () => {
  test("rejects a Polar server credential anywhere in source", () => {
    const violations = scanSecrets([
      file("src-tauri/src/lib.rs", 'let token = env("POLAR_ACCESS_TOKEN");'),
      file("src/config.ts", "export const polarWebhookSecret = process.env.X;"),
    ]);
    expect(violations).toHaveLength(2);
  });

  test("allows key handling only in the reviewed ephemeral set", () => {
    expect(
      scanSecrets([file("src-tauri/src/licensing/service.rs", "let license_key = key;")]),
    ).toEqual([]);
    expect(
      scanSecrets([file("src/features/history/panel.tsx", "const licenseKey = value;")]),
    ).toEqual([
      {
        file: "src/features/history/panel.tsx",
        line: 1,
        detail:
          "handles a license key outside the reviewed ephemeral/keychain-only set",
      },
    ]);
  });
});

test("collect reads a named file and skips build output", async () => {
  const files = await collect(["package.json"]);
  expect(files.map((entry) => entry.path)).toEqual(["package.json"]);
  expect(files[0]?.text).toContain('"name": "alfred"');
});
