# Plan 048: Bound every memory contribution to an agent prompt

> Implementation checkpoint. Structured prompt origins, the shared memory-byte
> allocator, bounded reads, and final contribution traces below are implemented
> and locally reviewed; retain the shared gate until it passes. Run all commands
> from `desktop/`.
>
> **Drift check:** run `git status --short`, `git diff --stat 85f0d64..HEAD -- src-tauri/src/db src-tauri/src/runner src/features/workflow tests/memory-retrieval.test.ts tests/memory-node-selection.test.ts tests/fixtures/memory-quality specs.md docs/install.md`,
> and `git diff HEAD -- src-tauri/src/db src-tauri/src/runner src/features/workflow tests/memory-retrieval.test.ts tests/memory-node-selection.test.ts tests/fixtures/memory-quality specs.md docs/install.md`. Read the live symbols cited
> here. The baseline already contains unrelated uncommitted runner, identity,
> canvas, memory-editor, store, and styling work. Preserve it. Reconcile changes
> made by completed dependencies; report unexplained conflicting changes before
> editing. A clean checkout of the baseline commit omits some current UI files.

## Status

- Status: DONE — high review and scoped browser QA accepted; final composition report frozen; verified 2026-09-15 by `bun run check` (548 frontend tests, frontend build, 1,072 Rust tests, 0 failures) and clean `git diff --check`
- Priority: P1
- Effort: L
- Risk: HIGH
- Depends on: [047](047-task-aware-memory-recall.md), including its query-source and ranking-version contracts
- Category: correctness / prompt composition
- Planned at: commit `85f0d64`, 2026-09-08, dirty working tree

## Why this matters

Pins and automatic recall have separate bounds, but explicit Memory nodes load
full bodies and append them to the base prompt. Templates can repeat those
bodies. Carry memory references until the agent prompt is rendered, then apply
one deterministic memory budget and record what actually reached that agent.
This protects useful task context without imposing arbitrary limits on user instructions.

## Historical planning baseline
The observations below are retained from the pre-implementation plan. The
implementation result that follows supersedes them for current local status;
the original constraints and compatibility requirements still apply.

`src-tauri/src/db/memories.rs:1274` formats explicit selections by id:

```rust
parts.push(format!(
    "### Memory — {}\n{}\n",
    memory.title,
    self.memory_full_body(&memory).trim()
));
```

`memory_full_body` uses `fs::read_to_string`; a separate bounded-read pattern
already exists in `bounded_memory_body` for pins. Automatic rendering also
loads full bodies before truncation in `memory_retrieval.rs`.
`runner/mod.rs` appends explicit memory to `context_prompt`, accumulates
exclusion ids, and later prepends pin/recall blocks. Its template branch can
replace or duplicate context through `{{context}}` and `{{output}}`.

Trace reasons in `schema.sql` currently allow `lexical`, `recent`, `pinned`
with one row per run/node/memory. Plan 047 adds score-version metadata.
Current non-memory prompt order, identities, HTML output requirements, and
utility-node semantics must survive this change.

## Implementation result

The runner now carries structured prompt parts with authored, generated,
observation, and memory origins through template composition. Explicit
selections, pins, and automatic candidates are allocated once under the
12,000-byte UTF-8 memory budget with explicit > pinned > lexical precedence;
template duplication deduplicates by identity without parsing user text. Utility
payloads retain their complete authored values, while agent-facing memory
contributions use bounded projections.

Explicit and automatic prompt paths use bounded artifact reads with UTF-8-safe
excerpts and explicit unavailable/incomplete accounting. Final inclusion,
omission counts, actual bytes, and versioned reasons are recorded in the
context summary/trace without storing prompt bodies or query text. The frozen
composition evaluation covers 60 synthetic cases, reports zero hard violations,
and measures at most 10,967 memory bytes. High review and scoped UI checks are
accepted; the final 2026-09-15 `bun run check` passed 548 frontend tests, the
frontend build, and 1,072 Rust tests with 0 failures, superseding the earlier
1,068-pass/3-fail checkpoint; no assertion was weakened.

## Scope

- New `src-tauri/src/runner/memory_context.rs`; `runner/mod.rs` prompt and
  template composition seams; Plan 047's `runner/retrieval_query.rs` only for
  shared origin integration.
- `src-tauri/src/db/memories.rs`, `memory_retrieval.rs`, `history.rs`,
  `schema.sql`, `migrate.rs`, and `db/mod.rs`.
- `src/features/workflow/types.ts`, History memory-use models/components,
  `components/memories-inspector/memory-automation-settings.tsx`, and existing
  Memory-node settings components only for limits/omission explanation.
- `tests/memory-retrieval.test.ts`, `tests/memory-node-selection.test.ts`,
  Plan 045 evaluation fixtures/module, `specs.md`, `docs/install.md`, plan/index.
