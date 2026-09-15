import type { Edge, Node } from "@xyflow/react";
import { detectDesktopPlatform } from "../../platform";

export type AgentProviderId =
  | "claude_code"
  | "cursor"
  | "codex"
  | "opencode"
  | "github_copilot"
  | "gemini"
  | "grok"
  | "pi"
  | "omp";

export const AGENT_PROVIDER_IDS: AgentProviderId[] = [
  "claude_code",
  "cursor",
  "codex",
  "opencode",
  "github_copilot",
  "gemini",
  "grok",
  "pi",
  "omp",
];

export type AgentHarness = "cli" | "alfred";

export type SkillSource = "project" | "user";

export type Skill = {
  name: string;
  description: string;
  path: string;
  source: SkillSource;
  /** Agent-branded directory it came from; shared `.agents/skills` has none. */
  sourceAgent?: AgentProviderId | null;
  providers: AgentProviderId[];
};

/** File or folder attached to an Input node for richer prompts. */
export type InputAttachment = {
  id: string;
  path: string;
  kind: "file" | "folder";
};

/**
 * Where a script's text comes from. Shared by the Script node and the Input
 * node's "use this script" option.
 */
export type ScriptRef = {
  source: "file" | "inline";
  /** File mode. Absolute, or relative to the workflow working directory. */
  path: string;
  /** Inline mode. On macOS/Linux a leading `#!` line is honored. */
  body: string;
  /** `bash`, `python3`, `node`, `pwsh`, … Ignored when a shebang applies. */
  interpreter: string;
};

export const DEFAULT_SCRIPT_MESSAGE = "Use this script for this task:";

/** `pwsh` on Windows — there is no shebang there. */
export function defaultInterpreter(): string {
  return detectDesktopPlatform() === "windows" ? "pwsh" : "bash";
}

/**
 * Input-node option: tell the agent about a script, and optionally run it
 * first. A non-zero exit here never fails the run — the output is handed to
 * the agent instead.
 */
export type InputScript = ScriptRef & {
  /** Editable sentence placed above the path or body in the prompt. */
  message: string;
  /** Run it before the agent and append `## Script output`. */
  run: boolean;
};

export function defaultInputScript(
  source: ScriptRef["source"] = "file",
): InputScript {
  return {
    source,
    path: "",
    body: "",
    interpreter: defaultInterpreter(),
    message: DEFAULT_SCRIPT_MESSAGE,
    run: false,
  };
}

/** Input node (legacy graphs may still use type `"prompt"`). */
export type PromptNodeData = {
  label: string;
  prompt: string;
  /** Prevent accidental edits to the input's content and canvas placement. */
  blocked?: boolean;
  /** Paths the agent should consider (files and/or folders). */
  attachments?: InputAttachment[];
  /** Absent on legacy graphs and whenever the user picks "None". */
  script?: InputScript;
};

export type AgentNodeData = {
  label: string;
  provider: AgentProviderId;
  /** Missing on old graphs; read as `cli` and emitted by all new writes. */
  harness?: AgentHarness;
  /** Opaque native-account id. Never a credential or token. */
  accountRef?: string | null;
  /** CLI model id / alias, e.g. `sonnet`, `gpt-5`, `opencode/big-pickle` */
  model?: string | null;
  /** Skills to invoke, e.g. `["tdd","review"]` → `/tdd /review …` */
  skillNames?: string[];
  /** @deprecated Prefer `skillNames`. Still read when loading older graphs. */
  skillName?: string | null;
};

export function defaultAgentNodeData(
  provider: AgentProviderId,
  model: string | null = null,
): AgentNodeData {
  return {
    label: "Agent",
    provider,
    harness: "cli",
    model,
    skillNames: [],
  };
}

export function parseAgentHarness(value: unknown): AgentHarness {
  if (value === undefined) return "cli";
  if (value === "cli" || value === "alfred") return value;
  throw new Error("invalid_agent_harness");
}

export function agentHarness(data: AgentNodeData): AgentHarness {
  return parseAgentHarness(data.harness);
}

/**
 * Bring-your-own agent CLI. Runs `command` with the workflow prompt —
 * either substituted via `{{prompt}}` or piped on stdin.
 */
