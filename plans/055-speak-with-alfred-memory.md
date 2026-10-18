# Plan 055: Speak with Alfred remembers the user, and speaks in the atelier's voice

## Status and scope

- Status: TODO.
- Priority: P1. Effort: L (about 4–5 focused days; Tracks 1–2 alone, about
  1½ days, already deliver "Alfred knows me").
- Requested on: 2026-10-16.
- Baseline: desktop commit `4894951` plus the uncommitted Speak with Alfred
  work (`src-tauri/src/builder/`, `src/features/builder/`). Commit that work
  before starting; this plan edits it.
- Depends on: Speak with Alfred (chat tab, loopback MCP server, chat history),
  and the memory store from Plans 045–051.
- Independent of: [Plan 046](046-restricted-memory-curator.md) and
  [Plan 052](052-agent-memory-lookup-tools.md). Both stay BLOCKED on live
  activation inside workflow runs; nothing here unblocks or changes them.

Give the Speak with Alfred agent a memory in the spirit of Hermes-style
assistants: it can search what Alfred already knows, it starts every chat
knowing the user's standing preferences, it proposes new memories that the user
approves with one click, it can search past conversations, and it speaks in a
persona the user sets for their atelier.

### Non-goals

- No automatic memory writes. Every new memory needs an explicit user click.
- No embeddings, vector store, cloud sync, or new retrieval ranking. Reuse the
  existing SQLite FTS search.
- No change to how workflow runs recall memory, to the memory review queue, or
  to the Memories inspector's workflow-scoped behavior.
- No agent-editable persona. Only the user sets tone.
- No deletion tool for the agent. Forgetting happens in the UI.

## Current state (verified 2026-10-16)

| Fact | Where |
| --- | --- |
| Memories have `scope_type` user / workspace / workflow; user scope uses `scope_key = 'local-user'` | `src-tauri/src/db/memories.rs:63`, `:961` |
| `memories.workflow_id` is nullable; `source` allows `manual`; `provenance_state` allows `manual`; `evidence_subject` allows `user_statement` | `src-tauri/src/db/schema.sql:94-115` |
| `create_memory` always resolves scope through a workflow (`resolve_scope` → `memory_context(workflow_id)`), so it cannot create a workflow-less user memory today | `memories.rs:953-967`, `:1090` |
| Bodies ≥ 32 KB spill to an artifact file under a workflow directory | `memories.rs:56` (`ARTIFACT_BODY_THRESHOLD`) |
| `search_history(HistorySearchInput { workflow_id: None, .. })` runs FTS over **all** run steps and **all** memories (the scope filter only applies when a workflow is given) | `src-tauri/src/db/history.rs:293-380` |
| Learned-claim candidates require `workflow_id NOT NULL` and `review_run_id` → `memory_reviews(run_id)`, so chat proposals cannot use that queue without a migration | `src-tauri/src/db/migrate.rs:585-607` |
| `format_pinned_context` exists but needs a workflow `MemoryContext` | `memories.rs:1724` |
| Speak with Alfred MCP tools live in one match in `call_tool`; tool cards are derived from tool-call input in the page | `src-tauri/src/builder/mcp.rs`, `src/features/builder/stream.ts` (`workflowPreview`) |
| Chats are owner-only JSON files in `<app data>/builder-chats/`; rules, MCP servers in `builder.json` | `src-tauri/src/builder/chats.rs`, `builder/mod.rs` |
| The system prompt is `BASE_PROMPT` plus enabled rules, rebuilt every turn | `builder/mod.rs` (`system_prompt`) |

## Design

### 1. Read tools on the Alfred MCP server

Add three tools to `tool_definitions()` and `call_tool()`:

| Tool | Input | Returns |
| --- | --- | --- |
| `search_memory` | `query` (required), `limit` (1–20, default 10) | `db.search_history` with `workflow_id: None`: `kind` (`memory` or `run_step`), `sourceId`, `title`, `snippet`, `workflowName`, `timestamp`. |
| `read_memory` | `id` | `get_memory`: title, type, scope, pinned, dates, and body. Use `memory_full_body` and cap at 8 KB with a `truncated: true` flag. Unknown or retracted ids return a tool error. |
| `get_profile` | none | The same block the prompt receives (section 2), for when the agent wants to re-read it. |

Searching every memory regardless of scope is intentional: Alfred is a single
local user, and this matches what History search already shows. State it in the
tool description so the agent does not assume workflow isolation.

The agent's system prompt gets one paragraph: search memory before asking the
user something they may already have told Alfred; cite memory titles when you
rely on them; never invent memories.

### 2. "Knows you" profile in every chat

