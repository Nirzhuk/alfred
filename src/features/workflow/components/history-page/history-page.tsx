import { useEffect, useMemo, useRef, useState } from "react";
import { SearchInput } from "../../../../components/search-input";
import * as api from "../../api";
import { memoryReviewFailureCopy } from "../../../settings/memory-review";
import {
  reviewStatusLabel,
} from "../memories-inspector/suggestions-model";
import type {
  HistorySearchHit,
  MemoryReviewJob,
  OutputPersistenceEvent,
  RunHistoryDetail,
  RunHistoryItem,
} from "../../types";
import {
  formatHistoryJson,
  formatHistoryWhen,
  historyHitLabel,
  historyMode,
  historyWorkflowId,
  isCurrentHistoryGeneration,
  literalHistorySnippet,
  memoryUseReasonLabel,
  memoryRecallSummary,
  openHistoryMemory,
  openHistorySuggestions,
  type HistoryScope,
} from "./history-format";
import {
  mergeOutputReceiptIntoHistory,
  mergeRefreshedOutputHistory,
  outputReceiptKey,
  retryOutputReceiptAndRefresh,
  type RetryOutputPersistenceRequest,
} from "./output-receipt";
import { OutputReceipts } from "./output-receipts";

const PAGE_SIZE = 25;

type Props = {
  activeWorkflowId: string | null;
  initialRunId?: string | null;
  onSelectedRunIdChange?: (runId: string | null) => void;
  onClose: () => void;
};

