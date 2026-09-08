# Maintenance audit

Audited on 2026-09-08 against `5b10719` and the existing working tree. The scope
was behavior-preserving maintenance of the desktop app, weighted toward saved
data, asynchronous state, scheduling, and process execution. No dependencies,
database schema, product features, styling, or licensing rules were changed by
this audit. Unrelated work already in progress was preserved.

## Fixes

| Problem | Change | Regression coverage |
| --- | --- | --- |
| A workflow save marked edits made during that save, or on a different tab, as saved. Overlapping writes could finish out of order. | Serialize saves per workflow and only clear dirty state for the snapshot actually saved. | Delayed saves, node and edge edits, tab changes, overlapping writes, and recovery after a failed write. |
| Run continued after saving the edited workflow failed. | Return the save result and stop before starting the persisted graph when saving fails. | Failed save never invokes the run command; successful save still runs. |
| One busy or broken schedule prevented later schedules from being checked. | Handle failures per schedule, retain failed starts for retry, and report the number of successful starts. | Busy workflow followed by a ready one; invalid cron followed by a valid schedule; missing and future timestamps. |
| Writing a large stdin payload could block process supervision before timeout checks and output consumption began. | Write stdin on a separate thread while the supervisor drains bounded output and checks cancellation and deadlines. | Large bidirectional input/output, a child that never reads stdin, cancellation, and write failures. |
| Clearing a memory date sent JSON null, but the backend interpreted it as an omitted field. | Distinguish missing, null, and string values for nullable memory patches. | Actual JSON patches replace, retain, and clear dates and superseded-memory references. |
| Artifact writes reused a filename derived from a supplied memory ID and overwrote live content before SQLite committed. A failed insertion could then delete the original file. | Allocate independent UUID filenames with exclusive creation. Commit the new path before removing the old file; remove only the new file on failure. Require an existing workflow context before writing files. | Duplicate creation and forced update failures preserve original file, database row, and search index. Invalid workflow contexts never reach the file writer. Existing artifact paths still work. |
| Editing artifact metadata submitted its shortened preview as replacement content. | Omit an unchanged body from the editor's update payload. | Backend metadata-only updates preserve the complete artifact byte for byte. |
| Legacy memory migration removed the old copy even after failed imports. | Retain failed records for retry, including when other memories have already reached SQLite. | Partial failures, later retries, and concurrent loads. |

Production changes are in `src/features/workflow/store.ts`,
`src/features/workflow/memories.ts`, the memories inspector, and the Rust
`scheduler`, `agents/process`, and `db/memories` modules. Frontend persistence
regressions live in `tests/workflow-persistence.test.ts` and
`tests/legacy-memory-migration.test.ts`; Rust regressions live beside the
affected modules.

## Remaining findings

These are follow-up work, not claims that every path was made safe by this pass.

| Priority | Finding and evidence | Next step | Effort / change risk |
| --- | --- | --- | --- |
| High | `src-tauri/src/runner/mod.rs:141` checks `active::has_workflow` before line 200 calls `active::register`. Those use separate registry lock acquisitions in `src-tauri/src/agents/active.rs:67` and `:100`, so simultaneous manual and trigger starts can both pass the check. | Add atomic workflow admission with rollback on enqueue failure; test concurrent manual, scheduled, and app-event starts. Confirm how pending app-event receipts should behave before changing admission. | Medium / medium |
| High | `src-tauri/src/agents/process.rs:451` and `src-tauri/src/agents/active.rs:28` terminate the direct child. The supervisor then waits for pipe readers at `process.rs:383`. A surviving descendant can retain pipe descriptors and delay completion after a timeout. | Define process-tree ownership on each desktop platform and reuse the existing managed-runtime process-group or Windows Job Object patterns where appropriate. Test descendant cleanup separately from stdin backpressure. | Medium / medium |
| High | `src/features/workflow/components/memories-inspector/memories-inspector.tsx:563` still initializes its editor from `selected.body`, which is a preview for large artifacts. This pass protects unchanged content during metadata edits at line 688; explicitly editing that preview still replaces the body with the submitted text. | Load complete artifact content through a scoped backend read before editing it, with loading, read-error, and stale-selection handling. Do not silently fall back to editing a preview when the file cannot be read. | Medium / medium |
| Medium | `src/features/workflow/store.ts:573` applies old workflow responses to the visible schedule. `src/features/workflow/components/schedule-modal/schedule-modal.tsx:63` resets to defaults when the schedule belongs to another workflow. This was reproduced by resolving workflow B before workflow A. | Track request ownership for schedule load, save, and clear operations. Test stale successes and failures while switching workflows. | Small / low |

These findings have high confidence from the cited code paths. The schedule
response race was also reproduced with mocked Tauri IPC; the concurrency and
descendant-process cases still need dedicated regression fixtures.

The long runner and workflow-store modules deserve smaller modules after the
critical behavior has characterization tests. Splitting files on size alone
would not address the concurrency bugs above, so a broad refactor was deferred.

## Verification and limits

The starting baseline passed 445 frontend tests, TypeScript checking, and 809
Rust tests. `bun audit` reported no vulnerabilities across 156 packages. That
result covers the Bun dependency audit, not all Rust or bundled runtime supply
chains.

Final verification passed:

- `bun run check`: 459 frontend tests, the TypeScript/Vite production build,
  and 824 Rust tests, all passing.
- `bun run test:managed-runtimes`: all 5 offline packaging tests passing.
- `git diff HEAD --check`: no whitespace errors.
- The diff for the 11 files already modified when this audit began was
  compared with the saved starting diff and remained identical. Additional
  unrelated icon work that appeared during the audit was also left alone.

The production frontend build has an existing warning for an application chunk
larger than 500 kB. Rust emits existing unused-code/import warnings. Neither was
silenced or addressed through broad cleanup.

This was a focused source audit, not an exhaustive review of the roughly
108,000-line Rust tree. The sibling `api-licenses` service was inspected only
for workspace context. Provider internals, live OAuth/payment flows, packaged
installers, cross-platform manual UI behavior, release infrastructure, and the
full Rust dependency advisory set were not audited. No real workflow database,
provider account, or saved artifact was used for new regression fixtures.

## Project environment

This snapshot records the environment-inspection result for later maintenance
work. Use the repository manifests and CI configuration as the authority when
they change.

```json
{
  "is_react_native": false,
  "is_native_ios": false,
  "is_native_android": false,
  "platforms": ["macOS", "Windows", "Linux"],
  "stack": ["Tauri 2", "Rust 2021", "rusqlite", "React 19", "TypeScript 5.8", "Vite 7", "Zustand 5"],
  "package_manager": "bun@1.3.8 (declared)",
  "install": "bun install --frozen-lockfile",
  "development": "bun run dev",
  "desktop_build": "bun run build",
  "required_checks": "bun run check",
  "frontend_tests": "bun run test:frontend",
  "frontend_build_and_typecheck": "bun run build:frontend",
  "rust_tests": "cargo test --locked --manifest-path src-tauri/Cargo.toml",
  "managed_runtime_tests": "bun run test:managed-runtimes",
  "ci": ".github/workflows/ci.yml",
  "formatting": "Format touched Rust only; repository-wide cargo fmt is not an existing merge gate.",
  "app_interaction": "Desktop-only Tauri; no mobile or Chromium device was started."
}
```
