import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { createRoot } from "react-dom/client";
import "../../src/App.css";
import "@xyflow/react/dist/style.css";
import {
  applyResolvedTheme,
  useThemeStore,
} from "../../src/features/settings/theme";
import { WorkflowCanvas } from "../../src/features/workflow/components/workflow-canvas";
import {
  installScheduleEventBridge,
  useWorkflowStore,
} from "../../src/features/workflow/store";
import { useLicenseStore } from "../../src/features/licensing";
import type {
  Schedule,
  ScheduleLayout,
  Workflow,
} from "../../src/features/workflow/types";
import { QaModalPortal } from "./qa-modal-portal";

/**
 * The real workflow canvas with mocked IPC, so the schedule node can be
 * inspected in a browser: composition, generated arrows, drag persistence,
 * the enabled switch, and the delete-key routing. Nothing here reads a user
 * database or starts a run.
 *
 * Scenarios (`?scenario=`): `chain`, `branches`, `empty`, `disabled`,
 * `invalid`, `over-limit`.
 */

const timestamp = "2026-09-08T16:00:00Z";
const params = new URLSearchParams(location.search);
const scenario = params.get("scenario") ?? "chain";
const theme = params.get("theme") === "light" ? "light" : "dark";
const license = params.get("license") ?? "notConfigured";
applyResolvedTheme(theme);
useThemeStore.setState({ preference: theme, resolved: theme });

const schedules: Schedule[] = [];
const layouts = new Map<string, ScheduleLayout>();
const calls: { command: string; payload: unknown }[] = [];
const unhandled: string[] = [];
const errors: string[] = [];
window.addEventListener("error", (event) => errors.push(event.message));
window.addEventListener("unhandledrejection", (event) =>
  errors.push(String(event.reason)),
);

