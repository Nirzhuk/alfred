import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { BuilderChatList, relativeTime } from "./builder-chat-list";

test("formats recency in short relative units", () => {
  const now = Date.UTC(2026, 9, 16, 12);
  expect(relativeTime(now - 10_000, now)).toBe("just now");
  expect(relativeTime(now - 5 * 60_000, now)).toContain("5");
  expect(relativeTime(now - 3 * 86_400_000, now)).toContain("3");
});

test("marks the open chat and names delete buttons", () => {
  const html = renderToStaticMarkup(
    <BuilderChatList
      chats={[
        { id: "a", title: "Morning digest", agent: "claude_code", updatedAt: Date.now() },
        { id: "b", title: "PR review", agent: "codex", updatedAt: Date.now() - 86_400_000 },
      ]}
      activeId="a"
      disabled={false}
      onOpen={() => {}}
      onDelete={() => {}}
    />,
  );
  expect(html).toContain('aria-current="true"');
  expect(html).toContain("Morning digest");
  expect(html).toContain('aria-label="Delete chat PR review"');
  expect(html).toContain("agent-mark-codex");
});

test("explains the empty history", () => {
  const html = renderToStaticMarkup(
    <BuilderChatList chats={[]} activeId="" disabled={false} onOpen={() => {}} onDelete={() => {}} />,
  );
  expect(html).toContain("appear here");
});
