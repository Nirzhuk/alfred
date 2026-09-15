import { describe, expect, test } from "bun:test";
import { createApp, MemoryRepository } from "../src/index";
import { digest } from "../src/contract";
import unicodeFixture from "../../contracts/workflow-template/fixtures/unicode-defaults.json";
import fixture from "../../contracts/workflow-template/fixtures/valid-note-summary.json";
import invalidCycle from "../../contracts/workflow-template/fixtures/invalid-cycle.json";
import invalidUnknownField from "../../contracts/workflow-template/fixtures/invalid-unknown-field.json";
import invalidDuplicateEdge from "../../contracts/workflow-template/fixtures/invalid-duplicate-edge.json";
import invalidDuplicateRequirement from "../../contracts/workflow-template/fixtures/invalid-duplicate-requirement.json";
import invalidParameterReference from "../../contracts/workflow-template/fixtures/invalid-parameter-reference.json";
import invalidStart from "../../contracts/workflow-template/fixtures/invalid-start.json";
import invalidEdgeHandle from "../../contracts/workflow-template/fixtures/invalid-edge-handle.json";
import invalidMinVersion from "../../contracts/workflow-template/fixtures/invalid-min-version.json";
import agentAccountFixture from "../../contracts/workflow-template/fixtures/agent-account-binding.json";
import parameterizedStartFixture from "../../contracts/workflow-template/fixtures/parameterized-start.json";
import invalidNumericRequirement from "../../contracts/workflow-template/fixtures/invalid-numeric-requirement-name.json";
import inputScriptFixture from "../../contracts/workflow-template/fixtures/input-script.json";
import canonicalBoundariesFixture from "../../contracts/workflow-template/fixtures/canonical-boundaries.json";
import notesToSlackFixture from "../../contracts/workflow-template/fixtures/notes-to-slack.json";
import { validateTemplate } from "../src/contract";

const capability = "a".repeat(64);
const request = (method: string, path: string, body?: unknown, headers: Record<string, string> = {}) => new Request(`https://share.example${path}`, { method, headers: { authorization: `Bearer ${capability}`, "content-type": "application/json", ...headers }, body: body === undefined ? undefined : JSON.stringify(body) });

