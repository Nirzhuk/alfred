# Workflow sharing

Alfred shares reviewed workflow templates through unlisted HTTPS links. A
publication is immutable: editing the local workflow and publishing again
creates a new digest and link. The hosted service receives the reviewed
template, not the publisher's credentials, machine paths, memories, run
history, or connection metadata.

Recipients open the preview page and can use **Open in Alfred**. The dedicated
`alfred-workflows://import/<shareId>` scheme carries only a validated share ID.
When browser-to-app opening is unavailable, **Copy link** followed by Alfred's
**Import from link…** follows the same fetch, validation, and setup flow.

Import is local and transactional. Accounts and resources are selected on the
recipient machine, fresh graph/start IDs are generated, starts remain disabled,
memory retrieval and review begin off, and the flow is never run during setup.
An imported copy retains its source link and digest for reference; it does not
follow future publication edits.

A template declares the oldest schema version that can carry its content.
A workflow with [conditional connections](conditional-connections.md) declares
version 2 and is refused by builds that predate them, because a build that
ignored a condition would run every branch and look correct. Everything else
still declares version 1 and imports anywhere.

## Local service

From `desktop/share-service/`, run `bun install`, `bun run dev`, `bun run test`,
`bun run test:d1`, and `bun run check`. The Worker uses D1 migration
`0001_shares.sql`; `test:d1` runs the real local Worker/D1 path in an isolated
temporary state and covers restart persistence, concurrent idempotency, and
Unicode byte quotas. Configure
an operator-owned HTTPS origin, D1 database binding, quota and rate limits,
backup-retention wording, abuse/support URL, and a disable-publishing switch
before public availability. The committed `wrangler.toml` names the intended
origin `https://alfred.nirzhuk.dev` and the existing GitHub support/download
destinations, but keeps publishing disabled, an empty D1 ID, and an example
rate-limit namespace until the operator provisions the resources. The support
URL is a prepared GitHub Issues destination; confirm its reporting and
takedown process with the operator before public availability. Wrangler's local runtime may
serve on localhost when explicitly started with a local test configuration.

Owner capabilities are generated once and saved through the desktop OS
credential store. Losing that credential removes self-service revocation for
the installation. Revocation removes content from read paths immediately;
operators must document finite backup retention and maintain a takedown route.

The preview uses escaped server-owned HTML with a restrictive CSP,
`Referrer-Policy: no-referrer`, `X-Robots-Tag: noindex, nofollow`, and
`Cache-Control: no-store`. It does not execute submitted HTML, load remote
assets, expose a listing endpoint, or include third-party scripts and tracking.

The Cloudflare rate limiter is distributed but eventually consistent within
locations; D1's conditional insert is the exact gate for global retained-row,
active-publication, and UTF-8 storage-byte caps. Production release still
requires packaged macOS, Windows, and Linux deep-link registration evidence
and an operator-configured origin. Local workflows and previously imported
copies remain usable when the sharing service is offline.
