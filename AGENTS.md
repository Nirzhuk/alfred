# Repository guidance

For UI changes, follow [the design system](docs/design-system.md) and
[iconography rules](docs/iconography.md).

## Shared search controls

- Changes to shared search styles or containing form/modal defaults must run the
  actual-consumer browser audit in `tests/ui/README.md`, including both themes,
  stylesheet order, disabled/read-only states, and physical pressed states.
  Keep `tests/ui/search-consumers.ts` and its real-screen scenarios complete.

- Verify shared controls inside their actual consumers, especially modal and
  nested-dialog containers. Standalone previews do not prove CSS integration.
  Container input defaults must exclude `.ui-search-input` so the shared
  component owns its transparent inner field.

- Use `SearchInput` from `src/components/search-input` for every application
  search, including text-based combobox searches. Follow
  [the usage guide](docs/search-input.md).
- Preserve accessible names, input refs, keyboard handlers, disabled states,
  limits, debounce, and filtering. Editable searches provide `onClear`.
- Keep field styling, icons, and clear buttons in the shared component.
  Consumer classes control layout only.
- A replacement must migrate existing consumers, including memory dialogs and
  pickers. Find references and remove obsolete markup, helpers, and CSS; do not
  leave an unused shared component beside the old implementations.
- Colocate component behavior tests. The application-wide ownership check is
  `tests/search-input.test.tsx`. Run `bun test`, `bun run build:frontend`, and
  `git diff --check` before handoff. Verify visible changes in both themes and
  check typing, clear/refocus, and picker keyboard navigation.
