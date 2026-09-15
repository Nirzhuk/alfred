import { describe, expect, test } from "bun:test";

const root = new URL("../", import.meta.url);

// The design system is one system served from four files, split so a second
// window can take the tokens and the base without parsing the main window's
// component rules. Every rule below still applies to all of them, so they are
// read together and scanned together.
const CSS_FILES = [
  "src/styles/tokens.css",
  "src/styles/base.css",
  "src/App.css",
  "src/features/quick-access/quick-access.css",
] as const;

const cssSources = await Promise.all(
  CSS_FILES.map(async (path) => ({
    path,
    text: await Bun.file(new URL(path, root)).text(),
  })),
);
const cssByPath = new Map(cssSources.map(({ path, text }) => [path, text]));
// Prettier reflows long selector lists and declaration values across several
// lines. Every contract below is about what a rule says, not where it wraps, so
// each logical line is rejoined before matching — otherwise a pure reformat
// fails a rule that is completely intact. The per-file scans that report line
// numbers read `cssSources` directly and are unaffected.
function unwrap(text: string): string {
  const lines: string[] = [];
  let buffer = "";
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    buffer = buffer === "" ? line : `${buffer} ${trimmed}`;
    if (trimmed === "" || /[{};]$/.test(trimmed) || trimmed.endsWith("*/")) {
      lines.push(buffer);
      buffer = "";
    }
  }
  if (buffer !== "") lines.push(buffer);
  return lines.join("\n");
}

const css = unwrap(cssSources.map(({ text }) => text).join("\n"));

const designSystem = await Bun.file(new URL("docs/design-system.md", root)).text();
const specs = await Bun.file(new URL("specs.md", root)).text();
const claudeRule = await Bun.file(
  new URL(".claude/rules/design-system.md", root),
).text();
const cursorRule = await Bun.file(
  new URL(".cursor/rules/design-system.mdc", root),
).text();
const selectControl = await Bun.file(
  new URL("src/components/select-control/select-control.tsx", root),
).text();
const quickAccessPopover = await Bun.file(
  new URL("src/features/quick-access/quick-access-popover.tsx", root),
).text();
const settingsPage = await Bun.file(
  new URL(
    "src/features/settings/components/settings-page/settings-page.tsx",
    root,
  ),
).text();
const flowEditor = await Bun.file(
  new URL(
    "src/features/workflow/components/flow-editor/flow-editor.tsx",
    root,
  ),
).text();

function cssBlock(selector: string): string {
  // Callers may pass a wrapped selector list; match it the way `unwrap` joins.
  const marker = `${selector.replace(/\s*\n\s*/g, " ")} {`;
  // A selector opens a rule when it starts a line or follows a comma in a
  // selector list. It must never match as the tail of a longer compound
  // selector (`.a .sidebar-nav {` is not `.sidebar-nav {`), which is why this
  // is anchored rather than a bare indexOf.
  const escaped = marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const found = new RegExp(`(?:^|\\n|, )${escaped}`).exec(css);
  const start = found ? found.index + found[0].length - marker.length : -1;
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
}

