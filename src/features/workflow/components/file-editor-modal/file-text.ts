/** A textarea rewrites CRLF to LF, so remember it to keep the file's line endings. */
export function fromDiskText(text: string): { text: string; crlf: boolean } {
  return { text: text.replace(/\r\n/g, "\n"), crlf: text.includes("\r\n") };
}

export function toDiskText(text: string, crlf: boolean): string {
  return crlf ? text.replace(/\n/g, "\r\n") : text;
}
