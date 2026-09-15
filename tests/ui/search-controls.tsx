import { searchConsumerSources } from "./search-consumers";
import { runSearchAudit, runSearchOrderAudit, runSearchStateAudit, proveModalRegressionIsCaught } from "./search-audit";
import { useState, type Dispatch, type SetStateAction } from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { ReactFlow, ReactFlowProvider } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import "../../src/App.css";
import { SearchInput } from "../../src/components/search-input";
import { SettingsSidebar } from "../../src/features/settings/components/settings-sidebar/settings-sidebar";
import { applyResolvedTheme } from "../../src/features/settings/theme";
import { useIntegrationsStore } from "../../src/features/integrations/store";
import type { ActionDescriptor, ActionFieldDescriptor, AppConnection, AppEventDescriptor, AppProvider } from "../../src/features/integrations/types";
import { useWorkflowStore } from "../../src/features/workflow/store";
import { defaultAgentNodeData, type OutputMemory, type Skill, type Workflow } from "../../src/features/workflow/types";
import { memoryVersionFixture } from "./memory-version-fixture";
import { AddStepPanel } from "../../src/features/workflow/components/add-step-panel/add-step-panel";
import { HistoryPage } from "../../src/features/workflow/components/history-page/history-page";
import { MemoriesInspector } from "../../src/features/workflow/components/memories-inspector/memories-inspector";
import { NodeSettingsModal } from "../../src/features/workflow/components/node-settings-modal/node-settings-modal";
import { RunActivityPanel } from "../../src/features/workflow/components/run-activity-panel/run-activity-panel";
import { TriggersModal } from "../../src/features/workflow/components/triggers-modal/triggers-modal";
import { WorkflowResourcesPanel } from "../../src/features/workflow/components/workflow-resources-panel/workflow-resources-panel";
const timestamp = "2026-09-08T16:00:00Z";
const workflow: Workflow = { id: "search-audit-workflow", name: "Content studio", description: "Search audit fixture", workingDirectory: "/tmp/alfred-search-audit", memoryRetrievalEnabled: true, memoryLookupEnabled: false, graph: { nodes: [], edges: [] }, createdAt: timestamp, updatedAt: timestamp };
const memoryBase: OutputMemory = { ...memoryVersionFixture(), id: "", workflowId: workflow.id, kind: "note", scopeType: "workflow", scopeKey: workflow.id, scopeLabel: workflow.name, memoryType: "note", source: "manual", title: "", body: "", pinned: false, confidence: 1, salience: 50, status: "active", createdAt: timestamp, updatedAt: timestamp, origin: "owned" };
const memories: OutputMemory[] = ["Editorial voice", "Product terminology", "Audience research", "Review checklist", "Article outline", "Publishing decisions"].map((title, index) => ({ ...memoryBase, id: `audit-memory-${index}`, title, body: `Reusable ${title.toLowerCase()} context for this workflow.`, pinned: index === 0 }));
const linkable: OutputMemory[] = [{ ...memoryBase, id: "audit-linkable", workflowId: "audit-research", scopeKey: "audit-research", title: "Customer interviews", body: "Readers prefer practical advice and concrete examples.", sourceWorkflowName: "Research", scopeLabel: "Research", origin: "linkable" }];
const skills: Skill[] = ["review", "research"].map((name) => ({ name, description: `${name} the work with useful examples`, path: `/tmp/alfred-search-audit/${name}/SKILL.md`, source: "project", sourceAgent: "claude_code", providers: ["claude_code"] }));
const provider: AppProvider = { id: "audit-app", name: "Fixture app", capabilitySummary: "Local audit resources", connectionModes: ["private"], connectAvailable: false, experimental: false, singleConnection: false };
const connection: AppConnection = { id: "audit-connection", providerId: provider.id, displayName: "Fixture account", externalAccountId: null, externalTenantId: null, connectionMode: "private", scopes: [], status: "connected", expiresAt: null, lastCheckedAt: timestamp, lastErrorCode: null, createdAt: timestamp, updatedAt: timestamp };
const field: ActionFieldDescriptor = { key: "channel", label: "Channel", description: "Choose a fixture channel", kind: "resource_selector", required: false, default: null, secret: false, optionSource: "channels", options: [], supportsInterpolation: false };
const action: ActionDescriptor = { providerId: provider.id, actionId: "audit-action", label: "Read channel", description: "A fixture action with a real resource selector", fields: [field], requiredScopes: [], outputSchemaVersion: 1, outputIsUntrusted: true };
const event: AppEventDescriptor = { providerId: provider.id, eventType: "audit-event", label: "Channel update", description: "A fixture event with a real resource selector", requiredScopes: [], deliveryModes: ["polling"], filterFields: [field], fetchesResourceContent: false, descriptorVersion: 1, externalEventIdRequired: true, allowedAttributeKeys: [], pollIntervalSeconds: 60, pendingCap: 10 };
const calls: {
    command: string;
    payload: unknown;
}[] = [];
const unhandled: string[] = [];
const interactions: unknown[] = [];
const pressSamples: { scenario: string; active: boolean; transform: string; background: string }[] = [];
// This entry point cannot fall through to native IPC. Unknown calls fail closed.
mockWindows("main");
mockIPC((command, payload) => {
    calls.push({ command, payload: structuredClone(payload ?? {}) });
    switch (command) {
        case "list_memories": return structuredClone(memories);
        case "list_linkable_memories": return structuredClone(linkable);
        case "list_agent_providers": return [{ id: "claude_code", label: "Claude Code", defaultModel: null }];
        case "get_memory_review_settings": return { enabled: true, provider: "claude_code", model: null, maxCandidates: 5, updatedAt: timestamp };
        case "get_workflow_memory_review": return { workflowId: workflow.id, enabled: false, updatedAt: timestamp };
        case "get_memory_lookup_availability": return { available: false, reason: "native_runtime_unavailable" };
        case "list_memory_candidates":
        case "list_memory_evidence":
        case "list_memory_reviews":
        case "list_run_history":
        case "search_history":
        case "list_workflow_triggers":
        case "list_app_trigger_statuses": return [];
        case "webhook_base_url": return null;
        case "list_app_providers": return [provider];
        case "list_app_connections": return [connection];
        case "list_app_action_descriptors": return [action];
        case "list_app_event_descriptors": return [event];
        case "list_app_action_resources":
        case "list_app_event_resources": return { items: [{ id: "audit-channel", label: "Editorial" }], nextPageToken: null };
        default:
            if (command.startsWith("plugin:window|") || command.startsWith("plugin:webview|"))
                return null;
            unhandled.push(command);
            throw new Error(`Unmocked search audit IPC: ${command}`);
    }
}, { shouldMockEvents: true });
useWorkflowStore.setState({ workflows: [workflow], activeWorkflowId: workflow.id, memories, skills, runPanelOpen: true, error: null, nodes: [
        { id: "audit-memory-node", type: "memory", position: { x: 0, y: 0 }, data: { label: "Research context", memoryIds: [memories[0].id] } },
        { id: "audit-agent-node", type: "agent", position: { x: 0, y: 0 }, data: defaultAgentNodeData("claude_code") },
        { id: "audit-action-node", type: "appAction", position: { x: 0, y: 0 }, data: { type: "appAction", label: "Channel lookup", providerId: provider.id, actionId: action.actionId, connectionId: connection.id, input: {} } },
    ] });
