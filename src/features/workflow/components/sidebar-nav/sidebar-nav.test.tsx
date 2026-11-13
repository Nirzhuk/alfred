import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { SidebarNav } from "./sidebar-nav";

const props = {
  view: "canvas" as const,
  activityOpen: false,
  activityEnabled: false,
  memoriesOpen: false,
  memoriesEnabled: false,
  unseenReplies: false,
  onChange: () => {},
  onNewWorkflow: () => {},
  onToggleActivity: () => {},
  onOpenMemories: () => {},
  onOpenConnectedApps: () => {},
  onOpenSettings: () => {},
};

test("shows the working bow and an accessible status while Alfred is working", () => {
  const html = renderToStaticMarkup(<SidebarNav {...props} working />);
  expect(html).toContain('class="sidebar-working-bow"');
  expect(html).toContain("Agent working");
});

test("keeps the idle icon and unread reply indicator when not working", () => {
  const html = renderToStaticMarkup(<SidebarNav {...props} working={false} unseenReplies />);
  expect(html).not.toContain("sidebar-working-bow");
  expect(html).not.toContain("Agent working");
  expect(html).toContain("New reply");
});
