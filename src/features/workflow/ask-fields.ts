import type { WorkflowNode } from "./types";

/** `{{ask:Name}}` or `{{ask:Name|default}}`; keep in sync with `runner/ask.rs`. */
const ASK = /\{\{\s*ask:([^}|]+?)\s*(?:\|([^}]*))?\}\}/g;

export type AskField = { name: string; defaultValue: string };

/** Placeholders the Run dialog asks for, deduped by name in canvas order. */
export function askFields(nodes: WorkflowNode[]): AskField[] {
  const fields = new Map<string, AskField>();
  for (const node of nodes) {
    const data = node.data as { prompt?: unknown; template?: unknown };
    const text =
      node.type === "input" || node.type === "prompt"
        ? data.prompt
        : node.type === "template"
          ? data.template
          : null;
    if (typeof text !== "string") continue;
    for (const [, rawName, rawDefault] of text.matchAll(ASK)) {
      const name = rawName.trim();
      if (!fields.has(name)) {
        fields.set(name, { name, defaultValue: (rawDefault ?? "").trim() });
      }
    }
  }
  return [...fields.values()];
}
