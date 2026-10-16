# Alfred Implementation Plans

Plans are handoff documents for zero-context executors. Execute each track in
its own dependency order, honor every STOP condition, and update status after
verification.

Status values: `TODO`, `IN PROGRESS`, `DONE`, `BLOCKED (reason)`, or
`REJECTED (reason)`.

---

## Track K — Share workflow templates by link

Let a user publish a reviewed, reusable workflow template and send an HTTPS
link. A small preview page opens Alfred, where the recipient chooses local
accounts, resources, and paths before creating an editable copy. No account
signup, public gallery, file exchange, or cloud execution is required.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 042 | Share workflow templates by link with guided local setup | P1 | L | Existing workflow/Connected Apps foundation; independent of 011 | IN PROGRESS — local review/UI passed; shared and packaged/release gates pending |

Open [Plan 042](042-share-workflows-by-link.md) for the portable template
contract, desktop publishing/import UI, small hosting service, deep-link
lifecycle, revocation, and acceptance cases. Imported start settings remain
disabled. Schedule nodes are planned in [Plan 043](043-schedule-node-on-canvas.md)
and are not a dependency.

---

## Track L — Canvas schedules and follow-up tracking

Manage existing schedules through a connected canvas node, and add persistent
follow-up tracking for explicitly selected Slack threads. These are separate
plans: the tracker has its own durable checking jobs and does not depend on a
workflow remaining active or on the schedule-node UI.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 043 | Manage schedules through a connected canvas node | P1 | M | Existing scheduler and canvas | TODO |
| 044 | Track follow-ups and prepare replies for review | P1 | XL | Existing Connected Apps; selected-thread reader and Inbox built in this plan | TODO |

[Plan 043](043-schedule-node-on-canvas.md) preserves one canonical schedule,
existing execution semantics, manual runs, and the global overview.
[Plan 044](044-follow-up-tracker.md) defines selected-thread monitoring,
responsibility states, bounded analysis, the first review Inbox, and explicit
user-approved reply delivery. Public Slack distribution and broader providers
remain separate from its initial private/local Slack support.

---

## Track N — Conditional connections

Let a connection between two canvas steps carry a condition so a branch runs
only when it should. Connections without a condition keep today's behavior
exactly, so existing workflows need no migration.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 053 | Conditional connections on the workflow canvas | P1 | M | Existing canvas, workflow store, and runner graph walk | IN PROGRESS — implemented and unit-tested; desktop visual/run verification pending |

[Plan 053](053-conditional-connections.md) defines the edge-level condition
model, the three runner rules that gate execution and propagate skips, the
`skipped` step status, the canvas popover, and the template-contract change
that keeps shared workflows from importing with their conditions stripped. It
adds no Condition node type, no expression language, and no failure-tolerant
steps.

---

## Track O — Live workflow inspection

Show the exact display-safe input a step received alongside its output while the
workflow is still running.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 054 | Live step inputs and outputs on the workflow canvas | P1 | L | Stable Plan 053 runner levels and existing run event bridge | IN PROGRESS — implementation/check green; desktop visual verification pending |

[Plan 054](054-live-step-inputs-on-canvas.md) defines versioned runtime-input
snapshots, live event and History parity, the tabbed node result card, the full
step-data viewer, redaction, and concurrent-run isolation.

---

## Track M — Reliable agent memory

Turn the [8 September memory audit](../docs/memory-audit-2026-09-08.md) into
measurable retrieval and learning improvements. These plans retain SQLite,
existing memory scopes, user-controlled pins/selections, and approval of learned
claims. Plans 025–028 remain DONE as historical v1 implementations; the policies
below are the selected next version, not a reason to repeat those migrations.