- Out of scope: global model token-limit management, changing node routing,
  limiting arbitrary authored prompts, provider wrappers, memory writes,
  new agent tools, template-language expansion, and artifact storage redesign.

## Commands

| Purpose | Command | Expected |
| --- | --- | --- |
| Memory storage/rendering | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memor` | all pass |
| Runner/template regression | `cargo test --locked --manifest-path src-tauri/Cargo.toml runner` | all pass |
| Migration/history | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::migrate` and `cargo test --locked --manifest-path src-tauri/Cargo.toml db::history` | all pass |
| Prompt evaluation | `cargo test --locked --manifest-path src-tauri/Cargo.toml db::memory_eval -- --nocapture` | all budget/isolation gates pass |
| Frontend | `bun test tests/memory-retrieval.test.ts tests/memory-node-selection.test.ts` | all pass |
| Full gate | `bun run check` and `git diff --check` | both exit 0 |

## Steps

### 1. Carry origin-aware context until agent dispatch

Introduce an explicit representation such as:

```rust
enum PromptPart {
    Text { text: String, origin: TextOrigin },
    Memory { id: String, source: MemoryContributionSource },
}
struct PromptContext { parts: Vec<PromptPart> }
```

Preserve authored task, generated output, external observation, and supplied
memory as distinguishable origins. Store references for explicit selections
instead of inserting full bodies immediately. The representation must cover
both current context and last output when either carries direct memory content.

Implement the existing template substitutions over parts, preserving literal
text and append/replace behavior. A literal string resembling an internal
reference remains literal; do not use string delimiters that user content can
forge. Replacing context removes references that are no longer present.
Repeating context copies parts, which the final memory allocator deduplicates.
Arbitrary agent-generated text remains generated output even if it happens to
quote a memory; do not infer provenance by text similarity.

Keep the standalone string `apply_template` behavior for unrelated utility
operations. Use distinct projections: bounded agent context, and the complete
explicit value required by an authored utility template. Never silently truncate
a file-write, script, HTTP, or notification payload to satisfy an agent budget.
Full-value utility reads remain scope/lifecycle checked and explicitly outside
this prompt budget. If a direct memory value returns to agent context, retain
its typed origin and apply the agent budget there. Actual tool results remain
observations rather than being relabeled as independent user statements.
Utility nodes receive no automatic recall.
Use Plan 047's eligible origins for retrieval; never reconstruct its query from
rendered memory. This origin structure is the prerequisite for Plan 049.

**Verify:** runner tests cover template append/replace, repeated context/output,
literal lookalikes, script/file/notification template compatibility, identity
placement, HTML instructions, and both agent paths.

### 2. Apply one budget with defined precedence

At final agent composition, collect live explicit references, visible pins,
and relevant automatic candidates. Recheck scope, lifecycle, and expiry before
reading content. A selected id alone does not bypass workflow visibility/link
policy. Use these fixed byte limits, including rendered metadata:

| Contribution | Maximum |
| --- | --- |
| Total memory in one agent prompt | 12,000 UTF-8 bytes |
| Explicit memory | 6,000 bytes; 1,500 bytes per item |
| Pinned memory | existing 6,000 total / 1,500 per item and scope allocations |
| Automatic recall | existing eight items / 6,000 total / 1,200 per item |

Allocate explicit selections first in authored order, then pins in existing
scope order, then automatic recall by relevance using the remaining total.
An id occurs once, with reason precedence explicit > pinned > lexical.
Allocation priority does not reorder surrounding authored text or the existing
trust-block/instruction order. Omit repeated reference occurrences with a small
count-only marker where needed for clarity.

Reserve room for aggregate notices before allocating bodies. Never emit a
per-omission marker once its bytes would exceed the total; trace the remaining
counts outside the prompt. An empty memory selection may render no block at all.

Count headings, trust labels, separators, and truncation/omission notices.
Return included ids, origin/reason, actual bytes, and omitted counts by cause.
Only ids that survive current context participate in exclusions. An explicit
reference removed by template replacement may be recalled if relevant again.
A retained selection omitted for size does not get silently reintroduced under
a different source to evade priority/size policy.

Bytes are a deterministic product limit, not model tokens. Do not truncate
the whole assembled prompt to implement this policy. Larger task text and
provider-added instructions are outside the memory-byte count.

**Verify:** runner and evaluation commands cover 0/1/many Memory nodes, repeated
ids, explicit+pin overlap, both template duplication paths, removal then recall,
many small items, oversized titles, and exact Unicode byte boundaries.

### 3. Bound artifact reads before allocation

Generalize the existing pinned bounded reader to support explicit and automatic
excerpts. Read only the allowed excerpt bytes plus a bounded lookahead for
truncation and UTF-8 completion. Reject excluded/oversized metadata before body
I/O. Preserve full-body access for the library's explicit full-content action;
it must not be called from prompt paths.

