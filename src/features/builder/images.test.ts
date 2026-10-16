import { expect, test } from "bun:test";
import { base64ToBlob, fitWithin, isImagePath, needsReencode } from "./images";

test("accepts image extensions only", () => {
  expect(isImagePath("/Users/me/Desktop/Screenshot 2026.PNG")).toBe(true);
  expect(isImagePath("/tmp/photo.heic")).toBe(true);
  expect(isImagePath("/tmp/notes.pdf")).toBe(false);
  expect(isImagePath("/tmp/no-extension")).toBe(false);
});

test("keeps small supported images untouched and re-encodes the rest", () => {
  expect(needsReencode("image/png", 400_000, 1200, 800)).toBe(false);
  expect(needsReencode("image/png", 6_000_000, 1200, 800)).toBe(true);
  expect(needsReencode("image/png", 400_000, 3024, 1964)).toBe(true);
  expect(needsReencode("image/heic", 400_000, 800, 600)).toBe(true);
  expect(needsReencode("image/bmp", 100, 10, 10)).toBe(true);
});

test("fits the longest edge without upscaling", () => {
  expect(fitWithin(4096, 2048, 2048)).toEqual({ width: 2048, height: 1024 });
  expect(fitWithin(300, 900, 320)).toEqual({ width: 107, height: 320 });
  expect(fitWithin(100, 50, 2048)).toEqual({ width: 100, height: 50 });
});

test("decodes base64 into a typed blob", async () => {
  const blob = base64ToBlob(btoa("hello"), "image/png");
  expect(blob.type).toBe("image/png");
  expect(await blob.text()).toBe("hello");
});
