import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ManagedRuntimeSettings, isSafeAuthorizationUrl } from "./managed-runtime-settings";
import type { ManagedRuntimeConnectionStatus, ManagedRuntimeProduct } from "../../managed-runtime-types";

const blockedProduct: ManagedRuntimeProduct = {
  providerId: "codex",
  productId: "chatgpt_codex",
  productName: "ChatGPT Codex",
  runtimeId: "codex_python_sdk",
  runtimeVersion: "0.147.0",
  installState: "blocked",
  connectionKind: "browser",
  connectAvailable: false,
  gateCodes: ["codex_python_sdk_sealed_package_unverified"],
  billingSource: "provider_subscription",
  custodyMode: "runtime_managed",
};

const blockedStatus: ManagedRuntimeConnectionStatus = {
  providerId: "codex",
  productId: "chatgpt_codex",
  installState: "blocked",
  connectionState: "disconnected",
  accountId: null,
  entitlementState: "unknown",
  lastErrorCode: null,
};

test("shows package preparation without offering sign-in or claiming it already started", () => {
  const markup = renderToStaticMarkup(
    <ManagedRuntimeSettings
      accounts={[]}
      snapshot={{
        products: [{ ...blockedProduct, installState: "preparing" }],
        statuses: [{ ...blockedStatus, installState: "preparing" }],
      }}
    />,
  );
  expect(markup).toContain("Preparing sign-in");
  expect(markup).not.toContain(">Signing in");
  expect(markup).not.toContain(">Sign in<");
});

test("renders product-first blocked state without advertising a runtime action", () => {
  const markup = renderToStaticMarkup(
    <ManagedRuntimeSettings
      accounts={[]}
      snapshot={{ products: [blockedProduct], statuses: [blockedStatus] }}
    />,
  );
  expect(markup).toContain("managed-runtime-settings");
  expect(markup).toContain("ChatGPT");
  expect(markup).toContain("Blocked");
  expect(markup).toContain("Alfred still needs to set up ChatGPT sign-in");
  expect(markup).not.toContain(">Connect<");
  expect(markup).not.toContain(">Install<");
  expect(markup).not.toContain("provider-token");
  expect(markup).not.toContain("apiKey");
});

test("tells ChatGPT sign-in to keep Alfred open and skip a dead localhost callback", () => {
  const chatgpt: ManagedRuntimeProduct = {
    providerId: "codex",
    productId: "chatgpt_codex",
    productName: "ChatGPT Codex",
    runtimeId: "codex_python_sdk",
    runtimeVersion: "0.147.0",
    installState: "ready",
    connectionKind: "browser",
    connectAvailable: true,
    gateCodes: [],
    billingSource: "provider_subscription",
    custodyMode: "runtime_managed",
  };
  const markup = renderToStaticMarkup(
    <ManagedRuntimeSettings
      accounts={[]}
      snapshot={{
        products: [chatgpt],
        statuses: [
          {
            providerId: "codex",
            productId: "chatgpt_codex",
            installState: "ready",
            connectionState: "connecting",
            accountId: null,
            entitlementState: "unknown",
            lastErrorCode: null,
          },
        ],
      }}
    />,
  );
  expect(markup).toContain("Sign in with ChatGPT in your browser.");
  expect(markup).not.toContain("localhost:65394");
});

test("accepts only HTTPS authorization URLs", () => {
  expect(isSafeAuthorizationUrl("https://chatgpt.com/auth")).toBe(true);
  expect(isSafeAuthorizationUrl("http://chatgpt.com/auth")).toBe(false);
  expect(isSafeAuthorizationUrl("javascript:alert(1)")).toBe(false);
});

test("offers Claude terminal and ChatGPT browser connect when packages are ready", () => {
  const claude: ManagedRuntimeProduct = {
    providerId: "claude_code",
    productId: "claude_code_subscription",
    productName: "Claude Code subscription",
    runtimeId: "claude_code_managed",
    runtimeVersion: "2.1.246",
    installState: "ready",
    connectionKind: "terminal",
    connectAvailable: true,
    gateCodes: [],
    billingSource: "provider_subscription",
    custodyMode: "runtime_managed",
  };
  const chatgpt: ManagedRuntimeProduct = {
    providerId: "codex",
    productId: "chatgpt_codex",
    productName: "ChatGPT Codex",
    runtimeId: "codex_python_sdk",
    runtimeVersion: "0.147.0",
    installState: "ready",
    connectionKind: "browser",
    connectAvailable: true,
    gateCodes: [],
    billingSource: "provider_subscription",
    custodyMode: "runtime_managed",
  };
  const markup = renderToStaticMarkup(
    <ManagedRuntimeSettings
      accounts={[]}
      snapshot={{
        products: [claude, chatgpt],
        statuses: [
          {
            providerId: "claude_code",
            productId: "claude_code_subscription",
            installState: "ready",
            connectionState: "disconnected",
            accountId: null,
            entitlementState: "unknown",
            lastErrorCode: null,
          },
          {
            providerId: "codex",
            productId: "chatgpt_codex",
            installState: "ready",
            connectionState: "disconnected",
            accountId: null,
            entitlementState: "unknown",
            lastErrorCode: null,
          },
        ],
      }}
    />,
  );
  expect(markup).toContain(">Sign in<");
  expect(markup).toContain("Claude");
  expect(markup).toContain("ChatGPT");
  expect(markup).not.toContain("Blocked");
});

test("offers Sign in when the sealed package is missing instead of a dead Set up button", () => {
  const product: ManagedRuntimeProduct = {
    providerId: "claude_code",
    productId: "claude_code_subscription",
    productName: "Claude Code subscription",
    runtimeId: "claude_code_managed",
    runtimeVersion: "2.1.246",
    installState: "missing",
    connectionKind: "terminal",
    connectAvailable: false,
    gateCodes: ["claude_managed_package_integration_missing"],
    billingSource: "provider_subscription",
    custodyMode: "runtime_managed",
  };
  const status: ManagedRuntimeConnectionStatus = {
    providerId: "claude_code",
    productId: "claude_code_subscription",
    installState: "missing",
    connectionState: "error",
    accountId: null,
    entitlementState: "unknown",
    lastErrorCode: "managed_runtime_package_missing",
  };
  const markup = renderToStaticMarkup(
    <ManagedRuntimeSettings
      accounts={[]}
      snapshot={{ products: [product], statuses: [status] }}
    />,
  );
  expect(markup).toContain("Not signed in");
  expect(markup).not.toContain("Needs attention");
  expect(markup).toContain(">Sign in<");
  expect(markup).not.toContain(">Connect<");
  expect(markup).not.toContain(">Set up<");
  expect(markup).not.toContain("Blocked");
});

test("allows Alfred to open ChatGPT and Claude sign-in URLs in the system browser", async () => {
  const capability = await Bun.file(
    new URL("../../../../../src-tauri/capabilities/default.json", import.meta.url),
  ).text();
  expect(capability).toContain('"https://chatgpt.com/*"');
  expect(capability).toContain('"https://auth.openai.com/*"');
  expect(capability).toContain('"https://claude.ai/*"');
  expect(capability).toContain('"https://console.anthropic.com/*"');
});
