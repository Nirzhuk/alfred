import type { ReactNode } from "react";
import { ChatMarkdown } from "../../../builder/components/chat-markdown";
import type { StepInputKind } from "../../types";

// Command and network output keeps its alignment, and a diff's `- ` lines are not a list.
const RAW_KINDS: ReadonlySet<StepInputKind> = new Set(["git", "script", "request"]);

function parseJson(text: string): object | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return null;
  try {
    return JSON.parse(trimmed) as object;
  } catch {
    return null;
  }
}

/** Whether `StepBody` shows anything other than the text as it is. */
export function isFormattedBody(text: string, kind?: StepInputKind) {
  return parseJson(text) !== null || !(kind && RAW_KINDS.has(kind));
}

const isMultiline = (value: unknown) => typeof value === "string" && value.includes("\n");

function JsonValue({ value, depth }: { value: unknown; depth: number }): ReactNode {
  if (typeof value === "string") {
    // Prose inside JSON reads as lines, not as `\n`. Copy and View source keep the original.
    return isMultiline(value) ? (
      <span className="json-text" style={{ marginInlineStart: `${(depth + 1) * 2}ch` }}>
        {value}
      </span>
    ) : (
      <span className="json-string">{JSON.stringify(value)}</span>
    );
  }
  if (typeof value === "number") return <span className="json-number">{value}</span>;
  if (typeof value === "boolean") return <span className="json-boolean">{String(value)}</span>;
  if (value === null || typeof value !== "object") return <span className="json-null">null</span>;

  const isArray = Array.isArray(value);
  const entries = isArray
    ? value.map((item, index) => [String(index), item] as const)
    : Object.entries(value);
  const [open, close] = isArray ? ["[", "]"] : ["{", "}"];
  if (entries.length === 0) return <span className="json-punctuation">{open + close}</span>;

  const indent = "  ".repeat(depth + 1);
  return (
    <>
      <span className="json-punctuation">{open}</span>
      {"\n"}
      {entries.map(([key, item], index) => (
        <span key={key}>
          {indent}
          {isArray ? null : (
            <>
              <span className="json-key">{JSON.stringify(key)}</span>
              <span className="json-punctuation">: </span>
            </>
          )}
          <JsonValue value={item} depth={depth + 1} />
          {/* A multi-line string is its own block: it already ends the line. */}
          {isMultiline(item) ? null : (
            <>
              {index < entries.length - 1 ? <span className="json-punctuation">,</span> : null}
              {"\n"}
            </>
          )}
        </span>
      ))}
      {"  ".repeat(depth)}
      <span className="json-punctuation">{close}</span>
    </>
  );
}

/** A step's text in the viewer: coloured JSON, rendered markdown, or raw output. */
export function StepBody({ text, kind }: { text: string; kind?: StepInputKind }) {
  const json = parseJson(text);
  if (json) {
    return (
      <pre className="output-modal-body">
        <JsonValue value={json} depth={0} />
      </pre>
    );
  }
  if (kind && RAW_KINDS.has(kind)) return <pre className="output-modal-body">{text}</pre>;
  return (
    <div className="output-modal-body output-modal-markdown">
      <ChatMarkdown text={text} />
    </div>
  );
}
