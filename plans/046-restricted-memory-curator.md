# Plan 046: Run memory review without tools or workflow permissions

> Implementation checkpoint. The restricted contract and fake-backed
> conformance work below are implemented; live activation remains a separate
> evidence gate. Run all commands from `desktop/` and update the index only
> when the applicable done criteria pass.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/agents src-tauri/src/runner/mod.rs src-tauri/src/db src-tauri/src/commands/mod.rs src/features/settings/memory-review.ts src/features/workflow tests/memory-curation.test.ts docs/memory-review-provider-support.md specs.md docs/install.md`,
> and `git diff HEAD -- src-tauri/src/agents src-tauri/src/runner/mod.rs src-tauri/src/db src-tauri/src/commands/mod.rs src/features/settings/memory-review.ts src/features/workflow tests/memory-curation.test.ts docs/memory-review-provider-support.md specs.md docs/install.md`. Compare the live symbols
> with the excerpts before editing. This baseline includes unrelated dirty
> runner, identity, canvas, memory-editor, store, and styling work. Preserve it.
> Reconcile completed dependency changes; report unexplained conflicting changes.

## Status

- Status: BLOCKED (live activation). Contract/conformance and scoped UI review are accepted, and the shared final check passed on 2026-09-15 (`bun run check`: 548 frontend tests, frontend build, 1,072 Rust tests, 0 failures; clean `git diff --check`). Live activation remains blocked: Claude 2.1.271 retains administrator-managed executable hooks under safe mode, and exact runtime restriction proof, managed-policy assurance, and an authorized adversarial live check are unavailable.
- Priority: P1
- Effort: L
- Risk: MED
- Depends on: completed memory foundation 025–028; independent of Plan 045
- Category: security
- Planned at: commit `85f0d64`, 2026-09-08, including the existing dirty tree

Track two milestones separately: contract/conformance and live activation.
Steps 1–4 plus fake-backed integration establish contract/conformance and unblock
Plan 049. Live activation additionally requires a verified provider invocation
and the authorized synthetic check in Step 5. If activation is blocked, record
both milestone states; dependent source/evidence work may proceed through the
injected restricted reviewer while automated production review stays unavailable.

## Implementation result

The dedicated restricted-review boundary is implemented with a typed request and
result, exact capability identity, bounded input/response/queue limits, and a
fake transport that rejects tool use without invoking an ordinary coding
adapter. Background jobs use the restricted identity, bounded queue/recovery,
stable unavailable errors, legacy-job migration, and release of the concurrency
permit on every completion path. The existing strict candidate parser and
approval flow remain the only path from review output to a proposal; ordinary
workflow permissions, authentication, and billing are unchanged. Scoped UI
states for disabled, unavailable, failed, and retry paths were accepted with
synthetic data.

This is contract/conformance completion, not live provider support. The inspected
machine has Claude Code `2.1.271`, while the exact allowlisted fixture is
`2.1.263`; managed-policy semantics and the required restricted execution
assurance cannot be proved from the installed route (for example,
`--safe-mode` still permits managed executable hooks). An already-authorized
account and an explicit synthetic adversarial live check are also required.
Until those gates are supplied, production review stays unavailable, no
provider call is used by ordinary verification, and this plan remains
`BLOCKED (live activation)`.

## Why this matters

The background reviewer proposes memory changes but currently runs through an
ordinary coding adapter. Approving the resulting suggestion does not prevent
actions taken while producing it. Give review a separate execution contract that
can return suggestions and cannot execute tools. Keep ordinary workflow agent
permissions and the selected provider's authentication and billing unchanged.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result above supersedes them for current local status; the
restricted execution contract and provider gates remain authoritative.

`src-tauri/src/runner/mod.rs:2167`, in `AdapterReviewAgent::run_review`, calls:

```rust
adapter_for(provider).run(request, AgentRunHooks {
    control: None,
    on_activity: None,
})
```

`run_memory_review_job` sets `working_directory` from the workflow at line 2356.
`src-tauri/src/agents/claude_code.rs:45` passes `--permission-mode
bypassPermissions` and permits 40 turns. Its JSON fallback uses the same policy.
`src-tauri/src/agents/codex.rs:35` tries ordinary coding invocations including
`--full-auto`. These adapters are correct places to preserve existing workflow
behavior; do not weaken their functionality to restrict the reviewer.

The native router is not a drop-in restriction. In
`src-tauri/src/agents/native/registry.rs:581`, `native_turn_tool_policy` replaces
caller policy with `Ask` and supported tools for `RuntimeExecutedWithHostApproval`.
Native Claude registration is blocked in `providers/claude/mod.rs:81`; native
Codex registration is blocked in `providers/codex/sdk_runtime.rs:157`. Those
registration, package, account, and commercial gates remain authoritative.

`src-tauri/src/db/memory_curation.rs` provides strict JSON parsing, a 32 KiB run
digest, five candidates, and transactional approval. Review settings default to
off globally and per workflow. Existing runner tests use `FakeAgent` through
`ReviewAgent`, with no real provider. Match `DbError`, camelCase serde DTOs, and
the existing stable error-code pattern. Never persist raw provider diagnostics.

`src-tauri/src/agents/process.rs:242` configures an ordinary agent environment.
It has bounded capture, but `reader.lines()` still reads a whole line before
capture. Do not assume this helper supplies review isolation or strict streaming
allocation limits merely because it has a timeout and bounded final output.

## Scope

Only modify these implementation paths:

- Create `src-tauri/src/agents/review.rs` and
  `src-tauri/src/agents/review/claude_cli.rs`; register the module in
  `src-tauri/src/agents/mod.rs`.
- `src-tauri/src/runner/mod.rs`, only background memory review and its tests.
- `src-tauri/src/db/memory_curation.rs`, `src-tauri/src/db/schema.sql`, and
  `src-tauri/src/db/migrate.rs`, only review identity, capability status, and limits.
- `src-tauri/src/commands/mod.rs`, existing review settings/commands only.
- `src/features/settings/memory-review.ts`, `src/features/workflow/api.ts`,
  `src/features/workflow/types.ts`, and
  `src/features/workflow/components/memories-inspector/memory-automation-settings.tsx`.
- `tests/memory-curation.test.ts`; create `docs/memory-review-provider-support.md`.
- This plan, its index row, and memory-review wording in `specs.md` and `docs/install.md`.

Read existing native contracts and transports for guidance; do not enable blocked
native runtimes or change their product decisions. Do not edit ordinary provider
adapter flags, general CLI permissions, account stores, provider billing,
retrieval, canonical approval policy, or the user's settings. No new package,
embedding service, or extra model call belongs in this plan.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Baseline | `cargo test --locked --manifest-path src-tauri/Cargo.toml memory` | all pass |
| New review boundary | `cargo test --locked --manifest-path src-tauri/Cargo.toml agents::review` | all pass, at least one matching test |
| Background lifecycle | `cargo test --locked --manifest-path src-tauri/Cargo.toml runner::tests::background_memory_review` | all pass |
| Review settings | `bun test tests/memory-curation.test.ts` | all pass |
| Full integration | `bun run check` | exit 0 |
| Patch hygiene | `git diff --check` | exit 0 |

New test filters below name modules to create. A command that runs zero tests
does not satisfy its gate. No normal verification command may contact a provider.

## Steps

### 1. Define a review-only request and capability result

In `agents/review.rs`, add a typed `RestrictedReviewRequest` with bounded prompt
data, explicit execution identity, model, timeout, cancellation, and limits.
Do not accept `AgentRequest.extra`, workspace roots, skills, session resume IDs,
arbitrary tool policy, or a caller-provided command. The result contains bounded
response text and safe usage metadata. The existing strict parser remains the
only way to turn that response into candidates.

Represent review availability as supported or unavailable with a stable reason,
keyed by product, CLI/runtime version, execution mode, and restriction contract
version. Persist the selected provider/product/mode/model and the capability
version in the job when it is enqueued. Ordinary CLI-owned authentication remains
CLI-owned. Never put credentials or discovered account files in the request.

Set initial hard limits to 64 KiB total input including instructions, 32 KiB
response text, 120 seconds including launch, one active review process, and a
ready queue of at most 32 reviews in memory. Other jobs remain durably pending
in SQLite and enter that queue when space opens. Reuse the existing
five-candidate/body bounds. Check limits before allocation and stop streams on
overflow, including a single unterminated line.
Keep these limits configurable in code, not untyped workflow JSON.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml agents::review::contract`
passes supported/unavailable identity, byte-boundary, cancellation, and no-tool
contract tests without spawning a provider.

