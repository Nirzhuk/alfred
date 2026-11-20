import React from "react";
import ReactDOM from "react-dom/client";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  bootstrapTheme,
  enableThemeTransitions,
} from "./features/settings/theme";
import { applyDesktopPlatform } from "./platform";

applyDesktopPlatform(document.documentElement);
bootstrapTheme();

const windowLabel = isTauri() ? getCurrentWindow().label : "main";
const isQuickAccessWindow = windowLabel === "quick-access";
const isRunWindow = windowLabel === "run-dialog";
const isMainWindow = windowLabel === "main";
document.documentElement.dataset.window = isQuickAccessWindow
  ? "quick-access"
  : isRunWindow
    ? "run-dialog"
    : "main";

async function revealMainWindow() {
  if (!isTauri() || !isMainWindow) return;
  try {
    await document.fonts.ready;
  } catch {
    // Showing the window still beats a hidden shell if font loading fails.
  }
  await getCurrentWindow().show();
}

if (isMainWindow) {
  window.setTimeout(() => {
    void revealMainWindow();
  }, 1200);
}

const WindowRoot = isQuickAccessWindow
  ? React.lazy(async () => ({
      default: (await import("./features/quick-access/quick-access-popover"))
        .QuickAccessPopover,
    }))
  : isRunWindow
    ? React.lazy(async () => ({
        default: (await import("./features/workflow/components/run-window"))
          .RunWindow,
      }))
    : React.lazy(async () => {
        const { default: App } = await import("./App");
        return {
          default: function ReadyMainWindow() {
            React.useLayoutEffect(() => {
              void revealMainWindow().then(() => {
                if (isTauri()) {
                  void invoke("main_window_ready").catch(() => {});
                }
              });
            }, []);
            return <App />;
          },
        };
      });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <React.Suspense fallback={null}>
      <WindowRoot />
    </React.Suspense>
  </React.StrictMode>,
);

enableThemeTransitions();
