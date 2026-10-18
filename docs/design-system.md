# Alfred design system

This document is the source of truth for Alfred's visual language. It keeps the
desktop app quiet, legible, and consistent. The canonical implementation lives
in the semantic custom properties in `src/styles/tokens.css`.

## Where the stylesheet lives

The design system is one system served from four files. The split exists so a
second window does not have to parse the main window's component rules to use
the tokens; nothing about the cascade changes, because `src/App.css` imports the
first two before anything else.

| File | Holds | Imported by |
| --- | --- | --- |
| `src/styles/tokens.css` | The bundled `@font-face` rules, the `:root` token block, and the `[data-theme="dark"]` overrides. Nothing here paints. | every window |
| `src/styles/base.css` | The reset, the document surface, the focus ring, the shared `button` and `SelectControl` chrome, and the reduced-motion rules. Resolves tokens; never defines them. | every window |
| `src/App.css` | Every component rule for the main window. | the main window |
| `src/features/quick-access/quick-access.css` | Every rule for the Quick Access popover. | the Quick Access window |

Add a new token to `:root` in `src/styles/tokens.css` and document it here. Add
a shared reset or control primitive to `src/styles/base.css`. Feature rules stay
with their feature or in `src/App.css` — do not put a component rule in the base
layer to make it reachable from another window; give that window its own
stylesheet that imports the tokens and the base, as Quick Access does.

## Principles

- **Quiet hierarchy:** spacing, color, and placement establish structure before
  font weight does. Most interactive text is regular weight.
- **One role, one scale:** components with the same purpose share typography,
  icons, shape, spacing, and interaction states.
- **Desktop-native density:** controls stay compact while remaining clear,
  keyboard accessible, and easy to target.
- **Flat by default:** borders and shadows communicate containment, not emphasis.
- **Tokens before literals:** repeated decisions use semantic tokens. A literal
  is acceptable only for unique geometry or a documented optical correction.
- **State never changes layout:** hover, selection, focus, and loading must not
  move surrounding content or change text metrics.

## Visual direction

Alfred uses a precise monochrome interface with one controlled emerald signal.
The dark canvas is `#101010`, ordinary controls are `#202020`, raised cards are
`#262626`, primary text is `#fcfcfc`, supporting text is `#aaaaaa`, and disabled
icons are `#8e8f8d`. The dark theme's signal emerald is `#38c99b`; accessible
primary buttons use its deeper `#137a5f` action tone. Emerald is reserved for
primary actions, focus, current location, running state, and success. It is not
a general surface tint.

Red and amber remain available for destructive, failed, and warning states.
Third-party marks may retain their identity colors. Everything else uses the
neutral surface scale.

## Cross-platform contract

Alfred supports macOS, Windows, and Linux. Cohesion means the application-owned
interface shares the same metrics and behavior everywhere; it does not mean
recreating OS-owned window controls or system dialogs.

- `src/platform.ts` detects the desktop family once and writes
  `data-platform="macos|windows|linux|unknown"` on the root element before the
  application UI loads.
- Platform selectors are limited to genuine OS boundaries such as window-chrome
  safe areas. Do not create platform-specific typography, spacing, radii,
  colors, or component layouts.
- macOS reserves 78px at the title bar's leading edge for traffic lights, or
  72px in compact layouts. Windows, Linux, unknown platforms, and fullscreen
  windows use the standard 12px inset.
- Geist, tokenized control metrics, custom select chevrons, checkboxes, focus
  rings, and selection colors prevent webview and OS defaults from changing the
  application-owned UI.
- Keyboard labels may use macOS symbols where appropriate. Windows and Linux use
  the equivalent Ctrl/Super terminology.
- File pickers, notifications, permission prompts, window buttons, and other
  OS-owned surfaces remain native. Their content and trigger affordances should
  still follow Alfred's terminology and state model.
- Never infer platform independently inside a component. Reuse
  `detectDesktopPlatform` or the root `data-platform` attribute.

Review at 100% scale on every supported OS. Also check Windows at 125% and 150%
display scaling, macOS at standard and Retina scale, and Linux on at least one
Wayland or X11 desktop before a visual release.

## Typography

