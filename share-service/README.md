# Workflow share service

This package is a standalone Cloudflare Worker/D1 implementation for Alfred's
unlisted immutable workflow links. It is intentionally deployable without being
configured for a production account: set `SHARE_ORIGIN`, create the D1 binding,
and apply migrations before release. Production requests also require a
distributed `RATE_LIMITER` binding; the Worker returns a configuration error
when that binding is absent. The checked-in `wrangler.toml` names the intended
HTTPS origin and existing support/download destinations, but still uses an
example rate-limit namespace and an empty D1 ID; replace those resources with
ones owned by the deployment account before deploying. Its example limiter
allows 120 requests per 60-second window. `bun run dev` uses the local Worker runtime;
`bun run test` exercises publish/read/retry/revoke, bounded streaming input,
quotas, and escaped preview behavior.

For browser QA, `bun run test:preview` starts a read-only fixture at
`http://127.0.0.1:1423` using the actual service preview handler and synthetic
in-memory publications. It prints normal, malicious-text, and revoked preview
URLs. It neither reads a deployment account nor publishes externally. Stop it
with Ctrl+C. Use `bun run test:d1` for real local Worker/D1 persistence checks.

The service stores only the canonical reviewed template, its SHA-256 digest, an
owner capability hash, and a durable request ID. The capability is never in a
URL or page. Configure `MAX_PUBLICATIONS`, `MAX_STORAGE_BYTES`, and
`MAX_RETAINED_RECORDS` for operator quotas. `RATE_LIMIT_PER_MINUTE` and
`RATE_LIMIT_ENTRIES` apply only to local development fallback rate limiting;
production must use the distributed binding. Cloudflare's limiter is shared
across isolates with eventual consistency, so D1 remains the source of truth
for publication counts, retained tombstones, and storage bytes.
