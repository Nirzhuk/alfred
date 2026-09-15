export const FORMAT = "alfred.workflow-template" as const;
export const SCHEMA_VERSION = 1 as const;
export const MAX_BYTES = 256 * 1024;
export const MAX_NODES = 100;
export const MAX_EDGES = 300;
export const MAX_PARAMETERS = 64;
export const MAX_STARTS = 16;
export const MAX_DEPTH = 16;
const NODE_TYPES = new Set(["input", "prompt", "agent", "customAgent", "chooseOutput", "output", "memory", "identity", "template", "fileInject", "gitStatus", "shell", "script", "http", "notify", "writeFile", "gitHost", "appAction"]);
const NODE_FIELDS: Record<string, string[]> = {
  input: ["label", "prompt", "blocked", "script", "attachments"], prompt: ["label", "prompt", "blocked", "script", "attachments"],
  agent: ["label", "provider", "harness", "model", "skillNames", "skillName"], memory: ["label", "memoryIds"], identity: ["kind", "label", "role", "personality", "instructions"], template: ["kind", "label", "template", "mode"], fileInject: ["kind", "label", "paths"], gitStatus: ["kind", "label", "includeDiff"], shell: ["kind", "label", "command", "appendOutput"], script: ["kind", "label", "source", "path", "body", "interpreter", "appendOutput"], http: ["kind", "label", "method", "url", "body", "headers"], notify: ["kind", "label", "title", "body", "desktop", "webhookUrl"], writeFile: ["kind", "label", "path", "content"], gitHost: ["kind", "label", "action", "repo", "title", "body", "base", "head", "draft"], customAgent: ["kind", "label", "command", "promptMode"], chooseOutput: ["label", "saveToMemory", "pinMemory", "memoryTitle", "asFinalResult", "htmlReport", "includeFilesChanged"], output: ["label", "saveToMemory", "pinMemory", "memoryTitle", "asFinalResult", "htmlReport", "includeFilesChanged"], appAction: ["type", "label", "providerId", "actionId", "connectionId", "input"],
};
const PARAMETER_KINDS = new Set(["text", "folder", "file", "agentAccount", "appConnection", "appResource", "memory", "channel", "email", "secretBacked"]);
const TARGET_KINDS = new Set(["node", "start", "workingDirectory"]);
const REQUIREMENT_KINDS = new Set(["agent", "harness", "model", "skill", "interpreter", "appAction", "appEvent"]);
const START_KINDS = new Set(["schedule", "file", "appEvent", "webhook"]);

export type Template = {
  format: typeof FORMAT;
  schemaVersion: 1;
  minAlfredVersion: string;
  title: string;
  description: string;
  graph: { nodes: Array<{ id: string; type: string; data: Record<string, unknown>; position: { x: number; y: number } }>; edges: Array<{ id: string; source: string; target: string; sourceHandle?: string | null; targetHandle?: string | null }> };
  parameters: Array<{ key: string; label: string; description?: string; kind: string; required: boolean; allowedProvider?: string | null; allowedAction?: string | null; target: { kind: string; nodeId: string | null; field: string }; targets?: Array<{ kind: string; nodeId: string | null; field: string }> }>;
  requirements: Array<{ kind: string; name: string; provider?: string | null; capability?: string | null }>;
  starts: Array<{ kind: string; config: Record<string, unknown> }>;
};

export class ContractError extends Error {}

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new ContractError(`${label} must be an object`);
  return value as Record<string, unknown>;
}

function keysOnly(value: Record<string, unknown>, allowed: readonly string[], label: string): void {
  for (const key of Object.keys(value)) if (!allowed.includes(key)) throw new ContractError(`unsupported field ${label}.${key}`);
}

function version(value: string): [number, number, number] | null {
  const parts = value.split("."); if (parts.length > 3 || parts.some((part) => !/^\d+$/.test(part))) return null;
  return [Number(parts[0]), Number(parts[1] ?? 0), Number(parts[2] ?? 0)];
}

function stringArray(value: unknown): boolean { return Array.isArray(value) && value.every((item) => typeof item === "string"); }

