import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { useWorkflowStore } from "../workflow/store";
import * as api from "./api";
import {
  applyStreamEvent,
  isWorking,
  newAssistantMessage,
  type AssistantMessage,
  type ChatMessage,
} from "./stream";

/** Only small per-viewer preferences live in the browser; chats are files. */
const PREFS_KEY = "alfred:builder-prefs";
const TITLE_CHARS = 64;

export type BuilderChat = {
  id: string;
  title: string;
  agent: api.BuilderAgentId;
  model: string;
  sessionId: string | null;
  createdAt: number;
  updatedAt: number;
  messages: ChatMessage[];
};

export type SendImage = api.BuilderImage & { thumb: string };

/** What the agent reads when an image arrives without words. */
export const IMAGE_ONLY_PROMPT = "Take a look at the attached image.";

type Prefs = { lastChatId: string | null; agent: api.BuilderAgentId; model: string };

function readPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) return { lastChatId: null, agent: "claude_code", model: "", ...JSON.parse(raw) };
  } catch {
    // Defaults below.
  }
  return { lastChatId: null, agent: "claude_code", model: "" };
}

function writePrefs(prefs: Prefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Preferences are a convenience.
  }
}

export function chatTitle(messages: ChatMessage[]): string {
  const first = messages.find((m) => m.role === "user");
  const text = first?.role === "user" ? first.text.replace(/\s+/g, " ").trim() : "";
  if (!text) return first?.role === "user" && first.images?.length ? "Image" : "New chat";
  return text.length > TITLE_CHARS ? `${text.slice(0, TITLE_CHARS - 1)}…` : text;
}

function emptyChat(agent: api.BuilderAgentId, model: string): BuilderChat {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    title: "New chat",
    agent,
    model,
    sessionId: null,
    createdAt: now,
    updatedAt: now,
    messages: [],
  };
}

/** A turn cannot survive a restart; settle anything saved mid-flight. */
function settle(chat: BuilderChat): BuilderChat {
  return {
    ...chat,
    messages: chat.messages.map((m) =>
      m.role === "assistant" && isWorking(m.status) ? { ...m, status: "cancelled" as const } : m,
    ),
  };
}

type BuilderStore = {
  chat: BuilderChat;
  chats: api.ChatSummary[];
  config: api.BuilderConfig;
  info: api.BuilderInfo | null;
  configError: string | null;
  saveError: string | null;
  working: boolean;
  init: () => Promise<void>;
  send: (text: string, images?: SendImage[]) => Promise<void>;
  stop: () => Promise<void>;
  newChat: () => void;
  openChat: (id: string) => Promise<void>;
  deleteChat: (id: string) => Promise<void>;
  setAgent: (agent: api.BuilderAgentId) => void;
  setModel: (model: string) => void;
  saveConfig: (config: api.BuilderConfig) => Promise<boolean>;
};

const prefs = readPrefs();
let activeAssistantId: string | null = null;
let initialized = false;

