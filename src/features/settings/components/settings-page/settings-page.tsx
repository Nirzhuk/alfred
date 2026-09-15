import { useEffect, useMemo, useState } from "react";
import { confirm as confirmDialog } from "@tauri-apps/plugin-dialog";
import { SelectControl } from "../../../../components/select-control";
import { ConnectedAppsSettings } from "../../../integrations/connected-apps-settings";
import { PublicationList } from "../../../sharing";
import {
  NativeAgentSettings,
  useAgentAccountsStore,
} from "../../../agent-accounts";
import { useIntegrationsStore } from "../../../integrations/store";
import { LicenseSettings } from "../../../licensing";
import {
  showQuickAccess,
  useQuickAccessPreferences,
} from "../../../quick-access/preferences";
import {
  clearDecidedMemoryCandidates,
} from "../../../workflow/api";
import {
  defaultModelFor,
  modelsForProvider,
  type ProviderModels,
} from "../../../workflow/models";
import { useWorkflowStore } from "../../../workflow/store";
import type { AgentProviderId } from "../../../workflow/types";
import { ShortcutSettings } from "../shortcut-settings";
import {
  SETTINGS_SECTION_LABELS,
  type SettingsSectionId,
} from "../settings-sidebar";
import {
  isMacPlatform,
  NOTIFICATION_SOUND_OPTIONS,
  type NotificationSound,
  useNotificationsStore,
} from "../../notifications";
import {
  MEMORY_REVIEW_ACKNOWLEDGEMENT,
  MEMORY_REVIEW_EXPLANATION,
  canSaveMemoryReview,
  memoryReviewDraftFromSettings,
  DEFAULT_MEMORY_REVIEW_DRAFT,
  type MemoryReviewDraft,
  useMemoryReviewStore,
} from "../../memory-review";
import {
  useThemeStore,
  type ThemePreference,
} from "../../theme";

type Props = {
  activeSection: SettingsSectionId;
};

