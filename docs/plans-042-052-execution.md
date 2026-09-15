# Plans 042 and 045–052 execution

Work began at desktop commit `85f0d64e23e55e655c9b6939aef802c5eb970ae2`
with existing user changes. Each of the nine requested plans has a dedicated
implementation agent. Implementation uses Luna/Sol where practical; independent
review uses high effort, and computer-use QA uses Luna at extra-high effort.

Implementation remains in progress. A passing focused checkpoint does not waive
the shared integration, packaged-app, or live-runtime gates. This ledger
separates accepted local implementation evidence from unpassed current checks
and from historical findings that have since been repaired.

## Memory status check, 2026-09-15 (implementation state)

Plans 045 and 047–051 are DONE. Plans 046 and 052 are complete locally and
remain BLOCKED only on live activation. This checkpoint supersedes the older
remaining-work estimates below; it does not change Plan 042's release status.

- Plans 045, 047, 048, 049, and 050 have their local implementations, accepted
  focused/review/UI evidence, frozen reports where applicable, and a green
  shared full check on 2026-09-15.
- Plan 046's restricted reviewer contract, fake transport/conformance, and
  scoped UI are implemented and accepted locally. Live activation is
  `BLOCKED` pending the exact restricted runtime, managed-policy assurance, an
  already-authorized account, and the authorized synthetic adversarial live
  check. Conformance fixtures do not enable production review.
- Plan 051's backend save, immutable execution-time Output disposition, unique
  receipts, storage-only retry, deletion hooks, startup recovery snapshot,
  bounded backlog drain, recent `Saving` recovery/fencing, promotion-safe
  cleanup, and frontend cutover are implemented and locally reviewed. Both
  History repairs are present. The older recovery and cleanup findings are
  superseded by the current implementation; the shared full check passed on
  2026-09-15.
- Plan 052's separate default-off workflow permission, bounded canonical chunk
  reads, scoped service, native allowance/terminal-event/trace boundary,
  fake-runtime conformance, settings UI, and lookup evaluator are implemented
  and locally reviewed. Production activation is `BLOCKED`: the release-enabled
  provider list and production native registry remain empty, and the installed
  tree has no release-eligible host-owned custom-tool transport with the
  required trusted context allowance and authorized live proof.

### Historical source findings retained for auditability

The following findings came from source inspection rather than new runtime
reproductions. They are retained as historical evidence and are not current
open defects:

- Plan 051 previously appeared to run one 32-row startup batch, recover only
  old `Saving` claims, and clean promoted artifacts through origin-run
  ownership. Current code snapshots pending/`Saving` claims before setup,
  drains bounded batches until exhaustion, fences recent `Saving` recovery,
  and cleans from the actual deleted-memory set.
- Plan 052 previously appeared to lack separate permission, canonical chunk
  reads, native allowance/terminal events, durable traces, UI, and evaluation.
  Current code supplies those local contracts and fixtures; production
  advertisement and live provider activation remain deliberately unavailable.

## Current verification

- The final 2026-09-15 `bun run check` passed 548 frontend tests, the frontend
  build, and 1,072 Rust tests with 0 failures; `git diff --check` is clean.
  Closing repairs: a memory runner test used a pre-UUID artifact file name that
  the trusted-path check correctly rejects; the OpenCode fake sidecar accepted
  sockets that inherited nonblocking mode on macOS, so provider tests failed
  intermittently under parallel load (0/20 failures after the fix); and an
  integration review fixed per-turn lookup serialization, duplicate-turn
  teardown, byte refunds for rejected lookup results, and legacy artifact
  indexing starvation, with regression tests. No assertion was weakened.
- An earlier 2026-09-15 attempt passed 547 frontend tests and the frontend
  build, then failed Rust compilation because four existing runner test calls
  lacked the new optional Output disposition and two receipt/deletion queries
  retained temporary borrows past prepared statements. Those six compilation
  sites were fixed before the later checkpoints.
- A prior historical 842-pass/24-fail Rust checkpoint recorded 23 subprocess
  startup/runtime fixture failures and one review queue-bound failure. It
  is superseded by the green 2026-09-15 check.
- Browser QA covered all 24 synthetic-IPC cases (12 lookup, 8 replay, and 4
  History) in both themes at 1360x860 and 960x640, including Space/Enter
  keyboard activation. This verifies the browser surface only; it does not
  prove provider access or lookup usefulness.
