# Alfred — product & engineering specs

Living doc for what Alfred is, how it works, and how we build it. Prefer updating this when product decisions change.

Related docs: [README.md](README.md), [docs/install.md](docs/install.md),
[docs/releasing.md](docs/releasing.md),
[docs/connected-apps.md](docs/connected-apps.md),
[docs/app-actions.md](docs/app-actions.md),
[docs/design-system.md](docs/design-system.md), [plans/](plans/).

---

## 1. Product

| | |
| --- | --- |
| **Name** | Alfred |
| **Repo / package** | `alfred` |
| **Bundle id** | `com.nirzhuk.alfred` |
| **Version** | `0.5.0` (see `package.json` / `tauri.conf.json`) |
| **License** | GPL-3.0-or-later |
| **Author** | nirzhuk |

**One-liner:** Local desktop app for composing multi-agent workflow automations that shell out to AI coding CLIs already installed and signed in on the machine.

**Not in scope (current):**

- Mobile (Android / iOS) or a deployable website / PWA
- Storing or replacing agent API keys / subscriptions
- Running schedules / file triggers / webhooks when the app is fully quit (they run while the app is open, including tray / menu bar)

---

## 2. Platforms & distribution

- **Desktop only:** macOS 11+, Windows 10/11, Linux
- **Dev:** `bun install` → `bun run dev` (Tauri desktop)
- **Build:** `bun run build` → OS installers (`app`/`dmg`, `deb`/`rpm`/`appimage`, `nsis`/`msi`)
- **Vite** (`dev:frontend` / `build:frontend`) exists only as Tauri before-commands — not a standalone site
- **Ship:** paid official installers staged as private GitHub Release drafts;
  public GPL source remains buildable (see install/releasing docs)

---

## 3. Tech stack

| Layer | Choice |
| --- | --- |
| UI | React 19, TypeScript, Vite 7 |
| Graph editor | `@xyflow/react` (React Flow) |
| Client state | Zustand |
| Desktop shell | Tauri 2 (Rust) |
| Persistence | SQLite via `rusqlite` (bundled) |
| Connected-app credentials | Native OS credential store via `keyring` |
| Package manager | Bun |
| Plugins | dialog, notification, opener, window-state, single-instance |

---

## 4. Core concepts

| Concept | Meaning |
| --- | --- |
| **Workflow** | Named automation: graph (nodes + edges), optional working directory, schedule, triggers |
| **Input** | Instruction / context for downstream steps (legacy graph type: `prompt`) |
| **Agent** | Local CLI step: provider + optional model + optional skill |
| **Memories** | Inject selected / pinned library items into the run context |
| **Output** | Execution-time save/pin preferences and final-result selection; backend-owned receipt persistence |
| **Run** | One execution (manual, schedule, or event); statuses: pending → running → completed \| failed \| cancelled |
| **Schedule** | Optional cron (one per workflow) with a missed-run choice (run on open / skip / ask); ticker while app is open |
| **Trigger** | File watcher or loopback webhook; fires the same runner with a payload |
| **Library** | Durable notes / artifacts / agent outputs; star to inject on next run |
| **Skill** | `SKILL.md` package pinned on an agent step (`/skill-name …`) |
| **Connected App** | Provider-neutral local connection metadata in SQLite; OAuth credentials remain in the OS credential store |
| **App Action** | One descriptor-driven workflow node whose validation, credentials, and execution stay in Rust |

Inputs can be blocked to protect their label, prompt, attachments, size, and
canvas position from accidental edits. The blocked state is stored with the
workflow graph and can be reversed from the node, its context menu, or settings.

### Agent providers

| Id | CLI |
| --- | --- |
| `claude_code` | `claude` |
| `cursor` | `cursor-agent` / `agent` |
| `codex` | `codex` |
| `opencode` | `opencode` |
| `github_copilot` | `copilot` |
| `gemini` | `gemini` |
| `grok` | `grok` |

---

## 5. Main surfaces (UI)

- **Sidebar** — History / Schedules / Settings nav; workflow list (+ create, reorder, context menu)
- **Canvas** — React Flow editor, add-step panel, toolbar (cwd, run/save, etc.)
- **Activity panel** — This run / Result / Library / Live log
- **Schedules page** — All cron schedules across workflows
- **Settings** — Theme, OS notifications, and Connected Apps status/disconnect
- **Modals** — rename, schedule, triggers, output, memories inspector, confirm delete
- **Tray / menu bar** — presence while running / scheduled work matters

---

## 6. Runtime (high level)