const THEME_OPTIONS: { value: ThemePreference; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function SettingsPage({ activeSection }: Props) {
  const preference = useThemeStore((s) => s.preference);
  const setPreference = useThemeStore((s) => s.setPreference);
  const quickAccessEnabled = useQuickAccessPreferences((s) => s.enabled);
  const quickAccessFullscreen = useQuickAccessPreferences(
    (s) => s.showInFullscreen,
  );
  const quickAccessMode = useQuickAccessPreferences((s) => s.mode);
  const quickAccessAlwaysOnTop = useQuickAccessPreferences(
    (s) => s.alwaysOnTop,
  );
  const quickAccessBusy = useQuickAccessPreferences((s) => s.busy);
  const setQuickAccessEnabled = useQuickAccessPreferences((s) => s.setEnabled);
  const setQuickAccessMode = useQuickAccessPreferences((s) => s.setMode);
  const setQuickAccessFullscreen = useQuickAccessPreferences(
    (s) => s.setShowInFullscreen,
  );
  const setQuickAccessAlwaysOnTop = useQuickAccessPreferences(
    (s) => s.setAlwaysOnTop,
  );
  const resetQuickAccessPosition = useQuickAccessPreferences(
    (s) => s.resetPosition,
  );

  const integrationsLoading = useIntegrationsStore((s) => s.loading);
  const refreshIntegrations = useIntegrationsStore((s) => s.refresh);
  const agentAccountsLoading = useAgentAccountsStore((s) => s.loading);
  const refreshAgentAccounts = useAgentAccountsStore((s) => s.load);

  const [shortcutHeaderActions, setShortcutHeaderActions] =
    useState<HTMLDivElement | null>(null);

  const notificationsEnabled = useNotificationsStore((s) => s.enabled);
  const notificationSound = useNotificationsStore((s) => s.sound);
  const permission = useNotificationsStore((s) => s.permission);
  const busy = useNotificationsStore((s) => s.busy);
  const setNotificationsEnabled = useNotificationsStore((s) => s.setEnabled);
  const setNotificationSound = useNotificationsStore((s) => s.setSound);
  const refreshPermission = useNotificationsStore((s) => s.refreshPermission);
  const openSystemSettings = useNotificationsStore((s) => s.openSystemSettings);
  const sendTest = useNotificationsStore((s) => s.sendTest);

  const reviewSettings = useMemoryReviewStore((s) => s.settings);
  const reviewProviders = useMemoryReviewStore((s) => s.providers);
  const reviewLoaded = useMemoryReviewStore((s) => s.loaded);
  const reviewSaving = useMemoryReviewStore((s) => s.saving);
  const reviewError = useMemoryReviewStore((s) => s.error);
  const loadReviewSettings = useMemoryReviewStore((s) => s.load);
  const saveReviewSettings = useMemoryReviewStore((s) => s.save);
  const providerModels = useWorkflowStore((s) => s.providerModels);
  const loadProviderModels = useWorkflowStore((s) => s.loadProviderModels);
  const activeWorkflowId = useWorkflowStore((s) => s.activeWorkflowId);

  const [reviewDraft, setReviewDraft] = useState<MemoryReviewDraft>(
    DEFAULT_MEMORY_REVIEW_DRAFT,
  );
  const [clearingSuggestions, setClearingSuggestions] = useState(false);
  const [clearedNote, setClearedNote] = useState<string | null>(null);

  useEffect(() => {
    void loadReviewSettings();
  }, [loadReviewSettings]);

  // Keep the draft in step with the SQLite-backed settings once loaded.
  useEffect(() => {
    if (!reviewLoaded) return;
    setReviewDraft(memoryReviewDraftFromSettings(reviewSettings));
  }, [
    reviewLoaded,
    reviewSettings.enabled,
    reviewSettings.provider,
    reviewSettings.model,
    reviewSettings.updatedAt,
  ]);

  useEffect(() => {
    void loadProviderModels();
  }, [loadProviderModels]);

  useEffect(() => {
    void refreshPermission();
    const onFocus = () => {
      void refreshPermission();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshPermission]);

  const clearSuggestionHistory = async () => {
    if (!activeWorkflowId) return;
    const confirmed = await confirmDialog(
      "Delete all decided memory suggestions for this workflow? Pending suggestions and saved memories are kept.",
      { title: "Clear suggestion history", kind: "warning" },
    );
    if (!confirmed) return;
    setClearingSuggestions(true);
    try {
      const cleared = await clearDecidedMemoryCandidates(activeWorkflowId);
      setClearedNote(
        cleared === 0
          ? "No decided suggestions to delete."
          : `Deleted ${cleared} decided suggestion${cleared === 1 ? "" : "s"}.`,
      );
    } catch {
      setClearedNote("Suggestion history could not be cleared. Try again.");
    } finally {
      setClearingSuggestions(false);
    }
  };

  const saveReviewDraft = async () => {
    if (await saveReviewSettings({ ...reviewDraft })) return;
    setReviewDraft(
      memoryReviewDraftFromSettings(useMemoryReviewStore.getState().settings),
    );
  };


  const permissionLabel =
    permission === "granted"
      ? "Allowed"
      : permission === "denied"
        ? "Not allowed"
        : "Checking…";

  return (
    <section className="settings-page" aria-label="Settings">
      <header className="settings-page-header">
        <div>
          <p className="settings-kicker">Settings</p>
          <h1>{SETTINGS_SECTION_LABELS[activeSection]}</h1>
        </div>
        {activeSection === "connected-apps" ? (
          <button
            type="button"
            className="ghost settings-header-action"
            disabled={integrationsLoading}
            onClick={() => void refreshIntegrations()}
          >
            {integrationsLoading ? "Refreshing…" : "Refresh"}
          </button>
        ) : null}
        {activeSection === "native-agents" ? (
          <button
            type="button"
            className="ghost settings-header-action"
            disabled={agentAccountsLoading}
            onClick={() => void refreshAgentAccounts()}
          >
            {agentAccountsLoading ? "Refreshing..." : "Refresh"}
          </button>
        ) : null}
        {activeSection === "shortcuts" ? (
          <div
            className="settings-page-header-actions"
            ref={setShortcutHeaderActions}
          />
        ) : null}
      </header>

      <div className="settings-page-body settings-page-panel">
        {activeSection === "general" ? (
          <section
            className="settings-section"
            aria-labelledby="theme-settings-heading"
          >
            <h2 id="theme-settings-heading">Theme</h2>
            <div className="settings-card">
              <div className="settings-row settings-row-theme">
                <div>
                  <p className="settings-label">Theme</p>
                  <p className="settings-value">
                    Match the system, or force light or dark. Transitions when
                    you change it.
                  </p>
                </div>
                <div
                  className="theme-switch"
                  role="radiogroup"
                  aria-label="Theme"
                >
                  {THEME_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      role="radio"
                      aria-checked={preference === option.value}
                      className={
                        preference === option.value
                          ? "theme-switch-option is-active"
                          : "theme-switch-option"
                      }
                      onClick={() => setPreference(option.value)}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "connected-apps" ? <ConnectedAppsSettings /> : null}

        {activeSection === "sharing" ? (
          <section className="settings-section" aria-labelledby="sharing-settings-heading">
            <h2 id="sharing-settings-heading">Published workflow links</h2>
            <PublicationList />
          </section>
        ) : null}

        {activeSection === "native-agents" ? <NativeAgentSettings /> : null}

        {activeSection === "license-billing" ? <LicenseSettings /> : null}

        {activeSection === "shortcuts" ? (
          <ShortcutSettings headerActionsContainer={shortcutHeaderActions} />
        ) : null}

        {activeSection === "quick-access" ? (
          <section
            className="settings-section"
            aria-labelledby="quick-access-settings-heading"
          >
            <h2 id="quick-access-settings-heading">Floating window</h2>
            <div className="settings-card">
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Always-ready quick access</p>
                  <p className="settings-value">
                    Keep workflows and schedules within reach while the main
                    Alfred window is hidden.
                  </p>
                </div>
                <div className="settings-controls settings-quick-access-actions">
                  <button
                    type="button"
                    className="ghost settings-test-btn"
                    disabled={quickAccessBusy}
                    onClick={() => void showQuickAccess()}
                  >
                    Open now
                  </button>
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      quickAccessEnabled ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={quickAccessEnabled}
                    aria-label="Enable always-ready quick access"
                    disabled={quickAccessBusy}
                    onClick={() =>
                      void setQuickAccessEnabled(!quickAccessEnabled)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Quick access style</p>
                  <p className="settings-value">
                    Use an invisible hover corner or a movable compact workflow
                    window.
                  </p>
                </div>
                <div
                  className="theme-switch"
                  role="radiogroup"
                  aria-label="Quick access style"
                >
                  <button
                    type="button"
                    role="radio"
                    aria-checked={quickAccessMode === "hover"}
                    className={
                      quickAccessMode === "hover"
                        ? "theme-switch-option is-active"
                        : "theme-switch-option"
                    }
                    disabled={quickAccessBusy || !quickAccessEnabled}
                    onClick={() => void setQuickAccessMode("hover")}
                  >
                    Hover corner
                  </button>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={quickAccessMode === "compact"}
                    className={
                      quickAccessMode === "compact"
                        ? "theme-switch-option is-active"
                        : "theme-switch-option"
                    }
                    disabled={quickAccessBusy || !quickAccessEnabled}
                    onClick={() => void setQuickAccessMode("compact")}
                  >
                    Compact window
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Always above other windows</p>
                  <p className="settings-value">
                    Keep the compact launcher above your other apps.
                  </p>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      quickAccessAlwaysOnTop ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={quickAccessAlwaysOnTop}
                    aria-label="Keep compact quick access above other windows"
                    disabled={
                      quickAccessBusy ||
                      !quickAccessEnabled ||
                      quickAccessMode !== "compact"
                    }
                    onClick={() =>
                      void setQuickAccessAlwaysOnTop(!quickAccessAlwaysOnTop)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Show on full-screen desktops</p>
                  <p className="settings-value">
                    Keep your chosen quick-access style available over
                    full-screen apps and across virtual desktops.
                  </p>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      quickAccessFullscreen ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={quickAccessFullscreen}
                    aria-label="Show quick access on full-screen desktops"
                    disabled={quickAccessBusy || !quickAccessEnabled}
                    onClick={() =>
                      void setQuickAccessFullscreen(!quickAccessFullscreen)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Compact window position</p>
                  <p className="settings-value">
                    Drag the grip beside Alfred to move it. Its position is
                    restored when Alfred starts again.
                  </p>
                </div>
                <button
                  type="button"
                  className="ghost settings-test-btn"
                  disabled={
                    quickAccessBusy ||
                    !quickAccessEnabled ||
                    quickAccessMode !== "compact"
                  }
                  onClick={() => void resetQuickAccessPosition()}
                >
                  Restore default
                </button>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "general" ? (
          <section
            className="settings-section"
            aria-labelledby="general-settings-heading"
          >
            <h2 id="general-settings-heading">Application behavior</h2>
            <div className="settings-card">
              <div className="settings-row">
                <div>
                  <p className="settings-label">Background operation</p>
                  <p className="settings-value">
                    Closing the window hides Alfred. Schedules keep running
                    until you choose Quit from the menu bar or system tray.
                  </p>
                </div>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">App</p>
                  <p className="settings-value">Alfred</p>
                </div>
                <span className="settings-meta">v0.5.0</span>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">Platform</p>
                  <p className="settings-value">
                    Desktop only (macOS, Linux, Windows)
                  </p>
                </div>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">Working directories</p>
                  <p className="settings-value">
                    Set a folder on each workflow so agent CLIs run in that
                    project path.
                  </p>
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "notifications" ? (
          <section
            className="settings-section"
            aria-labelledby="notification-settings-heading"
          >
            <h2 id="notification-settings-heading">Run notifications</h2>
            <div className="settings-card">
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Notifications</p>
                  <p className="settings-value">
                    Notify when a run finishes while the window is in the
                    background.
                  </p>
                  <p className="settings-hint">
                    macOS permission: {permissionLabel}
                    {permission === "denied" && isMacPlatform() ? (
                      <>
                        {" · "}
                        <button
                          type="button"
                          className="settings-link"
                          onClick={() => void openSystemSettings()}
                        >
                          Open System Settings
                        </button>
                      </>
                    ) : null}
                  </p>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      notificationsEnabled ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={notificationsEnabled}
                    aria-label="Enable notifications"
                    disabled={busy}
                    onClick={() =>
                      void setNotificationsEnabled(!notificationsEnabled)
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Notification sound</p>
                  <p className="settings-value">
                    Used for finished runs, failures, and desktop Notify nodes.
                  </p>
                </div>
                <div className="settings-sound-controls">
                  <SelectControl
                    containerClassName="settings-sound-select"
                    aria-label="Notification sound"
                    value={notificationSound}
                    disabled={!notificationsEnabled || busy}
                    onChange={(event) =>
                      void setNotificationSound(
                        event.currentTarget.value as NotificationSound,
                      )
                    }
                  >
                    {NOTIFICATION_SOUND_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </SelectControl>
                  <button
                    type="button"
                    className="ghost settings-test-btn"
                    disabled={
                      !notificationsEnabled || permission !== "granted" || busy
                    }
                    onClick={() => void sendTest()}
                  >
                    Preview
                  </button>
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "memory-review" ? (
          <section
            className="settings-section"
            aria-labelledby="memory-review-settings-heading"
          >
            <h2 id="memory-review-settings-heading">Memory review</h2>
            <div className="settings-card">
              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Suggest memories after runs</p>
                  <p className="settings-value">
                    After an eligible completed run, the agent CLI you choose
                    may propose memory changes for your approval. Off by
                    default, per workflow.
                  </p>
                  <ul className="settings-explanation">
                    {MEMORY_REVIEW_EXPLANATION.map((point) => (
                      <li key={point}>{point}</li>
                    ))}
                  </ul>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className={[
                      "settings-toggle",
                      reviewDraft.enabled ? "is-on" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    role="switch"
                    aria-checked={reviewDraft.enabled}
                    aria-label="Enable memory review globally"
                    disabled={reviewSaving}
                    onClick={() =>
                      setReviewDraft((draft) => ({
                        ...draft,
                        enabled: !draft.enabled,
                        acknowledged: reviewSettings.enabled,
                      }))
                    }
                  >
                    <span className="settings-toggle-knob" />
                  </button>
                </div>
              </div>

              <div className="settings-row settings-row-control">
                <div>
                  <p className="settings-label">Reviewer provider</p>
                  <p className="settings-value">
                    Which installed agent CLI reviews runs. It must already be
                    signed in; Alfred never stores credentials.
                  </p>
                </div>
                <div className="settings-controls">
                  <SelectControl
                    containerClassName="settings-sound-select"
                    aria-label="Reviewer provider"
                    value={reviewDraft.provider ?? ""}
                    disabled={!reviewDraft.enabled || reviewSaving}
                    onChange={(event) => {
                      const provider = event.currentTarget.value || null;
                      setReviewDraft((draft) => ({
                        ...draft,
                        provider,
                        model: null,
                        acknowledged: reviewSettings.enabled,
                      }));
                    }}
                  >
                    <option value="">Select a provider…</option>
                    {reviewProviders.map((provider) => (
                      <option key={provider.id} value={provider.id}>
                        {provider.label}
                      </option>
                    ))}
                  </SelectControl>
                </div>
              </div>

              {reviewDraft.provider === reviewSettings.provider && reviewSettings.availability ? (
                <div className="settings-row">
                  <div>
                    <p className="settings-label">Restricted review availability</p>
                    <p className="settings-value" role="status">
                      {reviewSettings.availability.supported
                        ? `Available: ${reviewSettings.availability.product ?? reviewDraft.provider} ${reviewSettings.availability.runtimeVersion ?? ""}`
                        : `Unavailable: ${reviewSettings.availability.reason?.replace(/_/g, " ") ?? "restriction capability is unverified"}. No provider call will be made.`}
                    </p>
                  </div>
                </div>
              ) : null}

              <MemoryReviewModelRow
                provider={reviewDraft.provider}
                model={reviewDraft.model}
                providerModels={providerModels}
                disabled={!reviewDraft.enabled || reviewSaving}
                onChange={(model) =>
                  setReviewDraft((draft) => ({ ...draft, model }))
                }
              />

              {reviewDraft.enabled && !reviewSettings.enabled ? (
                <label className="settings-acknowledgement">
                  <input
                    type="checkbox"
                    checked={reviewDraft.acknowledged}
                    onChange={(event) => {
                      const acknowledged = event.currentTarget.checked;
                      setReviewDraft((draft) => ({
                        ...draft,
                        acknowledged,
                      }));
                    }}
                  />
                  <span>{MEMORY_REVIEW_ACKNOWLEDGEMENT}</span>
                </label>
              ) : null}

              <div className="settings-row settings-row-control">
                <div>
                  {reviewError ? (
                    <p className="settings-value" role="alert">
                      {reviewError}
                    </p>
                  ) : null}
                  <p className="settings-hint">
                    If a review fails, History shows a stable reason such as{" "}
                    <code>auth_required</code> or <code>timeout</code> with
                    retry guidance. Failures never change run status or output.
                  </p>
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className="primary"
                    disabled={
                      !canSaveMemoryReview(reviewDraft) || reviewSaving
                    }
                    onClick={() => void saveReviewDraft()}
                  >
                    {reviewSaving ? "Saving…" : "Save"}
                  </button>
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeSection === "data" ? (
          <section
            className="settings-section"
            aria-labelledby="data-settings-heading"
          >
            <h2 id="data-settings-heading">Local data</h2>
            <div className="settings-card">
              <div className="settings-row">
                <div>
                  <p className="settings-label">Storage</p>
                  <p className="settings-value">
                    Workflows, memories, schedules, and run history are stored
                    locally in SQLite on this machine.
                  </p>
                </div>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">Linked memories</p>
                  <p className="settings-value">
                    Memories linked from other workflows stay owned by their
                    source workflow. Unlinking only removes the reference.
                  </p>
                </div>
              </div>
              <div className="settings-row">
                <div>
                  <p className="settings-label">Suggestion history</p>
                  <p className="settings-value">
                    Approved, rejected, and blocked memory suggestions stay for
                    audit until you delete them. Pending suggestions and
                    canonical memories are never touched.
                  </p>
                  {clearedNote ? (
                    <p className="settings-hint" role="status">
                      {clearedNote}
                    </p>
                  ) : null}
                </div>
                <div className="settings-controls">
                  <button
                    type="button"
                    className="ghost settings-test-btn"
                    disabled={!activeWorkflowId || clearingSuggestions}
                    onClick={() => void clearSuggestionHistory()}
                  >
                    Clear decided suggestions
                  </button>
                </div>
              </div>
            </div>
          </section>
        ) : null}
      </div>
    </section>
  );
}

type MemoryReviewModelRowProps = {
  provider: string | null;
  model: string | null;
  providerModels: ProviderModels[];
  disabled: boolean;
  onChange: (model: string | null) => void;
};

/**
 * Optional reviewer model. Providers with a fixed catalog get a select;
 * free-form CLIs get a text field, mirroring the agent node settings.
 */
function MemoryReviewModelRow({
  provider,
  model,
  providerModels,
  disabled,
  onChange,
}: MemoryReviewModelRowProps) {
  const catalog = useMemo(
    () =>
      provider
        ? modelsForProvider(providerModels, provider as AgentProviderId)
        : undefined,
    [provider, providerModels],
  );
  if (!provider) return null;

  if (catalog?.allowCustom) {
    return (
      <div className="settings-row settings-row-control">
        <div>
          <p className="settings-label">Reviewer model</p>
          <p className="settings-value">
            Optional. Leave empty to use the CLI&apos;s default model.
          </p>
        </div>
        <div className="settings-controls">
          <label className="field">
            <span>Model</span>
            <input
              type="text"
              aria-label="Reviewer model"
              placeholder={defaultModelFor(providerModels, provider as AgentProviderId)}
              value={model ?? ""}
              disabled={disabled}
              onChange={(event) => onChange(event.currentTarget.value || null)}
            />
          </label>
        </div>
      </div>
    );
  }

  return (
    <div className="settings-row settings-row-control">
      <div>
        <p className="settings-label">Reviewer model</p>
        <p className="settings-value">
          Optional. Leave empty to use the CLI&apos;s default model.
        </p>
      </div>
      <div className="settings-controls">
        <SelectControl
          containerClassName="settings-sound-select"
          aria-label="Reviewer model"
          value={model ?? ""}
          disabled={disabled}
          onChange={(event) => onChange(event.currentTarget.value || null)}
        >
          <option value="">Provider default</option>
          {(catalog?.models ?? []).map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </SelectControl>
      </div>
    </div>
  );
}
