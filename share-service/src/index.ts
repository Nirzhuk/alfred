import { canonicalBytes, ContractError, digest, parseTemplate, type Template } from "./contract";

const SHARE_ID = /^[A-Za-z0-9_-]{22,64}$/;
const REQUEST_ID = /^[A-Za-z0-9_.:-]{1,128}$/;
const OWNER_CAPABILITY = /^[A-Za-z0-9_-]{32,256}$/;
const MAX_BODY_BYTES = 256 * 1024;
const MAX_PUBLICATIONS = 10_000;
const MAX_RETAINED_RECORDS = 100_000;

export type ShareRecord = { id: string; requestId: string; ownerHash: string; digest: string; payload: string | null; revoked: boolean; createdAt: string };
type D1Record = { id: string; requestId: string; ownerHash: string; digest: string; payload: string | null; revoked: number; createdAt: string };
export interface ShareRepository { findByRequest(ownerHash: string, requestId: string): Promise<ShareRecord | null>; get(id: string): Promise<ShareRecord | null>; put(record: ShareRecord): Promise<void>; revoke(id: string, ownerHash: string): Promise<boolean>; }

export class MemoryRepository implements ShareRepository {
  readonly records = new Map<string, ShareRecord>();
  async findByRequest(ownerHash: string, requestId: string) { return [...this.records.values()].find((record) => record.ownerHash === ownerHash && record.requestId === requestId) ?? null; }
  async get(id: string) { return this.records.get(id) ?? null; }
  async put(record: ShareRecord) { this.records.set(record.id, record); }
  putWithCaps(record: ShareRecord, maxPublications: number, maxStorageBytes: number, maxRetainedRecords = MAX_RETAINED_RECORDS) { const active = [...this.records.values()].filter((item) => !item.revoked); const bytes = [...this.records.values()].reduce((total, item) => total + (item.payload ? new TextEncoder().encode(item.payload).byteLength : 0), 0); if (this.records.size >= maxRetainedRecords || active.length >= maxPublications || bytes + (record.payload ? new TextEncoder().encode(record.payload).byteLength : 0) > maxStorageBytes) throw new Error("publication storage cap reached"); this.records.set(record.id, record); }
  async revoke(id: string, ownerHash: string) { const record = this.records.get(id); if (!record || record.ownerHash !== ownerHash) return false; record.revoked = true; record.payload = null; this.records.set(id, record); return true; }
}

type D1Result = { meta?: { changes?: number } };
type D1Statement = { bind(...values: unknown[]): D1Statement; first<T>(): Promise<T | null>; run(): Promise<D1Result> };
type D1Database = { prepare(query: string): D1Statement };
type DistributedRateLimiter = { limit(input: { key: string }): Promise<{ success: boolean }> };
type WorkerExecutionContext = { waitUntil(promise: Promise<unknown>): void; passThroughOnException(): void };
export type Env = { DB?: D1Database; RATE_LIMITER?: DistributedRateLimiter; SHARE_ORIGIN?: string; PUBLISHING_ENABLED?: string; SUPPORT_URL?: string; DOWNLOAD_URL?: string; MAX_PUBLICATIONS?: string; MAX_STORAGE_BYTES?: string; MAX_RETAINED_RECORDS?: string; RATE_LIMIT_PER_MINUTE?: string; RATE_LIMIT_ENTRIES?: string; REPOSITORY?: ShareRepository };

