import { afterAll, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: { crypto: globalThis.crypto },
});

afterAll(() => {
  clearMocks();
  if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow);
  else Reflect.deleteProperty(globalThis, "window");
});

mockIPC((command) => {
  if (command === "builder_send") {
    return { sessionId: "session-1", cancelled: false, error: null };
  }
  if (command === "builder_list_chats") return [];
  return null;
});

// Import after installing the IPC mock so the store's first command call is
// answered by the fixture rather than the native backend.
const { useBuilderStore } = await import("./store");

function startChat(viewing: boolean): string {
  useBuilderStore.setState({
    unseenChatIds: [],
    viewingChatId: null,
    working: false,
  });
  useBuilderStore.getState().newChat();
  const id = useBuilderStore.getState().chat.id;
  if (viewing) useBuilderStore.setState({ viewingChatId: id });
  return id;
}

test("a reply that finishes while the chat is closed becomes unseen", async () => {
  const id = startChat(false);

  await useBuilderStore.getState().send("Plan the release");

  expect(useBuilderStore.getState().unseenChatIds).toEqual([id]);

  useBuilderStore.getState().markChatSeen(id);
  expect(useBuilderStore.getState().unseenChatIds).toEqual([]);
});

test("a reply that finishes on screen is already seen", async () => {
  startChat(true);

  await useBuilderStore.getState().send("Plan the release");

  expect(useBuilderStore.getState().unseenChatIds).toEqual([]);
});
