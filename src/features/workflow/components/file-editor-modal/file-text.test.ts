import { describe, expect, test } from "bun:test";
import { fromDiskText, toDiskText } from "./file-text";

describe("file editor line endings", () => {
  test("round-trips CRLF files", () => {
    const { text, crlf } = fromDiskText("a\r\nb\r\n");
    expect(text).toBe("a\nb\n");
    expect(toDiskText(text, crlf)).toBe("a\r\nb\r\n");
  });

  test("leaves LF files unchanged", () => {
    const { text, crlf } = fromDiskText("a\nb");
    expect(crlf).toBe(false);
    expect(toDiskText(text, crlf)).toBe("a\nb");
  });
});
