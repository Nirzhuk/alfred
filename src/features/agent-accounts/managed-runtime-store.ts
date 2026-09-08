import { create, type StoreApi, type UseBoundStore } from "zustand";
import {
  managedRuntimeApi,
  type ManagedRuntimeApi,
} from "./managed-runtime-api";
import type {
  ManagedRuntimeConnectionStarted,
  ManagedRuntimeConnectionStatus,
  ManagedRuntimeProduct,
} from "./managed-runtime-types";
import type { AgentProductId } from "./types";

const ERROR_MESSAGES: Record<string, string> = {
  managed_runtime_selection_invalid:
    "This managed runtime selection is not valid for the product.",
  managed_runtime_not_available:
    "This managed runtime is not available in the current build.",
  managed_runtime_state_unavailable:
    "Managed subscription state is not ready yet. Retry after the app finishes starting.",
  managed_runtime_storage_unavailable:
    "Managed subscription storage is not available in this session.",
  managed_runtime_preparing:
    "Alfred is preparing sign-in. This will finish automatically.",
  managed_runtime_operation_failed:
    "The managed runtime operation could not be completed. Try again.",
  managed_runtime_package_missing:
    "Couldn't start sign-in. This copy of Alfred doesn't include it yet.",
  managed_runtime_package_unverified:
    "The managed runtime package could not be verified.",
  managed_runtime_connection_failed:
    "The provider connection could not be started. Try again.",
  managed_runtime_connection_not_found:
    "That managed runtime connection is no longer active.",
  managed_runtime_api_key_product_invalid:
    "A managed API key can only be entered for OpenCode Go.",
  managed_runtime_api_key_invalid:
    "That OpenCode Go key could not be accepted. Check it and try again.",
  opencode_native_commercial_approval_missing:
    "OpenCode Go support is waiting for commercial distribution approval.",
  opencode_native_package_unverified:
    "The verified OpenCode Go package is not available in this build.",
  opencode_native_packaged_live_smoke_missing:
    "OpenCode Go packaged sign-in has not passed its no-installed-CLI smoke gate.",
  managed_runtime_terminal_not_found:
    "The provider terminal is no longer available. Start again.",
  managed_runtime_terminal_io_failed:
    "The provider terminal could not be reached. Start again.",
  claude_managed_package_integration_missing:
    "Alfred still needs to set up Claude sign-in in this app. You do not install a CLI.",
  claude_publisher_verification_integration_missing:
    "Claude Code publisher verification is not complete for this build.",
  claude_single_account_required:
    "Claude allows one connected subscription account. Disconnect the current one first.",
  claude_commercial_terms_unconfirmed:
    "Claude Code subscription support is waiting for commercial distribution approval.",
  codex_python_sdk_sealed_package_unverified:
    "Alfred still needs to set up ChatGPT sign-in in this app. You do not install a CLI.",
  codex_python_sdk_timed_out:
    "ChatGPT sign-in did not finish in time. Start again.",
};

export type ManagedRuntimeError = {
  code: string;
  message: string;
  recoverable: boolean;
};

export function redactManagedRuntimeProduct(
  product: ManagedRuntimeProduct,
): ManagedRuntimeProduct {
  return {
    providerId: product.providerId,
    productId: product.productId,
    productName: product.productName,
    runtimeId: product.runtimeId,
    runtimeVersion: product.runtimeVersion,
    installState: product.installState,
    connectionKind: product.connectionKind,
    connectAvailable: product.connectAvailable,
    gateCodes: product.gateCodes.filter(isSafeCode),
    billingSource: product.billingSource,
    custodyMode: product.custodyMode,
  };
}

export function redactManagedRuntimeStatus(
  status: ManagedRuntimeConnectionStatus,
): ManagedRuntimeConnectionStatus {
  return {
    providerId: status.providerId,
    productId: status.productId,
    installState: status.installState,
    connectionState: status.connectionState,
    accountId: status.accountId,
    entitlementState: status.entitlementState,
    lastErrorCode: safeCode(status.lastErrorCode),
  };
}

function isSafeCode(value: string): boolean {
  return /^[a-z][a-z0-9_]{0,95}$/.test(value);
}

function safeCode(value: string | null): string | null {
  return value && isSafeCode(value) ? value : value ? "managed_runtime_operation_failed" : null;
}

function unwrapManagedRuntimeError(error: unknown): {
  code?: unknown;
  recoverable?: unknown;
} {
  if (typeof error === "string") {
    return { code: error };
  }
  if (typeof error !== "object" || error === null) {
    return {};
  }
  const candidate = error as {
    code?: unknown;
    error?: unknown;
    message?: unknown;
    recoverable?: unknown;
  };
  if (typeof candidate.code === "string") {
    return candidate;
  }
  if (candidate.error !== undefined) {
    return unwrapManagedRuntimeError(candidate.error);
  }
  if (typeof candidate.message === "string") {
    return { code: candidate.message, recoverable: candidate.recoverable };
  }
  return candidate;
}

