Official Alfred builds for v__VERSION__. Free for everyone under GPL-3.0-or-later; the exact source for these binaries is the `v__VERSION__` tag in this repository.

### Which file do I download?

| Your computer | Download |
| --- | --- |
| Mac with Apple Silicon (M1, M2, M3, M4 — 2020 and newer) | [Alfred___VERSION___macOS-Apple-Silicon.dmg](https://github.com/Nirzhuk/alfred/releases/download/v__VERSION__/Alfred___VERSION___macOS-Apple-Silicon.dmg) |
| Mac with Intel (older Macs) | [Alfred___VERSION___macOS-Intel.dmg](https://github.com/Nirzhuk/alfred/releases/download/v__VERSION__/Alfred___VERSION___macOS-Intel.dmg) |
| Windows 10/11 | [Alfred___VERSION___Windows-setup.exe](https://github.com/Nirzhuk/alfred/releases/download/v__VERSION__/Alfred___VERSION___Windows-setup.exe) |
| Linux (x86_64) | [Alfred___VERSION___Linux.AppImage](https://github.com/Nirzhuk/alfred/releases/download/v__VERSION__/Alfred___VERSION___Linux.AppImage) |

Not sure which Mac you have? Apple menu → About This Mac: "Chip: Apple M…" means Apple Silicon, "Processor: Intel" means Intel. The `.app.tar.gz` files and `latest.json` are for Alfred's built-in updater; you don't need them.

**macOS:** Developer ID signed, notarized, and stapled — Gatekeeper opens them without warnings.

**Windows:** These are **unsigned beta** builds: Windows reports an unknown publisher and SmartScreen may warn before running. Verify the SHA-256 checksum against `SHA256SUMS.txt` before continuing.

Alfred shells out to local agent CLIs (Claude Code, Cursor, Codex, OpenCode, GitHub Copilot, Gemini CLI, Grok Build). Install and authenticate those on the machine before running workflows.

Alfred 1.2.0 and later update themselves: **Help -> Check for Updates...** (also checked at launch). Older installs update by downloading from this page.