export type CustomAgentNodeData = {
  kind: "customAgent";
  label: string;
  /**
   * Shell command. For `template` mode include `{{prompt}}` (quoted as needed).
   * Example: `my-agent --print "{{prompt}}"`
   */
  command: string;
  /** How the workflow prompt is passed to the command. */
  promptMode: "template" | "stdin";
};

export function defaultCustomAgentNodeData(
  label = "Custom agent",
): CustomAgentNodeData {
  return {
    kind: "customAgent",
    label,
    command: "",
    promptMode: "template",
  };
}

/** Resolved skill list for an agent node (supports legacy `skillName`). */
export function agentSkillNames(data: AgentNodeData): string[] {
  if (Array.isArray(data.skillNames)) {
    return data.skillNames.map((s) => s.trim()).filter(Boolean);
  }
  const legacy = data.skillName?.trim();
  return legacy ? [legacy] : [];
}

/**
 * Disposition sink after an agent: capture text/files into memory and/or
 * publish as the run’s final result. Graph type id stays `chooseOutput`.
 */
export type OutputNodeData = {
  label: string;
  /** Persist this step’s body as a workflow memory. */
  saveToMemory: boolean;
  /** Pin that memory when saving. */
  pinMemory: boolean;
  /** Override memory title; empty → node label / "Output". */
  memoryTitle?: string;
  /** Prefer this body as the run's final output when the run completes. */
  asFinalResult: boolean;
  /** Ask the nearest upstream agent to return a complete HTML report. */
  htmlReport: boolean;
  /** Append upstream agent filesChanged into the saved body. */
  includeFilesChanged: boolean;
};

/** @deprecated Use `OutputNodeData`. Kept for existing imports. */
export type ChooseOutputNodeData = OutputNodeData;

export function defaultOutputNodeData(label = "Output"): OutputNodeData {
  return {
    label,
    saveToMemory: true,
    pinMemory: false,
    memoryTitle: "",
    asFinalResult: true,
    htmlReport: false,
    includeFilesChanged: true,
  };
}

/** Normalize legacy `selectedKey` graphs and fill disposition defaults. */
export function normalizeOutputNodeData(
  data: Record<string, unknown>,
): OutputNodeData {
  const label =
    typeof data.label === "string" && data.label.trim()
      ? data.label
      : "Output";
  return {
    label,
    saveToMemory:
      typeof data.saveToMemory === "boolean" ? data.saveToMemory : true,
    pinMemory: typeof data.pinMemory === "boolean" ? data.pinMemory : false,
    memoryTitle:
      typeof data.memoryTitle === "string" ? data.memoryTitle : "",
    asFinalResult:
      typeof data.asFinalResult === "boolean" ? data.asFinalResult : true,
    htmlReport: typeof data.htmlReport === "boolean" ? data.htmlReport : false,
    includeFilesChanged:
      typeof data.includeFilesChanged === "boolean"
        ? data.includeFilesChanged
        : true,
  };
}

/** Pulls selected (often cross-workflow) memories into run context. */
export type MemoryNodeData = {
  label: string;
  memoryIds: string[];
};

export type IdentityNodeData = {
  kind: "identity";
  label: string;
  role: string;
  personality: string;
  instructions: string;
};

export function defaultIdentityNodeData(label = "Identity"): IdentityNodeData {
  return { kind: "identity", label, role: "", personality: "", instructions: "" };
}

export function normalizeIdentityNodeData(
  data: Record<string, unknown>,
): IdentityNodeData {
  return {
    kind: "identity",
    label:
      typeof data.label === "string" && data.label.trim() ? data.label : "Identity",
    role: typeof data.role === "string" ? data.role : "",
    personality: typeof data.personality === "string" ? data.personality : "",
    instructions: typeof data.instructions === "string" ? data.instructions : "",
  };
}

/** Rewrite / append to context using `{{context}}`, `{{output}}`, `{{cwd}}`. */
export type TemplateNodeData = {
  kind: "template";
  label: string;
  template: string;
  /** replace = overwrite context; append = add after existing context. */
  mode: "replace" | "append";
};

export function defaultTemplateNodeData(label = "Template"): TemplateNodeData {
  return {
    kind: "template",
    label,
    template: "{{output}}",
    mode: "append",
  };
}

