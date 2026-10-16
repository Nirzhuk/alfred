import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { AgentMark } from "../../../../components/agent-mark";
import { Icon } from "../../../../components/icon/icon";
import type { PhosphorIconName } from "../../../../components/icon/phosphor";
import { SelectControl } from "../../../../components/select-control";
import { nodeAccentColor } from "../../../workflow/node-colors";
import { useWorkflowStore } from "../../../workflow/store";
import type { BuilderAgentId } from "../../api";
import { useBuilderStore } from "../../store";
import {
  isWorking,
  splitToolName,
  toolDetail,
  toolLabel,
  workflowPreview,
  type AssistantMessage,
  type Part,
  type ToolPart,
} from "../../stream";
import { AlfredOrb } from "../alfred-orb";
import { BuilderChatList } from "../builder-chat-list";
import { BuilderSetupPanel } from "../builder-setup-panel";
import { ChatMarkdown } from "../chat-markdown";
import "./builder.css";

const STARTERS = [
  {
    icon: "clock" as const,
    title: "Morning repo digest",
    prompt:
      "Every weekday at 9:00, summarize the new commits in my current project and send me a desktop notification with the highlights.",
  },
  {
    icon: "chat-text" as const,
    title: "Agent debate",
    prompt:
      "Build a workflow where two different agents debate a technical decision I type in, and a third agent judges and writes the verdict.",
  },
  {
    icon: "git-pull-request" as const,
    title: "Ship review",
    prompt:
      "Create a workflow that reviews my uncommitted changes, lists blockers first, and drafts a pull request.",
  },
  {
    icon: "magnifying-glass" as const,
    title: "Audit my workflows",
    prompt: "Look at the workflows I already have and suggest the three most useful improvements.",
  },
];

function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}

function seconds(ms: number): string {
  const s = Math.max(0, ms) / 1000;
  return s < 10 ? `${s.toFixed(1)}s` : s < 60 ? `${Math.round(s)}s` : `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`;
}

function toolIcon(part: ToolPart): PhosphorIconName {
  const { server, tool } = splitToolName(part.name);
  if (server === "alfred") {
    if (tool.includes("workflow")) return "flow-arrow";
    if (tool === "get_run" || tool === "run_workflow") return "play";
    if (tool === "set_schedule") return "clock";
    if (tool.includes("rule")) return "scroll";
    if (tool.includes("mcp")) return "plug";
    if (tool === "list_agents") return "sparkle";
    return "database";
  }
  if (server) return "plug";
  if (tool === "Bash") return "terminal-window";
  if (tool === "Read" || tool === "Write" || tool === "Edit") return "file";
  if (tool.startsWith("Web")) return "globe";
  if (tool === "Glob" || tool === "Grep") return "magnifying-glass";
  return "wrench";
}

/** What the agent is doing right now, in a few words. */
export function activityLabel(message: AssistantMessage, now: number): string {
  if (message.status === "queued") {
    return now - message.startedAt > 1500 ? "Waiting for another Claude run to finish" : "Waking Alfred";
  }
  if (message.status === "connecting") return "Connecting to Alfred's API";
  const last = message.parts[message.parts.length - 1];
  const running = message.parts.filter((p): p is ToolPart => p.kind === "tool" && p.status === "running").pop();
  if (running) {
    const detail = toolDetail(running);
    return detail ? `${toolLabel(running)} · ${detail}` : toolLabel(running);
  }
  if (!last || (last.kind === "thinking" && last.endedAt === undefined)) return "Thinking";
  if (last.kind === "text") return "Writing";
  return "Thinking";
}

