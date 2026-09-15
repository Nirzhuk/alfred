import { create } from "zustand";
import type { StateCreator } from "zustand";
import * as api from "../workflow/api";
import type {
  AgentProvider,
  MemoryReviewSettings,
} from "../workflow/types";

/**
 * SQLite (via Tauri commands) is the source of truth for memory review
 * settings; this store is a small cache patterned after the notifications and
 * integrations stores. It never holds credentials — provider CLIs remain
 * responsible for authentication.
 */

export const DEFAULT_MEMORY_REVIEW_SETTINGS: MemoryReviewSettings = {
  enabled: false,
  provider: null,
  model: null,
  maxCandidates: 5,
  updatedAt: "",
};

/** A pending, unsaved settings draft plus the UI acknowledgement flag. */
export type MemoryReviewDraft = {
  enabled: boolean;
  provider: string | null;
  model: string | null;
  acknowledged: boolean;
};

export const DEFAULT_MEMORY_REVIEW_DRAFT: MemoryReviewDraft = {
  enabled: false,
  provider: null,
  model: null,
  acknowledged: false,
};

export function memoryReviewDraftFromSettings(
  settings: Pick<MemoryReviewSettings, "enabled" | "provider" | "model">,
): MemoryReviewDraft {
  return {
    enabled: settings.enabled,
    provider: settings.provider,
    model: settings.model,
    acknowledged: settings.enabled,
  };
}

/** Stable post-run review failure codes; raw provider errors never exist. */
export type MemoryReviewFailureCode =
  | "auth_required"
  | "provider_unavailable"
  | "restricted_review_unavailable"
  | "legacy_review_unavailable"
  | "review_queue_full"
  | "timeout"
  | "invalid_response"
  | "internal";

const FAILURE_COPY: Record<MemoryReviewFailureCode, string> = {
  auth_required:
    "The selected agent CLI is not signed in. Sign in with that CLI's own login command, then retry the review.",
  provider_unavailable:
    "The selected agent CLI could not be run. Confirm it is installed and on your PATH, then retry.",
  restricted_review_unavailable:
    "Restricted review is unavailable for this provider or CLI version. No provider call was made.",
  legacy_review_unavailable:
    "This review was created by an older Alfred version and cannot be resumed safely.",
  review_queue_full:
    "The review queue is busy. The run was kept unchanged; retry the review later.",
  timeout:
    "The review took too long and was stopped. Retry it from History when convenient.",
  invalid_response:
    "The CLI returned output that could not be used safely, so no suggestions were kept. Retrying may help.",
  internal:
    "Alfred hit an unexpected local problem while reviewing this run. Retrying may help.",
};

/**
 * Map a stable failure code to user guidance. Unknown or missing codes fall
 * back to safe copy; raw provider errors must never reach this surface.
 */
export function memoryReviewFailureCopy(
  code: string | null | undefined,
): string | null {
  if (!code) return null;
  return FAILURE_COPY[code as MemoryReviewFailureCode] ?? FAILURE_COPY.internal;
}

/** Global review runs only when enabled AND a supported provider is chosen. */
export function isMemoryReviewConfigured(
  settings: Pick<MemoryReviewSettings, "enabled" | "provider">,
): boolean {
  return Boolean(settings.enabled && settings.provider);
}

/**
 * The Memories inspector's per-workflow switch stays disabled until global
 * review is configured. Returns the reason it is disabled, or null when the
 * workflow switch may be used.
 */
export function workflowSuggestionGate(
  settings: Pick<MemoryReviewSettings, "enabled" | "provider" | "availability">,
): string | null {
  if (!settings.enabled) return "Enable Memory review in Settings first.";
  if (!settings.provider)
    return "Choose a reviewer provider in Settings first.";
  if (settings.availability && !settings.availability.supported) {
    const reason = settings.availability.reason?.replace(/_/g, " ");
    return `Restricted review is unavailable${reason ? `: ${reason}.` : "."}`;
  }
  return null;
}

/**
 * Save is allowed while review stays off in any state. Turning review on (or
 * keeping it on) requires a selected provider and the explicit cost
 * acknowledgement.
 */
export function canSaveMemoryReview(draft: MemoryReviewDraft): boolean {
  if (!draft.enabled) return true;
  if (!draft.provider) return false;
  return draft.acknowledged;
}

/** Payload sent to `update_memory_review_settings`; acknowledgement is UI-only. */
export function memoryReviewSavePayload(draft: MemoryReviewDraft): {
  enabled: boolean;
  provider: string | null;
  model: string | null;
} {
  if (!draft.enabled) {
    return { enabled: false, provider: draft.provider, model: draft.model };
  }
  return {
    enabled: true,
    provider: draft.provider,
    model: draft.model || null,
  };
}

export const MEMORY_REVIEW_EXPLANATION = [
  "After each eligible completed run, Alfred may make one additional model call with the agent CLI you pick here.",
  "The review request contains fixed review instructions, a bounded digest of persisted run text (at most 32 KiB), and bounded existing-memory context selected by Alfred; it has no workspace, tools, skills, or session data.",
  "Every suggestion is candidate-only: nothing is written to your memories until you approve it.",
  "Review failures never change a run's status or output.",
];

export const MEMORY_REVIEW_ACKNOWLEDGEMENT =
  "I understand enabling Memory review adds one possible model call after eligible completed runs, sends fixed review instructions plus a bounded digest of run text and bounded existing-memory context to the selected CLI, and that every suggestion still requires my approval.";

export type MemoryReviewApi = {
  getSettings: () => Promise<MemoryReviewSettings>;
  updateSettings: typeof api.updateMemoryReviewSettings;
};

export const tauriMemoryReviewApi: MemoryReviewApi = {
  getSettings: api.getMemoryReviewSettings,
  updateSettings: api.updateMemoryReviewSettings,
};

export type MemoryReviewState = {
  settings: MemoryReviewSettings;
  providers: AgentProvider[];
  loaded: boolean;
  loading: boolean;
  saving: boolean;
  error: string | null;
  load: () => Promise<void>;
  save: (draft: MemoryReviewDraft) => Promise<boolean>;
};

type StoreParams = {
  api?: MemoryReviewApi;
};

/** Injectable-state factory so tests can stub the Tauri boundary. */
export function createMemoryReviewState({
  api: reviewApi = tauriMemoryReviewApi,
}: StoreParams = {}): StateCreator<MemoryReviewState> {
  return (set) => ({
    settings: DEFAULT_MEMORY_REVIEW_SETTINGS,
    providers: [],
    loaded: false,
    loading: false,
    saving: false,
    error: null,

    load: async () => {
      set({ loading: true, error: null });
      try {
        const [settings, providers] = await Promise.all([
          reviewApi.getSettings(),
          api.listAgentProviders(),
        ]);
        set({
          settings,
          providers,
          loaded: true,
          loading: false,
        });
      } catch {
        set({
          loading: false,
          error: "Memory review settings could not be loaded.",
        });
      }
    },

    save: async (draft) => {
      if (!canSaveMemoryReview(draft)) return false;
      set({ saving: true, error: null });
      try {
        const settings = await reviewApi.updateSettings(
          memoryReviewSavePayload(draft),
        );
        set({ settings, loaded: true, saving: false });
        return true;
      } catch {
        set({ saving: false, error: "Memory review settings could not be saved." });
        return false;
      }
    },
  });
}

export const useMemoryReviewStore = create<MemoryReviewState>()(
  createMemoryReviewState(),
);
