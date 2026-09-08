import { useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { ConfirmDialog } from "../../../../components/confirm-dialog";
import { AgentMark } from "../../../../components/agent-mark";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useAgentAccountsStore } from "../../store";
import type { AgentAccount } from "../../types";
import {
  ManagedRuntimeApiKey,
} from "../managed-runtime-api-key";
import { ManagedRuntimeTerminal } from "../managed-runtime-terminal";
import {
  useManagedRuntimeStore,
} from "../../managed-runtime-store";
import type {
  ManagedRuntimeConnectionStarted,
  ManagedRuntimeConnectionStatus,
  ManagedRuntimeProduct,
} from "../../managed-runtime-types";
import type { AgentProductId } from "../../types";

const MANAGED_PRODUCT_IDS = new Set<AgentProductId>([
  "claude_code_subscription",
  "chatgpt_codex",
  "opencode_go",
]);

const GATE_MESSAGES: Record<string, string> = {
  claude_commercial_terms_unconfirmed:
    "Claude Code subscription support is waiting for commercial distribution approval.",
  claude_managed_package_integration_missing:
    "Alfred still needs to set up Claude sign-in in this app. You do not install a CLI.",
  claude_publisher_verification_integration_missing:
    "Alfred still needs to set up Claude sign-in in this app. You do not install a CLI.",
  claude_packaged_no_cli_smoke_missing:
    "Claude Code packaged sign-in has not passed its no-installed-CLI smoke gate.",
  claude_native_workflow_renderer_approval_missing:
    "Claude's provider-owned terminal is available only for sign-in; custom workflow rendering is not approved.",
  codex_python_sdk_host_approval_unavailable:
    "Codex native tool approval is not available through the public SDK surface.",
  codex_sdk_host_approval_unavailable:
    "Codex native tool approval is not available through the public SDK surface.",
  codex_python_sdk_public_capability_audit_blocked:
    "Codex native capability audit is incomplete, so this runtime stays blocked.",
  codex_python_sdk_known_client_enterprise_clearance_missing:
    "Codex's Alfred client and enterprise-account behavior are awaiting provider clearance.",
  codex_python_sdk_sealed_package_unverified:
    "Alfred still needs to set up ChatGPT sign-in in this app. You do not install a CLI.",
  codex_python_sdk_packaged_smoke_missing:
    "Codex packaged sign-in has not passed its no-installed-CLI smoke gate.",
  opencode_commercial_terms_unconfirmed:
    "OpenCode Go support is waiting for commercial distribution approval.",
  opencode_native_commercial_approval_missing:
    "OpenCode Go support is waiting for commercial distribution approval.",
  opencode_package_account_and_tool_bridge_unverified:
    "OpenCode Go's verified runtime, account bridge, and tool approval are not complete.",
  opencode_native_package_unverified:
    "Alfred still needs to set up OpenCode in this app. You do not install a CLI.",
  opencode_native_supervisor_http_unavailable:
    "OpenCode Go's managed server handoff is not available in this build.",
  opencode_native_supervisor_http_capability_unavailable:
    "OpenCode Go's managed server handoff is not available in this build.",
  opencode_native_secret_entry_unavailable:
    "OpenCode Go's secure subscription-key entry flow is not available yet.",
  opencode_native_host_approval_bridge_unavailable:
    "OpenCode Go's host approval bridge is not available in this build.",
  opencode_native_packaged_live_smoke_missing:
    "OpenCode Go packaged sign-in has not passed its no-installed-CLI smoke gate.",
  opencode_managed_package_integration_missing:
    "The verified OpenCode Go package is not connected to the desktop installer yet.",
  opencode_publisher_verification_integration_missing:
    "OpenCode publisher verification is not complete for this build.",
  opencode_packaged_smoke_missing:
    "OpenCode Go packaged sign-in has not passed its no-installed-CLI smoke gate.",
  native_capability_manifest_invalid:
    "This build's native capability manifest is invalid, so managed runtimes stay blocked.",
  native_capability_manifest_entry_missing:
    "This build does not declare a managed runtime for this product.",
};

type ManagedRuntimeSnapshot = {
  products: ManagedRuntimeProduct[];
  statuses?: ManagedRuntimeConnectionStatus[];
};

type ManagedRuntimeSettingsProps = {
  accounts: AgentAccount[];
  snapshot?: ManagedRuntimeSnapshot;
};

