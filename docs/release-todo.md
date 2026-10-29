# Alfred public-release TODO

**Status:** pipeline ready — owner follow-ups below.
**Model:** Alfred is free and open source (GPL-3.0-or-later). Official
maintainer-built installers are published as **public GitHub Release assets**
by [`.github/workflows/release.yml`](../.github/workflows/release.yml) when a
`vX.Y.Z` tag is pushed. macOS builds are Developer ID signed, notarized, and
stapled; Windows ships as an explicitly unsigned beta. There is no automatic
updater and no paid download boundary anymore.

The operator runbook lives in [releasing.md](releasing.md).

## What already works

- [x] Tag-push (`v*`) and manual-dispatch triggers; version lockstep gate
  across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`,
  plus tag-equals-version check.
- [x] Draft staging → clean-runner installer gates → checksum attachment →
  auto-publish. A failed gate leaves the release private.
- [x] macOS DMGs signed/notarized/stapled on both architectures (verified
  2026-08-13, run 31695713076; the flow is unchanged).
- [x] Windows NSIS EXE verified by CI: unsigned, silent install, two launches,
  uninstall.
- [x] `SHA256SUMS.txt` + acceptance manifest (JSON/txt) produced per run.
- [x] In-app **Help → Check for Updates…** installs from the signed
  `latest.json` and falls back to the GitHub releases page
  (`src/features/licensing/download-latest.ts`).
- [x] All six Apple signing secrets present in GitHub; workflow permissions
  set to read/write (verified via API).

## Before pushing the first public tag

- [ ] Confirm the repository stays `Nirzhuk/alfred-workflows`. The releases
  URL is baked into released binaries; renaming the repo breaks the in-app
  Download Latest link for every shipped copy.
- [x] First version number: **`1.0.0`** (bumped from `0.5.0` across
  `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`).
- [ ] Optional dry run: **Actions → release → Run workflow** from `main`,
  confirm all legs green and the draft looks right, then delete that draft.
- [ ] Write release notes for the first public release: supported OS versions,
  required agent CLIs, the unsigned-beta Windows warning, manual-update policy,
  known limitations, GPL notice with a link to the tagged source.

## Owner follow-ups (can trail the first release)

### Website

- [ ] Create the website. Minimum viable: landing page linking the GitHub
  releases page, docs, and source; state the license and the unsigned-beta
  Windows warning. The desktop app must stay Tauri-wrapped — do not turn Vite
  into a hosted site (see `scripts/guard-desktop-tauri.mjs`).

### Prod Polar configuration — decided 2026-08-25

Polar's remaining purpose is now decided: sell the optional one-time
**Alfred Supporter** licence whose perks are cron schedules and
file/webhook triggers. Perks are permanent and licence keys carry **no
expiry**; official binaries stay free and public, and nobody is required to
pay. Configure exactly this, nothing more:

- [ ] Approve the supporter price, then create **one** one-time product in
  prod Polar named **Alfred Supporter**.
- [ ] Attach a license-key benefit **without expiry** to that product (a
  recorded expiry is a configuration failure under the perpetual model).
- [ ] Create the hosted checkout link and wire it to
  `VITE_POLAR_DESKTOP_CHECKOUT_URL` (see the public-links seam in
  `src/features/licensing/public-links.ts`).
- [ ] Create/confirm the customer portal URL for
  `VITE_POLAR_CUSTOMER_PORTAL_URL`.
- [ ] Remove or fulfill every remaining Polar reference outside
  `plans/release-money/` (historical archive) so docs match reality.

### In-app licensing subsystem (supporter licences)

- [x] Decided 2026-08-25: `src/features/licensing/` and
  `src-tauri/src/licensing/` stay and serve the Alfred Supporter licence.
  Activation UI, key storage, and validation exist and are tested; the
  approved pro capabilities (`schedules`, `triggers`) are being wired to the
  entitlement resolver by Plan 008.
- [ ] After that wiring lands and is verified: confirm the in-app licence UI,
  checkout link, and portal link behave in a distribution build with and
  without a supporter key, with no nagging in source builds.

### Windows smoke test (gaming PC)

CI already proves silent install/launch/uninstall. On real hardware, check:

- [ ] SmartScreen warning appears as documented and the installer runs after
  "More info → Run anyway".
- [ ] Launch from the **Start menu** (not terminal): agent CLIs are detected.
- [ ] One real workflow end-to-end; output selection, history, and SQLite
  persistence survive restart.
- [ ] Tray/menu-bar behavior, a schedule, a file trigger, and the loopback
  webhook while running.
- [ ] Failure UX for a missing/unauthenticated CLI is acceptable.
- [ ] Upgrade-over-install keeps user data; uninstall behavior understood.

### Unlocked options (each needs its own decision)

- [ ] Automatic updater: shipped in 1.2.0. Verify the first real update
  (1.2.0 → 1.2.1) on macOS, Windows, and the AppImage.
- [ ] Homebrew cask: blocked before only because public binaries would bypass
  the paywall; that reason is gone. Cask would need the repo layout it expects.
- [ ] Promote Linux packages from informational artifacts to advertised
  downloads once someone has actually exercised them.
