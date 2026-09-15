# Install Alfred

Alfred is a **local desktop app**. It does not include AI models or agent
subscriptions. It runs workflows by calling CLIs you already use on this
machine.

There are two ways to get it, both free:

| Option | Cost | Intended for |
| --- | --- | --- |
| **[Official builds](https://github.com/Nirzhuk/alfred-workflows/releases/latest)** | **Free** | Most people — maintainer-built, CI-tested installers for macOS, Windows, and Linux |
| [Build from source](building-from-source.md) | **Free** | People comfortable installing Bun, Rust, and Tauri's platform prerequisites |

Neither route requires payment, and nobody is ever required to pay for
Alfred. A self-built Alfred has **every feature**, including cron schedules
and file/webhook triggers, enabled by design. Official builds are identical
except that those two automation perks unlock with an optional one-time
supporter licence — permanent, with no expiry — while manual workflow runs
and everything else stay free. Official macOS builds are Developer ID
signed, notarized, and stapled; the Windows installer is an unsigned beta.
Alfred runs no billing, account, or download backend of its own. Local
workflows stay on this machine unless you explicitly review and publish a
template through the optional sharing service; an executing workflow still
sends its configured prompt/data to the provider CLI or connected service it
uses. See
[Supporting Alfred](open-source.md#supporting-alfred).

## Requirements

| | |
| --- | --- |
| **OS** | macOS 11+, Windows 10/11 (64-bit), or a modern Linux desktop (x64) |
| **Agents** | At least one supported CLI installed and signed in (see below) |
| **Optional** | `git` — used when a run reports working-tree file changes |

You only need the CLIs for the agents you put on a workflow. Unused providers
can stay uninstalled.

### Supported agent CLIs

Install and authenticate each tool with its own installer / login flow. Alfred
looks for these binaries on your `PATH` (and common install locations):

| Provider | Binary | Check |
| --- | --- | --- |
| Claude Code | `claude` | `claude --version` |
| Cursor Agent | `cursor-agent` (or `agent`) | `cursor-agent --version` |
| Codex | `codex` | `codex --version` |
| OpenCode | `opencode` | `opencode --version` |
| GitHub Copilot | `copilot` | `copilot --version` |
| Gemini CLI | `gemini` | `gemini --version` |
| Grok Build | `grok` | `grok --version` |
| Pi | `pi` | `pi --version` |
| OMP | `omp` | `omp --version` |

Each CLI must already be logged in the same way you use it in a terminal.
Alfred does not collect provider passwords or API keys; it reuses the
credentials those CLIs already store on your machine.

Agent steps default to the first-class **Provider CLI** harness. The separate
**Alfred** harness is additive and currently reports native execution as
unavailable; it never falls back to a CLI or imports CLI credentials. See
[Agent harnesses](agent-harnesses.md) for the compatibility and account rules.

If a run fails with “CLI not found”, open a terminal, confirm the command
above works, then fully quit and reopen Alfred so it picks up an updated
`PATH`.

## Install an official build

Official installers are published as public assets on the
[GitHub releases page](https://github.com/Nirzhuk/alfred-workflows/releases/latest).

1. Open the releases page and download the file for your platform.
2. Compare its SHA-256 checksum against `SHA256SUMS.txt` attached to the same
   release.
3. Install it using the per-platform steps below.

### macOS

1. Download the `.dmg` for your chip (Apple Silicon or Intel).
2. Open it and drag **Alfred** into Applications.
3. Launch from Applications (or Spotlight).

Both official macOS disk images are Developer ID signed, notarized, and
stapled. An unsigned self-build may need separate approval under System
Settings → Privacy & Security.

### Windows — unsigned beta

1. Download the **NSIS** `.exe` installer from the release assets.
2. Run the installer and follow the prompts.
3. Launch **Alfred** from the Start menu.

The Windows installer is an **unsigned beta**. It is not Authenticode signed.
Windows will report an unknown publisher and SmartScreen will very likely warn
you before it runs. That warning is expected — this is not a warning-free
Windows release. Continue after comparing the SHA-256 checksum with
`SHA256SUMS.txt`.

### Linux

Linux packages ship with every release. Pick one:

- **AppImage** — mark executable (`chmod +x …AppImage`) and run
- **`.deb`** — `sudo dpkg -i …deb` (Debian/Ubuntu)
- **`.rpm`** — install with your distro’s package tool

WebKitGTK is required for the UI (same family of libs other Tauri apps use).

### Updating

Alfred has no automatic updater and does not download installers for you. In
the app, **Help → Download Latest Version…** (also in the tray/menu-bar menu)
opens the GitHub releases page in your browser; download the newer file and
install it over your existing copy. Your workflows, runs, schedules, and
memories are local and survive an upgrade install.

## Build it yourself for free

Follow [Build Alfred from source](building-from-source.md). The short form,
after installing the prerequisites, is:

```bash
bun install --frozen-lockfile
bun run check
bun run build
```

Self-built artifacts appear under `src-tauri/target/release/bundle/`. They do
not use the maintainers' signing identities and are not covered by official
binary support.

## First launch checklist

1. Install Alfred for your OS (above).
2. Confirm at least one agent CLI works in a normal terminal.
3. Open Alfred and create a workflow with that agent.
4. Run once manually before enabling schedules or file/webhook triggers.

## How Alfred behaves on your machine

- **Local data** — workflows, runs, schedules, and memories live in an on-disk
  SQLite database under the app’s application-support directory. Nothing is
  uploaded to an Alfred cloud. Memory remains local until you explicitly delete
  it; retracting or superseding a claim retains it for correction history.
- **Local history search** — searchable run history covers persisted run steps
  and saved memories using SQLite FTS5. It uses no embeddings or remote search
  service. The search tables are a derived index that Alfred can rebuild from
  canonical local rows; deleting canonical runs or memories removes them from
  search. History may include prompts and agent/tool results, so treat it as
  private local data.
- **Schedules & triggers** — cron, file watchers, and local webhooks only fire
  while Alfred is running (including when the window is closed but the app
  stays in the menu bar / tray). Fully quitting the app pauses automations.
- **Webhooks** — listen on loopback only (default port `8787`). Override with
  the `ALFRED_HTTP_PORT` environment variable and restart the app.
- **Permissions** — agent steps inherit whatever tools and file access those
  CLIs already have. Treat workflow prompts like instructions you would type
  into the agent yourself.

### Memory scope and prompt safety

- User memory is keyed to `local-user` and appears in every workflow on this
  installation. Workspace memory appears only for the same lexically normalized
  configured absolute working-directory path. Workflow memory stays with one
  workflow unless explicitly linked to another.
- Memory meaning (`preference`, `fact`, `decision`, `constraint`, `lesson`,
  `episode`, `checkpoint`, `note`, `output`, or `artifact`) is separate from
  content kind (`text`, `note`, or `artifact`).
- Memory lifecycle is `active`, `superseded`, or `retracted`. An active record
  can enter a run prompt from its optional `validFrom` instant (inclusive) until
  its optional `expiresAt` instant (exclusive). Missing dates do not impose a
  default expiry. Future, inactive, and expired records remain visible; a pinned
  future record stays pinned and is omitted until it becomes current.
- Selected, pinned, and automatically recalled memory share a 12,000-byte
  UTF-8 limit for each Agent or Custom agent step. Selected memory is allocated
  first (6,000 total / 1,500 per item), then pins (6,000 total / 1,500 per
  item), then recall from the remaining space. Pin capacity keeps its user,
  workspace, and workflow/linked scope order. Headings, trust text, separators,
  and omission notices count toward the limits.
- Large memory bodies and artifact files are read as bounded excerpts for agent
  prompts. A shortened excerpt is labeled; a missing, unreadable, or malformed
  artifact can use only its stored preview and is labeled incomplete. The
  original library item and artifact are not changed.
- Durable memory is reference data, not authorization. It cannot grant
  permission or override your current request, workflow instructions, or safety
  boundaries; instructions embedded in memory text are ignored.

### Saved outputs

Output save choices are read from the graph by backend execution when the
Output node runs. History records durable `Saved`, `Failed`, and `Deleted`
receipts alongside pending states; a failed receipt keeps the completed output
available for a save-only retry. **Retry save** retries persistence of that
output only, for retryable storage or database failures, and never reruns the
workflow.

Startup recovery begins immediately and processes pending or interrupted saves
in batches of at most 32. A larger backlog continues across batches, and a
failed recovery row remains pending rather than being dropped. A user or
workspace memory promoted from an output, including its artifact, survives
clearing the origin workflow.

### Automatic recall

- New workflows have **Automatic recall** enabled. Existing workflows remain
  off after migration until you opt in. Open the workflow's Memories inspector
  and use the Automatic recall switch to enable or disable it at any time; this
  does not rewrite the graph or delete memory.
- Recall runs locally immediately before each Agent and Custom agent step. Its
  bounded query uses authored task/template text and allowlisted message,
  subject, project, ticket, or document event fields. It excludes model/tool
  output, memory bodies, identity text, and provider wrappers.
- SQLite FTS5 matches words disjunctively and gives additional evidence to
  exact identifiers. Scope, recency, salience, and confidence order matching
  candidates. There is no recent-only fallback, so an empty or unrelated task
  succeeds with no recalled memory. Utility nodes receive no automatic memory.
- Automatic recall includes preference, fact, decision, constraint, and lesson
  records, plus manually authored or review-approved notes. Episode, checkpoint,
  output, and artifact records can still be pinned or selected explicitly.
- Each step can add at most 8 recalled items / 6,000 UTF-8 bytes, with a
  1,200-byte per-item limit inside the shared 12,000-byte memory limit. History
  records selected, pinned, and recalled ids, reasons, ranks, rendered sizes,
  policy versions, final memory bytes, omission counts, and trace availability
  without copying bodies or the search query.
- Recall failure is non-fatal: the workflow continues without recalled context
  and keeps its pinned core context. Recall uses no embeddings, network retrieval,
  or model download.

### Allow agents to look up memory

**Allow agents to look up memory** is a separate, default-off workflow
permission from **Automatic recall**. The current release has no
release-enabled native runtime for this path. Enabling the permission cannot
make an unavailable runtime available, and Alfred never falls back to Provider
CLI.

When a verified runtime exists, lookup is read-only and exposes exactly four
bounded operations: `memory.search`, `memory.get`, `history.search`, and
`history.get`. Each call rechecks permission and scope; opaque references and
cursors are bound to the run, node, workflow, workspace, and source version.
Per-step calls, response bytes, body pages, and the deadline are bounded (at
most 8 calls / 16,000 returned bytes in 5 seconds). Accepted deliveries leave
scoped, body-free evidence metadata; lookup bodies and raw search queries are
not copied into evidence traces.

### Memory review (optional, off by default)

Memory review is candidate-only and **off by default**, both globally (Settings
→ Memory review) and per workflow (Memories inspector → "Suggest memories after
runs"). Ordinary Provider CLI execution is not a reviewer. The restricted
reviewer is currently unavailable until its exact activation gate passes: exact
Claude Code runtime `2.1.263`, restriction contract
`memory-review-restricted-v1`, required restricted flags, verified managed-policy
assurance, and an authorized synthetic live-activation check on an
already-authorized account. The installed `2.1.271` runtime does not satisfy the
exact-runtime gate. Until the gate is recorded, review jobs are marked
unavailable/skipped with zero provider calls; there is no ordinary CLI fallback.

- **Consent** — when capability is available, enabling it requires choosing the
  restricted reviewer and ticking an acknowledgement that each eligible
  completed run may trigger at most one additional model invocation. Alfred
  never stores credentials; the restricted transport uses its own login.
- **Invocation** — at most **one additional model invocation** happens after
  each eligible completed run. Failed and cancelled runs are never reviewed,
  there are no automatic retries, and a failed review can be retried once from
  History or the Suggestions queue.
- **Privacy** — the restricted transport receives a bounded digest of the run's
  persisted text (at most 32 KiB) plus up to 12 relevant existing memories,
  together with fixed review instructions. It has no workspace, skills,
  session, or tool configuration. Authentication and billing remain
  CLI-owned.
- **Candidate-only** — suggestions never touch your saved memories directly.
  Each one shows its operation, proposed scope/type, confidence, rationale,
  source run, supported dates, and the exact reviewed target version. Candidate
  edits and approvals require the displayed revision. Approval rechecks the
  candidate and target in one transaction, so a stale or incomplete destructive
  suggestion becomes "blocked" with a plain-language reason instead of being
  retargeted or applied. External artifact targets are treated as incomplete.
  Future-effective corrections remain pending and do not supersede a current
  claim early. Approving user-scope changes or retractions always asks for
  confirmation.
- **Evidence and relationships** — observation and validity dates must be
  supported by quoted source text or host timestamps; review does not invent an
  expiry. Exact duplicate checks collapse whitespace while preserving case.
  Looser matches appear only as possible relationships for you to inspect.
- **Recovery** — turn review off globally or per workflow at any time; that
  stops all future reviews immediately without deleting anything. Decided
  suggestion history can be physically deleted through Settings → Data &
  storage → "Clear decided suggestions". Review failures show stable reasons
  (such as `auth_required`, `restricted_review_unavailable`, or `timeout`) and
  never change a run's status or output.

## Troubleshooting

| Symptom | What to try |
| --- | --- |
| “agent CLI not found” | Install the selected CLI, verify it in Terminal/PowerShell, restart Alfred |
| Agent runs but returns auth errors | Log in with that CLI’s own login command; Alfred does not re-auth for you |
| Alfred harness is blocked or unavailable | Keep using Provider CLI, or explicitly switch that node back to Provider CLI after confirming the account/billing difference; there is no automatic fallback |
| Native account is expired or stuck disconnecting | This zero-native release cannot refresh or reconnect it; use Settings → Native agent accounts to disconnect, or revoke/rotate at the provider before removing local recovery metadata |
| Schedule never fires | Keep Alfred running (tray is enough); confirm the schedule is enabled |
| A scheduled run didn’t happen while Alfred was closed | Open the workflow’s Schedule… dialog and check “If Alfred was closed at the scheduled time”: run when Alfred opens, skip the missed run, or ask per schedule |
| Webhook not reachable from another device | By design — binds to `127.0.0.1` only |
| macOS won’t open the app | Prefer a notarized build, or allow it under Privacy & Security |

## Related docs

- [Building from source](building-from-source.md) — complete platform setup
- [Open-source policy](open-source.md) — the distribution policy
- [Releasing](releasing.md) — how official binaries are built and published
- [README](../README.md) — product overview and developer setup
- [Native harness support](native-harness-support.md) — safe diagnostics, account recovery, and explicit fallback
