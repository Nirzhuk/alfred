# Agent harnesses

Each agent step stores two separate choices: a provider and a harness. Provider
ids stay stable; changing the harness never turns Codex into a different
provider, for example.

## Provider CLI

The `cli` harness delegates the step to the provider's installed command-line
agent. It is a first-class option, not a deprecated compatibility mode. The CLI
keeps ownership of its login, configuration, plugins, model aliases, and local
sessions. Alfred neither reads nor imports the CLI's credentials.

Workflows saved before the harness selector existed have no `harness` field.
Alfred reads those workflows as `cli`, so they continue through the same local
adapter. Newly created agent steps save `harness: "cli"` explicitly.

## Alfred

The `alfred` harness is the boundary for future Alfred-managed provider
execution. It has a separate native runtime and a separate opaque account
reference. Native account credentials are never written to workflow JSON, run
events, or model/provider capability responses, and CLI credentials are not
reused as native credentials.

Native account identity separates provider, product, runtime, and billing. The
stable products are:

```text
claude_code_subscription  claude_api
chatgpt_codex             openai_api
opencode_go               opencode_zen
cursor_cloud              github_copilot_subscription
gemini_api                grok_api
```

Managed subscription accounts point to an opaque isolated runtime profile and
do not carry a fake secret credential reference. Direct API/PAYG accounts may
carry an opaque secret reference. Neither reference crosses the Tauri command
DTO or enters React state. Billing source/owner is stored separately from a
sourced entitlement observation (`unknown`, `eligible`, `limited`,
`exhausted`, or `ineligible`), so exhaustion cannot silently switch products.

Native runtime descriptors also state who executes tools:
`alfred_executed`, `runtime_executed_with_host_approval`, or `no_tools`.
Runtime execution is valid only when Alfred approves the bounded request before
execution; otherwise tools stay disabled. A descriptor identifies one exact
provider/product pair, and an account for another product is rejected before
credential access or turn execution.

No provider-native runtime is enabled in this release. The existing dedicated
Claude API, Gemini API, and Grok API key intake can create isolated native
account records, but that does not make any corresponding runtime executable.
The editor reads the same versioned capability manifest as the runner and shows
each provider's `blocked` or `disabled` reason. Attempting to run an unavailable
native node returns `native_runtime_unavailable`; Alfred does not fall back to a
provider CLI. As native providers are added later, their model and account
support may remain narrower than the corresponding CLI.

## Compatibility and migration

Old workflows, imported graphs, duplicated nodes, and templates that omit the
`harness` field are read as `cli`. Alfred never rewrites them to `alfred`.
New agent nodes persist `harness: "cli"`, and changing the selector is an
explicit edit that also clears any account reference that belonged to the
previous harness.

Run history records both the provider and harness for completed, failed, and
cancelled agent steps. This is execution identity, not a migration hint: a
reader must never infer that a failed native step ran through the CLI.

## Current native release gates

No native provider is enabled. These provider-specific blockers do not affect
CLI execution or one another:

| Provider | Status | Exact external/release blocker |
| --- | --- | --- |
| ChatGPT Codex | No-go | The stable Python SDK `0.147.0` and exact binary package are not bundled; public approval/cancellation coverage, profile lifecycle, packaging, and no-CLI smoke remain unproved. Raw App Server was rejected for production on 2026-08-26. Diagnostic gate: `codex_cross_platform_signing_and_packaged_smoke_missing`. |
| OpenAI API | Blocked | Separate API-billed product; no native runtime is registered. |
| Claude Code subscription | Blocked | The unmodified Claude Code `2.1.246` binary is not bundled; isolated profile/login, host-approved tool ownership, packaging, and no-CLI smoke remain unproved. |
| Claude API | Blocked | Dedicated key intake exists for this separate Anthropic API-billed product; live runtime smoke and release registration remain gated as `claude_live_api_key_smoke_missing`. |
| Cursor | Blocked | API-key custody, explicit repository consent persistence, and live end-to-end Cloud Agents validation are missing |
| OpenCode Go | Blocked | Commercial ToS clarification, `opencode serve` `1.18.23` package/profile ownership, transient Go-key handoff, and host-approved built-in tool smoke are missing. Diagnostic gate: `opencode_package_account_and_tool_bridge_unverified`. |
| OpenCode Zen | Blocked | Separate PAYG credential and explicit route have no registered managed server; there is no Go-to-Zen fallback. |
| GitHub Copilot | Blocked | The pinned SDK/CLI is not linked and packaged with its required license notices and packaged smoke evidence |
| Gemini API | Blocked | Dedicated key intake exists; live API smoke and release registration are missing. Desktop OAuth is a separate product decision. |
| Grok API | Blocked | Dedicated xAI key intake exists; live API smoke and release registration are missing. |
| Pi / OMP | CLI only | They are not managed native products and remain unchanged CLI adapters. |

Runtime versions in the manifest are protocol/package targets, not claims that
an artifact is bundled. Bundled metadata separately reports resource presence,
checksum, licence/notice, signing, and rollback gates. A missing resource or
manifest entry fails closed.

## Recovery and explicit fallback

CLI and Alfred credentials are separate. If a provider later enables native
reconnection, it will not change the CLI's login; signing in to a CLI never
connects a native account. If native mode is unavailable:

1. Keep the failed node unchanged while collecting its safe provider/harness
   diagnostic.
2. This zero-native release does not make provider runtimes available. Existing
   recovery records can only use the lifecycle actions actually exposed by the
   gated product; a managed profile and a direct API secret have different
   cleanup owners.
3. To keep working through the CLI, explicitly edit the node and select
   **Provider CLI**. Confirm the CLI is installed and logged in first.

Alfred never performs step 3 automatically because it can change the account,
billing owner, data route, and tool permissions. See the
[native harness support runbook](native-harness-support.md) for safe diagnostic
and runtime-recovery guidance.

## Model fallback

An agent node can name one **Fallback model** in its settings. Alfred retries
the step once with that model only when the primary attempt fails because the
model was unavailable: a usage or rate limit, provider overload, or an unknown
or unsupported model. Authentication failures, a missing CLI, cancellation, and
ordinary task failures are not retried.

The fallback stays on the node's provider, harness, and account. It never
switches harnesses or accounts, so the CLI/native credential, billing owner,
data route, and tool permissions remain the ones the user selected. A fallback
that resolves to the same model as the primary is ignored. If the retry also
fails, the step fails with the retry's error and the run history shows the
fallback model in the step statistics.

The retry repeats the same prompt in the same working directory. If the primary
attempt already changed files before it reported the availability failure, the
fallback can repeat or extend that work. Prefer fallback for failures that
happen before the task starts mutating files.
