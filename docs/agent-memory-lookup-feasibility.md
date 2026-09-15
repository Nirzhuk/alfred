# Agent memory lookup feasibility

Checked on 2026-09-09 against `85f0d64e23e55e655c9b6939aef802c5eb970ae2` and the current dirty working tree. This checkpoint used source and offline fixtures only. It did not inspect the user database, credentials, provider profiles, or make a provider call.

## Decision

`adapter activation blocked`

No Alfred native runtime is release-enabled today. The offline release
fixture has an empty `enabledNativeProviders` list, the production control
plane creates an empty `NativeRuntimeRegistry`, and production installs
`DenyAllToolExecutor` for unrelated tools. No registered provider advertises
`supports_memory_lookup=true`; every native route remains blocked or disabled.

The provider-neutral bounded lookup contract and scoped SQLite service exist,
but they are not native-adapter acceptance evidence. The current native
seams still lack a release-enabled runtime with a trusted Alfred-owned
declaration, argument-delivery callback, result-acceptance path, and
provider/runtime-owned remaining-context allowance. OpenCode V1 exposes only
approval handling and ignores custom-tool events; other declared mappings
remain blocked, unlinked, or fail-closed. A CLI fallback is not an adapter
route.

Stages 4 and 5 therefore remain blocked. No provider call, account or
billing route, package enablement, or live transport/context proof is
present in this checkpoint.

## Drift check

The required commands were run from `desktop/`:

```text
git status --short
git diff --stat 85f0d64..HEAD -- src-tauri/src/db src-tauri/src/agents/native src-tauri/src/runner/mod.rs src-tauri/src/lib.rs src-tauri/src/commands/mod.rs src/features/workflow tests/memory-retrieval.test.ts tests/fixtures/memory-quality docs/agent-memory-lookup-feasibility.md
git diff HEAD -- src-tauri/src/db src-tauri/src/agents/native src-tauri/src/runner/mod.rs src-tauri/src/lib.rs src-tauri/src/commands/mod.rs src/features/workflow tests/memory-retrieval.test.ts tests/fixtures/memory-quality docs/agent-memory-lookup-feasibility.md
```

`HEAD` is still the planned-at commit, so the committed-range stat was empty. The working-tree diff is extensive in the named database, runner, native-provider, workflow, and memory-test paths. Those edits belong to the pending dependency plans and other work. Plan 052's Step 1 changes are limited to this report, the new provider-neutral contract module, and its additive `db/mod.rs` registration.

## What counts as available

Provider code or a descriptor is only a declared route. A usable route must pass every layer below:

1. The release manifest must permit the provider and Alfred harness.
2. The runner must pass that manifest gate.
3. A production path must register the runtime.
4. The runtime must declare host-owned tool execution.
5. The provider transport must accept an Alfred tool declaration, deliver its arguments to the host, and accept the host result.
6. The production router must install a narrow executor that can bind the call to the current run and workflow.

The current release fails the first condition for every native provider. The offline release fixture has an empty `enabledNativeProviders` list at `tests/fixtures/native-harness-release-matrix.json:22-22`. The live manifest builder marks Claude, Cursor, Codex, OpenCode, GitHub Copilot, Gemini, and Grok `Blocked`, and Pi and OMP `Disabled`, at `src-tauri/src/agents/capability_manifest.rs:665-807`. The runner calls `require_execution` before it creates or dispatches an agent request at `src-tauri/src/runner/mod.rs:1146-1158`.

CLI providers are available according to `src-tauri/src/agents/capability_manifest.rs:548-579`, but the CLI path calls the provider adapter directly at `src-tauri/src/agents/mod.rs:557-581`. It does not enter `NativeTurnHost::invoke_tool` and offers no host-owned custom callback. CLI support is therefore irrelevant to this feasibility gate, and a CLI fallback remains out of scope.

## Registry and host boundary

The application creates an empty `NativeRuntimeRegistry` at `src-tauri/src/lib.rs:210-222`. It installs `DenyAllToolExecutor` in the production `NativeExecutionRouter` at `src-tauri/src/lib.rs:226-233`. The executor returns `PermissionDenied` for every request at `src-tauri/src/agents/native/tools.rs:181-195`.

The generic callback exists, but it is not yet a production-scoped memory
callback:

- `NativeTurnHost::invoke_tool` accepts an `AlfredToolRequest` and returns an
  `AlfredToolResult`; the registry admits it only for `AlfredExecuted` owners,
  validates and normalizes the result, emits terminal tool events, and returns
  it.
- `AlfredToolExecutor::execute` receives only the tool request and
  cancellation handle, not the trusted `NativeTurnRequest`.
