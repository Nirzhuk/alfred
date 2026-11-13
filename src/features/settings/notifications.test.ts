import { expect, test } from "bun:test";
import { CONDITION_SKIP_SOUND_INTERVAL_MS } from "./feedback-sounds";
import {
  playConditionSkippedSound,
  resolveNotificationSounds,
} from "./notifications";

test("a pre-split sound seeds both success and error", () => {
  expect(
    resolveNotificationSounds({ legacy: "ping", success: null, error: null }),
  ).toEqual({ success: "ping", error: "ping" });
});

test("explicit success and error sounds win independently", () => {
  expect(
    resolveNotificationSounds({
      legacy: "ping",
      success: "chime",
      error: "pop",
    }),
  ).toEqual({ success: "chime", error: "pop" });
});

test("invalid stored values fall back to the legacy sound or system", () => {
  expect(
    resolveNotificationSounds({
      legacy: "pop",
      success: "loud",
      error: null,
    }),
  ).toEqual({ success: "pop", error: "pop" });

  expect(
    resolveNotificationSounds({
      legacy: "loud",
      success: null,
      error: "not-a-sound",
    }),
  ).toEqual({ success: "system", error: "system" });
});

test("a skipped branch sounds once per burst", () => {
  const start = 1_000_000;
  expect(playConditionSkippedSound(start)).toBe(true);
  expect(
    playConditionSkippedSound(start + CONDITION_SKIP_SOUND_INTERVAL_MS - 1),
  ).toBe(false);
  expect(
    playConditionSkippedSound(start + CONDITION_SKIP_SOUND_INTERVAL_MS),
  ).toBe(true);
});
