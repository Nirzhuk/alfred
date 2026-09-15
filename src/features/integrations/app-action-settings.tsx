import { useEffect, useMemo, useState } from "react";
import { SearchInput } from "../../components/search-input";
import type { AppActionNodeData } from "../workflow/types";
import { integrationsApi } from "./api";
import {
  compatibleConnections,
  loadActionResourceOptions,
  selectAppAction,
  selectAppActionProvider,
  unknownActionInputKeys,
  validateAppActionForm,
} from "./app-action-form";
import { useIntegrationsStore } from "./store";
import type {
  ActionDescriptor,
  ActionFieldDescriptor,
  ActionResourceItem,
} from "./types";

type Props = {
  data: AppActionNodeData;
  onUpdate: (patch: Partial<AppActionNodeData>) => void;
};

export function AppActionSettings({ data, onUpdate }: Props) {
  const providers = useIntegrationsStore((state) => state.providers);
  const connections = useIntegrationsStore((state) => state.connections);
  const descriptors = useIntegrationsStore((state) => state.descriptors);
  const loading = useIntegrationsStore((state) => state.loading);
  const load = useIntegrationsStore((state) => state.load);

  useEffect(() => {
    void load();
  }, [load]);

  const providerActions = descriptors.filter(
    (descriptor) => descriptor.providerId === data.providerId,
  );
  const descriptor =
    descriptors.find(
      (candidate) =>
        candidate.providerId === data.providerId &&
        candidate.actionId === data.actionId,
    ) ?? null;
  const availableConnections = compatibleConnections(
    connections,
    data.providerId,
  );
  const unknownKeys = descriptor
    ? unknownActionInputKeys(data, descriptor)
    : [];
  const errors = validateAppActionForm(data, descriptor);

  const replaceData = (next: AppActionNodeData) => {
    onUpdate({
      label: next.label,
      providerId: next.providerId,
      actionId: next.actionId,
      connectionId: next.connectionId,
      input: next.input,
    });
  };
  const updateInput = (key: string, value: unknown) => {
    onUpdate({ input: { ...data.input, [key]: value } });
  };

  return (
    <>
      <label className="field">
        <span>Provider</span>
        <select
          value={data.providerId}
          disabled={loading}
          onChange={(event) => {
            const provider = providers.find(
              (candidate) => candidate.id === event.currentTarget.value,
            );
            replaceData({
              ...selectAppActionProvider(
                data,
                event.currentTarget.value,
                connections,
              ),
              label: provider?.name ?? "",
            });
          }}
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
        <span>Action</span>
        <select
          value={data.actionId}
          disabled={!data.providerId || loading}
          onChange={(event) => {
            const selected =
              providerActions.find(
                (candidate) => candidate.actionId === event.currentTarget.value,
              ) ?? null;
            replaceData(selectAppAction(data, selected, connections));
          }}
        >
          <option value="">Choose an action…</option>
          {providerActions.map((action) => (
            <option key={action.actionId} value={action.actionId}>
              {action.label}
            </option>
          ))}
          {data.actionId && !descriptor ? (
            <option value={data.actionId}>{data.actionId} (unavailable)</option>
          ) : null}
        </select>
      </label>

      <label className="field">
        <span>Connection</span>
        <select
          value={data.connectionId}
          disabled={!descriptor || loading}
          onChange={(event) =>
            onUpdate({ connectionId: event.currentTarget.value })
          }
        >
          <option value="">Choose a connected account…</option>
          {availableConnections.map((connection) => (
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

      <label className="field">
        <span>Label</span>
        <input
          type="text"
          value={data.label}
          onChange={(event) => onUpdate({ label: event.currentTarget.value })}
        />
      </label>

      {descriptor ? (
        <>
          <p className="hint">{descriptor.description}</p>
          {descriptor.fields.map((field) => (
            <DescriptorField
              key={field.key}
              field={field}
              descriptor={descriptor}
              connectionId={data.connectionId}
              value={data.input[field.key]}
              displaySnapshot={data.input[`${field.key}__display`]}
              onChange={(value) => updateInput(field.key, value)}
              onSelectResource={(value, label) =>
                onUpdate({
                  input: {
                    ...data.input,
                    [field.key]: value,
                    [`${field.key}__display`]: label,
                  },
                })
              }
            />
          ))}
        </>
      ) : data.actionId ? (
        <p className="app-action-warning" role="status">
          This workflow uses an action descriptor that this Alfred version does
          not know. Its saved inputs are preserved unchanged, but it cannot run
          until the matching provider action is installed.
        </p>
      ) : providerActions.length === 0 && data.providerId ? (
        <p className="hint">No actions are registered for this provider yet.</p>
      ) : null}

      {unknownKeys.length > 0 ? (
        <p className="app-action-warning" role="status">
          Newer action fields are preserved but cannot be edited here: {unknownKeys.join(", ")}.
        </p>
      ) : null}
      {errors.length > 0 ? (
        <ul className="app-action-validation" aria-label="Action configuration issues">
          {errors.map((error) => (
            <li key={error}>{error}</li>
          ))}
        </ul>
      ) : null}
    </>
  );
}

function DescriptorField({
  field,
  descriptor,
  connectionId,
  value,
  displaySnapshot,
  onChange,
  onSelectResource,
}: {
  field: ActionFieldDescriptor;
  descriptor: ActionDescriptor;
  connectionId: string;
  value: unknown;
  displaySnapshot: unknown;
  onChange: (value: unknown) => void;
  onSelectResource: (value: string, label: string) => void;
}) {
  const description = [
    field.description,
    field.supportsInterpolation
      ? "Supports {{context}}, {{output}}, and {{cwd}}."
      : "",
  ]
    .filter(Boolean)
    .join(" ");

  if (field.kind === "boolean") {
    return (
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={value === true}
          onChange={(event) => onChange(event.currentTarget.checked)}
        />
        <span>{field.label}</span>
        {description ? <small className="hint">{description}</small> : null}
      </label>
    );
  }

  if (field.kind === "enum") {
    return (
      <label className="field">
        <span>{field.label}</span>
        <select
          value={typeof value === "string" ? value : ""}
          required={field.required}
          onChange={(event) => onChange(event.currentTarget.value)}
        >
          <option value="">Choose…</option>
          {field.options.map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
        {description ? <small className="hint">{description}</small> : null}
      </label>
    );
  }

  if (field.kind === "resource_selector") {
    return (
      <ResourceSelector
        field={field}
        descriptor={descriptor}
        connectionId={connectionId}
        value={typeof value === "string" ? value : ""}
        displaySnapshot={
          typeof displaySnapshot === "string" ? displaySnapshot : ""
        }
        onSelect={onSelectResource}
      />
    );
  }

  return (
    <label className="field">
      <span>{field.label}</span>
      {field.kind === "textarea" ? (
        <textarea
          rows={5}
          value={typeof value === "string" ? value : ""}
          required={field.required}
          onChange={(event) => onChange(event.currentTarget.value)}
        />
      ) : (
        <input
          type="text"
          value={typeof value === "string" ? value : ""}
          required={field.required}
          onChange={(event) => onChange(event.currentTarget.value)}
        />
      )}
      {description ? <small className="hint">{description}</small> : null}
    </label>
  );
}

function ResourceSelector({
  field,
  descriptor,
  connectionId,
  value,
  displaySnapshot,
  onSelect,
}: {
  field: ActionFieldDescriptor;
  descriptor: ActionDescriptor;
  connectionId: string;
  value: string;
  displaySnapshot: string;
  onSelect: (value: string, label: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<ActionResourceItem[]>([]);
  const [nextPageToken, setNextPageToken] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const selectedPresent = useMemo(
    () => items.some((item) => item.id === value),
    [items, value],
  );

  const fetchOptions = async (append: boolean) => {
    if (!connectionId) return;
    setBusy(true);
    setError(null);
    const result = await loadActionResourceOptions(integrationsApi, {
      connectionId,
      providerId: descriptor.providerId,
      actionId: descriptor.actionId,
      fieldKey: field.key,
      query,
      pageToken: append ? nextPageToken : null,
    });
    setBusy(false);
    if (result.page === null) {
      setError(result.error);
      return;
    }
    setItems((current) =>
      append ? dedupeResources([...current, ...result.page.items]) : result.page.items,
    );
    setNextPageToken(result.page.nextPageToken);
  };

  useEffect(() => {
    setItems([]);
    setNextPageToken(null);
    setError(null);
  }, [connectionId, descriptor.actionId, field.key]);

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
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void fetchOptions(false);
            }
          }}
        />
        <button
          type="button"
          className="ghost"
          disabled={!connectionId || busy}
          onClick={() => void fetchOptions(false)}
        >
          {busy ? "Loading…" : "Search"}
        </button>
      </div>
      <select
        value={value}
        disabled={!connectionId}
        required={field.required}
        onChange={(event) => {
          const nextId = event.currentTarget.value;
          const label = items.find((item) => item.id === nextId)?.label ?? "";
          onSelect(nextId, label);
        }}
      >
        <option value="">Choose…</option>
        {value && !selectedPresent ? (
          <option value={value}>{displaySnapshot || value}</option>
        ) : null}
        {items.map((item) => (
          <option key={item.id} value={item.id}>
            {item.label}
          </option>
        ))}
      </select>
      {nextPageToken ? (
        <button
          type="button"
          className="ghost app-action-load-more"
          disabled={busy}
          onClick={() => void fetchOptions(true)}
        >
          Load more
        </button>
      ) : null}
      {field.description ? <small className="hint">{field.description}</small> : null}
      {error ? <small className="app-action-field-error">{error}</small> : null}
    </div>
  );
}

function dedupeResources(items: ActionResourceItem[]): ActionResourceItem[] {
  return Array.from(new Map(items.map((item) => [item.id, item])).values());
}