export function mapManagedRuntimeError(error: unknown): ManagedRuntimeError {
  const candidate = unwrapManagedRuntimeError(error);
  const candidateCode =
    typeof candidate.code === "string" ? candidate.code : "";
  const code = Object.prototype.hasOwnProperty.call(
    ERROR_MESSAGES,
    candidateCode,
  )
    ? candidateCode
    : "managed_runtime_operation_failed";
  return {
    code,
    message:
      ERROR_MESSAGES[code] ??
      "The managed runtime operation could not be completed. Try again.",
    recoverable: candidate.recoverable === true,
  };
}

function productKey(providerId: string, productId: AgentProductId): string {
  return `${providerId}:${productId}`;
}

export type ManagedRuntimeState = {
  products: ManagedRuntimeProduct[];
  statuses: Record<string, ManagedRuntimeConnectionStatus>;
  loading: boolean;
  preparingId: string | null;
  connectingId: string | null;
  error: ManagedRuntimeError | null;
  load: () => Promise<boolean>;
  prepare: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeProduct | null>;
  start: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeConnectionStarted | null>;
  refreshStatus: (
    providerId: string,
    productId: AgentProductId,
  ) => Promise<ManagedRuntimeConnectionStatus | null>;
  connectApiKey: (
    providerId: string,
    productId: AgentProductId,
    apiKey: string,
  ) => Promise<boolean>;
  clearError: () => void;
};

export type ManagedRuntimeStore = UseBoundStore<
  StoreApi<ManagedRuntimeState>
>;

export function createManagedRuntimeStore(
  api: ManagedRuntimeApi = managedRuntimeApi,
): ManagedRuntimeStore {
  let loadVersion = 0;
  return create<ManagedRuntimeState>((set) => ({
    products: [],
    statuses: {},
    loading: false,
    preparingId: null,
    connectingId: null,
    error: null,

    load: async () => {
      const version = ++loadVersion;
      set({ loading: true, error: null });
      try {
        const products = (await api.listProducts()).map(
          redactManagedRuntimeProduct,
        );
        const statuses = await Promise.all(
          products.map(async (product) => {
            try {
              return redactManagedRuntimeStatus(
                await api.connectionStatus(product.providerId, product.productId),
              );
            } catch {
              return {
                providerId: product.providerId,
                productId: product.productId,
                installState: product.installState,
                connectionState: "error" as const,
                accountId: null,
                entitlementState: "unknown" as const,
                lastErrorCode: "managed_runtime_connection_failed",
              } satisfies ManagedRuntimeConnectionStatus;
            }
          }),
        );
        if (version !== loadVersion) return true;
        set({
          products,
          statuses: Object.fromEntries(
            statuses.map((status) => [
              productKey(status.providerId, status.productId),
              status,
            ]),
          ),
          loading: false,
        });
        return true;
      } catch (error) {
        if (version !== loadVersion) return false;
        set({ loading: false, error: mapManagedRuntimeError(error) });
        return false;
      }
    },

    prepare: async (providerId, productId) => {
      const key = productKey(providerId, productId);
      set({ preparingId: key, error: null });
      try {
        const product = redactManagedRuntimeProduct(
          await api.prepareProduct(providerId, productId),
        );
        set((state) => ({
          products: state.products.map((item) =>
            productKey(item.providerId, item.productId) === key ? product : item,
          ),
          preparingId: null,
        }));
        return product;
      } catch (error) {
        set({ preparingId: null, error: mapManagedRuntimeError(error) });
        return null;
      }
    },

    start: async (providerId, productId) => {
      const key = productKey(providerId, productId);
      set({ connectingId: key, error: null });
      try {
        const started = await api.startConnection(providerId, productId);
        set({ connectingId: null });
        return started;
      } catch (error) {
        set({ connectingId: null, error: mapManagedRuntimeError(error) });
        return null;
      }
    },

    refreshStatus: async (providerId, productId) => {
      try {
        const status = redactManagedRuntimeStatus(
          await api.connectionStatus(providerId, productId),
        );
        set((state) => ({
          statuses: {
            ...state.statuses,
            [productKey(providerId, productId)]: status,
          },
        }));
        return status;
      } catch (error) {
        set({ error: mapManagedRuntimeError(error) });
        return null;
      }
    },

    connectApiKey: async (providerId, productId, apiKey) => {
      const key = productKey(providerId, productId);
      set({ connectingId: key, error: null });
      try {
        const status = redactManagedRuntimeStatus(
          await api.connectApiKey(providerId, productId, apiKey),
        );
        set((state) => ({
          statuses: {
            ...state.statuses,
            [key]: status,
          },
          connectingId: null,
        }));
        return true;
      } catch (error) {
        set({ connectingId: null, error: mapManagedRuntimeError(error) });
        return false;
      }
    },

    clearError: () => set({ error: null }),
  }));
}

export const useManagedRuntimeStore = createManagedRuntimeStore();