- `NativeTurnRequest` carries `run_id` and `node_id`, but still lacks
  `workflow_id` and a provider-reported remaining-context allowance.
- Provider-neutral capability, permission, and tool records now include a
  `memory_lookup` field, but every registered descriptor remains false and no
  runtime advertises `supports_memory_lookup=true`.

The generic boundary's 64 KiB argument and 128 KiB output limits are not the
Plan 052 lookup limits. A later native path must enforce the tighter request,
response, page, call-count, deadline, and per-step byte ceilings before and
after serialization.

## Provider route matrix

| Route | Production availability | Callback and result evidence | Finding |
| --- | --- | --- | --- |
| OpenCode Go V1 | The production registry is empty; the release manifest blocks OpenCode, and `opencode::register` remains fail-closed. | The descriptor is `RuntimeExecutedWithHostApproval`; its callback resolves approval requests only, and the V1 event mapper ignores custom-tool events. | No host-owned lookup callback or result route. |
| Claude direct API | Registration remains unavailable until account intake and live smoke; the release manifest blocks Claude. | The fixture descriptor declares `AlfredExecuted` and a host tool/result loop, but it is a blocked adapter declaration. | Technically shaped seam only; no activation evidence. |
| Gemini direct API | Registration remains unavailable until API-key account intake and a paid-project smoke; the release manifest blocks Gemini. | The fixture runtime declares `AlfredExecuted` and maps host results, but no production route is enabled. | Declared callback pattern only; no activation evidence. |
| Grok direct API | No production registration function exists; the release manifest blocks Grok. | Fixture-only descriptor and function-result mapping are not a released transport. | Account- and release-blocked; no route to activate. |
| GitHub Copilot SDK | The SDK/CLI package is unlinked and its transport returns `ProviderUnavailable`; the release manifest blocks Copilot. | A proposed SDK session admits only `custom:alfred_*` tools and defines a result callback, but no linked production transport exists. | Viable-looking seam only; not a production route. |
| Codex app server and Python SDK | App-server registration is fail-closed; the Python SDK has blocked release gates; the release manifest blocks Codex. | No released host-owned memory callback is established; the stable mapper does not provide one. | No candidate callback to activate. |
| Cursor Cloud Agents | Cursor does not implement or register `NativeAgentRuntime`; the release manifest blocks it. | The public Cloud Agents route has no per-tool approval callback. | No host-owned tool-result path. |
| Pi and OMP | Both are disabled as not implemented. | No native provider implementation exists. | No route to inspect or activate. |

## Exact activation scope

None. This checkpoint does not add a provider adapter, release flag, account,
billing route, or CLI fallback to Plan 052.

Activation remains gated on every item below:

1. The release manifest must enable the provider and Alfred harness.
2. A production registration path must bind the selected runtime.
3. The descriptor and permission profile must advertise memory lookup
   independently, with unrelated tools still denied.
4. The runtime must accept an Alfred tool declaration, deliver its arguments
   to the host, and accept the bounded host result.
5. The callback must bind the trusted workflow/run/node context; model input,
   working directory, or caller-supplied IDs cannot establish scope.
6. The runtime must provide a verified remaining-context allowance and reserve
   lookup output against all turn traffic; static byte caps are insufficient.
7. Production must persist source, version, page, and accepted-delivery traces.
8. An already-authorized account must pass an explicitly authorized synthetic
   live smoke covering search/get, bounded output, cancellation, revocation,
   and trace delivery.

Current evidence fails the release, registration, callback, and allowance
gates. If a provider becomes eligible later, rerun this matrix against its
exact runtime version and packaged production path before naming adapter files
or advertising the four lookup tools. Keep `DenyAllToolExecutor` as the
fail-closed fallback for unrelated tools.

## Independent Stage 2 checkpoint

This is current provider-neutral source evidence from offline fixtures only,
not native-adapter acceptance or live-activation evidence. The
`AgentMemoryLookupService` resolves the workflow from the persisted run,
validates the captured absolute workspace, and binds each session to the
trusted run, node, workflow, and workspace tuple. Every call rechecks the
persisted run-to-workflow relation and `memory_lookup_enabled`; that
permission is independent of automatic retrieval. A running node need not
have a `run_steps` row, and editable graph membership is not execution
authority.

The service performs scoped SQLite searches across current memories, prior run
steps, historical memories, and captured evidence. Run-step search is limited
to indexed output and a saved Plan 051 receipt; prompt text, input metadata,
and error text cannot produce a hit. Each get rechecks visibility, lifecycle
state, receipt or evidence identity, and source version, returning explicit
deleted, expired, retracted, replaced, unlinked, incomplete, or unavailable
states instead of replaying stale content. References and cursors are random,
session-local tokens bound to scope and source version; raw database and
filesystem identifiers are not model-facing lookup handles.

