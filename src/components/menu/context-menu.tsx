import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type AriaRole,
  type ReactNode,
  type TransitionEvent,
} from "react";
import { Menu } from "./menu";
import { useDismissable } from "./use-dismissable";

const EXIT_MS = 160;

const ContextMenuCloseContext = createContext<(() => void) | null>(null);

/** Close the nearest `ContextMenu` (respects exit animation when enabled). */
export function useContextMenuClose() {
  const close = useContext(ContextMenuCloseContext);
  if (!close) {
    throw new Error("useContextMenuClose must be used within ContextMenu");
  }
  return close;
}

type Props = {
  x: number;
  y: number;
  onClose: () => void;
  children: ReactNode;
  className?: string;
  /** Soft enter/exit like the workflow list menu. */
  animated?: boolean;
  zIndex?: number;
  role?: AriaRole;
  "aria-label"?: string;
};

/**
 * Fixed-position context menu at screen coordinates.
 * Clamps to the viewport and dismisses on outside click / Escape.
 */
export function ContextMenu({
  x,
  y,
  onClose,
  children,
  className,
  animated = false,
  zIndex = 40,
  role = "menu",
  "aria-label": ariaLabel,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const closedRef = useRef(false);
  const [pos, setPos] = useState({ left: x, top: y });
  const [state, setState] = useState<"closed" | "open" | "closing">(
    animated ? "closed" : "open",
  );

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const pad = 8;
    setPos({
      left: Math.max(pad, Math.min(x, window.innerWidth - rect.width - pad)),
      top: Math.max(pad, Math.min(y, window.innerHeight - rect.height - pad)),
    });
  }, [x, y]);

  useEffect(() => {
    if (!animated) return;
    const id = requestAnimationFrame(() => {
      requestAnimationFrame(() => setState("open"));
    });
    return () => cancelAnimationFrame(id);
  }, [animated]);

  const finishClose = useCallback(() => {
    if (closedRef.current) return;
    closedRef.current = true;
    onClose();
  }, [onClose]);

  const requestClose = useCallback(() => {
    if (closedRef.current) return;
    if (!animated) {
      finishClose();
      return;
    }
    setState((current) => (current === "closing" ? current : "closing"));
  }, [animated, finishClose]);

  useEffect(() => {
    if (state !== "closing") return;
    const t = window.setTimeout(finishClose, EXIT_MS);
    return () => window.clearTimeout(t);
  }, [state, finishClose]);

  useDismissable(true, ref, requestClose);

  const onTransitionEnd = (event: TransitionEvent<HTMLDivElement>) => {
    if (!animated) return;
    if (event.target !== ref.current) return;
    if (event.propertyName !== "opacity") return;
    if (state === "closing") finishClose();
  };

  return (
    <ContextMenuCloseContext.Provider value={requestClose}>
      <Menu
        ref={ref}
        role={role}
        aria-label={ariaLabel}
        placement="fixed"
        animated={animated}
        state={state}
        className={className}
        style={{ left: pos.left, top: pos.top, zIndex }}
        onMouseDown={(e) => e.stopPropagation()}
        onTransitionEnd={onTransitionEnd}
      >
        {children}
      </Menu>
    </ContextMenuCloseContext.Provider>
  );
}
