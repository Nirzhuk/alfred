import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { createRoot } from "react-dom/client";
import { useState } from "react";
import "../../src/App.css";
import { applyResolvedTheme, useThemeStore } from "../../src/features/settings/theme";
import { ShareDialog } from "../../src/features/sharing/share-dialog";
import { ImportLinkDialog } from "../../src/features/sharing/import-link-dialog";
import { PublicationList } from "../../src/features/sharing/publication-list";
import type { SharingParameter, WorkflowTemplate } from "../../src/features/sharing/types";

const params = new URLSearchParams(location.search);
const mode = params.get("mode") ?? "import";
const scenario = params.get("scenario") ?? "normal";
const theme = params.get("theme") === "light" ? "light" : "dark";
applyResolvedTheme(theme);
useThemeStore.setState({ preference: theme, resolved: theme });
const calls: { command: string; payload: unknown }[] = [];
const errors: string[] = [];
window.addEventListener("error", (event) => errors.push(event.message));
window.addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const target = (nodeId: string, field: string) => ({ kind: "node", nodeId, field });
const parameter = (key: string, label: string, kind: string, nodeId: string, field: string, provider?: string): SharingParameter => ({ key, label, kind, required: true, allowedProvider: provider, target: target(nodeId, field), targets: [] });
const groupedConnection = parameter("connection-a", "Team connection", "appConnection", "a", "connectionId", "slack");
groupedConnection.targets = [target("b", "connectionId"), { kind: "start", nodeId: null, field: "0.connectionId" }];
const groupedResource = parameter("resource-a", "Team channel", "appResource", "a", "input.channel", "slack");
groupedResource.targets = [target("b", "input.channel")];
const eventResource = parameter("resource-event", "Incoming channel", "appResource", "a", "unused", "slack");
eventResource.target = { kind: "start", nodeId: null, field: "0.filters.channel" };
const publicText = parameter("message", "Release message", "text", "a", "input.text");
publicText.targets = [target("b", "input.text")];
const template: WorkflowTemplate = {
  format: "alfred.workflow-template", schemaVersion: 1, minAlfredVersion: "0.1.0", title: "Team release update", description: "Review release notes and prepare messages in two independent destinations.",
  graph: {
    nodes: [
      { id: "agent", type: "agent", data: { label: "Review notes", provider: "claude_code", harness: "alfred", prompt: "Summarize release notes." }, position: { x: 0, y: 0 } },
      ...["a", "b", "c"].map((id, index) => ({ id, type: "appAction", data: { label: `Message ${index + 1}`, providerId: "slack", actionId: "send_message", input: { channel: "", text: "" } }, position: { x: index * 200, y: 200 } })),
    ],
    edges: [{ id: "e1", source: "agent", target: "a" }, { id: "e2", source: "a", target: "b" }, { id: "e3", source: "b", target: "c" }],
  },
  parameters: [
    parameter("account", "Review account", "agentAccount", "agent", "accountRef", "claude_code"),
    groupedConnection,
    parameter("connection-b", "Partner connection", "appConnection", "c", "connectionId", "slack"),
    groupedResource, eventResource,
    parameter("resource-b", "Partner channel", "appResource", "c", "input.channel", "slack"),
    parameter("folder", "Notes folder", "folder", "agent", "workingDirectory"), publicText,
  ],
  starts: [{ kind: "appEvent", config: { providerId: "slack", eventType: "message", filters: { channel: "" }, enabled: false } }],
  requirements: [{ kind: "agent", name: "claude_code" }, { kind: "appAction", name: "send_message", provider: "slack" }, { kind: "appEvent", name: "message", provider: "slack" }],
};
const shareId = "11111111111111111111111111111111";
const url = `https://alfred.nirzhuk.dev/s/${shareId}`;
const publication = { shareId, url, digest: "synthetic-sha256", createdAt: "2026-09-09T12:00:00Z" };
let published = mode === "manage";
let revoked = false;
let imported = false;
const pendingResources: { connectionId: string; fieldKey: string; resolve: () => void }[] = [];