useIntegrationsStore.setState({ providers: [provider], connections: [connection], descriptors: [action], eventDescriptors: [event], loading: false, error: null });
applyResolvedTheme("dark");
type ScenarioId = keyof typeof searchConsumerSources;
export const scenarios = ([
    { id: "library", label: "Memory library", selector: '[aria-label="Search memories"]' },
    { id: "link", label: "Nested memory link picker", selector: '[aria-label="Search linkable memories"]' },
    { id: "memory-node", label: "Memories node settings", selector: '[aria-label="Search available memories"]' },
    { id: "add-step", label: "Canvas Add step picker", selector: '[aria-label="Search steps"]' },
    { id: "settings", label: "Settings sidebar", selector: '[aria-label="Search settings"]' },
    { id: "history", label: "History page", selector: '[aria-label="Search history"]' },
    { id: "skills", label: "Agent settings skill picker", selector: '[aria-label="Search skills"]' },
    { id: "activity", label: "Run activity memories", selector: '[aria-label="Search library"]' },
    { id: "resources", label: "Workflow resources", selector: '[aria-label="Search workflow resources"]' },
    { id: "action-options", label: "App action resource options", selector: '[aria-label="Search Channel options"]' },
    { id: "trigger-options", label: "App trigger resource options", selector: '[aria-label="Search Channel options"]' },
] satisfies { id: ScenarioId; label: string; selector: string }[]).map((scenario) => ({ ...scenario, source: searchConsumerSources[scenario.id] }));
const scenarioIds = new Set(scenarios.map((scenario) => scenario.id));
if (scenarioIds.size !== scenarios.length || scenarioIds.size !== Object.keys(searchConsumerSources).length || Object.keys(searchConsumerSources).some((id) => !scenarioIds.has(id as ScenarioId))) {
    throw new Error("Search fixture scenarios must exactly cover the production source manifest.");
}
type ViewState = {
    id: ScenarioId;
    generation: number;
};
type ReferenceState = {
    value: string;
    disabled: boolean;
    readOnly: boolean;
    width: number;
};
let setView: Dispatch<SetStateAction<ViewState>>;
let setReferenceState: Dispatch<SetStateAction<ReferenceState>>;
let current: ScenarioId = "library";
const settle = () => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
async function waitFor<T>(get: () => T | null | undefined | false, description: string): Promise<T> {
    const deadline = performance.now() + 5000;
    do {
        const result = get();
        if (result)
            return result;
        await settle();
    } while (performance.now() < deadline);
    throw new Error(`Search fixture did not render ${description}; IPC errors: ${unhandled.join(", ")}`);
}
async function choose(label: string, value: string) {
    const select = await waitFor(() => [...document.querySelectorAll<HTMLLabelElement>("label.field")].find((element) => element.querySelector("span")?.textContent === label)?.querySelector<HTMLSelectElement>("select"), label);
    await waitFor(() => [...select.options].some((option) => option.value === value), `${label} option ${value}`);
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
}
async function show(id: ScenarioId) {
    current = id;
    flushSync(() => setView((previous) => ({ id, generation: previous.generation + 1 })));
    await settle();
    if (id === "link") {
        const button = await waitFor(() => [...document.querySelectorAll<HTMLButtonElement>("button")].find((element) => element.textContent?.trim() === "Link memory"), "Link memory button");
        button.click();
    }
    else if (id === "add-step") {
        (await waitFor(() => document.querySelector<HTMLButtonElement>(".add-step-trigger"), "Add step trigger")).click();
    }
    else if (id === "trigger-options") {
        await choose("Add trigger", "app");
        await choose("Provider", provider.id);
        await choose("Event", event.eventType);
        await choose("Connection", connection.id);
    }
    const input = await waitFor(() => document.querySelector<HTMLInputElement>(scenarios.find((scenario) => scenario.id === id)!.selector), `${id} search`);
    input.scrollIntoView({ block: "nearest" });
    await settle();
    await Promise.all(document.getAnimations().filter((animation) => Number.isFinite(animation.effect?.getComputedTiming().endTime)).map((animation) => animation.finished.catch(() => undefined)));
}
function Consumer({ id }: {
    id: ScenarioId;
}) {
    const noOp = () => undefined;
    if (id === "library" || id === "link")
        return <MemoriesInspector open initialMemoryId={memories[0].id} onClose={noOp} onOpenRunHistory={noOp}/>;
    if (id === "memory-node" || id === "skills" || id === "action-options")
        return <NodeSettingsModal nodeId={id === "memory-node" ? "audit-memory-node" : id === "skills" ? "audit-agent-node" : "audit-action-node"} onClose={noOp}/>;
    if (id === "trigger-options")
        return <TriggersModal workflowId={workflow.id} workflowName={workflow.name} onClose={noOp}/>;
    if (id === "settings")
        return <div className="app-shell run-collapsed"><div className="sidebar-rail"><aside className="sidebar"><SettingsSidebar activeSection="general" onChange={noOp} onBack={noOp}/></aside></div><main /></div>;
    if (id === "history")
        return <div className="app-shell sidebar-collapsed run-collapsed"><div className="sidebar-rail" /><HistoryPage activeWorkflowId={workflow.id} onClose={noOp}/></div>;
    if (id === "activity")
        return <div className="app-shell sidebar-collapsed"><main /><RunActivityPanel /></div>;
    if (id === "resources")
        return <div className="workflow-workspace" style={{ height: "100%" }}><WorkflowResourcesPanel workflowId={workflow.id} workflowName={workflow.name} onClose={noOp} onOpenMemories={noOp} onOpenRunHistory={noOp}/><main /></div>;
    return <ReactFlowProvider><ReactFlow nodes={[]} edges={[]}><AddStepPanel onAddPrompt={(...args) => interactions.push(args)} onAddAgent={(...args) => interactions.push(args)} onAddChoose={(...args) => interactions.push(args)} onAddMemory={(...args) => interactions.push(args)} onAddStep={(...args) => interactions.push(args)}/></ReactFlow></ReactFlowProvider>;
}
function Fixture() {
    const [view, updateView] = useState<ViewState>({ id: "library", generation: 0 });
    const [referenceState, updateReference] = useState<ReferenceState>({ value: "", disabled: false, readOnly: false, width: 320 });
    setView = updateView;
    setReferenceState = updateReference;
    return <>
    <div className="search-audit-toolbar" style={{ position: "fixed", top: 0, left: 0, right: 0, zIndex: 100000, display: "flex", alignItems: "center", gap: 12, padding: "6px 14px", height: 44, background: "var(--surface-panel-opaque)", borderBottom: "1px solid var(--line)", color: "var(--ink)", fontSize: 12 }}>
      <strong>Search consumer audit</strong><select aria-label="Audit scenario" value={view.id} onChange={(e) => void show(e.target.value as ScenarioId)}>{scenarios.map((scenario) => <option key={scenario.id} value={scenario.id}>{scenario.label}</option>)}</select><button onClick={() => applyResolvedTheme("light")}>Light</button><button onClick={() => applyResolvedTheme("dark")}>Dark</button><span>Fixture data · native IPC blocked</span>
    </div>
    <div className="app-frame" style={{ height: "calc(100dvh - 44px)", marginTop: 44 }}><Consumer key={`${view.id}:${view.generation}`} id={view.id}/></div>
    <div data-search-audit-reference style={{ position: "fixed", left: -10000, top: 0, width: referenceState.width }}><SearchInput aria-label="Search audit reference" value={referenceState.value} disabled={referenceState.disabled} readOnly={referenceState.readOnly} onChange={(event) => updateReference((previous) => ({ ...previous, value: event.target.value }))} onClear={() => updateReference((previous) => ({ ...previous, value: "" }))}/></div>
  </>;
}
export const searchFixture = {
    scenarios: [...scenarios], calls, unhandled, interactions, pressSamples,
    armPressSample() {
        const capture = (event: PointerEvent) => {
            const button = (event.target as Element).closest<HTMLButtonElement>(".ui-search-clear");
            if (!button) return;
            document.removeEventListener("pointerdown", capture);
            window.setTimeout(() => pressSamples.push({ scenario: current, active: button.matches(":active"), transform: getComputedStyle(button).transform, background: getComputedStyle(button).backgroundColor }), 100);
        };
        document.addEventListener("pointerdown", capture);
    },
    show(id: string) { if (!scenarios.some((scenario) => scenario.id === id))
        return Promise.reject(new Error(`Unknown search scenario: ${id}`)); return show(id as ScenarioId); },
    setTheme(theme: "light" | "dark") { applyResolvedTheme(theme); },
    getSearch() { return document.querySelector<HTMLInputElement>(scenarios.find((scenario) => scenario.id === current)!.selector)!; },
    reference() { return document.querySelector<HTMLInputElement>('[aria-label="Search audit reference"]')!; },
    async setReference(patch: Partial<ReferenceState>) { flushSync(() => setReferenceState((previous) => ({ ...previous, ...patch }))); await settle(); },
};
Object.assign(window, { searchFixture, runSearchAudit: (options?: Parameters<typeof runSearchAudit>[1]) => runSearchAudit(searchFixture, options), runSearchOrderAudit: () => runSearchOrderAudit(searchFixture), runSearchStateAudit: () => runSearchStateAudit(searchFixture), proveModalRegressionIsCaught: () => proveModalRegressionIsCaught(searchFixture, "library") });
createRoot(document.getElementById("root")!).render(<Fixture />);
