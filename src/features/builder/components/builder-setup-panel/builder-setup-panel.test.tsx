import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { BuilderSetupPanel, parsePairs, splitCommand } from "./builder-setup-panel";

test("splits a command line with quotes", () => {
  expect(splitCommand(`npx -y "@scope/pkg name" 'two words'`)).toEqual([
    "npx",
    "-y",
    "@scope/pkg name",
    "two words",
  ]);
  expect(splitCommand("uvx mcp-server-git")).toEqual(["uvx", "mcp-server-git"]);
});

test("parses env and header lines, ignoring junk", () => {
  expect(parsePairs("A=1\n\nB = two=2\nnope", "=")).toEqual({ A: "1", B: "two=2" });
  expect(parsePairs("Authorization: Bearer x:y", ":")).toEqual({ Authorization: "Bearer x:y" });
});

test("renders rules, servers and the built-in API", () => {
  const html = renderToStaticMarkup(
    <BuilderSetupPanel
      info={{ agents: [], mcpReady: true, tools: [{ name: "create_workflow", description: "d" }] }}
      agent={{ id: "codex", label: "Codex", installed: true }}
      config={{
        rules: [{ id: "r", name: "House style", body: "b", enabled: true }],
        mcpServers: [
          { id: "m", name: "github", enabled: false, transport: "stdio", command: "npx", args: [], env: {}, url: "", headers: {} },
        ],
      }}
      error={null}
      onSave={async () => true}
    />,
  );
  expect(html).toContain("Ready on this Mac");
  expect(html).toContain("agent-mark-codex");
  expect(html).toContain("1 tool<");
  expect(html).toContain("House style");
  expect(html).toContain("github");
  expect(html).toContain('aria-label="Use MCP server github"');
  expect(html).toContain('aria-checked="false"');
});