/** Inject file/folder contents into context without an agent. */
export type FileInjectNodeData = {
  kind: "fileInject";
  label: string;
  paths: string[];
};

export function defaultFileInjectNodeData(
  label = "File inject",
): FileInjectNodeData {
  return { kind: "fileInject", label, paths: [] };
}

/** Snapshot git status (and optional diff) into context. */
export type GitStatusNodeData = {
  kind: "gitStatus";
  label: string;
  /** Include `git diff` (unstaged + staged) after the status listing. */
  includeDiff: boolean;
};

export function defaultGitStatusNodeData(
  label = "Git status",
): GitStatusNodeData {
  return { kind: "gitStatus", label, includeDiff: true };
}

/** Run a local shell command in the workflow working directory. */
export type ShellNodeData = {
  kind: "shell";
  label: string;
  command: string;
  /** Append stdout/stderr into context_prompt after the run. */
  appendOutput: boolean;
};

export function defaultShellNodeData(label = "Shell"): ShellNodeData {
  return { kind: "shell", label, command: "", appendOutput: true };
}

/**
 * Run a script and pass its stdout downstream. Supersedes `shell` in the
 * palette. Upstream output arrives on stdin and in `$ALFRED_OUTPUT`; a
 * non-zero exit fails the step.
 */
export type ScriptNodeData = ScriptRef & {
  kind: "script";
  label: string;
  /** Append stdout/stderr into context_prompt after the run. */
  appendOutput: boolean;
};

export function defaultScriptNodeData(label = "Script"): ScriptNodeData {
  return {
    kind: "script",
    label,
    source: "inline",
    path: "",
    body: "",
    interpreter: defaultInterpreter(),
    appendOutput: true,
  };
}

/** HTTP request; response body is appended to context. */
export type HttpNodeData = {
  kind: "http";
  label: string;
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  url: string;
  /** Optional raw body (templates supported). */
  body: string;
  /** Optional headers as `Name: value` lines. */
  headers: string;
};

export function defaultHttpNodeData(label = "HTTP"): HttpNodeData {
  return {
    kind: "http",
    label,
    method: "GET",
    url: "",
    body: "",
    headers: "",
  };
}

/** Desktop notification and/or outbound webhook. */
export type NotifyNodeData = {
  kind: "notify";
  label: string;
  title: string;
  body: string;
  desktop: boolean;
  /** Optional webhook URL (POST JSON `{ title, body }`). */
  webhookUrl: string;
};

export function defaultNotifyNodeData(label = "Notify"): NotifyNodeData {
  return {
    kind: "notify",
    label,
    title: "Alfred",
    body: "{{output}}",
    desktop: true,
    webhookUrl: "",
  };
}

/** Persist text to a file on disk. */
export type WriteFileNodeData = {
  kind: "writeFile";
  label: string;
  /** Absolute path, or relative to the workflow working directory. */
  path: string;
  /** Content template (`{{output}}`, `{{context}}`, …). */
  content: string;
};

export function defaultWriteFileNodeData(
  label = "Write file",
): WriteFileNodeData {
  return { kind: "writeFile", label, path: "", content: "{{output}}" };
}

/** Create a GitHub PR or issue via the `gh` CLI. */
export type GitHostNodeData = {
  kind: "gitHost";
  label: string;
  action: "pr" | "issue";
  /** `owner/name` (empty = repository of the working directory). */
  repo?: string;
  title: string;
  body: string;
  /** PR only — base branch (empty = repo default). */
  base: string;
  /** PR only — head branch (empty = current branch of the working directory). */
  head?: string;
  /** PR only — create as draft. */
  draft: boolean;
};

export function defaultGitHostNodeData(
  label = "Create PR",
  action: "pr" | "issue" = "pr",
): GitHostNodeData {
  return {
    kind: "gitHost",
    label,
    action,
    repo: "",
    title: "",
    body: "{{output}}",
    base: "",
    head: "",
    draft: false,
  };
}

/** Provider-neutral connected-app action. All fields are non-secret; Rust
 * resolves `connectionId` to a credential at execution time. */
