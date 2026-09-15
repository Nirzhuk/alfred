import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { SearchInput } from "./search-input";

describe("shared search input", () => {
  test("preserves combobox semantics used by keyboard-driven pickers", () => {
    const html = renderToStaticMarkup(
      <SearchInput
        aria-label="Search steps"
        type="text"
        role="combobox"
        aria-controls="steps"
        aria-activedescendant="step-agent"
        aria-expanded
        value="agent"
        onChange={() => {}}
        onClear={() => {}}
      />,
    );
    expect(html).toContain('role="combobox"');
    expect(html).toContain('aria-controls="steps"');
    expect(html).toContain('aria-activedescendant="step-agent"');
    expect(html).toContain('aria-label="Search steps"');
    expect(html).toContain('type="text"');
    expect(html).toContain('type="button"');
  });

  test("read-only searches cannot offer a mutating clear action", () => {
    const html = renderToStaticMarkup(
      <SearchInput
        aria-label="Search"
        value="locked"
        readOnly
        onClear={() => {}}
      />,
    );
    expect(html).toContain('readOnly=""');
    expect(html).not.toContain("<button");
  });

  test("disabled searches disable both input and clear action", () => {
    const html = renderToStaticMarkup(
      <SearchInput
        aria-label="Search"
        value="query"
        disabled
        onChange={() => {}}
        onClear={() => {}}
      />,
    );
    expect(html.match(/disabled=""/g)).toHaveLength(2);
  });
});