- New DB function `list_user_profile_memories(now, byte_budget)`: active,
  current (`is_current_at`), `scope_type = 'user'`, `pinned = 1`, and
  `memory_type IN ('preference','fact','constraint','decision')`. Order by
  salience then `updated_at`. Stop adding rows at the byte budget (4 KB).
- `system_prompt()` appends `# What you know about the user` with one bullet
  per memory: `- <title>: <first 300 chars of body>`. Empty profile → no section.
- Setup gets a switch, "Use my memories in chats" (default on), stored in
  `builder.json` as `memoryEnabled`. Off removes the profile block and the
  three read tools from `tools/list`.

### 3. Learning: Alfred proposes, the user approves in the chat

- MCP tool `propose_memory` with `title`, `body`, `memoryType` (preference,
  fact, constraint, decision, lesson, or note), and `reason`. It **writes
  nothing.** It validates bounds (title ≤ 120 chars, body ≤ 4 KB, which stays
  below the 32 KB spill) and returns `{ "status": "awaiting_user" }`.
- The page renders a "Remember this?" card from the tool call input, the same
  way `workflowPreview` renders workflow cards. It shows editable title and
  body, a type label, a "Pin to profile" toggle (default on for preference and
  constraint), and **Save** and **Dismiss** buttons.
- **Save** calls a new command, `builder_save_memory`, which calls a new DB
  function `create_user_memory`. That function inserts through the same
  canonical write path (`write_canonical_memory`) with `scope_type = user`,
  `scope_key = 'local-user'`, `workflow_id = NULL`, `source = 'manual'`,
  `provenance_state = 'manual'`, `evidence_subject = 'user_statement'`,
  `confidence = 1.0`, and runs `find_exact_duplicate` first. Refactor
  `plan_canonical_insert` to accept an already-resolved scope rather than
  duplicating its validation.
- The card stores its outcome (saved memory id, or dismissed) in the chat
  message, so reopening the chat shows the decision instead of the buttons.
- Why not the existing candidate queue: its foreign keys require a workflow and
  a review run (see Current state). Relaxing them touches the curation code of
  Plans 046 and 049–050. Revisit only if chat proposals must appear in the
  Memories inspector.

### 4. Memory rail tab

Add a third rail tab, **Memory**, beside History and Setup:

- It lists user-scope active memories, pinned first, with title, type,
  relative date, and body preview.
- It uses `SearchInput` from `src/components/search-input` (repository rule)
  to filter by text.
- Each row can pin or unpin (existing `update_memory`) and forget (existing
  retract or delete path; prefer retract so History keeps the audit trail).
- An empty state explains: "Memories Alfred saves with your approval appear
  here."
- A new command `builder_list_user_memories` returns the list. It needs no
  workflow context.

### 5. Search past conversations

- Keep the JSON files as the source of truth. Add an FTS5 table
  `builder_chat_fts(chat_id UNINDEXED, message_id UNINDEXED, role UNINDEXED,
  at UNINDEXED, text)` through the existing migration pattern in
  `src-tauri/src/db/migrate.rs`.
- `builder_save_chat` replaces that chat's rows in one transaction: user text,
  assistant text parts, and tool labels. Thumbnails and tool payloads are never
  indexed. `builder_delete_chat` deletes them too.
- Add a one-time backfill on first launch after the migration that indexes
  existing chat files.
- MCP tool `search_chats` with `query` and `limit` returns chat title, date,
  role, and snippet. It excludes the current chat.

### 6. Atelier persona

- Setup gets a **Persona** section stored in `builder.json` as
  `persona: { name, preset, notes }`:
  - `name`, default "Alfred", at most 40 characters.
  - `preset`: one of `butler` (warm, precise, discreet; the default),
    `studio` (casual, collaborative), or `terse` (minimal words).
  - `notes`, free text up to 2 KB.
- `system_prompt()` starts with `# Persona`, built from the preset text plus
  notes, and replaces the hard-coded "You are Alfred" line with the chosen
  name.
- The page header still says "Speak with Alfred"; only the agent's voice
  changes. The empty-state greeting uses the persona name.
- There is no MCP tool for persona. The agent may suggest a change; the user
  makes it.

## Privacy and safety

- Memories never leave the Mac except inside prompts sent to the chosen agent
  CLI, the same exposure workflow runs already have. The Setup switch is the
  off switch.
- Every tool output is bounded: search up to 20 hits, `read_memory` up to
  8 KB, profile up to 4 KB, chat search up to 20 snippets.
- A proposal cannot persist anything without a click. A test must prove
  `propose_memory` leaves the `memories` table untouched.
- Memory ids from the agent are validated, and retracted or superseded rows are
  not returned by `read_memory`.

## Files expected to change