describe("workflow share service", () => {
  test("publishes, retries by request id, reads, and revokes", async () => {
    const app = createApp(new MemoryRepository());
    const first = await app.fetch(request("POST", "/api/v1/shares", fixture, { "x-publication-request-id": "publish-1" }), { SHARE_ORIGIN: "https://share.example" });
    expect(first.status).toBe(201); const published = await first.json() as { shareId: string; digest: string };
    const retry = await app.fetch(request("POST", "/api/v1/shares", fixture, { "x-publication-request-id": "publish-1" }), { SHARE_ORIGIN: "https://share.example" });
    expect(retry.status).toBe(200); expect((await retry.json()).shareId).toBe(published.shareId);
    const read = await app.fetch(request("GET", `/api/v1/shares/${published.shareId}`), { SHARE_ORIGIN: "https://share.example" }); expect(read.status).toBe(200);
    expect((await app.fetch(request("DELETE", `/api/v1/shares/${published.shareId}`), { SHARE_ORIGIN: "https://share.example" })).status).toBe(204);
    expect((await app.fetch(request("GET", `/api/v1/shares/${published.shareId}`), { SHARE_ORIGIN: "https://share.example" })).status).toBe(410);
  });
  test("does not let another capability revoke", async () => {
    const app = createApp(new MemoryRepository()); const response = await app.fetch(request("POST", "/api/v1/shares", fixture, { "x-publication-request-id": "publish-2" }), { SHARE_ORIGIN: "https://share.example" }); const { shareId } = await response.json() as { shareId: string };
    const wrong = new Request(`https://share.example/api/v1/shares/${shareId}`, { method: "DELETE", headers: { authorization: `Bearer ${"b".repeat(64)}` } }); expect((await app.fetch(wrong, { SHARE_ORIGIN: "https://share.example" })).status).toBe(404);
  });
  test("escapes preview content", async () => { const app = createApp(new MemoryRepository()); const malicious = { ...fixture, title: "<script>alert(1)</script>", description: "<img src=x>" }; const response = await app.fetch(request("POST", "/api/v1/shares", malicious, { "x-publication-request-id": "publish-3" }), { SHARE_ORIGIN: "https://share.example" }); const { shareId } = await response.json() as { shareId: string }; const page = await app.fetch(new Request(`https://share.example/s/${shareId}`), { SHARE_ORIGIN: "https://share.example" }); const text = await page.text(); expect(text).not.toContain("<script>alert"); expect(text).toContain("&lt;script&gt;"); expect(page.headers.get("content-security-policy")).toContain("default-src 'none'"); });
  test("unfurls only reviewed title and description, with mobile copy fallback", async () => {
    const app = createApp(new MemoryRepository());
    const template = structuredClone(fixture);
    template.title = 'Notes " safely shared';
    template.description = '<img src="https://invalid.example/probe">';
    template.graph.nodes[0].data.prompt = "Executable-content-marker";
    const published = await app.fetch(request("POST", "/api/v1/shares", template, { "x-publication-request-id": "preview-meta" }), { SHARE_ORIGIN: "https://share.example" });
    expect(published.status).toBe(201);
    const { shareId } = await published.json() as { shareId: string };
    const page = await app.fetch(request("GET", `/s/${shareId}`), { SHARE_ORIGIN: "https://share.example" });
    const html = await page.text();
    const head = html.split("</head>")[0];
    expect(head).toContain('property="og:title" content="Notes &quot; safely shared"');
    expect(head).toContain('property="og:description" content="&lt;img src=&quot;https://invalid.example/probe&quot;&gt;"');
    expect(head).not.toContain("Executable-content-marker");
    expect(head).toContain('name="viewport" content="width=device-width, initial-scale=1"');
    expect(html).toContain("Alfred is a desktop app. Copy this link and open it on your computer.");
    expect(html).toContain(`data-copy="https://share.example/s/${shareId}"`);
    expect(page.headers.get("cache-control")).toBe("no-store");
    expect(page.headers.get("referrer-policy")).toBe("no-referrer");
  });
  test("bounds chunked request bodies and cancels the reader", async () => {
    const app = createApp(new MemoryRepository()); let cancelled = false; let pulls = 0;
    const body = new ReadableStream<Uint8Array>({ pull(controller) { pulls += 1; controller.enqueue(new Uint8Array(200_000)); if (pulls > 2) controller.close(); }, cancel() { cancelled = true; } });
    const response = await app.fetch(new Request("https://share.example/api/v1/shares", { method: "POST", headers: { authorization: `Bearer ${capability}`, "content-type": "application/json", "x-publication-request-id": "streamed-1" }, body, duplex: "half" } as RequestInit), { SHARE_ORIGIN: "https://share.example" });
    expect(response.status).toBe(400); expect(cancelled).toBe(true);
  });
  test("matches the Rust canonical digest for Unicode and numeric defaults", async () => {
    expect(await digest(unicodeFixture as any)).toBe("8e3404540176b29ce866f5d74ef0b69dad24217b60813c23fe9e34e65abc371b");
  });
  test("accepts the structured Input script binding", () => {
    expect(() => validateTemplate(inputScriptFixture as any)).not.toThrow();
  });
  test("accepts the synthetic notes-to-Slack two-profile template", async () => {
    expect(() => validateTemplate(notesToSlackFixture as any)).not.toThrow();
    const app = createApp(new MemoryRepository());
    const first = await app.fetch(request("POST", "/api/v1/shares", notesToSlackFixture, { "x-publication-request-id": "notes-profile-a" }), { SHARE_ORIGIN: "https://share.example" });
    const second = await app.fetch(request("POST", "/api/v1/shares", notesToSlackFixture, { "x-publication-request-id": "notes-profile-b" }), { SHARE_ORIGIN: "https://share.example" });
    expect(first.status).toBe(201);
    expect(second.status).toBe(201);
    expect((await first.json() as { shareId: string }).shareId).not.toBe((await second.json() as { shareId: string }).shareId);
  });
  test("accepts grouped agent account bindings without a local account value", () => {
    expect(() => validateTemplate(agentAccountFixture as any)).not.toThrow();
    expect(() => validateTemplate({ ...agentAccountFixture, graph: { ...agentAccountFixture.graph, nodes: [{ ...agentAccountFixture.graph.nodes[0], data: { ...agentAccountFixture.graph.nodes[0].data, accountRef: "local-account" } }, agentAccountFixture.graph.nodes[1]] } } as any)).toThrow();
  });
  test("accepts a recipient-parameterized file start", () => {
    expect(() => validateTemplate(parameterizedStartFixture as any)).not.toThrow();
  });
  test("keeps canonical bytes stable at numeric and Unicode boundaries", async () => {
    expect(await digest(canonicalBoundariesFixture as any)).toBe("66486eb47b6594137092f3dd9f97b4d3884bea740721ef1c76fad35e49fd8966");
  });
  test("serializes the in-process quota gate for concurrent creates", async () => {
    const app = createApp(new MemoryRepository());
    const responses = await Promise.all(["concurrent-a", "concurrent-b"].map((requestId) => app.fetch(request("POST", "/api/v1/shares", fixture, { "x-publication-request-id": requestId }), { SHARE_ORIGIN: "https://share.example", MAX_PUBLICATIONS: "1" })));
    expect(responses.filter((response) => response.status === 201)).toHaveLength(1); expect(responses.filter((response) => response.status === 429)).toHaveLength(1);
  });
  test("rejects the shared malformed contract fixtures", () => {
    for (const malformed of [invalidCycle, invalidUnknownField, invalidDuplicateEdge, invalidDuplicateRequirement, invalidParameterReference, invalidStart, invalidEdgeHandle, invalidMinVersion, invalidNumericRequirement]) {
      expect(() => validateTemplate(malformed as any)).toThrow();
    }
  });
  test("retained tombstones and production rate limiting stay bounded", async () => {
    const app = createApp(new MemoryRepository());
    const first = await app.fetch(request("POST", "/api/v1/shares", fixture, { "x-publication-request-id": "retained-a" }), { SHARE_ORIGIN: "https://share.example", MAX_RETAINED_RECORDS: "1" });
    expect(first.status).toBe(201);
    const published = await first.json() as { shareId: string };
    expect((await app.fetch(request("DELETE", `/api/v1/shares/${published.shareId}`), { SHARE_ORIGIN: "https://share.example", MAX_RETAINED_RECORDS: "1" })).status).toBe(204);
    const second = await app.fetch(request("POST", "/api/v1/shares", fixture, { "x-publication-request-id": "retained-b" }), { SHARE_ORIGIN: "https://share.example", MAX_RETAINED_RECORDS: "1" });
    expect(second.status).toBe(429);
    const production = createApp(new MemoryRepository(), false);
    expect((await production.fetch(request("GET", "/api/v1/shares/missing"), { SHARE_ORIGIN: "https://share.example", REPOSITORY: new MemoryRepository() })).status).toBe(503);
  });
});
