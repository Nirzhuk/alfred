import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { SettingsSidebar } from "./settings-sidebar";

test("includes License and Billing as a first-class settings destination", () => {
  const markup = renderToStaticMarkup(
    <SettingsSidebar
      activeSection="license-billing"
      onChange={() => {}}
      onBack={() => {}}
    />,
  );

  expect(markup).toContain("License &amp; Billing");
  expect(markup).toContain("Account");
  expect(markup).toContain('aria-current="page"');
});

test("keeps Native Agents separate from Connected apps", () => {
  const markup = renderToStaticMarkup(
    <SettingsSidebar
      activeSection="native-agents"
      onChange={() => {}}
      onBack={() => {}}
    />,
  );

  expect(markup).toContain("Native Agents");
  expect(markup).toContain("Connected apps");
  expect(markup.match(/settings-sidebar-item/g)?.length).toBeGreaterThan(2);
  expect(markup).toContain('aria-current="page"');
});

test("exposes MCP and skills under integrations", () => {
  const markup = renderToStaticMarkup(
    <SettingsSidebar
      activeSection="mcp-skills"
      onChange={() => {}}
      onBack={() => {}}
    />,
  );

  expect(markup).toContain("MCP &amp; Skills");
  expect(markup).toContain("Integrations");
  expect(markup).toContain('aria-current="page"');
});
