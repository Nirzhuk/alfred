import { useEffect, useMemo, useRef, useState } from "react";
import { Modal, ModalBody, ModalHeader } from "../../components/modal";
import { Icon } from "../../components/icon";
import { sharingApi, sharingErrorMessage } from "./api";
import type { Publication, PublicationDraft } from "./types";
import { withPublicExamples } from "./setup-model";
import { PublicationList } from "./publication-list";
import "./sharing.css";

type Props = { workflowId: string; onClose: () => void; onPublished?: (publication: Publication) => void; saveBeforeShare?: () => Promise<void> };
const STEP_LABELS = ["Describe", "Make reusable", "Review and share"];
const REQUIRED_KINDS = ["agentAccount", "appConnection", "appResource", "memory", "folder", "file", "channel"];
const RETAINABLE_KINDS = ["text", "email"];

export function ShareDialog({ workflowId, onClose, onPublished, saveBeforeShare }: Props) {
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState<PublicationDraft | null>(null);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [requestId] = useState(() => crypto.randomUUID());
  const [publishing, setPublishing] = useState(false);
  const [publication, setPublication] = useState<Publication | null>(null);
  const [error, setError] = useState<string | null>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  useEffect(() => { if (error) errorRef.current?.scrollIntoView({ block: "nearest" }); }, [error]);
  const [publicExamples, setPublicExamples] = useState<Record<string, string>>({});

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        await saveBeforeShare?.();
        const value = await sharingApi.preparePublication(workflowId);
        if (!active) return;
        setDraft(value);
        setTitle(value.title);
        setDescription(value.description);
      } catch (reason) {
        if (active) setError(sharingErrorMessage(reason, "Could not prepare this workflow."));
      }
    })();
    return () => { active = false; };
  }, [workflowId, saveBeforeShare]);

  const exampleReview = useMemo(() => {
    if (!draft) return { template: null, error: null };
    try { return { template: withPublicExamples({ ...draft.template, title: title.trim(), description: description.trim() }, publicExamples), error: null }; }
    catch (reason) { return { template: null, error: sharingErrorMessage(reason, "Check the public examples.") }; }
  }, [draft, title, description, publicExamples]);
  const outgoingTemplate = exampleReview.template;
  const nodes = outgoingTemplate?.graph.nodes ?? [];

  const setReusable = (key: string, required: boolean) => setDraft((current) => current ? {
    ...current,
    template: {
      ...current.template,
      parameters: current.template.parameters.map((parameter) => parameter.key === key
        ? { ...parameter, required: REQUIRED_KINDS.includes(parameter.kind) ? true : required }
        : parameter),
    },
  } : current);

  const publish = async () => {
    if (!outgoingTemplate || publishing) return;
    setPublishing(true);
    setError(null);
    try {
      const result = await sharingApi.publishPublication({ workflowId, requestId, template: outgoingTemplate });
      setPublication(result);
      onPublished?.(result);
    } catch (reason) {
      setError(sharingErrorMessage(reason, "Could not create the link."));
    } finally {
      setPublishing(false);
    }
  };

  return <Modal size="xl" onClose={onClose} label="Share workflow" className="sharing-dialog">
    <ModalHeader title="Share workflow" description="Create an unlisted reusable template. Anyone with the link can view its reviewed content." leading={<Icon name="link" size={20} />} actions={<button className="ghost modal-close-button" onClick={onClose} aria-label="Close"><Icon name="x" size={18} /></button>} />
    <div className="sharing-steps" aria-label="Sharing progress">{STEP_LABELS.map((label, index) => <span key={label} className={index === step ? "is-current" : index < step ? "is-done" : ""}>{index + 1}. {label}</span>)}</div>
    <ModalBody className="sharing-body">
      {error || (step === 1 && exampleReview.error) ? <div ref={errorRef} className="sharing-error" role="alert">{error ?? exampleReview.error}</div> : null}
      {publication ? <section className="sharing-success"><h3>Link ready</h3><p>This publication is immutable. Changes to the local workflow create a new link.</p><input readOnly value={publication.url} aria-label="Workflow share link" onFocus={(event) => event.currentTarget.select()} /><div className="sharing-actions"><button onClick={() => void navigator.clipboard.writeText(publication.url)}>Copy link</button><button className="primary" onClick={() => window.open(publication.url, "_blank", "noopener,noreferrer")}>Open preview</button></div><PublicationList workflowId={workflowId} /></section>
        : !draft ? <p className="muted">Preparing a reviewed snapshot…</p>
        : <>
          {step === 0 ? <section><label>Public name<input value={title} maxLength={120} onChange={(event) => setTitle(event.target.value)} autoFocus /></label><label>Description<textarea value={description} maxLength={2000} onChange={(event) => setDescription(event.target.value)} rows={4} /></label><p className="muted">The exact graph and text are read after the current editor save. Your accounts, machine paths, memories, and previous results are removed.</p></section>
            : step === 1 ? <section><h3>Recipient setup</h3><p className="muted">Choose which local values recipients must provide. Required setup stays typed and is never sent back to the publisher.</p>{draft.template.parameters.length === 0 ? <p className="sharing-empty">No local settings need setup.</p> : <div className="sharing-parameter-list">{draft.template.parameters.map((parameter) => {
              const mandatory = REQUIRED_KINDS.includes(parameter.kind);
              const retainable = RETAINABLE_KINDS.includes(parameter.kind);
              const retained = Object.prototype.hasOwnProperty.call(publicExamples, parameter.key);
              return <div className="sharing-parameter" key={parameter.key}>
                <strong>{parameter.label}</strong>
                <label className="sharing-checkbox"><input type="checkbox" checked={!retained && parameter.required} disabled={mandatory || retained} onChange={(event) => setReusable(parameter.key, event.currentTarget.checked)} aria-label={`Ask recipient for ${parameter.label}`} /> Ask recipient</label>
                <span>{parameter.kind} · {retained ? "Public example" : parameter.required ? "Required" : "Optional"}{mandatory ? " · required for execution" : ""}</span>
                {retainable ? <>
                  <label className="sharing-checkbox"><input type="checkbox" checked={retained} aria-label={`Retain a public example for ${parameter.label}`} onChange={(event) => {
                    const checked = event.currentTarget.checked;
                    setPublicExamples((current) => { const next = { ...current }; if (checked) next[parameter.key] = current[parameter.key] ?? ""; else delete next[parameter.key]; return next; });
                  }} /> Retain a public example</label>
                  {retained ? <input value={publicExamples[parameter.key] ?? ""} placeholder="Public example shown in the template" aria-label={`Public example for ${parameter.label}`} onChange={(event) => {
                    const value = event.currentTarget.value;
                    setPublicExamples((current) => ({ ...current, [parameter.key]: value }));
                  }} /> : null}
                </> : null}
              </div>;
            })}</div>}</section>
            : <section><h3>Review this publication</h3><p className="muted">Review the exact outgoing title, description, executable text, requirements, destinations, starts, and removed private values before creating the link.</p><div className="sharing-preview"><strong>{title || "Untitled workflow"}</strong><p>{description || "No description"}</p><ol>{nodes.map((node) => <li key={node.id}>{String(node.data.label ?? node.type)}</li>)}</ol><p>{outgoingTemplate?.graph.edges.length ?? 0} graph connections · {outgoingTemplate?.requirements.length ?? 0} requirements · {outgoingTemplate?.starts.length ?? 0} disabled start recipes</p><pre>{JSON.stringify(outgoingTemplate, null, 2)}</pre></div>{draft.removed.length > 0 ? <details><summary>Removed or replaced values ({draft.removed.length})</summary><ul>{draft.removed.map((item) => <li key={item}>{item}</li>)}</ul></details> : null}</section>}
        </>}
    </ModalBody>
    <footer className="sharing-footer"><button className="ghost" onClick={onClose}>Cancel</button>{!publication ? <>{step > 0 ? <button onClick={() => setStep(step - 1)}>Back</button> : null}{step < 2 ? <button className="primary" disabled={!draft || !title.trim() || (step === 1 && !!exampleReview.error)} onClick={() => setStep(step + 1)}>Continue</button> : <button className="primary" disabled={publishing || !outgoingTemplate} onClick={() => void publish()}>{publishing ? "Creating link…" : "Create link"}</button>}</> : null}</footer>
  </Modal>;
}