export type AppActionNodeData = {
  type: "appAction";
  label: string;
  providerId: string;
  actionId: string;
  connectionId: string;
  input: Record<string, unknown>;
};

export function defaultAppActionNodeData(
  label = "App action",
): AppActionNodeData {
  return {
    type: "appAction",
    label,
    providerId: "",
    actionId: "",
    connectionId: "",
    input: {},
  };
}

export type WorkflowNodeData =
  | PromptNodeData
  | AgentNodeData
  | CustomAgentNodeData
  | OutputNodeData
  | MemoryNodeData
  | IdentityNodeData
  | TemplateNodeData
  | FileInjectNodeData
  | GitStatusNodeData
  | ShellNodeData
  | ScriptNodeData
  | HttpNodeData
  | NotifyNodeData
  | WriteFileNodeData
  | GitHostNodeData
  | AppActionNodeData;

export type WorkflowNode = Node<WorkflowNodeData>;
export type WorkflowEdge = Edge;

export type WorkflowGraph = {
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
};

export type MemoryLookupAvailability = {
  available: boolean;
  reason: string | null;
};

export type Workflow = {
  id: string;
  name: string;
  description: string;
  /** Absolute path passed to agent CLIs as cwd. Empty = process default. */
  workingDirectory?: string;
  /** Sidebar organization folder. Null/empty means Unfiled. */
  folderId?: string | null;
  /** Host-side local memory retrieval before agent and custom-agent steps. */
  memoryRetrievalEnabled?: boolean;
  /** Permission for agents to request bounded memory lookup during a run. */
  memoryLookupEnabled: boolean;
  graph: WorkflowGraph;
  createdAt: string;
  updatedAt: string;
};

export type WorkflowFolder = {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
};

export type AgentProvider = {
  id: AgentProviderId;
  label: string;
  defaultModel?: string;
  harnesses?: AgentHarnessCapability[];
};

export type AgentAccountReference = {
  id: string;
  label: string;
};

export type AgentHarnessCapability = {
  harness: AgentHarness;
  available: boolean;
  requiresAccount: boolean;
  supportsOAuth: boolean;
  supportsApiKey: boolean;
  supportsUsage: boolean;
  connected: boolean;
  accounts: AgentAccountReference[];
  error?: string | null;
};

export type RunSummary = {
  id: string;
  workflowId: string;
  trigger: string;
  status: string;
  createdAt: string;
};

export type RunHistoryItem = {
  id: string;
  workflowId: string;
  workflowName: string;
  trigger: string;
  status: string;
  error: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
  stepCount: number;
  finalOutputPreview: string;
};

export type RunHistoryStep = {
  id: string;
  nodeId: string;
  agentProvider: string | null;
  skillName: string | null;
  status: string;
  input: unknown;
  output: unknown;
  error: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
};

export type OutputReceiptState =
  | "not_requested"
  | "skipped_empty"
  | "pending"
  | "saving"
  | "saved"
  | "failed"
  | "deleted";

export type OutputPersistenceErrorCode =
  | "invalid_disposition"
  | "step_output_unavailable"
  | "artifact_stage_io"
  | "artifact_finalize_io"
  | "artifact_content_mismatch"
  | "database_write"
  | "internal";

export type OutputDisposition = Omit<OutputNodeData, "memoryTitle"> & {
  memoryTitle: string;
};

export type OutputReceipt = {
  runId: string;
  nodeId: string;
  stepId: string;
  disposition: OutputDisposition;
  resolvedMemoryTitle: string;
  state: OutputReceiptState;
  storageIdentity: string | null;
  outputSha256: string | null;
  artifactRequired: boolean;
  memoryId: string | null;
  errorCode: OutputPersistenceErrorCode | null;
  savingStartedAt: string | null;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
};

export type OutputPersistenceEvent = {
  workflowId: string;
  receipt: OutputReceipt;
};

export type RetryOutputPersistenceRequest = {
  runId: string;
  nodeId: string;
};

export type RunHistoryDetail = {
  run: RunHistoryItem;
  steps: RunHistoryStep[];
  memoryUses: RunHistoryMemoryUse[];
  memoryContextSummaries: RunHistoryMemoryContextSummary[];
  evidenceSources: EvidenceSource[];
  outputReceipts?: OutputReceipt[];
};

