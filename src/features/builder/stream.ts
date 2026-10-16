/**
 * Folds Claude Code `stream-json` events (plus Alfred's own `alfred` lifecycle
 * events) into one assistant message. Pure, so it is replayable in tests.
 */

export type TextPart = { kind: "text"; text: string };
export type ThinkingPart = {
  kind: "thinking";
  text: string;
  startedAt: number;
  endedAt?: number;
};
export type ToolPart = {
  kind: "tool";
  id: string;
  name: string;
  inputJson: string;
  input?: Record<string, unknown>;
  status: "running" | "done" | "error";
  result?: string;
  startedAt: number;
  endedAt?: number;
};
export type Part = TextPart | ThinkingPart | ToolPart;

export type AssistantStatus =
  | "queued"
  | "connecting"
  | "streaming"
  | "done"
  | "error"
  | "cancelled";

export type AssistantMessage = {
  id: string;
  role: "assistant";
  parts: Part[];
  status: AssistantStatus;
  error?: string;
  startedAt: number;
  durationMs?: number;
  costUsd?: number;
  mcpServers?: { name: string; status: string }[];
  /** API block index → part index, for the message currently streaming. */
  blockMap: Record<number, number>;
  /** API message ids whose blocks arrived as partial stream events. */
  streamed: string[];
  currentMessageId?: string;
};

export type UserMessage = {
  id: string;
  role: "user";
  text: string;
  at: number;
  /** Thumbnails of the images sent with this message. */
  images?: string[];
};
export type ChatMessage = UserMessage | AssistantMessage;

type Json = Record<string, unknown>;
const obj = (value: unknown): Json =>
  value && typeof value === "object" ? (value as Json) : {};
const str = (value: unknown): string => (typeof value === "string" ? value : "");

export function newAssistantMessage(id: string, now: number): AssistantMessage {
  return {
    id,
    role: "assistant",
    parts: [],
    status: "queued",
    startedAt: now,
    blockMap: {},
    streamed: [],
  };
}

function parseInput(json: string): Record<string, unknown> | undefined {
  try {
    const value = JSON.parse(json);
    return value && typeof value === "object" ? value : undefined;
  } catch {
    return undefined;
  }
}

function toolResultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((item) => str(obj(item).text))
      .filter(Boolean)
      .join("\n");
  }
  return "";
}

function closeThinking(parts: Part[], now: number): Part[] {
  return parts.map((part) =>
    part.kind === "thinking" && part.endedAt === undefined
      ? { ...part, endedAt: now }
      : part,
  );
}

function blockToPart(block: Json, now: number): Part | null {
  switch (block.type) {
    case "text":
      return { kind: "text", text: str(block.text) };
    case "thinking":
    case "redacted_thinking":
      return { kind: "thinking", text: str(block.thinking), startedAt: now };
    case "tool_use":
    case "server_tool_use": {
      const input = obj(block.input);
      const inputJson = Object.keys(input).length ? JSON.stringify(input) : "";
      return {
        kind: "tool",
        id: str(block.id),
        name: str(block.name),
        inputJson,
        input: Object.keys(input).length ? input : undefined,
        status: "running",
        startedAt: now,
      };
    }
    default:
      return null;
  }
}

