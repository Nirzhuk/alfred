# Plan 052: Verify bounded memory lookup tools for a supported agent runtime

> Implementation checkpoint. The bounded service, fake-native boundary, separate
> permission, UI, and evaluator are implemented locally; live activation remains
> a feasibility-gated blocker. Contract/conformance evidence does not advertise
> a production provider. Run all commands from `desktop/` and update status only
> to reflect achieved evidence.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/db src-tauri/src/agents/native src-tauri/src/runner/mod.rs src-tauri/src/lib.rs src-tauri/src/commands/mod.rs src/features/workflow tests/memory-retrieval.test.ts tests/fixtures/memory-quality docs/agent-memory-lookup-feasibility.md`,
> and `git diff HEAD -- src-tauri/src/db src-tauri/src/agents/native src-tauri/src/runner/mod.rs src-tauri/src/lib.rs src-tauri/src/commands/mod.rs src/features/workflow tests/memory-retrieval.test.ts tests/fixtures/memory-quality docs/agent-memory-lookup-feasibility.md`. Compare the cited symbols
> with live code. Preserve unrelated uncommitted runner, identity, canvas, store,
> memory-editor, and styling work. Reconcile completed dependency changes; stop
> on unexplained conflicts before editing.

## Status

- Status: BLOCKED (live activation). Provider-neutral scoped reads, separate default-off permission, canonical chunk reads, native allowance/terminal/trace boundary, fake-runtime conformance, UI, and lookup evaluation are implemented and locally reviewed. A 2026-09-15 integration review fixed per-turn lookup serialization, duplicate-turn teardown, byte refunds for rejected lookup results, and legacy artifact indexing starvation, with regression tests. The full check passed on 2026-09-15 (`bun run check`: 548 frontend tests, frontend build, 1,072 Rust tests, 0 failures; clean `git diff --check`). Production adapter activation remains blocked: no release-enabled native provider supports the host-owned lookup callback with verified remaining-context accounting and an authorized live smoke.
- Priority: P2
- Effort: L, coarse estimate dependent on runtime feasibility
- Risk: MED
- Depends on: `plans/050-memory-corrections-and-validity.md` and
  `plans/051-backend-output-persistence.md`
- Category: direction
- Planned at: commit `85f0d64`, 2026-09-08, including the dirty working tree above

## Why this matters

Initial recall can omit useful evidence. Agents could recover it through small
read-only searches and selected-body reads, using the database Alfred already
has. The current native infrastructure does not establish an available production
path for those tools. Verify that path before advertising support or changing
workflow behavior.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result that follows supersedes them for current local status;
the bounded scope, permission, provider-gating, and no-fallback constraints
remain authoritative.

`src-tauri/src/agents/native/tools.rs:19-29` has no memory/history operation:

```rust
pub enum AlfredToolKind {
    FileRead, FileWrite, FileEdit, DirectoryList,
    Shell, Process, ApplyPatch, Mcp, Subagent,
}
```

`native/contract.rs:153-178` defines filesystem/shell/MCP/subagent permission
fields and filesystem/shell/patch/MCP/subagent capability fields. These do not
implicitly grant database reads. `src-tauri/src/lib.rs:224-229` constructs the
native router with `Arc::new(DenyAllToolExecutor)`.

`src-tauri/src/commands/managed_runtime.rs:844-865` can register verified OpenCode.
Its descriptor in `native/providers/opencode/runtime.rs:297-309` declares
`RuntimeExecutedWithHostApproval` and `supports_mcp=false`. Runtime-owned shell
tools do not establish an Alfred-owned memory callback. Gemini's declaration
table in `native/providers/gemini/protocol.rs:29-65` shows a concrete custom-tool
mapping pattern, but code existing in that directory is not evidence that Gemini
is enabled in the current release. Do not assume either route is available.

`src-tauri/src/db/history.rs:238-244` already exposes user-facing search:

```rust
pub fn search_history(&self, input: HistorySearchInput)
    -> Result<Vec<HistorySearchHit>, DbError>
{
    let fts_query = plain_text_fts_query(&input.query)
        .ok_or_else(|| DbError::Other("history search query must not be empty".into()))?;
    let limit = input.limit.unwrap_or(25).clamp(1, 50);
    // ...
}
```

Its optional workflow ID permits installation-wide search at `:267` and `:307`.
`get_run_history` at `:169-204` accepts a bare run ID and loads all steps.
These are appropriate existing user controls, not safe raw agent-tool contracts.
`memory_full_body` in `memories.rs:1149-1155` reads an entire artifact file before
returning it. An agent read must instead limit file I/O before allocation.

History FTS limits searchable JSON to 32 KiB at `history.rs:8`; query construction
uses at most 12 terms of 64 characters at `:9-10`. Memory artifact FTS indexes
the stored preview, not the full file, at `:441-454`. Do not promise full-document
artifact search without a separately verified index change.

Reuse `Db::with_conn`, `DbError`, serde camelCase DTOs, current memory scope and
validity rules, and native cancellation/event conventions. Use only synthetic
fixtures and `Db::open_in_memory()` in ordinary tests.

## Implementation result

Workflow storage and settings now carry a separate
`memory_lookup_enabled`/`memoryLookupEnabled` permission, defaulting off for
new and migrated workflows. Imported templates remain off, the UI explains
"Allow agents to look up memory", and every lookup call rechecks the host-owned
flag; Automatic recall's setting does not grant lookup access.

The frozen `LookupLimits v1` contract enforces a 4,096-byte request, 2,000-byte
query with at most 24 terms of 64 characters, 256-byte references, 512-byte
cursors, eight results, bounded metadata/excerpts, 6,000-byte search/get
responses, 4,000-byte body pages, eight calls and 16,000 returned bytes per
step, and a five-second per-call deadline. Search/get/history operations bind
to the actual run, node, workflow, and workspace with opaque references,
current-versus-historical validity, receipt-only step output, and no composed
prompt leakage.

Artifact pages use writer-owned verified manifests, per-chunk digests, a
trusted root, 4,096-byte chunks, and UTF-8 boundaries; missing, legacy, or
unverified content is reported unavailable/incomplete instead of reading and
truncating a whole file. Native integration has a distinct `MemoryLookup`
capability and permission, a trusted execution context, a runtime-specific
remaining-context allowance shared across all lookup traffic, fail-closed
reservation/release, terminal events for errors/cancellation, and staged
delivery traces. Durable lookup traces retain source, version, page, delivery,
and classification/evidence metadata after accepted serialization.

Fake-runtime and conformance coverage exercises declaration/dispatch/results,
revocation, budgets, context mismatch, cancellation, terminal statuses, and
trace/evidence release. The settings UI covers default-off, unavailable,
retry/rollback, and active-step disable states in both themes; lookup
evaluation uses fixed labels and separates policy/transport invariants from
model usefulness. Production still has no registered provider: the release
enabled-provider list and production registry are empty, and `lib.rs` installs
the lookup executor only with a `DenyAllToolExecutor` fallback. No fake fixture
or evaluator result enables a provider.

The isolated native Tauri startup smoke described in Plan 051 proves real
SQLite startup/recovery and artifact integrity, but does not prove a provider
custom-tool declaration/result path, lookup transport, or WKWebView interaction.

## Scope

- Create `src-tauri/src/db/agent_memory_lookup.rs`, registered/exported narrowly
  by `src-tauri/src/db/mod.rs`, plus focused tests.
- Modify `src-tauri/src/db/memories.rs`, `memory_retrieval.rs`, and `history.rs`
  only to share scoped query/bounded-read helpers. Extend `schema.sql` and
  `migrate.rs` for the default-off workflow read permission and, only if needed,
  additive tool-use trace data.
- Create `src-tauri/src/agents/native/memory_tools.rs`; modify native `tools.rs`,
  `contract.rs`, `context.rs`, `registry.rs`, `mod.rs`, and `conformance.rs` only
  for a distinct read capability, host-bound context, dispatch, and tests.
- Modify `src-tauri/src/runner/mod.rs` and `src-tauri/src/lib.rs` only for scoped
  service wiring and optional capability advertisement after feasibility passes.
- `src-tauri/src/db/workflows.rs`, existing workflow commands in
  `src-tauri/src/commands/mod.rs`, `src/features/workflow/types.ts`, `api.ts`,
  `store.ts`, and `components/memories-inspector/memory-automation-settings.tsx`
  for the separate opt-in read permission. Extend `tests/memory-retrieval.test.ts`.
- Stage 1 may inspect provider adapters and release gating read-only. Before
  editing an adapter, record its exact files in the feasibility report and this
  scope section. Restrict activation to one already-supported runtime route.
- Compatibility-only exhaustive matching for the distinct lookup kind may edit
  `src-tauri/src/agents/native/providers/gemini/protocol.rs` and exactly five
  full-literal fixture sites: two `NativeToolCapabilitySet` literals in
  `providers/claude/tests.rs`, one in `providers/gemini/tests.rs`, and the
  `NativePermissionProfile` plus `NativeToolCapabilitySet` literals in
  `providers/opencode/tests.rs`. These sites must keep `memory_lookup` false and
  must not add provider declarations, result mappings, registration, or support
  advertisement.
- Create `docs/agent-memory-lookup-feasibility.md`; extend the existing memory
  evaluation fixtures with separately versioned tool-read cases. Update this
  plan/index and native support documentation only for verified capability facts.
- Out of scope: memory writes/approvals, provider enablement, billing/account
  changes, a broad shell executor, CLI fallback, general MCP setup, private scope
  expansion, vector indexing, unbounded conversation export, UI redesign, and
  live calls in the normal test suite.

## Read contract (implemented locally; activation pending)

The contract below is frozen and implemented by the local scoped service and
fake-native boundary. It is not by itself a production provider capability;
release/runtime activation remains a separate gate.

| Tool | Allowed caller fields | Server behavior |
| --- | --- | --- |
| `memory.search` | Plain query, requested result count within a cap | Search eligible active memories visible to this run; return bounded excerpts/references |
| `memory.get` | Opaque memory reference, bounded page cursor | Recheck scope/version/validity and read a bounded body page |
| `history.search` | Plain query, optional bounded time filter and count | Search this workflow's retained run evidence and visible memory history, labeled as historical |
| `history.get` | Opaque run-step/evidence reference, bounded page cursor | Read one authorized evidence block/page, never all run steps by default |

The host derives run/node/workflow and captured workspace context from the actual
execution request and DB relationships. Tool arguments cannot override context,
supply a different workflow/run scope, or name database/artifact paths. Shared
service code validates the binding before every call; a bare working directory
or model-provided ID is not authority. Explicitly linked memory access follows
the existing link policy. Do not extend a link to unrelated run history.

Use Plan 050's validity/replacement rules on every search/get. A reference found
earlier does not override later deletion, expiry, retraction, replacement, or
unlinking. Ordinary memory search returns eligible current knowledge. History
can return retained inactive evidence with its status and dates plainly labeled.
It must not present an old claim as the current answer. Do not expose unrestricted
`search_history` or `get_run_history` as tools, or alter the user's global History UI.

Freeze a versioned `LookupLimits` contract. These ceilings are enforced by the
local service/native conformance path, not claimed as defaults of any provider:

| Field | Ceiling |
| --- | --- |
| Entire serialized request | 4,096 UTF-8 bytes |
| Query | 2,000 UTF-8 bytes, at most 24 terms of 64 Unicode characters |
| Time filter | optional RFC3339 from/to, at most 64 bytes each, from <= to |
| Opaque reference / cursor | 256 / 512 UTF-8 bytes |
| Results | eight per search |
| Title / provenance / excerpt per hit | 256 / 512 / 1,200 UTF-8 bytes |
| Search response including all metadata | 6,000 UTF-8 bytes |
| Get body page / entire get response | 4,000 / 6,000 UTF-8 bytes |
| Calls / returned bytes per agent step | eight / 16,000 UTF-8 bytes across all four tools |
| Read deadline | 5,000 ms per call, bounded by the remaining step deadline |

Reuse Plan 048's UTF-8 accounting helpers. The 16,000-byte incremental read
allowance is separate from its 12,000-byte initial memory prompt limit; enforce
the provider's remaining context capacity too and return budget exhaustion when
it cannot fit. Review the resulting 28,000-byte maximum source contribution in
evaluation before increasing any ceiling. Count all pages/calls in trusted
per-step state; a new provider message or cursor does not reset the allowance.

Enforce limits in trusted host/service code before allocation and after JSON
serialization, including metadata overhead. Clamp optional requests to documented
caps or reject invalid input consistently. Cursor validation must bind source,
version, offset, and scope. Use bounded file reads that preserve UTF-8 boundaries;
do not read the full artifact and truncate afterward. Return explicit truncated,
next-page, unavailable, deleted, and incomplete states. A missing artifact cannot
turn its preview into a supposedly complete body.

Record selected references, actual returned bytes, page/version/status, and tool
reason in the established run evidence/use trace. Tool results remain retrieved
data during later review and are not independent user confirmation. Do not leak
artifact paths or unrelated input/metadata fields through results or errors.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Read service | `cargo test --locked --manifest-path src-tauri/Cargo.toml agent_memory_lookup` | all new contract/scope/budget tests pass |
| Native contracts | `cargo test --locked --manifest-path src-tauri/Cargo.toml agents::native` | conformance and existing provider fixtures pass |
| Memory/history regressions | `cargo test --locked --manifest-path src-tauri/Cargo.toml memory` and `cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` | all pass |
| UI history regression | `bun test tests/history-search.test.ts tests/native-harness-release-matrix.test.ts` | all pass |
| Lookup opt-in | `bun test tests/memory-retrieval.test.ts` | toggles, rollback, and unsupported-state tests pass |
| Evaluation | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval -- --nocapture` | versioned results printed; hard invariants pass |
| Final integration | `bun run check` | exit 0 |
| Patch hygiene | `git diff --check` | exit 0 |

