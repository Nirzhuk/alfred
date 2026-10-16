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
  agents: BuilderAgentInfo[];
  mcpReady: boolean;
  tools: { name: string; description: string }[];
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

export function sendBuilderMessage(input: {
  chatId: string;
  message: string;
  agent: BuilderAgentId;
  model?: string | null;
  sessionId?: string | null;
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