- An isolated native Tauri debug-bundle startup/backend smoke passed exit 0 via
  `bun run build --debug --bundles app --config uniqueQA`, using bundle ID
  `com.nirzhuk.alfred.qa.101A09A0CA10` and a uniqueQA-only URL scheme. With a
  temporary HOME, allowlisted child environment, and no user-database/account
  reads, Boot 1 initialized real SQLite with 65 Pending, one current-time
  Saving, Failed, and Deleted receipts; Boot 2 recovered 66 Saved receipts,
  verified exact inline/large bodies and canonical artifact/manifest digest,
  state, and length, and left Failed/Deleted unchanged. This proves native
  startup/backend recovery only, not WKWebView visual interaction, lookup
  transport, or provider activation.
- No user database, provider account, production native provider activation,
  deployment, or publication was used. The production enabled-provider list
  remains empty.

| Plan | Current state |
| --- | --- |
| 042 | Local high review, scoped browser UI, actual public preview, and Worker/D1 checks accepted. Final shared checks and packaged/hosted release gates remain. |
| 045 | DONE. Fixed-label evaluator and historical baseline accepted and frozen; full check green 2026-09-15. |
| 046 | Restricted contract/conformance and scoped UI accepted; full check green 2026-09-15. Live activation is BLOCKED by the documented runtime/policy/account gates. |
| 047 | DONE. High review and scoped browser QA accepted; full check green 2026-09-15. |
| 048 | DONE. High review, scoped browser QA, and final-composition report accepted; full check green 2026-09-15. |
| 049 | DONE. Final backend high review and both-theme focused UI accepted; full check green 2026-09-15. |
| 050 | DONE. Backend high review, both-theme correction UI, and the 21/21 frozen evaluation with repeat determinism accepted; full check green 2026-09-15. |
| 051 | DONE. Backend/frontend implementation, startup recovery/backlog repairs, promotion-safe cleanup, both History repairs, and isolated native startup/backend smoke accepted locally; full check green 2026-09-15. |
| 052 | Bounded service/native/UI/evaluation implementation accepted locally; integration review fixes landed and full check green 2026-09-15. Native startup proof does not establish lookup transport; production provider activation is BLOCKED by release transport, trusted allowance/context, and authorized live-proof gates. |

## Verification

### Current combined checkpoint

The final 2026-09-15 `bun run check` passed 548 frontend tests, the frontend
build, and 1,072 Rust tests with 0 failures, and `git diff --check` is clean.
Browser QA completed 24/24 synthetic-IPC cases (12 lookup, 8 replay, and 4
History) in both themes at 1360x860 and 960x640, including Space/Enter
keyboard activation. This verifies the browser surface only; it does not prove
provider access or lookup usefulness.

### Accepted local evidence

- **042:** 11 sharing command/journey tests, 11 import database tests, 10
  setup-model tests, 14 service tests, service TypeScript, and 9 actual
  Worker/D1 scenarios passed. Verified Worker bundle SHA-256:
  `ae6888af8413d9c198d6604e74f4663c6bac8bdd41b30b5be0ebb17b3b4667ee`.
- **045:** 50 synthetic cases remain split 30 development/20 validation with a
  frozen reproducible v1 report. Six evaluator tests and the injected review
  lifecycle test passed; relevance misses remain visible rather than relabeled.
- **046:** process restrictions and dispatcher callback/reservation/failure paths
  passed high review; scoped save and failed-save rollback UI passed. This
  remains contract/conformance evidence only while live activation is blocked.
- **047/048:** production query origins, retrieval eligibility, prompt
  composition, UTF-8 budgets, unavailable memory, and audit summaries passed
  review. The 60-case composition report has zero hard violations and at most
  10,967 measured memory bytes.
- **049:** final scoped review accepted step-linked evidence capture, durable
  canonical qualification, and prior-memory reference tracing. The final broad
  memory checkpoint passed 160 tests; focused UI passed both themes.
- **050:** independent re-review closed all five backend findings. Eight
  conflict, 12 atomic approval, old-profile bootstrap, and date-evidence
  regressions passed. The manual editor hydrates by semantic revision even when
  timestamps match, while retaining the original version during a dirty edit.
- **051:** 31 focused tests cover attempt fencing, deletion, bounded recovery,
  artifact ownership, canonical writes, absent/failing event sinks, and
  distinct large outputs from separate runs. Seven receipt component/model tests
  with 50 assertions also passed independent review.

