import { expect, test } from "bun:test";
import type { ManagedRuntimeApi } from "./managed-runtime-api";
import {
  createManagedRuntimeStore,
  mapManagedRuntimeError,
} from "./managed-runtime-store";
import type {
  ManagedRuntimeConnectionStarted,
  ManagedRuntimeConnectionStatus,
  ManagedRuntimeProduct,
} from "./managed-runtime-types";

const product: ManagedRuntimeProduct = {
  providerId: "opencode",
  productId: "opencode_go",
  productName: "OpenCode Go",
  runtimeId: "opencode_server",
  runtimeVersion: "1.18.23",
  installState: "ready",
  connectionKind: "api_key",
  connectAvailable: true,
  gateCodes: [],
  billingSource: "provider_subscription",
  custodyMode: "runtime_managed",
};

const status: ManagedRuntimeConnectionStatus = {
  providerId: "opencode",
  productId: "opencode_go",
  installState: "ready",
  connectionState: "connected",
  accountId: "account_opaque",
  entitlementState: "eligible",
  lastErrorCode: null,
};

const started: ManagedRuntimeConnectionStarted = {
  kind: "api_key",
  attemptId: null,
  authorizationUrl: null,
  userCode: null,
  expiresAt: null,
  terminalSessionId: null,
};

function fakeApi(overrides: Partial<ManagedRuntimeApi> = {}): ManagedRuntimeApi {
  return {
    listProducts: async () => [product],
    prepareProduct: async () => product,
    startConnection: async () => started,
    connectionStatus: async () => status,
    connectApiKey: async () => status,
    readTerminal: async () => ({
      sessionId: "terminal-opaque",
      cursor: 0,
      output: "",
      closed: true,
    }),
    writeTerminal: async () => {},
    resizeTerminal: async () => {},
    closeTerminal: async () => {},
    ...overrides,
  };
}

test("a late startup response cannot overwrite completed package verification", async () => {
  let resolveStartup!: (products: ManagedRuntimeProduct[]) => void;
  let requests = 0;
  const store = createManagedRuntimeStore(fakeApi({
    listProducts: () => ++requests === 1
      ? new Promise((resolve) => { resolveStartup = resolve; })
      : Promise.resolve([product]),
  }));

  const startup = store.getState().load();
  await store.getState().load();
  expect(store.getState().products[0].installState).toBe("ready");
  resolveStartup([{ ...product, installState: "preparing", connectAvailable: false }]);
  await startup;
  expect(store.getState().products[0].installState).toBe("ready");
  expect(store.getState().loading).toBe(false);
});

test("keeps managed product state safe and never stores submitted key or raw backend detail", async () => {
  const secret = "opencode-go-secret-key";
  const store = createManagedRuntimeStore(
    fakeApi({
      connectApiKey: async (_providerId, _productId, received) => {
        expect(received).toBe(secret);
        return {
          ...status,
          credentialRef: "credential-secret",
          runtimeProfileRef: "profile-secret",
          accessToken: "token-secret",
        } as ManagedRuntimeConnectionStatus;
      },
    }),
  );
  await store.getState().load();
  expect(await store.getState().connectApiKey("opencode", "opencode_go", secret)).toBe(true);
  const serialized = JSON.stringify(store.getState());
  expect(serialized).not.toContain(secret);
  expect(serialized).not.toContain("credential-secret");
  expect(serialized).not.toContain("profile-secret");
  expect(serialized).not.toContain("token-secret");
  expect(serialized).not.toContain("apiKey");
  expect(store.getState().statuses["opencode:opencode_go"]).toEqual(status);
});

test("maps unknown managed-runtime failures to stable safe copy", () => {
  const secret = "provider-token-secret";
  const mapped = mapManagedRuntimeError({
    code: "unknown_provider_internal_detail",
    message: `unsafe ${secret}`,
    recoverable: true,
  });
  expect(mapped).toEqual({
    code: "managed_runtime_operation_failed",
    message: "The managed runtime operation could not be completed. Try again.",
    recoverable: true,
  });
  expect(JSON.stringify(mapped)).not.toContain(secret);
});

test("maps catalog state failures to retryable copy instead of unknown internals", () => {
  expect(mapManagedRuntimeError({ code: "managed_runtime_state_unavailable" })).toEqual({
    code: "managed_runtime_state_unavailable",
    message:
      "Managed subscription state is not ready yet. Retry after the app finishes starting.",
    recoverable: false,
  });
  expect(
    mapManagedRuntimeError({ error: { code: "managed_runtime_storage_unavailable" } }).code,
  ).toBe("managed_runtime_storage_unavailable");
});
