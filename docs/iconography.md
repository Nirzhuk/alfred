# Alfred iconography rules

Alfred's interface icons come from [Phosphor Icons](https://phosphor-icons.com)
(regular weight, MIT licensed). This document is the rulebook for using them;
`docs/design-system.md` remains the source of truth for sizing and layout.

## Rules

1. **Phosphor only.** Every new interface icon must be a Phosphor icon, regular
   weight. Do not hand-draw new SVG paths, copy icons from other sets, or add a
   new icon library.
2. **No inline SVGs.** Components never contain `<svg>` or `<path>` elements.
   Render through the shared component:

   ```tsx
   import { Icon } from "../../components/icon/icon";

   <Icon name="clock" size={16} />
   ```

3. **Current color only.** Icons inherit `fill="currentColor"` — no hard-coded
   fills. Recolor through the text color, never per-icon.
4. **Use the size tokens.** `--icon-size-default` (18), `--icon-size-compact`
   (16), `--icon-size-prominent` (20), `--icon-size-display` (24). Dense chrome
   may stay at its documented size (e.g. 14px menu icons) while the shared set
   is rolled out.
5. **Icon-only controls need a name.** Pass `label="…"` so the icon renders as
   an image with an accessible name; otherwise the icon is `aria-hidden`.
6. **One name per concept.** Check the mapping table below before choosing; if
   the concept exists, reuse its name. A new name is a deliberate decision —
   add it to the table.
7. **Keep the set lean.** `src/assets/icons/phosphor/` contains only icons the
   app uses. Delete the SVG and regenerate when an icon stops being used.

## Adding an icon

1. Download the regular-weight SVG from the Phosphor set into
   `src/assets/icons/phosphor/` (any source: phosphor-icons.com,
   the `@phosphor-icons/core` npm package, or the `phosphor-icons/web` repo's
   `core/assets/regular/` directory). File name must be the icon name, e.g.
   `magnifying-glass.svg`.
2. Run `bun run scripts/generate-phosphor-icons.mjs` — it rewrites
   `src/components/icon/phosphor.ts`, which drives the `<Icon>` component and
   its `name` prop typing.
3. Update the mapping table if the icon replaces a custom glyph.
4. Commit the SVG, the regenerated module, and the doc change together.

## Mapping of legacy inline icons

The table maps the hand-drawn inline icons that predate this ruleset to their
Phosphor replacements. Migration is mechanical: delete the local icon function
and replace its usages with `<Icon name="…" />`.

| Location (file → local icon) | Meaning | Phosphor name |
| --- | --- | --- |
| `identity-node.tsx` | Agent identity | `fingerprint` |
| `sidebar-nav.tsx` → `NewWorkflowIcon` | New workflow (doc + pencil) | `note-pencil` |
| `sidebar-nav.tsx` → `ActivityIcon` | Run activity bars | `chart-bar` |
| `sidebar-nav.tsx` → `MemoriesIcon` | Memories notebook | `note` |
| `sidebar-nav.tsx` → `ClockIcon` | Schedules | `clock` |
| `sidebar-nav.tsx` → `ConnectedAppsIcon` | Connected apps link | `link` |
| `sidebar-nav.tsx` → `SettingsIcon` | Settings gear | `gear-six` |
| `settings-sidebar.tsx` → `BackIcon` | Back arrow | `arrow-left` |
| `settings-sidebar.tsx` → `GeneralIcon` | General settings | `gear-six` |
| `settings-sidebar.tsx` → `QuickAccessIcon` | Quick access window | `monitor` |
| `settings-sidebar.tsx` → `ShortcutsIcon` | Keyboard shortcuts | `keyboard` |
| `settings-sidebar.tsx` → `NotificationIcon` | Notifications | `bell` |
| `settings-sidebar.tsx` → `ConnectedAppsIcon` | Connected apps | `link` |
| `settings-sidebar.tsx` → `DataIcon` | Data management | `database` |
| `settings-sidebar.tsx` → `SearchIcon` | Search | `magnifying-glass` |
| `workflow-context-menu.tsx` → `PencilIcon` | Rename | `pencil-simple` |
| `workflow-context-menu.tsx` → `PlayIcon` | Run | `play` |
| `workflow-context-menu.tsx` → `StopIcon` | Stop | `stop` |
| `workflow-context-menu.tsx` → `FolderIcon` | Move to folder | `folder` |
| `workflow-context-menu.tsx` → `ClockIcon` | Schedule | `clock` |
| `workflow-context-menu.tsx` → `FiltersIcon` | Triggers/filters | `sliders` |
| `workflow-context-menu.tsx` → `TrashIcon` | Delete | `trash` |
| `workflow-folder-context-menu.tsx` → `PlusIcon` | Create workflow | `plus` |
| `workflow-folder-context-menu.tsx` → `PencilIcon` | Rename folder | `pencil-simple` |
| `workflow-folder-context-menu.tsx` → `TrashIcon` | Delete folder | `trash` |
| `quick-access-popover.tsx` → `ClockIcon` | Schedule time | `clock` |
| `quick-access-popover.tsx` → `PlayIcon` | Run workflow | `play` |
| `quick-access-popover.tsx` → `ArrowIcon` | Open workflow | `arrow-right` |
| `quick-access-popover.tsx` → `ExpandIcon` | Expand window | `corners-out` |
| `quick-access-popover.tsx` → `GripIcon` | Drag handle | `dots-six-vertical` |
| `app-title-bar.tsx` → `SidebarIcon` | Toggle sidebar | `sidebar` |
| `app-title-bar.tsx` → `ActivityPanelIcon` | Toggle activity panel | `sidebar-simple` |
| `sidebar-bottom-bar.tsx` → `HelpIcon` | Help | `question` |
| `node-output-preview.tsx` → `TerminalGlyph` | Console output | `terminal-window` |
| `agent-usage-bar.tsx` → `RefreshGlyph` | Refresh usage | `arrow-clockwise` |
| `input-attachment-list.tsx` → `FolderGlyph` | Folder attachment | `folder` |
| `input-attachment-list.tsx` → `FileGlyph` | File attachment | `file` |
| `workflow-list.tsx` → `folderIcon` | Folder row | `folder` |
| `workflow-list-item.tsx` → inline folder | Working directory | `folder` |
| `workflow-canvas.tsx` → `FolderGlyph` | Folder button | `folder` |
| `workflow-canvas.tsx` → `FolderAddGlyph` | New folder | `folder-plus` |
| `prompt-node.tsx` → `BlockIcon` | Locked/unlocked node | `lock` / `lock-open` |
| `node-settings-modal.tsx` → `ChevronDownIcon` | Select chevron | `caret-down` |
| `node-settings-modal.tsx` → `CheckIcon` | Selected option | `check` |
| `select-control.tsx` → inline chevron | Select chevron | `caret-down` |
| `telegram-setup-progress.tsx` → inline check | Step complete | `check` |
| `external-link-icon.tsx` → `ExternalLinkIcon` | Open external link | `arrow-square-out` |

### Functional glyphs (keep custom)

These are controls, not concepts; they may stay inline or keep a custom shape.
Do not replace them with Phosphor unless the replacement is pixel-equivalent:

| Location | Glyph | Reason |
| --- | --- | --- |
| `prompt-node.tsx` → `ResizeGripIcon` | Diagonal resize grip | Interaction affordance; Phosphor has no grip equivalent (`corners-out` only if restyling is accepted). |