1. Start via UI `run_workflow`, schedule tick (~20s), file/webhook trigger, or tray
2. Insert SQLite `runs` row (`trigger_kind` + optional `payload_json`); spawn worker
3. Load graph → execution levels (Kahn waves: every node's predecessors sit in
earlier levels) → prelude (pinned memories + trigger payload)
4. Run one level at a time, gated by the workflow's `parallelBranches` switch
(toolbar Parallel/Sequential button; new workflows default to parallel, legacy
rows migrate to sequential). The Plan 053 skip gate evaluates against the
level-entry snapshot so every branch sees the same completed predecessors;
skips record immediately in graph order and only runnable nodes fan out.
Parallel on: width-1 levels run inline while wider levels fan out on scoped
threads sharing the snapshot, each branch getting a forked run control (shared
cancel flag, private CLI-child slot) so concurrent agent processes stay
killable and any failure or user cancel stops siblings fail-fast. Parallel
off: every node runs inline in order against the running state (pre-parallel
behavior).
5. Join each level deterministically in graph order: append-only context
suffixes concatenate (an overwriting input/template still wins, as in
sequence), step outputs merge into labeled `## <label>` sections — also lifted
into context for context-silent agent branches so a downstream judge sees
every debater instead of only the last one — task-query fields are
last-changed-wins, file attribution unions unless an agent step reset it, and
the last final-output set wins. The first error in graph order fails the run;
pure cancellation cancels it. Cyclic leftovers run one level each, sequentially.
6. Walk node kinds: Input → Memories → Agent (CLI adapter) → Output (capture execution-time disposition; backend persists save receipt / final result)
7. Persist `run_steps`, emit `run://event` for the activity UI; finish status; refresh tray

Schedules and event triggers only fire while Alfred is running (including tray).
A scheduled time that passes while Alfred is closed is a miss: that schedule's
own choice either runs it as soon as Alfred opens, drops it, or leaves it due in
a list the user answers per schedule (`list_missed_schedules` /
`resolve_missed_schedule`).

### Backend-owned output persistence

An Output node's save, pin, title, final-result, HTML-report, and changed-files
settings are captured from the executing graph at node execution time. The
backend inserts the completed `run_step` and its output-receipt seed in one
transaction; the renderer does not decide whether a completed output is saved.
The receipt key is `(run_id, node_id)`, and its storage identity is retained
across save retries.

The receipt is durable and reports `not_requested`, `skipped_empty`, `pending`,
`saving`, `saved`, `failed`, or `deleted`. In particular, `saved`, `failed`, and
`deleted` remain explicit outcomes in History: a saved receipt points to the
canonical memory or artifact, a failed receipt retains the completed output and
stable failure code, and a deleted receipt is a tombstone after cleanup is
requested. The full output remains in canonical `run_steps.output_json`; the
receipt does not duplicate it. SQLite is the source of truth; UI receipt events
are notifications emitted after a durable transition.

`Retry save` is an explicit save-only retry. It may claim only a failed receipt
with a retryable storage or database failure and reuses the completed `run_step`
and storage identity; it never invokes the agent or reruns the workflow.

Startup recovery begins immediately: Alfred atomically snapshots pending and
interrupted `saving` receipts, fences interrupted claims, and processes the
snapshot in batches of at most 32. A larger backlog is handled across batches;
failed recovery rows remain pending for later retry rather than being dropped.

When workflow or memory deletion starts, the backend tombstones owned output
receipts before removing staged or final files. A saved output promoted to user
or workspace scope is not owned by the origin workflow; its canonical memory
and artifact remain live when that workflow is cleared.

---

### Agent-requested memory lookup

`memory_lookup_enabled` is a separate workflow permission from
`memory_retrieval_enabled` (**Automatic recall**). It defaults off for new and
imported workflows. Each request rechecks this permission and the persisted
run/workflow relationship; enabling the setting does not grant an unavailable
runtime.

The release-enabled native provider registry is currently empty, so this path
is unavailable. Alfred never falls back to Provider CLI or exposes an
unrestricted second route.

The provider-neutral contract exposes exactly four bounded, read-only
operations:

| Operation | Read-only use |
| --- | --- |
| `memory.search` | Search current visible memories in the bound scope |
| `memory.get` | Read one current memory by an opaque reference, in pages |
| `history.search` | Search saved workflow outputs, historical memories, and evidence |
| `history.get` | Read one historical source by an opaque reference, in pages |