describe("design-system foundations", () => {
  test("prefers Infer and bundles its Geist fallbacks without a runtime font request", async () => {
    expect(css).not.toContain("@import url(");
    // The split introduced @import. Keep every one of them a relative, local,
    // build-time import — nothing that resolves over the network at runtime.
    for (const { path, text } of cssSources) {
      for (const [, target] of text.matchAll(/@import\s+"([^"]+)"/g)) {
        expect([path, target, target.startsWith(".")]).toEqual([
          path,
          target,
          true,
        ]);
      }
    }
    expect(css).toContain('font-family: "Geist";');
    expect(css).toContain('font-family: "Geist Mono";');
    expect(css).toContain('--font-sans: "Infer", "Geist"');
    expect(css).toContain('--font-display: var(--font-sans)');
    expect(css).toContain('--font-mono: "Geist Mono"');

    for (const path of [
      "src/assets/fonts/geist-variable.woff2",
      "src/assets/fonts/geist-mono-variable.woff2",
    ]) {
      const font = Bun.file(new URL(path, root));
      expect(await font.exists()).toBe(true);
      expect(font.size).toBeGreaterThan(10_000);
    }

    for (const path of ["src/assets/fonts/GEIST-LICENSE.txt"]) {
      const license = await Bun.file(new URL(path, root)).text();
      expect(license).toContain("SIL Open Font License");
      expect(license).not.toContain("404: Not Found");
    }
  });

  test("defines the canonical type, spacing, shape, icon, and motion scales", () => {
    for (const declaration of [
      "--text-xs: 11px;",
      "--text-sm: 12px;",
      "--text-md: 14px;",
      "--text-lg: 16px;",
      "--text-xl: 20px;",
      "--text-2xl: 24px;",
      "--space-1: 4px;",
      "--space-2: 8px;",
      "--space-4: 16px;",
      "--radius-md: 8px;",
      "--radius-lg: 12px;",
      "--icon-size-default: 18px;",
      "--duration-fast: 120ms;",
      "--duration-standard: 180ms;",
      "--layer-popover: 50;",
    ]) {
      expect(css).toContain(declaration);
    }
  });

  test("locks the release palette to neutral surfaces and one emerald accent", () => {
    const dark = cssBlock('[data-theme="dark"]');
    for (const declaration of [
      "--bg: #101010;",
      "--surface-card: #202020;",
      "--surface-raised: #262626;",
      "--ink: #fcfcfc;",
      "--muted: #aaaaaa;",
      "--icon: #fcfcfc;",
      "--icon-disabled: #8e8f8d;",
      "--accent: #38c99b;",
      "--btn-primary: #137a5f;",
      "--btn-primary-hover: #168266;",
    ]) {
      expect(dark).toContain(declaration);
    }

    for (const role of [
      "--node-accent-input",
      "--node-accent-agent",
      "--node-accent-custom-agent",
      "--node-accent-app-action",
      "--node-accent-output",
      "--node-accent-memory",
      "--node-accent-template",
      "--node-accent-file-inject",
      "--node-accent-git-status",
      "--node-accent-shell",
      "--node-accent-script",
      "--node-accent-http",
      "--node-accent-notify",
      "--node-accent-write-file",
      "--node-accent-git-host",
    ]) {
      expect(dark).toMatch(new RegExp(`${role}: #[0-9a-f]{6};`));
    }
  });

  test("uses a dot-only workflow canvas pattern", () => {
    const backgrounds = flowEditor.match(/<Background\b[\s\S]*?\/>/g) ?? [];
    expect(backgrounds.length).toBeGreaterThan(0);
    for (const background of backgrounds) {
      expect(background).toContain("variant={BackgroundVariant.Dots}");
    }
    expect(flowEditor).not.toContain("variant={BackgroundVariant.Lines}");
    expect(css).not.toContain("--canvas-guide:");
  });

  test("does not allow fractional font-weight drift", () => {
    expect(css).not.toMatch(/font-weight:\s*(?:550|560|620|650|750)\b/);
    expect(css).not.toContain("var(--fg)");

    const allowedFamilies = new Set([
      '"Geist"',
      '"Geist Mono"',
      "var(--font-sans)",
      "var(--font-display)",
      "var(--font-mono)",
      "inherit",
    ]);
    const families = [...css.matchAll(/^\s*font-family:\s*([^;]+);/gm)].map(
      ([, family]) => family,
    );
    expect(families.length).toBeGreaterThan(2);
    for (const family of families) expect(allowedFamilies.has(family)).toBe(true);
  });

  test("routes shape and stacking decisions through tokens", () => {
    expect(css).not.toMatch(/^\s*z-index:\s*\d+/m);
    expect(css).not.toMatch(
      /^\s*border-radius:\s*(?:[1-9]\d*px|0?\.\d+rem|50%)/m,
    );
  });

  test("routes type and spacing decisions through tokens", () => {
    // A literal here is drift, not a decision: values like 0.72rem (11.5px)
    // and 0.45rem (7.2px) sit between steps, so the UI grows a private scale
    // that no token describes. Unique geometry stays literal but must say so
    // with a `geometry:` comment on the line above.
    const offenders = (label: string, pattern: RegExp) =>
      cssSources.flatMap(({ path, text }) => {
        const lines = text.split("\n");
        return lines
          .map((line, index) => ({ line, index }))
          .filter(
            ({ line, index }) =>
              pattern.test(line) && !/geometry:/.test(lines[index - 1] ?? ""),
          )
          .map(({ line, index }) => `${label} ${path}:${index + 1}: ${line.trim()}`);
      });

    const found = [
      ...offenders("type", /^\s*font-size:\s*[0-9.]+(rem|px|em)/),
      // The `font:` shorthand sets size and family at once, so it slips past a
      // font-size check while doing the same damage. Only `font: inherit`.
      ...offenders("type", /^\s*font:(?!\s*inherit\s*;)/),
      ...offenders(
        "spacing",
        /^\s*(padding|padding-\w+|margin|margin-\w+|gap|row-gap|column-gap):\s*[^;]*(?<![-\w.])[0-9.]+(rem|px)/,
      ),
    ];
    expect(found).toEqual([]);
  });

  test("fills come from the surface role scale, not a hand-picked color", () => {
    // Fills are chosen by what an element sits ON. A raw color here is how a
    // control ends up brighter than its own card: the value looks fine alone
    // and wrong the moment it is stacked.
    const roles = [
      "--surface-panel",
      "--surface-inset",
      "--surface-card",
      "--surface-raised",
      "--surface",
    ];
    for (const role of roles) {
      expect(cssBlock(":root")).toContain(`${role}: `);
    }

    // One definition per role per theme — the sprawl this scale replaced had
    // three near-identical alphas all meaning "raised".
    for (const role of roles) {
      const defined = [...css.matchAll(new RegExp(`^\\s*${role}: `, "gm"))];
      expect([role, defined.length]).toEqual([role, 2]);
    }

    // Retired names must not come back.
    for (const dead of [
      "--surface-soft",
      "--surface-strong",
      "--surface-glass",
      "--surface-faint",
      "--surface-muted",
      "--panel-solid",
      "--panel-2",
    ]) {
      expect([dead, css.includes(dead)]).toEqual([dead, false]);
    }

    // Raw color fills outside the token blocks. A fill that genuinely must not
    // follow the theme (a scannable QR plate, an always-white logo tile) says
    // so with a `theme-exempt:` comment, the same escape hatch as `geometry:`.
    const rawFills = cssSources.flatMap(({ path, text }) => {
      const lines = text.split("\n");
      return lines
        .map((line, index) => ({ line, index }))
        .filter(
          ({ line, index }) =>
            /^\s*background(-color)?:\s*(#|rgba?\()/.test(line) &&
            !/theme-exempt:/.test(
              lines.slice(Math.max(0, index - 4), index).join("\n"),
            ),
        )
        .map(({ line, index }) => `${path}:${index + 1}: ${line.trim()}`);
    });
    expect(rawFills).toEqual([]);
  });
});

