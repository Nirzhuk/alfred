import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { readDraft, writeDraft } from "./drafts";

const stored = new Map<string, string>();

beforeEach(() => {
  stored.clear();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => stored.get(key) ?? null,
      setItem: (key: string, value: string) => void stored.set(key, value),
      removeItem: (key: string) => void stored.delete(key),
    },
  });
});

afterEach(() => {
  Reflect.deleteProperty(globalThis, "localStorage");
});

describe("unsent composer drafts", () => {
  test("each chat keeps its own draft", () => {
    writeDraft("chat-a", "Ship the release notes");
    writeDraft("chat-b", "Review the open pull request");

    expect(readDraft("chat-a")).toBe("Ship the release notes");
    expect(readDraft("chat-b")).toBe("Review the open pull request");
    expect(readDraft("chat-c")).toBe("");
  });

  test("clearing a draft leaves no blank entry behind", () => {
    writeDraft("chat-a", "Half a thought");
    writeDraft("chat-a", "");

    expect(readDraft("chat-a")).toBe("");
    expect(stored.size).toBe(0);
  });
});