Written with the improve skill against `85f0d64` and the dirty working tree on
2026-09-08. The original planning baseline was reviewed by three read-only
subagents covering retrieval/context, curation/provider boundaries, and
output/tool integration. The implementation checkpoints below now record local
code, fixture, review, and UI evidence as it landed. Live provider validation is
still excluded unless a status explicitly records an authorized runtime gate;
historical planning prose and checkpoints remain labeled rather than treated as
current acceptance.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| [045](045-memory-quality-evaluation.md) | Establish a reproducible memory quality baseline | P1 | M | Existing 025–028 | DONE (verified 2026-09-15: `bun run check` 548 frontend + 1,072 Rust tests, 0 failures) |
| [046](046-restricted-memory-curator.md) | Restrict the memory curator to structured suggestions | P1 | L | Existing review and provider contracts | BLOCKED — live activation only; contract/conformance accepted; full check green 2026-09-15 |
| [047](047-task-aware-memory-recall.md) | Retrieve task-relevant memory and abstain on unrelated work | P1 | L | 045 | DONE (high review/UI accepted; full check green 2026-09-15) |
| [048](048-shared-memory-context-budget.md) | Bound every memory contribution to an agent prompt | P1 | L | 047 | DONE (high review/UI accepted; composition report frozen; full check green 2026-09-15) |
| [049](049-evidence-backed-memory-curation.md) | Ground learned claims in verifiable source evidence | P1 | L | 046 contract/conformance milestone, 048 | DONE (high review/UI accepted; full check green 2026-09-15) |
| [050](050-memory-corrections-and-validity.md) | Correct stale or duplicate claims with version and validity checks | P1 | L | 047, 049 | DONE (high review/UI/evaluation accepted; full check green 2026-09-15) |
| [051](051-backend-output-persistence.md) | Persist saved outputs once from the executing run | P1 | L | 048 | DONE (local implementation and recovery repairs accepted; full check green 2026-09-15) |
| [052](052-agent-memory-lookup-tools.md) | Add bounded on-demand memory and history reads | P2 | L | 050, 051; verified provider tool transport for activation | BLOCKED — live activation only; implementation reviewed and fixed; full check green 2026-09-15 |

### Execution order

1. Start **045 and 046**. The synthetic baseline is independent of the
   restricted-provider execution contract. They can be developed in separate
   worktrees with distinct file ownership.
2. Land **047**, then **048**. Query-source tracking and ranking versions must
   stabilize before changing final prompt assembly and explicit-memory traces.
3. Land **049**, then **050**. Preserve source evidence before using it to
   suggest corrections, compare versions, or assign validity dates.
4. Land **051**. It may be prepared after 048, but serialize its runner/schema
   integration with 049/050. Saved output is execution state with its own retry
   identity, and ordinary recall eligibility is already owned by 047.
5. Execute **052** as the later capability extension. Its bounded service,
   fake-native boundary, UI, and evaluator can be verified locally; callable
   tools remain blocked until the exact release/runtime gates and an authorized
   declaration-to-result path pass.

Read-only research, fixture design, and review can run in parallel. Never let
two executors edit `runner/mod.rs`, `schema.sql`, `migrate.rs`, shared memory DTOs,
or the library store concurrently in the same checkout. One integration owner
rebases and verifies those files between plans. Do not resolve migration
conflicts by renumbering or discarding already-applied schema markers blindly.

The first restricted curator and on-demand tool route both require a verified
provider capability. Preserve existing runtime release/account/billing gates.
An unavailable provider must not trigger a switch to a normal coding CLI or
another billing product. The 046 contract/conformance milestone can unblock
049's offline implementation; it does not enable live review. Record outstanding
activation gates explicitly and do not mark a user-facing capability DONE on
fake-provider evidence alone.

### Shared contracts and integration ownership

- **045** owns versioned synthetic cases, honest v1 results, fixed clocks, and
  separate development/validation metrics. Later plans add gates without
  relabeling cases to hide misses.
- **047** owns Automatic versus Review retrieval purpose, the ten-type
  eligibility policy, bounded task/event query origins, and ranking versions.
- **048** owns structured prompt/memory origins, final inclusion/exclusion,
  bounded artifact excerpts, a 12,000-byte total memory allowance, and explicit
  use/omission traces. This is a memory-byte limit, not a provider token limit.
- **049** owns evidence identity, versioned review inputs, legacy provenance,
  approved-evidence retention, and source-deletion semantics. **050** owns
  target/candidate revision checks and operation-specific correction rules.
- **051** owns execution-time Output settings, run/node save receipts,
  artifact recovery, and retries that never rerun the agent. It preserves
  manual/curated-memory creation and the user's existing save preferences.
- **052** owns scoped read-only tool contracts and provider capability gating.
  It reuses 047/048/049/051 rather than exposing unrestricted frontend History
  commands or granting broad shell access.

Plans 042–044 are integration constraints, not prerequisites. Sharing excludes
runtime memory/evidence/receipts; schedule canvas nodes remain outside executable
graphs; follow-up tracking keeps its own evidence and delivery lifecycle.
Coordinate overlapping runtime and migration edits without folding those
features into this track.

### Verification and deferred choices

