/**
 * Images for the Workflow Builder: whatever the user drops, pastes, or picks
 * becomes a JPEG/PNG/GIF/WebP the agent CLIs accept, plus a small thumbnail
 * the chat keeps in its history.
 */

export const MAX_IMAGES = 6;
/** Mirrors the Rust cap: Claude rejects images over 5 MB of base64. */
export const MAX_IMAGE_BYTES = 3_750_000;
const TARGET_BYTES = 3_400_000;
const MAX_EDGE = 2048;
const THUMB_EDGE = 320;

export const SENDABLE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
export const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "bmp", "tif", "tiff"];

export type PendingImage = {
  id: string;
  name: string;
  status: "loading" | "ready" | "error";
  mediaType?: string;
  /** Base64 without the data-URL prefix. */
  data?: string;
  /** Small JPEG data URL for the composer and the saved chat. */
  thumb?: string;
  error?: string;
};

export function isImagePath(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.includes(ext);
}

/** Send as-is only when the agents accept the type and it fits every limit. */
export function needsReencode(type: string, bytes: number, width: number, height: number): boolean {
  return !SENDABLE_TYPES.includes(type) || bytes > TARGET_BYTES || Math.max(width, height) > MAX_EDGE;
}

/** Scale so the longest edge is at most `edge`, never upscaling. */
export function fitWithin(width: number, height: number, edge: number): { width: number; height: number } {
  const scale = Math.min(1, edge / Math.max(width, height));
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) };
}

export function base64ToBlob(data: string, type: string): Blob {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes], { type });
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

function draw(bitmap: ImageBitmap, edge: number): HTMLCanvasElement {
  const size = fitWithin(bitmap.width, bitmap.height, edge);
  const canvas = document.createElement("canvas");
  canvas.width = size.width;
  canvas.height = size.height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas unavailable");
  // theme-exempt: JPEG has no alpha; transparent pixels become white, not black.
  context.fillStyle = "#ffffff";
  context.fillRect(0, 0, size.width, size.height);
  context.drawImage(bitmap, 0, 0, size.width, size.height);
  return canvas;
}

function toBlob(canvas: HTMLCanvasElement, quality: number): Promise<Blob> {
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("Encode failed"))), "image/jpeg", quality),
  );
}

/** Decode, resize when needed, and thumbnail one image. */
export async function prepareImage(blob: Blob): Promise<{ mediaType: string; data: string; thumb: string }> {
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(blob);
  } catch {
    throw new Error("This image format can't be read.");
  }
  try {
    const thumb = draw(bitmap, THUMB_EDGE).toDataURL("image/jpeg", 0.8);
    if (!needsReencode(blob.type, blob.size, bitmap.width, bitmap.height)) {
      return { mediaType: blob.type, data: await blobToBase64(blob), thumb };
    }
    let edge = MAX_EDGE;
    for (const quality of [0.9, 0.82, 0.72, 0.6]) {
      const encoded = await toBlob(draw(bitmap, edge), quality);
      if (encoded.size <= TARGET_BYTES) {
        return { mediaType: "image/jpeg", data: await blobToBase64(encoded), thumb };
      }
      edge = Math.round(edge * 0.8);
    }
    throw new Error("This image is too large to send.");
  } finally {
    bitmap.close();
  }
}
