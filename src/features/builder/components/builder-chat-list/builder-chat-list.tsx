import { AgentMark } from "../../../../components/agent-mark";
import { Icon } from "../../../../components/icon/icon";
import type { ChatSummary } from "../../api";

const RELATIVE = new Intl.RelativeTimeFormat(undefined, { numeric: "auto", style: "short" });

export function relativeTime(at: number, now: number): string {
  const seconds = Math.round((at - now) / 1000);
  const steps: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ["year", 31_536_000],
    ["month", 2_592_000],
    ["week", 604_800],
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  for (const [unit, size] of steps) {
    if (Math.abs(seconds) >= size) return RELATIVE.format(Math.round(seconds / size), unit);
  }
  return "just now";
}

type Props = {
  chats: ChatSummary[];
  activeId: string;
  disabled: boolean;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
};

export function BuilderChatList({ chats, activeId, disabled, onOpen, onDelete }: Props) {
  if (chats.length === 0) {
    return <p className="builder-setup-empty">Your conversations with the builder appear here.</p>;
  }
  const now = Date.now();
  return (
    <ul className="builder-chat-list" aria-label="Previous chats">
      {chats.map((chat) => (
        <li key={chat.id} className={chat.id === activeId ? "is-active" : undefined}>
          <button
            type="button"
            className="builder-chat-item"
            aria-current={chat.id === activeId ? "true" : undefined}
            disabled={disabled && chat.id !== activeId}
            onClick={() => onOpen(chat.id)}
          >
            <span className="builder-chat-title">{chat.title || "New chat"}</span>
            <span className="builder-chat-meta">
              <AgentMark provider={chat.agent} size={12} />
              {relativeTime(chat.updatedAt, now)}
            </span>
          </button>
          <button
            type="button"
            className="ghost builder-icon-button builder-remove"
            aria-label={`Delete chat ${chat.title}`}
            title="Delete chat"
            disabled={disabled && chat.id === activeId}
            onClick={() => onDelete(chat.id)}
          >
            <Icon name="trash" size={14} />
          </button>
        </li>
      ))}
    </ul>
  );
}