Each plan includes focused test commands, migration cases where applicable,
source boundaries, and done criteria. Run `bun run check` before any integrated
implementation is marked DONE. Reconcile the existing dirty tree first;
historical green test results are context, not a substitute for new verification.
The audit's Rust runs passed 64 and 12 tests with overlapping filters, and five
Bun files passed 39 tests; those are historical audit checkpoints. The final
2026-09-15 `bun run check` passed 548 frontend tests, the frontend build, and
1,072 Rust tests with 0 failures, superseding the earlier 1,068-pass/3-fail
checkpoint. The native startup evidence below is scoped and does not satisfy
provider/lookup transport or visual acceptance gates.

A separate isolated native Tauri startup smoke passed exit 0 on the
`uniqueQA` debug bundle: Boot 1 seeded 65 Pending, one current-time `Saving`,
Failed, and Deleted receipts; Boot 2 recovered 66 Saved receipts and verified
inline/large bodies plus canonical artifact/manifest digest, state, and length,
while Failed/Deleted stayed unchanged. This is backend startup evidence only,
not WKWebView visual interaction, provider activation, or lookup transport.

The reviewer/tool plans require explicit bounded live verification only for
activation; ordinary test suites use synthetic data and no provider calls.
Report retrieval metrics separately from actual downstream answer quality.

Considered and deferred or rejected:

- **Wholesale memory-store replacement:** rejected for this track. Existing
  scope, lifecycle, transaction, and history support can carry these changes.
- **Embeddings/vector infrastructure:** deferred until the fixed evaluation
  shows remaining semantic misses worth the model, storage, privacy, and latency
  costs. Plan 047 makes no unrestricted semantic-search claim.
- **Automatic approval or deletion of old memories:** rejected. Corrections
  remain reviewable; no uniform expiry removes stable preferences.
- **Broad agent-private/team memory or cloud sync:** deferred. The selected
  work uses the current local-user/workspace/workflow identities.
- **General agent/provider/scheduler redesign:** outside this audit. Only the
  memory-related execution seams and documented integration constraints were
  reviewed for this track.

---

## Track A — Paid official release

The previous licensing, freemium, updater, signing, and Homebrew drafts have
been reconciled into one executable Polar-first 001–006 path:

**[Open the canonical paid-release roadmap](release-money/README.md)**

Start with the fully local
[release-money Plan 001](release-money/001-connect-desktop-polar-licensing.md),
which builds Tauri's direct Polar license client, secure key storage, mock
contract tests, and 30-day offline window without requiring a Polar login.
[Plan 002](release-money/002-build-polar-license-settings.md) builds the UI with
injected fixture links. Polar dashboard configuration is isolated in the later
operator-assisted Plan 003, after the executable code path is green.

Product boundary:

- Public GPL source/self-hosted builds remain usable without purchase.
- Desktop is annual or lifetime per named user.
- Company is monthly or annual by purchased seat quantity; every claimed seat
  includes Desktop. Revoking a member frees the assignment but the buyer must
  reduce quantity separately to lower the bill.
- Polar is merchant of record and owns checkout, tax, receipts, subscriptions,
  customer email authentication, keys, device activations, Company seats,
  portal, and protected downloads.
- Payment controls official signed downloads, support, and future hosted
  services—not local workflow execution.
- v0.5.0 updates are manual through Polar's portal. A paid automatic updater is
  deferred because it would require public assets or a separate authenticated
  update service.

Completed signing evidence and deferred Homebrew work live beside the roadmap.
The custom Bun/Stripe service and authenticated update gateway are rejected;
do not deploy or integrate them. Root plans 001–006 and 022 are compatibility
pointers only; do not execute them as separate tracks.

---

## Track C — Agent authentication feedback

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 007 | Surface agent authentication failures with an actionable toast | P1 | M | — | DONE |

### Dependency notes

- 007 is independent of the licensing and distribution tracks. It only
  surfaces provider-owned CLI authentication failures and does not manage
  credentials.

---

## Track D — Connected apps and Microsoft Copilot

Generated by the `improve` skill on 2026-08-11 after surveying the Tauri/React
workflow model, Rust runner, trigger runtime, SQLite schema, settings UI, and
current official provider integration documentation.

Plans 008–014 were reconciled on 2026-08-13 after a second audit. Corrected
improvement drafts were merged into these canonical files and the duplicate
`plans/improvements/` folder was removed.

### Product and architecture decisions

- Connected apps are separate from coding-agent providers. Slack, Outlook,
  Gmail, GitHub, and Copilot must not be added to `AgentProviderId`.
- Provider credentials live in the operating-system credential store. SQLite
  contains redacted connection metadata only; workflow graphs and React state
  never contain OAuth access/refresh tokens.
