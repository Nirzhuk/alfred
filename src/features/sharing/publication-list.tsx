import { useEffect, useState } from "react";
import { Icon } from "../../components/icon";
import { sharingApi, sharingErrorMessage } from "./api";
import type { PendingPublication, Publication } from "./types";
import "./sharing.css";

type Props = { workflowId?: string };

/** Small management surface for immutable links owned by this installation. */
export function PublicationList({ workflowId }: Props) {
  const [items, setItems] = useState<Publication[]>([]);
  const [pending, setPending] = useState<PendingPublication[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const reload = async () => { try { setError(null); const [active, waiting] = await Promise.all([sharingApi.listPublications(workflowId), sharingApi.listPendingPublications()]); setItems(active); setPending(workflowId ? waiting.filter((item) => item.workflowId === workflowId) : waiting); } catch (reason) { setError(sharingErrorMessage(reason, "Could not load publications.")); } };
  useEffect(() => { void reload(); }, [workflowId]);
  const revoke = async (shareId: string) => { if (busy) return; setBusy(shareId); try { await sharingApi.revokePublication(shareId); setItems((current) => current.filter((item) => item.shareId !== shareId)); } catch (reason) { setError(sharingErrorMessage(reason, "Could not revoke this link.")); } finally { setBusy(null); } };
  const resume = async (requestId: string) => { if (busy) return; setBusy(requestId); try { await sharingApi.resumePublication(requestId); await reload(); } catch (reason) { setError(sharingErrorMessage(reason, "Could not resume this publication.")); } finally { setBusy(null); } };
  return <section className="sharing-publications" aria-label="Workflow publications"><div className="sharing-publications-header"><div><h3>Published links</h3><p className="muted">Revoke a link to make its snapshot unavailable.</p></div><button className="ghost" onClick={() => void reload()} aria-label="Refresh published links"><Icon name="arrow-clockwise" size={16} /></button></div>{error ? <div className="sharing-error" role="alert">{error}</div> : null}{pending.length > 0 ? <><h4>Waiting to finish</h4><ul>{pending.map((item) => <li key={item.requestId}><span className="sharing-pending-label">Publication from {new Date(item.createdAt).toLocaleString()}</span><button className="ghost" disabled={busy === item.requestId} onClick={() => void resume(item.requestId)}>{busy === item.requestId ? "Resuming…" : "Resume"}</button></li>)}</ul></> : null}{items.length === 0 ? <p className="sharing-empty">No active links.</p> : <ul>{items.map((item) => <li key={item.shareId}><a href={item.url} target="_blank" rel="noreferrer">{item.url}</a><button className="ghost" disabled={busy === item.shareId} onClick={() => void revoke(item.shareId)}>{busy === item.shareId ? "Revoking…" : "Revoke"}</button></li>)}</ul>}</section>;
}