describe("shared component contracts", () => {
  test("keeps sidebar navigation on the shared scale", () => {
    for (const declaration of [
      "--sidebar-item-font-size: var(--text-md);",
      "--sidebar-item-font-weight: var(--font-weight-regular);",
      "--sidebar-item-color: color-mix(in srgb, var(--ink) 60%, var(--muted));",
      "--sidebar-icon-size: var(--icon-size-compact);",
      "--sidebar-section-font-size: var(--text-lg);",
      "--sidebar-section-font-weight: var(--font-weight-regular);",
      "--sidebar-item-min-height: var(--control-height-default);",
      "--sidebar-item-stack-gap: 2px;",
    ]) {
      expect(css).toContain(declaration);
    }

    // Navigation stays quiet: item ink is softened off full --ink.
    expect(css).not.toContain("--sidebar-item-color: var(--ink);");
    expect(css).toContain("--sidebar-section-color: color-mix(");

    for (const selector of [".sidebar-nav-item", ".settings-sidebar-item"]) {
      const block = cssBlock(selector);
      expect(block).toContain("font-size: var(--sidebar-item-font-size)");
      expect(block).toContain("font-weight: var(--sidebar-item-font-weight)");
      expect(block).toContain("gap: var(--sidebar-item-gap)");
    }

    expect(cssBlock(".sidebar-nav")).toContain(
      "gap: var(--sidebar-item-stack-gap)",
    );
    expect(cssBlock(".sidebar-nav-item")).toContain(
      "border: 1px solid transparent",
    );
    const activeNav = cssBlock(".sidebar-nav-item.is-active");
    expect(activeNav).toContain("border-color: var(--line)");
    expect(activeNav).not.toContain("box-shadow");
    expect(css).toContain(
      ".settings-sidebar-group-items {\n  gap: var(--sidebar-item-stack-gap);\n}",
    );

    for (const selector of [
      ".sidebar-header h2",
      ".settings-sidebar-heading h2",
      ".settings-sidebar-group h3",
    ]) {
      const block = cssBlock(selector);
      expect(block).toContain("font-size: var(--sidebar-section-font-size)");
      expect(block).toContain("font-weight: var(--sidebar-section-font-weight)");
    }
  });

  test("names the scrolled folder in the chrome instead of banding the list", () => {
    // The folder row stays in the flow. A pinned row has to paint an occluding
    // fill over the rows sliding beneath it — no fill, blur alone, or a gradient
    // all let that text read through the folder name — and that band is the one
    // thing this sidebar must not grow. The folder being read is named in the
    // header chrome above the scroller instead, where nothing scrolls behind it.
    const header = cssBlock(".workflow-folder-header");
    expect(header).not.toContain("position: sticky");
    expect(header).not.toContain("background");
    expect(header).not.toContain("z-index");
    expect(css).not.toContain("workflow-folder-sentinel");
    expect(css).not.toContain(".workflow-folder-header.is-pinned");

    // The context reads quieter than the section title it follows: item scale,
    // section color, and no weight change.
    const context = cssBlock(".sidebar-header-context-label");
    expect(context).toContain("font-size: var(--sidebar-item-font-size)");
    expect(context).toContain("font-weight: var(--sidebar-item-font-weight)");
    expect(context).toContain("color: var(--sidebar-section-color)");
    expect(cssBlock(".sidebar-header-title")).toContain("min-width: 0");

    // Enter, leave, and swap: both labels share one grid cell so an overlapping
    // swap cannot move the header, and both directions travel the same path —
    // a folder that rose into place leaves back down the way it came. Transform
    // and opacity only, one strong ease-out token, both sides under 300ms.
    expect(cssBlock(".sidebar-header-context")).toContain(
      "display: inline-grid",
    );
    expect(context).toContain("grid-area: 1 / 1");
    expect(context).toContain(
      "animation: sidebar-context-in var(--duration-standard) var(--ease-emphasized)",
    );
    expect(cssBlock(".sidebar-header-context-label.is-leaving")).toContain(
      "animation: sidebar-context-out var(--duration-fast) var(--ease-emphasized)",
    );
    for (const frames of ["sidebar-context-in", "sidebar-context-out"]) {
      const block = cssBlock(`@keyframes ${frames}`);
      expect(block).toContain("opacity: 0");
      expect(block).toContain("transform: translateY(var(--space-1))");
    }
    // Reduced motion keeps the crossfade that makes the swap legible and drops
    // the travel — gentler, not absent.
    for (const frames of [
      "sidebar-context-fade-in",
      "sidebar-context-fade-out",
    ]) {
      const block = cssBlock(`@keyframes ${frames}`);
      expect(block).toContain("opacity: 0");
      expect(block).not.toContain("transform");
    }
    expect(css).toContain(
      "  .sidebar-header-context-label {\n    animation-name: sidebar-context-fade-in;",
    );
    expect(css).toContain(
      "  .sidebar-header-context-label.is-leaving {\n    animation-name: sidebar-context-fade-out;",
    );

    // Off macOS the panel is opaque, so rows take container fills, ordered:
    // rest below hover, selection never dimmer than hover.
    expect(cssBlock(".workflow-card-button")).toContain(
      "background: var(--surface-raised)",
    );
    for (const selector of [
      ".workflow-card-button:hover",
      ".workflow-card.is-active .workflow-card-button",
      ".workflow-drag-ghost .workflow-card-button",
    ]) {
      expect(cssBlock(selector)).toContain("background: var(--surface)");
    }

    // On macOS the sidebar is a translucent material, so an opaque card fill
    // punches the vibrancy out of its own rectangle. Rows there take the same
    // quiet ink scale the nav items use, a few steps stronger — and the rule is
    // scoped to .sidebar so the drag ghost over the canvas stays opaque.
    expect(
      cssBlock('html[data-platform="macos"] .sidebar .workflow-card-button'),
    ).toContain("background: var(--surface-selected)");
    expect(
      cssBlock(
        'html[data-platform="macos"] .sidebar .workflow-card-button:hover,\nhtml[data-platform="macos"] .sidebar .workflow-card.is-active .workflow-card-button',
      ),
    ).toContain("background: var(--surface-pressed)");
  });

  test("quiet hover, pressed, and selected states share one neutral scale", () => {
    // The scale itself: transparent (tints the panel beneath rather than
    // painting an opaque tile) and mixed against --ink so it inverts by theme.
    for (const token of [
      "--surface-hover",
      "--surface-selected",
      "--surface-pressed",
    ]) {
      expect(css).toContain(`${token}: color-mix(in srgb, var(--ink)`);
      expect(cssBlock(":root")).toContain(`${token}: color-mix`);
    }

    // Every borderless row, nav item, ghost icon button, and list row consumes
    // the scale. A feature-local wash here is the drift this test exists to
    // catch: it looks fine on one panel and invisible or heavy on the next.
    for (const selector of [
      ".sidebar-nav-item:hover",
      ".sidebar-nav-item.is-active",
      ".settings-sidebar-item:hover:not(:disabled)",
      ".settings-sidebar-item.is-active",
      ".settings-sidebar-back:hover:not(:disabled)",
      ".workflow-folder-options:hover:not(:disabled)",
      ".integration-provider-row:hover",
      ".ui-menu-item:hover",
      ".theme-switch-option:hover:not(:disabled)",
      ".skill-picker-row:hover",
      ".memory-picker-row:hover",
    ]) {
      const block = cssBlock(selector);
      expect(block).toMatch(
        /background: var\(--surface-(hover|selected|pressed)\)/,
      );
      // Hover is never an accent tint; accent marks a real selection.
      expect(block).not.toContain("background: var(--accent");
    }

    // Settings selection is communicated by its neutral fill, without a
    // decorative leading rule competing with the content hierarchy.
    expect(cssBlock(".settings-sidebar-item.is-active")).not.toContain(
      "box-shadow",
    );
  });

  test("menus, controls, forms, and modal shells consume semantic tokens", () => {
    expect(cssBlock("button")).toContain("border-radius: var(--radius-md)");
    expect(cssBlock(".ui-menu")).toContain("border-radius: var(--radius-lg)");
    expect(cssBlock(".ui-menu-item")).toContain("font-size: var(--text-md)");
    expect(cssBlock(".settings-card")).toContain("border-radius: var(--radius-lg)");
    expect(css).toContain("border-radius: var(--radius-xl);");
    expect(css).toContain("box-shadow: var(--elevation-modal);");
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
    expect(css).toContain('@media (prefers-reduced-transparency: reduce)');
    expect(css).toContain("backdrop-filter: blur(32px)");
  });

  test("routes styled select fields through the shared native control", () => {
    expect(selectControl).toContain("SelectHTMLAttributes<HTMLSelectElement>");
    expect(selectControl).toContain("<select");
    expect(selectControl).toContain('className="ui-select-chevron"');
    expect(cssBlock(".ui-select-input")).toContain(
      "height: var(--control-height-comfortable)",
    );
    expect(cssBlock(".ui-select.is-compact .ui-select-input")).toContain(
      "height: var(--control-height-default)",
    );

    for (const feature of [quickAccessPopover, settingsPage]) {
      expect(feature).toContain("<SelectControl");
      expect(feature).not.toContain("<select");
    }
  });
});

