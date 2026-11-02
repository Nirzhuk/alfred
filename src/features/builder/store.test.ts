import { afterEach, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

/**
 * A launch where the composer last held text in a chat that was never sent to
 * disk: the chat has no file, so its id and draft are the only way back.
 */

const stored = new Map<string, string>();
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
const originalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: { crypto: globalThis.crypto },
});
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => void stored.set(key, value),
    removeItem: (key: string) => void stored.delete(key),
  },
});

stored.set(
  "alfred:builder-prefs",
  JSON.stringify({ lastChatId: "unsaved-chat", agent: "claude_code", model: "" }),
);
stored.set("alfred:builder-draft:unsaved-chat", "Keep this draft");

mockIPC(
  (command) => {
    if (command === "builder_list_chats") return [];
    if (command === "builder_get_config") return { rules: [], mcpServers: [] };
    if (command === "builder_info") {
      return {
        agents: [{ id: "claude_code", label: "Claude Code", installed: true }],
        mcpReady: true,
        tools: [],
      };
    }
    return null;
  },
  { shouldMockEvents: true },
);

const { readDraft } = await import("./drafts");
const { useBuilderStore } = await import("./store");

afterEach(() => {
  clearMocks();
  for (const [key, descriptor] of [
    ["window", originalWindow],
    ["localStorage", originalStorage],
  ] as const) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor);
    else Reflect.deleteProperty(globalThis, key);
  }
});

test("an unsaved chat reopens with its unsent draft after a launch", async () => {
  await useBuilderStore.getState().init();

  expect(useBuilderStore.getState().chat.id).toBe("unsaved-chat");
  expect(readDraft(useBuilderStore.getState().chat.id)).toBe("Keep this draft");
});