export type EvidenceOrigin =
  | "authored_task"
  | "configured_workflow_instruction"
  | "external_trigger_data"
  | "host_recorded_observation"
  | "model_output"
  | "prior_memory"
  | "retrieved_historical_summary";

export type EvidenceSubject =
  | "user_statement"
  | "workflow_configuration"
  | "recorded_observation";

export type EvidenceCaptureState =
  | { state: "complete"; originalBytes: number }
  | { state: "truncated"; originalBytes: number; capturedBytes: number }
  | {
      state: "omitted";
      originalBytes: number;
      reason: "run_byte_limit" | "sensitive_content" | "observation_unavailable";
    };

export type EvidenceSource = {
  sourceId: string;
  runId: string;
  stepId: string | null;
  nodeId: string | null;
  origin: EvidenceOrigin;
  producerIdentity: string | null;
  sourceLabel: string | null;
  observedAt: string;
  normalizationVersion: number;
  normalizedText: string;
  captureState: EvidenceCaptureState;
  digest: string;
};

export type RunHistoryMemoryContextSummary = {
  nodeId: string;
  budgetBytes: number;
  memoryBytes: number;
  includedCount: number;
  explicitOmittedCount: number;
  pinnedOmittedCount: number;
  automaticOmittedCount: number;
  unavailableCount: number;
  repeatedCount: number;
  traceAvailable: boolean;
  createdAt: string;
};

export type RunHistoryMemoryUse = {
  nodeId: string;
  memoryId: string;
  memoryTitle: string;
  scopeType: MemoryScopeType;
  memoryType: MemoryType;
  rank: number;
  score: number;
  reason: "explicit" | "lexical" | "recent" | "pinned";
  renderedBytes: number;
  rankingVersion:
    | "explicit_v1"
    | "lexical_v1"
    | "lexical_v2"
    | "pinned_v1"
    | string;
  createdAt: string;
};

export type HistorySearchInput = {
  query: string;
  workflowId?: string | null;
  limit?: number;
};

export type HistorySearchHit = {
  kind: "run_step" | "memory";
  sourceId: string;
  runId: string | null;
  workflowId: string | null;
  workflowName: string;
  title: string;
  snippet: string;
  timestamp: string;
  rank: number;
};

export type ActiveRunInfo = {
  runId: string;
  workflowId: string;
  workflowName: string;
};

export type RunStepStatus = "pending" | "running" | "completed" | "failed";

export type ChangedFileStatus = "created" | "modified" | "deleted" | "renamed";

export type ChangedFile = {
  /** Absolute path — resolved server-side against the workflow's cwd. */
  path: string;
  status: ChangedFileStatus;
};

/** Tokens, cost, and timing an agent CLI reported for a step (best effort — a
 * CLI that doesn't report a figure just omits that field). */
export type AgentStepStats = {
  provider?: string;
  model?: string;
  durationMs?: number;
  numTurns?: number;
  inputTokens?: number;
  outputTokens?: number;
  reasoningTokens?: number;
  cacheReadTokens?: number;
  cacheCreationTokens?: number;
  totalCostUsd?: number;
  /** Files the workflow's git working tree gained/lost during this step. */
  filesChanged?: ChangedFile[];
};

/** A provider-owned subscription window, or an explicitly labelled local estimate. */
export type AgentUsageWindow = {
  label: string;
  usedPercent: number;
  resetsAt?: number | null;
  resetDescription?: string | null;
};

export type AgentUsageSnapshot = {
  provider: AgentProviderId;
  connected: boolean;
  windows: AgentUsageWindow[];
  source: string;
  updatedAt: string;
  error?: string | null;
};

export type AgentActivityKind =
  | "status"
  | "assistant"
  | "tool"
  | "command"
  | "file"
  | "error";

export type AgentActivityState = "started" | "completed";

/** Safe, provider-neutral activity suitable for the live run console. */
export type AgentActivity = {
  /** Stable across the started/completed lifecycle for one activity. */
  id: string;
  kind: AgentActivityKind;
  state: AgentActivityState;
  label: string;
  detail?: string | null;
};

export type AgentAuthRequired = {
  provider: AgentProviderId;
  label: string;
  loginCommand: string;
};