describe("stylesheet split", () => {
  const fileText = (path: (typeof CSS_FILES)[number]) =>
    cssByPath.get(path) as string;

  test("keeps the popover off the main window's stylesheet", () => {
    // Quick Access runs in its own webview process. Importing App.css made it
    // parse the entire design system to render about thirty class names, and
    // that process is now built lazily precisely so it costs nothing when the
    // feature is off — loading the whole sheet would undo half of that.
    expect(quickAccessPopover).toContain('import "./quick-access.css"');
    expect(quickAccessPopover).not.toContain("App.css");

    const popoverCss = fileText("src/features/quick-access/quick-access.css");
    expect(popoverCss).toContain('@import "../../styles/tokens.css"');
    expect(popoverCss).toContain('@import "../../styles/base.css"');
    expect(popoverCss).toContain(".quick-access-shell {");
  });

  test("gives the tokens one home that every window imports", () => {
    // A second `:root` block is how two windows quietly drift apart.
    const tokens = fileText("src/styles/tokens.css");
    expect(tokens).toContain(":root {");
    expect(tokens).toContain('[data-theme="dark"] {');

    const app = fileText("src/App.css");
    expect(app).toContain('@import "./styles/tokens.css"');
    expect(app).toContain('@import "./styles/base.css"');
    for (const path of CSS_FILES) {
      if (path === "src/styles/tokens.css") continue;
      expect([path, fileText(path).includes(":root {")]).toEqual([path, false]);
    }

    // The base layer resolves tokens; it must never define them.
    expect(fileText("src/styles/base.css")).not.toMatch(/^\s*--[a-z-]+:/m);
  });
});

describe("design-system governance", () => {
  test("keeps product specs and coding-agent rules pointed at the source of truth", () => {
    expect(designSystem).toContain("## Interaction state contract");
    expect(designSystem).toContain("## Accessibility");
    expect(designSystem).toContain("Infer");
    expect(designSystem).toContain("Geist Mono");
    expect(designSystem).toContain("### Select controls");
    expect(designSystem).toContain("shared `SelectControl`");
    expect(specs).toContain("type scale is 11/12/14/16/20/24px");
    expect(claudeRule).toContain("## Prohibited drift");
    expect(claudeRule).toContain("shared `SelectControl`");
    expect(cursorRule).toContain("## Prohibited drift");
    expect(cursorRule).toContain("shared `SelectControl`");
  });
});
