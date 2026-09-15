import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
// `build.rs` bakes ALFRED_POLAR_ENVIRONMENT into the Rust binary; the frontend
// needs the same value to pick the matching Polar public-link allow-list (see
// src/features/licensing/public-link-rules.ts). ALFRED_DISABLE_LICENSE_GATING
// is the self-compile opt-out next to it: `1` forces every capability
// available without Polar. Only these two keys are loaded and defined — no
// other ALFRED_* publisher value reaches the bundle.
export default defineConfig(async ({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "ALFRED_");
  return {
    plugins: [react()],

    define: {
      "import.meta.env.ALFRED_POLAR_ENVIRONMENT": JSON.stringify(
        env.ALFRED_POLAR_ENVIRONMENT ?? "",
      ),
      "import.meta.env.ALFRED_DISABLE_LICENSE_GATING": JSON.stringify(
        env.ALFRED_DISABLE_LICENSE_GATING ?? "",
      ),
    },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  };
});