- **Native startup/backend smoke:** an isolated `uniqueQA` debug bundle passed
  exit 0 with temporary HOME and allowlisted child environment. Real SQLite
  Boot 1 seeded 65 Pending, one current-time Saving, Failed, and Deleted
  receipts; Boot 2 recovered 66 Saved receipts, verified exact inline/large
  bodies and canonical artifact/manifest digest/state/length, and left
  Failed/Deleted unchanged. This is not WKWebView visual or provider/lookup
  transport acceptance.
- **052:** 19 core service tests and high review accepted output-only historical
  sources, per-call clocks, metadata/page worker deadlines, SQLite lock/deadline
  handling, safe worker handoff, and the implemented native/UI/evaluator
  conformance boundary. This remains fake/local acceptance; no production
  provider is registered.

### Historical checkpoints and failures

The starting audit baseline was 494 frontend tests, 829 Rust tests, and the
frontend build. An earlier 2026-09-15 combined checkpoint passed 547 frontend
tests, 1,046 Rust tests, and the frontend build after six compilation-site
repairs. Its first attempt failed Rust compilation because four existing runner
test calls lacked the new optional Output disposition and two receipt/deletion
queries retained temporary borrows past prepared statements; those six sites
were fixed. A prior 842-pass/24-fail Rust run recorded 23 subprocess
startup/runtime fixture failures and one review queue-bound failure. These
historical results remain labeled and are not substituted for the latest
unpassed checkpoint.

See [review decisions](plans-042-052-review-log.md) for exact findings and logs.
Earlier attempts and corrective runs are preserved in the
[progress archive](plans-042-052-progress-2026-09-09.md).

## Frozen evaluation inputs

The seven accepted inputs and reports under `tests/fixtures/memory-quality/`
remain immutable. The five earlier files are:

| File | SHA-256 |
| --- | --- |
| `v1.json` | `f19b5e3ef701b31aaaed9aae8ab55f80d1a305a286b6c7e43c468bcc9e063d8e` |
| `baseline-v1.json` | `35d6bad040de6be65e36a5e8969a18f532865db0b86403053cd3b979cb32edfb` |
| `baseline-v2.json` | `23d66a22633225a5805b9b804bfe5d211d3535535763bb21dc93851e777c9495` |
| `composition-v1.json` | `8f9b0807258986793c4b748c00a291ad56b66a251d88ebcbe0bcc1fcdfdf318f` |
| `baseline-composition-v1.json` | `8d9a27d7d1ca6dd6d3a9d4963c88f9168e651dab4fcf830725d37da78de8413a` |

Root authored 21 fixed correction cases before 050 integration in
`correction-v1.json`, SHA-256
`c370db4160f7f33cb3bd86f344b87b4fa7e61b7c8e793ebc47490ee50407abf9`.
The real DB evaluator now runs captured evidence, reviewed target discovery,
candidate finalization, and fixed-clock approval against those labels. All 21
cases passed, including repeated fresh-DB equality and independent high review.
The new frozen `baseline-correction-v1.json` has SHA-256
`812f10089d579a12336452707aeef4847dc640b4831d3b392512b14a67410834`.
Labels come from product rules, not a similarity implementation. See the
[correction evaluation](memory-correction-evaluation.md) for scope and commands.

Historical v1 recall/precision: 39/40 and 40/80, with three unexpected nonempty
results. Automatic v2: 35/40 and 36/41, with known misses retained. Composition:
36/40 and 37/42 including deliberate selections; this is not an automatic-recall
improvement claim.

## UI evidence

- [042 sharing and actual public preview](plans-042-ui-results.md)
- [046 reviewer settings](plans-046-ui-results.md)
- [047/048 selection, library, and History](plans-047-048-ui-results.md)
- [049 evidence UI and completed fix retests](plans-049-ui-results.md)
- [050 correction, concurrent edit, and date UI](plans-050-ui-results.md)
- [051/052 output persistence, lookup settings, and History](plans-051-ui-results.md)
- [Remaining checklist and fixtures](plans-042-052-ui-checklist.md)

