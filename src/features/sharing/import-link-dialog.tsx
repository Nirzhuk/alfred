import { useEffect, useRef, useState } from "react";
import { Modal, ModalBody, ModalHeader } from "../../components/modal";
import { Icon } from "../../components/icon";
import { open } from "@tauri-apps/plugin-dialog";
import { sharingApi, sharingErrorMessage } from "./api";
import { integrationsApi } from "../integrations/api";
import { useWorkflowStore } from "../workflow/store";
import type { SharedTemplate, SharingParameter } from "./types";
import { agentAccountOptions, dependentResources, loadResourceOptions, resourceRequests, ResourceLoader, type LocalOption, type ResourceState } from "./setup-model";
import "./sharing.css";

type Props = { onClose: () => void; onImported?: (workflowId: string) => void; initialLink?: string };

function durableAttemptId(link: string): string {
  let identity = link.trim();
  try {
    const parsed = new URL(identity);
    const segments = parsed.pathname.split("/").filter(Boolean);
    if ((parsed.protocol === "https:" || parsed.protocol === "http:") && segments.length === 2 && segments[0] === "s") identity = `share:${segments[1]}`;
    else if (parsed.protocol === "alfred-workflows:" && parsed.hostname === "import" && segments.length === 1) identity = `share:${segments[0]}`;
  } catch {
    // Keep malformed values isolated; backend validation still decides whether
    // they can be imported.
  }
  const key = `alfred.share.import.${encodeURIComponent(identity)}`;
  try {
    const stored = window.localStorage.getItem(key);
    if (stored) return stored;
    const created = crypto.randomUUID();
    window.localStorage.setItem(key, created);
    return created;
  } catch {
    return crypto.randomUUID();
  }
}

function ParameterControl({ parameter, options, value, resourceState, onChange, onPick, onReload }: {
  parameter: SharingParameter;
  options: LocalOption[];
  value: unknown;
  resourceState?: ResourceState;
  onChange: (value: unknown) => void;
  onPick: () => void;
  onReload: () => void;
}) {
  if (parameter.kind === "memory") return <label>{parameter.label}<select multiple required={parameter.required} aria-label={parameter.label} value={Array.isArray(value) ? value.map(String) : []} onChange={(event) => onChange(Array.from(event.currentTarget.selectedOptions, (option) => option.value))}>{options.length > 0 ? options.map((option) => <option key={option.id} value={option.id}>{option.label}</option>) : <option value="" disabled>No selectable memories available</option>}</select></label>;
  if (["agentAccount", "appConnection", "channel", "appResource"].includes(parameter.kind)) {
    const loading = resourceState?.loading;
    const placeholder = loading ? "Loading resources…" : options.length ? "Choose a compatible local value" : parameter.kind === "appResource" ? "Choose a connection, then load resources" : "No compatible local values available";
    return <div><label>{parameter.label}<select required={parameter.required} disabled={loading || options.length === 0} aria-label={parameter.label} value={typeof value === "string" ? value : ""} onChange={(event) => onChange(event.currentTarget.value)}><option value="">{placeholder}</option>{options.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}</select></label>{resourceState?.error ? <p role="alert">{resourceState.error}</p> : null}{parameter.kind === "appResource" ? <button type="button" className="ghost" disabled={loading} onClick={onReload}>Reload {parameter.label}</button> : null}</div>;
  }
  if (parameter.kind === "folder" || parameter.kind === "file") return <label>{parameter.label}<span className="sharing-path-control"><input type="text" required={parameter.required} placeholder={parameter.kind === "folder" ? "Choose a local folder" : "Choose a local file"} aria-label={parameter.label} value={typeof value === "string" ? value : ""} onChange={(event) => onChange(event.currentTarget.value)} /><button type="button" className="ghost" onClick={onPick}>Browse</button></span></label>;
  return <label>{parameter.label}<input type={parameter.kind === "email" ? "email" : "text"} required={parameter.required} aria-label={parameter.label} value={typeof value === "string" ? value : ""} onChange={(event) => onChange(event.currentTarget.value)} /></label>;
}

