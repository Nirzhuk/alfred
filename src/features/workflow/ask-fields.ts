import type { WorkflowNode } from "./types";

/** `{{ask:Name}}` or `{{ask:Name|default}}`; keep in sync with `runner/ask.rs`. */
const ASK = /\{\{\s*ask:([^}|]+?)\s*(?:\|([^}]*))?\}\}/g;

/** A canvas step that consumes a placeholder. */
export type AskSource = {
  nodeId: string;
  /** The step's label as shown on the canvas. */
  label: string;
  type: "input" | "template";
};

export type AskField = {
  name: string;
  defaultValue: string;
  /**
   * True when an occurrence has no `|default`. An empty answer would leave at
   * least one occurrence unresolved, so the run window requires a value.
   */
  required: boolean;
  /** Steps that use this placeholder, in canvas order, deduped by node. */
  sources: AskSource[];
};

type AskAccumulator = {
  name: string;
  required: boolean;
  /** First default seen, used only when every occurrence has one. */
  defaultValue: string;
  hasDefault: boolean;
  sources: AskSource[];
};

function placeholderText(
  node: WorkflowNode,
): { text: string; type: AskSource["type"]; label: string } | null {
  const data = node.data as { prompt?: unknown; template?: unknown; label?: unknown };
  const type =
    node.type === "input" || node.type === "prompt"
      ? "input"
      : node.type === "template"
        ? "template"
        : null;
  if (!type) return null;
  const text = type === "input" ? data.prompt : data.template;
  if (typeof text !== "string") return null;
  const label =
    typeof data.label === "string" && data.label.trim()
      ? data.label.trim()
      : type === "template"
        ? "Template"
        : "Input";
  return { text, type, label };
}

/**
 * Placeholders the Run dialog asks for, deduped by name in canvas order.
 *
 * A name is required when any occurrence has no default. Its default is only
 * prefilled when every occurrence has one, because one answer is applied to
 * every occurrence.
 */
export function askFields(nodes: WorkflowNode[]): AskField[] {
  const fields = new Map<string, AskAccumulator>();
  for (const node of nodes) {
    const source = placeholderText(node);
    if (!source) continue;
    for (const [, rawName, rawDefault] of source.text.matchAll(ASK)) {
      const name = rawName.trim();
      let field = fields.get(name);
      if (!field) {
        field = {
          name,
          required: false,
          defaultValue: "",
          hasDefault: false,
          sources: [],
        };
        fields.set(name, field);
      }
      if (rawDefault === undefined) {
        field.required = true;
      } else if (!field.hasDefault) {
        field.hasDefault = true;
        field.defaultValue = rawDefault.trim();
      }
      if (!field.sources.some((item) => item.nodeId === node.id)) {
        field.sources.push({ nodeId: node.id, label: source.label, type: source.type });
      }
    }
  }
  return [...fields.values()].map(({ name, defaultValue, required, sources }) => ({
    name,
    defaultValue: required ? "" : defaultValue,
    required,
    sources,
  }));
}

/** Required names that would still resolve to empty text. */
export function missingRequired(
  fields: AskField[],
  answers: Record<string, string>,
): string[] {
  return fields
    .filter((field) => field.required && !(answers[field.name] ?? "").trim())
    .map((field) => field.name);
}

/** Placeholder names in one text, deduped, for the settings preview. */
export function askNames(text: string): string[] {
  return [...new Set([...text.matchAll(ASK)].map(([, name]) => name.trim()))];
}
