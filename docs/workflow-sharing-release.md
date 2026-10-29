# Workflow sharing release preparation

This is a prepared runbook, not deployment evidence. The user selected
`https://alfred.nirzhuk.dev` as the sharing origin. The proposed support/report
destination is `https://github.com/Nirzhuk/alfred/issues`; the repository
reports that issues are enabled. No public service or D1 database has been
created by this execution.

The current origin returned Cloudflare HTTP 526 during a read-only check.
Wrangler's saved login has expired and could not refresh in the noninteractive
environment. Authenticate through `wrangler login` in an interactive terminal
when preparing the actual deployment; do not paste credentials into repository
files, chat, or deployment logs.

## Prepared configuration

The checked-in `share-service/wrangler.toml` contains the chosen origin and
support/download URLs, with publication disabled. Its D1 database ID and rate
limiter namespace still need values for the selected deployment account.

The desktop accepts an explicit runtime `ALFRED_SHARE_ORIGIN` override and an
origin captured at build time. After the hosted release gates pass, build the
packaged client with `ALFRED_SHARE_ORIGIN=https://alfred.nirzhuk.dev` set in the
build environment. The compiled value lets Finder-launched applications resolve
links without requiring a shell environment. Builds without either setting keep
sharing unavailable. HTTPS validation still applies to release builds.

Proposed initial product caps are:

| Setting | Initial value |
| --- | --- |
| `MAX_PUBLICATIONS` | `1000` active publications |
| `MAX_STORAGE_BYTES` | `268435456` canonical payload bytes |
| `MAX_RETAINED_RECORDS` | `10000`, including revoked publications |
| Request body | Existing fixed maximum of 262144 UTF-8 bytes |
| Distributed request limiter | Current binding: 120 requests per 60 seconds |
| `PUBLISHING_ENABLED` | `false` until release checks pass |

These caps are proposed configuration, not provisioned capacity. Payload bytes
exclude SQLite indexes and metadata, so database capacity must retain headroom.
Cloudflare currently documents a 500 MB database ceiling on Free and 10 GB on
Workers Paid. Its distributed rate limiter is location scoped and eventually
consistent; D1's conditional insert enforces the exact global application caps.
See the [D1 limits](https://developers.cloudflare.com/d1/platform/limits/) and
[rate limiter contract](https://developers.cloudflare.com/workers/runtime-apis/bindings/rate-limit/).

## Retention wording

Prepared public wording, to confirm against the deployed account and any
operator-created exports:

> Revoking a link immediately removes its template from normal reads. Minimal
> publication metadata remains so retries and repeated revocation stay
> consistent. Deleted content may remain in the hosting provider's recovery
> history for up to 30 days. Revocation does not remove copies someone already
> imported or saved.

D1 Time Travel is always enabled and currently retains seven days on Free or
thirty days on Workers Paid. This implementation creates no additional cloud
backup/export job. Do not promise a shorter bound without checking the actual
account and operator backups. See
[Time Travel and backups](https://developers.cloudflare.com/d1/reference/time-travel/).

## Local evidence before release

From `desktop/share-service/`:

```sh
bun install --frozen-lockfile
bun run check
bun test
bun run test:d1
```

`test:d1` bundles the current Worker and prints its SHA-256. It runs real local
workerd/D1 with temporary storage, then terminates only its own processes and
removes that storage. It does not use the deployment account or a public URL.

The desktop gate is `bun run check` from `desktop/`, plus the sharing-specific
contract, database, provider-fixture and browser acceptance cases. A passing
Worker test does not prove the recipient setup flow, native OS registration,
or packaged cold/warm deep links. Those results remain in the execution ledger
and the per-plan review/UI reports.

## Deployment preparation

Before writing hosted resources, finish the reviewed code and produce the exact
account, database binding, origin route, quota values and migration command for
review. Inspect existing DNS/routes for `alfred.nirzhuk.dev`; do not replace an
existing site merely to obtain the selected hostname. The service owns
`/s/:id` and `/api/v1/shares` routes.

The first hosted configuration keeps publishing disabled. Verify its binding
and schema, escaped preview/support/download links, trusted HTTPS behavior,
and disable-publishing behavior against isolated synthetic publications before
opening writes. Public publishing requires a functioning report/takedown path
and the confirmed retention statement.

For rollback, set publishing disabled without removing existing reads. Do not
roll back or delete D1 to revert a Worker code version. If a database restore
is required, keep read traffic unavailable until later revocations have been
reapplied; restoring an old database must not make a revoked template public
again.

## Current unresolved release gates

- Final local export/setup corrections passed high review. Public preview
  browser QA and the focused visual retest passed with synthetic data.
- The full integrated test gate and complete execution journey have not passed.
- Packaged macOS, Windows and Linux cold/warm deep-link evidence is absent.
- The chosen origin currently returns HTTP 526.
- Cloudflare authentication and concrete account-owned bindings are unavailable.
- Initial quota values and the report/retention operation need release confirmation.

This runbook does not authorize or claim that any of those gates passed.
