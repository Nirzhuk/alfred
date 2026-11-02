import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { FileInjectPaths } from "./file-inject-paths";

const render = (paths: string[]) =>
  renderToStaticMarkup(
    <FileInjectPaths paths={paths} onChange={() => undefined} variant="modal" />,
  );

test("invites a drop when empty", () => {
  expect(render([])).toContain("Drop files here to inject them");
});

test("offers Edit and Remove for each file", () => {
  const html = render(["/docs/blocks.json"]);
  expect(html).toContain('aria-label="Edit blocks.json"');
  expect(html).toContain('aria-label="Remove blocks.json"');
});