export function ImportLinkDialog({ onClose, onImported, initialLink }: Props) {
  const [link, setLink] = useState(initialLink ?? "");
  const [shared, setShared] = useState<SharedTemplate | null>(null);
  const [values, setValues] = useState<Record<string, unknown>>({});
  const valuesRef = useRef(values);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  useEffect(() => { if (error) errorRef.current?.scrollIntoView({ block: "nearest" }); }, [error]);
  const [existingCopy, setExistingCopy] = useState<string | null>(null);
  const [importAttemptId, setImportAttemptId] = useState(() => durableAttemptId(initialLink ?? "manual"));
  const [accounts, setAccounts] = useState<LocalOption[]>([]);
  const [connections, setConnections] = useState<LocalOption[]>([]);
  const [resources, setResources] = useState<Record<string, ResourceState>>({});
  const [resourceLoader] = useState(() => new ResourceLoader());
  const resolveGeneration = useRef(0);
  const memories = useWorkflowStore((state) => state.memories);
  const optionsFor = (parameter: SharingParameter) => parameter.kind === "agentAccount"
    ? accounts.filter((option) => !parameter.allowedProvider || option.providerId === parameter.allowedProvider)
    : parameter.kind === "appConnection"
      ? connections.filter((option) => !parameter.allowedProvider || option.providerId === parameter.allowedProvider)
      : parameter.kind === "memory" ? memories.map((memory) => ({ id: memory.id, label: memory.title }))
        : resources[parameter.key]?.options ?? [];

  const loadResources = (parameter: SharingParameter, currentValues = valuesRef.current) => {
    if (!shared) return;
    void resourceLoader.load(parameter.key, async () => {
      const requests = resourceRequests(shared.template, parameter, currentValues);
      if (!requests) throw new Error("Choose a connection for every use of this resource first.");
      const options = await loadResourceOptions(requests, (request, pageToken) => request.kind === "action"
        ? integrationsApi.listActionResources({ connectionId: request.connectionId, providerId: request.providerId, actionId: request.capability, fieldKey: request.fieldKey, query: "", pageToken })
        : integrationsApi.listEventResources({ connectionId: request.connectionId, providerId: request.providerId, eventType: request.capability, fieldKey: request.fieldKey, query: "", pageToken }));
      if (!options.length) throw new Error("No compatible resources are available for every use of this field.");
      return options;
    }, (state) => setResources((current) => ({ ...current, [parameter.key]: state })));
  };

  const resolve = async (value = link) => {
    const candidate = value.trim();
    if (!candidate) return;
    const generation = ++resolveGeneration.current;
    resourceLoader.reset();
    valuesRef.current = {};
    setValues({}); setResources({}); setShared(null); setAccounts([]); setConnections([]);
    setBusy(true); setError(null); setExistingCopy(null); setLink(candidate);
    setImportAttemptId(durableAttemptId(candidate));
    try {
      const resolved = await sharingApi.resolveLink(candidate);
      if (generation !== resolveGeneration.current) return;
      setShared(resolved);
      const [agentResult, connectionResult] = await Promise.allSettled([sharingApi.listAgentAccounts(), sharingApi.listAppConnections()]);
      if (generation !== resolveGeneration.current) return;
      if (agentResult.status === "fulfilled") setAccounts(agentAccountOptions(agentResult.value));
      if (connectionResult.status === "fulfilled") setConnections(connectionResult.value.filter((connection) => connection.status === "connected").map((connection) => ({ id: connection.id, label: `${connection.displayName ?? connection.providerId} (${connection.providerId})`, providerId: connection.providerId })));
      if (agentResult.status === "rejected" || connectionResult.status === "rejected") setError("Some local accounts could not be loaded. Reopen setup to try again.");
    } catch (reason) {
      if (generation === resolveGeneration.current) setError(sharingErrorMessage(reason, "Could not load this template."));
    } finally {
      if (generation === resolveGeneration.current) setBusy(false);
    }
  };
  useEffect(() => {
    if (initialLink?.trim()) void resolve(initialLink);
    return () => { resolveGeneration.current += 1; resourceLoader.reset(); };
  }, [initialLink]);

  const changeValue = (parameter: SharingParameter, value: unknown) => {
    const dependents = parameter.kind === "appConnection" ? dependentResources(shared?.template.parameters ?? [], parameter) : [];
    const next = { ...valuesRef.current, [parameter.key]: value };
    for (const dependent of dependents) {
      delete next[dependent.key];
      resourceLoader.invalidate(dependent.key);
    }
    valuesRef.current = next;
    setValues(next);
    for (const dependent of dependents) loadResources(dependent, next);
  };
  const browse = async (parameter: SharingParameter) => { const picked = await open({ directory: parameter.kind === "folder", multiple: false, title: parameter.kind === "folder" ? "Choose a local folder" : "Choose file" }); const value = typeof picked === "string" ? picked : ""; if (value) changeValue(parameter, value); };
  const create = async () => { if (!shared || busy) return; setBusy(true); setError(null); try { const check = await sharingApi.checkSetup({ template: shared.template, values }); if (!check.complete) { setError(check.errors.join(" ")); return; } const result = await sharingApi.importTemplate({ template: shared.template, values, sourceLink: link.trim(), digest: shared.digest, importAttemptId }); onImported?.(result.workflowId); if (result.existingCopy) setExistingCopy(result.workflowId); else onClose(); } catch (reason) { setError(sharingErrorMessage(reason, "Could not create the workflow copy.")); } finally { setBusy(false); } };
  return <Modal size="lg" onClose={onClose} label="Import workflow from link" className="sharing-dialog"><ModalHeader title="Import from link" description="Review a shared template and create an independent local copy." leading={<Icon name="arrow-square-out" size={20} />} actions={<button className="ghost modal-close-button" onClick={onClose} aria-label="Close"><Icon name="x" size={18} /></button>} /><ModalBody className="sharing-body">{error ? <div ref={errorRef} className="sharing-error" role="alert">{error}</div> : null}{existingCopy ? <div className="sharing-notice" role="status">This link is already imported as a local copy. Alfred kept that existing workflow and did not create another one.</div> : null}{!shared ? <section><label>Alfred share link<input value={link} onChange={(event) => setLink(event.target.value)} placeholder="https://…/s/…" autoFocus /></label><p className="muted">Paste the canonical HTTPS link when your browser cannot open Alfred directly.</p><button className="primary" disabled={!link.trim() || busy} onClick={() => void resolve()}>{busy ? "Loading…" : "Preview template"}</button></section> : <section><h3>{shared.template.title}</h3><p>{shared.template.description}</p><div className="sharing-preview"><strong>Steps and requirements</strong><ol>{shared.template.graph.nodes.map((node) => <li key={node.id}>{String(node.data.label ?? node.type)}</li>)}</ol>{shared.template.graph.edges.length > 0 ? <p className="muted">{shared.template.graph.edges.length} connections preserved</p> : null}<ul>{shared.template.requirements.map((requirement) => <li key={`${requirement.kind}:${requirement.name}`}>{requirement.kind}: {requirement.name}</li>)}</ul>{shared.template.starts.length > 0 ? <p className="muted">{shared.template.starts.length} start settings will be imported disabled.</p> : null}</div><details className="sharing-preview"><summary>Review executable content and destinations</summary><pre>{JSON.stringify(shared.template, null, 2)}</pre></details><h3>Set up this copy</h3>{shared.template.parameters.length === 0 ? <p className="muted">No local settings are required.</p> : shared.template.parameters.map((parameter) => <ParameterControl key={parameter.key} parameter={parameter} options={optionsFor(parameter)} resourceState={resources[parameter.key]} onReload={() => loadResources(parameter)} value={values[parameter.key]} onPick={() => void browse(parameter)} onChange={(value) => changeValue(parameter, value)} />)}<p className="muted">Import creates disabled starts and never runs actions, installs skills, or contacts destinations.</p></section>}</ModalBody><footer className="sharing-footer"><button className="ghost" onClick={onClose}>Cancel</button>{shared ? <button className="primary" disabled={busy || Object.values(resources).some((state) => state.loading)} onClick={() => existingCopy ? onClose() : void create()}>{busy ? "Creating…" : existingCopy ? "Close" : "Create workflow copy"}</button> : null}</footer></Modal>;
}
