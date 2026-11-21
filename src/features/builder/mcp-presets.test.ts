import { expect, test } from "bun:test";
import { MCP_PATH_TOKEN, MCP_PRESETS, presetToServer } from "./mcp-presets";

test("every preset has a valid server name and a usable transport", () => {
  for (const preset of MCP_PRESETS) {
    expect(preset.id).toMatch(/^[a-z0-9_-]+$/);
    const server = presetToServer(preset);
    expect(server.name).toBe(preset.id);
    if (server.transport === "http") {
      expect(server.url).toMatch(/^https?:\/\//);
    } else {
      expect(server.command.length).toBeGreaterThan(0);
    }
  }
});

test("path presets use the active project when one is known", () => {
  const filesystem = MCP_PRESETS.find((preset) => preset.id === "filesystem");
  expect(filesystem).toBeDefined();
  const server = presetToServer(filesystem!, "/tmp/my-project");
  expect(server.args).toContain("/tmp/my-project");
  expect(server.args).not.toContain(MCP_PATH_TOKEN);
});

test("path presets keep a visible placeholder without a project", () => {
  const git = MCP_PRESETS.find((preset) => preset.id === "git");
  expect(git).toBeDefined();
  expect(presetToServer(git!, null).args).toContain(MCP_PATH_TOKEN);
});

test("GitHub preset carries the token key without inventing a secret", () => {
  const github = MCP_PRESETS.find((preset) => preset.id === "github");
  expect(github).toBeDefined();
  const server = presetToServer(github!);
  expect(server.env.GITHUB_PERSONAL_ACCESS_TOKEN).toBe("");
});

test("Linear uses the hosted HTTP endpoint", () => {
  const linear = MCP_PRESETS.find((preset) => preset.id === "linear");
  expect(linear).toBeDefined();
  const server = presetToServer(linear!);
  expect(server.transport).toBe("http");
  expect(server.url).toBe("https://mcp.linear.app/mcp");
});
