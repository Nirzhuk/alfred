import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { McpSkillsSettings } from "./mcp-skills-settings";

test("renders the build, MCP, and skills sections", () => {
  const html = renderToStaticMarkup(<McpSkillsSettings />);

  expect(html).toContain("Build");
  expect(html).toContain("Speak with Alfred");
  expect(html).toContain("Loading MCP configuration");
  expect(html).toContain("Skills");
  expect(html).toContain("Alfred skills");
  expect(html).toContain("Our shared");
  expect(html).toContain("Filter skills");
  expect(html).toContain("Reading");
});
