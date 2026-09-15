import type {
  OutputPersistenceErrorCode,
  OutputReceipt,
  OutputReceiptState,
  RetryOutputPersistenceRequest,
  RunHistoryDetail,
} from "../../types";

export type {
  OutputDisposition,
  OutputPersistenceErrorCode,
  OutputReceipt,
  OutputReceiptState,
  RetryOutputPersistenceRequest,
} from "../../types";

const RETRYABLE_ERRORS = new Set<OutputPersistenceErrorCode>([
  "artifact_stage_io",
  "artifact_finalize_io",
  "database_write",
]);

export function outputReceiptKey(
  receipt: Pick<OutputReceipt, "runId" | "nodeId">,
): string {
  return JSON.stringify([receipt.runId, receipt.nodeId]);
}

export function retryOutputPersistenceRequest(
  receipt: OutputReceipt,
): RetryOutputPersistenceRequest | null {
  if (
    receipt.state !== "failed" ||
    receipt.errorCode === null ||
    !RETRYABLE_ERRORS.has(receipt.errorCode)
  ) {
    return null;
  }
  return { runId: receipt.runId, nodeId: receipt.nodeId };
}

export function mergeOutputReceipt(
  receipts: readonly OutputReceipt[],
  incoming: OutputReceipt,
): OutputReceipt[] {
  let replaced = false;
  const merged = receipts.map((receipt) => {
    if (
      receipt.runId !== incoming.runId ||
      receipt.nodeId !== incoming.nodeId
    ) {
      return receipt;
    }
    replaced = true;
    if (receipt.state === "deleted" && incoming.state !== "deleted") {
      return receipt;
    }
    return incoming;
  });
  return replaced ? merged : [...merged, incoming];
}

export function mergeOutputReceiptIntoHistory(
  detail: RunHistoryDetail | null,
  incoming: OutputReceipt,
): RunHistoryDetail | null {
  if (!detail || detail.run.id !== incoming.runId) return detail;
  return {
    ...detail,
    outputReceipts: mergeOutputReceipt(detail.outputReceipts ?? [], incoming),
  };
}

export function mergeRefreshedOutputHistory(
  current: RunHistoryDetail | null,
  refreshed: RunHistoryDetail,
): RunHistoryDetail {
  if (!current || current.run.id !== refreshed.run.id) return refreshed;
  const currentByKey = new Map(
    (current.outputReceipts ?? []).map((receipt) => [
      outputReceiptKey(receipt),
      receipt,
    ]),
  );
  const merged = (refreshed.outputReceipts ?? []).map((receipt) => {
    const existing = currentByKey.get(outputReceiptKey(receipt));
    currentByKey.delete(outputReceiptKey(receipt));
    if (!existing) return receipt;
    if (existing.state === "deleted") return existing;
    if (receipt.state === "deleted") return receipt;
    return isTerminalOutputReceipt(existing) ? existing : receipt;
  });
  merged.push(...currentByKey.values());
  return { ...refreshed, outputReceipts: merged };
}

function isTerminalOutputReceipt(receipt: OutputReceipt): boolean {
  return (
    receipt.state === "not_requested" ||
    receipt.state === "skipped_empty" ||
    receipt.state === "saved" ||
    receipt.state === "deleted"
  );
}

export type OutputReceiptRetryResult = {
  receipt: OutputReceipt;
  refreshedDetail: RunHistoryDetail | null;
  refreshFailed: boolean;
  saved: boolean;
};

export async function retryOutputReceiptAndRefresh(
  request: RetryOutputPersistenceRequest,
  operations: {
    retry: (request: RetryOutputPersistenceRequest) => Promise<OutputReceipt>;
    onReceipt: (receipt: OutputReceipt) => void;
    refresh: (runId: string) => Promise<RunHistoryDetail | null>;
  },
): Promise<OutputReceiptRetryResult> {
  const receipt = await operations.retry(request);
  if (receipt.runId !== request.runId || receipt.nodeId !== request.nodeId) {
    throw new Error("output_receipt_identity_mismatch");
  }
  operations.onReceipt(receipt);
  if (receipt.state !== "saved" || receipt.memoryId === null) {
    return {
      receipt,
      refreshedDetail: null,
      refreshFailed: false,
      saved: false,
    };
  }
  try {
    const refreshedDetail = await operations.refresh(request.runId);
    const trustedDetail =
      refreshedDetail?.run.id === request.runId ? refreshedDetail : null;
    return {
      receipt,
      refreshedDetail: trustedDetail
        ? retainSuccessfulRetry(trustedDetail, receipt)
        : null,
      refreshFailed: trustedDetail === null,
      saved: true,
    };
  } catch {
    return {
      receipt,
      refreshedDetail: null,
      refreshFailed: true,
      saved: true,
    };
  }
}

function retainSuccessfulRetry(
  refreshedDetail: RunHistoryDetail,
  saved: OutputReceipt,
): RunHistoryDetail {
  const current = (refreshedDetail.outputReceipts ?? []).find(
    (receipt) =>
      receipt.runId === saved.runId && receipt.nodeId === saved.nodeId,
  );
  if (current?.state === "deleted") return refreshedDetail;
  return mergeOutputReceiptIntoHistory(refreshedDetail, saved) ?? refreshedDetail;
}

export function outputReceiptStateLabel(state: OutputReceiptState): string {
  switch (state) {
    case "not_requested":
      return "Not saved";
    case "skipped_empty":
      return "Nothing to save";
    case "pending":
      return "Waiting to save";
    case "saving":
      return "Saving";
    case "saved":
      return "Saved to Memories";
    case "failed":
      return "Save failed";
    case "deleted":
      return "Removed from Memories";
  }
}

export function outputReceiptDescription(receipt: OutputReceipt): string {
  switch (receipt.state) {
    case "not_requested":
      return "Saving to Memories was off when this output ran.";
    case "skipped_empty":
      return "The completed output contained no text to save.";
    case "pending":
      return "This output is queued to be saved by Alfred.";
    case "saving":
      return "Alfred is saving this output.";
    case "saved":
      return receipt.artifactRequired
        ? "The complete report is stored in Memories."
        : "The output is stored in Memories.";
    case "deleted":
      return "This output was deliberately removed and will not be recreated.";
    case "failed":
      return outputPersistenceErrorDescription(receipt.errorCode);
  }
}

export function outputPersistenceErrorDescription(
  errorCode: OutputPersistenceErrorCode | null,
): string {
  switch (errorCode) {
    case "invalid_disposition":
      return "The captured save settings are invalid.";
    case "step_output_unavailable":
      return "The completed output is unavailable for storage.";
    case "artifact_stage_io":
      return "Alfred could not prepare the report file.";
    case "artifact_finalize_io":
      return "Alfred could not finish the report file.";
    case "artifact_content_mismatch":
      return "The stored report file no longer matches this run.";
    case "database_write":
      return "Alfred could not add this output to Memories.";
    case "internal":
    case null:
      return "Alfred could not finish saving this output.";
  }
}
