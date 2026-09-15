# Restricted memory review provider support

Checked 2026-09-09 on macOS from the Alfred desktop environment.

| Product / runtime | Execution mode | Restriction contract | Evidence | Status |
| --- | --- | --- | --- | --- |
| Claude Code `2.1.271` (installed) | `restricted_print` | `memory-review-restricted-v1` | Read-only local inspection on 2026-09-09 returned `2.1.271 (Claude Code)` and its current help documented `--safe-mode`, `--restricted`, `--tools`, `--strict-mcp-config`, `--disable-slash-commands`, `--setting-sources`, `--permission-prompts`, `--max-turns`, `--input-format`, `--output-format`, and `--no-session-persistence`. Primary references: https://code.claude.com/docs/en/cli-reference, https://code.claude.com/docs/en/managed-settings, https://code.claude.com/docs/en/settings-reference#allowmanagedhooksonly | **BLOCKED:** the adapter accepts only exact runtime `2.1.263`, requires all restriction flags and `live_activation_verified=true`, and the observed `2.1.271` is `unknown_runtime_version`; no authorized live synthetic activation check or managed-policy assurance exists. |

The allowlisted fixture version (`2.1.263`) is historical conformance evidence
only. It is not installed-runtime evidence and does not qualify the observed
Claude Code `2.1.271`. The adapter's read-only `installed_capability()` probe
calls `inspect_capability(..., false)`; the source-derived result for `2.1.271`
is unavailable with `unknown_runtime_version`, so production makes zero
provider calls in this environment. Unknown or future versions, missing
restriction flags, and a missing live activation proof remain unavailable.

The current Claude Code references describe `--safe-mode` as disabling user
customizations while retaining policy-configured behavior, including managed
executable hooks. `--restricted` removes built-in command/code tools and
WebFetch unless explicitly named, confines built-in files, loads managed
settings and `--settings`, and refuses `bypassPermissions`. Managed settings
have precedence; `allowManagedHooksOnly` and `disableAllHooks` semantics mean
lower-level flags cannot override managed hooks. Alfred cannot prove the
effective managed policy without administrator-controlled assurance, so it
fails closed.

The conformance invocation supplies the bounded review prompt over stdin and
uses fixed arguments: print mode, JSON output, one turn, no session
persistence, safe mode, restricted mode, an empty built-in tool list, strict
empty MCP configuration, disabled slash commands, empty user/project/local
setting sources, and no permission prompt handler. Safe mode still retains
policy-configured behavior, including managed executable hooks; restricted
mode removes built-in command/code tools and WebFetch unless explicitly named,
confines built-in files, loads managed settings and `--settings`, and refuses
`bypassPermissions`. The process runs in a fresh directory, captures stdout
with a 32 KiB hard limit, drains stderr, polls cancellation and the
120-second deadline, and kills its process group on termination.

Authentication and billing remain owned by the selected Claude Code CLI. The
restricted request contains no account file, credential, workspace, skills,
session resume id, or caller-provided command. A live check would require an
already-authorized account and explicit synthetic adversarial fixtures; that
evidence is not present in this checkout.

## Contract/conformance milestone

The typed request/result boundary, exact capability identity, fake-capability
fixtures, bounded transport seam, no-tool event rejection, and background
runner routing are implemented. This is the contract/conformance milestone for
Plan 049. Live activation remains blocked pending the authorized check above.
