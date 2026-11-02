import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { createRoot } from "react-dom/client";
import "@xyflow/react/dist/style.css";
import "../../src/App.css";
import { WorkflowCanvas } from "../../src/features/workflow/components/workflow-canvas";
import { useWorkflowStore } from "../../src/features/workflow/store";
import {
  applyResolvedTheme,
  useThemeStore,
} from "../../src/features/settings/theme";
import type { Workflow } from "../../src/features/workflow/types";

/**
 * The real app shell on mocked IPC and demo data, for the README screenshots.
 * Nothing here reads a user database, credentials, or workflows.
 *
 * `?theme=light|dark&screen=<sidebar item name>`
 */

const params = new URLSearchParams(location.search);
const theme = params.get("theme") === "light" ? "light" : "dark";
const screen = params.get("screen");
applyResolvedTheme(theme);
useThemeStore.setState({ preference: theme, resolved: theme });

const at = "2026-10-28T09:00:00Z";
const folders = [{ id: "folder-acme", name: "Acme app", createdAt: at, updatedAt: at }];
const workflow = (id: string, name: string, graph: unknown): Workflow => ({
  id,
  name,
  description: "",
  workingDirectory: "~/projects/acme-app",
  folderId: "folder-acme",
  memoryLookupEnabled: false,
  graph: graph as Workflow["graph"],
  createdAt: at,
  updatedAt: at,
});

const agent = (id: string, label: string, provider: string, x: number, y: number, model: string, skillNames: string[] = []) => ({
  id,
  type: "agent",
  position: { x, y },
  data: { label, provider, harness: "cli", model, skillNames },
});

const release = workflow("wf-release", "Weekly release notes", {
  nodes: [
    { id: "brief", type: "input", position: { x: 0, y: 0 }, data: { label: "Brief", prompt: "Summarize everything merged this week for our users." } },
    { id: "voice", type: "identity", position: { x: 40, y: 290 }, data: { kind: "identity", label: "Release editor", role: "Release editor", personality: "Plain and direct", instructions: "Lead with what changed for users." } },
    { id: "context", type: "memory", position: { x: 40, y: 430 }, data: { label: "Style guide", memoryIds: ["m-style"] } },
    agent("draft", "Draft", "claude_code", 330, 170, "sonnet", ["changelog"]),
    agent("review", "Review", "codex", 600, 170, "gpt-5"),
    { id: "notes", type: "chooseOutput", position: { x: 870, y: 170 }, data: { label: "Release notes", saveToMemory: true, pinMemory: false, memoryTitle: "", asFinalResult: true, htmlReport: false, includeFilesChanged: false } },
  ],
  edges: [
    { id: "e1", source: "brief", target: "draft" },
    { id: "e2", source: "voice", target: "draft" },
    { id: "e3", source: "context", target: "draft" },
    { id: "e4", source: "draft", target: "review" },
    { id: "e5", source: "review", target: "notes" },
  ],
});

const workflows = [
  release,
  workflow("wf-triage", "Triage new issues", { nodes: [agent("a", "Triage", "cursor", 0, 0, "auto")], edges: [] }),
  workflow("wf-review", "Review open pull requests", { nodes: [agent("a", "Review", "claude_code", 0, 0, "sonnet"), agent("b", "Second opinion", "gemini", 320, 0, "pro")], edges: [] }),
];

const run = (id: string, workflowId: string, workflowName: string, trigger: string, status: string, day: number, stepCount: number, finalOutputPreview: string, error: string | null = null) => {
  const started = `2026-10-${day}T09:00:00Z`;
  return { id, workflowId, workflowName, trigger, status, error, startedAt: started, finishedAt: `2026-10-${day}T09:03:24Z`, createdAt: started, stepCount, finalOutputPreview };
};
const runs = [
  run("r1", "wf-release", "Weekly release notes", "schedule", "completed", 28, 3, "Faster search, a new export menu, and two sync fixes."),
  run("r2", "wf-review", "Review open pull requests", "manual", "completed", 27, 2, "Three pull requests are ready. One needs a migration check."),
  run("r3", "wf-triage", "Triage new issues", "webhook", "completed", 27, 1, "Labelled 6 new issues. Two look like the same sync bug."),
  run("r4", "wf-release", "Weekly release notes", "schedule", "failed", 21, 3, "", "Codex CLI is not signed in."),
];