export type RunEvent = {
  runId: string;
  workflowId: string;
  kind:
    | "started"
    | "step_started"
    | "step_log"
    | "step_completed"
    | "step_failed"
    | "completed"
    | "failed"
    | string;
  nodeId?: string | null;
  nodeType?: string | null;
  nodeLabel?: string | null;
  status?: string | null;
  message: string;
  output?: string | null;
  activity?: AgentActivity | null;
  authRequired?: AgentAuthRequired;
  stats?: AgentStepStats | null;
  at: string;
};

export type RunLogLine = {
  id: string;
  at: string;
  kind: string;
  nodeId?: string | null;
  nodeLabel?: string | null;
  message: string;
  output?: string | null;
  activity?: AgentActivity | null;
  status?: string | null;
};

export type MemoryKind = "text" | "note" | "artifact";
export type MemorySource = "run" | "manual" | "import" | "review";
export type MemoryScopeType = "user" | "workspace" | "workflow";
export type MemoryType =
  | "preference"
  | "fact"
  | "decision"
  | "constraint"
  | "lesson"
  | "episode"
  | "checkpoint"
  | "note"
  | "output"
  | "artifact";
export type MemoryStatus = "active" | "superseded" | "retracted";
export type MemoryProvenanceState =
  | "legacy_unknown"
  | "manual"
  | "evidence_verified"
  | "source_removed";

export type MemoryCandidateOperation = "create" | "supersede" | "retract";
export type MemoryCandidateStatus =
  | "pending"
  | "approved"
  | "rejected"
  | "blocked";
export type MemoryReviewJobStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "skipped";

/** Global post-run memory review settings (SQLite-backed singleton). */
export type MemoryReviewSettings = {
  enabled: boolean;
  provider: string | null;
  model: string | null;
  maxCandidates: number;
  updatedAt: string;
  availability?: MemoryReviewAvailability;
};

export type MemoryReviewAvailability = {
  supported: boolean;
  reason: string | null;
  product: string | null;
  runtimeVersion: string | null;
  executionMode: string | null;
  restrictionContractVersion: string;
};

/** Per-workflow review toggle. */
export type WorkflowMemoryReview = {
  workflowId: string;
  enabled: boolean;
  updatedAt: string;
};

/** Review job metadata — never carries raw provider errors or output. */
export type MemoryReviewJob = {
  runId: string;
  workflowId: string;
  status: MemoryReviewJobStatus;
  provider: string;
  providerProduct: string | null;
  executionMode: string | null;
  capabilityVersion: string | null;
  restrictionContractVersion: string | null;
  model: string | null;
  errorCode: string | null;
  candidateCount: number;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
};

/** A model-proposed memory change awaiting user approval. */
export type MemoryCandidate = {
  id: string;
  reviewRunId: string;
  workflowId: string;
  sourceNodeId: string | null;
  operation: MemoryCandidateOperation;
  targetMemoryId: string | null;
  candidateRevision: number;
  expectedTargetMemoryId: string | null;
  expectedTargetSemanticRevision: number | null;
  expectedTargetContentDigest: string | null;
  targetComplete: boolean | null;
  scopeType: MemoryScopeType;
  scopeKey: string;
  memoryType: MemoryType;
  title: string;
  body: string;
  confidence: number;
  rationale: string;
  factKey: string | null;
  observedAt: string | null;
  lastConfirmedAt: string | null;
  validFrom: string | null;
  expiresAt: string | null;
  possibleConflicts: PossibleMemoryConflict[];
  curationContractVersion: number;
  evidenceState:
    | "verified"
    | "legacy_unavailable"
    | "needs_reconfirmation"
    | "source_removed";
  evidenceSubject: EvidenceSubject | null;
  isInference: boolean;
  claimDigest: string | null;
  evidence: MemoryEvidenceItem[];
  status: MemoryCandidateStatus;
  blockedCode: string | null;
  createdAt: string;
  decidedAt: string | null;
};

export type PossibleMemoryConflict = {
  memoryId: string;
  title: string;
  relation: "exact_duplicate" | "same_fact_key" | "possible_relationship";
};