- All provider actions use one generic `appAction` node and backend registry.
  Providers do not add runner match arms or provider-specific React Flow types.
- All provider triggers normalize and minimize external content before it is
  persisted or injected into a workflow. Message, mail, and document text is
  always labeled as untrusted external data.
- Local polling/Socket Mode is an honest first release: it works only while
  Alfred is running, including tray mode.
- A thin cloud relay is optional for local/native-PKCE integrations but
  mandatory for confidential OAuth callbacks, public webhooks/offline queues,
  Slack bot-scope installation, and Microsoft Copilot. Workflow and agent
  execution remains on the paired desktop.
- Microsoft Copilot is a remote control surface over an authenticated API—not a
  model/CLI that executes an Alfred node.
- Provider rollout order is Slack + Microsoft + Gmail, then GitHub/Linear,
  knowledge sources, and finally broader Sentry/SharePoint depth based on usage.
- Personal messaging notifications are outbound-only and use fixed self
  destinations. Telegram uses the official Bot API and ships first. WhatsApp is
  a separate, visibly experimental QR-linked-device integration: it supports
  one account, sends only to that account's self-chat while Alfred runs, and
  persists only encrypted protocol state.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 008 | Build the secure Connected Apps foundation | P0 | L | — | CODE COMPLETE (gates + security invariants verified at `5e62adf` 2026-08-20; only the **packaged Linux** Secret Service credential-store smoke remains here — the signed-macOS and packaged-Windows smokes moved to [release-money 005](release-money/005-run-polar-paid-release-acceptance.md) matrix E rows E10/E11 on 2026-08-20) |
| 009 | Add a descriptor-driven app action framework | P0 | L | 008 | DONE |
| 010 | Add app events, polling, and subscription lifecycle | P0 | XL | 008 (009 recommended) | DONE (archived) |
| 011 | Specify and build the optional cloud relay | P1 / Copilot P0 | XL | 008; 010 for events | BLOCKED (ADR approvals pending) |
| 012 | Add the Slack connected app | P0 | L–XL | 008, 009; 010 events; 011 public bot OAuth | IN PROGRESS (private actions + local mentions green; manual/PKCE/public pending) |
| 013 | Add Outlook Mail and Calendar through Microsoft Graph | P0 | XL | 008, 009; 010 events; 011 webhooks | IN PROGRESS (native auth + actions + local events; webhooks gated on 011) |
| 014 | Add Gmail with staged, least-privilege access | P0–P1 | L–XL | 008, 009; 010 events; 011 push | TODO |
| 015 | Add GitHub, Linear, and Sentry in priority order | P1–P2 | XL | 008, 009; 010 events; 011 public modes | IN PROGRESS (GitHub/Linear/Sentry local modes green; public OAuth, relay webhooks, and live smoke pending) |
| 016 | Add Notion, Google Drive, and SharePoint context sources | P1–P2 | XL | 008, 009; 010 events; 011 public modes | IN PROGRESS (private Notion + local Obsidian retrieval; Drive/Microsoft auth gates pending) |
| 017 | Expose selected Alfred workflows to Microsoft Copilot | P1 | XL | 008, 011 | TODO |
| 021 | Send personal notifications through Telegram | P1 | M | 008, 009 | DONE (archived) |
| 023 | Send self-notifications through an experimental WhatsApp linked device | P1 | XL | 008, 009; execute after 021 | DONE (macOS smoke; Windows/Linux gates still off) |

### Recommended execution slices

1. **Foundation**: 008 → 009 → 010. App actions (009) and local events (010)
   are done
   ([archive/010-app-events-sync-framework.md](archive/010-app-events-sync-framework.md)).
   008's only remaining gate is the packaged **Linux** Secret Service
   credential-store smoke; the signed-macOS and packaged-Windows equivalents
   are now run by release-money 005 matrix E.
2. **Local useful beta**: Slack private/BYO mode from 012, Microsoft native PKCE
   actions from 013, and Gmail send-only from 014. Do not wait for the relay.
3. **Personal notifications**: Telegram (021) and experimental WhatsApp
   self-send (023) are done. WhatsApp is enabled on macOS after maintainer
   smoke; Windows and Linux stay hidden until those packaged gates pass.
   Neither plan waits for the relay.
4. **Remote-capability gate**: execute Plan 011's ADR/threat model. If approved,
   build public Slack bot OAuth/webhooks, Graph/Gmail push, and offline queues.
5. **Core developer loop**: GitHub, Linear, and Sentry local modes from 015
   are implemented (GitHub App device auth; Linear personal API key; Sentry
   auth token). Public OAuth and relay webhooks for all three wait on Plan 011;
   measure Sentry demand before investing in its public mode.