function workflow(
  id: string,
  name: string,
  graph: Workflow["graph"],
  folderId: string | null = null,
): Workflow {
  return {
    id,
    name,
    description: "Schedule canvas fixture",
    workingDirectory: "",
    folderId,
    memoryLookupEnabled: false,
    graph,
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

const chain = workflow("wf-chain", "Morning release digest", {
  nodes: [
    { id: "step-input", type: "input", position: { x: 320, y: 40 }, data: { label: "Brief", prompt: "Draft the digest" } },
    { id: "step-agent", type: "agent", position: { x: 700, y: 40 }, data: { label: "Writer", provider: "claude_code", model: null, skills: [], attachments: [], outputMode: "text", memoryIds: [] } as never },
    { id: "step-output", type: "chooseOutput", position: { x: 1060, y: 40 }, data: { label: "Digest", outputId: "digest", persist: "download" } as never },
  ],
  edges: [
    { id: "e1", source: "step-input", target: "step-agent" },
    { id: "e2", source: "step-agent", target: "step-output" },
  ],
});

const branches = workflow("wf-branches", "Two-branch publish", {
  nodes: [
    { id: "branch-a", type: "input", position: { x: 360, y: 0 }, data: { label: "Launch note", prompt: "Write the launch note" } },
    { id: "branch-b", type: "input", position: { x: 360, y: 320 }, data: { label: "Changelog", prompt: "Write the changelog" } },
    { id: "branch-a-out", type: "chooseOutput", position: { x: 760, y: 0 }, data: { label: "Note", outputId: "note", persist: "download" } as never },
    { id: "branch-b-out", type: "chooseOutput", position: { x: 760, y: 320 }, data: { label: "Changelog file", outputId: "changelog", persist: "download" } as never },
  ],
  edges: [
    { id: "b1", source: "branch-a", target: "branch-a-out" },
    { id: "b2", source: "branch-b", target: "branch-b-out" },
  ],
});

const empty = workflow("wf-empty", "Untitled workflow", { nodes: [], edges: [] });

const workflows = [chain, branches, empty];
const active =
  scenario === "branches" ? branches : scenario === "empty" ? empty : chain;

if (license === "unlicensed") {
  // A distribution build with no license: only the oldest schedule keeps the
  // single free slot, so the extra one is locked but still fully visible.
  useLicenseStore.setState({
    hasLoaded: true,
    status: {
      product: "individual",
      state: "unlicensed",
      maskedKey: null,
      benefitId: null,
      activationLabel: null,
      currentDevice: false,
      updateDeadline: null,
      inUpdateWindow: true,
      lastSuccessfulValidation: null,
      nextRefresh: null,
      offlineDeadline: null,
      errorCode: null,
    },
  });
}

if (scenario === "over-limit") {
  // Two saved schedules: the free slot belongs to the oldest one, so this
  // workflow's schedule is the extra one a license build would lock.
  schedules.push(
    { id: "sched-older", workflowId: "wf-branches", cron: "0 0 8 * * *", enabled: false, nextRunAt: null, onMissed: "run", createdAt: "2026-08-01T00:00:00Z", updatedAt: timestamp },
    { id: "sched-extra", workflowId: active.id, cron: "0 0 7 * * 1-5", enabled: false, nextRunAt: null, onMissed: "run", createdAt: "2026-09-01T00:00:00Z", updatedAt: timestamp },
  );
} else {
  schedules.push({
    id: "sched-1",
    workflowId: active.id,
    cron: scenario === "invalid" ? "nope" : "0 0 7 * * 1-5",
    enabled: scenario !== "disabled",
    nextRunAt:
      scenario === "disabled" || scenario === "invalid"
        ? null
        : "2026-09-10T07:00:00Z",
    onMissed: "run",
    createdAt: timestamp,
    updatedAt: timestamp,
  });
}

mockWindows("main");
mockIPC(
  (command, payload) => {
    calls.push({ command, payload: structuredClone(payload ?? {}) });
    switch (command) {
      case "list_workflows":
        return structuredClone(workflows);
      case "list_workflow_folders":
        return [];
      case "get_workflow":
        return structuredClone(
          workflows.find(
            (item) => item.id === (payload as { id: string }).id,
          ) ?? null,
        );
      case "list_schedules":
        return structuredClone(
          schedules.map((row) => ({
            ...row,
            workflowName:
              workflows.find((item) => item.id === row.workflowId)?.name ??
              "Workflow",
          })),
        );
      case "list_active_runs":
        return [];
      case "get_workflow_schedule":
        return structuredClone(
          schedules.find(
            (row) => row.workflowId === (payload as { workflowId: string }).workflowId,
          ) ?? null,
        );
      case "get_schedule_layout":
        return structuredClone(layouts.get((payload as { scheduleId: string }).scheduleId) ?? null);
      case "set_schedule_layout": {
        const input = payload as { scheduleId: string; x: number; y: number };
        const layout: ScheduleLayout = { ...input, updatedAt: timestamp };
        layouts.set(input.scheduleId, layout);
        return layout;
      }
      case "set_schedule_enabled": {
        const input = payload as { scheduleId: string; enabled: boolean };
        const row = schedules.find((item) => item.id === input.scheduleId);
        if (!row) throw `schedule not found: ${input.scheduleId}`;
        row.enabled = input.enabled;
        row.nextRunAt = input.enabled ? "2026-09-10T07:00:00Z" : null;
        return structuredClone(row);
      }
      case "preview_schedule_next_run": {
        const cron = (payload as { cron: string }).cron;
        if (!/^0 \d{1,2} \d{1,2} \* \* [\d,\-*]+$/.test(cron)) {
          throw `invalid cron \`${cron}\`: expected 6 fields`;
        }
        return "2026-09-10T07:00:00Z";
      }
      case "upsert_workflow_schedule": {
        const input = payload as {
          workflowId: string;
          cron: string;
          enabled: boolean;
          onMissed: Schedule["onMissed"];
        };
        const existing = schedules.find((row) => row.workflowId === input.workflowId);
        if (existing) {
          Object.assign(existing, input, { updatedAt: timestamp });
          return structuredClone(existing);
        }
        const created: Schedule = {
          id: `sched-${schedules.length + 1}`,
          ...input,
          nextRunAt: input.enabled ? "2026-09-10T07:00:00Z" : null,
          createdAt: timestamp,
          updatedAt: timestamp,
        };
        schedules.push(created);
        return structuredClone(created);
      }
      case "delete_workflow_schedule": {
        const workflowId = (payload as { workflowId: string }).workflowId;
        const index = schedules.findIndex((row) => row.workflowId === workflowId);
        if (index >= 0) schedules.splice(index, 1);
        return null;
      }
      case "webhook_base_url":
        return null;
      case "list_workflow_triggers":
      case "list_app_trigger_statuses":
      case "list_memories":
      case "list_missed_schedules":
      case "list_agent_providers":
      case "list_agent_models":
      case "get_agent_usage":
      case "list_skills":
        return [];
      case "get_agent_capability_manifest":
        return { providers: [] };
      case "get_license_status":
        return { state: "notConfigured", inUpdateWindow: true };
      case "update_workflow": {
        const input = (payload as { input: { id: string; graph: Workflow["graph"] } }).input;
        const target = workflows.find((item) => item.id === input.id);
        if (target) target.graph = input.graph;
        return structuredClone(target);
      }
      default:
        if (command.startsWith("plugin:")) return null;
        unhandled.push(command);
        throw new Error(`Unmocked schedule canvas IPC: ${command}`);
    }
  },
  { shouldMockEvents: true },
);

useWorkflowStore.setState(
  {
    ...useWorkflowStore.getInitialState(),
    workflows,
    workflowFolders: [],
    workflowSchedules: schedules.map((row) => ({
      ...row,
      workflowName:
        workflows.find((item) => item.id === row.workflowId)?.name ?? "Workflow",
    })),
    activeWorkflowId: active.id,
    openWorkflowIds: [active.id],
    nodes: active.graph.nodes,
    edges: active.graph.edges,
    schedule:
      schedules.find((row) => row.workflowId === active.id) ?? null,
    dirty: false,
  },
  true,
);

// The app installs this from `App.tsx`; the fixture mounts the canvas alone.
void installScheduleEventBridge();

const fixture = {
  calls,
  unhandled,
  errors,
  schedules,
  layouts,
  scenario,
  store: useWorkflowStore,
  preview: (cron: string) => useWorkflowStore.getState().previewScheduleCron(cron),
  /** Move a schedule's next run in the backend, as a fire would. */
  scheduleFired(nextRunAt: string | null) {
    const row = schedules.find(
      (item) => item.workflowId === useWorkflowStore.getState().activeWorkflowId,
    );
    if (row) row.nextRunAt = nextRunAt;
    return emit("scheduler://fired", 1);
  },
  node() {
    return document.querySelector<HTMLElement>(".react-flow__node-schedule");
  },
  arrows() {
    return [...document.querySelectorAll<SVGGElement>(".react-flow__edge.schedule-edge")];
  },
  scheduleCalls(command: string) {
    return calls.filter((call) => call.command === command);
  },
  setTheme(next: "light" | "dark") {
    applyResolvedTheme(next);
  },
};
(window as typeof window & { scheduleCanvasFixture?: typeof fixture })
  .scheduleCanvasFixture = fixture;

function Fixture() {
  return (
    <>
      <div className="app-frame" style={{ height: "100dvh" }}>
        <WorkflowCanvas />
      </div>
      <QaModalPortal>
        <p data-qa="fixture">
          scenario {scenario} · {schedules.length} saved schedule(s)
        </p>
      </QaModalPortal>
    </>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
