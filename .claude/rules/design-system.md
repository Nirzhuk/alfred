# Design system

For user-interface work, read and follow `docs/design-system.md`.

## Required behavior

- Use Infer as the preferred interface family with bundled Geist as the local
  fallback. Use Geist Mono for utility accents through `--font-sans`,
  `--font-display`, and `--font-mono`. Do not add remote font imports.
- Reuse semantic CSS custom properties from `src/styles/tokens.css`; do not
  duplicate a shared visual decision with feature-local literals.
- When a repeated visual rule does not have a token, add one to `:root` in
  `src/styles/tokens.css` and document it in `docs/design-system.md`.
- Related surfaces must share typography, icon sizing, spacing, and interaction
  states unless the product hierarchy explicitly requires a difference.
- Keep navigation quiet and flat. Selection may use the shared two-pixel
  emerald inset marker, but never heavier text or a layout change.
- Use one of two surface scales for every hover, pressed, and selected state.
  Quiet and transparent (`--surface-hover`, `--surface-selected`,
  `--surface-pressed`) for borderless rows, nav and menu items, list rows, and
  ghost icon buttons. Filled and opaque (`--control-background-hover`,
  `--control-background-pressed`, `--control-background-selected`) for controls
  that already paint their own background. Never invent a feature-local wash
  from `--surface-soft`, `--surface-wash`, `--ink-faint`, or a raw rgba, and
  never use an accent tint for hover — accent marks a real selection.
- Use only the documented typography weights, spacing, radius, control height,
  icon, elevation, motion, and layer tokens for shared UI.
- Resolve every `font-size`, `padding`, `margin`, and `gap` to a token. A literal
  is drift even when it looks reasonable alone, because near-miss values like
  `0.72rem` land between steps. Unique geometry (column indents, clearance for
  an absolute control, panel widths) may stay literal only with a `geometry:`
  comment on the line above explaining it; the tests fail on untagged literals.
- Pick a background from the surface role scale by asking what the element sits
  on: `--surface-panel` (chrome), `--surface-inset` (recessed well),
  `--surface-card` (container on the panel), `--surface-raised` (control above a
  card), `--surface` (opaque, occludes). Never add a fourth fill or a raw
  rgba/hex background — a fill that must not follow the theme needs a
  `theme-exempt:` comment. These are containers; `--surface-hover` /
  `-selected` / `-pressed` are interaction states. Do not mix the two.
- Every interactive component must account for hover, pressed, focus-visible,
  disabled, and applicable selected/error/loading states without layout shifts.
- Preserve keyboard focus, accessible names, contrast, and reduced-motion
  behavior in both themes.
- Use the shared `SelectControl` for new or restyled select fields. Do not add
  feature-local select chrome or native arrow styling; keep native select
  semantics and use compact density only in dense desktop chrome.
- Keep application-owned metrics identical across macOS, Windows, and Linux.
  Use `src/platform.ts` or the root `data-platform` attribute only for true OS
  boundaries such as title-bar safe areas, shortcut notation, and permissions.
- Normalize form controls through the shared CSS patterns; do not depend on a
  browser or OS default appearance for application-owned controls.

## Shared search controls

- Changes to shared search styles or containing form/modal defaults must run the
  actual-consumer browser audit in `tests/ui/README.md`, including both themes,
  stylesheet order, disabled/read-only states, and physical pressed states.
  Keep `tests/ui/search-consumers.ts` and its real-screen scenarios complete.

- Verify shared controls inside their actual consumers, especially modal and
  nested-dialog containers. Standalone previews do not prove CSS integration.
  Container input defaults must exclude `.ui-search-input` so the shared
  component owns its transparent inner field.

- Use `SearchInput` from `src/components/search-input` for every search field,
  including Memories, link pickers, settings, and text-based combobox searches.
  Do not create raw search inputs or feature-local search icons/clear buttons.
- Keep search field styling in `search-input.css`. Consumer classes may control
  only layout, such as width, flex, and margins. Preserve native input refs,
  accessible names, keyboard handlers, debounce, and filtering in the caller.
- When replacing a control, migrate every consumer and delete its unused markup,
  helpers, and CSS. Check references before removing code. A shared component
  is not a completed migration until existing screens actually use it.
- Follow `docs/search-input.md` for usage and the migration checklist. Run
  `bun test tests/search-input.test.tsx src/components/search-input` to check
  ownership and component behavior.

## Sidebar invariant

- Item text: `14px`, weight `400`.
- Icons: `18px` square.
- Section labels: `16px`, weight `400`.
- Use the `--sidebar-*` tokens; never restate these values in sidebar component
  selectors.
- Selected items retain weight `400` and may add only the shared emerald inset
  marker to the shared surface background.

## Prohibited drift

- No fractional font weights such as 550, 620, 650, or 750.
- No arbitrary z-index for application UI; use the `--layer-*` scale.
- No new radius, spacing, font-size, icon-size, or motion literal when a semantic
  token describes the role.
- No color-only status or removal of `:focus-visible` styling.
- No component-local user-agent detection or platform-specific visual scale.

Verify meaningful visual changes in both themes and run tests, the frontend
build, and `git diff --check` before handoff.
