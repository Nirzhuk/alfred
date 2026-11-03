import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { defaultResultTab, previewText, ResultFileChips } from "./node-run-result";

const chips = (paths: string[]) =>
  renderToStaticMarkup(<ResultFileChips paths={paths} onShowAll={() => undefined} />);

test("names up to two files, then only counts them", () => {
  expect(chips([])).toBe("");

  const two = chips(["/docs/cv.pdf", "/docs/letter.md"]);
  expect(two).toContain('aria-label="Open cv.pdf"');
  expect(two).toContain('aria-label="Open letter.md"');

  const three = chips(["/docs/cv.pdf", "/docs/letter.md", "/docs/notes.txt"]);
  expect(three).toContain("3 files");
  expect(three).not.toContain("Open cv.pdf");
});

test("shows only the first output paragraph in the compact card", () => {
  const output =
    'Telegram accepted the notification\n\n{\n  "acceptedAt": "2026-08-17T16:26:57Z"\n}';
  expect(previewText(output)).toBe("Telegram accepted the notification");
});

test("flattens and bounds compact output text", () => {
  expect(previewText("Hi!\nHow can I help you today?")).toBe(
    "Hi! How can I help you today?",
  );
  expect(previewText("a".repeat(200), 10)).toBe(`${"a".repeat(10)}…`);
});

test("defaults to Output once it exists, except on steps that supply input", () => {
  expect(defaultResultTab(true, true, false)).toBe("output");
  expect(defaultResultTab(true, true, true)).toBe("input");
  expect(defaultResultTab(true, false, false)).toBe("input");
  expect(defaultResultTab(false, true, true)).toBe("output");
});