export type MemoryEvidenceItem = {
  ordinal: number;
  sourceId: string | null;
  fragmentId: string;
  sourceRunId: string;
  sourceStepId: string | null;
  sourceNodeId: string | null;
  origin: EvidenceOrigin;
  producerIdentity: string | null;
  sourceLabel: string | null;
  quote: string | null;
  byteStart: number | null;
  byteEnd: number | null;
  normalizationVersion: number | null;
  fragmentDigest: string | null;
  sourceRemoved: boolean;
};

/** Durable workflow memory — SQLite-backed, optionally pinned into agent prompts. */
export type MemoryOrigin = "owned" | "linked" | "linkable" | "inherited";

export type OutputMemory = {
  id: string;
  workflowId: string | null;
  runId?: string | null;
  nodeId?: string | null;
  kind: MemoryKind;
  scopeType: MemoryScopeType;
  scopeKey: string;
  scopeLabel: string;
  memoryType: MemoryType;
  source: MemorySource;
  title: string;
  body: string;
  artifactPath?: string | null;
  artifactContentDigest: string | null;
  pinned: boolean;
  confidence: number;
  salience: number;
  status: MemoryStatus;
  semanticRevision: number;
  contentDigest: string;
  supersedesId?: string | null;
  factKey: string | null;
  observedAt: string | null;
  lastConfirmedAt: string | null;
  validFrom: string | null;
  expiresAt: string | null;
  createdAt: string;
  updatedAt: string;
  provenanceState?: MemoryProvenanceState;
  evidenceSubject?: EvidenceSubject | null;
  evidenceIsInference?: boolean;
  /** `"owned"` for local memories; `"linked"` when imported from another workflow. */
  origin?: MemoryOrigin;
  /** Owning workflow name when `origin` is linked/linkable. */
  sourceWorkflowName?: string | null;
};

/**
 * What Alfred does with a scheduled time that passed while it was closed:
 * run as soon as Alfred opens, drop the run, or ask per schedule.
 */
export type OnMissed = "run" | "skip" | "ask";

export type Schedule = {
  id: string;
  workflowId: string;
  /** 6-field cron: sec min hour day month dow */
  cron: string;
  enabled: boolean;
  nextRunAt: string | null;
  onMissed: OnMissed;
  createdAt: string;
  updatedAt: string;
};

/** Schedule row for the schedules page (includes workflow name). */
export type ScheduleListItem = Schedule & {
  workflowName: string;
};

/**
 * A scheduled run that was missed while Alfred was closed and waits on the
 * user's decision. `missedAt` is the scheduled time that passed.
 */
export type MissedSchedule = {
  scheduleId: string;
  workflowId: string;
  workflowName: string;
  cron: string;
  missedAt: string;
};

export type TriggerSource = "file" | "webhook" | "app";

/** A file trigger's `config`. `pattern` empty means any file. */
export type FileTriggerConfig = {
  path: string;
  pattern?: string;
  debounceMs?: number;
};

export type AppTriggerConfig = {
  providerId: string;
  eventType: string;
  connectionId: string;
  filters: Record<string, unknown>;
  descriptorVersion: number;
};

export type AppTriggerStatus = {
  triggerId: string;
  cursorPresent: boolean;
  subscriptionActive: boolean;
  expiresAt: string | null;
  lastPolledAt: string | null;
  lastSuccessAt: string | null;
  lastErrorCode: string | null;
  nextAttemptAt: string | null;
  overrunCount: number;
  pendingCount: number;
};

/** Event trigger — a workflow can have many, unlike schedules. */
export type Trigger = {
  id: string;
  workflowId: string;
  source: TriggerSource;
  label: string;
  config: FileTriggerConfig | AppTriggerConfig | Record<string, unknown>;
  /** Bearer token for `webhook` triggers. */
  secret: string | null;
  enabled: boolean;
  lastFiredAt: string | null;
  createdAt: string;
  updatedAt: string;
};

export type SchedulePreset = {
  id: string;
  label: string;
  cron: string;
};

