import { useEffect, useState, type FormEvent } from "react";
import { AgentMark } from "../../../../components/agent-mark";
import { Icon } from "../../../../components/icon/icon";
import { SelectControl } from "../../../../components/select-control";
import { checkMcpServer, connectClaudeCode } from "../../api";
import type { BuilderAgentInfo, BuilderConfig, BuilderInfo, BuilderMcpServer, BuilderRule, McpCheck } from "../../api";
import { MCP_PATH_TOKEN, MCP_PRESETS, presetToServer } from "../../mcp-presets";

/** Shell-like split: `npx -y "@scope/pkg name"` → ["npx", "-y", "@scope/pkg name"]. */
export function splitCommand(line: string): string[] {
  const parts: string[] = [];
  const pattern = /"([^"]*)"|'([^']*)'|(\S+)/g;
  for (const match of line.matchAll(pattern)) {
    parts.push(match[1] ?? match[2] ?? match[3]);
  }
  return parts;
}

function joinCommand(server: BuilderMcpServer): string {
  return [server.command, ...server.args]
    .filter(Boolean)
    .map((part) => (/\s/.test(part) ? `"${part}"` : part))
    .join(" ");
}

/** `KEY=value` (env) or `Name: value` (headers), one per line. */
export function parsePairs(text: string, separator: "=" | ":"): Record<string, string> {
  const pairs: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const at = line.indexOf(separator);
    if (at <= 0) continue;
    const key = line.slice(0, at).trim();
    if (key) pairs[key] = line.slice(at + 1).trim();
  }
  return pairs;
}

function formatPairs(pairs: Record<string, string>, separator: "=" | ": "): string {
  return Object.entries(pairs)
    .map(([key, value]) => `${key}${separator}${value}`)
    .join("\n");
}

function Toggle({ on, label, onChange }: { on: boolean; label: string; onChange: (on: boolean) => void }) {
  return (
    <button
      type="button"
      className={["settings-toggle", "builder-toggle", on ? "is-on" : ""].filter(Boolean).join(" ")}
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={() => onChange(!on)}
    >
      <span className="settings-toggle-knob" />
    </button>
  );
}

type Draft =
  | { kind: "rule"; value: BuilderRule }
  | { kind: "mcp"; value: BuilderMcpServer; commandLine: string; envText: string; headersText: string };

const emptyRule = (): BuilderRule => ({ id: "", name: "", body: "", enabled: true });
const emptyServer = (): BuilderMcpServer => ({
  id: "",
  name: "",
  enabled: true,
  transport: "stdio",
  command: "",
  args: [],
  env: {},
  url: "",
  headers: {},
});

function serverFromDraft(draft: Extract<Draft, { kind: "mcp" }>): BuilderMcpServer {
  const [command = "", ...args] = splitCommand(draft.commandLine);
  return {
    ...draft.value,
    command: draft.value.transport === "stdio" ? command : "",
    args: draft.value.transport === "stdio" ? args : [],
    env: draft.value.transport === "stdio" ? parsePairs(draft.envText, "=") : {},
    url: draft.value.transport === "http" ? draft.value.url.trim() : "",
    headers: draft.value.transport === "http" ? parsePairs(draft.headersText, ":") : {},
  };
}

function hasPlaceholderArgument(draft: Extract<Draft, { kind: "mcp" }>): boolean {
  return splitCommand(draft.commandLine).some(
    (argument) => argument === MCP_PATH_TOKEN || /^<[^>]+>$/.test(argument),
  );
}

type SetupSection = "agent" | "rules" | "mcp" | "connect";

const ALL_SECTIONS: SetupSection[] = ["agent", "rules", "mcp", "connect"];

type Props = {
  info: BuilderInfo | null;
  agent: BuilderAgentInfo | undefined;
  config: BuilderConfig;
  error: string | null;
  onSave: (config: BuilderConfig) => Promise<boolean>;
  /** Which sections to render. Defaults to all of them. */
  sections?: SetupSection[];
  /** Folder used by path-based presets when the context knows one. */
  suggestedPath?: string | null;
};

