import { describe, expect, test } from "bun:test";

const root = new URL("../", import.meta.url);
const css = await Bun.file(new URL("src/App.css", root)).text();
const modal = await Bun.file(
  new URL("src/components/modal/modal.tsx", root),
).text();
const confirmDialog = await Bun.file(
  new URL("src/components/confirm-dialog/confirm-dialog.tsx", root),
).text();
const renameWorkflowModal = await Bun.file(
  new URL(
    "src/features/workflow/components/rename-workflow-modal/rename-workflow-modal.tsx",
    root,
  ),
).text();
const workflowFolderModal = await Bun.file(
  new URL(
    "src/features/workflow/components/workflow-folder-modal/workflow-folder-modal.tsx",
    root,
  ),
).text();
const memoriesInspector = await Bun.file(
  new URL(
    "src/features/workflow/components/memories-inspector/memories-inspector.tsx",
    root,
  ),
).text();

function cssBlock(selector: string): string {
  const start = css.indexOf(`${selector} {`);
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
}

const namedModalCallers = [
  "src/components/confirm-dialog/confirm-dialog.tsx",
  "src/features/integrations/components/connected-app-tutorial-layout.tsx",
  "src/features/integrations/github-connect.tsx",
  "src/features/integrations/gmail-connect.tsx",
  "src/features/integrations/microsoft-connect.tsx",
  "src/features/integrations/telegram-connect.tsx",
  "src/features/integrations/whatsapp-connect.tsx",
  "src/features/workflow/components/memories-inspector/memories-inspector.tsx",
  "src/features/workflow/components/missed-schedules-modal/missed-schedules-modal.tsx",
  "src/features/workflow/components/node-settings-modal/node-settings-modal.tsx",
  "src/features/workflow/components/output-modal/output-modal.tsx",
  "src/features/workflow/components/rename-workflow-modal/rename-workflow-modal.tsx",
  "src/features/workflow/components/schedule-modal/schedule-modal.tsx",
  "src/features/workflow/components/triggers-modal/triggers-modal.tsx",
  "src/features/workflow/components/workflow-folder-modal/workflow-folder-modal.tsx",
];

describe("shared modal system", () => {
  test("contains keyboard focus, Escape, and focus-return behavior", () => {
    expect(modal).toContain('panel.querySelector<HTMLElement>("[autofocus]")');
    expect(modal).toContain("FOCUSABLE_SELECTOR");
    expect(modal).toContain('e.key !== "Tab"');
    expect(modal).toContain('e.key === "Escape"');
    expect(modal).toContain("previouslyFocusedRef.current?.isConnected");
    expect(modal).toContain('aria-modal="true"');
    expect(modal).toContain("tabIndex={-1}");
  });

  test("gives every shared modal an accessible name", async () => {
    for (const path of namedModalCallers) {
      const source = await Bun.file(new URL(path, root)).text();
      expect([path, /<Modal[\s\S]*?(?:labelledBy|label)=/.test(source)]).toEqual([
        path,
        true,
      ]);
    }
  });

  test("connects visible header descriptions to the dialog accessibility tree", async () => {
    expect(modal).toContain("descriptionId?: string");
    expect(modal).toContain("id={descriptionId}");
    for (const path of namedModalCallers) {
      const source = await Bun.file(new URL(path, root)).text();
      expect([path, source.includes("describedBy=")]).toEqual([path, true]);
    }
  });

  test("uses one shell for nested memory dialogs", () => {
    expect(memoriesInspector).toContain('className="memories-link-picker-modal"');
    expect(memoriesInspector).not.toContain('className="memories-link-picker-backdrop"');
    expect(memoriesInspector).not.toContain('role="dialog"');
    expect(memoriesInspector).toContain("Could not load memories. Try again.");
    expect(memoriesInspector).not.toContain("setLinkerError(String(error))");
  });

  test("keeps destructive confirmation visually and semantically distinct", () => {
    expect(confirmDialog).toContain('role="alertdialog"');
    expect(confirmDialog).toContain('confirm-modal is-danger');
    expect(confirmDialog).toContain('name={danger ? "trash" : "question"}');
    expect(confirmDialog).toContain('className="confirm-modal-footer"');
    expect(confirmDialog).toContain('type="submit"');
    expect(confirmDialog).toMatch(/type="submit"[\s\S]*?autoFocus/);
    expect(css).toContain(".confirm-modal-icon.is-danger");
    expect(css).toContain(".confirm-modal .modal-header");
    expect(css).not.toContain(".confirm-modal.is-danger .modal-header h3");
  });

  test("uses the shared compact form composition for workflow naming", () => {
    for (const source of [renameWorkflowModal, workflowFolderModal]) {
      expect(source).toContain('className="compact-form-modal"');
      expect(source).toContain('className="ghost modal-close-button"');
      expect(source).toContain('name="x"');
      expect(source).toContain('className="compact-form-modal-body"');
      expect(source).toContain('className="compact-form-modal-footer"');
      expect(source).not.toMatch(/<ModalHeader[\s\S]*?>[\s\S]*?Cancel/);
    }
    expect(css).toContain(".modal.compact-form-modal");
    expect(css).toContain(".compact-form-modal-footer");
  });

  test("uses the release modal composition in every theme", () => {
    expect(css).toContain("align-items: center;");
    expect(css).toContain("backdrop-filter: blur(14px) saturate(0.72);");
    expect(css).toContain("grid-template-columns: auto minmax(0, 1fr) auto;");
    expect(css).toContain(".modal-header-leading");
    expect(modal).toMatch(
      /modal-header-leading[\s\S]*modal-header-copy[\s\S]*<TitleTag/,
    );
    expect(modal).toContain("modal-header-description muted");
    expect(css).toContain("font-size: var(--text-2xl);");
    expect(css).toContain("background: var(--surface-raised);");
    expect(css).toContain("@media (prefers-reduced-transparency: reduce)");
    expect(css).toContain("@media (prefers-reduced-motion: no-preference)");
  });

  test("keeps close icons centered when a modal action overrides display", () => {
    expect(cssBlock(".modal-close-button")).toContain("justify-content: center;");
  });
});