Requests are capped at 4 KiB; queries at 2,000 bytes / 24 terms (64 characters
per term); results at 8 items; search and get responses at 6,000 bytes; and a
body page at 4,000 bytes. A step may make at most 8 calls and receive 16,000
bytes total within a 5-second deadline. References and cursors are opaque,
session-local, and bound to the run, node, workflow, workspace, and source
version. Scope, lifecycle, receipt, and evidence visibility are rechecked on
every call.

Lookup traces are scoped to the run and node. They record source kind,
identifier/version/state/page, request/tool identity, retrieved and returned
byte counts, and delivery classification. Accepted deliveries can be projected
into body-free evidence inputs; lookup bodies, raw search queries, and provider
context are not copied into those evidence traces.

---

### Reviewable post-run memory suggestions

Memory review is a **candidate-only** pipeline, off by default globally and per
workflow. Ordinary Provider CLI execution is not a reviewer. The only reviewer
path currently specified is the restricted Claude Code transport, whose
activation remains unavailable in this release until every gate passes: exact
runtime `2.1.263`, restriction contract `memory-review-restricted-v1`, all
required restricted flags, verified managed-policy assurance, and an
authorized synthetic live-activation check on an already-authorized account.
The currently installed `2.1.271` runtime does not satisfy the exact-runtime
gate. Before the gate is recorded, jobs are marked unavailable/skipped with
zero provider calls; Alfred does not fall back to an ordinary CLI invocation.

Review settings require choosing the restricted reviewer and acknowledging that
each eligible completed run may make at most **one additional model
invocation** (never for failed or cancelled runs). There are no automatic
provider retries; a failed review can be retried once manually. The restricted
request receives a bounded digest of persisted run text — at most 32 KiB —
plus up to 12 relevant existing memories. It carries fixed review instructions
only, with no workspace, skills, session, or tool configuration. Provider
authentication and billing remain CLI-owned.

The restricted transport's required flags are the allowlisted
`--safe-mode`, `--restricted`, `--tools`, `--strict-mcp-config`,
`--disable-slash-commands`, `--setting-sources`, `--permission-prompts`,
`--max-turns`, `--input-format`, `--output-format`, and
`--no-session-persistence`. The selected product/runtime and restriction
contract are persisted with the job. One review process is active at a time;
ready work is bounded at 32 jobs, with overflow durable in SQLite for restart
recovery.

The reviewer returns strict JSON proposing `create`, `supersede`, or `retract`
candidates (at most five). Every candidate is deterministically validated for
size, scope, target visibility, exact current duplicates, secret-like material,
invisible characters, and instruction-like language before storage. Exact
duplicate identity collapses whitespace but preserves case; looser matches are
shown only as possible relationships. Malformed responses are rejected whole
without a "repair" call.

Targeted candidates retain the reviewed target ID, semantic revision, content
digest, relevant dates, and completeness. Candidate edits and approvals require
the displayed candidate revision. Approval reloads and revalidates both
versions in one transaction; stale or incomplete destructive targets become
`blocked` instead of being retargeted. Artifact targets are incomplete because
their external bytes can change independently of the database. Observation and
validity dates require supporting source text or host timestamps; review does
not invent an expiry. Future-effective corrections remain pending and do not
supersede the current claim early.

Candidates never change canonical memory by themselves. Users can edit
title/body/scope/type and supported validity dates while pending, approve,
reject, or retry a failed review once manually. Reviews never modify skills and
never write directly to memory; failures persist only stable codes
(`auth_required`, `restricted_review_unavailable`, `provider_unavailable`,
`timeout`, `invalid_response`, `queue`, `internal`) and never affect run status
or output. Review can be disabled globally or per workflow at any time, and
decided suggestion history can be physically deleted through Data settings.

---

## 7. Data (SQLite)

Primary entities (see `src-tauri/src/db/schema.sql`):

- `workflows` — name, description, working directory, `graph_json`, sort order
- `agents` — provider registry / metadata as used by the app
- `runs` / `run_steps` — execution history
- `schedules` — one cron row per workflow
- `triggers` — file / webhook configs
- `memories` / `memory_links` — library + cross-workflow links
- `app_connections` — non-secret provider/account/scopes/health metadata; credentials are never stored in SQLite

### Local history search

Alfred provides searchable run history across persisted run steps and saved
memories. Search stays on the machine and uses SQLite FTS5 only—there is no
embedding model, remote search service, or network request. The FTS tables are a
derived index that can be rebuilt from canonical `runs`, `run_steps`, and
`memories` rows. Creating, updating, or deleting canonical data updates or
removes the corresponding search documents transactionally.

