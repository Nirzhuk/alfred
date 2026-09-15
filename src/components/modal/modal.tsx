import {
  useEffect,
  useRef,
  type HTMLAttributes,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

export type ModalSize = "sm" | "md" | "lg" | "settings" | "xl";

type ModalProps = {
  open?: boolean;
  onClose: () => void;
  size?: ModalSize;
  role?: "dialog" | "alertdialog";
  /** Extra class on the panel (size class is always applied). */
  className?: string;
  labelledBy?: string;
  /** Used as `aria-label` when `labelledBy` is omitted. */
  label?: string;
  describedBy?: string;
  closeOnBackdrop?: boolean;
  closeOnEscape?: boolean;
  children: ReactNode;
};

const SIZE_CLASS: Record<ModalSize, string> = {
  sm: "modal--sm",
  md: "modal--md",
  lg: "modal--lg",
  settings: "modal--settings",
  xl: "modal--xl",
};

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "details > summary:first-of-type",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

function isTabbable(element: HTMLElement): boolean {
  if (
    element.tabIndex < 0 ||
    element.matches(":disabled") ||
    element.closest('[inert], [aria-hidden="true"]') ||
    element.getClientRects().length === 0
  ) return false;
  const visibility = window.getComputedStyle(element).visibility;
  return visibility !== "hidden" && visibility !== "collapse";
}

function modalTabStops(panel: HTMLElement): HTMLElement[] {
  return Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
    .filter(isTabbable);
}

/**
 * Shared modal shell: portal to `document.body`, backdrop dismiss, Escape,
 * and size variants used across Alfred dialogs.
 */
export function Modal({
  open = true,
  onClose,
  size = "lg",
  role = "dialog",
  className,
  labelledBy,
  label,
  describedBy,
  closeOnBackdrop = true,
  closeOnEscape = true,
  children,
}: ModalProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const wasOpenRef = useRef(false);

  if (open && !wasOpenRef.current) {
    previouslyFocusedRef.current =
      typeof document === "undefined"
        ? null
        : (document.activeElement as HTMLElement | null);
  }
  wasOpenRef.current = open;

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!open) return;

    const focusPanel = window.requestAnimationFrame(() => {
      const panel = panelRef.current;
      if (!panel || panel.contains(document.activeElement)) return;
      const autofocus = panel.querySelector<HTMLElement>("[autofocus]");
      const preferred = autofocus && isTabbable(autofocus)
        ? autofocus
        : modalTabStops(panel)[0];
      (preferred ?? panel).focus();
    });

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (
          !closeOnEscape ||
          !panelRef.current?.contains(document.activeElement)
        ) {
          return;
        }
        e.stopPropagation();
        onCloseRef.current();
        return;
      }

      if (e.key !== "Tab") return;
      const panel = panelRef.current;
      if (!panel || !panel.contains(document.activeElement)) return;
      const focusable = modalTabStops(panel);

      if (focusable.length === 0) {
        e.preventDefault();
        panel.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.cancelAnimationFrame(focusPanel);
      window.removeEventListener("keydown", onKey);
      if (previouslyFocusedRef.current?.isConnected) {
        previouslyFocusedRef.current.focus();
      }
    };
  }, [open, closeOnEscape]);

  useEffect(() => {
    if (!open) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [open]);

  if (!open) return null;

  const panelClass = ["modal", SIZE_CLASS[size], className]
    .filter(Boolean)
    .join(" ");

  return createPortal(
    <div
      className="modal-backdrop"
      role="presentation"
      onClick={closeOnBackdrop ? onClose : undefined}
    >
      <div
        ref={panelRef}
        className={panelClass}
        role={role}
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby={labelledBy}
        aria-label={labelledBy ? undefined : label}
        aria-describedby={describedBy}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

type ModalHeaderProps = {
  leading?: ReactNode;
  title: ReactNode;
  titleId?: string;
  titleAs?: "h2" | "h3";
  description?: ReactNode;
  descriptionId?: string;
  /** Extra content under the title block (e.g. stats line). */
  children?: ReactNode;
  actions?: ReactNode;
  /** Use the denser / stronger header surface (Memories). */
  strong?: boolean;
  className?: string;
};

export function ModalHeader({
  leading,
  title,
  titleId,
  titleAs = "h3",
  description,
  descriptionId,
  children,
  actions,
  strong = false,
  className,
}: ModalHeaderProps) {
  const TitleTag = titleAs;
  const headerClass = [
    "modal-header",
    leading ? "modal-header--with-leading" : null,
    strong ? "modal-header--strong" : null,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <header className={headerClass}>
      {leading ? <div className="modal-header-leading">{leading}</div> : null}
      <div className="modal-header-copy">
        <TitleTag id={titleId}>{title}</TitleTag>
        {description ? (
          <div id={descriptionId} className="modal-header-description muted">
            {description}
          </div>
        ) : null}
        {children}
      </div>
      {actions ? <div className="modal-actions">{actions}</div> : null}
    </header>
  );
}

type ModalBodyProps = HTMLAttributes<HTMLDivElement> & {
  children: ReactNode;
};

/** Scrollable content region with standard modal body padding. */
export function ModalBody({ className, children, ...rest }: ModalBodyProps) {
  return (
    <div
      className={["modal-body", className].filter(Boolean).join(" ")}
      {...rest}
    >
      {children}
    </div>
  );
}