async function sha256(value: string): Promise<string> { const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)); return [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, "0")).join(""); }
function json(data: unknown, status = 200, extra: Record<string, string> = {}) { return new Response(JSON.stringify(data), { status, headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store", ...extra } }); }
function error(status: number, code: string, message: string) { return json({ error: { code, message } }, status); }
function id() { const bytes = crypto.getRandomValues(new Uint8Array(24)); return btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", ""); }
function owner(request: Request) { const raw = request.headers.get("authorization")?.replace(/^Bearer\s+/i, "").trim() ?? ""; return OWNER_CAPABILITY.test(raw) ? raw : null; }
function repo(env: Env): ShareRepository { if (env.REPOSITORY) return env.REPOSITORY; if (!env.DB) throw new Error("D1 is not configured"); return new D1Repository(env.DB); }

class D1Repository implements ShareRepository {
  constructor(readonly db: D1Database) {}
  async findByRequest(ownerHash: string, requestId: string) { const row = await this.db.prepare("SELECT id, request_id AS requestId, owner_hash AS ownerHash, digest, payload, revoked, created_at AS createdAt FROM shares WHERE owner_hash = ? AND request_id = ?").bind(ownerHash, requestId).first<D1Record>(); return row ? normalize(row) : null; }
  async get(id: string) { const row = await this.db.prepare("SELECT id, request_id AS requestId, owner_hash AS ownerHash, digest, payload, revoked, created_at AS createdAt FROM shares WHERE id = ?").bind(id).first<D1Record>(); return row ? normalize(row) : null; }
  async put(record: ShareRecord) { await this.db.prepare("INSERT INTO shares (id, request_id, owner_hash, digest, payload, revoked, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)").bind(record.id, record.requestId, record.ownerHash, record.digest, record.payload, record.revoked ? 1 : 0, record.createdAt).run(); }
  async putWithCaps(record: ShareRecord, maxPublications: number, maxStorageBytes: number, maxRetainedRecords = MAX_RETAINED_RECORDS) {
    const result = await this.db.prepare("INSERT INTO shares (id, request_id, owner_hash, digest, payload, revoked, created_at) SELECT ?, ?, ?, ?, ?, ?, ? WHERE (SELECT COUNT(*) FROM shares) < ? AND (SELECT COUNT(*) FROM shares WHERE revoked = 0) < ? AND (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM shares) + length(CAST(? AS BLOB)) <= ?").bind(record.id, record.requestId, record.ownerHash, record.digest, record.payload, record.revoked ? 1 : 0, record.createdAt, maxRetainedRecords, maxPublications, record.payload, maxStorageBytes).run();
    if ((result.meta?.changes ?? 0) === 0) throw new Error("publication storage cap reached");
  }
  async revoke(id: string, ownerHash: string) { const result = await this.db.prepare("UPDATE shares SET revoked = 1, payload = NULL WHERE id = ? AND owner_hash = ?").bind(id, ownerHash).run(); return (result.meta?.changes ?? 0) > 0; }
}
function normalize(row: D1Record): ShareRecord { return { id: row.id, requestId: row.requestId, ownerHash: row.ownerHash, digest: row.digest, payload: row.payload, createdAt: row.createdAt, revoked: Boolean(row.revoked) }; }

function headers(origin: string) { return { "cache-control": "no-store", "referrer-policy": "no-referrer", "x-robots-tag": "noindex, nofollow", "content-security-policy": "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'", "access-control-allow-origin": origin }; }
function escapeHtml(value: string) { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
function preview(record: ShareRecord, env: Env) {
  const template = record.payload ? JSON.parse(record.payload) as Template : null;
  const origin = env.SHARE_ORIGIN ?? "";
  const copyUrl = `${origin}/s/${record.id}`;
  const requirements = template?.requirements.map((requirement) =>
    `<li><strong>${escapeHtml(requirement.kind)}</strong>: ${escapeHtml(requirement.name)}${requirement.provider ? ` (${escapeHtml(requirement.provider)})` : ""}</li>`,
  ).join("") ?? "";
  const body = template ? `<main>
    <h1>${escapeHtml(template.title)}</h1>
    <p>${escapeHtml(template.description)}</p>
    <h2>Steps</h2>
    <ol>${template.graph.nodes.map((node) => `<li>${escapeHtml(String(node.data.label ?? node.type))}</li>`).join("")}</ol>
    <h2>Connections</h2>
    ${template.graph.edges.length ? `<ul>${template.graph.edges.map((edge) => `<li>${escapeHtml(edge.source)} → ${escapeHtml(edge.target)}</li>`).join("")}</ul>` : "<p>No connections.</p>"}
    <h2>Required apps and tools</h2>
    ${requirements ? `<ul>${requirements}</ul>` : "<p>No additional local tools are declared.</p>"}
    <p>This unlisted template is available to anyone with this link. Review the exact setup and executable content in Alfred before importing.</p>
    <p><a href="alfred-workflows://import/${escapeHtml(record.id)}">Open in Alfred</a> · <button type="button" data-copy="${escapeHtml(copyUrl)}">Copy link</button> · <a href="${escapeHtml(env.DOWNLOAD_URL ?? "https://github.com/Nirzhuk/alfred-workflows/releases/latest")}">Download Alfred</a>${env.SUPPORT_URL ? ` · <a href="${escapeHtml(env.SUPPORT_URL)}">Report a problem</a>` : ""}</p>
    <p class="mobile-instructions">Alfred is a desktop app. Copy this link and open it on your computer.</p>
    <p class="instructions">If Alfred is not installed, download it first, then reopen this link or paste it into “Import from link…” inside Alfred. The browser never sends setup values to the publisher.</p>
    </main><script>document.querySelector('[data-copy]').addEventListener('click',async(e)=>{const b=e.currentTarget;try{await navigator.clipboard.writeText(b.dataset.copy||'');b.textContent='Copied';}catch{b.textContent='Copy this URL from the address bar';}});</script>` : `<main><h1>Template unavailable</h1><p>This publication was revoked or is no longer available.</p></main>`;
  const title = escapeHtml(template?.title ?? "Template unavailable");
  const description = escapeHtml(template?.description ?? "This publication is no longer available.");
  return new Response(`<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex,nofollow"><meta name="description" content="${description}"><meta property="og:title" content="${title}"><meta property="og:description" content="${description}"><title>${title}</title><style>:root{color-scheme:light dark}body{font:16px/1.5 system-ui;max-width:720px;margin:4rem auto;padding:0 1rem;overflow-wrap:anywhere}a{color:#137a5f}button{font:inherit;cursor:pointer}.instructions{color:#555}.mobile-instructions{display:none}@media(prefers-color-scheme:dark){a{color:#6cdbb7}.instructions{color:#bbb}}@media(max-width:600px){body{margin:2rem auto}.mobile-instructions{display:block}}</style></head><body>${body}</body></html>`, { status: template ? 200 : 410, headers: { "content-type": "text/html; charset=utf-8", ...headers(origin) } });
}

async function boundedJson(request: Request): Promise<unknown> {
  const length = request.headers.get("content-length");
  if (length && Number.isFinite(Number(length)) && Number(length) > MAX_BODY_BYTES) throw new ContractError("template is too large");
  const reader = request.body?.getReader();
  if (!reader) throw new ContractError("request body is required");
  const chunks: Uint8Array[] = []; let total = 0;
  try {
    while (true) {
      const next = await reader.read(); if (next.done) break;
      total += next.value.byteLength;
      if (total > MAX_BODY_BYTES) { await reader.cancel("template is too large"); throw new ContractError("template is too large"); }
      chunks.push(next.value);
    }
  } finally { reader.releaseLock(); }
  const body = new Uint8Array(total); let offset = 0; for (const chunk of chunks) { body.set(chunk, offset); offset += chunk.byteLength; }
  try { return JSON.parse(new TextDecoder().decode(body)); } catch { throw new ContractError("request body is not valid JSON"); }
}

export function createApp(defaultRepository = new MemoryRepository(), allowMemory = true) {
  const rate = new Map<string, { started: number; count: number }>();
  return { async fetch(request: Request, env: Env = {}) { const url = new URL(request.url); const repository = env.REPOSITORY ?? (env.DB ? repo(env) : allowMemory ? defaultRepository : null); if (!repository) return error(503, "service_unconfigured", "The sharing service is not configured."); if (!allowMemory && !env.RATE_LIMITER) return error(503, "rate_limiter_unconfigured", "The sharing service requires a distributed rate limiter."); const origin = env.SHARE_ORIGIN?.trim() || url.origin; const key = request.headers.get("cf-connecting-ip") ?? request.headers.get("x-forwarded-for") ?? "unknown"; if (env.RATE_LIMITER) { try { if (!(await env.RATE_LIMITER.limit({ key })).success) return error(429, "rate_limited", "Too many requests. Try again later."); } catch { return error(503, "rate_limiter_unavailable", "The sharing service is temporarily unavailable."); } } else { const now = Date.now(); const limit = Math.min(10_000, Math.max(1, Number(env.RATE_LIMIT_PER_MINUTE) || 120)); const entries = Math.min(10_000, Math.max(100, Number(env.RATE_LIMIT_ENTRIES) || 10_000)); const bucket = rate.get(key); if (!bucket || now - bucket.started >= 60_000) { if (rate.size >= entries) { const oldest = rate.keys().next().value; if (oldest) rate.delete(oldest); } rate.set(key, { started: now, count: 1 }); } else { bucket.count += 1; if (bucket.count > limit) return error(429, "rate_limited", "Too many requests. Try again later."); } }
    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: { ...headers(origin), "access-control-allow-methods": "GET,POST,DELETE,OPTIONS", "access-control-allow-headers": "authorization,content-type,x-publication-request-id" } });
    const match = url.pathname.match(/^\/api\/v1\/shares(?:\/([^/]+))?$/);
    if (match && request.method === "POST" && !match[1]) { if (env.PUBLISHING_ENABLED === "false") return error(503, "publishing_disabled", "Publishing is temporarily disabled."); const capability = owner(request); if (!capability) return error(401, "owner_required", "A valid owner capability is required."); const requestId = request.headers.get("x-publication-request-id")?.trim() ?? ""; if (!REQUEST_ID.test(requestId)) return error(400, "invalid_request_id", "A durable publication request ID is required."); let template: Template; try { template = parseTemplate(await boundedJson(request)); } catch (caught) { return error(400, "invalid_template", caught instanceof ContractError ? caught.message : "The template payload is invalid."); } const bytes = canonicalBytes(template); const templateDigest = await digest(template); const ownerHash = await sha256(capability); const existing = await repository.findByRequest(ownerHash, requestId); if (existing) { if (existing.digest !== templateDigest) return error(409, "request_conflict", "The request ID is already bound to different publication content."); return json({ shareId: existing.id, url: `${origin}/s/${existing.id}`, digest: existing.digest }, 200, headers(origin)); } const maxPublications = Math.min(MAX_PUBLICATIONS, Math.max(1, Number(env.MAX_PUBLICATIONS) || MAX_PUBLICATIONS)); const maxStorageBytes = Math.min(2621440000, Math.max(MAX_BODY_BYTES, Number(env.MAX_STORAGE_BYTES) || 2621440000)); const maxRetainedRecords = Math.min(MAX_RETAINED_RECORDS, Math.max(1, Number(env.MAX_RETAINED_RECORDS) || MAX_RETAINED_RECORDS)); const record: ShareRecord = { id: id(), requestId, ownerHash, digest: templateDigest, payload: new TextDecoder().decode(bytes), revoked: false, createdAt: new Date().toISOString() }; try { if (repository instanceof D1Repository) await repository.putWithCaps(record, maxPublications, maxStorageBytes, maxRetainedRecords); else if (repository instanceof MemoryRepository) repository.putWithCaps(record, maxPublications, maxStorageBytes, maxRetainedRecords); else await repository.put(record); } catch (caught) { if (String(caught).includes("publication storage cap")) return error(429, "storage_cap_reached", "The publication storage cap has been reached."); return error(409, "request_conflict", "The publication request was already accepted; retry with the same request ID."); } return json({ shareId: record.id, url: `${origin}/s/${record.id}`, digest: record.digest }, 201, headers(origin)); }
    if (match && match[1] && !SHARE_ID.test(match[1])) return error(400, "invalid_share_id", "The share ID is malformed.");
    if (match && match[1] && request.method === "GET") { const record = await repository.get(match[1]); if (!record || record.revoked || !record.payload) return error(410, "share_unavailable", "This publication is unavailable."); return json({ format: "alfred.workflow-template", digest: record.digest, template: JSON.parse(record.payload) }, 200, headers(origin)); }
    if (match && match[1] && request.method === "DELETE") { const capability = owner(request); if (!capability) return error(401, "owner_required", "A valid owner capability is required."); const ok = await repository.revoke(match[1], await sha256(capability)); return ok ? new Response(null, { status: 204, headers: headers(origin) }) : error(404, "share_unavailable", "This publication is unavailable."); }
    const page = url.pathname.match(/^\/s\/([^/]+)$/); if (page && request.method === "GET") { if (!SHARE_ID.test(page[1])) return error(400, "invalid_share_id", "The share ID is malformed."); const record = await repository.get(page[1]); return record ? preview(record, env) : preview({ id: page[1], requestId: "", ownerHash: "", digest: "", payload: null, revoked: true, createdAt: "" }, env); }
    return error(404, "not_found", "Not found.");
  } };
}

const defaultApp = createApp();
const productionApp = createApp(new MemoryRepository(), false);
export default { fetch: (request: Request, env: Env, _ctx: WorkerExecutionContext) => productionApp.fetch(request, env) };
