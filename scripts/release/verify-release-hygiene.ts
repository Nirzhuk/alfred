/**
 * Plan 005 repeatable release-hygiene scans.
 *
 * Runs the architecture and secret scans the acceptance matrix
 * requires and exits non-zero on any violation. Every check is a pure function
 * over file contents so it can be unit tested without touching the repository.
 */
import { readdir } from "node:fs/promises";
import { extname, join, relative } from "node:path";

export type Violation = { file: string; line: number; detail: string };
export type SourceFile = { path: string; text: string };

const REPOSITORY_ROOT = join(import.meta.dir, "..", "..");

/** Obsolete commerce/update architecture that must not survive in shipped surfaces. */
const OBSOLETE_ARCHITECTURE =
  /Stripe|stripe|CrabNebula|license-server|authenticated updater/;

/** A Polar server credential must never appear in application source. */
const POLAR_SECRET = /POLAR_ACCESS_TOKEN|polar[\w .-]{0,40}secret/i;

/** Identifiers that carry a raw customer key through memory. */
const LICENSE_KEY_HANDLING = /licenseKey|license_key/;

/**
 * Files allowed to name a license key at all. Key material stays ephemeral in
 * these and reaches disk only through the OS credential store, so a new entry
 * here is a deliberate review decision, not a rename.
 */
const KEY_HANDLING_ALLOW_LIST = new Set([
  "src-tauri/src/commands/mod.rs",
  "src-tauri/src/db/license.rs",
  "src-tauri/src/db/migrate.rs",
  "src-tauri/src/licensing/client.rs",
  "src-tauri/src/licensing/mod.rs",
  "src-tauri/src/licensing/service.rs",
  "src-tauri/src/licensing/store.rs",
  "src/features/licensing/api.test.ts",
  "src/features/licensing/api.ts",
  "src/features/licensing/components/license-settings/license-settings.tsx",
  "src/features/licensing/store.ts",
]);

function findLines(
  files: SourceFile[],
  pattern: RegExp,
  detail: (match: string, file: SourceFile) => string,
): Violation[] {
  const violations: Violation[] = [];
  for (const file of files) {
    file.text.split("\n").forEach((text, index) => {
      const match = text.match(pattern);
      if (match) {
        violations.push({
          file: file.path,
          line: index + 1,
          detail: detail(match[0], file),
        });
      }
    });
  }
  return violations;
}

/** No shipped surface may still describe the pre-Polar commerce architecture. */
export function scanArchitecture(files: SourceFile[]): Violation[] {
  return findLines(
    files,
    OBSOLETE_ARCHITECTURE,
    (match) => `obsolete architecture reference "${match}"`,
  );
}

/**
 * No Polar server secret ships, and only reviewed files touch key material.
 */
export function scanSecrets(files: SourceFile[]): Violation[] {
  const violations = findLines(
    files,
    POLAR_SECRET,
    (match) => `Polar secret "${match}" must never ship in the app`,
  );
  for (const file of files) {
    if (
      LICENSE_KEY_HANDLING.test(file.text) &&
      !KEY_HANDLING_ALLOW_LIST.has(file.path)
    ) {
      violations.push({
        file: file.path,
        line: 1,
        detail:
          "handles a license key outside the reviewed ephemeral/keychain-only set",
      });
    }
  }
  return violations;
}

const IGNORED_DIRECTORIES = new Set(["node_modules", "target", "dist", ".git"]);
const TEXT_EXTENSIONS = new Set([
  ".rs", ".ts", ".tsx", ".js", ".mjs", ".json", ".md",
  ".yml", ".yaml", ".toml", ".css", ".html", ".sql", ".sh",
]);

async function walk(absolute: string, into: string[]): Promise<void> {
  for (const entry of await readdir(absolute, { withFileTypes: true })) {
    if (IGNORED_DIRECTORIES.has(entry.name)) continue;
    const path = join(absolute, entry.name);
    if (entry.isDirectory()) {
      await walk(path, into);
    } else if (TEXT_EXTENSIONS.has(extname(entry.name))) {
      into.push(path);
    }
  }
}

export async function collect(roots: string[]): Promise<SourceFile[]> {
  const files: SourceFile[] = [];
  const seen = new Set<string>();
  for (const root of roots) {
    const absolute = join(REPOSITORY_ROOT, root);
    const paths: string[] = [];
    if (await Bun.file(absolute).exists()) {
      paths.push(absolute);
    } else {
      await walk(absolute, paths);
    }
    for (const path of paths) {
      const relativePath = relative(REPOSITORY_ROOT, path);
      if (seen.has(relativePath)) continue;
      seen.add(relativePath);
      files.push({ path: relativePath, text: await Bun.file(path).text() });
    }
  }
  return files.sort((left, right) => left.path.localeCompare(right.path));
}

const CHECKS = [
  {
    name: "architecture-scan",
    roots: ["README.md", "docs", "src", "src-tauri"],
    scan: scanArchitecture,
  },
  {
    name: "secret-scan",
    roots: ["src", "src-tauri/src"],
    scan: scanSecrets,
  },
];

export async function verifyReleaseHygiene(): Promise<boolean> {
  let passed = true;
  for (const check of CHECKS) {
    const violations = check.scan(await collect(check.roots));
    console.log(`${violations.length === 0 ? "PASS" : "FAIL"} ${check.name}`);
    for (const violation of violations) {
      console.log(`  ${violation.file}:${violation.line} ${violation.detail}`);
    }
    passed &&= violations.length === 0;
  }
  return passed;
}

if (import.meta.main) {
  process.exitCode = (await verifyReleaseHygiene()) ? 0 : 1;
}
