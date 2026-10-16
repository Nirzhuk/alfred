import { expect, test } from "bun:test";
import { previewText } from "./node-run-result";

test("shows only the first output paragraph in the compact card", () => {
  const output =
    'Telegram accepted the notification\n\n{\n  "acceptedAt": "2026-08-17T16:26:57Z"\n}';
  expect(previewText(output)).toBe("Telegram accepted the notification");
});

test("flattens and bounds compact output text", () => {
  expect(previewText("Hi!\nHow can I help you today?")).toBe(
    "Hi! How can I help you today?",
  );
  expect(previewText("a".repeat(200), 10)).toBe(`${"a".repeat(10)}…`);
});
