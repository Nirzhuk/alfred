import type { ReactNode } from "react";
import "./output-receipts.css";
import {
  outputReceiptDescription,
  outputReceiptKey,
  outputReceiptStateLabel,
  retryOutputPersistenceRequest,
  type OutputReceipt,
  type RetryOutputPersistenceRequest,
} from "./output-receipt";

type Props = {
  receipts: readonly OutputReceipt[];
  retryingReceiptKeys?: ReadonlySet<string>;
  onRetry?: (request: RetryOutputPersistenceRequest) => void | Promise<void>;
  onOpenMemory?: (memoryId: string) => void;
};

const NO_RETRYING_RECEIPTS = new Set<string>();

export function OutputReceipts({
  receipts,
  retryingReceiptKeys = NO_RETRYING_RECEIPTS,
  onRetry,
  onOpenMemory,
}: Props) {
  if (receipts.length === 0) return null;

  return (
    <section
      className="history-memory-context output-receipts"
      aria-labelledby="history-output-receipts-title"
    >
      <div className="history-section-heading">
        <h3 id="history-output-receipts-title">Output saves</h3>
        <span>{receipts.length}</span>
      </div>
      <ul className="output-receipt-list">
        {receipts.map((receipt) => {
          const key = outputReceiptKey(receipt);
          const retryRequest = retryOutputPersistenceRequest(receipt);
          const retrying = retryingReceiptKeys.has(key);
          return (
            <li className="output-receipt-row" data-state={receipt.state} key={key}>
              <div className="output-receipt-body">
                <div className="output-receipt-heading">
                  <ReceiptTitle receipt={receipt} onOpenMemory={onOpenMemory} />
                  <span className={statusClassName(receipt.state)}>
                    {outputReceiptStateLabel(receipt.state)}
                  </span>
                </div>
                <p className="output-receipt-copy">
                  {outputReceiptDescription(receipt)}
                </p>
                <p className="output-receipt-meta">
                  {receipt.artifactRequired ? "Report artifact" : "Output"}
                  {pinMetadata(receipt)}
                </p>
              </div>
              {retryRequest && onRetry ? (
                <button
                  type="button"
                  className="ghost output-receipt-retry"
                  aria-label={`Retry saving ${receipt.resolvedMemoryTitle}`}
                  disabled={retrying}
                  onClick={() => void onRetry(retryRequest)}
                >
                  {retrying ? "Retrying…" : "Retry save"}
                </button>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function ReceiptTitle({
  receipt,
  onOpenMemory,
}: {
  receipt: OutputReceipt;
  onOpenMemory?: (memoryId: string) => void;
}): ReactNode {
  const memoryId = receipt.memoryId;
  if (memoryId && onOpenMemory) {
    return (
      <button
        type="button"
        className="history-memory-link output-receipt-title"
        aria-label={`Open ${receipt.resolvedMemoryTitle} in Memories`}
        onClick={() => onOpenMemory(memoryId)}
      >
        {receipt.resolvedMemoryTitle}
      </button>
    );
  }
  return <strong className="output-receipt-title">{receipt.resolvedMemoryTitle}</strong>;
}

function statusClassName(state: OutputReceipt["state"]): string {
  if (state === "saved") return "history-status is-completed";
  if (state === "failed") return "history-status is-failed";
  if (state === "pending" || state === "saving") {
    return "history-status is-output-saving";
  }
  return "history-status";
}

function pinMetadata(receipt: OutputReceipt): string {
  if (!receipt.disposition.pinMemory) return "";
  if (receipt.state === "saved") {
    return " · Pinned when saved";
  }
  if (
    receipt.state === "pending" ||
    receipt.state === "saving" ||
    receipt.state === "failed" ||
    receipt.state === "deleted"
  ) {
    return " · Pin requested";
  }
  return "";
}
