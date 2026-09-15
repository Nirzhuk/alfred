import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  WorkflowList,
  folderAtEdge,
} from "../src/features/workflow/components/workflow-list";
import type {
  Workflow,
  WorkflowFolder,
} from "../src/features/workflow/types";

function workflow(id: string, folderId: string | null): Workflow {
  return {
    id,
    name: `Workflow ${id}`,
    description: "",
    folderId,
    memoryLookupEnabled: false,
    graph: { nodes: [], edges: [] },
    createdAt: "2026-08-24T00:00:00Z",
    updatedAt: "2026-08-24T00:00:00Z",
  };
}

const folders: WorkflowFolder[] = [
  { id: "folder-potato", name: "Potato", createdAt: "2026-08-24T00:00:00Z" },
];

function renderList(withFolders: boolean) {
  return renderToStaticMarkup(
    <WorkflowList
      workflows={[workflow("a", "folder-potato"), workflow("b", null)]}
      folders={withFolders ? folders : []}
      activeWorkflowId={null}
      schedules={[]}
      onSelect={() => {}}
      onOpenMenu={() => {}}
      onOpenFolderMenu={() => {}}
      onMoveToFolder={() => {}}
    />,
  );
}

describe("WorkflowList folder groups", () => {
  test("keeps folder headers in the flow, with nothing pinned over the rows", () => {
    const markup = renderList(true);
    // One header per group (the folder plus Unfiled), and no sentinel or pinned
    // strip: a pinned row can only stay readable by painting a band over the
    // rows sliding beneath it.
    expect(markup.match(/class="workflow-folder-header"/g)?.length).toBe(2);
    expect(markup).not.toContain("workflow-folder-sentinel");
    expect(markup).not.toContain("is-pinned");
  });

  test("groups the flat list under one headerless section", () => {
    const markup = renderList(false);
    expect(markup).toContain("workflow-folder-group--flat");
    expect(markup).not.toContain("workflow-folder-header");
  });
});

describe("folderAtEdge", () => {
  // Groups are stacked in document order; `top` is each section's viewport top
  // and `edge` is the top of the scroller.
  const bands = [
    { key: "folder-potato", top: 40 },
    { key: "__unfiled__", top: 300 },
  ];

  test("names nothing while the first group still sits below the edge", () => {
    expect(folderAtEdge(bands, 0)).toBeNull();
  });

  test("names the group whose rows are crossing the edge", () => {
    expect(folderAtEdge(bands, 40)).toBe("folder-potato");
    expect(folderAtEdge(bands, 250)).toBe("folder-potato");
  });

  test("hands over as the next group reaches the edge", () => {
    // Rects are fractional, so the handover carries a 1px tolerance rather than
    // flickering between two groups on a subpixel scroll position.
    expect(folderAtEdge(bands, 298)).toBe("folder-potato");
    expect(folderAtEdge(bands, 299)).toBe("__unfiled__");
    expect(folderAtEdge(bands, 900)).toBe("__unfiled__");
  });

  test("names nothing when there are no groups", () => {
    expect(folderAtEdge([], 500)).toBeNull();
  });
});
