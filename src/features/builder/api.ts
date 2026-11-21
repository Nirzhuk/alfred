import { invoke } from "@tauri-apps/api/core";

export type BuilderRule = {
  id: string;
  name: string;
  body: string;
  enabled: boolean;
};

export type BuilderMcpServer = {
  id: string;
  name: string;
  enabled: boolean;
  transport: "stdio" | "http";
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
};

export type BuilderConfig = {
  rules: BuilderRule[];
  mcpServers: BuilderMcpServer[];
};

export type BuilderAgentId = "claude_code" | "codex";

export type BuilderAgentInfo = {
  id: BuilderAgentId;
  label: string;
  installed: boolean;
};

export type BuilderInfo = {
  /** Which identity this build runs as: isolated data dir and MCP name. */
  build: "development" | "production";
  dataDir: string | null;
  agents: BuilderAgentInfo[];
  mcpReady: boolean;
  tools: { name: string; description: string }[];
  /** How other agents launch Alfred's stdio MCP bridge. */
  externalMcp: {
    name: string;
    command: string;
    config: { mcpServers: Record<string, unknown> } | null;
  } | null;
};

export type McpCheck = {
  ok: boolean;
  detail: string;
};

export type BuilderTurnResult = {
  sessionId: string | null;
  cancelled: boolean;
  error: string | null;
};

export const STREAM_EVENT = "builder://stream";
export const WORKFLOW_CHANGED_EVENT = "builder://workflow-changed";
export const CONFIG_CHANGED_EVENT = "builder://config-changed";

export function getBuilderConfig(): Promise<BuilderConfig> {
  return invoke("builder_get_config");
}

export function saveBuilderConfig(config: BuilderConfig): Promise<BuilderConfig> {
  return invoke("builder_save_config", { config });
}

export function getBuilderInfo(): Promise<BuilderInfo> {
  return invoke("builder_info");
}

/** Registers Alfred in Claude Code (user scope). Resolves to the server name. */
export function connectClaudeCode(): Promise<string> {
  return invoke("builder_connect_claude");
}

/** Check a draft server without saving it or starting it. */
export function checkMcpServer(server: BuilderMcpServer): Promise<McpCheck> {
  return invoke("builder_check_mcp_server", { server });
}

export type SkillSyncResult = {
  linked: number;
  copied: number;
  removed: number;
  skipped: string[];
};

export type AlfredSkill = {
  name: string;
  path: string;
  linked: boolean;
};

/** The shared .agents/skills library and its Claude Code link state. */
export function listAlfredSkills(): Promise<AlfredSkill[]> {
  return invoke("builder_alfred_skills");
}

/** Link Alfred's shared .agents/skills into Claude Code's user skills folder. */
export function syncAlfredSkills(): Promise<SkillSyncResult> {
  return invoke("builder_sync_claude_skills");
}

export type BuilderImage = { mediaType: string; data: string };

export function sendBuilderMessage(input: {
  chatId: string;
  message: string;
  agent: BuilderAgentId;
  model?: string | null;
  sessionId?: string | null;
  images?: BuilderImage[];
}): Promise<BuilderTurnResult> {
  return invoke("builder_send", { input });
}

export function cancelBuilderTurn(chatId: string): Promise<boolean> {
  return invoke("builder_cancel", { chatId });
}

export type ChatSummary = {
  id: string;
  title: string;
  agent: string;
  updatedAt: number;
};

export function listBuilderChats(): Promise<ChatSummary[]> {
  return invoke("builder_list_chats");
}

export function loadBuilderChat<T>(id: string): Promise<T> {
  return invoke("builder_load_chat", { id });
}

export function saveBuilderChat(chat: object): Promise<void> {
  return invoke("builder_save_chat", { chat });
}

export function deleteBuilderChat(id: string): Promise<void> {
  return invoke("builder_delete_chat", { id });
}

export type DroppedImage = { name: string; mediaType: string; data: string };

export function readBuilderImage(path: string): Promise<DroppedImage> {
  return invoke("builder_read_image", { path });
}