6. **Knowledge retrieval**: Notion then Google Drive from 016; SharePoint follows
   enterprise demand and policy review.
7. **Copilot**: execute 017 only after the relay, Entra tenant model, remote
   publication, queue, revocation, and security tests are production-ready.

### Explicitly deferred or rejected

- Rejected: one hard-coded workflow node and runner branch per provider.
- Rejected: OAuth tokens/client secrets in workflow JSON, SQLite, frontend
  stores, logs, generic HTTP headers, or the desktop binary.
- Rejected: presenting a local loopback webhook as a Microsoft Copilot endpoint.
- Deferred: cloud execution of workflows/agents, workflow sync/collaboration,
  full mailbox/document indexing, arbitrary provider plugin code, and cloud
  provider-token custody.
- Deferred: Slack DMs, broad channel-history search/indexing, and admin APIs;
  selected-thread follow-up reads are planned in [Plan 044](044-follow-up-tracker.md).
  Also deferred: Gmail attachments/bulk mail, Microsoft
  application mailbox permissions, and tenant-wide Drive/SharePoint crawling.
- Deferred: Telegram/WhatsApp inbound events, delivery tracking, dynamic
  recipients, broadcasts, media, and managed messaging accounts.
- Accepted only inside Plan 023's narrow experimental boundary: unofficial
  WhatsApp Web linked-device automation for one paired account's self-chat,
  with mandatory disclosure, encrypted local protocol state, no inbound data,
  and per-platform release gates. Other consumer-account automation remains
  rejected.

### Track D verification baseline observed on 2026-08-11

- `bun test` passes 3 tests.
- `bun run build:frontend` passes; Vite reports the existing >500 kB chunk
  warning.
- `cargo test --manifest-path src-tauri/Cargo.toml` passes 9 tests with the
  existing `compose_prompt_with_skill` dead-code warning.
- At the 2026-08-11 baseline there was no usable Git `HEAD`. The repository was
  later initialized; Plans 008–010 were reconciled at commit `36835c9` on
  2026-08-13 and retain SHA-256 drift anchors for source changes.
- Repository-wide formatting remains outside these plans; do not create broad
  unrelated formatting diffs.

---

## Track E — Live agent activity and living-node motion

Generated by the `improve-animations` skill on 2026-08-11 after auditing the
React Flow canvas, per-workflow run state, activity panel, provider CLI adapters,
and existing CSS motion/reduced-motion rules.

### Product and motion decisions

- The working node owns the execution signal. Edges stay static because they
  represent dependency, not measurable transfer or percent progress.
- The provider glyph is never distorted. A separate compositor-only orbit says
  the agent is active; a slower opacity-only node perimeter makes the active
  step easy to locate.
- Motion is not the only signal. `Working`, the active node label, and an
  `Open console` action remain visible; reduced-motion users get the same state
  as static rings/borders/text.
- “Thinking” means a safe phase label. Raw provider chain-of-thought/reasoning
  text is never placed in Tauri events, Zustand state, logs, or the console.
- The console exposes observable status, tool calls, commands, file activity,
  safe tool results, and assistant messages. It is view-only and attached to
  the running child stream.
- Starting a run does not force the Activity panel open or resize the canvas.
  The user opens the console from the working node or the titlebar Live control.
- A separate OS terminal is not used for live inspection because it cannot
  attach to the already-running child process.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 018 | Stream safe agent activity in real time | P0 | L | — | DONE |
| 019 | Open a node-scoped live console | P0 | L | 018 | DONE |
| 020 | Give the active node a living signal | P1 | M | 018, 019 | DONE |

### Recommended execution order

1. **018 — truthful data first:** stream provider activity, normalize it, filter
   reasoning content, upsert lifecycle rows, and remove the artificial 350ms
   inter-node delay.
2. **019 — inspection:** make the console node-scoped and on demand, preserve
   user scroll ownership, and convert the panel to a transform/opacity overlay
   so the canvas stays still.
3. **020 — motion:** add the provider orbit and contained node cadence only
   after the underlying stream and console affordance are real.

### Track E implementation verification — 2026-08-11

- `cargo test --manifest-path src-tauri/Cargo.toml` passes 32 tests; the existing
  `compose_prompt_with_skill` dead-code warning remains.
- `bun test` passes 26 tests, including 14 store tests for activity lifecycle,
  console scope continuity, run-start behavior, and cancellation.
- `bun run build:frontend`, `cargo fmt --check`, and `git diff --check` pass;
  Vite retains its existing >500 kB chunk advisory.