New filters must execute the intended tests. Fixture conformance is not live
provider verification, and no tool declaration is proof of available support.

## Steps

### 1. Establish feasibility and freeze the bounded contract

Inspect the current registry, release manifest/gates, actual runtime ownership,
provider declarations, callback/result transport, cancellation, and tool limits.
Write `docs/agent-memory-lookup-feasibility.md` with a matrix of inspected routes,
exact source evidence, available versus merely declared capabilities, and the
smallest verified custom-read path. Include OpenCode's runtime-owned constraint
and the deny-all Alfred executor described above. Do not infer readiness from
stale support docs or provider files alone.

Freeze request/result schemas and the specified numeric ceilings in the new
service module, with malformed/boundary fixtures. Reuse Plan 048 accounting;
measure serialized overhead rather than counting only bodies.
Record whether an already-supported adapter can advertise these tools without
enabling a blocked provider, installing a new broad executor, or changing accounts.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml agent_memory_lookup`
passes schema/limit fixture tests, and `git diff --check` passes. The report names
one viable route with evidence or explicitly states `adapter activation blocked`.

If no viable route exists, stop adapter activation and advertisement here. Steps
2 and 3 may still build/test core contracts through fixtures. Report that partial
state; Steps 4 and 5 cannot be completed and this plan cannot be marked DONE.

### 2. Implement scoped bounded reads over the existing database

Add the four read operations in `agent_memory_lookup.rs`. Derive context server-
side and use small internal helpers for existing indexed search. Distinguish
eligible current memory from historical evidence. Recheck source state/version
on get, restrict history to the bound workflow, and restrict memory links to their
actual targets. Keep user-facing global history methods unchanged.

Implement page reads with pre-I/O limits, validated cursors, explicit incomplete
states, and serialized result-size enforcement. Use receipt provenance from 051
without revealing full run input/metadata. Do not rewrite the artifact index;
document its existing searchable extent.

**Verify:** run the read-service and memory/history commands. Cases for forged
references, unrelated workflows, link removal, validity changes, unavailable
files, huge Unicode bodies, pagination, and exact byte boundaries pass.

### 3. Integrate the host boundary using a fake runtime

Add a distinct read capability and permission/availability contract in native
types. Dispatch only these four operations to a host-owned scoped service. Keep
all unrelated Alfred tools denied where they were denied before. Do not replace
the deny-all executor with a shell/filesystem implementation.

Implement the separate workflow opt-in and UI contract specified under
Compatibility and migration. Bind all four operations to that flag; neither
Automatic recall nor a provider's general filesystem permission grants reads.
Test both advertisement and dispatch with the flag disabled, including after
revocation while a step is running. A stale declaration cannot authorize a call.

Bind each call to the actual run/node host, share per-turn accounting across
search/get/history calls, and reuse cancellation/deadline and terminal event
conventions. Extend trace storage additively only if needed after dependency
reconciliation. Ensure tool evidence is classified for later memory curation.

**Verify:** run read-service and native-contract commands. Fake runtime tests
prove the full declaration/dispatch/result shape, cancellation, aggregate budget
exhaustion, denied unrelated tool kinds, and no cross-run scope reuse. The existing
release-matrix test still passes with unavailable providers unavailable.

### 4. Verify and activate one supported adapter

Proceed only with Stage 1's viable route. Update the exact adapter files added
to scope after inspecting their live transport contract. Add native declaration
mapping and result handling for the four operations, with strict unsupported-
operation errors. Advertise only when runtime registration, release eligibility,
provider custom-tool support, host binding, and read service are all available.

Test the provider protocol with fixture transport first. Then record one bounded
real-runtime acceptance run using synthetic memories and an already configured,
authorized test account, including provider/runtime/model versions and costs.
If no authorized test account exists, report the gate unavailable; do not scrape
credentials, switch billing source, or silently use a CLI. Normal CI remains
offline. Missing gates leave advertisement off and completion pending.

**Verify:** run the native-contract command and the selected adapter's existing
fixture-test command recorded in the feasibility report. Then execute its exact
bounded smoke command recorded there and attach sanitized evidence proving a
provider-requested search/get reached the scoped service and returned successfully.
No placeholder smoke command or fake-only evidence satisfies this gate.

### 5. Evaluate usefulness and verify integration

Add versioned cases requiring recovery after initial recall misses, report
history access, correction-aware reads, and abstention when no evidence exists.
Compare initial recall alone against optional tool reads with fixed fixtures.
Report necessary-memory recovery, wrong-memory use, bytes, calls, and measured
latency. Distinguish deterministic policy tests from real model success/cost.

Keep support documentation honest about the enabled provider/harness and known
search extent. Do not imply every agent can now use tools. Finish release checks
only after real adapter evidence exists.

**Verify:** run evaluation, UI history regression, `bun run check`, and
`git diff --check`. Existing invariants and unrelated native gates remain intact;
the feasibility report contains the exact adapter activation evidence.
Verify the opt-in control with synthetic IPC data in both themes: default off,
unavailable runtime, successful enable, failed-save rollback, and disable during
an active step. Follow the existing design system and applicable UI tool skill.

## Test plan

- Schema tests cover every field's valid maximum, oversized value, empty value,
  invalid cursor/time bound, and serialized result ceiling including metadata.
- Scope tests use two workflows, two workspaces, user memories, and explicit
  links. Forged source IDs never widen reads; links never grant full run access.
- Mutation-between-calls tests cover unlink, expiry, replacement, retraction,
  deletion, and receipt tombstones. Responses reflect current validity.
- I/O tests use large temporary UTF-8 artifacts, missing files, incomplete pages,
  and matching 2,000-byte previews; memory use never scales with full file size.
- Host tests cover multiple tools sharing aggregate limits, cancellation, timeout,
  terminal error events, wrong run/node binding, and unsupported capabilities.
- Adapter tests cover declaration/result round trip and no advertisement when
  disabled/unavailable. No CLI fallback, broad tool grant, or provider enabling.
- History user API/UX regression tests retain existing installation-wide search.
- Evaluation retains fixed reference labels and explicitly separates fake model
  fixtures from observed provider behavior.

## Compatibility and migration

Add a separate workflow `memory_lookup_enabled` / `memoryLookupEnabled` flag,
default false for both existing and new workflows. In the current automation
controls, label it "Allow agents to look up memory" and show verified runtime
availability. Existing Automatic recall governs initial injection only; the new
flag governs all four tool reads. Pins and explicit selections retain their
existing behavior. A disabled flag prevents both advertisement and dispatch.

Bind the native read permission to this host-owned flag and current runtime
capability; caller-provided tool arguments cannot grant it. Keep it off on
imported templates and require local selection to enable it. If Plan 042 has
landed, reconcile its import allowlist in the implementation scope before editing
that narrowly necessary integration. Persist/rollback the toggle using the
existing workflow update pattern and add backend enforcement tests. No provider
calls or lookups occur merely from rendering these controls.

Version native contracts consistently and update exhaustive kind/capability
matches and fixtures without changing other tool grants. Old runtimes lacking
the read capability get no declaration; unsupported invocations fail explicitly.
Any trace migration is additive and idempotent, preserving ids, bodies, artifacts,
links, receipts, and history on fresh and existing databases. Never repair tests
by removing legacy records or widening scopes.

## Done criteria

- [x] Feasibility report distinguishes inspected, declared, gated, and verified
  support.
- [x] Four scoped read-only operations enforce all field/per-call/per-turn
  ceilings.
- [x] Bounded artifact I/O and explicit incomplete/invalidated source states
  pass tests.
- [x] Tool reads use current validity and provenance, with trace/evidence
  classification.
- [ ] One already-supported real runtime passes the recorded end-to-end acceptance gate.
- [x] Advertisement reflects actual capability; unavailable providers remain
  unavailable.
- [x] No raw global history API, unrestricted body read, broad executor, or CLI
  fallback exists.
- [x] Evaluation, focused tests, `bun run check`, and hygiene pass; docs/index reflect reality.

The local fake-runtime, conformance, evaluator, and full-check results do not
satisfy the real-runtime criterion or enable a provider, so this plan cannot be
marked DONE until an authorized live runtime passes.

## Specific stop conditions

Stop activation if the only route needs enabling a blocked native provider,
introducing a broad shell executor, scraping credentials/profile files, changing
provider/account/billing implicitly, or using CLI fallback. Continue only the
independent fixture/service work explicitly allowed above and report partial status.

Stop and report if the existing permission controls cannot express read access,
context cannot be bound to a real run, provider framing cannot enforce bounded
results, or completion would require unrestricted history/artifact access. An
absent authorized runtime test is an unpassed gate, not inferred approval.
After two failed verification attempts, record the command and cause; do not
weaken scope/budget assertions or broaden capabilities to get a pass.

Use an isolated branch/worktree when overlap requires it; suggested branch
`advisor/052-agent-memory-lookup-tools`. Preserve unrelated working-tree changes.
Do not push/publish without authorization. Never inspect the live user database
or reproduce secret values in tests, plans, reports, or diagnostics.

## Maintenance

Recheck adapter support when runtime/protocol versions change. A release gate
must never advertise capability solely because an enum/table lists it. Update
the shared budget and read-trace tests when tools or memory sources are added.
Keep provenance and historical/current status explicit as correction rules evolve.

Plan 042 excludes tool results and local history from shared templates. Plan 043
adds render-only schedule nodes, not new execution context. Plan 044's analyzer
remains text-only and receives no memory tools. Coordinate shared schema/runner/
native files without implementing any of those plans' additional features.
