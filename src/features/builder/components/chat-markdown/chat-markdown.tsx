import { openUrl } from "@tauri-apps/plugin-opener";
import { Fragment, type ReactNode } from "react";

// ponytail: the subset agents actually write (headings, lists, quotes, code,
// tables-as-text, bold/italic/code/links). Swap for a full parser if replies
// start needing nested lists or real tables.

const INLINE = /(`[^`\n]+`|\*\*[^*\n]+\*\*|\*[^*\s][^*\n]*\*|\[[^\]\n]+\]\([^)\s]+\))/g;

function inline(text: string, keyPrefix: string): ReactNode[] {
  return text.split(INLINE).map((chunk, i) => {
    const key = `${keyPrefix}-${i}`;
    if (chunk.startsWith("`") && chunk.endsWith("`") && chunk.length > 1) {
      return <code key={key}>{chunk.slice(1, -1)}</code>;
    }
    if (chunk.startsWith("**") && chunk.endsWith("**") && chunk.length > 4) {
      return <strong key={key}>{chunk.slice(2, -2)}</strong>;
    }
    if (chunk.startsWith("*") && chunk.endsWith("*") && chunk.length > 2) {
      return <em key={key}>{chunk.slice(1, -1)}</em>;
    }
    const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(chunk);
    if (link && /^https?:\/\//.test(link[2])) {
      return (
        <a
          key={key}
          href={link[2]}
          onClick={(event) => {
            event.preventDefault();
            void openUrl(link[2]).catch(() => undefined);
          }}
        >
          {link[1]}
        </a>
      );
    }
    return <Fragment key={key}>{chunk}</Fragment>;
  });
}

type Block =
  | { kind: "code"; lang: string; text: string }
  | { kind: "heading"; level: number; text: string }
  | { kind: "list"; ordered: boolean; items: string[] }
  | { kind: "quote"; text: string }
  | { kind: "rule" }
  | { kind: "paragraph"; text: string };

export function parseBlocks(source: string): Block[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = /^\s*```(\S*)/.exec(line);
    if (fence) {
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !/^\s*```/.test(lines[i])) body.push(lines[i++]);
      i += 1; // closing fence (or end of a still-streaming block)
      blocks.push({ kind: "code", lang: fence[1], text: body.join("\n") });
      continue;
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2] });
      i += 1;
      continue;
    }
    if (/^\s*([-*_])\s*\1\s*\1[\s\-*_]*$/.test(line)) {
      blocks.push({ kind: "rule" });
      i += 1;
      continue;
    }
    const item = /^\s*([-*+]|\d+[.)])\s+(.*)$/.exec(line);
    if (item) {
      const ordered = /\d/.test(item[1]);
      const items: string[] = [];
      while (i < lines.length) {
        const next = /^\s*([-*+]|\d+[.)])\s+(.*)$/.exec(lines[i]);
        if (next && /\d/.test(next[1]) === ordered) {
          items.push(next[2]);
        } else if (lines[i].trim() && /^\s{2,}/.test(lines[i]) && items.length) {
          items[items.length - 1] += ` ${lines[i].trim()}`;
        } else {
          break;
        }
        i += 1;
      }
      blocks.push({ kind: "list", ordered, items });
      continue;
    }
    if (/^\s*>/.test(line)) {
      const body: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) {
        body.push(lines[i++].replace(/^\s*>\s?/, ""));
      }
      blocks.push({ kind: "quote", text: body.join(" ") });
      continue;
    }
    if (!line.trim()) {
      i += 1;
      continue;
    }
    const body: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^\s*(```|#{1,4}\s|>|[-*+]\s|\d+[.)]\s)/.test(lines[i])
    ) {
      body.push(lines[i++]);
    }
    blocks.push({ kind: "paragraph", text: body.join("\n") });
  }
  return blocks;
}

export function ChatMarkdown({ text }: { text: string }) {
  return (
    <div className="chat-markdown user-select-text">
      {parseBlocks(text).map((block, i) => {
        const key = `b${i}`;
        switch (block.kind) {
          case "code":
            return (
              <pre key={key} data-lang={block.lang || undefined}>
                <code>{block.text}</code>
              </pre>
            );
          case "heading":
            return block.level <= 2 ? (
              <h3 key={key}>{inline(block.text, key)}</h3>
            ) : (
              <h4 key={key}>{inline(block.text, key)}</h4>
            );
          case "list": {
            const items = block.items.map((item, j) => (
              <li key={j}>{inline(item, `${key}-${j}`)}</li>
            ));
            return block.ordered ? <ol key={key}>{items}</ol> : <ul key={key}>{items}</ul>;
          }
          case "quote":
            return <blockquote key={key}>{inline(block.text, key)}</blockquote>;
          case "rule":
            return <hr key={key} />;
          default:
            return (
              <p key={key}>
                {block.text.split("\n").map((line, j) => (
                  <Fragment key={j}>
                    {j > 0 ? <br /> : null}
                    {inline(line, `${key}-${j}`)}
                  </Fragment>
                ))}
              </p>
            );
        }
      })}
    </div>
  );
}
