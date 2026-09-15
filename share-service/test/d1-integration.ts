import assert from "node:assert/strict";
import { spawn, type ChildProcess } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { canonicalBytes, digest, type Template } from "../src/contract";

const service = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspace = resolve(service, "..");
const directory = await mkdtemp(join(tmpdir(), "alfred-share-d1-"));
const config = join(directory, "wrangler.toml");
const state = await mkdtemp(join(tmpdir(), "alfred-share-d1-state-"));
const wrangler = join(service, "node_modules/wrangler/bin/wrangler.js");
const node = Bun.which("node");
assert(node, "Wrangler requires Node.js on PATH");
const fixture = await Bun.file(join(workspace, "contracts/workflow-template/fixtures/unicode-defaults.json")).json() as Template;
const owner = "local-integration-owner-".padEnd(48, "a");
const server = createServer();
await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
const address = server.address();
assert(address && typeof address === "object");
const port = address.port;
await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
const origin = `http://127.0.0.1:${port}`;
let worker: ChildProcess | undefined;
let logs = "";
let assertions = 0;

async function command(args: string[]) {
  const result = Bun.spawn([node!, wrangler, ...args], {
    cwd: service, env: { ...process.env, WRANGLER_SEND_METRICS: "false" },
    stdout: "pipe", stderr: "pipe",
  });
  const [stdout, stderr, code] = await Promise.all([new Response(result.stdout).text(), new Response(result.stderr).text(), result.exited]);
  assert.equal(code, 0, `${args.join(" ")}\n${stdout}\n${stderr}`);
  return stdout;
}

async function sql(query: string) {
  const output = await command(["d1", "execute", "DB", "--config", config, "--local", "--persist-to", state, "--command", query, "--yes", "--json"]);
  return JSON.parse(output)[0].results as Array<Record<string, unknown>>;
}

async function stop() {
  const child = worker;
  if (!child) return;
  worker = undefined;
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
  const signal = (value: NodeJS.Signals) => { try { if (process.platform === "win32") child.kill(value); else process.kill(-child.pid!, value); } catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; } };
  signal("SIGTERM");
  const timer = setTimeout(() => signal("SIGKILL"), 3000);
  await exited;
  clearTimeout(timer);
}

async function start(options: { publishing?: boolean; count?: number; bytes?: number; retained?: number } = {}) {
  await stop();
  await writeFile(config, `name = "alfred-share-local-integration"\nmain = ${JSON.stringify(join(directory, "index.js"))}\ncompatibility_date = "2026-09-08"\n[vars]\nSHARE_ORIGIN = ${JSON.stringify(origin)}\nPUBLISHING_ENABLED = "${options.publishing !== false}"\nMAX_PUBLICATIONS = "${options.count ?? 100}"\nMAX_STORAGE_BYTES = "${options.bytes ?? 2621440000}"\nMAX_RETAINED_RECORDS = "${options.retained ?? 1000}"\n[[ratelimits]]\nname = "RATE_LIMITER"\nnamespace_id = "1001"\n[ratelimits.simple]\nlimit = 1000\nperiod = 60\n[[d1_databases]]\nbinding = "DB"\ndatabase_name = "alfred-share-local-integration"\ndatabase_id = "00000000-0000-0000-0000-000000000042"\n`);
  logs = "";
  worker = spawn(node!, [wrangler, "dev", "--config", config, "--local", "--ip", "127.0.0.1", "--port", String(port), "--persist-to", state, "--no-bundle", "--log-level", "warn", "--show-interactive-dev-session=false"], {
    cwd: service, detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, WRANGLER_SEND_METRICS: "false" },
  });
  worker.stdout!.on("data", (data) => { logs = (logs + data).slice(-20000); });
  worker.stderr!.on("data", (data) => { logs = (logs + data).slice(-20000); });
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    assert(worker.exitCode === null, `Worker exited during startup:\n${logs}`);
    try { if ((await fetch(`${origin}/`, { signal: AbortSignal.timeout(1000) })).status === 404) return; } catch {}
    await Bun.sleep(100);
  }
  throw new Error(`Worker startup timed out:\n${logs}`);
}

async function publish(requestId: string, template = fixture) {
  const response = await fetch(`${origin}/api/v1/shares`, { method: "POST", signal: AbortSignal.timeout(10000), headers: { authorization: `Bearer ${owner}`, "x-publication-request-id": requestId, "content-type": "application/json" }, body: JSON.stringify(template) });
  return { status: response.status, body: await response.json() as { shareId?: string; digest?: string; error?: { code: string } } };
}

function pass(label: string) { assertions++; console.log(`PASS ${label}`); }