Infer is Alfred's primary interface family. Geist is the bundled release-safe
fallback until the licensed Infer WOFF2 asset is added to `src/assets/fonts/`.
Geist Mono is bundled and provides the utility accent for metadata, status,
shortcuts, identifiers, schedules, logs, and code. The UI never makes a remote
font request.

| Role | Family | Size | Weight | Line height |
| --- | --- | --- | --- | --- |
| Page title | `--font-display` | `--text-2xl` / 24px | 600 | 1.2 |
| Dialog title | `--font-display` | `--text-2xl` / 24px | 600 | 1.2 |
| Section title | `--font-sans` | `--text-lg` / 16px | 600 | 1.2 |
| Navigation section label | `--font-sans` | `--text-lg` / 16px | 400 | 1.2 |
| Body, item, control | `--font-sans` | `--text-md` / 14px | 400 | 1.4 |
| Supporting text | `--font-sans` | `--text-sm` / 12px | 400 | 1.4 |
| Micro/status text | `--font-mono` | `--text-xs` / 11px | 500–700 | 1.2 |
| Dock/badge micro text | `--font-mono` | `--text-2xs` / 10px | 400–600 | 1 |
| Technical content | `--font-mono` | appropriate role size | 400–600 | 1.4–1.55 |

Rules:

- `--font-sans` prefers Infer and falls back to bundled Geist.
  `--font-display` follows the same family, and `--font-mono` resolves to
  Geist Mono.
- Page and dialog titles use the primary family through `--font-display`.
  Alfred does not mix a decorative display face into product chrome.
- Use only weights 400, 500, 600, and 700 through the weight tokens. Fractional
  weights such as 550 or 650 are prohibited.
- Controls and navigation use weight 400. Selected state never adds weight.
  This includes navigation section labels: in the rails, size and color carry
  the hierarchy, so a section label separates itself from its items by being
  larger and lighter in color, never heavier.
- Use 600 for headings and meaningful hierarchy. Reserve 700 for short status
  labels or urgent emphasis.
- All caps and added letter spacing are limited to short status badges. Do not
  use them for navigation or section headings.
- `--text-2xs` is the floor and exists only for dense always-on chrome such
  as the agent usage dock, where a secondary line must not compete with the
  status it annotates. Never use it for body copy, controls, form labels, or
  anything a user must read to act. If a surface needs it in more than one
  place, that surface is too dense, not the scale too coarse.
- Numeric logs, schedules, identifiers, shortcuts, and code use Geist Mono.

## Spacing

Use the four-pixel spacing grid. Half steps exist for dense desktop alignment:

| Token | Value | Typical use |
| --- | --- | --- |
| `--space-0-5` | 2px | optical separation |
| `--space-1` | 4px | tight internal gap |
| `--space-1-5` | 6px | compact control gap |
| `--space-2` | 8px | standard internal gap |
| `--space-2-5` | 10px | icon-to-label gap |
| `--space-3` | 12px | control/card padding |
| `--space-4` | 16px | component separation |
| `--space-5` | 20px | section padding |
| `--space-6` | 24px | page padding |
| `--space-8` | 32px | large separation |
| `--space-10` | 40px | page-level separation |

`font-size` and `padding` / `margin` / `gap` must resolve to a token. These are
the two properties that drift fastest, because a literal always looks defensible
in isolation: `0.72rem` is 11.5px and `0.45rem` is 7.2px, so each one lands
between steps and the app quietly grows a second scale nobody declared.

Unique layout constraints — a panel width, a graph coordinate, a column indent,
clearance for an absolutely positioned control — may stay literal, but must say
so with a `geometry:` comment on the line above:

```css
.run-console-detail {
  /* geometry: 9.72rem indent aligns detail under the timestamp column. */
  margin: var(--space-1) 0 0 9.72rem;
}
```

`tests/design-system.test.ts` fails on any untagged literal and names the file
and line. The comment is the whole escape hatch: if a value cannot be explained
in one line, it is drift, not geometry.

## Shape and control density