export function applyStreamEvent(
  message: AssistantMessage,
  raw: unknown,
  now: number,
): AssistantMessage {
  const event = obj(raw);
  const type = str(event.type);

  if (type === "alfred") {
    switch (event.subtype) {
      case "queued":
        return { ...message, status: "queued" };
      case "started":
        return { ...message, status: "connecting" };
      case "finished": {
        const parts = closeThinking(message.parts, now).map((part) =>
          part.kind === "tool" && part.status === "running"
            ? { ...part, status: "error" as const, endedAt: now }
            : part,
        );
        if (event.cancelled) return { ...message, parts, status: "cancelled" };
        const error = str(event.error);
        if (error) return { ...message, parts, status: "error", error };
        return {
          ...message,
          parts,
          status: message.status === "error" ? "error" : "done",
          durationMs: message.durationMs ?? now - message.startedAt,
        };
      }
      default:
        return message;
    }
  }

  if (type === "system" && event.subtype === "init") {
    const servers = Array.isArray(event.mcp_servers)
      ? event.mcp_servers.map((s) => ({
          name: str(obj(s).name),
          status: str(obj(s).status),
        }))
      : [];
    return { ...message, status: "streaming", mcpServers: servers };
  }

  if (type === "stream_event") {
    const inner = obj(event.event);
    const index = typeof inner.index === "number" ? inner.index : -1;
    switch (inner.type) {
      case "message_start": {
        const id = str(obj(inner.message).id);
        return {
          ...message,
          status: "streaming",
          blockMap: {},
          currentMessageId: id,
          streamed: id ? [...message.streamed, id] : message.streamed,
        };
      }
      case "content_block_start": {
        const part = blockToPart(obj(inner.content_block), now);
        if (!part) return message;
        const parts = closeThinking(message.parts, now);
        return {
          ...message,
          status: "streaming",
          parts: [...parts, part],
          blockMap: { ...message.blockMap, [index]: parts.length },
        };
      }
      case "content_block_delta": {
        const at = message.blockMap[index];
        const part = at === undefined ? undefined : message.parts[at];
        if (!part) return message;
        const delta = obj(inner.delta);
        let next: Part = part;
        if (delta.type === "text_delta" && part.kind === "text") {
          next = { ...part, text: part.text + str(delta.text) };
        } else if (delta.type === "thinking_delta" && part.kind === "thinking") {
          next = { ...part, text: part.text + str(delta.thinking) };
        } else if (delta.type === "input_json_delta" && part.kind === "tool") {
          next = { ...part, inputJson: part.inputJson + str(delta.partial_json) };
        }
        if (next === part) return message;
        const parts = message.parts.slice();
        parts[at] = next;
        return { ...message, parts };
      }
      case "content_block_stop": {
        const at = message.blockMap[index];
        const part = at === undefined ? undefined : message.parts[at];
        if (!part) return message;
        const parts = message.parts.slice();
        if (part.kind === "thinking") parts[at] = { ...part, endedAt: now };
        if (part.kind === "tool") {
          parts[at] = { ...part, input: parseInput(part.inputJson) ?? part.input };
        }
        return { ...message, parts };
      }
      default:
        return message;
    }
  }

  if (type === "assistant") {
    const body = obj(event.message);
    const id = str(body.id);
    const blocks = Array.isArray(body.content) ? body.content.map(obj) : [];
    let parts = message.parts;
    if (id && message.streamed.includes(id)) {
      // Already streamed: only adopt the final, parsed tool inputs.
      parts = parts.map((part) => {
        if (part.kind !== "tool") return part;
        const block = blocks.find((b) => b.type === "tool_use" && b.id === part.id);
        return block ? { ...part, input: obj(block.input) } : part;
      });
      return { ...message, parts };
    }
    const added = blocks
      .map((block) => blockToPart(block, now))
      .filter((part): part is Part => part !== null)
      .filter((part) => part.kind !== "tool" || !parts.some((p) => p.kind === "tool" && p.id === part.id));
    return { ...message, status: "streaming", parts: [...closeThinking(parts, now), ...added] };
  }

  if (type === "user") {
    const content = obj(event.message).content;
    if (!Array.isArray(content)) return message;
    let parts = message.parts;
    for (const item of content.map(obj)) {
      if (item.type !== "tool_result") continue;
      const id = str(item.tool_use_id);
      parts = parts.map((part) =>
        part.kind === "tool" && part.id === id
          ? {
              ...part,
              status: item.is_error ? "error" : "done",
              result: toolResultText(item.content),
              endedAt: now,
            }
          : part,
      );
    }
    return { ...message, parts };
  }

  if (type === "result") {
    const isError = event.is_error === true;
    const errorText = str(event.result).trim();
    let parts = closeThinking(message.parts, now);
    // Claude also prints the error as an assistant line; show it once.
    const last = parts[parts.length - 1];
    if (isError && last?.kind === "text" && last.text.trim() === errorText) parts = parts.slice(0, -1);
    return {
      ...message,
      parts,
      status: isError ? "error" : "done",
      error: isError ? str(event.result) || "The agent stopped with an error." : undefined,
      durationMs: typeof event.duration_ms === "number" ? event.duration_ms : undefined,
      costUsd: typeof event.total_cost_usd === "number" ? event.total_cost_usd : undefined,
    };
  }

  return message;
}

export function isWorking(status: AssistantStatus): boolean {
  return status === "queued" || status === "connecting" || status === "streaming";
}

/** `mcp__alfred__create_workflow` → `{ server: "alfred", tool: "create_workflow" }`. */
export function splitToolName(name: string): { server: string | null; tool: string } {
  const match = /^mcp__(.+?)__(.+)$/.exec(name);
  return match ? { server: match[1], tool: match[2] } : { server: null, tool: name };
}