function ThinkingRow({ part, now }: { part: Extract<Part, { kind: "thinking" }>; now: number }) {
  const [open, setOpen] = useState(false);
  const live = part.endedAt === undefined;
  const text = part.text.trim();
  return (
    <div className={["builder-thinking", live ? "is-live" : ""].filter(Boolean).join(" ")}>
      <button
        type="button"
        className="builder-thinking-head"
        disabled={!text}
        aria-expanded={text ? open : undefined}
        onClick={() => setOpen((v) => !v)}
      >
        <Icon name="brain" size={14} />
        <span className={live ? "builder-shimmer" : undefined}>
          {live ? "Thinking" : `Thought for ${seconds((part.endedAt ?? now) - part.startedAt)}`}
        </span>
        {text ? <Icon name="caret-down" size={12} className={open ? "builder-caret is-open" : "builder-caret"} /> : null}
      </button>
      {open && text ? <p className="builder-thinking-body user-select-text">{text}</p> : null}
    </div>
  );
}

function pretty(json: string): string {
  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
}

function ToolRow({ part, now }: { part: ToolPart; now: number }) {
  const [open, setOpen] = useState(false);
  const detail = toolDetail(part);
  const { server } = splitToolName(part.name);
  const elapsed = (part.endedAt ?? now) - part.startedAt;
  return (
    <li className={`builder-tool is-${part.status}`}>
      <button type="button" className="builder-tool-head" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <span className="builder-tool-status" aria-hidden>
          {part.status === "running" ? (
            <span className="builder-spinner" />
          ) : part.status === "done" ? (
            <Icon name="check" size={12} />
          ) : (
            <Icon name="x" size={12} />
          )}
        </span>
        <Icon name={toolIcon(part)} size={16} className="builder-tool-icon" />
        <span className={["builder-tool-label", part.status === "running" ? "builder-shimmer" : ""].filter(Boolean).join(" ")}>
          {toolLabel(part)}
        </span>
        {detail ? <span className="builder-tool-detail">{detail}</span> : null}
        {server && server !== "alfred" ? <span className="builder-tool-server">{server}</span> : null}
        {part.status === "running" || elapsed >= 100 ? (
          <span className="builder-tool-time">{seconds(elapsed)}</span>
        ) : null}
        <Icon name="caret-down" size={12} className={open ? "builder-caret is-open" : "builder-caret"} />
      </button>
      {open ? (
        <div className="builder-tool-body user-select-text">
          <p className="builder-tool-caption">Input</p>
          <pre>{pretty(part.inputJson) || "{}"}</pre>
          {part.result !== undefined ? (
            <>
              <p className="builder-tool-caption">{part.status === "error" ? "Error" : "Result"}</p>
              <pre>{pretty(part.result).slice(0, 6000)}</pre>
            </>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}

function WorkflowCard({
  part,
  onOpen,
  onTest,
  busy,
}: {
  part: ToolPart;
  onOpen: (id: string) => void;
  onTest: (name: string) => void;
  busy: boolean;
}) {
  const preview = workflowPreview(part);
  if (!preview) return null;
  const steps = preview.columns.reduce((n, c) => n + c.length, 0);
  let order = 0;
  return (
    <div className="builder-workflow-card">
      <div className="builder-workflow-head">
        <span className="builder-workflow-badge">
          <Icon name="flow-arrow" size={16} />
        </span>
        <div className="builder-workflow-title">
          <strong>{preview.name}</strong>
          <span>
            {steps} step{steps === 1 ? "" : "s"} · {preview.edges} connection{preview.edges === 1 ? "" : "s"} ·{" "}
            {splitToolName(part.name).tool === "create_workflow" ? "added to your sidebar" : "saved"}
          </span>
        </div>
      </div>
      <div className="builder-graph" role="list" aria-label={`${preview.name} steps`}>
        {preview.columns.map((column, c) => (
          <div key={c} className="builder-graph-column" style={{ "--col": c } as CSSProperties}>
            {c > 0 ? <span className="builder-graph-link" aria-hidden /> : null}
            <div className="builder-graph-stack">
              {column.map((node) => (
                <span
                  key={node.id}
                  role="listitem"
                  className="builder-graph-node"
                  style={{ "--i": order++, "--node-color": nodeAccentColor(node.type) } as CSSProperties}
                >
                  <span className="builder-graph-dot" aria-hidden />
                  {node.label}
                </span>
              ))}
            </div>
          </div>
        ))}
      </div>
      <div className="builder-workflow-actions">
        {preview.workflowId ? (
          <button type="button" className="primary" onClick={() => onOpen(preview.workflowId!)}>
            <Icon name="arrow-square-out" size={14} />
            Open on canvas
          </button>
        ) : null}
        <button type="button" disabled={busy} onClick={() => onTest(preview.name)}>
          <Icon name="play" size={14} />
          Test run
        </button>
      </div>
    </div>
  );
}

function ActivityBar({ message, now }: { message: AssistantMessage; now: number }) {
  return (
    <div className="builder-activity" role="status" aria-live="polite">
      <AlfredOrb size="xs" busy />
      <span className="builder-activity-label builder-shimmer">{activityLabel(message, now)}</span>
      <span className="builder-activity-time">{seconds(now - message.startedAt)}</span>
    </div>
  );
}

function AssistantBlock({
  message,
  agent,
  now,
  onOpenWorkflow,
  onSend,
  busy,
}: {
  message: AssistantMessage;
  agent: BuilderAgentId;
  now: number;
  onOpenWorkflow: (id: string) => void;
  onSend: (text: string) => void;
  busy: boolean;
}) {
  const working = isWorking(message.status);
  // Consecutive tool calls share one timeline.
  const groups: Array<{ key: string; parts: Part[] }> = [];
  message.parts.forEach((part, i) => {
    const last = groups[groups.length - 1];
    if (part.kind === "tool" && last?.parts[0]?.kind === "tool") last.parts.push(part);
    else groups.push({ key: `g${i}`, parts: [part] });
  });
  const lastText = [...message.parts].reverse().find((p) => p.kind === "text");

  return (
    <article className={["builder-message", "is-assistant", working ? "is-working" : ""].filter(Boolean).join(" ")}>
      <div className="builder-avatar" aria-hidden>
        <AgentMark provider={agent} size={18} />
      </div>
      <div className="builder-message-body">
        {groups.map((group) => {
          const first = group.parts[0];
          if (first.kind === "text") {
            return first.text ? (
              <div key={group.key} className={first === lastText && working ? "builder-text is-streaming" : "builder-text"}>
                <ChatMarkdown text={first.text} />
              </div>
            ) : null;
          }
          if (first.kind === "thinking") return <ThinkingRow key={group.key} part={first} now={now} />;
          const tools = group.parts as ToolPart[];
          return (
            <div key={group.key} className="builder-tool-group">
              <ol className="builder-tools">
                {tools.map((part) => (
                  <ToolRow key={part.id} part={part} now={now} />
                ))}
              </ol>
              {tools.map((part) => (
                <WorkflowCard
                  key={`${part.id}-card`}
                  part={part}
                  busy={busy}
                  onOpen={onOpenWorkflow}
                  onTest={(name) => onSend(`Run the “${name}” workflow now and tell me how it went.`)}
                />
              ))}
            </div>
          );
        })}
        {working ? <ActivityBar message={message} now={now} /> : null}
        {message.status === "error" ? (
          <p className="builder-error" role="alert">
            <Icon name="warning-circle" size={16} />
            <span>{message.error ?? "The agent stopped with an error."}</span>
          </p>
        ) : null}
        {message.status === "cancelled" ? <p className="builder-meta">Stopped.</p> : null}
        {message.status === "done" && message.durationMs !== undefined ? (
          <p className="builder-meta">
            {seconds(message.durationMs)}
            {message.costUsd ? ` · $${message.costUsd.toFixed(3)}` : ""}
          </p>
        ) : null}
      </div>
    </article>
  );
}

function EmptyState({ onPick, ready }: { onPick: (prompt: string) => void; ready: boolean }) {
  return (
    <div className="builder-empty">
      <AlfredOrb size="hero" />
      <h2>What should Alfred automate?</h2>
      <p>
        Describe it in plain words. The agent designs the workflow, builds it on your canvas through Alfred's API,
        and can test-run it for you.
      </p>
      <div className="builder-starters">
        {STARTERS.map((starter, i) => (
          <button
            key={starter.title}
            type="button"
            className="builder-starter"
            style={{ "--i": i } as CSSProperties}
            disabled={!ready}
            onClick={() => onPick(starter.prompt)}
          >
            <Icon name={starter.icon} size={18} />
            <span>
              <strong>{starter.title}</strong>
              <span>{starter.prompt}</span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

type Props = {
  onOpenWorkflow: (workflowId: string) => void;
};

export function BuilderPage({ onOpenWorkflow }: Props) {
  const chat = useBuilderStore((s) => s.chat);
  const chats = useBuilderStore((s) => s.chats);
  const working = useBuilderStore((s) => s.working);
  const config = useBuilderStore((s) => s.config);
  const info = useBuilderStore((s) => s.info);
  const configError = useBuilderStore((s) => s.configError);
  const saveError = useBuilderStore((s) => s.saveError);
  const providerModels = useWorkflowStore((s) => s.providerModels);
  const { init, send, stop, newChat, openChat, deleteChat, setAgent, setModel, saveConfig } =
    useBuilderStore.getState();
  const [draft, setDraft] = useState("");
  const [rail, setRail] = useState<"chats" | "setup" | null>("chats");
  const threadRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const pinned = useRef(true);
  const now = useNow(working);
  const messages = chat.messages;
  const agent = info?.agents.find((a) => a.id === chat.agent);
  const ready = agent?.installed !== false;
  const catalog = providerModels.find((c) => c.provider === chat.agent && c.harness === "cli");
  const models = catalog?.models ?? [];

  useEffect(() => {
    void init();
    inputRef.current?.focus();
  }, [init]);

  // Follow the stream while the reader is at the bottom; let them scroll up.
  useLayoutEffect(() => {
    const thread = threadRef.current;
    if (thread && pinned.current) thread.scrollTop = thread.scrollHeight;
  }, [messages, now]);

  // A different chat starts at its latest message.
  useLayoutEffect(() => {
    pinned.current = true;
    const thread = threadRef.current;
    if (thread) thread.scrollTop = thread.scrollHeight;
  }, [chat.id]);

  useLayoutEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 200)}px`;
  }, [draft]);

  const submit = (text = draft) => {
    if (!text.trim() || working || !ready) return;
    pinned.current = true;
    setDraft("");
    void send(text);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      submit();
    }
  };

  const toggleRail = (tab: "chats" | "setup") => setRail((open) => (open === tab ? null : tab));

  return (
    <section className="settings-page builder-page" aria-label="Workflow Builder">
      <div className={["builder-layout", rail ? "" : "rail-closed"].filter(Boolean).join(" ")}>
        <div className="builder-chat">
          <header className="builder-header">
            <AlfredOrb busy={working} />
            <h1 className="builder-header-title">Workflow Builder</h1>
            <div className="builder-header-actions">
              <button
                type="button"
                className="ghost"
                disabled={working || messages.length === 0}
                onClick={() => {
                  newChat();
                  inputRef.current?.focus();
                }}
              >
                <Icon name="note-pencil" size={16} />
                New chat
              </button>
              <button
                type="button"
                className={["ghost", rail === "chats" ? "is-active" : ""].filter(Boolean).join(" ")}
                aria-pressed={rail === "chats"}
                onClick={() => toggleRail("chats")}
              >
                <Icon name="clock" size={16} />
                History
              </button>
              <button
                type="button"
                className={["ghost", rail === "setup" ? "is-active" : ""].filter(Boolean).join(" ")}
                aria-pressed={rail === "setup"}
                onClick={() => toggleRail("setup")}
              >
                <Icon name="sliders" size={16} />
                Setup
              </button>
            </div>
          </header>

          <div
            ref={threadRef}
            className="builder-thread"
            onScroll={(event) => {
              const el = event.currentTarget;
              pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
            }}
          >
            {messages.length === 0 ? (
              <EmptyState ready={ready} onPick={(prompt) => submit(prompt)} />
            ) : (
              <div className="builder-messages">
                {messages.map((message) =>
                  message.role === "user" ? (
                    <article key={message.id} className="builder-message is-user">
                      <div className="builder-bubble user-select-text">{message.text}</div>
                    </article>
                  ) : (
                    <AssistantBlock
                      key={message.id}
                      message={message}
                      agent={chat.agent}
                      now={now}
                      busy={working}
                      onOpenWorkflow={onOpenWorkflow}
                      onSend={submit}
                    />
                  ),
                )}
              </div>
            )}
          </div>

          <form
            className={["builder-composer", working ? "is-working" : ""].filter(Boolean).join(" ")}
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            {agent && !agent.installed ? (
              <p className="builder-error">
                <Icon name="warning-circle" size={16} />
                <span>Install {agent.label} and make sure it is on your PATH, or pick another agent.</span>
              </p>
            ) : null}
            {saveError ? (
              <p className="builder-error">
                <Icon name="warning-circle" size={16} />
                <span>{saveError}</span>
              </p>
            ) : null}
            <textarea
              ref={inputRef}
              rows={1}
              value={draft}
              placeholder={working ? "Alfred is working…" : "Describe a workflow, or ask to change one…"}
              aria-label="Message the Workflow Builder"
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={onKeyDown}
            />
            <div className="builder-composer-bar">
              <SelectControl
                density="compact"
                aria-label="Agent"
                title={messages.length > 0 ? "Start a new chat to switch agents" : undefined}
                value={chat.agent}
                disabled={working || messages.length > 0}
                onChange={(event) => setAgent(event.target.value as BuilderAgentId)}
              >
                {(info?.agents ?? [{ id: chat.agent, label: "Claude Code", installed: true }]).map((option) => (
                  <option key={option.id} value={option.id} disabled={!option.installed}>
                    {option.installed ? option.label : `${option.label} (not installed)`}
                  </option>
                ))}
              </SelectControl>
              <SelectControl
                density="compact"
                aria-label="Model"
                value={chat.model}
                disabled={working}
                onChange={(event) => setModel(event.target.value)}
              >
                <option value="">Default model</option>
                {models.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
                {chat.model && !models.some((m) => m.id === chat.model) ? (
                  <option value={chat.model}>{chat.model}</option>
                ) : null}
              </SelectControl>
              <span className="builder-composer-hint">Enter to send · Shift+Enter for a new line</span>
              {working ? (
                <button type="button" className="builder-send is-stop" aria-label="Stop" title="Stop" onClick={() => void stop()}>
                  <Icon name="stop" size={16} />
                </button>
              ) : (
                <button
                  type="submit"
                  className="builder-send"
                  aria-label="Send"
                  title="Send"
                  disabled={!draft.trim() || !ready}
                >
                  <Icon name="arrow-up" size={16} />
                </button>
              )}
            </div>
          </form>
        </div>

        {rail ? (
          <aside className="builder-rail" aria-label={rail === "chats" ? "Chat history" : "Agent setup"}>
            {rail === "chats" ? (
              <section className="builder-setup-section">
                <h3 className="builder-setup-title">Chats</h3>
                <BuilderChatList
                  chats={chats}
                  activeId={chat.id}
                  disabled={working}
                  onOpen={(id) => void openChat(id)}
                  onDelete={(id) => void deleteChat(id)}
                />
              </section>
            ) : (
              <BuilderSetupPanel info={info} agent={agent} config={config} error={configError} onSave={saveConfig} />
            )}
          </aside>
        ) : null}
      </div>
    </section>
  );
}
