import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { applyStreamEvent, newAssistantMessage } from "../../stream";
import { activityLabel, BuilderPage } from "./builder-page";

test("empty chat shows the starters and composer", () => {
  const html = renderToStaticMarkup(<BuilderPage onOpenWorkflow={() => {}} />);
  expect(html).toContain("What should Alfred automate?");
  expect(html).toContain("Agent debate");
  expect(html).toContain('aria-label="Message the Workflow Builder"');
  expect(html).toContain('aria-label="Chat history"');
  expect(html).toContain('aria-label="Agent"');
  expect(html).toContain("alfred-orb is-hero");
});

test("activity label follows the agent from queue to tool call", () => {
  let message = newAssistantMessage("a", 0);
  expect(activityLabel(message, 100)).toBe("Waking Alfred");
  expect(activityLabel(message, 5000)).toBe("Waiting for another Claude run to finish");
  message = applyStreamEvent(message, { type: "alfred", subtype: "started" }, 1);
  expect(activityLabel(message, 2)).toBe("Connecting to Alfred's API");
  message = applyStreamEvent(message, { type: "system", subtype: "init", mcp_servers: [] }, 2);
  expect(activityLabel(message, 3)).toBe("Thinking");
  message = applyStreamEvent(
    message,
    { type: "stream_event", event: { type: "content_block_start", index: 0, content_block: { type: "tool_use", id: "t", name: "mcp__alfred__create_workflow", input: { name: "Digest" } } } },
    3,
  );
  expect(activityLabel(message, 4)).toBe("Building workflow · Digest");
});
