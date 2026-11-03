import type { AgentHarness, AgentProviderId } from "./types";

export type ModelOption = {
  id: string;
  label: string;
  description: string;
  baseId?: string;
  fastVariantId?: string;
  isFastVariant?: boolean;
  supportsFastToggle?: boolean;
};

export function modelOptionForValue(
  catalog: ProviderModels,
  model: string,
): ModelOption | undefined {
  return catalog.models.find(
    (option) =>
      option.id === model ||
      option.baseId === model ||
      option.fastVariantId === model,
  );
}

export function supportsFastToggle(
  option: ModelOption | undefined,
): option is ModelOption & { baseId: string; fastVariantId: string } {
  return Boolean(
    option?.supportsFastToggle === true &&
      option.baseId &&
      option.fastVariantId,
  );
}

export function isFastModel(
  option: ModelOption | undefined,
  model: string,
): boolean {
  return supportsFastToggle(option) && option?.fastVariantId === model;
}

export function modelIdForFastToggle(
  option: ModelOption | undefined,
  fast: boolean,
): string | null {
  if (!supportsFastToggle(option)) return null;
  return fast ? (option.fastVariantId ?? null) : (option.baseId ?? null);
}

export type ProviderModels = {
  provider: AgentProviderId;
  harness: AgentHarness;
  defaultModel: string;
  models: ModelOption[];
  allowCustom: boolean;
  source?: "discovered" | "fallback" | string;
  available?: boolean;
  error?: string | null;
  requiresAccount: boolean;
  supportsOAuth: boolean;
  supportsApiKey: boolean;
  supportsUsage: boolean;
  accountConnected: boolean;
  nativeRuntimeAvailable: boolean;
};

const CLI_MODEL_CAPABILITIES = {
  harness: "cli" as const,
  requiresAccount: false,
  supportsOAuth: false,
  supportsApiKey: false,
  supportsUsage: false,
  accountConnected: false,
  nativeRuntimeAvailable: false,
};

function fallbackCatalog(
  provider: AgentProviderId,
  defaultModel = "default",
  models: ModelOption[] = [
    { id: "default", label: "CLI default", description: "" },
  ],
): ProviderModels {
  return {
    provider,
    ...CLI_MODEL_CAPABILITIES,
    defaultModel,
    allowCustom: true,
    source: "fallback",
    available: false,
    models,
  };
}

/**
 * Fallback catalog if the backend isn't available yet. Version-free ids only:
 * the real list comes from each CLI at load time.
 */
export const FALLBACK_PROVIDER_MODELS: ProviderModels[] = [
  fallbackCatalog("claude_code", "sonnet", [
    { id: "sonnet", label: "Sonnet", description: "" },
    { id: "opus", label: "Opus", description: "" },
    { id: "haiku", label: "Haiku", description: "" },
    { id: "fable", label: "Fable", description: "" },
  ]),
  fallbackCatalog("cursor", "default", [
    { id: "default", label: "Auto", description: "" },
  ]),
  fallbackCatalog("codex"),
  fallbackCatalog("opencode"),
  fallbackCatalog("github_copilot"),
  fallbackCatalog("gemini", "auto", [
    { id: "auto", label: "Auto", description: "" },
    { id: "pro", label: "Pro", description: "" },
    { id: "flash", label: "Flash", description: "" },
    { id: "flash-lite", label: "Flash Lite", description: "" },
  ]),
  fallbackCatalog("grok"),
  fallbackCatalog("pi"),
  fallbackCatalog("omp"),
];

export const NATIVE_UNAVAILABLE_PROVIDER_MODELS: ProviderModels[] =
  FALLBACK_PROVIDER_MODELS.map((catalog) => ({
    provider: catalog.provider,
    harness: "alfred",
    defaultModel: "",
    models: [],
    allowCustom: false,
    source: "unavailable",
    available: false,
    error: "native_runtime_unavailable",
    requiresAccount: true,
    supportsOAuth: false,
    supportsApiKey: false,
    supportsUsage: false,
    accountConnected: false,
    nativeRuntimeAvailable: false,
  }));

export function modelsForProvider(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
  harness: AgentHarness = "cli",
): ProviderModels {
  return (
    catalogs.find(
      (catalog) =>
        catalog.provider === provider && (catalog.harness ?? "cli") === harness,
    ) ??
    (harness === "cli"
      ? FALLBACK_PROVIDER_MODELS.find((c) => c.provider === provider)
      : NATIVE_UNAVAILABLE_PROVIDER_MODELS.find((c) => c.provider === provider)) ??
    FALLBACK_PROVIDER_MODELS[0]
  );
}

export function defaultModelFor(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
  harness: AgentHarness = "cli",
): string {
  return modelsForProvider(catalogs, provider, harness).defaultModel;
}

export function selectionForAgentTarget(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
  harness: AgentHarness,
  currentModel: string | null | undefined,
): { harness: AgentHarness; model: string | null; accountRef: null } {
  const catalog = modelsForProvider(catalogs, provider, harness);
  const normalizedModel = currentModel?.trim() || null;
  const compatible = normalizedModel
    ? Boolean(modelOptionForValue(catalog, normalizedModel))
    : false;
  return {
    harness,
    model: compatible ? normalizedModel : catalog.defaultModel || null,
    accountRef: null,
  };
}