### 2. Verify one provider path before implementing live activation

The first candidate is a dedicated Claude Code CLI invocation because the
existing reviewer settings use CLI-owned authentication. This is a candidate for
verification, not a claim that the installed version supports the required
restriction. In `docs/memory-review-provider-support.md`, record official primary
documentation URLs, check date, exact verified version/range, test evidence, and
the restriction mechanism for each row. Do not guess flags from memory.

Verify all of the following before setting a supported row: no model tools;
no MCP, plugins, hooks, skills, automatic project/user instructions, resumed
sessions, or user-defined command configuration; ordinary selected CLI billing
and authentication; no prompt in the process argument list; bounded process
output; and cancellation of the whole review process tree. Authentication reads
needed by that CLI are permitted; model access to those files is not.

Read the candidate's installed `--version`/`--help` only after confirming they
are read-only. Use official provider documentation to establish semantics.
If a required capability cannot be proved, record its stable blocker and leave
the implementation unsupported. Continue the fake-backed boundary and runner
work. Do not fall back to an API account, another provider, the ordinary adapter,
or relaxed settings. Unknown future versions remain unsupported until checked.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml agents::review::capability`
passes known-supported fixtures, unknown-version denial, changed-contract denial,
and unavailable-provider tests. `rg -n 'https://|version|blocked|verified' docs/memory-review-provider-support.md`
shows the evidence fields and an honest activation status.

### 3. Implement the dedicated restricted invocation

Implement only the verified invocation in `agents/review/claude_cli.rs`. Use an
isolated execution directory with no project material and an explicit environment
policy that permits the CLI's own authentication but excludes user/project
extensions. Directory isolation alone is insufficient. Put input on stdin and
use fixed argv constructed by code. No generic coding-adapter fallback is allowed.

Use a transport seam with synthetic process fixtures so tests can inspect argv,
environment policy, stdin, streams, timeout, and cleanup. Reject any observed tool
request before dispatch and terminate the review. There is no tool dispatcher,
approval broker, tool-result loop, or automatic repair/retry inside this contract.
Release the concurrency permit on every completion and error path.

If Step 2 has no supported row, implement the unavailable production branch and
the full fake transport contract now. Do not construct a partially restricted
real invocation. Core tests can pass while activation remains blocked.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml agents::review`
passes zero-tool-dispatch, unknown-version, no-fallback, stdin, oversized-line,
timeout, cancellation, and child-cleanup fixtures. Tests assert zero ordinary
adapter calls on every reviewer failure path.

