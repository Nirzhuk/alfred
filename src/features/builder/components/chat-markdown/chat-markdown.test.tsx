import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatMarkdown, parseBlocks } from "./chat-markdown";

test("parses the block subset agents write", () => {
  const blocks = parseBlocks(
    "## Done\n\n1. Input\n2. Agent\n\n- a\n- b\n\n```json\n{\"a\":1}\n```\n> note\n\ntext\nmore",
  );
  expect(blocks.map((b) => b.kind)).toEqual([
    "heading",
    "list",
    "list",
    "code",
    "quote",
    "paragraph",
  ]);
});

test("keeps an unterminated fence while streaming", () => {
  expect(parseBlocks("```bash\necho hi")).toEqual([
    { kind: "code", lang: "bash", text: "echo hi" },
  ]);
});

test("renders inline marks and never raw html", () => {
  const html = renderToStaticMarkup(
    <ChatMarkdown text={"**Built** `daily-digest` *now* [docs](https://x.dev) <img src=x onerror=1>"} />,
  );
  expect(html).toContain("<strong>Built</strong>");
  expect(html).toContain("<code>daily-digest</code>");
  expect(html).toContain("<em>now</em>");
  expect(html).toContain('href="https://x.dev"');
  expect(html).toContain("&lt;img");
});

test("drops non-http links to plain text", () => {
  const html = renderToStaticMarkup(<ChatMarkdown text="[x](javascript:alert)" />);
  expect(html).not.toContain("href");
});
