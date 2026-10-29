# Open-source and distribution policy

Alfred is **free and open source software** under the
[GNU General Public License, version 3 or later](../LICENSE). Nobody is ever
required to pay to use, build, or redistribute it. Official maintainer-built
installers and a build you compile yourself are both free; the only
difference is that an official build unlocks its automation perks with an
optional one-time supporter licence, while a self-built Alfred has every
feature enabled by design.

## Supporting Alfred

Payment is optional and buys nothing you cannot already have by compiling
Alfred yourself:

- A one-time **Alfred Supporter** licence permanently enables cron
  **schedules** and file-watch/webhook **triggers** in official builds. It
  never expires, and nothing a supporter paid for is ever removed later.
- A source build includes schedules and triggers unlocked from the start,
  with no licence entry and no nagging. Under the GPL the gate is a switch,
  not enforcement.
- Manual workflow runs, your data, export, and history stay free in every
  build.

<!-- TODO: supporter checkout link (`VITE_POLAR_DESKTOP_CHECKOUT_URL`) once configured -->

## What you may do

The complete source code in this repository is GPL-3.0-or-later. You may use,
inspect, modify, compile, and redistribute it under that license, including
commercially. Someone may charge for an unofficial copy or fork, but they must
preserve the recipients' GPL rights and provide the corresponding source. They
may not present that copy as an official Alfred release.

The AI agent tools that Alfred launches are separate products. Their
providers may require their own subscriptions, accounts, or usage payments.

## Official builds

Official installers are published as public assets on this repository's
[GitHub releases page](https://github.com/Nirzhuk/alfred-workflows/releases/latest)
and built by [`.github/workflows/release.yml`](../.github/workflows/release.yml)
from the tagged source commit:

- **macOS** — `…_macOS-Apple-Silicon.dmg` and `…_macOS-Intel.dmg`, Developer
  ID signed, notarized, and stapled.
- **Windows** — `…_Windows-setup.exe` (NSIS). This is an **unsigned beta**:
  Windows reports an unknown publisher and SmartScreen may warn. It is never
  advertised as signed or warning-free.
- **Linux** — `…_Linux.AppImage`.

Every release attaches `SHA256SUMS.txt`; verify your download against it.

Official builds from 1.2.0 on update themselves from the signed `latest.json`
on the newest GitHub release, after asking. Source builds update manually.

Official binaries are a convenience, not a capability upgrade over source:
they save you a Rust/Tauri toolchain and come from a CI pipeline that
smoke-tests each installer on clean runners. They do not include a support
contract, an assurance that local modifications will interoperate with future
official builds, or any feature beyond what the tagged source contains.

Alfred ships **no** commerce backend: no payment gateway, account service,
webhook receiver, email service, server-side database, or server backups. Your
workflows, runs, schedules, memories, and files never leave your machine
through Alfred's own code.

## Unofficial builds

A self-built or third-party build must not claim to be an official release.
Personal builds of an unmodified checkout may keep the default name and icon.
If you distribute a modified build, make the changes clear and follow
[BRANDING.md](../BRANDING.md), including renaming it when the presentation
could otherwise confuse users about its origin.

Never copy signing certificates or other maintainer secrets into a source
build. They are not required to compile Alfred, and no such secret ships in an
official build either.

## Contributions

Contributions are accepted under GPL-3.0-or-later. See
[CONTRIBUTING.md](../CONTRIBUTING.md) for the development workflow and
[SECURITY.md](../SECURITY.md) for private vulnerability reporting.

## Policy changes

The license on an existing version cannot be retroactively withdrawn. A future
version may change its distribution or support terms, but previously published
GPL versions retain their existing rights.
