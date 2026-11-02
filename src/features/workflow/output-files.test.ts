import { expect, test } from "bun:test";
import { isSafeToOpen, outputFilePaths } from "./output-files";

test("finds the file in real step output", () => {
  const output = `CV ready: /Users/nirzhuk/Documents/cv-builder/applications/2026-11-02-nebius-senior-frontend-engineer-user-interface/Joan_Serrano_Perez__CV.pdf
Pages: 2
Why these blocks: Senior React/TypeScript role, matched with architecture.
(exit 0)`;
  expect(outputFilePaths(output)).toEqual([
    "/Users/nirzhuk/Documents/cv-builder/applications/2026-11-02-nebius-senior-frontend-engineer-user-interface/Joan_Serrano_Perez__CV.pdf",
  ]);
});

test("reads home, Windows, accented, and quoted paths with spaces", () => {
  expect(
    outputFilePaths(
      'Saved ~/Documents/Currículum.pdf, C:\\Users\\me\\report.docx and `/Users/me/My Docs/final report.pdf`. Also "/tmp/out.json".',
    ),
  ).toEqual([
    "~/Documents/Currículum.pdf",
    "C:\\Users\\me\\report.docx",
    "/Users/me/My Docs/final report.pdf",
    "/tmp/out.json",
  ]);
});

test("ignores trailing punctuation and repeats", () => {
  expect(outputFilePaths("Wrote /tmp/a/b.tar.gz. Then /tmp/a/b.tar.gz again (/tmp/c.txt)")).toEqual([
    "/tmp/a/b.tar.gz",
    "/tmp/c.txt",
  ]);
});

test("leaves URLs, prose, MIME types, and folders alone", () => {
  expect(
    outputFilePaths(
      'See https://example.com/a/b.pdf and "//cdn.example.com/x.js", and/or 1/2.5, e.g. application/vnd.ms.pdf in /Users/me/project/',
    ),
  ).toEqual([]);
});

test("caps a noisy log", () => {
  const log = Array.from({ length: 30 }, (_, i) => `/tmp/file-${i}.txt`).join("\n");
  expect(outputFilePaths(log)).toHaveLength(10);
});

test("programs from output text never get an Open button", () => {
  for (const path of ["/tmp/x.command", "/tmp/X.APP", "C:\\x\\run.exe", "/tmp/a.sh"]) {
    expect(isSafeToOpen(path)).toBe(false);
  }
  for (const path of ["/tmp/cv.pdf", "/tmp/data.csv", "/tmp/page.html"]) {
    expect(isSafeToOpen(path)).toBe(true);
  }
});