try {
  const build = await Bun.build({ entrypoints: [join(service, "src/index.ts")], outdir: directory, target: "browser", format: "esm" });
  assert(build.success, `Worker bundle failed: ${build.logs}`);
  console.log(`Worker bundle SHA-256: ${createHash("sha256").update(await Bun.file(join(directory, "index.js")).bytes()).digest("hex")}`);
  await start();
  await command(["d1", "execute", "DB", "--config", config, "--local", "--persist-to", state, "--file", join(service, "migrations/0001_shares.sql"), "--yes", "--json"]);

  const created = await publish("durable-first");
  assert.equal(created.status, 201, JSON.stringify(created.body));
  assert.equal(created.body.digest, await digest(fixture));
  const id = created.body.shareId!;
  assert.deepEqual((await publish("durable-first")).body, created.body);
  assert.equal((await publish("durable-first", { ...fixture, title: "Changed" })).status, 409);
  pass("D1 create, Unicode digest, stable retry, changed-content conflict");

  await start();
  const fetched = await fetch(`${origin}/api/v1/shares/${id}`);
  assert.equal(fetched.status, 200);
  assert.deepEqual((await fetched.json() as { template: Template }).template, JSON.parse(new TextDecoder().decode(canonicalBytes(fixture))));
  assert.deepEqual((await publish("durable-first")).body, created.body);
  pass("Worker restart preserves publication and owner request identity");

  const repeated = await Promise.all(Array.from({ length: 8 }, () => publish("concurrent-retry")));
  assert(repeated.every((result) => [200, 201, 409].includes(result.status)), JSON.stringify(repeated));
  const converged = await publish("concurrent-retry");
  assert.equal(converged.status, 200);
  assert(repeated.filter((result) => result.status < 300).every((result) => result.body.shareId === converged.body.shareId));
  assert.equal((await sql("SELECT COUNT(*) AS count FROM shares WHERE request_id = 'concurrent-retry'"))[0].count, 1);
  pass("Concurrent retries converge on exactly one durable row");

  assert.equal((await fetch(`${origin}/api/v1/shares/${id}`, { method: "DELETE", headers: { authorization: `Bearer ${"other-owner".padEnd(48, "b")}` } })).status, 404);
  assert.equal((await fetch(`${origin}/api/v1/shares/${id}`, { method: "DELETE", headers: { authorization: `Bearer ${owner}` } })).status, 204);
  assert.equal((await fetch(`${origin}/api/v1/shares/${id}`)).status, 410);
  assert.equal((await fetch(`${origin}/s/${id}`)).status, 410);
  const revoked = (await sql(`SELECT revoked, payload FROM shares WHERE id = '${id}'`))[0];
  assert.deepEqual(revoked, { revoked: 1, payload: null });
  pass("Owner-only revocation immediately removes payload and keeps tombstone");

  await start({ publishing: false });
  assert.equal((await publish("disabled")).status, 503);
  assert.equal((await fetch(`${origin}/api/v1/shares/${converged.body.shareId}`)).status, 200);
  pass("Disabling writes preserves existing reads");

  await sql("DELETE FROM shares");
  await start({ count: 3 });
  const limited = await Promise.all(Array.from({ length: 10 }, (_, index) => publish(`count-${index}`)));
  assert.equal(limited.filter((result) => result.status === 201).length, 3, JSON.stringify(limited));
  assert(limited.filter((result) => result.status !== 201).every((result) => result.status === 429 && result.body.error?.code === "storage_cap_reached"));
  assert.equal((await sql("SELECT COUNT(*) AS count FROM shares"))[0].count, 3);
  pass("Concurrent publication count cap is enforced in D1");

  await sql("DELETE FROM shares");
  await start({ bytes: 262144 });
  const large = structuredClone(fixture);
  large.graph.nodes[0].data.prompt = "界".repeat(46000);
  assert(canonicalBytes(large).byteLength > 131072 && canonicalBytes(large).byteLength < 262144);
  assert.equal((await publish("byte-first", large)).status, 201);
  const overBytes = await publish("byte-second", large);
  assert.equal(overBytes.status, 429);
  assert.equal(overBytes.body.error?.code, "storage_cap_reached");
  assert.equal((await sql("SELECT SUM(length(CAST(payload AS BLOB))) AS bytes FROM shares"))[0].bytes, canonicalBytes(large).byteLength);
  pass("Storage cap counts Unicode UTF-8 bytes rather than characters");

  await sql("DELETE FROM shares");
  await start({ retained: 1 });
  const retained = await publish("retained-first");
  assert.equal(retained.status, 201);
  assert.equal((await fetch(`${origin}/api/v1/shares/${retained.body.shareId}`, { method: "DELETE", headers: { authorization: `Bearer ${owner}` } })).status, 204);
  assert.equal((await publish("retained-second")).status, 429);
  assert.equal((await publish("retained-first")).body.shareId, retained.body.shareId);
  pass("Retained-row cap includes revoked records without breaking idempotency");

  const malformed = await Bun.file(join(workspace, "contracts/workflow-template/fixtures/invalid-unknown-field.json")).json();
  assert.equal((await publish("malformed", malformed)).status, 400);
  pass("Malformed shared contract fixture is rejected by actual Worker");
  console.log(`${assertions} local Worker/D1 integration scenarios passed.`);
} catch (error) {
  console.error(logs);
  throw error;
} finally {
  await stop();
  await rm(directory, { recursive: true, force: true });
  await rm(state, { recursive: true, force: true });
}