export const useBuilderStore = create<BuilderStore>((set, get) => {
  const rememberPrefs = () => {
    const { chat } = get();
    writePrefs({
      lastChatId: chat.messages.length ? chat.id : null,
      agent: chat.agent,
      model: chat.model,
    });
  };

  /** Write the open chat to disk and refresh the history list. */
  const persist = async () => {
    const chat = get().chat;
    if (chat.messages.length === 0) return;
    const saved = { ...chat, title: chatTitle(chat.messages), updatedAt: Date.now() };
    set({ chat: saved });
    try {
      await api.saveBuilderChat(saved);
      const chats = await api.listBuilderChats();
      set({ chats, saveError: null });
    } catch (error) {
      set({ saveError: String(error) });
    }
    rememberPrefs();
  };

  const updateAssistant = (id: string, change: (m: AssistantMessage) => AssistantMessage) => {
    set((state) => ({
      chat: {
        ...state.chat,
        messages: state.chat.messages.map((m) =>
          m.role === "assistant" && m.id === id ? change(m) : m,
        ),
      },
    }));
  };

  return {
    chat: emptyChat(prefs.agent, prefs.model),
    chats: [],
    config: { rules: [], mcpServers: [] },
    info: null,
    configError: null,
    saveError: null,
    working: false,

    init: async () => {
      await installBridges();
      const [config, info, chats] = await Promise.all([
        api.getBuilderConfig().catch(() => get().config),
        api.getBuilderInfo().catch(() => null),
        api.listBuilderChats().catch(() => [] as api.ChatSummary[]),
      ]);
      set({ config, info, chats });
      if (initialized) return;
      initialized = true;
      // Reopen where the user left off, once per launch.
      if (prefs.lastChatId && get().chat.messages.length === 0) {
        await get().openChat(prefs.lastChatId);
      }
      // Fall back to an installed agent when the remembered one is missing.
      const agents = info?.agents ?? [];
      if (agents.length && !agents.some((a) => a.id === get().chat.agent && a.installed)) {
        const installed = agents.find((a) => a.installed);
        if (installed && get().chat.messages.length === 0) get().setAgent(installed.id);
      }
    },

    send: async (text, images = []) => {
      const typed = text.trim();
      if ((!typed && images.length === 0) || get().working) return;
      const message = typed || IMAGE_ONLY_PROMPT;
      const assistantId = crypto.randomUUID();
      const now = Date.now();
      set((state) => ({
        working: true,
        chat: {
          ...state.chat,
          messages: [
            ...state.chat.messages,
            {
              id: crypto.randomUUID(),
              role: "user",
              text: typed,
              at: now,
              images: images.length ? images.map((image) => image.thumb) : undefined,
            },
            newAssistantMessage(assistantId, now),
          ],
        },
      }));
      activeAssistantId = assistantId;
      void persist();
      const { chat } = get();
      try {
        const result = await api.sendBuilderMessage({
          chatId: chat.id,
          message,
          agent: chat.agent,
          model: chat.model || null,
          sessionId: chat.sessionId,
          images: images.map(({ mediaType, data }) => ({ mediaType, data })),
        });
        if (get().chat.id === chat.id) {
          set((state) => ({ chat: { ...state.chat, sessionId: result.sessionId ?? state.chat.sessionId } }));
          updateAssistant(assistantId, (m) =>
            isWorking(m.status)
              ? applyStreamEvent(
                  m,
                  { type: "alfred", subtype: "finished", cancelled: result.cancelled, error: result.error },
                  Date.now(),
                )
              : m,
          );
        }
      } catch (error) {
        updateAssistant(assistantId, (m) => ({ ...m, status: "error", error: String(error) }));
      } finally {
        activeAssistantId = null;
        set({ working: false });
        await persist();
      }
    },

    stop: async () => {
      await api.cancelBuilderTurn(get().chat.id).catch(() => false);
    },

    newChat: () => {
      if (get().working) return;
      const { agent, model } = get().chat;
      set({ chat: emptyChat(agent, model) });
      rememberPrefs();
    },

    openChat: async (id) => {
      if (get().working || get().chat.id === id) return;
      try {
        const chat = settle(await api.loadBuilderChat<BuilderChat>(id));
        set({ chat: { ...chat, agent: chat.agent ?? "claude_code", model: chat.model ?? "" } });
        rememberPrefs();
      } catch (error) {
        set({ saveError: String(error) });
        set({ chats: await api.listBuilderChats().catch(() => get().chats) });
      }
    },

    deleteChat: async (id) => {
      if (get().working && get().chat.id === id) return;
      await api.deleteBuilderChat(id).catch(() => undefined);
      if (get().chat.id === id) get().newChat();
      set({ chats: get().chats.filter((c) => c.id !== id) });
    },

    // The agent belongs to a chat's session, so it is chosen before the
    // first message; the page disables the picker afterwards.
    setAgent: (agent) => {
      if (get().chat.messages.length > 0) return;
      set((state) => ({ chat: { ...state.chat, agent, model: "" } }));
      rememberPrefs();
    },

    setModel: (model) => {
      set((state) => ({ chat: { ...state.chat, model } }));
      rememberPrefs();
    },

    saveConfig: async (config) => {
      try {
        const next = await api.saveBuilderConfig(config);
        set({ config: next, configError: null });
        return true;
      } catch (error) {
        set({ configError: String(error) });
        return false;
      }
    },
  };
});

let bridges: Promise<void> | null = null;

/** Once per window: stream events, workflow refreshes, config changes. */
function installBridges(): Promise<void> {
  bridges ??= Promise.all([
    listen<{ chatId: string; event: unknown }>(api.STREAM_EVENT, ({ payload }) => {
      const id = activeAssistantId;
      if (!id || payload.chatId !== useBuilderStore.getState().chat.id) return;
      const now = Date.now();
      useBuilderStore.setState((state) => ({
        chat: {
          ...state.chat,
          messages: state.chat.messages.map((m) =>
            m.role === "assistant" && m.id === id ? applyStreamEvent(m, payload.event, now) : m,
          ),
        },
      }));
    }),
    listen<{ workflowId: string }>(api.WORKFLOW_CHANGED_EVENT, ({ payload }) => {
      void useWorkflowStore
        .getState()
        .loadWorkflows()
        .then(() => {
          const current = useWorkflowStore.getState();
          if (current.activeWorkflowId === payload.workflowId && !current.dirty) {
            void current.selectWorkflow(payload.workflowId);
          }
        });
    }),
    listen<api.BuilderConfig>(api.CONFIG_CHANGED_EVENT, ({ payload }) => {
      useBuilderStore.setState({ config: payload });
    }),
  ]).then(() => undefined);
  return bridges;
}
