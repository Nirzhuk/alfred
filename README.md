# Alfred

**Alfred** is a local desktop app for building automations across your AI coding agents.

Compose workflows as visual graphs: write prompts, route them through agents you already use on your machine, optionally pin a **skill** and a **model**, then run the sequence and choose which outputs to keep.

> **Free and open source.** The complete source is available under
> [GPL-3.0-or-later](LICENSE). Official maintainer-built installers are
> published as public
> [GitHub Release assets](https://github.com/Nirzhuk/alfred-workflows/releases/latest)
> — free for everyone. An optional one-time supporter licence exists (see
> [Support Alfred](#support-alfred)); nobody is ever required to pay.

| Get Alfred | Cost | What you get |
| --- | --- | --- |
| **Official builds** | **Free** | Maintainer-built, CI-tested installers from GitHub Releases. macOS builds are Developer ID signed, notarized, and stapled; the Windows installer is an unsigned beta |
| [Build from source](docs/building-from-source.md) | **Free** | A local build you compile yourself, with every feature |

Updates are **manual**. Alfred has no automatic updater: **Help → Download
Latest Version…** opens the GitHub releases page in your browser, where you
download the newer installer and install it over your existing copy. Your
workflows, runs, schedules, and memories are local and survive an upgrade.

Fees charged by Claude, Cursor, OpenAI, or other agent providers for their own
services are separate from Alfred in every case. See
[Open-source and distribution policy](docs/open-source.md) for details.

## Support Alfred

Official binaries and source builds are both completely free, and nobody is
ever required to pay for Alfred. If you want to support its development, a
one-time **Alfred Supporter** licence permanently unlocks two automation
perks — cron **schedules** and file-watch/webhook **triggers** — in official
builds. The licence never expires, and nothing you bought is ever taken away
later. A build you compile from source includes those features unlocked by
design, and manual workflow runs stay free in every build.

<!-- TODO: supporter checkout link (`VITE_POLAR_DESKTOP_CHECKOUT_URL`) once configured -->

## Install

**User guide:** [docs/install.md](docs/install.md) — official installers,
source builds, OS requirements, and agent CLI setup.

Short version:

1. Download an official installer from
   [GitHub Releases](https://github.com/Nirzhuk/alfred-workflows/releases/latest)
   or [compile from source](docs/building-from-source.md) — both free, both
   fully featured.
2. Install and sign in to at least one agent CLI you plan to use:
   - Claude Code → `claude`
   - Cursor Agent → `cursor-agent` (or `agent`)
   - Codex → `codex`
   - OpenCode → `opencode`
   - GitHub Copilot → `copilot`
   - Gemini CLI → `gemini`
   - Grok Build → `grok`
3. Confirm the CLI works in a normal terminal, then open Alfred and run a workflow.

Alfred does **not** replace those subscriptions or store their API keys. The
first-class Provider CLI harness shells out to CLIs already authenticated on
your machine; the additive Alfred harness has separate accounts and currently
reports native execution as unavailable. See [Agent harnesses](docs/agent-harnesses.md).

Schedules, file triggers, and webhooks only run while the app is open (including tray / menu bar).

## What it does

- **Workflows as automations** — sequences of inputs → agent steps → outputs. Every workflow can be run manually; scheduling is an optional later trigger for the same automation.
- **Multi-agent** — Claude Code, Cursor, Codex, OpenCode, GitHub Copilot, Gemini, and Grok via local CLIs.
- **Skills** — pin a `SKILL.md` skill on an agent step (invoked as `/skill-name …`).
- **Models** — pick the model/alias each agent should use (`sonnet`, `gpt-5`, `provider/model`, etc.).
- **Triggers** — optional cron schedules, file watchers, and loopback webhooks.
- **Local persistence** — workflows, runs, memories, and schedules in SQLite on disk.

## Core ideas

| Concept | Meaning |
| --- | --- |
| Prompt node | The instruction text fed into the next step |
| Agent node | Which CLI runs, with optional model + skill |
| Choose output | Pick which result continues downstream |
| Run | Manually enqueue/execute the automation |
| Schedule | Optional cron that fires the same runner while the app is open |

## Platforms

Desktop only: **macOS 11+**, **Windows 10/11**, and **Linux**. Android, iOS, and standalone website builds are not supported.

Official binaries are published on the
[GitHub releases page](https://github.com/Nirzhuk/alfred-workflows/releases/latest):
macOS `.dmg` (Apple Silicon and Intel), Windows `.exe`/`.msi`, and Linux
`.deb`/`.rpm`/`.AppImage`. The Windows installer is an **unsigned beta**:
Windows reports an unknown publisher and may show a SmartScreen warning. See
[installing](docs/install.md), [building from source](docs/building-from-source.md),
and the [maintainer release runbook](docs/releasing.md).

## License

Alfred is free software licensed under
[GPL-3.0-or-later](LICENSE), Copyright © 2026 nirzhuk. You may use, modify,
compile, and redistribute it, including commercially, provided you follow the
GPL. Distributed binaries must be accompanied by access to their corresponding
source under the same license.

The GPL does not grant trademark rights or permission to imply that an
unofficial build is an official Alfred release. See
[BRANDING.md](BRANDING.md) and the
[open-source and distribution policy](docs/open-source.md).

## Develop

```bash
bun install
bun run dev      # desktop app (Tauri)
bun run build    # desktop installers for the current OS
bun run check    # frontend tests/build + Rust tests
```

Frontend Vite is started automatically by Tauri (`dev:frontend` / `build:frontend`). Do not use Vite alone as a website.

To inspect startup timing, run `ALFRED_STARTUP_TRACE=1 bun run dev`. The terminal
reports native setup, window reveal, and background runtime verification times.
Bundled runtime checks run in the background; schedules and event triggers start
after those checks finish.

The complete platform prerequisites and expected artifact paths are in
[docs/building-from-source.md](docs/building-from-source.md).

### Stack

- **UI** — React + React Flow (`@xyflow/react`)
- **Shell** — Tauri 2
- **Data** — SQLite (`rusqlite`) in the Rust backend
- **Execution** — Rust agent adapters that spawn local CLIs

## Contributing

Issues and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md),
follow the [Code of Conduct](CODE_OF_CONDUCT.md), and report vulnerabilities as
described in [SECURITY.md](SECURITY.md).

Official binaries publish automatically when a `vX.Y.Z` tag is pushed; the
runbook is [docs/releasing.md](docs/releasing.md) and the current checklist is
[docs/release-todo.md](docs/release-todo.md).
