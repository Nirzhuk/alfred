import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** Keep fixture controls discoverable inside the active modal's accessibility scope. */
export function QaModalPortal({ children }: { children: ReactNode }) {
  const [panel, setPanel] = useState<Element | null>(null);
  useEffect(() => {
    const discover = () => {
      const panels = document.querySelectorAll('[role="dialog"][aria-modal="true"], [role="alertdialog"][aria-modal="true"]');
      setPanel(panels.item(panels.length - 1));
    };
    discover();
    const observer = new MutationObserver(discover);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);
  return panel ? createPortal(children, panel) : children;
}