- Static motion review confirms transform-only provider rotation, opacity-only
  node cadence, opacity/transform-only drawer motion, reduced-motion fallbacks,
  and static React Flow edges.
- Paid-provider live smoke runs and the recording/theme/zoom feel checks remain
  explicitly unchecked in the individual plan files.
- At that implementation baseline the workspace had no usable Git `HEAD`; the
  changes were applied in place while preserving the existing staged worktree.

---

## Track J — Stylesheet weight

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 041 | Split `src/App.css` into per-window and per-feature stylesheets | P3 | M | — | IN PROGRESS |

### Dependency notes

- 041 is a mechanical refactor with no visual intent: no rule may change its
  declarations, ordering, or specificity. It is independent of every other
  track and each of its four steps ships on its own.
- Steps 1 and 2 (token/base extraction and the Quick Access stylesheet) landed
  with the Quick Access memory work, which needed the popover to stop importing
  the whole design system. Steps 3 and 4 are still TODO.
- The blocker for the remaining steps is the CSS-contract test suite: six test
  files read `src/App.css` as a single string, so moving rules out of it fails
  those tests until they are taught about the new layer files.

---

## Track H — Agent model picker

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 029 | Fast model toggle across agent providers | P2 | M | — | DONE |

### Dependency notes

- 029 is independent of licensing, Connected Apps, and memory tracks. It extends
  agent model discovery metadata and the workflow model picker only.
- Cursor and Codex are the first targets because they expose explicit `*-fast`
  slugs and (for Cursor) IDE `variants` with a documented `fast` parameter.
- Claude Code and OpenCode follow only after live CLI output confirms stable
  base ↔ fast pairing rules.

---

## Track F — Workflow script steps

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 024 | Script steps and agent script hints | P2 | M | — | DONE (archived) |

### Dependency notes

- 024 is independent of every other track. It touches the workflow node model,
  the Rust runner, and the Input node only; it adds no provider, credential, or
  entitlement surface.
- 024 supersedes the Shell node in the Add-step palette but deliberately leaves
  the `shell` node type, runner arm, and styling in place so saved graphs keep
  working. Do not schedule a Shell removal or migration off the back of it.
- Completed plan: [archive/024-script-node.md](archive/024-script-node.md).

---

## Track G — Persistent, searchable agent memory

Generated by the `improve` skill on 2026-08-18 after surveying Alfred's
SQLite schema/migrations, Rust workflow runner and CLI adapters, Tauri command
surface, workflow Zustand store, Memories inspector, Activity rail, History
gap, settings patterns, tests, product specs, and current implementation plans.

This track combines four decisions from the Hermes/Cumora comparison without
turning Alfred into a chat-agent clone: exact run history remains canonical;
memories become small scoped records; recall is local, bounded, deterministic,
and explainable; model-authored learning is staged for approval.

### Product and architecture decisions

- Raw `runs`, `run_steps`, and `memories` are canonical. FTS5 and future
  retrieval indexes are derived and rebuildable.
- Memory has user, workspace, and workflow scopes. Content kind remains
  separate from semantic type and lifecycle.
- Pinned core memory and retrieved memory have hard UTF-8 byte budgets and are
  framed as untrusted reference data, never authorization or instructions.
- Existing workflows opt into automatic recall; new workflows default it on.
- V1 retrieval uses local FTS5, scope, recency, salience, and confidence. No
  embedding API, model download, vector extension, or network dependency is
  introduced before quality and privacy requirements are measured.
- Every recalled memory is auditable by run and step without duplicating its
  body in trace tables.
- Background review is off by default, may add one explicitly configured model
  invocation after an eligible completed run, and can create suggestions only.
  Canonical memory changes always require user approval.
- Skills/procedural self-modification, external memory providers, cloud sync,
  and auto-approval are separate future decisions.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 025 | Make run history and saved memories searchable | P0 | L | — | DONE |
| 026 | Turn workflow notes into scoped atomic memory | P0 | L | 025 | DONE |
| 027 | Retrieve relevant memory automatically for agent steps | P0 | L | 025, 026 | DONE |
| 028 | Curate memory through reviewable post-run suggestions | P1 | XL | 025, 026, 027 | DONE (verified 2026-08-25 at `c1969ab`: `bun test` 304 pass, focused memory-curation tests 17 pass, `bun run build:frontend`, full `bun run check` incl. cargo suite, and `git diff --check` all clean) |

### Required execution order

1. **025 — exact history first:** add transactional FTS5 indexes, safe
   plain-text query construction, exact run details, and a local History page.
