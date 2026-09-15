import { useEffect, useMemo, useState } from "react";
import { Icon } from "../../../../components/icon";
import { SearchInput } from "../../../../components/search-input";
import { Modal, ModalHeader } from "../../../../components/modal";
import {
  compatibleEventConnections,
  emptyAppTriggerConfig,
  loadEventResourceOptions,
  selectAppEvent,
  selectAppEventProvider,
  validateAppEventForm,
} from "../../../integrations/app-event-form";
import { integrationsApi } from "../../../integrations/api";
import { useIntegrationsStore } from "../../../integrations/store";
import { LockedCapability } from "../../../licensing/components";
import type { CapabilityDecision } from "../../../licensing";
import { useWorkflowCapability } from "../../capability";
import { useWorkflowStore } from "../../store";
import type {
  ActionFieldDescriptor,
  AppEventDescriptor,
  AppEventResourceItem,
} from "../../../integrations/types";
import type {
  AppTriggerConfig,
  AppTriggerStatus,
  FileTriggerConfig,
  Trigger,
  TriggerSource,
} from "../../types";

function formatWhen(value: string | null | undefined) {
  if (!value) return "never";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function fileConfig(trigger: Trigger): FileTriggerConfig {
  const config = trigger.config as FileTriggerConfig;
  return {
    path: config?.path ?? "",
    pattern: config?.pattern ?? "",
    debounceMs: config?.debounceMs ?? 2000,
  };
}

function appConfig(trigger: Trigger): AppTriggerConfig | null {
  if (trigger.source !== "app") return null;
  const config = trigger.config as Partial<AppTriggerConfig>;
  if (
    typeof config.providerId !== "string" ||
    typeof config.eventType !== "string" ||
    typeof config.connectionId !== "string"
  ) {
    return null;
  }
  return {
    providerId: config.providerId,
    eventType: config.eventType,
    connectionId: config.connectionId,
    filters:
      typeof config.filters === "object" && config.filters !== null
        ? config.filters
        : {},
    descriptorVersion: config.descriptorVersion ?? 1,
  };
}

function hookUrl(baseUrl: string | null, trigger: Trigger) {
  if (!baseUrl) return null;
  return `${baseUrl}/hooks/${trigger.id}`;
}

function curlFor(baseUrl: string | null, trigger: Trigger) {
  const url = hookUrl(baseUrl, trigger);
  if (!url || !trigger.secret) return null;
  return `curl -X POST ${url} \\\n  -H "X-Alfred-Token: ${trigger.secret}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"hello":"world"}'`;
}

type Props = {
  workflowId: string;
  workflowName: string;
  onClose: () => void;
  /** Test seam: overrides the resolver's answer. Production never passes it;
   * the build kind is compile-time fact either way. */
  decision?: CapabilityDecision;
};

export function TriggersModal({ workflowId, workflowName, onClose, decision }: Props) {
  const resolved = useWorkflowCapability("triggers");
  const gate = decision ?? resolved;
  const triggers = useWorkflowStore((state) => state.triggers);
  const statuses = useWorkflowStore((state) => state.appTriggerStatuses);
  const webhookBase = useWorkflowStore((state) => state.webhookBaseUrl);
  const loading = useWorkflowStore((state) => state.loading);
  const loadTriggers = useWorkflowStore((state) => state.loadTriggers);
  const saveTrigger = useWorkflowStore((state) => state.saveTrigger);
  const removeTrigger = useWorkflowStore((state) => state.removeTrigger);
  const testTrigger = useWorkflowStore((state) => state.testTrigger);

  const providers = useIntegrationsStore((state) => state.providers);
  const connections = useIntegrationsStore((state) => state.connections);
  const descriptors = useIntegrationsStore((state) => state.eventDescriptors);
  const loadIntegrations = useIntegrationsStore((state) => state.load);

  const [source, setSource] = useState<TriggerSource>("file");
  const [label, setLabel] = useState("");
  const [path, setPath] = useState("");
  const [pattern, setPattern] = useState("");
  const [connectedApp, setConnectedApp] = useState<AppTriggerConfig>(
    emptyAppTriggerConfig,
  );
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    void loadTriggers(workflowId);
    void loadIntegrations();
    const refresh = window.setInterval(() => {
      void loadTriggers(workflowId);
    }, 5_000);
    return () => window.clearInterval(refresh);
  }, [workflowId, loadTriggers, loadIntegrations]);

  const providerEvents = descriptors.filter(
    (descriptor) => descriptor.providerId === connectedApp.providerId,
  );
  const descriptor =
    descriptors.find(
      (candidate) =>
        candidate.providerId === connectedApp.providerId &&
        candidate.eventType === connectedApp.eventType,
    ) ?? null;
  const availableConnections = compatibleEventConnections(
    connections,
    descriptor,
  );
  const appErrors = validateAppEventForm(connectedApp, descriptor);
  const canAdd =
    source === "webhook" ||
    (source === "file"
      ? path.trim().length > 0
      : appErrors.length === 0);

  async function copy(id: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(id);
      window.setTimeout(() => setCopied(null), 1500);
    } catch {
      /* clipboard blocked — the value is on screen anyway */
    }
  }

  async function addTrigger() {
    const config =
      source === "file"
        ? { path: path.trim(), pattern: pattern.trim(), debounceMs: 2000 }
        : source === "app"
          ? connectedApp
          : {};
    const created = await saveTrigger({
      workflowId,
      source,
      label: label.trim(),
      config,
    });
    if (created) {
      setLabel("");
      setPath("");
      setPattern("");
      setConnectedApp(emptyAppTriggerConfig());
    }
  }

  return (
    <Modal
      size="md"
      onClose={onClose}
      labelledBy="triggers-modal-title"
      describedBy="triggers-modal-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="arrow-clockwise" size={20} />
          </span>
        }
        title={`Triggers for ${workflowName}`}
        titleId="triggers-modal-title"
        description="Choose which events can start this workflow automatically."
        descriptionId="triggers-modal-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={onClose}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />

      {gate.available ? (
        <div className="schedule-modal-body">
        <p className="muted">
          Start this automation when a file changes, a local HTTP request
          arrives, or a connected app reports an event. Local triggers run only
          while Alfred is open, including in the tray.
        </p>

        {triggers.length === 0 ? (
          <p className="hint">No triggers yet.</p>
        ) : (
          <ul className="trigger-list">
            {triggers.map((trigger) => (
              <TriggerItem
                key={trigger.id}
                trigger={trigger}
                workflowId={workflowId}
                webhookBase={webhookBase}
                status={statuses.find(
                  (candidate) => candidate.triggerId === trigger.id,
                )}
                descriptors={descriptors}
                connections={connections}
                copied={copied === trigger.id}
                loading={loading}
                onCopy={(text) => void copy(trigger.id, text)}
                onSave={(enabled) =>
                  void saveTrigger({
                    id: trigger.id,
                    workflowId,
                    source: trigger.source,
                    label: trigger.label,
                    config: trigger.config as Record<string, unknown>,
                    enabled,
                  })
                }
                onTest={() => void testTrigger(trigger.id)}
                onRemove={() => void removeTrigger(trigger.id)}
              />
            ))}
          </ul>
        )}

        <hr />

        <label className="field">
          <span>Add trigger</span>
          <select
            value={source}
            onChange={(event) =>
              setSource(event.currentTarget.value as TriggerSource)
            }
          >
            <option value="file">File change</option>
            <option value="webhook">Webhook (local HTTP POST)</option>
            <option value="app">Connected app</option>
          </select>
        </label>

        <label className="field">
          <span>Name (optional)</span>
          <input
            type="text"
            value={label}
            placeholder={
              source === "file"
                ? "Repo saves"
                : source === "app"
                  ? "Workspace mentions"
                  : "Local webhook"
            }
            onChange={(event) => setLabel(event.currentTarget.value)}
          />
        </label>

        {source === "file" ? (
          <FileTriggerFields
            path={path}
            pattern={pattern}
            onPath={setPath}
            onPattern={setPattern}
          />
        ) : source === "app" ? (
          <ConnectedAppFields
            config={connectedApp}
            descriptor={descriptor}
            providers={providers}
            providerEvents={providerEvents}
            connections={availableConnections}
            errors={appErrors}
            onChange={setConnectedApp}
          />
        ) : (
          <p className="hint">
            A URL and token are generated on save. This listener is bound to
            localhost and is separate from connected-app credentials.
          </p>
        )}

        <div className="schedule-actions">
          <button
            type="button"
            className="primary"
            disabled={loading || !canAdd}
            onClick={() => void addTrigger()}
          >
            Add trigger
          </button>
        </div>
        </div>
      ) : (
        <TriggersLockedContent triggers={triggers} />
      )}
    </Modal>
  );
}