### 4. Route background jobs through the restricted boundary

Replace `AdapterReviewAgent` with a production implementation of the new
contract. Preserve fake injection, one atomic claim per run, lock release before
provider execution, final candidate transaction, stable errors, and notification
after commit. Keep review failure independent from the completed run's status.

Check capability both before enqueue and immediately before launch. If capability
changes after enqueue, record a stable unavailable code and make zero provider
calls. Keep disabled/unconfigured paths at zero jobs/calls. Bounded queue overflow
gets an explicit skipped/retryable status rather than an unbounded task spawn.
Retry preserves the job's selected identity; changing providers requires an
explicit new selection, never an implicit retry fallback.

Migrate old jobs with no verified restriction identity as unavailable legacy jobs.
Preserve their metadata and candidates, but never execute a legacy pending job
through the old adapter. Add columns through `migrate.rs` as well as `schema.sql`;
do not reset saved global or workflow preferences during upgrade.

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml memory`
passes duplicate-claim, retry identity, disabled settings, legacy-job upgrade,
queue saturation, capability change, and zero-canonical-write tests.

### 5. Show availability and validate activation honestly

Update review settings DTOs and existing automation controls to show which
selected provider is available for restricted review and why another is blocked.
Replace the existing claim that the CLI receives "nothing else" with an accurate
description of the bounded review input. Preserve opt-in, cost acknowledgement,
candidate-only approval, and existing state rollback on IPC error.

Run synthetic UI verification for off, supported, unsupported-version, failed,
and retry states. Reuse semantic tokens and current controls from
`docs/design-system.md`. Use the applicable interaction skill and an isolated
synthetic IPC setup; macOS Tauri is WKWebView, not necessarily Chromium.

A live activation check is separate from CI. With an explicitly selected,
already-authorized provider/account and bounded synthetic input, verify one
successful structured response and adversarial instruction fixtures that produce
no tools or side effects. Record version, restriction configuration, safe result,
latency, and usage in the support document without raw private transcripts.
If credentials, provider access, or capability proof are unavailable, record the
activation blocker; do not label the plan DONE or restricted review live-ready.

**Verify:** `bun test tests/memory-curation.test.ts`, then `bun run check` and
`git diff --check`, all exit 0. The support document identifies either a verified
first live path or the precise activation blocker.

## Test plan

Use existing `background_memory_review` fake-agent tests as lifecycle examples.
Add named `agents::review::{contract,capability}` tests and transport tests in the
new module. Cover malformed input, zero candidates, malformed response, tool
events, same-run races, process limits, auth failures, stable error redaction,
capability drift, migration twice, and ordinary workflow behavior unchanged.
No test supplies private memory or reads real account credentials.

## Done criteria

- [x] Production review cannot reach ordinary `adapter_for(...).run(...)`.
- [x] Contract/conformance fixtures use a verified capability identity and strict
  limits.
- [x] Unsupported providers/versions fail closed with no provider fallback.
- [x] Normal workflow adapter permissions and billing are unchanged.
- [x] Off-by-default, per-workflow opt-in, and candidate approval are preserved.
- [x] Fresh and legacy databases pass migration and identity tests.
- [ ] One first provider path has documented capability proof and an authorized
  synthetic live check. Record contract/conformance completion separately while
  live activation remains BLOCKED when this evidence is unavailable.
- [x] Focused tests, full check, patch hygiene, and isolated UI verification pass.
- [x] Only scoped files changed; plan/index status reports the actual result.

## STOP conditions

Stop the dependent activation work and report if the provider cannot disable
tools/extensions, the installed version is unverified, or isolation depends only
on prompt instructions. Continue independent fake-backed work while recording
that blocker. Never activate a blocked native runtime or substitute billing.

Stop and report unexplained source drift, a required out-of-scope dependency, or
the same failed verification twice after a reasonable fix. Record pre-existing
full-check failures with their command and cause; do not call them passes.
Missing live credentials are an activation blocker, not a reason to inspect
private credential files or create another account implicitly.

## Execution and maintenance

Use an isolated branch/worktree if another executor owns overlapping files.
Suggested branch: `advisor/046-restricted-memory-curator`. Never reset, stash,
overwrite, or commit unrelated dirty work. Commit logical units with descriptive
messages; do not push or publish without authorization.

Reverify restriction semantics whenever the supported CLI version, configuration
loader, authentication product, or process launcher changes. Review the provider
support table as executable release policy. Keep Plan 049's structured evidence
inside these limits. Do not add a second adjudication model call as a convenience.
Memory remains reference data and cannot grant tool authority or override the
current user request.
