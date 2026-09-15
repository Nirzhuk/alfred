import { describe, expect, test } from "bun:test";
import { agentAccountOptions, dependentResources, loadResourceOptions, resourceRequests, ResourceLoader, withPublicExamples, type LocalOption, type ResourceState } from "./setup-model";
import type { SharingParameter, WorkflowTemplate } from "./types";
import type { AgentAccount } from "../agent-accounts/types";

const nodeTarget = (nodeId: string, field: string) => ({ kind: "node", nodeId, field });
const startTarget = (field: string) => ({ kind: "start", nodeId: null, field });
const parameter = (key: string, kind: string, nodeId: string, field: string): SharingParameter => ({ key, kind, label: key, required: true, target: nodeTarget(nodeId, field), targets: [] });
const connectionA = parameter("connection-a", "appConnection", "a", "connectionId");
connectionA.targets = [nodeTarget("b", "connectionId"), startTarget("0.connectionId")];
const connectionB = parameter("connection-b", "appConnection", "c", "connectionId");
const resourceA = parameter("resource-a", "appResource", "b", "input.channel");
const resourceB = parameter("resource-b", "appResource", "c", "input.channel");
const resourceEvent = { ...parameter("resource-event", "appResource", "a", "unused"), target: startTarget("0.filters.channel") };
const template: WorkflowTemplate = {
  format: "alfred.workflow-template", schemaVersion: 1, minAlfredVersion: "0.1.0", title: "Setup fixture", description: "",
  graph: { nodes: ["a", "b", "c"].map((id) => ({ id, type: "appAction", position: { x: 0, y: 0 }, data: { providerId: "slack", actionId: "send_message", input: { channel: "", text: "" } } })), edges: [] },
  starts: [{ kind: "appEvent", config: { providerId: "slack", eventType: "message", filters: { channel: "" } } }],
  parameters: [connectionA, connectionB, resourceA, resourceB, resourceEvent], requirements: [],
};
const option = (id: string): LocalOption => ({ id, label: id });
const deferred = <T>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
};

describe("recipient setup bindings", () => {
  test("grouped connection updates every secondary node and event role, leaving independent roles alone", () => {
    expect(dependentResources(template.parameters, connectionA).map((item) => item.key)).toEqual(["resource-a", "resource-event"]);
    expect(resourceRequests(template, resourceA, { "connection-a": "local-a" })).toEqual([{ kind: "action", connectionId: "local-a", providerId: "slack", capability: "send_message", fieldKey: "channel" }]);
    expect(resourceRequests(template, resourceEvent, { "connection-a": "local-a" })?.[0]).toEqual({ kind: "event", connectionId: "local-a", providerId: "slack", capability: "message", fieldKey: "channel" });
  });

  test("a grouped resource waits for all connections and intersects local permissions across pages", async () => {
    const grouped = { ...resourceA, targets: [nodeTarget("c", "input.channel")] };
    expect(resourceRequests(template, grouped, { "connection-a": "first" })).toBeNull();
    const requests = resourceRequests(template, grouped, { "connection-a": "first", "connection-b": "second" })!;
    const calls: string[] = [];
    const options = await loadResourceOptions(requests, async (request, token) => {
      calls.push(`${request.connectionId}:${token ?? "initial"}`);
      if (request.connectionId === "second") return { items: [option("shared"), option("second-only")], nextPageToken: null };
      return token ? { items: [option("shared")], nextPageToken: null } : { items: [option("first-only")], nextPageToken: "next" };
    });
    expect(options.map((item) => item.id)).toEqual(["shared"]);
    expect(calls).toContain("first:next");
  });

  test("repeated or overflowing pagination cannot silently offer partial results", async () => {
    const request = resourceRequests(template, resourceA, { "connection-a": "local" })!;
    await expect(loadResourceOptions(request, async () => ({ items: [option("partial")], nextPageToken: "same" }))).rejects.toThrow("repeated");
    let pages = 0;
    await expect(loadResourceOptions(request, async () => ({ items: [], nextPageToken: String(++pages) }))).rejects.toThrow("incomplete");
    expect(pages).toBe(10);
  });

  test("account compatibility uses provider IDs, excluding disconnected metadata", () => {
    const base = { id: "account", providerId: "claude_code", providerName: "Claude Code", displayName: "Work", status: "connected" } as AgentAccount;
    expect(agentAccountOptions([base, { ...base, id: "expired", status: "expired" }])).toEqual([{ id: "account", label: "Work (Claude Code)", providerId: "claude_code" }]);
  });
});

describe("resource response ownership", () => {
  test("independent selectors both retain out-of-order replies", async () => {
    const loader = new ResourceLoader();
    const a = deferred<LocalOption[]>(); const b = deferred<LocalOption[]>();
    const states: Record<string, ResourceState> = {};
    const first = loader.load("a", () => a.promise, (state) => { states.a = state; });
    const second = loader.load("b", () => b.promise, (state) => { states.b = state; });
    b.resolve([option("B")]); await second;
    a.resolve([option("A")]); await first;
    expect(states.a.options[0].id).toBe("A"); expect(states.b.options[0].id).toBe("B");
    expect(states.a.loading || states.b.loading).toBe(false);
  });

  test("changing a connection ignores its stale reply or error without cancelling other roles", async () => {
    const loader = new ResourceLoader(); const old = deferred<LocalOption[]>();
    let state: ResourceState = { options: [], loading: false };
    const previous = loader.load("resource", () => old.promise, (value) => { state = value; });
    await loader.load("resource", async () => [option("new")], (value) => { state = value; });
    old.reject(new Error("old failed")); await previous;
    expect(state).toEqual({ options: [option("new")], loading: false });
  });

  test("closing or resolving another template invalidates pending commits", async () => {
    const loader = new ResourceLoader(); const old = deferred<LocalOption[]>();
    const commits: ResourceState[] = [];
    const pending = loader.load("resource", () => old.promise, (state) => commits.push(state));
    loader.reset(); old.resolve([option("old")]); await pending;
    expect(commits).toEqual([{ options: [], loading: true }]);
  });
});

describe("retained public examples", () => {
  const example = { ...parameter("message", "text", "a", "input.text"), targets: [nodeTarget("b", "input.text")] };
  const source = { ...template, parameters: [...template.parameters, example] };
  test("blank selected example remains a validation error until entered", () => {
    expect(() => withPublicExamples(source, { message: "" })).toThrow("Enter a public example for message");
    expect(withPublicExamples(source, {}).parameters).toContainEqual(example);
  });
  test("retained example fills all bindings and removes the recipient question without mutating source", () => {
    const result = withPublicExamples(source, { message: " Public release note " });
    for (const id of ["a", "b"]) expect(result.graph.nodes.find((node) => node.id === id)?.data.input).toEqual({ channel: "", text: "Public release note" });
    expect(result.parameters.some((item) => item.key === "message")).toBe(false);
    expect(source.parameters).toContainEqual(example);
    expect(source.graph.nodes[0].data.input).toEqual({ channel: "", text: "" });
  });
  test("local connection fields cannot be retained as examples", () => {
    expect(() => withPublicExamples(source, { "connection-a": "private-id" })).toThrow("cannot retain");
  });
});
