# MCP servers and skills

**Settings → MCP & Skills** is the global view of the tools Alfred agents can
use. It has three parts: build identity, MCP servers for Speak with Alfred, and
the skills discovered on this machine.

## Build identity

Alfred ships two identities:

| Identity | Data folder | MCP server name |
| --- | --- | --- |
| Stable | `.../com.nirzhuk.alfred` | `alfred` |
| Development | `.../com.nirzhuk.alfred.dev` | `alfred-dev` |

MCP servers, builder rules, and the Claude Code bridge endpoint live in the
selected build's own data folder as `builder.json` and `mcp-endpoint.json`.
Editing MCP servers in the development build cannot change the stable build.
The **Build** card names the active identity and data folder.

Skills are not per build. Both identities read the same user skill folders and
the project folder selected in **Settings → MCP & Skills → Skills**.

## MCP servers for Speak with Alfred

The **MCP servers** section edits the servers passed to the builder agent:

- **Command / stdio** runs a local process, for example
  `npx -y @modelcontextprotocol/server-github`, with optional `KEY=value`
  environment lines.
- **HTTP** points at a remote endpoint, with optional `Name: value` header
  lines.
- Each server has an enable switch. Disabled servers are not passed to the
  agent.
- **Add from preset** pre-fills GitHub, Filesystem, Git, Fetch, Memory, Time,
  Sequential thinking, and Linear. Presets are starting points: check the
  command and add credentials before saving. Path-based presets use the active
  project folder when one is selected; otherwise they keep a `<project path>`
  placeholder that blocks saving until replaced.
- **Test** checks the draft without saving it. For stdio it verifies the command
  exists on `PATH` (it does not start the package); for HTTP it sends a short
  request and reports any HTTP status as reachable.

These servers apply to **Speak with Alfred**, not to workflow agent steps.
Workflow steps choose skills per node and do not inherit builder MCP servers.
Environment values and headers can contain tokens; `builder.json` is written
owner-only.

## Claude Code and other MCP clients

**Use from other agents → Sync to Claude Code** registers Alfred's stdio
bridge and every enabled MCP server above in Claude Code's user scope. The
bridge and the configured servers are named with the build prefix:

| Build | Bridge | Configured server `github` |
| --- | --- | --- |
| Stable | `alfred` | `alfred-github` |
| Development | `alfred-dev` | `alfred-dev-github` |

The prefix keeps development and stable registrations from overwriting each
other. Re-running sync replaces this build's entries and removes entries that
were disabled since the previous sync.

The same section shows the exact bridge command and a JSON config containing
the bridge plus every enabled server. Paste that JSON into any client that
accepts the `mcpServers` shape; clients that only accept a single stdio command
can still use the bridge alone.

The bridge also exposes Alfred's workflow API and the same setup context to
outside agents:

- `list_rules` — the user's standing rules.
- `list_mcp_servers` — configured server names and transports, secrets redacted.
- `list_skills` — skill names, descriptions, and sources.
- `get_skill` — the full `SKILL.md` instructions for a named skill.

`add_rule` and `add_mcp_server` stay hidden: outside agents can use the setup
but cannot reconfigure it. Skills remain readable through `get_skill` even when
native links are not used.

## Skills

The Settings list shows only Alfred's own shared skills: the user-level
`.agents/skills` packages. Provider-owned Cursor, Claude, Codex, and other skill
folders are not listed here. The per-node **Skills** picker in a workflow still
discovers all provider skill folders.

Each row is the slash command and its Claude Code link state. **Sync** links
the shared packages into Claude Code's user skills folder so they appear as
native `/skill` commands. On macOS and Linux the entries are symlinks; Windows
falls back to a managed copy. Re-running removes links/copies whose source skill
disappeared and never overwrites a folder Alfred did not create. **Show in
folder** reveals the source `SKILL.md`; the filter narrows the list.

The list is read-only. To add, edit, or remove a shared skill, edit its
`SKILL.md` under `~/.agents/skills`; Refresh picks up the change. A project's
skills are available to the per-node picker without being synced globally.