export function BuilderSetupPanel({ info, agent, config, error, onSave, sections, suggestedPath }: Props) {
  const visible = new Set(sections ?? ALL_SECTIONS);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [claude, setClaude] = useState<{ busy?: boolean; added?: string; error?: string }>({});
  const [testResult, setTestResult] = useState<McpCheck | null>(null);
  const [testing, setTesting] = useState(false);
  const [copiedConfig, setCopiedConfig] = useState(false);

  const addToClaude = async () => {
    setClaude({ busy: true });
    try {
      setClaude({ added: await connectClaudeCode() });
    } catch (e) {
      setClaude({ error: String(e) });
    }
  };

  const editRule = (rule: BuilderRule) => setDraft({ kind: "rule", value: { ...rule } });
  const editServer = (server: BuilderMcpServer) =>
    setDraft({
      kind: "mcp",
      value: { ...server },
      commandLine: joinCommand(server),
      envText: formatPairs(server.env, "="),
      headersText: formatPairs(server.headers, ": "),
    });

  const applyPreset = (id: string) => {
    const preset = MCP_PRESETS.find((candidate) => candidate.id === id);
    if (!preset) return;
    const server = presetToServer(preset, suggestedPath);
    setDraft({
      kind: "mcp",
      value: server,
      commandLine: joinCommand(server),
      envText: formatPairs(server.env, "="),
      headersText: formatPairs(server.headers, ": "),
    });
    setTestResult(null);
  };

  const mcpDraft = draft?.kind === "mcp" ? draft : null;
  const placeholderArgument = mcpDraft ? hasPlaceholderArgument(mcpDraft) : false;
  const draftFingerprint = mcpDraft
    ? `${mcpDraft.commandLine}\n${mcpDraft.envText}\n${mcpDraft.headersText}\n${mcpDraft.value.transport}\n${mcpDraft.value.url}`
    : (draft?.kind ?? "");

  useEffect(() => {
    setTestResult(null);
  }, [draftFingerprint]);

  const testDraft = async () => {
    if (!mcpDraft) return;
    setTesting(true);
    setTestResult(null);
    try {
      setTestResult(await checkMcpServer(serverFromDraft(mcpDraft)));
    } catch (error) {
      setTestResult({ ok: false, detail: String(error) });
    } finally {
      setTesting(false);
    }
  };

  const upsert = <T extends { id: string }>(items: T[], item: T) =>
    item.id && items.some((i) => i.id === item.id)
      ? items.map((i) => (i.id === item.id ? item : i))
      : [...items, item];

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!draft) return;
    let next: BuilderConfig;
    if (draft.kind === "rule") {
      next = { ...config, rules: upsert(config.rules, draft.value) };
    } else {
      if (hasPlaceholderArgument(draft)) return;
      next = { ...config, mcpServers: upsert(config.mcpServers, serverFromDraft(draft)) };
    }
    if (await onSave(next)) setDraft(null);
  };

  const ruleEditor =
    draft?.kind === "rule" ? (
      <form className="builder-editor" onSubmit={submit}>
        <input
          autoFocus
          placeholder="Rule name, e.g. House style"
          aria-label="Rule name"
          value={draft.value.name}
          onChange={(e) => setDraft({ ...draft, value: { ...draft.value, name: e.target.value } })}
        />
        <textarea
          rows={5}
          placeholder="Always end workflows with an Output step. Prefer codex for code review…"
          aria-label="Rule instructions"
          value={draft.value.body}
          onChange={(e) => setDraft({ ...draft, value: { ...draft.value, body: e.target.value } })}
        />
        <div className="builder-editor-actions">
          <button type="button" className="ghost" onClick={() => setDraft(null)}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={!draft.value.name.trim()}>
            Save rule
          </button>
        </div>
      </form>
    ) : null;

  const mcpEditor =
    draft?.kind === "mcp" ? (
      <form className="builder-editor" onSubmit={submit}>
        <input
          autoFocus
          placeholder="Name, e.g. github"
          aria-label="Server name"
          value={draft.value.name}
          onChange={(e) => setDraft({ ...draft, value: { ...draft.value, name: e.target.value } })}
        />
        <div className="theme-switch builder-segmented" role="radiogroup" aria-label="Transport">
          {(["stdio", "http"] as const).map((transport) => (
            <button
              key={transport}
              type="button"
              role="radio"
              aria-checked={draft.value.transport === transport}
              className={draft.value.transport === transport ? "is-active" : undefined}
              onClick={() => setDraft({ ...draft, value: { ...draft.value, transport } })}
            >
              {transport === "stdio" ? "Command" : "HTTP"}
            </button>
          ))}
        </div>
        {draft.value.transport === "stdio" ? (
          <>
            <input
              className="builder-mono"
              placeholder="npx -y @modelcontextprotocol/server-github"
              aria-label="Command"
              value={draft.commandLine}
              onChange={(e) => setDraft({ ...draft, commandLine: e.target.value })}
            />
            <textarea
              className="builder-mono"
              rows={3}
              placeholder={"GITHUB_TOKEN=…\nONE_PER_LINE=value"}
              aria-label="Environment variables"
              value={draft.envText}
              onChange={(e) => setDraft({ ...draft, envText: e.target.value })}
            />
          </>
        ) : (
          <>
            <input
              className="builder-mono"
              placeholder="https://mcp.example.com/mcp"
              aria-label="Server URL"
              value={draft.value.url}
              onChange={(e) => setDraft({ ...draft, value: { ...draft.value, url: e.target.value } })}
            />
            <textarea
              className="builder-mono"
              rows={3}
              placeholder="Authorization: Bearer …"
              aria-label="Headers"
              value={draft.headersText}
              onChange={(e) => setDraft({ ...draft, headersText: e.target.value })}
            />
          </>
        )}
        <p className="builder-editor-note">Stored locally.</p>
        {placeholderArgument ? (
          <p className="builder-editor-note builder-editor-warning">
            Replace <code>{MCP_PATH_TOKEN}</code> with a real folder before saving.
          </p>
        ) : null}
        {testResult ? (
          <p
            className={`builder-test ${testResult.ok ? "is-ok" : "is-error"}`}
            role="status"
          >
            {testResult.detail}
          </p>
        ) : null}
        <div className="builder-editor-actions">
          <button
            type="button"
            className="ghost"
            disabled={testing}
            onClick={() => void testDraft()}
          >
            {testing ? "Testing…" : "Test"}
          </button>
          <span className="builder-editor-spacer" />
          <button type="button" className="ghost" onClick={() => setDraft(null)}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary"
            disabled={!draft.value.name.trim() || placeholderArgument}
          >
            Save server
          </button>
        </div>
      </form>
    ) : null;

  return (
    <div className="builder-setup" aria-label="Agent setup">
      {visible.has("agent") ? (
      <section className="builder-setup-section">
        <h3 className="builder-setup-title">Agent</h3>
        <div className="builder-agent-card">
          <AgentMark provider={agent?.id ?? "claude_code"} size={28} />
          <div className="builder-agent-text">
            <strong>{agent?.label ?? "Claude Code"}</strong>
            <span className={agent?.installed === false ? "is-error" : undefined}>
              {!agent ? "Checking…" : agent.installed ? "Ready on this Mac" : "CLI not found on PATH"}
            </span>
          </div>
        </div>
        <button
          type="button"
          className="builder-api-row"
          aria-expanded={toolsOpen}
          onClick={() => setToolsOpen((open) => !open)}
        >
          <span className={["builder-live-dot", info?.mcpReady ? "is-on" : ""].filter(Boolean).join(" ")} aria-hidden />
          <span className="builder-api-name">Alfred API</span>
          <span className="builder-api-count">{info ? `${info.tools.length} tool${info.tools.length === 1 ? "" : "s"}` : "…"}</span>
          <Icon name="caret-down" size={14} className={toolsOpen ? "builder-caret is-open" : "builder-caret"} />
        </button>
        {toolsOpen && info ? (
          <ul className="builder-tool-list">
            {info.tools.map((tool) => (
              <li key={tool.name} title={tool.description}>
                <code>{tool.name}</code>
                <span>{tool.description}</span>
              </li>
            ))}
          </ul>
        ) : null}
      </section>
      ) : null}

      {visible.has("rules") ? (
      <section className="builder-setup-section">
        <div className="builder-setup-head">
          <h3 className="builder-setup-title">Rules</h3>
          <button
            type="button"
            className="ghost builder-icon-button"
            aria-label="Add rule"
            title="Add rule"
            disabled={draft !== null}
            onClick={() => setDraft({ kind: "rule", value: emptyRule() })}
          >
            <Icon name="plus" size={16} />
          </button>
        </div>
        {config.rules.length === 0 && draft?.kind !== "rule" ? (
          <p className="builder-setup-empty">Standing instructions the agent follows in every chat.</p>
        ) : null}
        <ul className="builder-setup-list">
          {config.rules.map((rule) =>
            draft?.kind === "rule" && draft.value.id === rule.id ? (
              <li key={rule.id}>{ruleEditor}</li>
            ) : (
              <li key={rule.id} className="builder-setup-item">
                <Icon name="scroll" size={16} className="builder-setup-icon" />
                <button type="button" className="builder-setup-name" onClick={() => editRule(rule)} title={rule.body}>
                  {rule.name}
                </button>
                <button
                  type="button"
                  className="ghost builder-icon-button builder-remove"
                  aria-label={`Delete rule ${rule.name}`}
                  onClick={() => void onSave({ ...config, rules: config.rules.filter((r) => r.id !== rule.id) })}
                >
                  <Icon name="trash" size={14} />
                </button>
                <Toggle
                  on={rule.enabled}
                  label={`Use rule ${rule.name}`}
                  onChange={(enabled) =>
                    void onSave({ ...config, rules: config.rules.map((r) => (r.id === rule.id ? { ...r, enabled } : r)) })
                  }
                />
              </li>
            ),
          )}
          {draft?.kind === "rule" && !draft.value.id ? <li>{ruleEditor}</li> : null}
        </ul>
      </section>
      ) : null}

      {visible.has("mcp") ? (
      <section className="builder-setup-section">
        <div className="builder-setup-head">
          <h3 className="builder-setup-title">MCP servers</h3>
          <button
            type="button"
            className="ghost builder-icon-button"
            aria-label="Add MCP server"
            title="Add MCP server"
            disabled={draft !== null}
            onClick={() =>
              setDraft({ kind: "mcp", value: emptyServer(), commandLine: "", envText: "", headersText: "" })
            }
          >
            <Icon name="plus" size={16} />
          </button>
        </div>
        <SelectControl
          containerClassName="builder-preset-select"
          aria-label="Add MCP server from preset"
          value=""
          disabled={draft !== null}
          onChange={(event) => {
            if (event.target.value) applyPreset(event.target.value);
          }}
        >
          <option value="">Add from preset…</option>
          {MCP_PRESETS.map((preset) => (
            <option key={preset.id} value={preset.id}>
              {preset.label}
            </option>
          ))}
        </SelectControl>
        {config.mcpServers.length === 0 && draft?.kind !== "mcp" ? (
          <p className="builder-setup-empty">No MCP servers.</p>
        ) : null}
        <ul className="builder-setup-list">
          {config.mcpServers.map((server) =>
            draft?.kind === "mcp" && draft.value.id === server.id ? (
              <li key={server.id}>{mcpEditor}</li>
            ) : (
              <li key={server.id} className="builder-setup-item">
                <Icon name="plug" size={16} className="builder-setup-icon" />
                <button
                  type="button"
                  className="builder-setup-name"
                  onClick={() => editServer(server)}
                  title={server.transport === "http" ? server.url : joinCommand(server)}
                >
                  {server.name}
                  <span className="builder-setup-sub">{server.transport === "http" ? "HTTP" : server.command}</span>
                </button>
                <button
                  type="button"
                  className="ghost builder-icon-button builder-remove"
                  aria-label={`Delete MCP server ${server.name}`}
                  onClick={() =>
                    void onSave({ ...config, mcpServers: config.mcpServers.filter((s) => s.id !== server.id) })
                  }
                >
                  <Icon name="trash" size={14} />
                </button>
                <Toggle
                  on={server.enabled}
                  label={`Use MCP server ${server.name}`}
                  onChange={(enabled) =>
                    void onSave({
                      ...config,
                      mcpServers: config.mcpServers.map((s) => (s.id === server.id ? { ...s, enabled } : s)),
                    })
                  }
                />
              </li>
            ),
          )}
          {draft?.kind === "mcp" && !draft.value.id ? <li>{mcpEditor}</li> : null}
        </ul>
      </section>
      ) : null}

      {visible.has("connect") ? (
      <section className="builder-setup-section">
        <h3 className="builder-setup-title">Use from other agents</h3>
        <div className="builder-transfer-row">
          <div className="builder-transfer-copy">
            <strong>Claude Code</strong>
            <span>Bridge + MCP servers</span>
          </div>
          <button
            type="button"
            className="ghost"
            disabled={!info?.externalMcp || claude.busy}
            onClick={() => void addToClaude()}
          >
            {claude.busy ? "Syncing…" : "Sync"}
          </button>
        </div>
        {claude.added ? (
          <p className="builder-editor-note" role="status">
            Registered <code>{claude.added}</code>.
          </p>
        ) : null}
        {claude.error ? (
          <p className="builder-setup-error" role="alert">
            <Icon name="warning-circle" size={16} />
            {claude.error}
          </p>
        ) : null}

        {info?.externalMcp?.config ? (
          <details className="builder-config">
            <summary>Other clients</summary>
            {info.externalMcp ? (
              <p className="builder-editor-note">
                Bridge: <code>{info.externalMcp.name}</code>
              </p>
            ) : null}
            {info.externalMcp ? (
              <p className="builder-editor-note builder-connect-command user-select-text">
                <code>{info.externalMcp.command}</code>
              </p>
            ) : null}
            <div className="builder-config-actions">
              <button
                type="button"
                className="ghost"
                onClick={() => {
                  const text = JSON.stringify(info.externalMcp?.config, null, 2);
                  void navigator.clipboard
                    ?.writeText(text)
                    .then(() => {
                      setCopiedConfig(true);
                      window.setTimeout(() => setCopiedConfig(false), 1500);
                    })
                    .catch(() => undefined);
                }}
              >
                {copiedConfig ? "Copied" : "Copy JSON"}
              </button>
            </div>
            <pre className="builder-mono builder-config-json user-select-text">
              {JSON.stringify(info.externalMcp.config, null, 2)}
            </pre>
          </details>
        ) : null}

      </section>
      ) : null}

      {error ? (
        <p className="builder-setup-error" role="alert">
          <Icon name="warning-circle" size={16} />
          {error}
        </p>
      ) : null}
    </div>
  );
}
