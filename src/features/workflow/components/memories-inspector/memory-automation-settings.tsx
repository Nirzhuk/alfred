import { Icon } from "../../../../components/icon";
const MEMORY_LOOKUP_STATUS_ID = "memory-lookup-availability-status";


export function MemoryAutomationSettings({
  automaticRecall,
  recallSaving,
  onToggleRecall,
  memoryLookupEnabled,
  memoryLookupAvailable,
  memoryLookupAvailabilityLoading,
  memoryLookupAvailabilityError,
  onRetryMemoryLookupAvailability,
  memoryLookupSaving,
  memoryLookupAvailabilityReason,
  onToggleMemoryLookup,
  automaticSuggestions,
  reviewSaving,
  reviewGateReason,
  onToggleReview,
  disabled,
}: {
  automaticRecall: boolean;
  recallSaving: boolean;
  onToggleRecall: () => void;
  memoryLookupEnabled: boolean;
  memoryLookupAvailable: boolean;
  memoryLookupAvailabilityLoading: boolean;
  memoryLookupAvailabilityError: string | null;
  onRetryMemoryLookupAvailability: () => void;
  memoryLookupSaving: boolean;
  memoryLookupAvailabilityReason: string | null;
  onToggleMemoryLookup: () => void;
  automaticSuggestions: boolean;
  reviewSaving: boolean;
  reviewGateReason: string | null;
  onToggleReview: () => void;
  disabled: boolean;
}) {
  const memoryLookupStatus: {
    message: string;
    role?: "status" | "alert";
  } = memoryLookupSaving
    ? { message: "Saving memory lookup permission…", role: "status" }
    : memoryLookupAvailabilityLoading
      ? { message: "Checking memory lookup availability…", role: "status" }
      : memoryLookupAvailabilityError
        ? { message: memoryLookupAvailabilityError, role: "alert" }
        : memoryLookupAvailable
          ? {
              message: "Memory lookup is available for this workflow.",
              role: "status",
            }
          : {
              message:
                memoryLookupAvailabilityReason ??
                "Memory lookup is unavailable for this workflow right now.",
              role: "status",
            };
  return (
    <div className="memory-automation-settings">
      <div className="memory-detail-eyebrow">
        <Icon name="sliders" size={16} /> Workflow settings
      </div>
      <h3>How this workflow remembers</h3>
      <p>
        Choose when memories help your agents and how new knowledge reaches your
        library.
      </p>
      <div className="memory-automation-setting">
        <div>
          <strong>Automatic recall</strong>
          <p>
            Find relevant memories for each agent’s task and add them to its
            context.
          </p>
        </div>
        <button
          type="button"
          role="switch"
          aria-label="Automatic recall"
          aria-checked={automaticRecall}
          className={`settings-toggle${automaticRecall ? " is-on" : ""}`}
          disabled={disabled || recallSaving}
          onClick={onToggleRecall}
        >
          <span className="settings-toggle-knob" />
        </button>
      </div>
      <div className="memory-automation-setting">
        <div>
          <strong>Allow agents to look up memory</strong>
          <p>
            Let agents request relevant saved memories during a run. This is
            separate from automatic recall, which adds memories without an agent
            request.
          </p>
          <div className="memory-lookup-status-row">
            <small id={MEMORY_LOOKUP_STATUS_ID} role={memoryLookupStatus.role}>
              {memoryLookupStatus.message}
            </small>
            {memoryLookupAvailabilityError ? (
              <button
                type="button"
                className="ghost memory-lookup-retry"
                disabled={disabled || memoryLookupSaving}
                onClick={onRetryMemoryLookupAvailability}
              >
                Try again
              </button>
            ) : null}
          </div>
        </div>
        <button
          type="button"
          role="switch"
          aria-label="Allow agents to look up memory"
          aria-checked={memoryLookupEnabled}
          aria-describedby={MEMORY_LOOKUP_STATUS_ID}
          aria-busy={memoryLookupSaving}
          className={`settings-toggle${memoryLookupEnabled ? " is-on" : ""}`}
          disabled={
            disabled ||
            memoryLookupSaving ||
            (!memoryLookupEnabled &&
              (memoryLookupAvailabilityLoading ||
                !memoryLookupAvailable ||
                Boolean(memoryLookupAvailabilityError)))
          }
          onClick={onToggleMemoryLookup}
        >
          <span className="settings-toggle-knob" />
        </button>
      </div>
      <div className="memory-automation-setting">
        <div>
          <strong>Suggest memories after runs</strong>
          <p>
            Your configured reviewer can propose useful knowledge after a run.
            Review each suggestion before saving it.
          </p>
          {reviewGateReason ? (
            <small>{reviewGateReason}</small>
          ) : (
            <small>
              Reviews use your selected provider and may incur provider costs.
            </small>
          )}
        </div>
        <button
          type="button"
          role="switch"
          aria-label="Suggest memories after runs"
          aria-checked={automaticSuggestions}
          className={`settings-toggle${automaticSuggestions ? " is-on" : ""}`}
          disabled={disabled || reviewSaving || Boolean(reviewGateReason)}
          onClick={onToggleReview}
        >
          <span className="settings-toggle-knob" />
        </button>
      </div>
      <div className="memory-automation-guide">
        <Icon name="push-pin" size={18} />
        <div>
          <strong>Pin what matters every time</strong>
          <p>
            Pinned memories are considered on every run, within the context
            limit. Automatic recall adds other relevant memories when it is on.
          </p>
        </div>
        <Icon name="note" size={18} />
        <div>
          <strong>Choose exact memories on the canvas</strong>
          <p>
            Use a Memories node when you want to select the context for a
            particular workflow path.
          </p>
        </div>
      </div>
      <details className="memory-advanced-settings">
        <summary>
          Context limits <Icon name="caret-down" size={14} />
        </summary>
        <p>
          Recall searches locally for matching words and exact identifiers from
          your task, template, and supported event fields. It can return no
          memories when nothing is relevant. Automatic recall uses up to 8
          memories and 6,000 bytes per agent step. Selected, pinned, and
          recalled memory share a 12,000-byte limit. Selected memories are
          considered first, then pins across personal, workspace, and workflow
          scopes, then automatic recall. Entries that do not fit remain in the
          library.
        </p>
      </details>
    </div>
  );
}