/** Presets use 6-field cron (with seconds) for the Rust `cron` crate. */
export const SCHEDULE_PRESETS: SchedulePreset[] = [
  { id: "hourly", label: "Every hour", cron: "0 0 * * * *" },
  { id: "every_15m", label: "Every 15 minutes", cron: "0 */15 * * * *" },
  { id: "daily_9", label: "Every day at 09:00", cron: "0 0 9 * * *" },
  { id: "weekdays_9", label: "Weekdays at 09:00", cron: "0 0 9 * * 1-5" },
  { id: "monday_9", label: "Mondays at 09:00", cron: "0 0 9 * * 1" },
];

export const emptyGraph = (): WorkflowGraph => ({
  nodes: [],
  edges: [],
});

function hasKind<K extends string>(
  data: WorkflowNodeData,
  kind: K,
): data is WorkflowNodeData & { kind: K } {
  return "kind" in data && (data as { kind?: string }).kind === kind;
}

export function isAppActionNodeData(
  data: WorkflowNodeData,
): data is AppActionNodeData {
  return "type" in data && data.type === "appAction";
}

export function isAgentNodeData(data: WorkflowNodeData): data is AgentNodeData {
  return !("kind" in data) && "provider" in data;
}

export function isPromptNodeData(
  data: WorkflowNodeData,
): data is PromptNodeData {
  return !("kind" in data) && "prompt" in data;
}

export function isMemoryNodeData(
  data: WorkflowNodeData,
): data is MemoryNodeData {
  return !("kind" in data) && "memoryIds" in data;
}

export function isOutputNodeData(
  data: WorkflowNodeData,
): data is OutputNodeData {
  if ("kind" in data) return false;
  return (
    "saveToMemory" in data ||
    "asFinalResult" in data ||
    "htmlReport" in data ||
    ("selectedKey" in data &&
      !("prompt" in data) &&
      !("provider" in data) &&
      !("memoryIds" in data))
  );
}

/** @deprecated Use `isOutputNodeData`. */
export function isChooseOutputNodeData(
  data: WorkflowNodeData,
): data is OutputNodeData {
  return isOutputNodeData(data);
}

export function isTemplateNodeData(
  data: WorkflowNodeData,
): data is TemplateNodeData {
  return hasKind(data, "template");
}

export function isIdentityNodeData(
  data: WorkflowNodeData,
): data is IdentityNodeData {
  return hasKind(data, "identity");
}

export function isFileInjectNodeData(
  data: WorkflowNodeData,
): data is FileInjectNodeData {
  return hasKind(data, "fileInject");
}

export function isGitStatusNodeData(
  data: WorkflowNodeData,
): data is GitStatusNodeData {
  return hasKind(data, "gitStatus");
}

export function isShellNodeData(data: WorkflowNodeData): data is ShellNodeData {
  return hasKind(data, "shell");
}

export function isScriptNodeData(
  data: WorkflowNodeData,
): data is ScriptNodeData {
  return hasKind(data, "script");
}

export function isHttpNodeData(data: WorkflowNodeData): data is HttpNodeData {
  return hasKind(data, "http");
}

export function isNotifyNodeData(
  data: WorkflowNodeData,
): data is NotifyNodeData {
  return hasKind(data, "notify");
}

export function isWriteFileNodeData(
  data: WorkflowNodeData,
): data is WriteFileNodeData {
  return hasKind(data, "writeFile");
}

export function isGitHostNodeData(
  data: WorkflowNodeData,
): data is GitHostNodeData {
  return hasKind(data, "gitHost");
}

export function isCustomAgentNodeData(
  data: WorkflowNodeData,
): data is CustomAgentNodeData {
  return hasKind(data, "customAgent");
}

/** Human title for settings / context menus. */
export function titleForNodeType(type: string | undefined): string {
  switch (type) {
    case "prompt":
    case "input":
      return "Input";
    case "memory":
      return "Memories";
    case "identity":
      return "Identity";
    case "chooseOutput":
      return "Output";
    case "agent":
      return "Agent";
    case "customAgent":
      return "Custom agent";
    case "template":
      return "Template";
    case "fileInject":
      return "File inject";
    case "gitStatus":
      return "Git status";
    case "shell":
      return "Shell";
    case "script":
      return "Script";
    case "http":
      return "HTTP";
    case "notify":
      return "Notify";
    case "writeFile":
      return "Write file";
    case "gitHost":
      return "GitHub";
    case "appAction":
      return "App action";
    default:
      return "Step";
  }
}