Keep complete short claims where they fit. Label shortened reports as excerpts
with an id and a truncation marker. Missing/unreadable files may provide their
stored preview only when explicitly labeled incomplete. Do not suggest that an
agent can expand a reference until Plan 052 is actually available.

**Verify:** memory tests use an injected reader or bounded-read instrumentation
to show that a huge synthetic artifact is never loaded in full, and cover
missing files, UTF-8 cuts, fallback previews, and intact original artifacts.

### 4. Trace the final memory contribution

Migrate trace reasons to allow `explicit` with version `explicit_v1`, retaining
historical lexical/recent=`lexical_v1`, pinned=`pinned_v1`, new lexical=`lexical_v2`,
ids, and uniqueness. Preserve these values when rebuilding SQLite CHECK
constraints. Add a per-run/node context summary for
budget, actual memory bytes, omission counts, and trace availability; no memory
bodies or query text enter that summary. Keep schema startup ordering safe for
upgrades.

Record only final included memories after formatting. When trace persistence
fails, preserve existing best-effort pinned behavior and apply the same
best-effort rule to explicit selections; both remain bounded. Automatic recall
still drops if it cannot be audited. Recompute final byte counts after removing
automatic material and emit only a stable diagnostic. Do not fail the whole
agent step for an optional trace write.

**Verify:** migration/history/runner tests pass historical rows, explicit
reason display, duplicate origins, actual rendered byte accounting, database
failure, and deletion/cascade behavior.

### 5. Explain omissions in the existing UI and complete integration

History shows explicit use and count-based reasons for omissions. The Memory
node picker states that selections are subject to context limits. Its selection
state remains editable even when an item is unavailable or omitted. Avoid new
sliders or model-specific token settings.

Update the fixture report and documentation from separate unbounded explicit
behavior to the shared memory budget. Verify large selections, empty selections,
an omitted selection, a missing artifact, and normal selections in synthetic
UI QA.

**Verify:** frontend, evaluation, full integration, and hygiene commands pass.

## Done criteria

- [x] Final agent memory contribution never exceeds 12,000 UTF-8 bytes.
- [x] Every direct memory source and template-copy path participates in that
  limit.
- [x] Authored text, utility templates, identity order, and HTML output remain
  correct.
- [x] Removed references no longer create stale automatic exclusions.
- [x] Prompt excerpts use bounded artifact I/O and declare incompleteness.
- [x] Explicit use, omissions, versions, and actual bytes appear in traces.
- [x] Migration, frontend, runner, evaluation, and full gates pass; index is updated.

## Specific stop conditions

If preserving existing template behavior would require parsing generated text to
guess memory provenance, redesign the structured representation before proceeding.
If a selected memory is outside the workflow's visibility, return an unavailable
selection state rather than broadening scope. Do not treat this plan's byte limit
as proof that every provider context window will fit.


## Execution boundaries

A filtered test command must run its named cases; zero matching tests is not a
passing gate. Add the specified tests before using their new filters.

Use an isolated branch or worktree if another executor owns overlapping files.
Never reset, stash, overwrite, or commit the user's unrelated changes. Branch
suggestion: `advisor/048-shared-memory-context-budget`.
Do not push, publish, or open a PR unless authorized. Only mark this plan DONE
after its focused gates and `bun run check` pass. A pre-existing full-check
failure must be recorded with its exact command and cause; it is not a pass.

All schema changes must work on both fresh and existing databases, preserve
canonical ids, bodies, artifacts, links, and history, and be idempotent. Follow
`Db::with_conn`, `DbError`, serde camelCase DTOs, and transactional index
maintenance in `src-tauri/src/db/memories.rs`. Never edit a user's live database,
drop canonical rows to repair a migration, or call a provider in ordinary tests.

UI work must reuse the existing Memories/History views, state rollback patterns,
and shared controls. `docs/design-system.md` requires semantic tokens and stable
layout across loading/error states. Verify changed UI with isolated synthetic
IPC data and the available desktop/browser tools. Native macOS Tauri uses
WKWebView; do not assume it is a Chromium target. Read the applicable interaction
skill before UI automation and record the tested states.

Stop and report if the change would widen memory scope, require secret/profile
scraping, change the selected provider or billing source implicitly, or require
an unexplained out-of-scope dependency. Routine conflicts resolved by the stated
contracts do not require another product decision.

## Maintenance

Keep the tested behavior, fixture expectations, and `specs.md` /
`docs/install.md` descriptions aligned. Preserve the distinction between
reference data and authority: memory never grants tool access or overrides a
current user request. Use the existing user/workspace/workflow scopes and lexical
workspace normalization. No cloud memory service, agent-private identity system,
automatic approval, or vector database is part of this plan unless explicitly
specified in its scope.
