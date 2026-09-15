import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  mergeOutputReceiptIntoHistory,
  mergeRefreshedOutputHistory,
  outputPersistenceErrorDescription,
  outputReceiptKey,
  retryOutputReceiptAndRefresh,
  retryOutputPersistenceRequest,
  type OutputPersistenceErrorCode,
  type OutputReceipt,
  type OutputReceiptState,
} from "./output-receipt";
import { OutputReceipts } from "./output-receipts";
import type { RunHistoryDetail } from "../../types";

const AT = "2026-09-09T12:00:00Z";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function receipt(
  state: OutputReceiptState,
  errorCode: OutputPersistenceErrorCode | null = null,
): OutputReceipt {
  return {
    runId: "run-1",
    nodeId: `output-${state}-${errorCode ?? "none"}`,
    stepId: "step-1",
    disposition: {
      label: "Release report",
      saveToMemory: state !== "not_requested",
      pinMemory: true,
      memoryTitle: "Captured report",
      asFinalResult: true,
      htmlReport: false,
      includeFilesChanged: true,
    },
    resolvedMemoryTitle: "Captured report",
    state,
    storageIdentity: state === "not_requested" || state === "skipped_empty" ? null : "storage-1",
    outputSha256: state === "not_requested" || state === "skipped_empty" ? null : "a".repeat(64),
    artifactRequired: false,
    memoryId: state === "saved" ? "memory-1" : null,
    errorCode,
    savingStartedAt: state === "saving" ? AT : null,
    createdAt: AT,
    updatedAt: AT,
    deletedAt: state === "deleted" ? AT : null,
  };
}

function historyDetail(outputReceipts: OutputReceipt[]): RunHistoryDetail {
  return {
    run: {
      id: "run-1",
      workflowId: "workflow-1",
      workflowName: "Release workflow",
      trigger: "manual",
      status: "completed",
      error: null,
      startedAt: AT,
      finishedAt: AT,
      createdAt: AT,
      stepCount: 1,
      finalOutputPreview: "Release report",
    },
    steps: [],
    memoryUses: [],
    memoryContextSummaries: [],
    evidenceSources: [],
    outputReceipts,
  };
}

