import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { WorkflowListItem } from "../src/features/workflow/components/workflow-list-item";
import type { Workflow } from "../src/features/workflow/types";

const workflow: Workflow = {
  id: "wf-seo",
  name: "SEO workflow",
  description: "",
  workingDirectory: "/Users/nirzhuk/development/apps/krysos",
  memoryLookupEnabled: false,
  graph: { nodes: [], edges: [] },
  createdAt: "2026-08-19T00:00:00Z",
  updatedAt: "2026-08-19T00:00:00Z",
};

function renderCard(dirty = false) {
  return renderToStaticMarkup(
    <WorkflowListItem
      workflow={workflow}
      active
      dirty={dirty}
      onSelect={() => {}}
      onOpenMenu={() => {}}
      onDragPointerDown={() => {}}
    />,
  );
}

describe("WorkflowListItem unsaved state", () => {
  test("keeps a solid card when the workflow is saved", () => {
    const markup = renderCard(false);
    expect(markup).not.toContain("is-dirty");
    expect(markup).not.toContain("workflow-card-dirty-border");
    expect(markup).not.toContain("Unsaved changes");
  });

  test("uses one normal border for the active saved card", async () => {
    const css = await Bun.file(new URL("../src/App.css", import.meta.url)).text();
    const selector = ".workflow-card.is-active .workflow-card-button {";
    const start = css.indexOf(selector);
    const block = css.slice(start, css.indexOf("}", start));
    expect(start).toBeGreaterThan(-1);
    expect(block).toContain("border-color: var(--accent-border-loud)");
    expect(block).not.toContain("box-shadow");
  });

  test("marks the active card with an accessible dashed unsaved frame", () => {
    const markup = renderCard(true);
    expect(markup).toContain("workflow-card is-active is-dirty");
    expect(markup).toContain('class="workflow-card-dirty-border"');
    expect(markup).toContain("Unsaved changes");
    expect(markup).toContain('class="sr-only"');
  });

  test("marches the unsaved stroke and keeps dashes when motion is reduced", async () => {
    const css = await Bun.file(new URL("../src/App.css", import.meta.url)).text();
    expect(css).toContain("@keyframes workflow-card-dash-march");
    expect(css).toContain(".workflow-card.is-dirty .workflow-card-button");
    expect(css).toContain("stroke-dasharray: 5 4;");
    expect(css).toContain(
      ".workflow-card-dirty-border rect {\n    animation: none;\n  }",
    );
  });
});