Root independently inspected representative screenshots before accepting scoped
UI results. Component fixtures use synthetic IPC; public preview QA uses the actual
local service renderer. These UI checks do not prove native Tauri transport,
provider access, or real user-data persistence. A separate native startup/backend
smoke is recorded in the verification section above; it does not establish
WKWebView visual interaction or lookup transport. The app is Tauri React/macOS
WKWebView, not React Native. QA uses isolated Chrome 152 through Argent and Vite
port 1422; the user app at port 1420 and live database are untouched.

## Release and runtime limits

Sharing is configured for `https://alfred.nirzhuk.dev`; publishing stays disabled.
[Release preparation](workflow-sharing-release.md) records expired Wrangler
authentication and the historical HTTP 526 origin check. No deployment, DNS,
route, or account changes were made. Hosted validation, account bindings,
retention operations, and packaged cold/warm links remain release gates.
[Review provider support](memory-review-provider-support.md) records Claude Code
`2.1.271` on the inspected machine versus the exact `2.1.263` allowlisted
fixture, and no administrator-controlled assurance that managed policy disables
all extensions (`--safe-mode` still permits managed executable hooks). Plan 046
therefore remains blocked until a verified restricted runtime, managed-policy
assurance, already-authorized account, and explicit synthetic adversarial live
check are available.

[Lookup feasibility](agent-memory-lookup-feasibility.md) found no currently
release-supported native runtime with the required host-owned custom-tool
callback and result path. The local service/fake boundary may proceed, but the
release enabled-provider list and production registry remain empty; adapter
advertisement and live acceptance remain blocked.

Plan 052's activation gate requires every item below, not merely a declared enum
or fake conformance fixture:

1. The release manifest enables the provider/harness for this capability.
2. Production registers the provider and its host-owned custom-tool transport.
3. Its descriptor and permission advertise `MemoryLookup` independently of
   unrelated denied capabilities.
4. The runtime accepts Alfred's declaration, delivers bounded arguments, and
   accepts the bounded result shape.
5. The callback binds trusted workflow/run/node context; arguments, cwd, and
   model-provided IDs carry no authority.
6. A verified remaining-context allowance is reserved from the shared per-turn
   ledger across all lookup traffic.
7. Production persists source/version/page and accepted-delivery traces.
8. An authorized account completes a bounded synthetic search/get smoke,
   including cancellation, revocation, and trace evidence.

Accordingly, core fixtures can proceed locally; production advertisement and live
acceptance remain blocked.

## Remaining order

1. Main completes repair and triage of the current Rust checkpoint, preserving
   assertions and recording exact causes for the 3 failures. Reconcile any
   remaining source-review or scope findings without weakening tests.
2. Rerun the combined checks on the repaired tree, then confirm fixture
   integrity/hashes, targeted plan gates, and `git diff --check`. The 24/24
   browser evidence and the scoped native startup/backend result are recorded
   above; any additional native desktop result must be recorded separately.
3. Keep Plan 046 live activation BLOCKED until the exact restricted runtime,
   managed-policy assurance, authorized account, and adversarial synthetic live
   acceptance gates pass.
4. Keep Plan 052 provider activation BLOCKED until a release-eligible,
   host-owned custom-tool transport binds trusted context, shares a verified
   remaining-context allowance, persists delivery traces, and passes authorized
   live search/get/cancellation/revocation smoke evidence.
5. Only after those applicable checks pass may Main update DONE/full-green
   language. Fake conformance, evaluator results, and synthetic IPC never
   activate a provider.

Shared schema/runner/canonical-writer edits are serialized. Ordinary tests use
synthetic in-memory DBs and temporary artifacts. No live provider calls, private
database migration, commit, push, or publication is part of these local checks.
Initial dirty-tree backups remain at `/tmp/alfred-before-nine-plans*`, and the
complete starting source is at `/tmp/alfred-plans-baseline.kKbPiV`.

### Historical ownership note

The user reported another session editing `db/agent_memory_lookup.rs` on
2026-09-10, so this team's 052 owner paused. A project-scoped session-log check
subsequently traced the questioned `StallingMetadataLookupFileReader` helper to
this team's 052 agent at 01:37:40 UTC. The other lookup-related session found in
the available logs had completed. At 03:39 UTC, the file had been unchanged for
about 108 minutes, with SHA-256
`69bcb6f14ab71af0c54d8e53a467561a5fdeda6e8ea4a4b50bee02664aac48c9`.
This retained note records the preserved integration baseline; it does not
prove every external session idle or override the current implementation state
above. The accepted 19-test core is historical evidence; shared integration and
final combined verification remain required.