function depth(value: unknown, current = 0): number {
  if (Array.isArray(value)) return Math.max(current, ...value.map((item) => depth(item, current + 1)));
  if (value && typeof value === "object") return Math.max(current, ...Object.values(value).map((item) => depth(item, current + 1)));
  return current;
}

function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).filter(([key, item]) => !(item === null && ["sourceHandle", "targetHandle", "allowedProvider", "allowedAction", "provider", "capability"].includes(key))).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([key, item]) => [key, sorted(item)]));
  return value;
}

export function canonicalBytes(template: Template): Uint8Array {
  validateTemplate(template);
  const normalized = {
    ...template,
    parameters: template.parameters.map((parameter) => ({
      ...parameter,
      description: parameter.description ?? "",
      targets: parameter.targets ?? [],
    })),
  };
  return new TextEncoder().encode(JSON.stringify(sorted(normalized)));
}

export async function digest(template: Template): Promise<string> {
  const hash = await crypto.subtle.digest("SHA-256", canonicalBytes(template));
  return [...new Uint8Array(hash)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function parseTemplate(value: unknown): Template {
  if (!value || typeof value !== "object") throw new ContractError("template must be an object");
  const template = value as Template;
  validateTemplate(template);
  return template;
}

export function validateTemplate(template: Template): void {
  const envelope = record(template, "template");
  keysOnly(envelope, ["format", "schemaVersion", "minAlfredVersion", "title", "description", "graph", "parameters", "requirements", "starts"], "template");
  if (!Array.isArray(template.parameters) || !Array.isArray(template.requirements) || !Array.isArray(template.starts)) throw new ContractError("parameters, requirements, and starts are required arrays");
  if (template.format !== FORMAT || template.schemaVersion !== SCHEMA_VERSION) throw new ContractError("unsupported template format or schema version");
  if (typeof template.minAlfredVersion !== "string" || template.minAlfredVersion.length === 0 || template.minAlfredVersion.length > 32) throw new ContractError("invalid minimum Alfred version");
  const minimum = version(template.minAlfredVersion); if (!minimum || minimum[0] > 1 || (minimum[0] === 1 && (minimum[1] > 0 || minimum[2] > 0))) throw new ContractError("template requires a newer Alfred version");
  if (typeof template.title !== "string" || template.title.trim().length === 0 || template.title.length > 120) throw new ContractError("title must be 1-120 characters");
  if (typeof template.description !== "string" || template.description.length > 2000) throw new ContractError("description exceeds 2,000 characters");
  if (!template.graph || !Array.isArray(template.graph.nodes) || !Array.isArray(template.graph.edges)) throw new ContractError("graph nodes and edges are required");
  keysOnly(record(template.graph, "graph"), ["nodes", "edges"], "graph");
  if (template.graph.nodes.length > MAX_NODES || template.graph.edges.length > MAX_EDGES || template.parameters.length > MAX_PARAMETERS || template.starts.length > MAX_STARTS) throw new ContractError("template exceeds a size limit");
  const ids = new Set<string>();
  for (const node of template.graph.nodes) {
    const nodeRecord = record(node, `node ${node?.id ?? ""}`); keysOnly(nodeRecord, ["id", "type", "data", "position"], `node ${String(node?.id ?? "")}`);
    if (!/^[A-Za-z0-9_-]{1,96}$/.test(node.id) || ids.has(node.id)) throw new ContractError("node IDs must be unique and bounded");
    ids.add(node.id);
    if (!NODE_TYPES.has(node.type)) throw new ContractError(`unsupported node type ${node.type}`);
    if (!node.data || typeof node.data !== "object" || Array.isArray(node.data)) throw new ContractError(`node ${node.id} data must be an object`);
    for (const field of Object.keys(node.data)) if (!NODE_FIELDS[node.type]?.includes(field)) throw new ContractError(`unsupported execution field ${node.id}.${field}`);
    validateNodeValues(node.type, node.data, node.id);
    if (node.type === "appAction" && typeof node.data.connectionId === "string" && node.data.connectionId.length > 0) throw new ContractError(`node ${node.id} retains a local connection`);
    if (node.type === "agent" && typeof node.data.accountRef === "string" && node.data.accountRef.trim().length > 0) throw new ContractError(`node ${node.id} retains a local agent account`);
    if (node.type === "memory" && Array.isArray(node.data.memoryIds) && node.data.memoryIds.length > 0) throw new ContractError(`node ${node.id} retains a local memory`);
    if (node.type === "fileInject" && Array.isArray(node.data.paths) && node.data.paths.length > 0) throw new ContractError(`node ${node.id} retains a local path`);
    const position = record(node.position, `node ${node.id}.position`); keysOnly(position, ["x", "y"], `node ${node.id}.position`);
    if (typeof position.x !== "number" || typeof position.y !== "number" || !Number.isFinite(node.position.x) || !Number.isFinite(node.position.y)) throw new ContractError(`node ${node.id} has a non-finite position`);
  }
  const edgeIds = new Set<string>();
  for (const edge of template.graph.edges) { const edgeRecord = record(edge, "edge"); keysOnly(edgeRecord, ["id", "source", "target", "sourceHandle", "targetHandle"], "edge"); if (typeof edge.id !== "string" || edgeIds.has(edge.id)) throw new ContractError("edge IDs must be unique"); edgeIds.add(edge.id); if (edge.source === edge.target || !ids.has(edge.source) || !ids.has(edge.target)) throw new ContractError("edge references an invalid node"); for (const handle of [edge.sourceHandle, edge.targetHandle]) if (handle !== undefined && handle !== null && (typeof handle !== "string" || handle.length === 0 || handle.length > 96)) throw new ContractError("edge handle is out of bounds"); }
  const parents = new Map<string, string[]>();
  for (const edge of template.graph.edges) parents.set(edge.target, [...(parents.get(edge.target) ?? []), edge.source]);
  const visiting = new Set<string>(); const visited = new Set<string>();
  const visit = (id: string): boolean => { if (visiting.has(id)) return false; if (visited.has(id)) return true; visiting.add(id); if ((parents.get(id) ?? []).some((parent) => !visit(parent))) return false; visiting.delete(id); visited.add(id); return true; };
  for (const id of ids) if (!visit(id)) throw new ContractError("graph must be acyclic");
  const parameterKeys = new Set<string>();
  for (const parameter of template.parameters) {
    const parameterRecord = record(parameter, "parameter"); keysOnly(parameterRecord, ["key", "label", "description", "kind", "required", "allowedProvider", "allowedAction", "target", "targets"], "parameter");
    if (!/^[a-z][a-zA-Z0-9_.-]{0,63}$/.test(parameter.key) || parameterKeys.has(parameter.key)) throw new ContractError("parameter keys must be unique and bounded");
    parameterKeys.add(parameter.key);
    if (!parameter.label || parameter.label.length > 120 || ("description" in parameter && (typeof parameter.description !== "string" || parameter.description.length > 500)) || ("allowedProvider" in parameter && parameter.allowedProvider !== null && typeof parameter.allowedProvider !== "string") || ("allowedAction" in parameter && parameter.allowedAction !== null && typeof parameter.allowedAction !== "string") || (typeof parameter.allowedProvider === "string" && parameter.allowedProvider.length > 96) || (typeof parameter.allowedAction === "string" && parameter.allowedAction.length > 96) || !PARAMETER_KINDS.has(parameter.kind) || typeof parameter.required !== "boolean" || !parameter.target || !record(parameter.target, "parameter target") || !TARGET_KINDS.has(parameter.target.kind) || !parameter.target.field || parameter.target.field.length > 128 || parameter.key === "__proto__") throw new ContractError(`invalid parameter ${parameter.key}`);
    if ("targets" in parameter && (!Array.isArray(parameter.targets) || parameter.targets.length > 16)) throw new ContractError(`invalid parameter targets ${parameter.key}`);
    for (const target of [parameter.target, ...(parameter.targets ?? [])]) {
      validateTarget(target, parameter.key, ids, template);
      validateParameterKindTarget(parameter, target, template);
    }
  }
  const requirementKeys = new Set<string>();
  for (const requirement of template.requirements) { const requirementRecord = record(requirement, "requirement"); keysOnly(requirementRecord, ["kind", "name", "provider", "capability"], "requirement"); const key = JSON.stringify([requirement.kind, requirement.name, requirement.provider ?? null, requirement.capability ?? null]); if (!REQUIREMENT_KINDS.has(requirement.kind) || typeof requirement.name !== "string" || !requirement.name.trim() || requirement.name.length > 120 || ("provider" in requirement && requirement.provider !== null && typeof requirement.provider !== "string") || ("capability" in requirement && requirement.capability !== null && typeof requirement.capability !== "string") || (typeof requirement.provider === "string" && requirement.provider.length > 96) || (typeof requirement.capability === "string" && requirement.capability.length > 120) || requirementKeys.has(key)) throw new ContractError("invalid requirement"); requirementKeys.add(key); }
  for (const [index, start] of template.starts.entries()) validateStart(start, index, template);
  if (depth(template) > MAX_DEPTH) throw new ContractError("JSON depth exceeds 16");
  if (new TextEncoder().encode(JSON.stringify(sorted(template))).length > MAX_BYTES) throw new ContractError("template is too large");
}

function validateNodeValues(type: string, data: Record<string, unknown>, id: string): void {
  const stringFields = ["label", "prompt", "provider", "harness", "model", "skillName", "role", "personality", "instructions", "template", "mode", "kind", "command", "source", "path", "body", "interpreter", "message", "method", "url", "headers", "title", "webhookUrl", "content", "action", "repo", "base", "head", "type", "providerId", "actionId", "connectionId", "memoryTitle"];
  for (const field of stringFields) if (field in data && typeof data[field] !== "string") throw new ContractError(`node ${id}.${field} must be a string`);
  for (const field of ["skillNames", "memoryIds", "paths"]) if (field in data && !stringArray(data[field])) throw new ContractError(`node ${id}.${field} must be a string array`);
  if ("attachments" in data && (!Array.isArray(data.attachments) || data.attachments.some((item) => { const attachment = item as Record<string, unknown>; return !item || typeof item !== "object" || Array.isArray(item) || Object.keys(attachment).some((key) => !["id", "path", "kind"].includes(key)) || typeof attachment.id !== "string" || !attachment.id || typeof attachment.path !== "string" || !attachment.path || !["file", "folder"].includes(String(attachment.kind)); }))) throw new ContractError(`node ${id}.attachments must contain structured file or folder entries`);
  for (const field of ["blocked", "appendOutput", "includeDiff", "desktop", "draft", "saveToMemory", "pinMemory", "asFinalResult", "htmlReport", "includeFilesChanged", "run"]) if (field in data && typeof data[field] !== "boolean") throw new ContractError(`node ${id}.${field} must be boolean`);
  if ("script" in data) {
    const script = data.script;
    if (!script || typeof script !== "object" || Array.isArray(script)) throw new ContractError(`node ${id}.script must be a structured script`);
    const scriptRecord = script as Record<string, unknown>;
    keysOnly(scriptRecord, ["source", "path", "body", "interpreter", "message", "run"], `node ${id}.script`);
    if (typeof scriptRecord.source !== "string" || !["file", "inline"].includes(scriptRecord.source)) throw new ContractError(`node ${id}.script.source is invalid`);
    for (const field of ["path", "body", "interpreter", "message"]) if (field in scriptRecord && typeof scriptRecord[field] !== "string") throw new ContractError(`node ${id}.script.${field} must be a string`);
    if ("run" in scriptRecord && typeof scriptRecord.run !== "boolean") throw new ContractError(`node ${id}.script.run must be boolean`);
  }
  for (const field of ["url", "webhookUrl"]) { const value = data[field]; if (typeof value === "string" && ["token=", "secret=", "password=", "api_key=", "apikey=", "auth="].some((needle) => value.toLowerCase().includes(needle))) throw new ContractError(`node ${id}.${field} contains a secret-bearing URL`); }
  if (type === "appAction" && (typeof data.input !== "object" || !data.input || Array.isArray(data.input))) throw new ContractError(`node ${id}.input must be an object`);
  if (type === "appAction" && typeof data.connectionId === "string" && data.connectionId.length > 0) throw new ContractError(`node ${id} retains a local connection`);
  if (type === "memory" && Array.isArray(data.memoryIds) && data.memoryIds.length > 0) throw new ContractError(`node ${id} retains a local memory`);
  if (type === "fileInject" && Array.isArray(data.paths) && data.paths.length > 0) throw new ContractError(`node ${id} retains a local path`);
}

function validateTarget(target: { kind: string; nodeId: string | null; field: string }, key: string, ids: Set<string>, template: Template): void {
  if (!target || !TARGET_KINDS.has(target.kind) || typeof target.field !== "string" || target.field.length === 0 || target.field.length > 128 || target.field === "__proto__") throw new ContractError(`invalid parameter ${key}`);
  keysOnly(record(target, `parameter ${key} target`), ["kind", "nodeId", "field"], `parameter ${key} target`);
  if (target.kind === "node") { if (!target.nodeId || !ids.has(target.nodeId)) throw new ContractError(`parameter ${key} references a missing node`); const node = template.graph.nodes.find((candidate) => candidate.id === target.nodeId)!; const field = target.field.split(".")[0]; const nested = (node.type === "appAction" && target.field.startsWith("input.") && target.field.length > 6) || ((node.type === "input" || node.type === "prompt") && target.field === "script.path"); const agentAccountBinding = node.type === "agent" && target.field === "accountRef"; if ((!NODE_FIELDS[node.type]?.includes(field) && !agentAccountBinding) || (target.field.includes(".") && !nested)) throw new ContractError(`parameter ${key} targets an unsupported field`); }
  else if (target.nodeId !== null && target.nodeId !== undefined) throw new ContractError(`parameter ${key} has an invalid non-node target`);
  if (target.kind === "workingDirectory" && target.field !== "workingDirectory") throw new ContractError(`parameter ${key} has an invalid working directory target`);
  if (target.kind === "start") {
    const match = /^(\d+)\.([A-Za-z][A-Za-z0-9_.-]{0,63})$/.exec(target.field);
    if (!match || Number(match[1]) >= template.starts.length) throw new ContractError(`parameter ${key} has an invalid start target`);
    const [field, nested] = match[2].split(".", 2);
    const start = template.starts[Number(match[1])];
    const allowed = start.kind === "schedule" ? ["cron", "label", "timezone"] : start.kind === "file" ? ["path", "label", "pattern"] : start.kind === "appEvent" ? ["event", "eventType", "providerId", "connectionId", "label", "filters"] : ["label"];
    if (!allowed.includes(field) || (nested && field !== "filters")) throw new ContractError(`parameter ${key} targets an unsupported start field`);
    if (nested && (!start.config.filters || typeof start.config.filters !== "object" || Array.isArray(start.config.filters))) throw new ContractError(`parameter ${key} targets missing start filters`);
  }
}

function incompatibleParameter(parameter: Template["parameters"][number], target: Template["parameters"][number]["target"], expected: string): never {
  throw new ContractError(`parameter ${parameter.key} has an incompatible kind for ${expected} target ${target.field}`);
}

function validateParameterKindTarget(parameter: Template["parameters"][number], target: Template["parameters"][number]["target"], template: Template): void {
  if (target.kind === "workingDirectory") {
    if (parameter.kind !== "folder") incompatibleParameter(parameter, target, "folder");
    return;
  }
  if (target.kind === "start") {
    const parts = target.field.split(".");
    const index = Number(parts[0]);
    const field = parts[1] ?? "";
    const start = template.starts[index];
    if (start?.kind === "appEvent" && field === "connectionId" && parameter.kind !== "appConnection") incompatibleParameter(parameter, target, "app connection");
    else if (start?.kind === "appEvent" && field === "filters" && parameter.kind !== "appResource") incompatibleParameter(parameter, target, "descriptor-backed app resource");
    else if (start?.kind === "file" && field === "path" && parameter.kind !== "folder") incompatibleParameter(parameter, target, "folder");
    else if (!(start?.kind === "appEvent" && (field === "connectionId" || field === "filters")) && !(start?.kind === "file" && field === "path") && !["text", "email", "channel"].includes(parameter.kind)) incompatibleParameter(parameter, target, "text");
    return;
  }
  const node = template.graph.nodes.find((candidate) => candidate.id === target.nodeId);
  if (!node) return;
  if (node.type === "agent" && target.field === "accountRef" && parameter.kind !== "agentAccount") incompatibleParameter(parameter, target, "agent account");
  else if (node.type === "memory" && target.field === "memoryIds" && parameter.kind !== "memory") incompatibleParameter(parameter, target, "memory");
  else if (node.type === "appAction" && target.field === "connectionId" && parameter.kind !== "appConnection") incompatibleParameter(parameter, target, "app connection");
  else if (node.type === "appAction" && target.field.startsWith("input.")) {
    if (parameter.allowedAction && parameter.kind !== "appResource") incompatibleParameter(parameter, target, "descriptor-backed app resource");
  } else if ((node.type === "fileInject" && target.field === "paths") || ((node.type === "input" || node.type === "prompt") && target.field === "attachments")) {
    if (parameter.kind !== "folder") incompatibleParameter(parameter, target, "folder");
  } else if ((node.type === "writeFile" && target.field === "path") || (node.type === "script" && target.field === "path") || ((node.type === "input" || node.type === "prompt") && target.field === "script.path")) {
    if (parameter.kind !== "file") incompatibleParameter(parameter, target, "file");
  } else if ((target.field === "connectionId" && node.type !== "appAction") || (target.field === "accountRef" && node.type !== "agent") || (target.field === "memoryIds" && node.type !== "memory") || (target.field === "paths" && !["fileInject", "input", "prompt"].includes(node.type))) {
    incompatibleParameter(parameter, target, "execution binding");
  }
}

function validateStart(start: { kind: string; config: Record<string, unknown> }, index: number, template: Template): void {
  keysOnly(record(start, "start"), ["kind", "config"], "start");
  if (!START_KINDS.has(start.kind) || !start.config || typeof start.config !== "object" || Array.isArray(start.config)) throw new ContractError("invalid start recipe");
  const allowed = start.kind === "schedule" ? ["cron", "label", "timezone"] : start.kind === "file" ? ["path", "label", "pattern"] : start.kind === "appEvent" ? ["event", "eventType", "providerId", "connectionId", "label", "filters"] : ["label"];
  keysOnly(start.config, allowed, `start ${start.kind}`);
  for (const [key, value] of Object.entries(start.config)) {
    if (key === "filters") { if (!value || typeof value !== "object" || Array.isArray(value)) throw new ContractError(`start ${start.kind}.${key} must be an object`); }
    else if (typeof value !== "string") throw new ContractError(`start ${start.kind}.${key} must be a string`);
  }
  if (start.kind === "appEvent") {
    const hasBinding = (field: string, kind: string) => template.parameters.some((parameter) => parameter.kind === kind && [parameter.target, ...(parameter.targets ?? [])].some((target) => target.kind === "start" && target.field === `${index}.${field}`));
    if (typeof start.config.connectionId === "string" && start.config.connectionId.trim() && !hasBinding("connectionId", "appConnection")) throw new ContractError(`app event start ${index} retains a local connection`);
    if (start.config.filters && typeof start.config.filters === "object" && !Array.isArray(start.config.filters)) {
      for (const [field, value] of Object.entries(start.config.filters as Record<string, unknown>)) if (typeof value === "string" && value.trim() && !hasBinding(`filters.${field}`, "appResource") && !isPublicEventFilterField(field)) throw new ContractError(`app event start ${index} retains local resource ${field}`);
    }
  }
  const required = start.kind === "schedule" ? "cron" : start.kind === "file" ? "path" : start.kind === "appEvent" ? "event" : "label";
  const appEvent = start.kind === "appEvent" && ["event", "eventType"].some((field) => typeof start.config[field] === "string" && (start.config[field] as string).trim());
  const requiredParameterized = template.parameters.some((parameter) => [parameter.target, ...(parameter.targets ?? [])].some((target) => target.kind === "start" && target.field === `${index}.${required}`));
  if (!appEvent && !requiredParameterized && (typeof start.config[required] !== "string" || !(start.config[required] as string).trim())) throw new ContractError(`start ${required} is required`);
}

function isPublicEventFilterField(field: string): boolean {
  const lower = field.toLowerCase();
  return !["id", "ids", "ref", "resource", "connection", "channel", "conversation", "repository", "project", "workspace", "account", "folder", "file"].some((marker) => lower === marker || lower.endsWith(marker) || (!["id", "ids", "ref"].includes(marker) && lower.includes(marker)));
}