2. **026 — stable memory model:** migrate existing rows/links/artifacts into
   user/workspace/workflow-scoped atomic records and bound pinned core context.
3. **027 — deterministic recall:** rank visible memory per agent step using
   FTS5 + recency + metadata, preserve an off switch, and persist inclusion
   traces with “why included?” UI.
4. **028 — consent-aware learning:** run an explicitly enabled bounded
   post-completion review, validate strict JSON suggestions, and require
   transactional approval for create/supersede/retract.

Do not parallelize schema work across these plans. Plan 026 rebuilds the memory
table and must reconcile Plan 025's FTS index; Plan 027 consumes both contracts;
Plan 028 writes through all three.

### Track G verification baseline — 2026-08-18

- Planned at Git commit `62ff2bf` with a large unrelated Connected Apps dirty
  worktree. Executors must preserve overlapping edits in `runner/mod.rs`,
  `lib.rs`, `App.css`, and this index.
- `bun run build:frontend` passes.
- `bun test` passes 103 tests and has two pre-existing copy-assertion failures
  in dirty Connected Apps tests (`github-connect` and
  `notion-private-connect`). These are baseline noise, not acceptable permanent
  exemptions; the full gate must pass on a normal clean integration branch.
- Rust compiles and 133 non-network tests pass in the restricted planning
  sandbox. Twenty-seven integration tests that bind local fixture servers fail
  there with `PermissionDenied`; run the complete Rust gate on a normal host/CI.
- Every plan includes focused non-network Rust/frontend commands plus the final
  `bun run check` gate. Do not weaken existing tests or convert environmental
  failures into ignored tests.

### Explicitly deferred or rejected

- Deferred: semantic embeddings until Alfred has retrieval-quality metrics, a
  privacy/consent decision, a local model lifecycle, and a proven need beyond
  FTS5 + recency.
- Deferred: memory export/import, physical retention controls, team identity,
  sync, and external providers.
- Deferred: procedural skill learning; factual candidate quality must be
  measured first.
- Rejected for this track: one giant unbounded `MEMORY.md`, injecting all past
  runs, remote-only vector search, silent cross-provider review, hidden token
  spend, direct model writes, and auto-approval.
---

## Track I — Dual CLI and Alfred-native agent harnesses

This track keeps every existing provider CLI as a first-class harness while
adding an Alfred-owned native harness. Provider, stable product, managed
runtime, billing owner, and harness are separate dimensions. A provider may
have both a subscription product and an API/PAYG product without sharing
credentials, entitlement, or fallback.

### Product and architecture decisions

- `cli` is supported, not legacy. Existing users keep the provider's own
  harness, account, plugins, configuration, and behavior.
- `alfred` is additive. Existing graphs without a `harness` field default to
  `cli`; migration never silently switches billing, credentials, or runtime.
- Native credentials use a dedicated agent-account store. Alfred never scrapes
  provider CLI keychains, auth files, local databases, or private bundles.
- Managed subscription products store an opaque runtime profile reference and
  never a fake secret reference. Both `runtime_profile_ref` and
  `credential_ref` are excluded from command DTOs and React state.
- Stable product IDs are `claude_code_subscription`, `claude_api`,
  `chatgpt_codex`, `openai_api`, `opencode_go`, `opencode_zen`,
  `cursor_cloud`, `github_copilot_subscription`, `gemini_api`, and `grok_api`.
- Billing source/owner is independent from entitlement. Entitlement is an
  observed `unknown|eligible|limited|exhausted|ineligible` state with source
  and timestamp; it never triggers cross-product fallback.
- Native provider capabilities are declared per provider, auth method,
  platform, and runtime version. Unsupported capabilities are visible rather
  than emulated silently.
- ChatGPT Codex targets the stable Python SDK and exact binary package first;
  raw App Server remains an approval-gated alternative. Claude subscription
  targets an unmodified isolated Claude Code binary. OpenCode Go targets a
  managed local server. None is packaged or registered in this phase.
- Every native descriptor declares `alfred_executed`,
  `runtime_executed_with_host_approval`, or `no_tools`. Capability flags do not
  imply execution ownership.
- Existing dedicated key intake for Claude API, Gemini API, and Grok API
  remains an account-only surface. It does not register or make a provider
  runtime available.
