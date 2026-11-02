/**
 * Unsent composer text, one localStorage key per chat. Chats themselves are
 * files; a draft is per-window work in progress, so switching chats or leaving
 * the Builder never discards what was typed but not sent.
 */

const KEY_PREFIX = "alfred:builder-draft:";

export function readDraft(chatId: string): string {
  try {
    return localStorage.getItem(`${KEY_PREFIX}${chatId}`) ?? "";
  } catch {
    return "";
  }
}

export function writeDraft(chatId: string, text: string): void {
  try {
    if (text) localStorage.setItem(`${KEY_PREFIX}${chatId}`, text);
    else localStorage.removeItem(`${KEY_PREFIX}${chatId}`);
  } catch {
    // The composer still works when storage is unavailable or full.
  }
}