const ALFRED_LABELS: Record<string, [running: string, done: string]> = {
  list_workflows: ["Reading your workflows", "Read your workflows"],
  get_workflow: ["Opening workflow", "Opened workflow"],
  create_workflow: ["Building workflow", "Built workflow"],
  update_workflow: ["Updating workflow", "Updated workflow"],
  run_workflow: ["Starting a run", "Started a run"],
  get_run: ["Watching the run", "Checked the run"],
  set_schedule: ["Scheduling", "Scheduled"],
  list_agents: ["Checking installed agents", "Checked installed agents"],
  list_skills: ["Looking up skills", "Looked up skills"],
  list_connected_apps: ["Checking connected apps", "Checked connected apps"],
  list_rules: ["Reading rules", "Read rules"],
  add_rule: ["Saving a rule", "Saved a rule"],
  list_mcp_servers: ["Reading MCP servers", "Read MCP servers"],
  add_mcp_server: ["Adding an MCP server", "Added an MCP server"],
};

const BUILTIN_LABELS: Record<string, [string, string]> = {
  Bash: ["Running a command", "Ran a command"],
  Read: ["Reading a file", "Read a file"],
  Write: ["Writing a file", "Wrote a file"],
  Edit: ["Editing a file", "Edited a file"],
  Glob: ["Finding files", "Found files"],
  Grep: ["Searching files", "Searched files"],
  WebFetch: ["Fetching a page", "Fetched a page"],
  WebSearch: ["Searching the web", "Searched the web"],
  TodoWrite: ["Planning", "Planned"],
  Task: ["Delegating to a subagent", "Delegated to a subagent"],
  Agent: ["Delegating to a subagent", "Delegated to a subagent"],
  ToolSearch: ["Loading tools", "Loaded tools"],
};

function humanize(tool: string): string {
  const words = tool.replace(/[_-]+/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function toolLabel(part: ToolPart): string {
  const { server, tool } = splitToolName(part.name);
  const labels = server === "alfred" ? ALFRED_LABELS[tool] : server ? undefined : BUILTIN_LABELS[tool];
  if (labels) return part.status === "running" ? labels[0] : labels[1];
  return server ? `${server} · ${humanize(tool)}` : humanize(tool);
}

/** One short line describing the call's target, from its input. */
export function toolDetail(part: ToolPart): string {
  if (part.name === "ToolSearch") return "";
  const input = part.input ?? parseInput(part.inputJson) ?? {};
  const pick = (...keys: string[]) => keys.map((k) => str(input[k])).find(Boolean) ?? "";
  const value = pick(
    "name",
    "cron",
    "command",
    "file_path",
    "path",
    "pattern",
    "url",
    "query",
    "description",
    "id",
    "runId",
  );
  return value.length > 90 ? `${value.slice(0, 89)}…` : value;
}

export type GraphPreview = {
  workflowId?: string;
  name: string;
  columns: { id: string; type: string; label: string }[][];
  edges: number;
};

/** The workflow a create/update call built, laid out in dependency columns. */
export function workflowPreview(part: ToolPart): GraphPreview | null {
  const { server, tool } = splitToolName(part.name);
  if (server !== "alfred" || (tool !== "create_workflow" && tool !== "update_workflow")) {
    return null;
  }
  if (part.status !== "done") return null;
  const input = part.input ?? {};
  let summary: Json = {};
  try {
    summary = obj(JSON.parse(part.result ?? ""));
  } catch {
    // The preview still renders from the call input.
  }
  const graph = obj(input.graph);
  const nodes = Array.isArray(graph.nodes) ? graph.nodes.map(obj) : [];
  const edges = Array.isArray(graph.edges) ? graph.edges.map(obj) : [];
  if (nodes.length === 0) return null;
  const ids = nodes.map((n, i) => str(n.id) || `${str(n.type)}-${i + 1}`);
  const column = new Map(ids.map((id) => [id, 0]));
  // Longest path, bounded by node count so a cycle cannot spin forever.
  for (let pass = 0; pass < ids.length; pass += 1) {
    let moved = false;
    for (const edge of edges) {
      const from = column.get(str(edge.source));
      const to = column.get(str(edge.target));
      if (from === undefined || to === undefined || to > from) continue;
      column.set(str(edge.target), from + 1);
      moved = true;
    }
    if (!moved) break;
  }
  const columns: GraphPreview["columns"] = [];
  nodes.forEach((node, i) => {
    const c = column.get(ids[i]) ?? 0;
    (columns[c] ??= []).push({
      id: ids[i],
      type: str(node.type),
      label: str(obj(node.data).label) || humanize(str(node.type)),
    });
  });
  return {
    workflowId: str(summary.id) || str(input.id) || undefined,
    name: str(summary.name) || str(input.name) || "Workflow",
    columns: columns.filter(Boolean),
    edges: edges.length,
  };
}