const memory = (id: string, title: string, body: string, memoryType: string, scopeType: string, scopeLabel: string, pinned = false) => ({
  id, workflowId: "wf-release", kind: "note", scopeType, scopeKey: scopeLabel, scopeLabel, memoryType, source: "manual", title, body,
  artifactContentDigest: null, pinned, confidence: 1, salience: 1, status: "active", semanticRevision: 1, contentDigest: id,
  factKey: null, observedAt: null, lastConfirmedAt: null, validFrom: null, expiresAt: null, createdAt: at, updatedAt: at,
  provenanceState: "manual", origin: "owned",
});
const memories = [
  memory("m-style", "Style guide", "Short sentences. Name the feature, then say what changed for the user.", "preference", "workspace", "Acme app", true),
  memory("m-audience", "Who reads the notes", "Customers on the Team plan. Skip internal refactors.", "fact", "workflow", "Weekly release notes"),
  memory("m-versioning", "Version numbers", "We ship calendar versions, such as 2026.10.", "decision", "workspace", "Acme app"),
  memory("m-links", "Link every feature", "Each item links to its docs page.", "constraint", "user", "All workflows"),
];

const unhandled = new Set<string>();
const errors: string[] = [];
window.addEventListener("error", (event) => errors.push(event.message));
window.addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
Object.assign(window, { readmeScreens: { unhandled, errors } });

mockWindows("main");
mockIPC(
  (command, payload) => {
    const args = (payload ?? {}) as Record<string, never>;
    switch (command) {
      case "list_workflows":
        return structuredClone(workflows);
      case "get_workflow":
        return structuredClone(workflows.find((item) => item.id === args.id) ?? null);
      case "update_workflow":
        return { ...release, ...(args.input as object) };
      case "list_workflow_folders":
        return folders;
      case "list_run_history":
        return runs;
      case "list_memories":
        return memories;
      case "get_agent_usage":
        return [
          { provider: "claude_code", connected: true, source: "cli", updatedAt: at, windows: [{ label: "Session", usedPercent: 24 }, { label: "Week", usedPercent: 41 }] },
          { provider: "codex", connected: true, source: "cli", updatedAt: at, windows: [{ label: "Session", usedPercent: 12 }, { label: "Week", usedPercent: 30 }] },
        ];
      case "get_memory_review_settings":
        return { enabled: false, provider: null, model: null, maxCandidates: 3, updatedAt: at };
      case "get_workflow_memory_review":
        return { workflowId: "wf-release", enabled: false, updatedAt: at };
      case "builder_get_config":
        return { rules: [], mcpServers: [] };
      case "builder_info":
        return {
          agents: [
            { id: "claude_code", label: "Claude Code", installed: true },
            { id: "codex", label: "Codex", installed: true },
          ],
          mcpReady: true,
          tools: [],
        };
      default:
        unhandled.add(command);
        return /^list_|_list$|^search_|^builder_list_/.test(command) ? [] : null;
    }
  },
  { shouldMockEvents: true },
);

createRoot(document.getElementById("root") as HTMLElement).render(
  <div className="app-frame" style={{ height: "100dvh" }}>
    <WorkflowCanvas />
  </div>,
);

void useWorkflowStore
  .getState()
  .loadWorkflows()
  .then(async () => {
    await useWorkflowStore.getState().selectWorkflow("wf-release");
    // Let the canvas settle first, so the header reads as a saved workflow.
    await new Promise((resolve) => setTimeout(resolve, 800));
    await useWorkflowStore.getState().saveActiveWorkflow();
    await useWorkflowStore.getState().loadAgentUsage(["claude_code", "codex"]);
    if (!screen) return;
    // Sidebar views are local state, so go through the same control a person uses.
    const item = [...document.querySelectorAll<HTMLElement>("button, a")].find(
      (el) => el.textContent?.trim() === screen,
    );
    item?.click();
  });
