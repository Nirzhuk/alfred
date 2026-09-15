import assert from "node:assert/strict";
import { createApp, MemoryRepository } from "../src/index";
import fixture from "../../contracts/workflow-template/fixtures/notes-to-slack.json";

// This loopback-only fixture exercises the actual preview handler. D1 persistence
// has its separate workerd integration suite; no remote service is contacted here.
const app = createApp(new MemoryRepository());
const origin = "http://127.0.0.1:1423";
const env = {
  SHARE_ORIGIN: origin,
  SUPPORT_URL: "https://github.com/Nirzhuk/alfred-workflows/issues",
  DOWNLOAD_URL: "https://github.com/Nirzhuk/alfred-workflows/releases/latest",
};
const owner = "synthetic-local-preview-owner".padEnd(48, "a");
const probes: string[] = [];

async function publish(requestId: string, malicious = false) {
  const template = structuredClone(fixture);
  if (malicious) {
    template.title = '<script>window.previewInjected=true</script>';
    template.description = `<img src="${origin}/probe" onerror="window.previewInjected=true">`;
    template.graph.nodes[0].data.label = '<svg onload="window.previewInjected=true">';
  }
  const response = await app.fetch(new Request(`${origin}/api/v1/shares`, {
    method: "POST",
    headers: { authorization: `Bearer ${owner}`, "content-type": "application/json", "x-publication-request-id": requestId },
    body: JSON.stringify(template),
  }), env);
  assert.equal(response.status, 201, await response.clone().text());
  return await response.json() as { shareId: string; url: string };
}

const normal = await publish("preview-normal");
const malicious = await publish("preview-malicious", true);
const revoked = await publish("preview-revoked");
assert.equal((await app.fetch(new Request(`${origin}/api/v1/shares/${revoked.shareId}`, {
  method: "DELETE", headers: { authorization: `Bearer ${owner}` },
}), env)).status, 204);

const server = Bun.serve({
  hostname: "127.0.0.1", port: 1423,
  fetch(request) {
    const path = new URL(request.url).pathname;
    if (path === "/probe") probes.push(request.url);
    if (path === "/__qa/status") return Response.json({ normal, malicious, revoked, probes });
    if (request.method !== "GET") return new Response("Read-only browser fixture", { status: 405 });
    return app.fetch(request, env);
  },
});
console.log(JSON.stringify({ origin, normal: normal.url, malicious: malicious.url, revoked: revoked.url, status: `${origin}/__qa/status` }, null, 2));
process.on("SIGTERM", () => { server.stop(true); process.exit(0); });