| Role | Token | Value |
| --- | --- | --- |
| Small detail | `--radius-sm` | 4px |
| Controls and rows | `--radius-md` | 8px |
| Cards and menus | `--radius-lg` | 12px |
| Dialogs and floating panels | `--radius-xl` | 16px |
| Pills, toggles, dots | `--radius-pill` | 999px |
| Compact control | `--control-height-compact` | 28px |
| Default control | `--control-height-default` | 32px |
| Comfortable input | `--control-height-comfortable` | 36px |
| Navigation row | `--control-height-navigation` | 38px |
| Large primary control | `--control-height-large` | 40px |

Do not invent intermediate radii or control heights. A visible pointer target is
at least 28px in dense chrome and 32px for ordinary controls.

## Iconography

- Icons are Phosphor Icons (regular weight) rendered through the shared
  `Icon` component; see `docs/iconography.md` for the rules, the add-an-icon
  workflow, and the mapping of legacy inline icons.
- Use one outline icon language per surface with consistent optical stroke.
- Standard interface icons are `--icon-size-default` / 18px.
- `--icon-size-compact` / 16px is for dense secondary controls only.
- `--icon-size-prominent` / 20px and `--icon-size-display` / 24px are for
  intentional emphasis, not ordinary rows.
- Icons in a repeated list use the same bounding box. Align the box, not the
  path's visual edges.
- Icon-only controls require an accessible name and visible tooltip or title.

## Color, surfaces, and elevation

- Use semantic color tokens such as `--ink`, `--muted`, `--surface-*`,
  `--accent-*`, and `--danger-*`; never branch component CSS by theme.
- Use `--control-*` tokens for shared control states.
- Use emerald only when the interface is communicating action, location,
  focus, progress, or success. Hover is neutral.
- `--brand-alfred` and `--brand-alfred-deep` are the app-icon teal. They paint
  only Alfred's own mustache mark (the Speak with Alfred orb), never controls,
  states, or text, and stay fixed across themes like third-party agent marks.
