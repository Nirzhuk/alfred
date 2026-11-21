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
      info={{
        build: "production",
        dataDir: "/Users/me/Library/Application Support/alfred",
        agents: [],
        mcpReady: true,
        tools: [{ name: "create_workflow", description: "d" }],
        externalMcp: { name: "alfred", command: "/Applications/Alfred.app/Contents/MacOS/alfred mcp /tmp/e.json", config: { mcpServers: { alfred: { command: "/Applications/Alfred.app/Contents/MacOS/alfred", args: ["mcp", "/tmp/e.json"] } } } },
      }}
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
  expect(html).toContain("Bridge + MCP servers");
  expect(html).toContain("alfred mcp /tmp/e.json");
  expect(html).toContain("Other clients");
});

test("renders only the requested setup sections", () => {
  const html = renderToStaticMarkup(
    <BuilderSetupPanel
      sections={["mcp"]}
      info={{
        build: "development",
        dataDir: "/tmp/alfred-dev",
        agents: [],
        mcpReady: true,
        tools: [],
        externalMcp: { name: "alfred-dev", command: "/tmp/alfred mcp /tmp/e.json", config: { mcpServers: { "alfred-dev": { command: "/tmp/alfred", args: ["mcp", "/tmp/e.json"] } } } },
      }}
      agent={{ id: "codex", label: "Codex", installed: true }}
      config={{
        rules: [{ id: "r", name: "House style", body: "b", enabled: true }],
        mcpServers: [
          { id: "m", name: "github", enabled: true, transport: "stdio", command: "npx", args: [], env: {}, url: "", headers: {} },
        ],
      }}
      error={null}
      onSave={async () => true}
    />,
  );
  expect(html).toContain("MCP servers");
  expect(html).toContain("github");
  expect(html).not.toContain("House style");
  expect(html).not.toContain("Ready on this Mac");
  expect(html).not.toContain("Use from other agents");
});

test("can render the bridge section without the server list", () => {
  const html = renderToStaticMarkup(
    <BuilderSetupPanel
      sections={["connect"]}
      info={{
        build: "production",
        dataDir: "/tmp/alfred",
        agents: [],
        mcpReady: true,
        tools: [],
        externalMcp: { name: "alfred", command: "/tmp/alfred mcp /tmp/e.json", config: { mcpServers: { alfred: { command: "/tmp/alfred", args: ["mcp", "/tmp/e.json"] } } } },
      }}
      agent={undefined}
      config={{ rules: [], mcpServers: [] }}
      error={null}
      onSave={async () => true}
    />,
  );
  expect(html).toContain("Bridge + MCP servers");
  expect(html).toContain("alfred mcp /tmp/e.json");
  expect(html).toContain("Other clients");
  expect(html).not.toContain('aria-label="Add MCP server"');
});