Verified artifact reads now require the canonical content digest and a
writer-owned verified chunk manifest. The service accepts only a trusted
artifact root/path with no symlink component, reads at most one or two
adjacent 4,096-byte chunks for a 4,000-byte UTF-8 page, and verifies each
returned chunk digest and page boundary. Missing, legacy, or unverified
manifests remain unavailable; metadata-only or stat-only identity is not
proof. No arbitrary stored artifact path becomes readable merely because a
digest is present.

Every call samples the injected clock and applies Plan 050's `valid_from` and
`expires_at` rules, so expiry can take effect between search and get without a
database mutation. Opaque memory versions bind semantic revision, canonical
content digest, artifact identity, lifecycle fields, provenance state, source,
inference qualification, and evidence subject. A qualification change
invalidates an older reference even when the body is unchanged. Successful
service calls emit in-memory `LookupTrace` values only after response
serialization and budget acceptance; each trace carries the tool, every
search hit or get source, backend source identity/version, state, page,
returned bytes, and `retrieved_data` classification. Durable native delivery
and step association remain separate production gates.

Calls share the fixed eight-call, 16,000-returned-byte, five-second read
deadline, and 4,000-byte body-page ceilings. SQLite reads avoid blocking on
the process mutex, temporarily disable the default busy wait, retry busy or
locked operations with cancellation/deadline checkpoints, and interrupt
active computation. File operations use one bounded worker per service; a
stalled worker blocks further file work, and symlinks or non-regular files are
rejected.

Schema inspection requires Plan 050's `semantic_revision`,
`artifact_content_digest`, `observed_at`, and `valid_from` columns and returns
`Plan050IntegrationRequired` otherwise. Verified artifact paging additionally
consumes the writer-owned chunk manifest and digest tables. Legacy artifacts
stay in an explicit unavailable/unverified state until a legitimate bounded
indexing path supplies proof.

The remaining production gates are precise: no registered runtime advertises
`supports_memory_lookup=true`; `NativeTurnRequest` still lacks workflow
identity and a provider-reported remaining-context allowance; and the
production registry retains fail-closed behavior when no verified allowance or
host callback exists. Stage 3 must pass trusted execution context into a
narrow lookup-only executor, reserve output capacity against all turn traffic,
persist source/version/page and accepted-delivery traces, and keep
`DenyAllToolExecutor` for unrelated tools. No native provider, registry entry,
manifest capability, workflow opt-in, or UI surface advertises these tools.

## Historical verification
The commands and results below are inherited checkpoint evidence; they were
not rerun for this update.

The combined Step 1 contract and independent Stage 2 service suite passed nineteen tests. The fixtures freeze all numeric ceilings and dotted tool names; reject malformed and unknown request fields; cover exact and oversized UTF-8 byte, Unicode-character, term-count, RFC3339, reference, cursor, field, and aggregate-step limits; and measure the actual serialized JSON envelopes including escape and metadata overhead. They also enforce tool/source/state consistency, explicit unavailable or inactive responses, and a compatible get contract for every source/state pair accepted by search.

The Stage 2 additions exercise real in-memory and file-backed SQLite databases and temporary files. They cover user, workspace, workflow, linked, and unrelated memory scope; body, qualification, link, natural expiration, evidence, receipt, and artifact mutations between search and get or between pages; opaque token reuse across runs; bounded multibyte artifact pages; aggregate call and byte budgets; cancellation; and retrieved-data traces. Prompt and error sentinels cannot reappear as saved output after their linked memory is unlinked and retracted. Deterministic stalls cover the database mutex, an external SQLite exclusive lock, long-running SQL, file-page reads, and file metadata probes on the actual history search/get paths. A worker handoff test holds a completed worker after publication while its successor succeeds. Memory artifact tests prove that the service returns only the stored preview and marks the body unavailable. The focused command was `cargo test --locked --manifest-path src-tauri/Cargo.toml agent_memory_lookup`; all nineteen tests passed.

`bun test tests/native-harness-release-matrix.test.ts` passed all three tests with 122 assertions. The test confirms that every CLI provider remains available and `enabledNativeProviders` remains empty across macOS, Windows, Linux, development, and packaged cases.

`git diff --check` passed for the complete working tree.

The five pre-existing memory-quality JSON fixtures remain byte-for-byte unchanged. In particular, `baseline-composition-v1.json` remains SHA-256 `8d9a27d7d1ca6dd6d3a9d4963c88f9168e651dab4fcf830725d37da78de8413a`.