describe("OutputReceipts", () => {
  test("renders every durable state with plain-language status", () => {
    const markup = renderToStaticMarkup(
      <OutputReceipts
        receipts={[
          receipt("not_requested"),
          receipt("skipped_empty"),
          receipt("pending"),
          receipt("saving"),
          receipt("saved"),
          receipt("failed", "database_write"),
          receipt("deleted"),
        ]}
      />,
    );

    for (const label of [
      "Not saved",
      "Nothing to save",
      "Waiting to save",
      "Saving",
      "Saved to Memories",
      "Save failed",
      "Removed from Memories",
    ]) {
      expect(markup).toContain(label);
    }
    expect(markup).toContain("will not be recreated");
    expect(markup).toContain("Pinned when saved");
    expect(markup).toContain("Pin requested");
    expect(markup).not.toContain("claimIdentity");
  });

  test("describes pinning according to persistence state", () => {
    for (const state of ["not_requested", "skipped_empty"] satisfies OutputReceiptState[]) {
      const markup = renderToStaticMarkup(<OutputReceipts receipts={[receipt(state)]} />);
      expect(markup).not.toContain("Pinned when saved");
      expect(markup).not.toContain("Pin requested");
    }

    for (const state of [
      "pending",
      "saving",
      "failed",
      "deleted",
    ] satisfies OutputReceiptState[]) {
      const markup = renderToStaticMarkup(
        <OutputReceipts
          receipts={[receipt(state, state === "failed" ? "database_write" : null)]}
        />,
      );
      expect(markup).toContain("Pin requested");
      expect(markup).not.toContain("Pinned when saved");
    }

    const savedMarkup = renderToStaticMarkup(
      <OutputReceipts receipts={[receipt("saved")]} />,
    );
    expect(savedMarkup).toContain("Pinned when saved");
    expect(savedMarkup).not.toContain("Pin requested");
  });

  test("offers retry only for the three backend-retryable failures", () => {
    const retryable: OutputPersistenceErrorCode[] = [
      "artifact_stage_io",
      "artifact_finalize_io",
      "database_write",
    ];
    for (const errorCode of retryable) {
      const failed = receipt("failed", errorCode);
      expect(retryOutputPersistenceRequest(failed)).toEqual({
        runId: failed.runId,
        nodeId: failed.nodeId,
      });
      expect(
        renderToStaticMarkup(<OutputReceipts receipts={[failed]} onRetry={() => {}} />),
      ).toContain("Retry save");
    }

    for (const errorCode of [
      "invalid_disposition",
      "step_output_unavailable",
      "artifact_content_mismatch",
      "internal",
    ] satisfies OutputPersistenceErrorCode[]) {
      const failed = receipt("failed", errorCode);
      expect(retryOutputPersistenceRequest(failed)).toBeNull();
      expect(
        renderToStaticMarkup(<OutputReceipts receipts={[failed]} onRetry={() => {}} />),
      ).not.toContain("Retry save");
    }
  });

  test("retry request contains identity only and retrying state stays disabled", () => {
    const failed = receipt("failed", "database_write");
    const key = outputReceiptKey(failed);
    expect(retryOutputPersistenceRequest(failed)).toEqual({
      runId: "run-1",
      nodeId: failed.nodeId,
    });
    const markup = renderToStaticMarkup(
      <OutputReceipts
        receipts={[failed]}
        retryingReceiptKeys={new Set([key])}
        onRetry={() => {}}
      />,
    );
    expect(markup).toContain("Retrying…");
    expect(markup).toContain("disabled");
    expect(markup).not.toContain("memoryTitle=");
    expect(markup).not.toContain("outputSha256=");
  });

  test("commits a successful retry before a fallible history refresh", async () => {
    const failed = receipt("failed", "database_write");
    const saved = {
      ...failed,
      state: "saved" as const,
      memoryId: failed.storageIdentity,
      errorCode: null,
    };
    let committed: OutputReceipt | null = null;
    const result = await retryOutputReceiptAndRefresh(
      { runId: failed.runId, nodeId: failed.nodeId },
      {
        retry: async () => saved,
        onReceipt: (next) => {
          committed = next;
        },
        refresh: async () => {
          expect(committed).toEqual(saved);
          throw new Error("history_unavailable");
        },
      },
    );

    expect(result.receipt).toEqual(saved);
    expect(result.refreshedDetail).toBeNull();
    expect(result.refreshFailed).toBe(true);
    expect(result.saved).toBe(true);
    expect(committed).toEqual(saved);
  });

  test("does not regress a saved retry when refreshed history is stale", async () => {
    const failed = receipt("failed", "database_write");
    const saved = {
      ...failed,
      state: "saved" as const,
      memoryId: failed.storageIdentity,
      errorCode: null,
    };
    const result = await retryOutputReceiptAndRefresh(
      { runId: failed.runId, nodeId: failed.nodeId },
      {
        retry: async () => saved,
        onReceipt: () => {},
        refresh: async () => historyDetail([failed]),
      },
    );

    expect(result.saved).toBe(true);
    expect(result.refreshFailed).toBe(false);
    expect(result.refreshedDetail?.outputReceipts?.[0]).toEqual(saved);
  });

  test("overlapping stale refreshes preserve every locally terminal receipt", async () => {
    const failedA = {
      ...receipt("failed", "database_write"),
      nodeId: "output-a",
    };
    const failedB = { ...failedA, nodeId: "output-b" };
    const savedA = {
      ...failedA,
      state: "saved" as const,
      memoryId: "memory-a",
      errorCode: null,
    };
    const savedB = {
      ...failedB,
      state: "saved" as const,
      memoryId: "memory-b",
      errorCode: null,
    };
    let current: RunHistoryDetail | null = historyDetail([failedA, failedB]);
    const staleRefreshA = deferred<RunHistoryDetail | null>();

    const applyRetry = async (
      saved: OutputReceipt,
      refresh: () => Promise<RunHistoryDetail | null>,
    ) => {
      const result = await retryOutputReceiptAndRefresh(
        { runId: saved.runId, nodeId: saved.nodeId },
        {
          retry: async () => saved,
          onReceipt: (next) => {
            current = mergeOutputReceiptIntoHistory(current, next);
          },
          refresh,
        },
      );
      if (result.refreshedDetail) {
        current = mergeRefreshedOutputHistory(
          current,
          result.refreshedDetail,
        );
      }
      return result;
    };

    const retryA = applyRetry(savedA, () => staleRefreshA.promise);
    const retryB = applyRetry(savedB, async () => {
      throw new Error("history_unavailable");
    });
    expect((await retryB).refreshFailed).toBe(true);
    expect(current?.outputReceipts?.map((item) => item.state)).toEqual([
      "saved",
      "saved",
    ]);

    staleRefreshA.resolve(historyDetail([savedA, failedB]));
    await retryA;
    expect(current?.outputReceipts?.map((item) => item.state)).toEqual([
      "saved",
      "saved",
    ]);

    current = mergeOutputReceiptIntoHistory(current, {
      ...savedB,
      state: "deleted",
      memoryId: null,
      deletedAt: AT,
    });
    current = mergeRefreshedOutputHistory(
      current,
      historyDetail([savedA, savedB]),
    );
    expect(
      current?.outputReceipts?.find((item) => item.nodeId === "output-b")?.state,
    ).toBe("deleted");
  });

  test("shows a terminal non-saved retry result without claiming success", async () => {
    const failed = receipt("failed", "database_write");
    const deleted = {
      ...failed,
      state: "deleted" as const,
      errorCode: null,
      deletedAt: AT,
    };
    let committed: OutputReceipt | null = null;
    let refreshed = false;
    const result = await retryOutputReceiptAndRefresh(
      { runId: failed.runId, nodeId: failed.nodeId },
      {
        retry: async () => deleted,
        onReceipt: (next) => {
          committed = next;
        },
        refresh: async () => {
          refreshed = true;
          return historyDetail([deleted]);
        },
      },
    );

    expect(result.saved).toBe(false);
    expect(result.refreshFailed).toBe(false);
    expect(committed).toEqual(deleted);
    expect(refreshed).toBe(false);
  });

  test("rejects a retry response for a different receipt identity", async () => {
    const failed = receipt("failed", "database_write");
    let committed = false;
    let refreshed = false;
    await expect(
      retryOutputReceiptAndRefresh(
        { runId: failed.runId, nodeId: failed.nodeId },
        {
          retry: async () => ({ ...failed, nodeId: "different-node" }),
          onReceipt: () => {
            committed = true;
          },
          refresh: async () => {
            refreshed = true;
            return null;
          },
        },
      ),
    ).rejects.toThrow("output_receipt_identity_mismatch");
    expect(committed).toBe(false);
    expect(refreshed).toBe(false);
  });

  test("composite receipt keys cannot alias delimiter-like IDs", () => {
    expect(outputReceiptKey({ runId: "run:a", nodeId: "b" })).not.toBe(
      outputReceiptKey({ runId: "run", nodeId: "a:b" }),
    );
  });

  test("saved output can open its canonical memory and legacy empty data stays absent", () => {
    expect(renderToStaticMarkup(<OutputReceipts receipts={[]} />)).toBe("");
    const markup = renderToStaticMarkup(
      <OutputReceipts receipts={[receipt("saved")]} onOpenMemory={() => {}} />,
    );
    expect(markup).toContain('aria-label="Open Captured report in Memories"');
    expect(markup).toContain("history-memory-link");
  });

  test("error copy remains explicit when retry is unavailable", () => {
    expect(outputPersistenceErrorDescription("artifact_content_mismatch")).toBe(
      "The stored report file no longer matches this run.",
    );
    expect(outputPersistenceErrorDescription(null)).toBe(
      "Alfred could not finish saving this output.",
    );
  });
});
