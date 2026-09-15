import type { AgentAccount } from "../agent-accounts/types";
import type { ParameterTarget, SharingParameter, WorkflowTemplate } from "./types";

export type LocalOption = { id: string; label: string; providerId?: string };
export type ResourceRequest = {
  kind: "action" | "event";
  connectionId: string;
  providerId: string;
  capability: string;
  fieldKey: string;
};
export type ResourceState = { options: LocalOption[]; loading: boolean; error?: string };
type ResourcePage = { items: LocalOption[]; nextPageToken: string | null };
export type ReadResources = (request: ResourceRequest, pageToken?: string) => Promise<ResourcePage>;

export const parameterTargets = (parameter: SharingParameter) => [parameter.target, ...parameter.targets ?? []];

function role(target: ParameterTarget): string | null {
  if (target.kind === "node" && target.nodeId) return `node:${target.nodeId}`;
  const match = target.kind === "start" && /^(\d+)\./.exec(target.field);
  return match ? `start:${match[1]}` : null;
}

export function dependentResources(parameters: SharingParameter[], connection: SharingParameter): SharingParameter[] {
  const roles = new Set(parameterTargets(connection).map(role).filter(Boolean));
  return parameters.filter((parameter) => parameter.kind === "appResource"
    && parameterTargets(parameter).some((target) => roles.has(role(target))));
}

export function agentAccountOptions(accounts: AgentAccount[]): LocalOption[] {
  return accounts.filter((account) => account.status === "connected").map((account) => ({
    id: account.id,
    label: `${account.displayName ?? account.providerName} (${account.providerName})`,
    providerId: account.providerId,
  }));
}

export function resourceRequests(template: WorkflowTemplate, parameter: SharingParameter, values: Record<string, unknown>): ResourceRequest[] | null {
  const requests = new Map<string, ResourceRequest>();
  for (const target of parameterTargets(parameter)) {
    const targetRole = role(target);
    const connection = template.parameters.find((candidate) => candidate.kind === "appConnection"
      && parameterTargets(candidate).some((binding) => role(binding) === targetRole));
    const connectionId = connection && values[connection.key];
    if (typeof connectionId !== "string" || !connectionId.trim()) return null;
    let request: ResourceRequest;
    if (target.kind === "node") {
      const node = template.graph.nodes.find((item) => item.id === target.nodeId);
      if (!node || !target.field.startsWith("input.")) throw new Error("Unsupported resource binding.");
      request = { kind: "action", connectionId, providerId: String(node.data.providerId ?? ""), capability: String(node.data.actionId ?? ""), fieldKey: target.field.slice(6) };
    } else {
      const match = /^(\d+)\.filters\.([^.]+)$/.exec(target.field);
      const start = match && template.starts[Number(match[1])];
      if (!match || !start) throw new Error("Unsupported event resource binding.");
      request = { kind: "event", connectionId, providerId: String(start.config.providerId ?? ""), capability: String(start.config.eventType ?? start.config.event ?? ""), fieldKey: match[2] };
    }
    if (!request.providerId || !request.capability) throw new Error("Resource capability is missing.");
    requests.set(JSON.stringify(request), request);
  }
  return [...requests.values()];
}

export async function loadResourceOptions(requests: ResourceRequest[], read: ReadResources): Promise<LocalOption[]> {
  const groups = await Promise.all(requests.map(async (request) => {
    const options = new Map<string, LocalOption>();
    const tokens = new Set<string>();
    let pageToken: string | undefined;
    for (let page = 0; page < 10; page += 1) {
      const result = await read(request, pageToken);
      for (const item of result.items) options.set(item.id, item);
      if (options.size > 5_000) throw new Error("Too many resources to list. Narrow this connection's access and try again.");
      if (!result.nextPageToken) return options;
      if (tokens.has(result.nextPageToken)) throw new Error("The provider repeated a resource page. Try loading again.");
      tokens.add(result.nextPageToken);
      pageToken = result.nextPageToken;
    }
    throw new Error("The resource list is incomplete. Narrow this connection's access and try again.");
  }));
  const first = groups[0];
  return first ? [...first.values()].filter((item) => groups.every((group) => group.has(item.id))) : [];
}

/** One generation per setup field: independent connections may load concurrently. */
export class ResourceLoader {
  private epoch = 0;
  private generations = new Map<string, number>();

  reset() { this.epoch += 1; this.generations.clear(); }

  invalidate(key: string) {
    const generation = (this.generations.get(key) ?? 0) + 1;
    this.generations.set(key, generation);
    return generation;
  }

  async load(key: string, task: () => Promise<LocalOption[]>, commit: (state: ResourceState) => void) {
    const epoch = this.epoch;
    const generation = this.invalidate(key);
    const current = () => this.epoch === epoch && this.generations.get(key) === generation;
    commit({ options: [], loading: true });
    try {
      const options = await task();
      if (current()) commit({ options, loading: false });
    } catch (error) {
      if (current()) commit({ options: [], loading: false, error: error instanceof Error ? error.message : "Could not load resources." });
    }
  }
}

export function withPublicExamples(template: WorkflowTemplate, examples: Record<string, string>): WorkflowTemplate {
  const result = structuredClone(template);
  const retained = new Set(Object.keys(examples));
  for (const parameter of result.parameters) {
    if (!retained.has(parameter.key)) continue;
    if (!["text", "email"].includes(parameter.kind)) throw new Error("This setup field cannot retain a public example.");
    const value = examples[parameter.key].trim();
    if (!value) throw new Error(`Enter a public example for ${parameter.label}.`);
    for (const target of parameterTargets(parameter)) {
      let cursor: Record<string, unknown> | undefined;
      let parts = target.field.split(".");
      if (target.kind === "node") cursor = result.graph.nodes.find((node) => node.id === target.nodeId)?.data;
      else if (target.kind === "start") {
        cursor = result.starts[Number(parts[0])]?.config;
        parts = parts.slice(1);
      }
      if (!cursor || parts.some((part) => ["__proto__", "prototype", "constructor"].includes(part))) throw new Error("Unsupported public example binding.");
      for (const part of parts.slice(0, -1)) {
        const child: unknown = cursor[part];
        if (!child || typeof child !== "object" || Array.isArray(child)) throw new Error("Public example field is missing.");
        cursor = child as Record<string, unknown>;
      }
      cursor[parts[parts.length - 1]] = value;
    }
  }
  result.parameters = result.parameters.filter((parameter) => !retained.has(parameter.key));
  return result;
}