/**
 * The locked treatment for the trigger authoring surface (Plan 008 Step 4):
 * the shared LockedCapability card, plus every saved trigger stated plainly
 * and read-only — a lock never hides or drops what is already persisted.
 * Exported so tests can render it statically without the portal-based modal
 * shell.
 */
export function TriggersLockedContent({ triggers }: { triggers: Trigger[] }) {
  return (
    <div className="schedule-modal-body" data-locked-capability="triggers">
      <LockedCapability
        capabilityName="Triggers"
        description="Start this workflow from a file change, a webhook, or a connected-app event while Alfred is open. Manual runs stay free in every build."
      />
      <p className="muted">Saved triggers, kept exactly as configured:</p>
      {triggers.length === 0 ? (
        <p className="hint">No triggers yet.</p>
      ) : (
        <ul className="trigger-list">
          {triggers.map((trigger) => (
            <li key={trigger.id}>
              {trigger.label || trigger.source} — {trigger.source} (
              {trigger.enabled ? "enabled" : "paused"})
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function TriggerItem({
  trigger,
  workflowId,
  webhookBase,
  status,
  descriptors,
  connections,
  copied,
  loading,
  onCopy,
  onSave,
  onTest,
  onRemove,
}: {
  trigger: Trigger;
  workflowId: string;
  webhookBase: string | null;
  status?: AppTriggerStatus;
  descriptors: AppEventDescriptor[];
  connections: ReturnType<typeof useIntegrationsStore.getState>["connections"];
  copied: boolean;
  loading: boolean;
  onCopy: (text: string) => void;
  onSave: (enabled: boolean) => void;
  onTest: () => void;
  onRemove: () => void;
}) {
  const file = fileConfig(trigger);
  const app = appConfig(trigger);
  const url = hookUrl(webhookBase, trigger);
  const curl = curlFor(webhookBase, trigger);
  const eventDescriptor = app
    ? descriptors.find(
        (candidate) =>
          candidate.providerId === app.providerId &&
          candidate.eventType === app.eventType,
      )
    : null;
  const connection = app
    ? connections.find((candidate) => candidate.id === app.connectionId)
    : null;
  const fallbackLabel =
    trigger.source === "file"
      ? "File change"
      : trigger.source === "app"
        ? eventDescriptor?.label ?? app?.eventType ?? "Connected app"
        : "Webhook";

  return (
    <li className="trigger-item" data-workflow-id={workflowId}>
      <div className="trigger-item-head">
        <strong>{trigger.label || fallbackLabel}</strong>
        <span className="schedule-badge">
          {trigger.source === "app" ? "connected app" : trigger.source}
        </span>
        <label className="checkbox-field">
          <input
            type="checkbox"
            checked={trigger.enabled}
            onChange={(event) => onSave(event.currentTarget.checked)}
          />
          <span>Enabled</span>
        </label>
      </div>

      {trigger.source === "file" ? (
        <p className="hint">
          Watching <code>{file.path}</code>
          {file.pattern ? (
            <>
              {" "}matching <code>{file.pattern}</code>
            </>
          ) : null}
        </p>
      ) : trigger.source === "app" ? (
        <AppTriggerHealth
          descriptor={eventDescriptor ?? undefined}
          connectionLabel={
            connection?.displayName ??
            connection?.externalAccountId ??
            app?.connectionId ??
            "Unknown connection"
          }
          connectionStatus={connection?.status}
          status={status}
        />
      ) : url ? (
        <>
          <p className="hint">
            <code>POST {url}</code>
          </p>
          <div className="schedule-actions">
            <button
              type="button"
              className="ghost"
              onClick={() => onCopy(curl ?? url)}
            >
              {copied ? "Copied" : "Copy curl"}
            </button>
          </div>
        </>
      ) : (
        <p className="hint">
          Listener is not running — port in use? Set
          <code> ALFRED_HTTP_PORT</code> and restart.
        </p>
      )}

      <p className="hint">
        Last fired: <strong>{formatWhen(trigger.lastFiredAt)}</strong>
      </p>
      <div className="schedule-actions">
        <button type="button" className="ghost" disabled={loading} onClick={onTest}>
          Test run
        </button>
        <button
          type="button"
          className="ghost danger"
          disabled={loading}
          onClick={onRemove}
        >
          Remove
        </button>
      </div>
    </li>
  );
}

function AppTriggerHealth({
  descriptor,
  connectionLabel,
  connectionStatus,
  status,
}: {
  descriptor?: AppEventDescriptor;
  connectionLabel: string;
  connectionStatus?: string;
  status?: AppTriggerStatus;
}) {
  const localMode = descriptor?.deliveryModes.includes("socket")
    ? "Local socket"
    : descriptor?.deliveryModes.includes("polling")
      ? "Local polling"
      : "Local subscription";
  return (
    <div className="app-trigger-health">
      <p className="hint">
        {localMode} · {connectionLabel} · runs while Alfred is open
      </p>
      <p className="hint">
        Last success: <strong>{formatWhen(status?.lastSuccessAt)}</strong>
        {status?.pendingCount ? ` · ${status.pendingCount} queued` : ""}
        {status?.overrunCount ? ` · ${status.overrunCount} dropped` : ""}
      </p>
      {connectionStatus && connectionStatus !== "connected" ? (
        <p className="app-action-warning">
          Connection is {connectionStatus}.{" "}
          <button
            type="button"
            className="link-button"
            onClick={() =>
              window.dispatchEvent(
                new CustomEvent("alfred:open-settings", {
                  detail: { section: "connected-apps" },
                }),
              )
            }
          >
            Reconnect
          </button>
        </p>
      ) : status?.lastErrorCode ? (
        <p className="app-action-warning">
          Paused: {status.lastErrorCode.split("_").join(" ")}. Next attempt{" "}
          {formatWhen(status.nextAttemptAt)}.
        </p>
      ) : null}
    </div>
  );
}

function FileTriggerFields({
  path,
  pattern,
  onPath,
  onPattern,
}: {
  path: string;
  pattern: string;
  onPath: (value: string) => void;
  onPattern: (value: string) => void;
}) {
  return (
    <>
      <label className="field">
        <span>Folder or file to watch</span>
        <input
          type="text"
          value={path}
          placeholder="/Users/you/code/my-project"
          onChange={(event) => onPath(event.currentTarget.value)}
        />
      </label>
      <label className="field">
        <span>Only these files (optional)</span>
        <input
          type="text"
          value={pattern}
          placeholder="*.ts,*.tsx"
          onChange={(event) => onPattern(event.currentTarget.value)}
        />
      </label>
      <p className="hint">
        <code>.git</code>, <code>node_modules</code>, <code>target</code> and
        other build folders are ignored. Bursts of saves collapse into one run.
      </p>
    </>
  );
}

function ConnectedAppFields({
  config,
  descriptor,
  providers,
  providerEvents,
  connections,
  errors,
  onChange,
}: {
  config: AppTriggerConfig;
  descriptor: AppEventDescriptor | null;
  providers: ReturnType<typeof useIntegrationsStore.getState>["providers"];
  providerEvents: AppEventDescriptor[];
  connections: ReturnType<typeof useIntegrationsStore.getState>["connections"];
  errors: string[];
  onChange: (config: AppTriggerConfig) => void;
}) {
  const updateFilter = (key: string, value: unknown) =>
    onChange({
      ...config,
      filters: { ...config.filters, [key]: value },
    });
  return (
    <>
      <label className="field">
        <span>Provider</span>
        <select
          value={config.providerId}
          onChange={(event) =>
            onChange(selectAppEventProvider(config, event.currentTarget.value))
          }
        >
          <option value="">Choose a provider…</option>
          {providers.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.name}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>Event</span>
        <select
          value={config.eventType}
          disabled={!config.providerId}
          onChange={(event) =>
            onChange(
              selectAppEvent(
                config,
                providerEvents.find(
                  (candidate) =>
                    candidate.eventType === event.currentTarget.value,
                ) ?? null,
              ),
            )
          }
        >
          <option value="">Choose an event…</option>
          {providerEvents.map((event) => (
            <option key={event.eventType} value={event.eventType}>
              {event.label}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>Connection</span>
        <select
          value={config.connectionId}
          disabled={!descriptor}
          onChange={(event) =>
            onChange({ ...config, connectionId: event.currentTarget.value })
          }
        >
          <option value="">Choose a connected account…</option>
          {connections.map((connection) => (
            <option
              key={connection.id}
              value={connection.id}
              disabled={connection.status !== "connected"}
            >
              {connection.displayName ??
                connection.externalAccountId ??
                "Connected account"}
              {connection.status === "connected"
                ? ""
                : ` — ${connection.status}`}
            </option>
          ))}
        </select>
      </label>
      {descriptor ? (
        <>
          <p className="hint">
            {descriptor.description} Runs while Alfred is open.
          </p>
          {descriptor.filterFields.map((field) => (
            <EventFilterField
              key={field.key}
              field={field}
              descriptor={descriptor}
              connectionId={config.connectionId}
              value={config.filters[field.key]}
              onChange={(value) => updateFilter(field.key, value)}
            />
          ))}
        </>
      ) : config.providerId && providerEvents.length === 0 ? (
        <p className="hint">No events are registered for this provider yet.</p>
      ) : null}
      {errors.length > 0 ? (
        <ul className="app-action-validation" aria-label="Trigger configuration issues">
          {errors.map((error) => (
            <li key={error}>{error}</li>
          ))}
        </ul>
      ) : null}
    </>
  );
}

function EventFilterField({
  field,
  descriptor,
  connectionId,
  value,
  onChange,
}: {
  field: ActionFieldDescriptor;
  descriptor: AppEventDescriptor;
  connectionId: string;
  value: unknown;
  onChange: (value: unknown) => void;
}) {
  if (field.kind === "boolean") {
    return (
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={value === true}
          onChange={(event) => onChange(event.currentTarget.checked)}
        />
        <span>{field.label}</span>
      </label>
    );
  }
  if (field.kind === "enum") {
    return (
      <label className="field">
        <span>{field.label}</span>
        <select
          value={typeof value === "string" ? value : ""}
          onChange={(event) => onChange(event.currentTarget.value)}
        >
          <option value="">Choose…</option>
          {field.options.map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
    );
  }
  if (field.kind === "resource_selector") {
    return (
      <EventResourceSelector
        field={field}
        descriptor={descriptor}
        connectionId={connectionId}
        value={typeof value === "string" ? value : ""}
        onChange={onChange}
      />
    );
  }
  return (
    <label className="field">
      <span>{field.label}</span>
      <input
        type="text"
        maxLength={2_048}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
      {field.description ? <small className="hint">{field.description}</small> : null}
    </label>
  );
}

function EventResourceSelector({
  field,
  descriptor,
  connectionId,
  value,
  onChange,
}: {
  field: ActionFieldDescriptor;
  descriptor: AppEventDescriptor;
  connectionId: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<AppEventResourceItem[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const selectedPresent = useMemo(
    () => items.some((item) => item.id === value),
    [items, value],
  );

  useEffect(() => {
    setItems([]);
    setError(null);
  }, [connectionId, descriptor.eventType, field.key]);

  async function search() {
    if (!connectionId) return;
    setBusy(true);
    setError(null);
    const result = await loadEventResourceOptions(integrationsApi, {
        connectionId,
        providerId: descriptor.providerId,
        eventType: descriptor.eventType,
        fieldKey: field.key,
        query,
    });
    setBusy(false);
    if (result.page === null) {
      setError(result.error);
      return;
    }
    setItems(result.page.items);
  }

  return (
    <div className="field app-action-resource-field">
      <span>{field.label}</span>
      <div className="field-row">
        <SearchInput
          aria-label={`Search ${field.label} options`}
          containerClassName="app-action-resource-search"
          value={query}
          maxLength={200}
          disabled={!connectionId || busy}
          placeholder="Search options"
          onChange={(event) => setQuery(event.currentTarget.value)}
          onClear={() => setQuery("")}
        />
        <button
          type="button"
          className="ghost"
          disabled={!connectionId || busy}
          onClick={() => void search()}
        >
          {busy ? "Loading…" : "Search"}
        </button>
      </div>
      <select value={value} onChange={(event) => onChange(event.currentTarget.value)}>
        <option value="">Choose…</option>
        {value && !selectedPresent ? <option value={value}>{value}</option> : null}
        {items.map((item) => (
          <option key={item.id} value={item.id}>
            {item.label}
          </option>
        ))}
      </select>
      {error ? <small className="app-action-warning">{error}</small> : null}
    </div>
  );
}
