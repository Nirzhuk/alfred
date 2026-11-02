import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { InputAttachment } from "./types";

const IMAGE_EXTENSIONS = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "bmp",
  "svg",
  "heic",
  "heif",
  "avif",
]);

function newAttachmentId() {
  return crypto.randomUUID();
}

function asPaths(value: string | string[] | null): string[] {
  if (!value) return [];
  return Array.isArray(value) ? value : [value];
}

export function attachmentFileName(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function attachmentExtension(path: string): string {
  const name = attachmentFileName(path);
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "";
  return name.slice(dot + 1).toLowerCase();
}

export function isImageAttachmentPath(path: string): boolean {
  return IMAGE_EXTENSIONS.has(attachmentExtension(path));
}

/** Local path → WebView URL for thumbnails (requires asset protocol). */
export function attachmentAssetUrl(path: string): string {
  return convertFileSrc(path);
}

export async function pickFileAttachments(): Promise<InputAttachment[]> {
  const picked = await open({
    multiple: true,
    directory: false,
    title: "Attach files to Input",
  });
  return asPaths(picked).map((path) => ({
    id: newAttachmentId(),
    path,
    kind: "file" as const,
  }));
}

export async function pickFolderAttachments(): Promise<InputAttachment[]> {
  const picked = await open({
    multiple: true,
    directory: true,
    title: "Attach folders to Input",
  });
  return asPaths(picked).map((path) => ({
    id: newAttachmentId(),
    path,
    kind: "folder" as const,
  }));
}

export async function attachmentsFromDroppedPaths(
  paths: string[],
): Promise<InputAttachment[]> {
  const folders = await invoke<boolean[]>("paths_are_folders", { paths });
  return paths.map((path, index) => ({
    id: newAttachmentId(),
    path,
    kind: folders[index] ? ("folder" as const) : ("file" as const),
  }));
}

export function shortAttachmentPath(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/").filter(Boolean);
  if (parts.length <= 2) return normalized;
  return `…/${parts.slice(-2).join("/")}`;
}

export function mergeAttachments(
  current: InputAttachment[] | undefined,
  incoming: InputAttachment[],
): InputAttachment[] {
  const next = [...(current ?? [])];
  const seen = new Set(next.map((a) => a.path));
  for (const item of incoming) {
    if (seen.has(item.path)) continue;
    seen.add(item.path);
    next.push(item);
  }
  return next;
}
