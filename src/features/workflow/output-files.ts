const MAX_OUTPUT_FILES = 10;

const SEGMENT = String.raw`[\p{L}\p{N}_.@+\-]+`;
const EXTENSION = String.raw`\.[A-Za-z0-9]{1,8}`;

// ponytail: an unquoted path with spaces is not detected, because nothing marks
// where it ends. Checking candidates against the disk in Rust would fix that.
const BARE_PATH = new RegExp(
  String.raw`(?<![\p{L}\p{N}_./:~\\\-])(?:~/(?:${SEGMENT}/)*|/(?:${SEGMENT}/)+|[A-Za-z]:\\(?:${SEGMENT}\\)*)${SEGMENT}${EXTENSION}(?![\p{L}\p{N}_/\\\-])`,
  "gu",
);
const QUOTED_PATH = new RegExp(
  String.raw`["'\x60]((?:~/|/(?!/)|[A-Za-z]:\\)[^"'\x60\n]*${EXTENSION})["'\x60]`,
  "gu",
);

/**
 * Absolute file paths a step mentioned in its output, in order. Requiring an
 * extension and a full path keeps "and/or", URLs, and API routes out.
 */
export function outputFilePaths(text: string): string[] {
  const found = [
    ...[...text.matchAll(BARE_PATH)].map((m) => ({ at: m.index, path: m[0] })),
    ...[...text.matchAll(QUOTED_PATH)].map((m) => ({ at: m.index, path: m[1] })),
  ].sort((a, b) => a.at - b.at);
  return [...new Set(found.map((item) => item.path))].slice(0, MAX_OUTPUT_FILES);
}

/** Opening these runs them, so a path read from output text only gets "Open folder". */
const RUNS_WHEN_OPENED = new Set(
  "action app appimage bash bat cmd com command cpl csh deb desktop dmg exe fileloc fish hta inetloc jar js jse ksh lnk mpkg msi osax pif pkg ps1 py pyw rb reg rpm scpt scr sh terminal tool url vbe vbs webloc workflow wsf wsh zsh".split(
    " ",
  ),
);

export function isSafeToOpen(path: string): boolean {
  const extension = path.split(".").pop()?.toLowerCase() ?? "";
  return !RUNS_WHEN_OPENED.has(extension);
}
