import { expect, test } from "bun:test";
import type { StepInputSnapshot } from "../../types";
import { injectedFilePaths } from "./output-modal";

const snapshot = (kind: string, body: unknown) =>
  ({ version: 1, kind, summary: "", body, redacted: false }) as StepInputSnapshot;

test("lists the files of a File inject input only", () => {
  expect(injectedFilePaths(snapshot("file", { paths: ["/a.json", 3] }))).toEqual(["/a.json"]);
  expect(injectedFilePaths(snapshot("context", { paths: ["/a.json"] }))).toBeNull();
  expect(injectedFilePaths(snapshot("file", "text"))).toBeNull();
});
