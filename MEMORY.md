# Project Environment

```json
{"project_type":"tauri-react-desktop","is_react_native":false,"is_native_ios":false,"is_native_android":false,"package_manager":"bun","frontend":"React 19 + Vite 7 + TypeScript 5.8 + Zustand + React Flow","frontend_dev":"bun run dev:frontend","frontend_port":1420,"desktop_dev":"bun run dev","frontend_build":"bun run build:frontend","frontend_tests":"bun test","rust_tests":"bun run test:rust","full_check":"bun run check","metro_port":null}
```

Run commands from `desktop/`. Native Tauri uses WKWebView on macOS. Frontend UI
can be checked in a browser with isolated Tauri IPC fixtures. The design system
is documented in `docs/design-system.md`; reuse semantic tokens and shared icons.