Run history can contain prompts, agent/tool results, errors, and saved memory
text. Treat it as private local data with the same sensitivity as the workflows
that produced it.

### Scoped atomic memory

Each memory is one compact claim or note. Its semantic type—`preference`,
`fact`, `decision`, `constraint`, `lesson`, `episode`, `checkpoint`, `note`,
`output`, or `artifact`—is separate from its rendering/content kind (`text`,
`note`, or `artifact`). Memory has three visibility scopes:

- **User memory** uses the fixed local identity `local-user` and is visible to
  every workflow on this installation.
- **Workspace memory** is visible to workflows whose configured absolute
  working-directory path has the same purely lexical normalization. Alfred
  removes `.` and collapses safe `..` components; it does not query the
  filesystem, resolve symlinks, probe mounts, or contact network paths.
- **Workflow memory** is visible to its owning workflow and to workflows with
  an explicit legacy memory link.

Lifecycle is explicit: `active`, `superseded`, or `retracted`. A record is
current only while active, after its optional `validFrom` instant (inclusive),
and before its optional `expiresAt` instant (exclusive). Missing dates do not
create a default expiry. Future, inactive, and expired records remain
inspectable for correction history but never enter a prompt; a pinned future
record stays pinned and is omitted until it becomes current. Explicit deletion
physically removes the canonical local row; deleting a workflow removes its
workflow memory and owned output receipts while preserving promoted
user/workspace memory and its output artifact.

Selected Memory-node references, pinned active memory, and automatic recall
share a 12,000-byte UTF-8 limit at final Agent or Custom agent dispatch.
Selected references are allocated first in authored order (6,000 total / 1,500
per item), then pins (6,000 total / 1,500 per item), then up to eight recalled
items (6,000 total / 1,200 per item) in the remaining space. Pin allocation
retains the user, workspace, and workflow/linked order and its 1,500 / 2,000 /
2,500 soft scope shares. An id is rendered once with selected, pinned, then
recalled precedence. Headings, trust text, separators, and aggregate omission
notices count toward the limit. Overflow remains in the library. Durable memory
is reference data, not authorization: it cannot
override the current request, workflow instructions, permissions, or safety
boundaries, and instructions embedded in memory text are ignored.

Current context and last output retain typed origins until dispatch. A direct
Memory-node value remains a memory reference through template copies; generated
or tool output that happens to quote it does not become a reference. Utility
templates still receive complete visible memory values and are outside this
agent-memory limit. Prompt rendering checks scope, lifecycle, and expiry again,
and reads artifact excerpts with bounded I/O. Truncated excerpts and fallback
previews for unavailable or malformed artifacts are labeled without changing
the stored source.

### Automatic recall

New workflows default **Automatic recall** on; existing workflows migrated
from earlier Alfred versions remain off until the user opts in from the
Memories inspector. The same switch disables recall again without changing the
workflow graph or deleting memory.

Immediately before every Agent or Custom agent step, Alfred builds a bounded
query from authored Input/Prompt text, authored template text and substitutions,
and allowlisted message, subject, project, ticket, or document fields from a
connected-app event. Model output, arbitrary tool output, memory bodies,
identity instructions, and provider wrappers do not enter this query. Candidate
visibility reuses the scoped, active, unexpired memory rules above.

Automatic recall uses literal-safe disjunctive FTS5 matching, term coverage,
and exact identifier evidence. Scope, salience, confidence, and last-confirmed
recency deterministically order candidates that already have query evidence;
there is no recent-only fallback. An unrelated or empty query succeeds with no
recalled memory. Preference, fact, decision, constraint, and lesson records are
eligible; notes are eligible only when manually authored or approved through
review. Episode, checkpoint, output, and artifact records remain available for
pinning and explicit Memory nodes but are excluded from automatic recall.
Pinned core memory and memories already loaded by a Memory node are excluded
from recall so prompt text is not duplicated. Utility nodes never receive
automatic memory.

Recalled context is capped at 8 items and 6,000 UTF-8 bytes, with at most 1,200
bytes per item inside the shared limit, and the query projection is separately
bounded to 8,000 bytes. Every final included id, reason, rank, score, rendered
byte count, and ranking policy version is recorded in `run_memory_uses` and
shown in History. Selected rows use `explicit_v1`, pins retain `pinned_v1`, and
current recall uses `lexical_v2`; historical lexical versions remain readable.
A per-run/node summary records the 12,000-byte budget, actual memory bytes,
omission counts, and trace availability. Neither audit table copies a memory
body or search query. History distinguishes a successful empty result from
unavailable recall and explains omissions by count and cause.
A retrieval or FTS5 failure is non-fatal and the agent proceeds without recalled
context while preserving pinned core behavior.