type Ceremony =
  | {
      product: ManagedRuntimeProduct;
      started: ManagedRuntimeConnectionStarted;
    }
  | null;

type PendingLogout = {
  accountId: string;
  product: ManagedRuntimeProduct;
  account: AgentAccount | null;
};

export function ManagedRuntimeSettings({
  accounts,
  snapshot,
}: ManagedRuntimeSettingsProps) {
  const storeProducts = useManagedRuntimeStore((state) => state.products);
  const storeStatuses = useManagedRuntimeStore((state) => state.statuses);
  const loading = useManagedRuntimeStore((state) => state.loading);
  const preparingId = useManagedRuntimeStore((state) => state.preparingId);
  const connectingId = useManagedRuntimeStore((state) => state.connectingId);
  const error = useManagedRuntimeStore((state) => state.error);
  const load = useManagedRuntimeStore((state) => state.load);
  const prepare = useManagedRuntimeStore((state) => state.prepare);
  const start = useManagedRuntimeStore((state) => state.start);
  const refreshStatus = useManagedRuntimeStore((state) => state.refreshStatus);
  const clearError = useManagedRuntimeStore((state) => state.clearError);
  const refreshAccounts = useAgentAccountsStore((state) => state.load);
  const disconnectAccount = useAgentAccountsStore((state) => state.disconnect);
  const busyAccountId = useAgentAccountsStore((state) => state.busyId);
  const accountError = useAgentAccountsStore((state) => state.error);
  const [ceremony, setCeremony] = useState<Ceremony>(null);
  const [apiKeyProduct, setApiKeyProduct] = useState<ManagedRuntimeProduct | null>(
    null,
  );
  const [terminalProduct, setTerminalProduct] = useState<{
    product: ManagedRuntimeProduct;
    sessionId: string;
  } | null>(null);
  const [pendingLogout, setPendingLogout] = useState<PendingLogout | null>(null);
  const [pendingRemove, setPendingRemove] = useState<PendingLogout | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);

  useEffect(() => {
    if (snapshot) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("managed-runtime://products-changed", () => {
      if (!disposed) void load();
    }).then((stop) => {
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
      void load();
    }).catch(() => {
      if (!disposed) void load();
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load, snapshot]);

  const products = snapshot?.products ?? storeProducts;
  const statuses = useMemo(() => {
    if (!snapshot?.statuses) return storeStatuses;
    return Object.fromEntries(
      snapshot.statuses.map((status) => [
        managedProductKey(status.providerId, status.productId),
        status,
      ]),
    );
  }, [snapshot, storeStatuses]);

  useEffect(() => {
    if (snapshot) return;
    const connectingProducts = products.filter((product) => {
      const status = statuses[managedProductKey(product.providerId, product.productId)];
      return status?.connectionState === "connecting";
    });
    if (connectingProducts.length === 0) return;
    const timer = window.setInterval(() => {
      for (const product of connectingProducts) {
        void refreshStatus(product.providerId, product.productId);
      }
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [products, refreshStatus, snapshot, statuses]);

  useEffect(() => {
    const connected = ceremony?.started
      ? statuses[
          managedProductKey(
            ceremony.product.providerId,
            ceremony.product.productId,
          )
        ]
      : null;
    if (
      connected?.connectionState === "connected" ||
      connected?.connectionState === "limited"
    ) {
      setCeremony(null);
      void refreshAccounts();
    }
  }, [ceremony, refreshAccounts, statuses]);

  const begin = async (product: ManagedRuntimeProduct) => {
    if (isBlocked(product)) return;
    clearError();
    setLocalError(null);
    let current = product;
    if (current.installState === "missing" || current.installState === "failed") {
      const prepared = await prepare(current.providerId, current.productId);
      if (prepared === null) return;
      current = prepared;
    }
    if (current.installState !== "ready") {
      setLocalError(
        "Couldn't start sign-in. This copy of Alfred doesn't include it yet.",
      );
      return;
    }
    if (current.connectionKind === "api_key") {
      if (current.productId !== "opencode_go") {
        setLocalError("This managed product's key flow is unavailable.");
        return;
      }
      setApiKeyProduct(current);
      return;
    }
    const started = await start(current.providerId, current.productId);
    if (started === null) return;
    void refreshStatus(current.providerId, current.productId);
    if (started.kind === "terminal") {
      if (
        current.productId !== "claude_code_subscription" ||
        started.terminalSessionId === undefined ||
        started.terminalSessionId === null
      ) {
        setLocalError("The provider terminal could not be started for this product.");
        return;
      }
      setTerminalProduct({ product: current, sessionId: started.terminalSessionId });
      return;
    }
    if (started.kind !== "browser" && started.kind !== "device_code") {
      setLocalError("The provider returned an unsupported sign-in flow.");
      return;
    }
    setCeremony({ product: current, started });
    if (started.authorizationUrl && isSafeAuthorizationUrl(started.authorizationUrl)) {
      void openUrl(started.authorizationUrl).catch(() => {
        setLocalError("The provider sign-in page could not be opened.");
      });
    }
  };

  const refreshProduct = (product: ManagedRuntimeProduct) => {
    void refreshStatus(product.providerId, product.productId);
  };

  const confirmLogout = async () => {
    if (!pendingLogout) return;
    const pending = pendingLogout;
    setPendingLogout(null);
    const removed = await disconnectAccount(pending.accountId);
    if (removed) {
      await refreshStatus(pending.product.providerId, pending.product.productId);
      void refreshAccounts();
      return;
    }
    const latest = useAgentAccountsStore
      .getState()
      .accounts.find((account) => account.id === pending.accountId);
    if (latest?.status === "disconnect_pending") {
      setPendingRemove({ ...pending, account: latest });
    }
  };

  const confirmRemove = async () => {
    if (!pendingRemove) return;
    const pending = pendingRemove;
    setPendingRemove(null);
    await disconnectAccount(pending.accountId, true);
    await refreshStatus(pending.product.providerId, pending.product.productId);
  };

  return (
    <section
      className="managed-runtime-settings"
      aria-label="Subscription sign-in"
    >
      {error || localError || accountError ? (
        <div className="integrations-error" role="alert">
          <span>{error?.message ?? localError ?? accountError?.message}</span>
          {error ? (
            <button
              type="button"
              className="settings-link"
              onClick={() => {
                clearError();
                void load();
              }}
            >
              Retry
            </button>
          ) : (
            <button
              type="button"
              className="settings-link"
              onClick={() => {
                clearError();
                setLocalError(null);
              }}
            >
              Dismiss
            </button>
          )}
        </div>
      ) : null}

      <div className="settings-section-heading">
        <div>
          <h2>Sign in with a subscription</h2>
          <p className="settings-section-copy">
            Alfred opens Claude or ChatGPT sign-in. API keys stay optional below.
          </p>
        </div>
      </div>

      <div className="settings-card managed-runtime-card">
        {products.length === 0 && loading ? (
          <div className="settings-row">
            <p className="settings-value">Loading managed subscription products…</p>
          </div>
        ) : null}
        {products.length === 0 && !loading && !snapshot && error ? (
          <div className="settings-row">
            <p className="settings-value">
              Could not load Claude and ChatGPT sign-in. Retry.
            </p>
          </div>
        ) : null}
        {products.length === 0 && !loading && !snapshot && !error ? (
          <div className="settings-row">
            <p className="settings-value">
              Subscription sign-in is not available in this copy of the app.
            </p>
          </div>
        ) : null}
        {products.map((product) => {
          const key = managedProductKey(product.providerId, product.productId);
          const storedStatus =
            statuses[key] ?? disconnectedStatusFor(product);
          const awaitingProviderAuth = Boolean(
            (ceremony &&
              managedProductKey(
                ceremony.product.providerId,
                ceremony.product.productId,
              ) === key) ||
              (terminalProduct &&
                managedProductKey(
                  terminalProduct.product.providerId,
                  terminalProduct.product.productId,
                ) === key),
          );
          const status = awaitingProviderAuth
            ? { ...storedStatus, connectionState: "connecting" as const }
            : storedStatus;
          const account = findAccount(accounts, status.accountId, product.productId);
          const busy = preparingId === key || connectingId === key;
          return (
            <ManagedRuntimeProductRow
              key={key}
              product={product}
              status={status}
              account={account}
              busy={
                busy ||
                (status.accountId !== null && busyAccountId === status.accountId)
              }
              onConnect={() => void begin(product)}
              onRefresh={() => refreshProduct(product)}
              onLogout={() =>
                status.accountId
                  ? setPendingLogout({ accountId: status.accountId, product, account })
                  : null
              }
              onRemove={() =>
                status.accountId
                  ? setPendingRemove({ accountId: status.accountId, product, account })
                  : null
              }
            />
          );
        })}
      </div>

      {ceremony ? (
        <ManagedRuntimeCeremony
          product={ceremony.product}
          started={ceremony.started}
          onClose={() => setCeremony(null)}
        />
      ) : null}

      {apiKeyProduct ? (
        <ManagedRuntimeApiKey
          providerId={apiKeyProduct.providerId}
          productId={apiKeyProduct.productId}
          productName={apiKeyProduct.productName}
          onClose={() => setApiKeyProduct(null)}
          onConnected={() => {
            void refreshAccounts();
            void refreshStatus(apiKeyProduct.providerId, apiKeyProduct.productId);
          }}
        />
      ) : null}

      {terminalProduct ? (
        <ManagedRuntimeTerminal
          productName={terminalProduct.product.productName}
          sessionId={terminalProduct.sessionId}
          onClose={() => {
            setTerminalProduct(null);
            void refreshStatus(
              terminalProduct.product.providerId,
              terminalProduct.product.productId,
            );
          }}
        />
      ) : null}

      {pendingLogout ? (
        <ConfirmDialog
          title={`Log out of ${pendingLogout.product.productName}?`}
          message={
            pendingLogout.account
              ? runtimeLogoutMessage(pendingLogout.account)
              : "Alfred will ask the isolated provider runtime to sign out, then remove this local account metadata. Provider-side sessions may remain active."
          }
          confirmLabel="Log out"
          danger
          onCancel={() => setPendingLogout(null)}
          onConfirm={() => void confirmLogout()}
        />
      ) : null}

      {pendingRemove ? (
        <ConfirmDialog
          title="Remove local account metadata?"
          message="The managed runtime could not finish sign-out. Remove the local recovery record only after completing provider-side sign-out when supported."
          confirmLabel="Remove"
          danger
          onCancel={() => setPendingRemove(null)}
          onConfirm={() => void confirmRemove()}
        />
      ) : null}
    </section>
  );
}

type ProductRowProps = {
  product: ManagedRuntimeProduct;
  status: ManagedRuntimeConnectionStatus;
  account: AgentAccount | null;
  busy: boolean;
  onConnect: () => void;
  onRefresh: () => void;
  onLogout: () => void;
  onRemove: () => void;
};

function ManagedRuntimeProductRow({
  product,
  status,
  account,
  busy,
  onConnect,
  onRefresh,
  onLogout,
  onRemove,
}: ProductRowProps) {
  const blocked = isBlocked(product);
  const connected =
    status.connectionState === "connected" ||
    status.connectionState === "limited" ||
    Boolean(account);
  const displayName = productDisplayName(product);
  const actionLabel = blocked
    ? "Unavailable"
    : status.connectionState === "connecting"
      ? "Signing in…"
      : connected
        ? "Sign in again"
        : product.connectionKind === "api_key"
          ? "Add key"
          : "Sign in";
  const actionDisabled =
    blocked ||
    busy ||
    product.installState === "preparing" ||
    status.connectionState === "connecting";
  const statusLabel = managedStatusLabel(product, status, connected);
  const statusClass = managedStatusClass(product, status, connected);
  const gateMessages = uniqueGateMessages(
    product,
    blocked ? status.lastErrorCode : null,
  );

  return (
    <div className="settings-row managed-runtime-product-row">
      <div className="managed-runtime-product-copy">
        <span className="native-agent-provider-mark" aria-hidden>
          <AgentMark
            provider={product.providerId}
            label={displayName}
            size={20}
          />
        </span>
        <div className="managed-runtime-product-text">
          <p className="settings-label">{displayName}</p>
          <p className="settings-value">
            {productSignInHint(product)}
          </p>
          <div className="managed-runtime-state-line">
            <span className={`integration-status ${statusClass}`}>
              {statusLabel}
            </span>
            {status.entitlementState !== "unknown" ? (
              <span className="integration-scopes">
                {entitlementLabel(status.entitlementState)}
              </span>
            ) : null}
          </div>
          {blocked && gateMessages.length > 0 ? (
            <div className="managed-runtime-gates">
              {gateMessages.map((message) => (
                <p className="settings-value native-agent-gate" key={message}>
                  {message}
                </p>
              ))}
            </div>
          ) : null}
          {blocked === false &&
          status.lastErrorCode &&
          product.installState === "ready" ? (
            <p className="settings-value native-agent-gate">
              {gateMessageFor(status.lastErrorCode)}
            </p>
          ) : null}
          {connected && account ? (
            <p className="settings-value managed-runtime-account-label">
              {account.displayName ?? "Provider account connected"}
            </p>
          ) : null}
        </div>
      </div>

      <div className="integration-actions managed-runtime-actions">
        {product.installState === "preparing" ? (
          <span className="managed-runtime-preparing" role="status">
            Preparing sign-in…
          </span>
        ) : blocked === false && connected === false ? (
          <button
            type="button"
            className="integration-action integration-connect"
            disabled={actionDisabled}
            onClick={onConnect}
          >
            {busy ? "Signing in…" : actionLabel}
          </button>
        ) : null}
        {connected ? (
          <>
            {account?.status === "disconnect_pending" ? (
              <>
                <button
                  type="button"
                  className="ghost integration-action"
                  disabled={busy || !status.accountId}
                  onClick={onLogout}
                >
                  Retry logout
                </button>
                <button
                  type="button"
                  className="ghost danger integration-action"
                  disabled={busy || !status.accountId}
                  onClick={onRemove}
                >
                  Remove
                </button>
              </>
            ) : !blocked ? (
              <button
                type="button"
                className="ghost integration-action"
                disabled={actionDisabled}
                onClick={onConnect}
              >
                {actionLabel}
              </button>
            ) : null}
            {account?.status !== "disconnect_pending" ? (
              <button
                type="button"
                className="ghost danger integration-action"
                disabled={busy || !status.accountId}
                onClick={onLogout}
              >
                Logout
              </button>
            ) : null}
          </>
        ) : null}
        {status.connectionState === "error" && product.installState === "ready" ? (
          <button
            type="button"
            className="ghost integration-action"
            disabled={busy || blocked}
            onClick={onRefresh}
          >
            Check again
          </button>
        ) : null}
      </div>
    </div>
  );
}

type CeremonyProps = {
  product: ManagedRuntimeProduct;
  started: ManagedRuntimeConnectionStarted;
  onClose: () => void;
};

function ManagedRuntimeCeremony({ product, started, onClose }: CeremonyProps) {
  const titleId = `managed-runtime-ceremony-${product.productId}-title`;
  const descriptionId = `managed-runtime-ceremony-${product.productId}-description`;
  const safeUrl = started.authorizationUrl && isSafeAuthorizationUrl(started.authorizationUrl)
    ? started.authorizationUrl
    : null;
  return (
    <Modal
      size="md"
      className="managed-runtime-ceremony-modal"
      onClose={onClose}
      labelledBy={titleId}
      describedBy={descriptionId}
    >
      <ModalHeader
        leading={
          <span className="native-agent-provider-mark" aria-hidden>
            <AgentMark provider={product.providerId} label={product.productName} size={20} />
          </span>
        }
        title={`Sign in with ${productDisplayName(product)}`}
        titleId={titleId}
        description="Finish sign-in in your browser, and keep Alfred open. If the browser then shows a localhost page that will not load, close that tab and click Sign in again. Do not paste that localhost address."
        descriptionId={descriptionId}
        actions={
          <button type="button" className="ghost integration-action" onClick={onClose}>
            Close
          </button>
        }
      />
      <div className="managed-runtime-ceremony-body">
        {safeUrl ? (
          <a
            className="managed-runtime-safe-url"
            href={safeUrl}
            target="_blank"
            rel="noreferrer"
            onClick={(event) => {
              event.preventDefault();
              void openUrl(safeUrl).catch(() => {
                // begin() already reports a failed open; this retry uses the same opener.
              });
            }}
          >
            Open provider sign-in
          </a>
        ) : null}
        {product.productId === "chatgpt_codex" ? (
          <p className="settings-value">
            Keep this Alfred window open until ChatGPT finishes. A localhost tab after ChatGPT is Alfred collecting the result — if it will not load, start Sign in again.
          </p>
        ) : null}
        {started.userCode ? (
          <code className="native-agent-user-code">{started.userCode}</code>
        ) : null}
        {started.expiresAt ? (
          <p className="settings-value">This sign-in request expires {safeDate(started.expiresAt)}.</p>
        ) : null}
        {!safeUrl && !started.userCode ? (
          <p className="settings-value">The provider sign-in request is active. Return here when it finishes.</p>
        ) : null}
      </div>
    </Modal>
  );
}

function managedProductKey(providerId: string, productId: AgentProductId): string {
  return `${providerId}:${productId}`;
}

function disconnectedStatusFor(
  product: ManagedRuntimeProduct,
): ManagedRuntimeConnectionStatus {
  return {
    providerId: product.providerId,
    productId: product.productId,
    installState: product.installState,
    connectionState: "disconnected",
    accountId: null,
    entitlementState: "unknown",
    lastErrorCode: null,
  };
}

function findAccount(
  accounts: AgentAccount[],
  accountId: string | null,
  productId: AgentProductId,
): AgentAccount | null {
  if (!accountId) return null;
  return (
    accounts.find(
      (account) => account.productId === productId && account.id === accountId,
    ) ?? null
  );
}

function productDisplayName(product: ManagedRuntimeProduct): string {
  if (product.productId === "claude_code_subscription") return "Claude";
  if (product.productId === "chatgpt_codex") return "ChatGPT";
  if (product.productId === "opencode_go") return "OpenCode";
  return product.productName;
}

function uniqueGateMessages(
  product: ManagedRuntimeProduct,
  lastErrorCode: string | null,
): string[] {
  const codes = [
    ...product.gateCodes,
    ...(lastErrorCode ? [lastErrorCode] : []),
  ];
  return [...new Set(codes.map((code) => gateMessageFor(code)))];
}

function productSignInHint(product: ManagedRuntimeProduct): string {
  if (product.productId === "claude_code_subscription") {
    return "Sign in with your Claude account. Nothing extra to install.";
  }
  if (product.productId === "chatgpt_codex") {
    return "Sign in with ChatGPT in your browser.";
  }
  if (product.productId === "opencode_go") {
    return "Paste your OpenCode Go key. Nothing extra to install.";
  }
  return "Sign in with the provider. Nothing extra to install.";
}

function isBlocked(product: ManagedRuntimeProduct): boolean {
  return (
    product.installState === "blocked" ||
    product.connectionKind === "unsupported"
  );
}

function managedStatusLabel(
  product: ManagedRuntimeProduct,
  status: ManagedRuntimeConnectionStatus,
  connected: boolean,
): string {
  if (isBlocked(product)) return "Blocked";
  if (product.installState === "preparing") return "Preparing";
  if (product.installState === "missing") return "Not signed in";
  if (product.installState === "failed") return "Needs attention";
  if (product.connectAvailable === false && product.installState === "blocked") {
    return "Unavailable";
  }
  if (status.connectionState === "connecting") return "Signing in";
  if (status.connectionState === "limited") return "Limited";
  if (status.connectionState === "error") return "Needs attention";
  if (connected) return "Signed in";
  return "Not signed in";
}

function managedStatusClass(
  product: ManagedRuntimeProduct,
  status: ManagedRuntimeConnectionStatus,
  connected: boolean,
): string {
  if (isBlocked(product)) return "is-error";
  if (product.installState === "failed") return "is-error";
  if (status.connectionState === "connecting") return "is-connecting";
  if (status.connectionState === "limited") return "is-limited";
  if (status.connectionState === "error") return "is-error";
  if (connected) return "is-connected";
  return "is-disconnected";
}

function gateMessageFor(code: string | undefined): string {
  if (!code) return "This managed runtime is blocked by the current build's release gates.";
  return (
    GATE_MESSAGES[code] ??
    "This managed runtime is blocked by the current build's release gates."
  );
}

function entitlementLabel(state: ManagedRuntimeConnectionStatus["entitlementState"]): string {
  if (state === "eligible") return "Subscription eligible";
  if (state === "limited") return "Subscription limited";
  if (state === "exhausted") return "Subscription exhausted";
  if (state === "ineligible") return "Subscription unavailable";
  return "Entitlement unknown";
}

function runtimeLogoutMessage(account: AgentAccount): string {
  return `Alfred will ask the isolated ${account.productName} runtime to sign out, then remove local account metadata. Provider-side sessions may remain active when the provider cannot revoke them.`;
}

export function isSafeAuthorizationUrl(value: string): boolean {
  try {
    return new URL(value).protocol === "https:";
  } catch {
    return false;
  }
}

function safeDate(value: string): string {
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp)
    ? new Date(timestamp).toLocaleString()
    : "when the provider request expires";
}

export function isManagedProductId(productId: AgentProductId): boolean {
  return MANAGED_PRODUCT_IDS.has(productId);
}
