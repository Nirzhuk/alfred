import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type AriaRole,
  type ButtonHTMLAttributes,
  type CSSProperties,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { Menu } from "./menu";

type DropdownMenuContextValue = {
  open: boolean;
  setOpen: (open: boolean) => void;
  menuId: string;
  triggerRef: React.RefObject<HTMLButtonElement | null>;
};

const DropdownMenuContext = createContext<DropdownMenuContextValue | null>(
  null,
);

function useDropdownMenu() {
  const ctx = useContext(DropdownMenuContext);
  if (!ctx) {
    throw new Error("DropdownMenu components must be used within DropdownMenu");
  }
  return ctx;
}

type DropdownMenuProps = {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: ReactNode;
  className?: string;
};

/** Controlled or uncontrolled dropdown shell (trigger + content). */
export function DropdownMenu({
  open: openProp,
  onOpenChange,
  children,
  className,
}: DropdownMenuProps) {
  const [uncontrolled, setUncontrolled] = useState(false);
  const controlled = openProp !== undefined;
  const open = controlled ? openProp : uncontrolled;
  const setOpen = useCallback(
    (next: boolean) => {
      if (!controlled) setUncontrolled(next);
      onOpenChange?.(next);
    },
    [controlled, onOpenChange],
  );
  const menuId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);

  const value = useMemo(
    () => ({ open, setOpen, menuId, triggerRef }),
    [open, setOpen, menuId],
  );

  return (
    <DropdownMenuContext.Provider value={value}>
      <div className={["ui-dropdown", className].filter(Boolean).join(" ")}>
        {children}
      </div>
    </DropdownMenuContext.Provider>
  );
}

type TriggerProps = ButtonHTMLAttributes<HTMLButtonElement>;

export function DropdownMenuTrigger({
  className,
  onClick,
  children,
  ...rest
}: TriggerProps) {
  const { open, setOpen, menuId, triggerRef } = useDropdownMenu();
  return (
    <button
      ref={triggerRef}
      type="button"
      className={className}
      aria-haspopup="menu"
      aria-expanded={open}
      aria-controls={menuId}
      onClick={(e) => {
        setOpen(!open);
        onClick?.(e);
      }}
      {...rest}
    >
      {children}
    </button>
  );
}

type ContentProps = {
  className?: string;
  role?: AriaRole;
  /** Horizontal alignment relative to the trigger. */
  align?: "start" | "end";
  /** Side of the trigger to open toward. */
  side?: "top" | "bottom";
  children: ReactNode;
  "aria-label"?: string;
};

type Coords = { top: number; left: number };

export function DropdownMenuContent({
  className,
  role = "menu",
  align = "end",
  side = "bottom",
  children,
  "aria-label": ariaLabel,
}: ContentProps) {
  const { open, menuId, triggerRef, setOpen } = useDropdownMenu();
  const menuRef = useRef<HTMLDivElement>(null);
  const [coords, setCoords] = useState<Coords | null>(null);

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current;
    const menu = menuRef.current;
    if (!trigger) return;

    const rect = trigger.getBoundingClientRect();
    const menuWidth = menu?.offsetWidth || 200;
    const menuHeight = menu?.offsetHeight || 0;
    const gap = 6;
    const pad = 8;

    let top =
      side === "top" ? rect.top - gap - menuHeight : rect.bottom + gap;
    let left = align === "end" ? rect.right - menuWidth : rect.left;

    // If opening upward and height wasn't measured yet, keep near trigger.
    if (side === "top" && menuHeight === 0) {
      top = rect.top - gap;
    }

    left = Math.min(Math.max(pad, left), window.innerWidth - menuWidth - pad);
    top = Math.min(
      Math.max(pad, top),
      window.innerHeight - Math.max(menuHeight, 48) - pad,
    );

    setCoords({ top, left });
  }, [align, side, triggerRef]);

  useLayoutEffect(() => {
    if (!open) {
      setCoords(null);
      return;
    }
    updatePosition();
    // Second pass after paint so measured menu size is accurate.
    const id = requestAnimationFrame(() => updatePosition());
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      cancelAnimationFrame(id);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [open, updatePosition, children]);

  useEffect(() => {
    if (!open) return;
    const onPointer = (e: PointerEvent) => {
      const target = e.target as Node;
      if (menuRef.current?.contains(target)) return;
      if (triggerRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, setOpen, triggerRef]);

  if (!open) return null;

  const style: CSSProperties = coords
    ? { top: coords.top, left: coords.left, zIndex: 80 }
    : { top: 0, left: 0, visibility: "hidden", zIndex: 80 };

  return createPortal(
    <Menu
      ref={menuRef}
      role={role}
      id={menuId}
      placement="fixed"
      className={className}
      style={style}
      aria-label={ariaLabel}
      onMouseDown={(e) => e.stopPropagation()}
    >
      {children}
    </Menu>,
    document.body,
  );
}

export function useDropdownMenuClose() {
  const { setOpen } = useDropdownMenu();
  return useCallback(() => setOpen(false), [setOpen]);
}