Recall has no embeddings, remote retrieval API, network call, or model download.
Retrieved text is explicitly untrusted reference data: it cannot override
current instructions, authorize actions, expand connected-app scope, or grant
tool access.

---

## 8. Repo layout

```
src/                          # React app
  components/                 # shared UI (kebab folders)
  features/workflow/          # canvas, store, api, nodes, panels
  features/settings/          # settings, theme, notifications
  features/integrations/      # Connected Apps and generic App Action UI/state
src-tauri/src/
  agents/                     # CLI adapters
  runner/                     # execute graphs
  db/                         # SQLite + schema
  scheduler/                  # cron ticker
  triggers/                   # file + webhook
  skills/                     # SKILL.md discovery
  commands/                   # Tauri invoke handlers
  integrations/               # provider catalog, OAuth, keychain, refresh, action registry
docs/                         # install, releasing
plans/                        # product / licensing / release handoffs
scripts/guard-desktop-tauri.mjs
```

---

## 9. Engineering conventions (current)

### Desktop-only

- Never add Android/iOS targets or treat Vite as a deployable website
- Always use `bun run dev` / `bun run build` for the desktop shell

### Components

- Shared UI → `src/components/<kebab-name>/`
- Feature UI → `src/features/<feature>/components/<kebab-name>/`
- Folder + files: kebab-case; export: PascalCase
- Colocate `*.tsx`, `index.ts`, optional `*.test.tsx` in the same folder
- Import via folder path, not a lone PascalCase file at a components root

### Style / UX (working defaults)

- `docs/design-system.md` is the visual source of truth; shared decisions use
  semantic CSS custom properties in `App.css`, not feature-local literals
- Alfred prefers Infer for interface text, uses bundled Geist as its local
  fallback, and uses Geist Mono for utility accents; the desktop UI makes no
  runtime font request
- Application-owned UI uses identical tokens on macOS, Windows, and Linux;
  platform branches are limited to OS chrome, shortcuts, permissions, and
  native system surfaces
- The root `data-platform` attribute is the CSS platform boundary. Components
  must not implement their own user-agent checks
- The type scale is 11/12/14/16/20/24px and weights are limited to
  400/500/600/700; fractional font weights are not allowed
- Shared spacing follows the documented four-pixel grid; controls, cards,
  dialogs, icons, motion, elevation, and layering use their semantic token scales
- Product dialogs use the shared top-anchored precision-sheet shell with a
  blurred scrim, mono context rail, trapped keyboard focus, and opener focus return
- Sidebar item text is always 14px/400, icons are 18px, and section labels are
  16px/600 across workflow and Settings navigation
- Navigation is flat by default: selection uses a subtle surface background,
  never a weight shift, border, or shadow
- Interactive components preserve dimensions across default, hover, pressed,
  selected, focus-visible, disabled, error, and loading states
- Keyboard focus, accessible names, AA contrast, and reduced-motion behavior
  are required in light and dark themes
- Title-bar content reserves macOS traffic-light space but uses the standard
  content inset on Windows, Linux, unknown platforms, and in fullscreen
- Keep activity panels compact (e.g. Library list max-height) when lists can grow large
- Menus: shared `src/components/menu/` (dropdowns portaled when needed)

### Open / planned (see `plans/`)

- Offline licensing / freemium entitlements (Polar) — **not implemented yet**
- Release signing secrets, Homebrew cask, in-app updater — tracked in `plans/004`–`006`

---

## 10. Open questions (fill in)

These need product / team answers. Edit this section as decisions land.

### Product

- [ ] Target user (solo local-first? teams later?)
- [ ] What “done” looks like for v1 vs freemium paid tier
- [ ] Offline / tray expectations: any “run when quit” story, or stay “app must be open”?
- [ ] Default privacy story: data never leaves machine except agent CLIs?

### Code style

- [ ] Formatter / linter of record (Prettier? Biome? rustfmt only? ESLint?)
- [ ] Prefer CSS modules / Tailwind later, or stay global `App.css`?
- [ ] Test policy: unit required for new components? Rust tests for runner?
- [ ] Commit message style (conventional commits?)
- [ ] Absolute import alias (`@/…`) required vs relative?

### Specs process

- [ ] Is `specs.md` the source of truth, or should product live under `docs/`?
- [ ] Who updates this on merge — author of the PR?
- [ ] How do `plans/` handoffs relate (link only vs promote into specs when shipped)?