| Area | Files |
| --- | --- |
| DB | `src-tauri/src/db/memories.rs` (`create_user_memory`, `list_user_profile_memories`, a listing helper), `src-tauri/src/db/migrate.rs` (chat FTS), a new `src-tauri/src/db/builder_chats.rs` (index/search) |
| Builder backend | `src-tauri/src/builder/mcp.rs` (new tools), `builder/mod.rs` (persona, memory switch, prompt), `builder/chats.rs` (index on save and delete), `src-tauri/src/lib.rs` (new commands) |
| Frontend | `src/features/builder/api.ts`, `store.ts`, `stream.ts` (proposal extraction, labels), `components/builder-page/` (proposal card, Memory tab), new `components/builder-memory-panel/`, `components/builder-setup-panel/` (persona and memory switch), `builder.css` |
| Docs | `docs/` page describing what Alfred remembers and how to make it forget; this plan's status |

## Delivery tracks and STOP conditions

### Track 1: Read tools (about ½ day)

- Add `search_memory`, `read_memory`, `get_profile` (stubbed until Track 2) and
  their labels in `stream.ts` (for example "Searching memories").
- Exit when Rust tests cover bounds, unknown ids, and retracted rows, and a
  live chat answers "what do you know about X?" from a seeded memory.
- STOP if `search_history` with no workflow returns rows the History page would
  hide. Reconcile visibility with the History owners before shipping.

### Track 2: Profile and persona (about 1 day)

- Add `list_user_profile_memories`, the prompt block, the memory switch, and
  the Persona section.
- Exit when a pinned preference changes the agent's behavior in a fresh chat
  with both Claude Code and Codex, and when switching the persona preset
  changes the tone.

### Track 3: Proposals and Memory tab (1½–2 days)

- Add `propose_memory`, the proposal card, `builder_save_memory`,
  `create_user_memory`, and the Memory tab.
- Exit when "remember that I prefer Codex for reviews" produces a card, Save
  creates exactly one user memory (a duplicate click creates none), the next
  chat's profile contains it, and Forget removes it from the profile.
- STOP if refactoring `plan_canonical_insert` changes any existing memory test
  outcome.

### Track 4: Chat search (about 1 day)

- Add the migration, indexing on save and delete, the backfill, and
  `search_chats`.
- Exit when a fact said only in an older chat is found from a new chat, and
  deleting that chat removes it from results.
- STOP if the migration cannot be made idempotent with the existing
  `migrate.rs` versioning; ask before adding a new mechanism.

## Acceptance tests

| Scenario | Expected result |
| --- | --- |
| "What do you know about my deploy process?" with a matching memory | Agent calls `search_memory`, cites the memory title |
| Same question, no matching memory | Agent says it has nothing saved; no invented memory |
| Pinned preference "Prefers Codex for code review" | A new chat's first workflow uses Codex without being told |
| Memory switch off | No profile block in the prompt; memory tools absent from `tools/list` |
| "Remember that my repo lives in ~/dev/app" | "Remember this?" card; Save creates one user memory with `source = manual`, `workflow_id = NULL` |
| Dismiss a proposal, reopen the chat | Card shows "Dismissed"; no memory row |
| Agent calls `propose_memory` but the user never clicks | `memories` table unchanged |
| Forget a memory in the Memory tab | It leaves the profile and `read_memory` returns an error; History still shows it as retracted |
| Fact mentioned only in a week-old chat | `search_chats` finds it from a new chat |
| Delete that old chat | It disappears from `search_chats` |
| Persona preset `terse`, name "Jeeves" | Short replies; the agent introduces itself as Jeeves; header unchanged |
| Codex and Claude Code | Every scenario above passes with both agents |
| Both themes, keyboard only | Proposal card and Memory tab fully operable; focus visible |

## Verification

- Run `bun run check` and `git diff --check`.
- New Rust tests cover each tool's bounds and errors, `create_user_memory`
  duplicate handling, profile byte budget, chat FTS indexing and deletion, and
  migration idempotency.
- New frontend tests cover proposal extraction from a stream transcript, the
  card states, and the Memory tab rendering.
- Live verification must happen in the desktop app (`bun run dev`) with real
  Claude Code and Codex turns. Source reading does not count.

## Risks and mitigations

- **Prompt bloat:** fixed byte budgets and a single profile block.
- **Agent over-proposing memories:** the prompt rule "propose only durable
  preferences or facts the user stated, at most one per reply," and dismissals
  are cheap.
- **Visibility mismatch with History:** Track 1's STOP condition.
- **Canonical write refactor regressions:** run the full memory test suite
  (Plans 045–051 fixtures) after the refactor.
- **Large chat archives:** index text only, never images or tool payloads;
  backfill runs once in the background.

## Completion definition

Done when Alfred, in a new chat with either agent, uses what the user approved
it to remember, can search old memories, runs, and chats, never saves anything
without a click, speaks in the persona the user picked, and the user can see
and remove everything Alfred knows from the Memory tab.
