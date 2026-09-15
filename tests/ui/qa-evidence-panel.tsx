import { useState } from "react";
import { QaModalPortal } from "./qa-modal-portal";

/** Optional fixture diagnostics for computer-use tools without JS evaluation. */
export function QaEvidencePanel({ snapshot }: { snapshot: () => unknown }) {
  const [report, setReport] = useState<string | null>(null);
  if (new URLSearchParams(location.search).get("diagnostics") !== "1") return null;
  return <QaModalPortal><aside aria-label="Synthetic QA evidence" style={{
    position: "fixed", left: 8, bottom: 8, zIndex: 10000,
    maxWidth: "min(700px, 90vw)", color: "#171717", background: "#fff",
    border: "1px solid #aaa", borderRadius: 8, padding: 8,
  }}>
    <button type="button" onClick={() => setReport(report === null ? JSON.stringify(snapshot(), null, 2) : null)}>
      {report === null ? "Show QA evidence" : "Hide QA evidence"}
    </button>
    {report !== null && <pre aria-label="QA command and state snapshot" style={{
      maxHeight: "45vh", overflow: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere",
    }}>{report}</pre>}
  </aside></QaModalPortal>;
}
