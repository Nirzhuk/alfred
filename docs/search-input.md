# Shared search input

All application search fields use `SearchInput` from
`src/components/search-input`. Its stylesheet owns the inset background, border,
search icon, typography, focus state, clear button, and shortcut hint.

## Usage

```tsx
import { useRef, useState } from "react";
import { SearchInput } from "./components/search-input";

function MemorySearch() {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  return (
    <SearchInput
      ref={inputRef}
      aria-label="Search memories"
      placeholder="Search memories…"
      value={query}
      onChange={(event) => setQuery(event.currentTarget.value)}
      onClear={() => setQuery("")}
      clearLabel="Clear memory search"
      shortcut="/"
    />
  );
}
```

Adjust the import path relative to the caller. The shortcut hint only displays
text; the caller implements the shortcut. Clearing calls `onClear` once and
restores input focus. Supply `onClear` for editable searches. Read-only searches
hide the clear button; disabled searches disable both controls.

Native input props and refs reach the actual input. Keep filtering, debounce,
remote requests, and result selection in the screen. Combobox pickers retain
`type="text"`, `role="combobox"`, ARIA relationships, and `onKeyDown` handlers.
Do not put the component inside a `label`: it includes an interactive clear
button. Use `aria-label` to name the input.

`containerClassName` is for layout only, such as width, margins, or flex sizing.
Do not override field colors, borders, heights, icons, or descendant controls.
Modal input defaults must exclude `.ui-search-input`. The outer `.ui-search`
paints the field; the inner input must stay transparent. A competing modal
background rule produces a visible rectangle behind the text.

## Consumers

The component is used in Settings, History, the Add step picker, Skill picker,
workflow Resources, the run panel's memory library, app-action resource options,
trigger resource options, and these three memory controls:

- Memory library search in `memory-library-list.tsx`.
- Link-memory search in `memories-inspector.tsx`.
- Memories node selection search in `memory-node-settings.tsx`.

## Replacing an existing search

1. Locate every consumer, including text inputs used as search comboboxes.
2. Preserve its accessible name, ref, value, change handler, limits, disabled
   state, and keyboard behavior. Add its clear callback.
3. Replace custom search icons and clear buttons with `SearchInput`.
4. Delete unused search markup, helpers, and field styles after checking their
   references. Keep only container layout rules.
5. Run `bun test tests/search-input.test.tsx src/components/search-input` and
   `bun run build:frontend`. For visible changes, check light/dark themes,
   narrow layouts, typing, clearing/refocus, and picker keyboard navigation.
   Test the actual Memories library and nested link dialog with `App.css` loaded,
   not only the standalone component. Check the input's computed background is
   transparent at rest, focused, and with a query in both themes.

`tests/search-input.test.tsx` checks production TSX for raw search inputs,
including text inputs named as searches. Component behavior tests live beside
the component. Claude and Cursor design-system rules require this component.

## Browser regression audit

Follow [the actual-consumer audit](../tests/ui/README.md) when changing search
styles or their containing form/modal defaults. It mounts all eleven real
consumers with application CSS and mocked backend data. The checks compare
computed styles in both themes, empty/query/focused states, clear/refocus, and
shared CSS loaded first and last. Separate checks cover disabled/read-only
controls, narrow control geometry, and physical clear-button presses.

Run the negative check too: injecting the original modal background rule must
fail the transparency assertion. Remove the injected rule and confirm the
normal audit passes. This establishes that the check catches the reported bug.

Add new search consumers to `tests/ui/search-consumers.ts` and provide their
actual-screen scenarios. The ownership test compares this manifest with
production component uses. Typecheck the browser fixture separately using
`bunx tsc --noEmit -p tests/ui/tsconfig.json`.

The shared component owns pressed transforms and disabled opacity as well as
its normal appearance. Dim the wrapper once; do not dim its clear button again
or let generic disabled SVG rules recolor the icon independently.
