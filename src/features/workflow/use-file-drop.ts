import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";
import { detectDesktopPlatform } from "../../platform";

/**
 * Tauri labels drop positions physical, but only Windows reports them that way;
 * macOS (AppKit points) and Linux (GTK coordinates) are already CSS pixels.
 */
export function dropPointToCss(
  position: { x: number; y: number },
  platform: string,
  scale: number,
): { x: number; y: number } {
  const divisor = platform === "windows" ? scale : 1;
  return { x: position.x / divisor, y: position.y / divisor };
}

/**
 * Accepts OS file drops on the card or modal containing the returned ref.
 * Hit-tests the top element, so a modal covering a card keeps the drop to itself.
 */
export function useFileDrop<T extends HTMLElement>(
  onDrop: (paths: string[]) => void,
  disabled = false,
) {
  const ref = useRef<T>(null);
  const [over, setOver] = useState(false);
  const onDropRef = useRef(onDrop);
  onDropRef.current = onDrop;

  useEffect(() => {
    if (disabled || !isTauri()) return;
    let unlisten: (() => void) | undefined;
    let alive = true;
    void getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "leave") {
          setOver(false);
          return;
        }
        const zone = ref.current?.closest(".wf-node, .modal") ?? ref.current;
        const { x, y } = dropPointToCss(
          payload.position,
          detectDesktopPlatform(),
          window.devicePixelRatio,
        );
        const top = document.elementFromPoint(x, y);
        const inside = Boolean(zone && top && zone.contains(top));
        if (payload.type === "drop") {
          setOver(false);
          if (inside) onDropRef.current(payload.paths);
        } else {
          setOver(inside);
        }
      })
      .then((stop) => {
        if (alive) unlisten = stop;
        else stop();
      });
    return () => {
      alive = false;
      unlisten?.();
      setOver(false);
    };
  }, [disabled]);

  return { ref, over };
}