- On macOS, the transparent window installs one full-window native
  `NSVisualEffectView` through [window-vibrancy](https://github.com/tauri-apps/window-vibrancy)
  with HudWindow material. The HTML roots remain transparent and add a 60%
  panel tint, so the same wallpaper color reaches the sidebar and titlebar,
  while canvas and page surfaces mask it wherever content needs an opaque
  reading plane.
- Quick Access uses the same HudWindow vibrancy and 60% theme panel tint
  inside a rounded 16px borderless window. Its light and dark appearances
  follow the shared semantic tokens. Windows, Linux, and the
  reduced-transparency fallback use the opaque panel surface.
- The main window stays hidden until the lazy React application has committed
  and its fonts are ready. Window-state restoration deliberately excludes
  visibility so native material and web content appear in the same frame.
- Windows and Linux use the fully opaque panel surface. They do not imitate
  native glass with a translucent window. Reduced-transparency mode on macOS
  also uses the opaque panel token to respect the system accessibility setting.

### Surface fills

Fills are named by stacking role, not by how light they are. Choose one by
asking what the element sits **on**, never by picking the white that looks
right in isolation:

| Token | Role |
| --- | --- |
| `--surface-panel` | app chrome behind everything |
| `--surface-inset` | recessed well: segmented-control track, list well |
| `--surface-card` | container resting on the panel |
| `--surface-raised` | control or row lifted above a card |
| `--surface` | opaque; occludes what is beneath it (menus, popovers) |

These replaced seven overlapping tokens (`--surface-soft`, `-strong`, `-glass`,
`-faint`, `-muted`, `--panel-solid`, `--panel-2`) that named intensity instead
of role. Three of them meant "raised" at three arbitrary alphas, which is how a
shortcut recorder ended up filled at 92% white while sitting on a 55% white
card — a control brighter than its own container. Picking by role makes that
mistake unrepresentable.

Rules:

- Never add a fourth fill or a raw `rgba()` / hex background. If nothing fits,
  the element's stacking is wrong, not the scale.
- A fill that genuinely must not follow the theme — an always-white logo tile,
  a scannable QR plate — needs a `theme-exempt:` comment saying why, the same
  escape hatch as `geometry:`.
- Do not confuse these with `--surface-hover` / `-selected` / `-pressed`. Those
  are interaction states; these are containers.
- Navigation and list selection use a subtle background without a new border,
  shadow, or weight change.
- Use `--elevation-raised` for small floating controls,
  `--elevation-popover` for menus, and `--elevation-modal` for dialogs.
- Do not add a shadow to a flat card or selected row. Add elevation only when an
  element sits above another interaction plane.
- Use the `--layer-*` scale from content through toast; arbitrary z-index values
  are prohibited.

## Interaction state contract

Every interactive component supports the states that apply to it:

| State | Required behavior |
| --- | --- |
| Default | Uses the role's semantic foreground, background, and border. |
| Hover | Changes color or surface only; does not change layout or weight. |
| Pressed | Uses the pressed surface; buttons may translate by 1px. |
| Selected | Uses the selected surface and retains its default text metrics. |
| Focus visible | Shows the shared two-pixel focus ring with a two-pixel offset. |
| Disabled | Uses `--disabled-opacity`, blocks activation, and keeps its label readable. |
| Error/destructive | Uses `--danger-*`; color is never the only explanation. |
| Loading | Keeps the control's dimensions stable and exposes status accessibly. |

Do not remove focus indication. `:focus-visible` is preferred so pointer clicks
do not create unnecessary rings.

### Which surface scale to use

Two scales cover every hover, pressed, and selected surface. Pick by whether
the element owns a background of its own.

| Scale | Tokens | Use for |
| --- | --- | --- |
| Quiet (transparent) | `--surface-hover`, `--surface-selected`, `--surface-pressed` | Borderless rows, navigation items, menu items, list rows, ghost icon buttons — anything sitting directly on a panel |
| Filled (opaque) | `--control-background-hover`, `--control-background-pressed`, `--control-background-selected` | Buttons, inputs, selects, and other controls that already paint their own background |

Both are mixed against `--ink`, so they invert with the theme and stay visible
on a panel that is already near-white (light) or near-black (dark), where a
same-tone white or black alpha wash reads as invisible. The quiet scale stays
transparent so the panel, canvas, or card beneath tints through instead of
being covered by an opaque tile.

Rules:

- Hover is never an accent tint. Accent marks a genuinely selected, active, or
  primary state; using it for hover makes every pointer move look like a
  selection.
- Do not introduce a feature-local wash (`--surface-soft`, `--surface-wash`,
  `--ink-faint`, a raw rgba) for one component's hover. That is the drift this
  contract exists to prevent: a value tuned against one panel is invisible or
  heavy on the next.
- `--surface-*` and `--surface-hover`/`-selected`/`-pressed` are different
  things. The former are theme fills for panels and cards; the latter three are
  interaction states. Do not substitute one for the other.

`tests/design-system.test.ts` enforces this across the navigation rails, menus,
folder rows, integration rows, and picker lists.

## Shared component contracts

### Sidebar navigation

Workflow navigation, Settings navigation, folders, and workflow rows share:

| Role | Token | Resolved value |
| --- | --- | --- |
| Item text | `--sidebar-item-font-size` | 14px |
| Item weight | `--sidebar-item-font-weight` | 400 |
| Item color | `--sidebar-item-color` | `#aaaaaa` in dark mode |
| Item icon | `--sidebar-icon-size` | 16px square |
| Icon color (default) | `--sidebar-icon-color` | `#8e8f8d` in dark mode |
| Icon color (hover/selected) | none | `#fcfcfc` in dark mode |
| Section text | `--sidebar-section-font-size` | 16px |
| Section weight | `--sidebar-section-font-weight` | 400 |
| Section color | `--sidebar-section-color` | muted at 72% alpha |
| Hover surface | `--surface-hover` | ink at 4% alpha |
| Selected surface | `--surface-selected` | ink at 7% alpha |
| Item minimum height | `--sidebar-item-min-height` | 32px |
| Icon-to-label gap | `--sidebar-item-gap` | 4px |
| Row-to-row gap | `--sidebar-item-stack-gap` | 2px |
| Item inline padding | `--sidebar-item-inline-padding` | 8px |

Sidebar rows are borderless. Secondary metadata may use the supporting or micro
scale but must not compete with the item label. Keep only 2px between adjacent
rows so their hover and selected surfaces read as one compact navigation stack.

Navigation is the quietest surface in the app, so its colors step back from the
global scale in three stages: item ink is softened off full `--ink`, icons and
section labels sit lighter still, and the section label never reaches item
contrast even at weight 400. Hover and selected use the shared quiet scale
(`--surface-hover`, `--surface-selected`) described under **Which surface scale
to use**. The selected row adds only a two-pixel emerald inset marker. It does
not change weight or layout.

Settings pages carry no rule between the header and the body. The heading's
type scale already separates them; a divider there re-adds the boxed-in look
the rails are tuned to avoid.

Folder rows stay in the flow and scroll away with their group. **Do not pin
them.** A pinned row must occlude the rows sliding beneath it, and every way of
softening that fill was tried and fails: no fill, blur alone, and a gradient
dissolve all let row text read through the folder name, while an opaque strip
paints a band across a translucent panel. The band is the artifact; the folder
name is the requirement.

So the sidebar header carries it. While the list is scrolled, the chrome row
above the scroller reads `Workflows / <folder> <count>` — the folder whose rows
are crossing the top of the scroller, resolved by `folderAtEdge` in
`workflow-list.tsx` and reported upward on scroll. Nothing scrolls behind chrome,
so nothing has to be painted over. The context uses the item scale in the
section color, so it reads quieter than the title it follows, and it is rendered
into the existing header row, which keeps its height whether the context is
present or not.

That context appears, leaves, and swaps rather than teleporting: it rises
`var(--space-1)` while fading in over `--duration-standard` and leaves back down
the same path over `--duration-fast`, both on `--ease-emphasized`. Both labels
occupy one grid cell, so an overlapping swap cannot move the header, and the
incoming label animates on mount because it exists only while it is current — a
count change on the same folder updates in place and does not animate. Under
reduced motion the crossfade stays and the travel is dropped. The outgoing label
is unmounted on a timer that must stay equal to `--duration-fast`; a test pins
the two together.

Workflow rows follow the panel they sit on. Off macOS the panel is opaque, so
they take container fills — `--surface-raised` at rest, `--surface` for hover,
selection, and the drag ghost — ordered so rest sits below hover and selection
is never dimmer than hover. On macOS the sidebar is a translucent material,
where an opaque fill punches the vibrancy out of the row's own rectangle, so
rows inside `.sidebar` take the same quiet ink scale the nav items use there:
`--surface-selected` at rest and `--surface-pressed` for hover and selection —
the same kind of surface as a selected nav item, several steps stronger, because
a workflow row is a card and a nav item is not. The drag ghost is excluded: it
floats over the canvas, not the panel, so it stays opaque. Selection stays
identified by the loud accent border and the accent dot, not by a heavier fill.

### Menus

- Menu items use 14px/400, 18px icons, 32px minimum height, and 8px radius.
- Menu labels use 12px/600 without all caps.
- Hover and open states use a flat selected surface without a visible border.

### Forms and buttons

- Ordinary inputs and buttons use 14px text and 8px radius.
- Field labels use 14px/500; descriptions and hints use 12px/400.
- Primary buttons may use 600 weight. Secondary and ghost buttons use 400.
- Validation stays next to the affected field and does not rely on color alone.

### Select controls

- New or restyled select fields use the shared `SelectControl` from
  `src/components/select-control`; feature code must not add its own native
  arrow treatment or field chrome.
- Keep native select semantics, keyboard behavior, and option menus. The shared
  wrapper owns only the surface, interaction states, and single chevron.
- Use the default density in forms and Settings. Compact density is reserved for
  dense desktop chrome such as the compact Quick Access window.
- The caller owns layout width; the component owns height, padding, typography,
  border, focus, hover, and disabled states.
- All field-style controls share one face: the `--control-surface` gradient
  over `--control-border` and `--radius-md`, lifted by `--elevation-raised`.
  Select-like popover triggers (such as the model picker trigger) reuse that
  face, the comfortable control height, and the same hover/focus states as
  `SelectControl`, so every select in the app reads as one physical control.

### Search fields

See [Shared search input](search-input.md) for usage, consumers, and the migration
checklist. Replacing a search includes migrating existing consumers and removing
their unused markup, helpers, and field CSS.

- Use `SearchInput` from `src/components/search-input` for search fields in
  dialogs, sidebars, pickers, and toolbars. It owns the search icon, inset field,
  focus state, typography, clear button, and optional shortcut hint.
- Supply an accessible `aria-label`, controlled `value`, native `onChange`, and
  `onClear`. Clearing restores input focus. Feature code owns filtering, debounce,
  selection, and keyboard behavior.
- Refs and native input/ARIA props pass through to the input. Combobox pickers
  may use `type="text"` and their existing combobox roles and relationships.
- Use `containerClassName` only for placement and width. Do not add local field
  paint, icons, or clear buttons. Disabled and read-only fields retain native
  semantics.

### Cards and dialogs

- Cards use 12px radius. Borders communicate grouping; shadows are normally off.
- Dialogs use the shared 12px precision-sheet shell and modal elevation token.
  They open near the top of the workspace instead of floating at dead center.
- Modal backdrops blur and mute the workspace. Reduced-transparency mode keeps
  the scrim but removes the filter.
- Dialog headers use one horizontal composition: an optional 40px identity
  tile, an Infer title with one concise explanatory line, and compact actions
  aligned at the trailing edge. Provider setup dialogs use the provider mark;
  workflow tools use a shared Phosphor icon tile.
- Dialog titles use 24px/600. Header descriptions use 14px/400 and should
  explain the decision or task instead of repeating a category label.
- Modal headers close with the shared X icon button: `ghost modal-close-button`
  carrying the 16px Phosphor `x` icon and `aria-label="Close"`. Never use a
  labeled Done/OK pill as a modal's close affordance; a decision that needs
  explicit confirmation gets a real action button in the body instead.
- Dialog headers use the raised surface and bodies use the card surface. Inputs
  and nested controls step up to the raised surface. Emerald is reserved for
  focus and the primary decision.
- Shared modals trap Tab, close with Escape when allowed, lock background
  scrolling, and return focus to the opener. Feature dialogs must not rebuild
  this behavior.
- A workflow card with unsaved edits replaces its solid border with a marching
  dashed accent stroke. The dashes themselves are the status cue, so they stay
  visible when reduced motion turns the march off.

### Workflow canvas

- The canvas uses a single 22px dot grid for placement. It moves and zooms with
  React Flow without adding graph-paper line guides behind the workflow.
- Pattern contrast stays subordinate to nodes and connections. It organizes
  the editing surface; it is never used as decoration on cards or pages.
- Canvas zoom ranges from 25% to 100%. Initial framing and Fit canvas use the same
  limits so large workflows can fit without changing node sizes.

## Motion

- Use `--duration-fast` / 120ms for direct feedback,
  `--duration-standard` / 180ms for component transitions, and
  `--duration-slow` / 280ms for panels.
- Use `--ease-standard` for fades and color changes and `--ease-emphasized` for
  spatial transitions.
- Prefer opacity and transform. Avoid animating layout during direct manipulation.
- Looping status motion (orbit, marching dashes) may use a longer linear
  duration than the transition scale. The static rest pose must still read.
- Rotating highlights on a non-circular shape (a rounded tile, a composer, a
  card border) animate a registered `@property` angle inside a fixed
  `conic-gradient`, never `transform: rotate` on the shape, so the outline
  stays put while the light travels around it.
- The global reduced-motion rule must remain in place. Critical state changes
  must remain understandable when animation is effectively disabled.

## Accessibility

- Text and meaningful icons meet WCAG AA contrast in light and dark themes.
- Keyboard order follows visual order; all actions are operable without a mouse.
- Icon-only actions have accessible names. Form controls have programmatic labels.
- Do not use color alone for status, errors, or selection.
- Verify focus, disabled, loading, empty, and error states in both themes.
- Do not replace native OS chrome or dialogs solely for visual similarity; their
  platform behavior and accessibility are part of the interaction contract.

## Implementation and review

1. Reuse an existing token or shared component before adding a literal.
2. Add repeated decisions to `:root` in `src/styles/tokens.css` and document
   them here.
3. Do not duplicate a token under a feature-specific name.
4. Compare related surfaces at the same viewport and theme.
5. Run `bun test`, `bun run build:frontend`, and `git diff --check`.

Exceptions must be intentional, narrowly scoped, and explained beside the CSS
or in this document. Canvas geometry and third-party component constraints are
the common legitimate exceptions; visual preference alone is not.