mockWindows("main");
mockIPC(async (command, payload) => {
  const input = payload as Record<string, any> | undefined;
  calls.push({ command, payload: structuredClone(payload ?? {}) });
  switch (command) {
    case "prepare_workflow_publication": return { title: template.title, description: template.description, template, removed: ["Publisher account references", "Publisher connection IDs", "Publisher folder"], requirements: [] };
    case "publish_workflow_template":
      if (scenario === "publish-failure") throw new Error("Synthetic sharing service unavailable");
      published = true; return publication;
    case "list_workflow_publications": return published && !revoked ? [publication] : [];
    case "list_pending_workflow_publications": return [];
    case "revoke_workflow_publication": revoked = true; return null;
    case "resolve_shared_workflow_link":
      if (scenario === "revoked") throw new Error("This shared workflow has been revoked.");
      return { format: template.format, digest: publication.digest, template };
    case "list_agent_accounts": return [
      { id: "account-work", displayName: "Work", providerName: "Claude Code", providerId: "claude_code", status: "connected" },
      { id: "account-expired", displayName: "Expired", providerName: "Claude Code", providerId: "claude_code", status: "expired" },
      { id: "account-other", displayName: "Other provider", providerName: "Codex", providerId: "codex", status: "connected" },
    ];
    case "list_app_connections": return ["team", "alternate", "partner"].map((id) => ({ id, displayName: id === "team" ? "Team Slack" : id === "alternate" ? "Alternate Slack" : "Partner Slack", providerId: "slack", status: "connected" }));
    case "list_app_action_resources":
    case "list_app_event_resources": {
      const connectionId = input?.connectionId as string;
      const fieldKey = input?.fieldKey as string;
      if (scenario === "resource-failure") throw new Error("Synthetic resource lookup failed");
      const result = { items: [{ id: `${connectionId}-channel`, label: `${connectionId} channel` }], nextPageToken: null };
      if (scenario === "delayed") return new Promise((resolve) => pendingResources.push({ connectionId, fieldKey, resolve: () => resolve(result) }));
      return result;
    }
    case "check_workflow_template_setup": {
      const values = input?.input?.values ?? {};
      const missing = template.parameters.filter((item) => item.required && !String(values[item.key] ?? "").trim());
      return { complete: missing.length === 0, errors: missing.map((item) => `Choose ${item.label}.`) };
    }
    case "import_workflow_template": {
      const existingCopy = imported || scenario === "existing";
      imported = true;
      return { workflowId: "recipient-copy", existingCopy };
    }
    case "plugin:dialog|open": return "/tmp/recipient-notes";
    default:
      if (command.startsWith("plugin:")) return null;
      throw new Error(`Unmocked sharing fixture IPC: ${command}`);
  }
}, { shouldMockEvents: true });

function SharingFixture() {
  const [open, setOpen] = useState(true);
  return <main style={{ padding: "2rem" }}>
    <h1>Sharing UI fixture</h1><p>Synthetic IPC only. No publication or provider action leaves this browser.</p>
    <button onClick={() => setOpen(true)}>Reopen {mode}</button>
    {!open ? <p role="status">Dialog closed.</p> : null}
    {open && mode === "share" ? <ShareDialog workflowId="publisher-workflow" onClose={() => setOpen(false)} /> : null}
    {open && mode === "import" ? <ImportLinkDialog initialLink={url} onClose={() => setOpen(false)} /> : null}
    {mode === "manage" ? <PublicationList workflowId="publisher-workflow" /> : null}
  </main>;
}

createRoot(document.getElementById("root")!).render(<SharingFixture />);
const fixture = {
  calls, errors, pendingResources, template,
  releaseResources(connectionId: string) {
    const ready = pendingResources.filter((item) => item.connectionId === connectionId);
    for (const item of ready) { pendingResources.splice(pendingResources.indexOf(item), 1); item.resolve(); }
    return ready.length;
  },
};
(window as typeof window & { sharingFixture?: typeof fixture }).sharingFixture = fixture;
