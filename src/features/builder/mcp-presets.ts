import type { BuilderMcpServer } from "./api";

/** Placeholder argument replaced by the active project folder when known. */
export const MCP_PATH_TOKEN = "<project path>";

export type McpPreset = {
  id: string;
  label: string;
  description: string;
  transport: "stdio" | "http";
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
};

/**
 * Starting points, not installers. The user still reviews the command, adds
 * credentials, and saves it; Alfred never downloads a package by itself.
 */
export const MCP_PRESETS: McpPreset[] = [
  {
    id: "github",
    label: "GitHub",
    description: "Issues, pull requests, and repository search.",
    transport: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-github"],
    env: { GITHUB_PERSONAL_ACCESS_TOKEN: "" },
  },
  {
    id: "filesystem",
    label: "Filesystem",
    description: "Read and write files inside one folder.",
    transport: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-filesystem", MCP_PATH_TOKEN],
  },
  {
    id: "git",
    label: "Git",
    description: "Inspect commits, branches, and diffs in a repository.",
    transport: "stdio",
    command: "uvx",
    args: ["mcp-server-git", "--repository", MCP_PATH_TOKEN],
  },
  {
    id: "fetch",
    label: "Fetch",
    description: "Fetch a URL and convert the page to markdown.",
    transport: "stdio",
    command: "uvx",
    args: ["mcp-server-fetch"],
  },
  {
    id: "memory",
    label: "Memory",
    description: "A small local knowledge graph for the agent.",
    transport: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-memory"],
  },
  {
    id: "time",
    label: "Time",
    description: "Current time and time-zone conversions.",
    transport: "stdio",
    command: "uvx",
    args: ["mcp-server-time"],
  },
  {
    id: "sequential-thinking",
    label: "Sequential thinking",
    description: "A staged reasoning scratchpad.",
    transport: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-sequential-thinking"],
  },
  {
    id: "linear",
    label: "Linear",
    description: "Linear's hosted MCP server. Finish OAuth in the agent client.",
    transport: "http",
    url: "https://mcp.linear.app/mcp",
  },
];

export function presetToServer(
  preset: McpPreset,
  suggestedPath?: string | null,
): BuilderMcpServer {
  const folder = suggestedPath?.trim();
  return {
    id: "",
    name: preset.id,
    enabled: true,
    transport: preset.transport,
    command: preset.command ?? "",
    args: (preset.args ?? []).map((argument) =>
      argument === MCP_PATH_TOKEN ? (folder || MCP_PATH_TOKEN) : argument,
    ),
    env: { ...(preset.env ?? {}) },
    url: preset.url ?? "",
    headers: { ...(preset.headers ?? {}) },
  };
}
