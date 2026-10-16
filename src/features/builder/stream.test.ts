import { expect, test } from "bun:test";
import {
  applyStreamEvent,
  newAssistantMessage,
  toolDetail,
  toolLabel,
  workflowPreview,
  type ToolPart,
} from "./stream";

const graph = {
  nodes: [
    { id: "goal", type: "input", data: { label: "Goal" } },
    { id: "for", type: "agent", data: { label: "For", provider: "codex" } },
    { id: "against", type: "agent", data: { label: "Against", provider: "claude_code" } },
    { id: "judge", type: "agent", data: { label: "Judge", provider: "claude_code" } },
    { id: "out", type: "chooseOutput", data: { label: "Verdict" } },
  ],
  edges: [
    { source: "goal", target: "for" },
    { source: "goal", target: "against" },
    { source: "for", target: "judge" },
    { source: "against", target: "judge" },
    { source: "judge", target: "out" },
  ],
};
const input = JSON.stringify({ name: "Debate", graph });

const se = (event: object) => ({ type: "stream_event", event });

/** Shape captured from `claude -p --output-format stream-json --include-partial-messages`. */
const transcript = [
  { type: "alfred", subtype: "queued" },
  { type: "alfred", subtype: "started" },
  { type: "system", subtype: "init", session_id: "s1", mcp_servers: [{ name: "alfred", status: "connected" }] },
  se({ type: "message_start", message: { id: "m1" } }),
  se({ type: "content_block_start", index: 0, content_block: { type: "thinking", thinking: "" } }),
  se({ type: "content_block_delta", index: 0, delta: { type: "thinking_delta", thinking: "plan" } }),
  se({ type: "content_block_stop", index: 0 }),
  se({ type: "content_block_start", index: 1, content_block: { type: "text", text: "" } }),
  se({ type: "content_block_delta", index: 1, delta: { type: "text_delta", text: "Building " } }),
  se({ type: "content_block_delta", index: 1, delta: { type: "text_delta", text: "it." } }),
  se({ type: "content_block_stop", index: 1 }),
  se({ type: "content_block_start", index: 2, content_block: { type: "tool_use", id: "t1", name: "mcp__alfred__create_workflow", input: {} } }),
  se({ type: "content_block_delta", index: 2, delta: { type: "input_json_delta", partial_json: input.slice(0, 20) } }),
  se({ type: "content_block_delta", index: 2, delta: { type: "input_json_delta", partial_json: input.slice(20) } }),
  { type: "assistant", message: { id: "m1", content: [{ type: "tool_use", id: "t1", name: "mcp__alfred__create_workflow", input: JSON.parse(input) }] } },
  se({ type: "content_block_stop", index: 2 }),
  se({ type: "message_stop" }),
  { type: "user", message: { content: [{ type: "tool_result", tool_use_id: "t1", content: [{ type: "text", text: '{"id":"wf-1","name":"Debate"}' }], is_error: false }] } },
  se({ type: "message_start", message: { id: "m2" } }),
  se({ type: "content_block_start", index: 0, content_block: { type: "text", text: "" } }),
  se({ type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "Done." } }),
  { type: "result", subtype: "success", is_error: false, duration_ms: 4200, total_cost_usd: 0.02 },
  { type: "alfred", subtype: "finished", cancelled: false, error: null },
];

test("replays a create_workflow turn into ordered parts", () => {
  let message = newAssistantMessage("a1", 0);
  const statuses: string[] = [];
  transcript.forEach((event, i) => {
    message = applyStreamEvent(message, event, i);
    statuses.push(message.status);
  });

  expect(statuses.slice(0, 3)).toEqual(["queued", "connecting", "streaming"]);
  expect(message.status).toBe("done");
  expect(message.durationMs).toBe(4200);
  expect(message.mcpServers).toEqual([{ name: "alfred", status: "connected" }]);
  expect(message.parts.map((p) => p.kind)).toEqual(["thinking", "text", "tool", "text"]);
  expect(message.parts[1]).toMatchObject({ text: "Building it." });
  expect(message.parts[3]).toMatchObject({ text: "Done." });

  const tool = message.parts[2] as ToolPart;
  expect(tool.status).toBe("done");
  expect(tool.input).toMatchObject({ name: "Debate" });
  expect(toolLabel(tool)).toBe("Built workflow");
  expect(toolDetail(tool)).toBe("Debate");

  const preview = workflowPreview(tool);
  expect(preview?.workflowId).toBe("wf-1");
  expect(preview?.columns.map((c) => c.map((n) => n.id))).toEqual([
    ["goal"],
    ["for", "against"],
    ["judge"],
    ["out"],
  ]);
});

test("a cancelled turn settles running tools", () => {
  let message = newAssistantMessage("a1", 0);
  message = applyStreamEvent(message, se({ type: "message_start", message: { id: "m" } }), 1);
  message = applyStreamEvent(
    message,
    se({ type: "content_block_start", index: 0, content_block: { type: "tool_use", id: "t", name: "Bash", input: {} } }),
    2,
  );
  expect(toolLabel(message.parts[0] as ToolPart)).toBe("Running a command");
  message = applyStreamEvent(message, { type: "alfred", subtype: "finished", cancelled: true }, 3);
  expect(message.status).toBe("cancelled");
  expect((message.parts[0] as ToolPart).status).toBe("error");
});

test("non-streamed assistant messages still produce parts, and errors surface", () => {
  let message = newAssistantMessage("a1", 0);
  message = applyStreamEvent(message, { type: "assistant", message: { id: "x", content: [{ type: "text", text: "hi" }] } }, 1);
  expect(message.parts).toEqual([{ kind: "text", text: "hi" }]);
  message = applyStreamEvent(message, { type: "result", is_error: true, result: "Usage limit reached" }, 2);
  expect(message).toMatchObject({ status: "error", error: "Usage limit reached" });
});

test("labels third-party MCP tools by server", () => {
  const part: ToolPart = { kind: "tool", id: "1", name: "mcp__linear__create_issue", inputJson: "", status: "running", startedAt: 0 };
  expect(toolLabel(part)).toBe("linear · Create issue");
});