- Native failure never falls through to CLI automatically. The user chooses
  the account and harness explicitly.

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 030 | Make CLI and Alfred harnesses first-class | P0 | L | — | DONE (reconciled 2026-08-26; graph compatibility and fail-closed routing verified below) |
| 031 | Add secure native-agent accounts and credential lifecycle | P0 | L | 030 | FOUNDATION DONE; managed-product contract verification pending |
| 032 | Define the native harness compatibility contract | P0 | L | 030, 031 | FOUNDATION DONE; capability v3 tool-owner verification pending |
| 033 | Run Codex through Alfred with ChatGPT Codex | P0 | XL | 030–032 | BLOCKED (stable Python package/lifecycle, public approval/cancellation coverage, packaging, and smoke missing) |
| 034 | Add managed Claude products without weakening CLI support | P1 | XL | 030–032 | BLOCKED (unmodified binary packaging/profile/tool-approval smoke missing; API route remains separate) |
| 035 | Add a native Cursor harness | P1 | XL | 030–032 | BLOCKED (API-key intake, repository consent, and per-tool approval missing) |
| 036 | Add an Alfred-managed OpenCode server | P1 | XL | 030–032 | BLOCKED (commercial ToS clarification, package/profile lifecycle, transient Go-key handoff, and host-approved tool smoke missing) |
| 037 | Add a native GitHub Copilot harness | P1 | XL | 030–032 | BLOCKED (SDK package/license notices and live seat/SSO smoke missing) |
| 038 | Add a native Gemini harness | P1 | L–XL | 030–032 | BLOCKED (live API-key smoke missing; desktop OAuth packaging remains separate) |
| 039 | Add a native Grok harness | P2 | L–XL | 030–032 | BLOCKED (live API-key smoke missing) |
| 040 | Release and roll out the dual-harness system | P0 | XL | 030–039 | DONE (zero-native fail-closed rollout verified) |

### Plans 030–032 reconciliation evidence — 2026-08-26

- **030:** `src-tauri/src/agents/mod.rs` keeps `cli` and `alfred` additive,
  defaults missing graph values to `cli`, preserves every CLI adapter, and
  routes native execution without a CLI fallback. The runner, workflow store,
  target-scoped model catalog, and editor consume the same harness choice.
- **031:** the managed-product cutover adds registry-validated product/runtime
  identity, nullable profile/secret references, billing ownership, and sourced
  entitlement observations. Managed subscription accounts resolve only by
  profile; both opaque references remain outside command DTOs.
- **032:** capability contract v3 adds explicit tool execution ownership and
  rejects contradictory descriptor/capability combinations.
- **Verification:** the earlier foundation gates remain historical evidence.
  Per the Phase 1 execution boundary, no formatter, build, or test was run for
  the 2026-08-26 managed-product cutover; focused and full gates are pending.

### Required execution order

1. **030 — stable workflow contract:** add `harness: cli|alfred`, preserve old
   graphs, and route native-unavailable states explicitly.
2. **031 — credential boundary:** add agent-specific account metadata,
   secure storage, authorization attempts, refresh, revoke, and redaction.
3. **032 — runtime contract:** freeze normalized events, tools, permissions,
   sessions, capability declarations, and conformance fixtures.
4. **033 — Codex first:** use the stable Python SDK/package route; keep raw App
   Server approval-gated and fail closed if public SDK approval/cancellation is
   insufficient.
5. **034–039 — provider gates:** run each provider's policy/runtime decision
   independently. A blocked provider does not block CLI support or other native
   providers.
6. **040 — rollout:** enable native modes per provider/platform only after
   packaged smoke, account recovery, redaction, and no-fallback gates pass.

### Parallel execution boundaries

- Plans 034–039 may run in parallel after Plans 030–032 freeze their contracts.
- Each provider owns its runtime/auth/usage adapter and provider fixtures.
- No provider plan may edit the core runner's semantics or account schema
  independently; use the registered runtime and account seams.
- Plan 033's packaged-runtime decision is a release dependency for Codex only,
  not a reason to install Codex CLI or block the other providers.
- Plan 040 is serialized after the provider slices and owns final integration,
  packaging, capability manifest, and release gates.

### Explicitly deferred or rejected

- Rejected: renaming current CLIs to `legacy`, removing them, or hiding them.
- Rejected: one universal OAuth token/account across providers.
- Rejected: importing or scraping CLI credential files/keychains.
- Rejected: silently retrying a failed Alfred-native turn through a CLI.
- Deferred: cloud execution of Alfred workflows and remote token custody.
- Deferred: custom third-party subscription renderers or credential
  intermediation without an official contract and required approval.
- Deferred: full native parity for provider-specific plugins, subagents, MCP,
  cloud agents, and proprietary UI features until each provider passes Plan 032.
