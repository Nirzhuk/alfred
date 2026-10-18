import { listen } from "@tauri-apps/api/event";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { ToastViewport } from "./components/toast";
import { syncQuickAccessPreference } from "./features/quick-access/preferences";
import { installThemeListeners } from "./features/settings/theme";
import type { OpenRunOutputPayload } from "./features/settings/notifications";
import {
  getShortcutHelpLines,
  syncShortcutPreferences,
  useShortcutPreferences,
} from "./features/settings/shortcuts";
import { WorkflowCanvas } from "./features/workflow/components/workflow-canvas";
import { ImportLinkDialog } from "./features/sharing";
import { NativeHostApproval } from "./features/agent-accounts/components/native-host-approval";
import { MissedSchedulesModal } from "./features/workflow/components/missed-schedules-modal";
import {
  installOutputPersistenceEventBridge,
  installRunEventBridge,
  installScheduleEventBridge,
  useWorkflowStore,
} from "./features/workflow/store";
import { installAppMenu } from "./menu";
import { prepareNotifications } from "./native";
import "./App.css";

function App() {
  const loadWorkflows = useWorkflowStore((s) => s.loadWorkflows);
  const loadMissedSchedules = useWorkflowStore((s) => s.loadMissedSchedules);
  const missedSchedules = useWorkflowStore((s) => s.missedSchedules);
  const createWorkflow = useWorkflowStore((s) => s.createWorkflow);
  const saveActiveWorkflow = useWorkflowStore((s) => s.saveActiveWorkflow);
  const runActiveWorkflow = useWorkflowStore((s) => s.runActiveWorkflow);
  const shortcuts = useShortcutPreferences((s) => s.shortcuts);
  const [sharingLink, setSharingLink] = useState<string | null>(null);
  const [manualImportOpen, setManualImportOpen] = useState(false);
  // Answered rows leave the list; this only covers "Decide later", where the
  // unanswered rows stay due and are asked again on the next launch.
  const [missedDismissed, setMissedDismissed] = useState(false);
  /** Refusal from the backend for one missed-run answer, shown in the prompt. */
  const [missedError, setMissedError] = useState<string | null>(null);
  const [, setQueuedSharingLinks] = useState<string[]>([]);
  const sharingLinkRef = useRef<string | null>(null);
  const queuedSharingLinksRef = useRef<string[]>([]);
  const manualImportOpenRef = useRef(false);
  const inFlightSharingLinksRef = useRef<Set<string>>(new Set());

  const canonicalSharingLink = (value: string) => {
    try {
      const parsed = new URL(value.trim());
      if (!["https:", "http:", "alfred-workflows:"].includes(parsed.protocol)) {
        return null;
      }
      return parsed.toString();
    } catch {
      return null;
    }
  };

  // HTTPS and the native scheme are two transports for the same share. Keep
  // the original URL (including any fragment) for backend validation, but use
  // the validated-looking share ID as the bounded in-flight identity.
  const sharingLinkIdentity = (link: string) => {
    try {
      const parsed = new URL(link);
      const segments = parsed.pathname.split("/").filter(Boolean);
      if (parsed.protocol === "https:" || parsed.protocol === "http:") {
        if (segments.length === 2 && segments[0] === "s") return `share:${segments[1]}`;
      }
      if (parsed.protocol === "alfred-workflows:" && parsed.hostname === "import" && segments.length === 1) {
        return `share:${segments[0]}`;
      }
    } catch {
      // The backend owns full canonical-link validation; malformed URLs still
      // get a stable local key so repeated deliveries cannot grow state.
    }
    return `url:${link}`;
  };

  const enqueueSharingLink = useCallback((value: string) => {
    const link = canonicalSharingLink(value);
    if (!link) return;
    const identity = sharingLinkIdentity(link);
    if (inFlightSharingLinksRef.current.has(identity)) return;
    if (!sharingLinkRef.current && !manualImportOpenRef.current) {
      inFlightSharingLinksRef.current.add(identity);
      sharingLinkRef.current = link;
      setSharingLink(link);
      return;
    }
    if (queuedSharingLinksRef.current.length >= 8) return;
    inFlightSharingLinksRef.current.add(identity);
    queuedSharingLinksRef.current = [...queuedSharingLinksRef.current, link];
    setQueuedSharingLinks(queuedSharingLinksRef.current);
  }, []);

  const closeSharingImport = useCallback(() => {
    const current = sharingLinkRef.current;
    if (current) inFlightSharingLinksRef.current.delete(sharingLinkIdentity(current));
    const [next, ...rest] = queuedSharingLinksRef.current;
    queuedSharingLinksRef.current = rest;
    setQueuedSharingLinks(rest);
    sharingLinkRef.current = next ?? null;
    setSharingLink(next ?? null);
    if (!next) {
      manualImportOpenRef.current = false;
      setManualImportOpen(false);
    }
  }, []);

  useEffect(() => {
    manualImportOpenRef.current = manualImportOpen;
  }, [manualImportOpen]);

  useEffect(() => {
    if (!isTauri()) return;
    void loadWorkflows();
    void loadMissedSchedules();
    void prepareNotifications();
    void installRunEventBridge();
    void installOutputPersistenceEventBridge();
    void installScheduleEventBridge();
    void syncQuickAccessPreference();
    void syncShortcutPreferences();
  }, [loadWorkflows, loadMissedSchedules]);

  useEffect(() => installThemeListeners(), []);

  useEffect(() => {
    if (!isTauri()) return;
    const unsubs: Array<() => void> = [];
    let active = true;
    void listen("app://open-settings", () => {
      window.dispatchEvent(new Event("alfred:open-settings"));
    }).then((u) => unsubs.push(u));
    void listen("app://open-schedules", () => {
      window.dispatchEvent(new Event("alfred:open-schedules"));
    }).then((u) => unsubs.push(u));
    void listen("app://download-latest", () => {
      window.dispatchEvent(new Event("alfred:download-latest"));
    }).then((u) => unsubs.push(u));
    void listen<string>("app://open-workflow", (event) => {
      window.dispatchEvent(
        new CustomEvent("alfred:open-workflow", {
          detail: { workflowId: event.payload },
        }),
      );
    }).then((u) => unsubs.push(u));
    void listen("app://open-activity", () => {
      window.dispatchEvent(new Event("alfred:open-activity"));
    }).then((u) => unsubs.push(u));
    void listen<OpenRunOutputPayload>("app://open-run-output", (event) => {
      window.dispatchEvent(
        new CustomEvent("alfred:open-run-output", {
          detail: event.payload,
        }),
      );
    }).then((u) => unsubs.push(u));
    // The plugin retains the launch URL for cold starts. Await registration
    // before reading it so a warm delivery cannot race recovery on startup.
    const deepLinkReady = listen<string[]>("deep-link://new-url", (event) => {
      for (const link of event.payload ?? []) enqueueSharingLink(link);
    }).then((unsubscribe) => {
      if (active) unsubs.push(unsubscribe);
      else unsubscribe();
    });
    void deepLinkReady.then(() => invoke<string[]>("plugin:deep-link|get_current"))
      .then((links) => { for (const link of links ?? []) enqueueSharingLink(link); })
      .catch(() => { /* browser preview and older plugin versions have no current URL */ });
    return () => {
      active = false;
      for (const u of unsubs) u();
    };
  }, [enqueueSharingLink]);

  useEffect(() => {
    const openImport = (event: Event) => {
      const link = (event as CustomEvent<{ link?: string }>).detail?.link;
      if (link) enqueueSharingLink(link);
      else if (!sharingLinkRef.current) {
        manualImportOpenRef.current = true;
        setManualImportOpen(true);
      }
    };
    window.addEventListener("alfred:open-sharing-import", openImport);
    return () => window.removeEventListener("alfred:open-sharing-import", openImport);
  }, [enqueueSharingLink]);

  useEffect(() => {
    if (!isTauri()) return;
    void installAppMenu(
      {
        onOpenSettings: () => {
          window.dispatchEvent(new Event("alfred:open-settings"));
        },
        onOpenSchedules: () => {
          window.dispatchEvent(new Event("alfred:open-schedules"));
        },
        onNewWorkflow: () => void createWorkflow(),
        onSaveWorkflow: () => void saveActiveWorkflow(),
        onRenameWorkflow: () => {
          window.dispatchEvent(new Event("alfred:rename-workflow"));
        },
        onRunWorkflow: () => void runActiveWorkflow(),
        onDeleteWorkflow: () => {
          window.dispatchEvent(new Event("alfred:delete-workflow"));
        },
        onScheduleWorkflow: () => {
          window.dispatchEvent(new Event("alfred:open-schedule"));
        },
        onToggleSidebar: () => {
          window.dispatchEvent(new Event("alfred:toggle-sidebar"));
        },
        onToggleActivity: () => {
          window.dispatchEvent(new Event("alfred:toggle-activity"));
        },
        onFitCanvas: () => {
          window.dispatchEvent(new Event("alfred:fit-canvas"));
        },
        onDownloadLatest: () => {
          window.dispatchEvent(new Event("alfred:download-latest"));
        },
        onImportWorkflow: () => {
          window.dispatchEvent(new Event("alfred:open-sharing-import"));
        },
        onShowShortcuts: () => {
          window.alert(
            [
              "Alfred keyboard shortcuts",
              "",
              ...getShortcutHelpLines(shortcuts),
            ].join("\n"),
          );
        },
      },
      shortcuts,
    ).catch((err) => {
      console.warn("Native menu unavailable", err);
    });
  }, [createWorkflow, saveActiveWorkflow, runActiveWorkflow, shortcuts]);

  useEffect(() => {
    const isEditable = (target: EventTarget | null) => {
      const el = target as HTMLElement | null;
      if (!el?.closest) return false;
      return Boolean(
        el.closest(
          'input, textarea, select, [contenteditable="true"], .user-select-text, .output-modal-body',
        ),
      );
    };

    // Kill browser-y selection gestures outside editable/output surfaces.
    const onSelectStart = (e: Event) => {
      if (!isEditable(e.target)) e.preventDefault();
    };

    // Avoid accidental image/text drags looking like a webpage.
    const onDragStart = (e: DragEvent) => {
      const el = e.target as HTMLElement | null;
      if (!el?.closest) return;
      if (isEditable(e.target)) return;
      if (el.closest(".workflow-card-grip")) return;
      if (el.closest(".react-flow__node, .react-flow__edge, .react-flow__handle")) {
        return;
      }
      e.preventDefault();
    };

    // Disable the browser context menu on chrome; canvas uses its own menu.
    const onContextMenu = (e: MouseEvent) => {
      if (isEditable(e.target)) return;
      e.preventDefault();
    };

    document.addEventListener("selectstart", onSelectStart);
    document.addEventListener("dragstart", onDragStart);
    document.addEventListener("contextmenu", onContextMenu);
    return () => {
      document.removeEventListener("selectstart", onSelectStart);
      document.removeEventListener("dragstart", onDragStart);
      document.removeEventListener("contextmenu", onContextMenu);
    };
  }, []);

  return (
    <>
      <WorkflowCanvas />
      {sharingLink !== null || manualImportOpen ? (
        <ImportLinkDialog
          key={sharingLink ?? "manual"}
          initialLink={sharingLink || undefined}
          onClose={closeSharingImport}
        />
      ) : null}
      {missedSchedules.length > 0 && !missedDismissed ? (
        <MissedSchedulesModal
          missed={missedSchedules}
          error={missedError}
          onResolve={async (scheduleId, decision) => {
            setMissedError(
              await useWorkflowStore
                .getState()
                .resolveMissedSchedule(scheduleId, decision),
            );
          }}
          onClose={() => setMissedDismissed(true)}
        />
      ) : null}
      <NativeHostApproval />
      <ToastViewport />
    </>
  );
}

export default App;
