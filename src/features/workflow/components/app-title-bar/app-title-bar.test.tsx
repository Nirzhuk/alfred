import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { AppTitlebar } from "./app-title-bar";

function render(updateVersion: string | null): string {
  return renderToStaticMarkup(
    <AppTitlebar
      tabs={[]}
      activeWorkflowId={null}
      sidebarCollapsed={false}
      activityOpen={false}
      activityEnabled={false}
      activityRunning={false}
      updateVersion={updateVersion}
      onInstallUpdate={() => {}}
      onToggleSidebar={() => {}}
      onToggleActivity={() => {}}
      onSelectTab={() => {}}
      onCloseTab={() => {}}
      onNewTab={() => {}}
      onRenameTab={() => {}}
    />,
  );
}

describe("titlebar update offer", () => {
  test("stays hidden while Alfred is current", () => {
    expect(render(null)).not.toContain("titlebar-update");
  });

  test("is a top-left button naming the waiting version", () => {
    const markup = render("1.4.0");
    expect(markup).toContain('class="titlebar-update"');
    expect(markup).toContain("Update 1.4.0");
    expect(markup).toContain(
      'aria-label="Update available: Alfred 1.4.0. Install it and restart Alfred."',
    );
  });
});
