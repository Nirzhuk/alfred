# Releasing Alfred

Alfred is free and open source under GPL-3.0-or-later. Official
maintainer-built binaries are published as **public GitHub Release assets** on
this repository — anyone can download them, and anyone can compile the source
instead. This document is the maintainer runbook for cutting a release.

The launch-gate and follow-up checklist lives in
[release-todo.md](release-todo.md).

## How a release ships

[`.github/workflows/release.yml`](../.github/workflows/release.yml) does the
work:

1. Builds all four platform legs: macOS Apple Silicon, macOS Intel,
   Windows x64 (NSIS), and Linux (`AppImage`), one installer each, named
   by `releaseAssetNamePattern` with the matrix `label`.
2. Stages every artifact on a **draft** release tagged `v<version>`.
3. Runs clean-runner gates before publication:
   - Both DMGs are downloaded from the draft, verified (`hdiutil`,
     `codesign --verify --deep --strict`, `spctl`, `stapler validate`),
     installed on native runners, launched twice, and removed.
   - The NSIS EXE is downloaded, confirmed **unsigned**, silently installed,
     launched twice, and uninstalled.
   - `verify-version` fails the run if `package.json`,
     `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` disagree, or if a
     pushed tag does not equal `v<version>`.
4. The `publish-release` job attaches `SHA256SUMS.txt` to the release, uploads
   an acceptance manifest artifact (version, source commit, filenames, sizes,
   architectures, signing status, SHA-256 — no secrets), and only then flips
   the draft public and marks it latest.

If any gate fails, the release stays a private draft: nothing broken ever goes
public by accident.

## Cut a release

1. Bump `version` in lockstep:
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`

   `verify-version` fails the run if these three disagree with each other or
   with the pushed tag.
2. Merge that commit to `main`.
3. Tag and push:

   ```bash
   git tag vX.Y.Z <commit>
   git push origin vX.Y.Z
   ```

   A manual **Actions → release → Run workflow** dispatch also exists for
   test builds from any branch; it stages a draft but still requires the
   three versions to agree. Prefer a real tag for anything you intend to keep.
4. Wait for the workflow. Watch the gates; a red gate means the release did
   **not** publish. Fix, bump or retag, rerun.
5. After publication, spot-check `https://github.com/Nirzhuk/alfred-workflows/releases/tag/v<version>`:
   - `Alfred_<VERSION>_macOS-Apple-Silicon.dmg` and `Alfred_<VERSION>_macOS-Intel.dmg`
   - `Alfred_<VERSION>_Windows-setup.exe` and `Alfred_<VERSION>_Linux.AppImage`
   - `latest.json` plus the two `.app.tar.gz` updater bundles (no `.sig`
     files: the signatures are inside `latest.json`)
   - `SHA256SUMS.txt` matching the acceptance manifest in the run artifacts.
6. Write the release notes body (or edit what tauri-action pre-filled).

## Rollback

Disable the broken release instead of hiding history:

```bash
gh release edit vX.Y.Z --latest=false      # unmark latest
gh release delete-asset vX.Y.Z <asset>     # pull a bad artifact
gh release delete vX.Y.Z --yes && git push origin :refs/tags/vX.Y.Z
```

Then fix, retag, and rerun. Installed copies that already updated keep the
bad version until a higher fixed version is published.

## Signing

### macOS — Developer ID + notarization

Both disk images are signed, notarized, and stapled by CI. Gatekeeper opens
them without warnings.

### Windows — Authenticode waived

For the 2026-08-13 release decision, Windows signing is explicitly waived due
to budget. Every Windows artifact is an **unsigned beta**: Windows reports an
unknown publisher and SmartScreen warns. Label it that way wherever it is
offered. Adding signing later means a certificate-store flow in Tauri
(`certificateThumbprint`, SHA-256 digest, timestamp URL), matching CI secrets,
and removing the unsigned-beta labeling everywhere in the same change.

## Automatic updater

Installed apps check
`releases/latest/download/latest.json` at launch and from **Help → Check for
Updates…** (also in the tray menu). Drafts are invisible to it, so an update
only reaches users once the release is published.

- `src-tauri/tauri.release.conf.json` turns on `createUpdaterArtifacts` for
  release builds only, so source builds need no key.
- `tauri-action` signs each updater artifact with the
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets
  and uploads `latest.json` (`uploadUpdaterJson: true`).
- The matching public key is `plugins.updater.pubkey` in
  `src-tauri/tauri.conf.json`.
- **Never lose the private key.** Without it no installed copy can accept a
  new update; changing keys means every user reinstalls by hand once.
- Rollback: unpublish or un-latest the bad release (see Rollback) and publish
  a fixed higher version; the updater never downgrades.

`ALFRED_RELEASE_DATE` is intentionally left unset in release builds: an unset
value means the update-window logic never locks anything, which matches the
free distribution. Revisit only if a time-boxed entitlement model returns.

## GitHub settings

Settings → Actions → General → Workflow permissions → **Read and write
permissions** (the publish job edits the release; the gates read draft assets).

## Secrets

The macOS jobs fail when these values are absent. Windows artifacts remain
unsigned by the waiver above.

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded `.p12` export of **Developer ID Application** |
| `APPLE_CERTIFICATE_PASSWORD` | Password for that `.p12` |
| `KEYCHAIN_PASSWORD` | Any strong password for the ephemeral CI keychain |
| `APPLE_ID` | Apple ID email used for notarization |
| `APPLE_PASSWORD` | App-specific password (not the account password) |
| `APPLE_TEAM_ID` | 10-character Team ID |

Export the cert locally:

```bash
base64 -i DeveloperID.p12 | pbcopy   # macOS
```

## Local smoke build

```bash
bun install
bun run build
```

Produces installers for the **current** OS only under
`src-tauri/target/release/bundle/`. They use no signing identity and are not
official builds.