export function HistoryPage({
  activeWorkflowId,
  initialRunId = null,
  onSelectedRunIdChange,
  onClose,
}: Props) {
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [scope, setScope] = useState<HistoryScope>(
    activeWorkflowId ? "current" : "all",
  );

  useEffect(() => {
    const receiveReceipt = (event: Event) => {
      const payload = (event as CustomEvent<OutputPersistenceEvent>).detail;
      if (!payload?.receipt) return;
      setDetail((current) =>
        mergeOutputReceiptIntoHistory(current, payload.receipt),
      );
    };
    window.addEventListener("alfred:output-persistence-receipt", receiveReceipt);
    return () =>
      window.removeEventListener(
        "alfred:output-persistence-receipt",
        receiveReceipt,
      );
  }, []);
  const [runs, setRuns] = useState<RunHistoryItem[]>([]);
  const [hits, setHits] = useState<HistorySearchHit[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(
    initialRunId,
  );
  const [detail, setDetail] = useState<RunHistoryDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [reviewJob, setReviewJob] = useState<MemoryReviewJob | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState(false);
  const [outputReceiptError, setOutputReceiptError] = useState<string | null>(
    null,
  );
  const [retryingReceiptKeys, setRetryingReceiptKeys] = useState<Set<string>>(
    () => new Set(),
  );
  const retryingReceiptKeysRef = useRef(new Set<string>());
  const requestGeneration = useRef(0);
  const detailGeneration = useRef(0);

  useEffect(() => {
    const timeout = window.setTimeout(() => setDebouncedQuery(query), 250);
    return () => window.clearTimeout(timeout);
  }, [query]);

  useEffect(() => {
    if (!activeWorkflowId && scope === "current") setScope("all");
  }, [activeWorkflowId, scope]);

  useEffect(() => {
    setSelectedRunId(initialRunId);
  }, [initialRunId]);

  useEffect(
    () => () => {
      requestGeneration.current += 1;
      detailGeneration.current += 1;
    },
    [],
  );

  const workflowId = historyWorkflowId(scope, activeWorkflowId);
  const mode = historyMode(debouncedQuery);

  useEffect(() => {
    const generation = ++requestGeneration.current;
    let cancelled = false;
    setLoading(true);
    setLoadingMore(false);
    setError(false);

    const request =
      mode === "browse"
        ? api
            .listRunHistory({ workflowId, limit: PAGE_SIZE, offset: 0 })
            .then((rows) => {
              if (
                cancelled ||
                !isCurrentHistoryGeneration(
                  generation,
                  requestGeneration.current,
                )
              ) {
                return;
              }
              setRuns(rows);
              setHits([]);
              setHasMore(rows.length === PAGE_SIZE);
            })
        : api
            .searchHistory({
              query: debouncedQuery.trim(),
              workflowId,
              limit: 50,
            })
            .then((rows) => {
              if (
                cancelled ||
                !isCurrentHistoryGeneration(
                  generation,
                  requestGeneration.current,
                )
              ) {
                return;
              }
              setHits(rows);
              setRuns([]);
              setHasMore(false);
            });

    void request
      .catch(() => {
        if (
          !cancelled &&
          isCurrentHistoryGeneration(generation, requestGeneration.current)
        ) {
          setError(true);
        }
      })
      .finally(() => {
        if (
          !cancelled &&
          isCurrentHistoryGeneration(generation, requestGeneration.current)
        ) {
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [debouncedQuery, mode, workflowId]);

  const loadMore = async () => {
    if (loadingMore || !hasMore || mode !== "browse") return;
    const generation = requestGeneration.current;
    setLoadingMore(true);
    setError(false);
    try {
      const rows = await api.listRunHistory({
        workflowId,
        limit: PAGE_SIZE,
        offset: runs.length,
      });
      if (!isCurrentHistoryGeneration(generation, requestGeneration.current)) {
        return;
      }
      setRuns((current) => [...current, ...rows]);
      setHasMore(rows.length === PAGE_SIZE);
    } catch {
      if (isCurrentHistoryGeneration(generation, requestGeneration.current)) {
        setError(true);
      }
    } finally {
      if (isCurrentHistoryGeneration(generation, requestGeneration.current)) {
        setLoadingMore(false);
      }
    }
  };

  useEffect(() => {
    if (!selectedRunId) {
      detailGeneration.current += 1;
      setLoadingDetail(false);
      setDetail(null);
      setReviewJob(null);
      setOutputReceiptError(null);
      return;
    }
    const generation = ++detailGeneration.current;
    let cancelled = false;
    setLoadingDetail(true);
    setError(false);
    setOutputReceiptError(null);
    void api
      .getRunHistory(selectedRunId)
      .then((next) => {
        if (!cancelled && generation === detailGeneration.current) {
          setDetail(next);
        }
      })
      .catch(() => {
        if (!cancelled && generation === detailGeneration.current) {
          setError(true);
        }
      })
      .finally(() => {
        if (!cancelled && generation === detailGeneration.current) {
          setLoadingDetail(false);
        }
      });
    // Review metadata is a separate read-only query; absence is normal.
    void api
      .getMemoryReviewJob(selectedRunId)
      .then((job) => {
        if (!cancelled) setReviewJob(job ?? null);
      })
      .catch(() => {
        if (!cancelled) setReviewJob(null);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedRunId]);

  const openRun = (runId: string) => {
    setSelectedRunId(runId);
    onSelectedRunIdChange?.(runId);
  };

  const retryOutputReceipt = async (
    request: RetryOutputPersistenceRequest,
  ) => {
    const key = outputReceiptKey(request);
    if (retryingReceiptKeysRef.current.has(key)) return;
    const nextRetrying = new Set(retryingReceiptKeysRef.current);
    nextRetrying.add(key);
    retryingReceiptKeysRef.current = nextRetrying;
    setRetryingReceiptKeys(nextRetrying);
    setOutputReceiptError(null);
    const generation = detailGeneration.current;
    try {
      const result = await retryOutputReceiptAndRefresh(request, {
        retry: api.retryOutputPersistence,
        onReceipt: (receipt) => {
          if (generation !== detailGeneration.current) return;
          setDetail((current) =>
            mergeOutputReceiptIntoHistory(current, receipt),
          );
        },
        refresh: api.getRunHistory,
      });
      if (generation !== detailGeneration.current) return;
      const refreshedDetail = result.refreshedDetail;
      if (refreshedDetail) {
        setDetail((current) =>
          mergeRefreshedOutputHistory(current, refreshedDetail),
        );
      }
      if (!result.saved && result.receipt.state !== "deleted") {
        setOutputReceiptError(
          "This retry did not save the output. Its latest status is shown above.",
        );
      } else if (result.refreshFailed) {
        setOutputReceiptError(
          "Output was saved, but History couldn’t refresh. Reload History to see the latest details.",
        );
      }
    } catch {
      if (generation === detailGeneration.current) {
        setOutputReceiptError("Output couldn’t be saved. Try again.");
      }
    } finally {
      const remaining = new Set(retryingReceiptKeysRef.current);
      remaining.delete(key);
      retryingReceiptKeysRef.current = remaining;
      setRetryingReceiptKeys(remaining);
    }
  };

  const groupedHits = useMemo(
    () => ({
      runSteps: hits.filter((hit) => hit.kind === "run_step"),
      memories: hits.filter((hit) => hit.kind === "memory"),
    }),
    [hits],
  );

  return (
    <section className="settings-page history-page" aria-labelledby="history-title">
      <header className="settings-page-header">
        <div>
          <p className="settings-kicker">Private local data</p>
          <h1 id="history-title">History</h1>
          <p className="settings-page-lead">Browse past runs, steps, and saved memories.</p>
        </div>
        <button type="button" className="ghost" onClick={onClose}>
          Back to canvas
        </button>
      </header>

      <div className="settings-page-body history-page-body">
        <div className="history-toolbar">
          <SearchInput
            aria-label="Search history"
            containerClassName="history-search"
            value={query}
            placeholder="Search prompts, outputs, and memories"
            onChange={(event) => setQuery(event.target.value)}
            onClear={() => setQuery("")}
          />
          <div className="history-scope" role="group" aria-label="History scope">
            {activeWorkflowId ? (
              <button
                type="button"
                className={scope === "current" ? "is-active" : ""}
                aria-pressed={scope === "current"}
                onClick={() => setScope("current")}
              >
                Current workflow
              </button>
            ) : null}
            <button
              type="button"
              className={scope === "all" ? "is-active" : ""}
              aria-pressed={scope === "all"}
              onClick={() => setScope("all")}
            >
              All workflows
            </button>
          </div>
        </div>

        {error ? (
          <div className="history-message is-error" role="alert">
            History couldn’t be loaded. Try again.
          </div>
        ) : null}

        {detail ? (
          <RunDetail
            detail={detail}
            reviewJob={reviewJob}
            outputReceiptError={outputReceiptError}
            retryingReceiptKeys={retryingReceiptKeys}
            onRetryOutput={retryOutputReceipt}
            onBack={() => {
              setSelectedRunId(null);
              onSelectedRunIdChange?.(null);
            }}
          />
        ) : loadingDetail ? (
          <div className="history-message" role="status">
            Loading run detail…
          </div>
        ) : loading ? (
          <div className="history-message" role="status">
            {mode === "browse" ? "Loading recent runs…" : "Searching history…"}
          </div>
        ) : mode === "browse" ? (
          <RunBrowser
            runs={runs}
            hasMore={hasMore}
            loadingMore={loadingMore}
            onOpenRun={openRun}
            onLoadMore={loadMore}
          />
        ) : (
          <SearchResults
            runSteps={groupedHits.runSteps}
            memories={groupedHits.memories}
            onOpenRun={openRun}
          />
        )}
      </div>
    </section>
  );
}

function RunBrowser({
  runs,
  hasMore,
  loadingMore,
  onOpenRun,
  onLoadMore,
}: {
  runs: RunHistoryItem[];
  hasMore: boolean;
  loadingMore: boolean;
  onOpenRun: (runId: string) => void;
  onLoadMore: () => void;
}) {
  if (runs.length === 0) {
    return <div className="history-message">No persisted runs in this scope.</div>;
  }
  return (
    <div className="history-results">
      <div className="history-section-heading">
        <h2>Newest runs</h2>
        <span>{runs.length} loaded</span>
      </div>
      <ul className="history-list">
        {runs.map((run) => (
          <li key={run.id}>
            <button
              type="button"
              className="history-card"
              onClick={() => onOpenRun(run.id)}
            >
              <span className="history-card-topline">
                <strong>{run.workflowName}</strong>
                <span className={`history-status is-${run.status}`}>{run.status}</span>
              </span>
              <span className="history-card-meta">
                {formatHistoryWhen(run.createdAt)} · {run.trigger} · {run.stepCount}{" "}
                {run.stepCount === 1 ? "step" : "steps"}
              </span>
              {run.finalOutputPreview ? (
                <span className="history-card-snippet">{run.finalOutputPreview}</span>
              ) : null}
            </button>
          </li>
        ))}
      </ul>
      {hasMore ? (
        <button
          type="button"
          className="ghost history-load-more"
          disabled={loadingMore}
          onClick={onLoadMore}
        >
          {loadingMore ? "Loading…" : "Load more"}
        </button>
      ) : null}
    </div>
  );
}

function SearchResults({
  runSteps,
  memories,
  onOpenRun,
}: {
  runSteps: HistorySearchHit[];
  memories: HistorySearchHit[];
  onOpenRun: (runId: string) => void;
}) {
  if (runSteps.length === 0 && memories.length === 0) {
    return <div className="history-message">No history matched this search.</div>;
  }
  return (
    <div className="history-search-groups">
      <SearchGroup
        title="Run steps"
        hits={runSteps}
        onOpen={(hit) => hit.runId && onOpenRun(hit.runId)}
      />
      <SearchGroup
        title="Memories"
        hits={memories}
      />
    </div>
  );
}

function SearchGroup({
  title,
  hits,
  onOpen,
}: {
  title: string;
  hits: HistorySearchHit[];
  onOpen?: (hit: HistorySearchHit) => void;
}) {
  if (hits.length === 0) return null;
  return (
    <section className="history-results" aria-label={title}>
      <div className="history-section-heading">
        <h2>{title}</h2>
        <span>{hits.length}</span>
      </div>
      <ul className="history-list">
        {hits.map((hit) => (
          <li key={`${hit.kind}:${hit.sourceId}`}>
            {onOpen ? (
              <button
                type="button"
                className="history-card"
                onClick={() => onOpen(hit)}
              >
                <SearchHitContent hit={hit} />
              </button>
            ) : (
              <article className="history-card">
                <SearchHitContent hit={hit} />
              </article>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

function SearchHitContent({ hit }: { hit: HistorySearchHit }) {
  return (
    <>
      <span className="history-card-topline">
        <strong>{hit.title}</strong>
        <span className="history-kind">{historyHitLabel(hit.kind)}</span>
      </span>
      <span className="history-card-meta">
        {hit.workflowName} · {formatHistoryWhen(hit.timestamp)}
      </span>
      <span className="history-card-snippet">
        {literalHistorySnippet(hit.snippet)}
      </span>
    </>
  );
}

function RunDetail({
  detail,
  reviewJob,
  outputReceiptError,
  retryingReceiptKeys,
  onRetryOutput,
  onBack,
}: {
  detail: RunHistoryDetail;
  reviewJob: MemoryReviewJob | null;
  outputReceiptError: string | null;
  retryingReceiptKeys: ReadonlySet<string>;
  onRetryOutput: (request: RetryOutputPersistenceRequest) => Promise<void>;
  onBack: () => void;
}) {
  const memoryUses = detail.memoryUses ?? [];
  const contextSummaries = detail.memoryContextSummaries ?? [];
  const omittedCounts = contextSummaries.reduce(
    (counts, summary) => ({
      selected: counts.selected + summary.explicitOmittedCount,
      pinned: counts.pinned + summary.pinnedOmittedCount,
      recalled: counts.recalled + summary.automaticOmittedCount,
      unavailable: counts.unavailable + summary.unavailableCount,
      repeated: counts.repeated + summary.repeatedCount,
    }),
    { selected: 0, pinned: 0, recalled: 0, unavailable: 0, repeated: 0 },
  );
  const omittedContextReasons = [
    omittedCounts.selected > 0 ? `${omittedCounts.selected} selected for size` : null,
    omittedCounts.pinned > 0 ? `${omittedCounts.pinned} pinned for size` : null,
    omittedCounts.recalled > 0
      ? `${omittedCounts.recalled} recalled for size or trace availability`
      : null,
    omittedCounts.unavailable > 0 ? `${omittedCounts.unavailable} unavailable` : null,
    omittedCounts.repeated > 0 ? `${omittedCounts.repeated} repeated` : null,
  ].filter((reason): reason is string => reason !== null);
  const recallSummary = memoryRecallSummary(detail.steps);
  return (
    <article className="history-detail">
      <button type="button" className="ghost history-detail-back" onClick={onBack}>
        Back to results
      </button>
      <header>
        <div>
          <p className="history-kicker">{detail.run.trigger} run</p>
          <h2>{detail.run.workflowName}</h2>
          <p>{formatHistoryWhen(detail.run.createdAt)}</p>
        </div>
        <span className={`history-status is-${detail.run.status}`}>
          {detail.run.status}
        </span>
      </header>
      {detail.run.error ? (
        <pre className="history-error-text">{detail.run.error}</pre>
      ) : null}
      {reviewJob ? (
        <section className="history-memory-context" aria-labelledby="history-review-title">
          <div className="history-section-heading">
            <h3 id="history-review-title">Memory review</h3>
            <span className={`history-status is-${reviewJob.status}`}>
              {reviewStatusLabel(reviewJob.status)}
            </span>
          </div>
          <p className="history-review-meta">
            Reviewer: {reviewJob.provider}
            {reviewJob.model ? ` · ${reviewJob.model}` : ""} ·{" "}
            {reviewJob.candidateCount} suggestion
            {reviewJob.candidateCount === 1 ? "" : "s"} proposed
            {reviewJob.finishedAt
              ? ` · finished ${formatHistoryWhen(reviewJob.finishedAt)}`
              : ""}
          </p>
          {reviewJob.status === "failed" && reviewJob.errorCode ? (
            <p className="muted" role="alert">
              {memoryReviewFailureCopy(reviewJob.errorCode)}
            </p>
          ) : null}
          {reviewJob.candidateCount > 0 || reviewJob.status === "failed" ? (
            <button
              type="button"
              className="ghost history-memory-link"
              onClick={() => openHistorySuggestions(detail.run.id)}
            >
              Open suggestions from this run in Memories
            </button>
          ) : null}
        </section>
      ) : null}
      <OutputReceipts
        receipts={detail.outputReceipts ?? []}
        retryingReceiptKeys={retryingReceiptKeys}
        onRetry={onRetryOutput}
        onOpenMemory={(memoryId) =>
          openHistoryMemory(memoryId, detail.run.workflowId)
        }
      />
      {outputReceiptError ? (
        <p className="history-message is-error" role="alert">
          {outputReceiptError}
        </p>
      ) : null}
      <section className="history-memory-context" aria-labelledby="history-memory-title">
        <div className="history-section-heading">
          <h3 id="history-memory-title">Memory context</h3>
          <span>{memoryUses.length}</span>
        </div>
        {contextSummaries.map((summary) => {
          const stepIndex = detail.steps.findIndex(
            (step) => step.nodeId === summary.nodeId,
          );
          const stepLabel = stepIndex >= 0 ? `Step ${stepIndex + 1}` : summary.nodeId;
          return (
            <p className="history-memory-meta" key={summary.nodeId}>
              {stepLabel}: {summary.memoryBytes.toLocaleString()} of{" "}
              {summary.budgetBytes.toLocaleString()} memory bytes used
            </p>
          );
        })}
        {recallSummary.status === "unavailable" ? (
          <p className="muted" role="alert">
            Automatic memory recall was unavailable for one or more agent steps.
          </p>
        ) : recallSummary.status === "abstained" ? (
          <p className="muted">
            Automatic recall ran and found no relevant memories for one or more
            agent steps.
          </p>
        ) : null}
        {recallSummary.omittedCount > 0 ? (
          <p className="muted">
            {recallSummary.omittedCount} additional relevant memor
            {recallSummary.omittedCount === 1 ? "y was" : "ies were"} omitted by
            the context limit.
          </p>
        ) : null}
        {omittedContextReasons.length > 0 ? (
          <p className="muted">
            Memory references omitted: {omittedContextReasons.join(", ")}.
          </p>
        ) : null}
        {contextSummaries.some((summary) => !summary.traceAvailable) ? (
          <p className="muted" role="alert">
            Some selected or pinned memory could not be recorded in the audit
            trace. Automatic memory was left out of those steps.
          </p>
        ) : null}
        {memoryUses.length === 0 ? (
          <p className="muted">
            No selected, pinned, or recalled memory was recorded for this run.
          </p>
        ) : (
          <div className="history-memory-groups">
            {detail.steps.map((step, index) => {
              const uses = memoryUses.filter(
                (memoryUse) => memoryUse.nodeId === step.nodeId,
              );
              if (uses.length === 0) return null;
              return (
                <section key={step.id} className="history-memory-group">
                  <h4>
                    Step {index + 1} · {step.skillName || step.agentProvider || step.nodeId}
                  </h4>
                  <ul>
                    {uses.map((memoryUse) => (
                      <li key={`${step.id}:${memoryUse.memoryId}`}>
                        <button
                          type="button"
                          className="history-memory-link"
                          onClick={() =>
                            openHistoryMemory(
                              memoryUse.memoryId,
                              detail.run.workflowId,
                            )
                          }
                        >
                          {memoryUse.memoryTitle || "[deleted memory]"}
                        </button>
                        <span className="history-memory-reason">
                          {memoryUseReasonLabel(memoryUse.reason)}
                        </span>
                        <span className="history-memory-meta">
                          {memoryUse.memoryId} · {memoryUse.scopeType} · {memoryUse.memoryType}
                          {" · "}rank {memoryUse.rank} · score {memoryUse.score.toFixed(2)}
                          {" · "}policy {memoryUse.rankingVersion}
                          {" · "}{memoryUse.renderedBytes.toLocaleString()} bytes
                          {" · "}{formatHistoryWhen(memoryUse.createdAt)}
                        </span>
                      </li>
                    ))}
                  </ul>
                </section>
              );
            })}
          </div>
        )}
      </section>
      {detail.steps.length === 0 ? (
        <div className="history-message">This run has no persisted steps.</div>
      ) : (
        <ol className="history-step-list">
          {detail.steps.map((step, index) => (
            <li key={step.id} className="history-step">
              <div className="history-step-heading">
                <div>
                  <span>Step {index + 1}</span>
                  <h3>{step.skillName || step.agentProvider || step.nodeId}</h3>
                  <p>
                    {step.agentProvider || "Local"} · {formatHistoryWhen(step.createdAt)}
                  </p>
                </div>
                <span className={`history-status is-${step.status}`}>{step.status}</span>
              </div>
              {step.error ? <pre className="history-error-text">{step.error}</pre> : null}
              <details>
                <summary>Input</summary>
                <pre>{formatHistoryJson(step.input)}</pre>
              </details>
              <details>
                <summary>Output</summary>
                <pre>{formatHistoryJson(step.output)}</pre>
              </details>
            </li>
          ))}
        </ol>
      )}
    </article>
  );
}
