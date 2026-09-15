# Plan 044: Track follow-ups and prepare replies for review

## Status and scope

- Status: TODO. Planning only; no connector extension, background tracking, model evaluation, or UI implementation has been performed.
- Priority: P1. Effort: XL, delivered in ordered slices.
- Requested on: 2026-09-08.
- Baseline: desktop commit `85f0d64` with unrelated uncommitted workflow/identity/canvas work. Preserve it and inspect current state before execution.
- Dependencies: Connected Apps credentials/actions, a selected-thread Slack reader built here, durable local jobs, an explicitly configured text-analysis adapter, and the small review Inbox built here.
- Independent of: [schedule nodes](043-schedule-node-on-canvas.md), [link sharing](042-share-workflows-by-link.md), and Plan 011's cloud relay for the private/local Slack release.

Build a persistent request tracker. A user selects one Slack thread, identifies the person they are waiting for, states the expected outcome, and chooses when to follow up. Alfred monitors that thread while open, distinguishes acknowledgment from fulfillment, tracks who owes the next action, and prepares an editable follow-up for the user to send.

This plan includes the first Inbox implementation because no general Action Inbox exists today. It initially contains follow-up drafts only. A broad cross-app inbox, automatic discovery of commitments across an account, Gmail/Outlook tracking, cross-channel identity matching, automatic sending, and cloud monitoring are separate work.

## Concrete acceptance story

The user sends Sarah a homepage preview in Slack and requests approval. In Alfred they choose Track follow-up, paste that thread's link, select Sarah, enter "Approval of the revised homepage", and choose Thursday at 09:00 for follow-up.

| Event | Alfred's behavior |
| --- | --- |
| Sarah: "Thanks, I'll look tomorrow" | Keep Waiting for Sarah; show that she acknowledged the request |
| Sarah: "Please shorten the headline" | Move to Needs me with the message as evidence; suppress a pending nudge |
| User revises the page and replies | Offer Return to waiting; the user confirms the change and next follow-up time |
| Thursday arrives without approval | Refresh the thread, then prepare one draft in the Inbox |
| User edits and clicks Send | Recheck current evidence and send the reviewed reply through the configured Slack app |
| Sarah: "Looks good, approved" | Resolve when the evidence satisfies the conservative rules below; make any unsent draft obsolete |
| Sarah: "Looks better" | Request the user's judgment; do not assume approval |

The current Slack sender is a bot/app connection. Show **Send as [connected Slack app]** before sending. Do not imply that the reply will be posted as the human user. Sending as the signed-in human requires a separate supported user-auth capability and is outside this first release.

## Product decisions

1. Tracking begins only from an explicit user action. No mailbox or workspace scan discovers requests automatically.
2. One item tracks one outcome, one selected conversation, and one primary target Slack user ID. Display names are labels, not identity. Multiple tracked outcomes can share a thread, but require separately confirmed items.
3. A deadline passing creates work for review. It never authorizes a message send.
4. Acknowledgment, requested changes, refusal, and fulfillment have different meanings. A new message alone does not close an item.
5. Clear fulfillment may resolve an item automatically only with complete current evidence and a verified classifier path. Show the evidence and provide Undo. Ambiguous evidence asks the user.
6. The selected agent/provider/model is configured explicitly for analysis and drafting. Explain that tracked conversation excerpts are sent to that provider and that repeated analysis uses the user's provider allowance. No provider or billing fallback is automatic.
7. Monitoring works while Alfred is open, including tray mode. After restart, refresh before evaluating overdue reminders. Display last successful check and any coverage gap.
8. Manual state changes remain usable when Slack/model access is unavailable. They do not imply that monitoring or automatic interpretation is working.

## UI and entry points

Add two sidebar destinations using the existing view/navigation pattern:

- **Follow-ups** lists persistent requests in Needs me, Waiting, and Resolved views, with paused/stopped filters. Each row shows the outcome, person, source, follow-up time, and last checked state.
- **Inbox** lists prepared drafts. Selecting a draft opens its detail pane with source context, editable text, the sending identity/destination, and Send, Snooze, and Dismiss controls.

These are full workspace views beside the existing sidebar. Preserve the workflow canvas position/selection when switching. Do not put a large review overlay over ongoing canvas work. Badges should count the appropriate items, not repeatedly notify for the same state transition.

### Creating a tracked request

**New follow-up** opens a form with:

- Slack connection and pasted message/thread link.
- A fetched, bounded preview confirming the actual workspace, channel, and root thread.
- Target person, chosen by stable Slack user identity from accessible thread participants. Resolve display names through an existing or narrowly added profile capability only when necessary.
- Expected outcome in plain text, distinct from the follow-up message itself.
- Follow up at: an explicit date/time and timezone.
- Analysis/drafting provider settings, inherited from Follow-ups settings only after the user has configured them.

Also add Track follow-up to eligible Slack reply/send results or app-mention details already visible in Alfred. Do not add a Slash command or Slack-installed message shortcut as an unmentioned dependency. Natural-language deadline extraction can prefill the form later; v1 always shows and confirms an exact timestamp.

One-time follow-up times store a UTC instant plus the chosen IANA timezone for editing/display. Use the existing `chrono-tz` dependency for conversion. Reject nonexistent DST wall times and ask the user to select the intended occurrence for ambiguous ones. This is a one-time reminder and does not require changing cron scheduling.

### Detail and state actions

Show a short evidence timeline, the original request, the latest relevant response, source links, next follow-up time, and last successful sync. Provide Mark resolved, Return to waiting, Snooze, Pause monitoring, Stop tracking, and Delete with distinct behavior.

When a human reply appears to answer a requested change, suggest Return to waiting instead of inferring ownership from a display name. The user can confirm that they completed their part even if their response happened outside the thread.

Pause preserves the request and suppresses checks/drafts until resumed. Stop is terminal until explicitly resumed. Delete removes the local request, evidence, and drafts; it does not delete Slack messages. A resolved item stays visible in Resolved and can be reopened. Dismissing a draft does not resolve the underlying request.

## State model

Keep responsibility, monitoring health, and draft delivery as separate fields. A rate limit is not a business outcome.

| Dimension | Values / meaning |
| --- | --- |
| Request status | `waiting`, `needs_me`, `resolved`, `stopped` |
| Monitoring state | `active`, `paused`, `reconnect_required`, `rate_limited`, `incomplete`, `source_unavailable` |
| Evidence interpretation | `acknowledged`, `action_requested`, `fulfilled`, `declined`, `unclear`, `no_relevant_change` |
| Draft state | `ready`, `sending`, `sent`, `dismissed`, `obsolete`, `failed`, `delivery_unknown` |

Transitions:

- `acknowledged` from the tracked person keeps `waiting`.
- `action_requested` moves to `needs_me` with cited evidence and invalidates outstanding drafts.
- `declined` moves to `needs_me` for a decision. It is not successful fulfillment and must not cause repeated nudges by default.
- `fulfilled` can move to `resolved` only under the evidence policy; invalidate unsent drafts in the same transaction.
- `unclear` presents a review question and suppresses a due send proposal until answered. Do not rewrite the user's expected outcome.
- An elapsed follow-up time creates a draft only when the item is still `waiting`, monitoring is current, and no unresolved interpretation is pending.
- Sending a draft leaves the item `waiting`. Clear the consumed follow-up time; keep monitoring replies and let the user choose another follow-up time. Do not silently start recurring nudges.
- Snooze changes the follow-up instant, increments the due revision, and obsoletes the current draft. Dismiss consumes that draft's due revision so the next poll does not recreate it.
- Editing the outcome, target, source, or due time increments the item revision and invalidates dependent analysis/drafts. Old results cannot apply to the new version.
- User decisions take precedence over in-flight model results. Apply analysis through a conditional revision check.

## Slack connector work

The current connector supports send/reply actions and `app_mention` over Socket Mode. It does not read a thread, and it intentionally ignores edited/bot mention events. A new selected-thread read path is a required feature dependency; an ordinary mention trigger cannot implement this tracker.

Start with public channels and explicitly enabled private channels that the connected app can actually access. Add opt-in history scopes and connection capability checks. Existing connections retain their current permissions until the user enables thread tracking and completes the applicable reinstall/reconnect. Do not require or introduce pasted user tokens merely to bypass a failed bot capability.

The current Slack `conversations.replies` documentation lists history scopes for bot and user tokens and describes cursor pagination. Verify the intended token/channel combination with an owned test workspace before enabling it in a build. App distribution affects read limits, so do not assume internal-app limits apply to a publicly distributed installation. [Slack thread API](https://docs.slack.dev/reference/methods/conversations.replies/)

The thread adapter must:

- Parse allow-listed Slack permalink forms as identifiers, never fetch an arbitrary supplied URL. Validate workspace binding, channel ID, message timestamp, and resolved parent thread.
- Fetch only the selected thread. Avoid broad channel-history search or document/attachment downloads.
- Preserve Slack timestamps as exact strings or exact decimal values, never floating-point numbers used as identities.
- Normalize messages to stable author ID, timestamp, edited/deleted status where observable, bounded text, and source permalink.
- Validate read access at creation, refresh, and before sending. Scope loss, archive/deletion, and incomplete data are visible monitoring states.
- Ignore the connected app's own follow-up posts as evidence of fulfillment while recording them in the timeline. Other bots cannot impersonate the selected human target.
- Reconnect only to the matching workspace/connection identity. Rebinding an item to another workspace requires an explicit new source selection.

Use a dedicated bounded thread snapshot contract. Do not expand `NormalizedAppEvent` to store full thread bodies or change its existing external-data limits. The tracker owns its narrow evidence storage and feeds only relevant, bounded text into analysis.

### Reading, freshness, edits, and restart recovery

Use selected-thread polling for the initial implementation, with a proposed five-minute desired interval. Add a shared per-app/workspace/method request budget across trackers and existing Slack reads. Honor `Retry-After`; Slack applies Web API limits per method/workspace. [Slack rate limits](https://docs.slack.dev/apis/web-api/rate-limits/)

Begin with a cap of 25 active tracked threads per connection, fair scheduling, and priority for due items and explicit pre-send refreshes. This cap is a product limit to test, not a claim that every installation can refresh all 25 within five minutes. Display delays honestly under tighter provider limits, and tune the cap against the actual supported distribution mode.

Deduplicate reads for items sharing a thread. Paginate completely to a captured cutoff before calling a snapshot complete. Suggested bounds are 200 messages and 64 KiB normalized text per thread, with an explicit incomplete result if exceeded. Incomplete, truncated, stale, or partially paginated evidence cannot prove silence or fulfillment. Offer manual review or a narrower/new thread instead of guessing.

Refresh the bounded selected thread rather than reading only messages newer than the last timestamp; older approvals can be edited or removed. Compare message IDs/content revisions between complete snapshots. Never interpret absence from one page as deletion. If later complete evidence contradicts or removes a cited automatic resolution, reopen for user review. Do not overwrite a user-confirmed resolution automatically.

Continuous observation of resolved items is bounded: keep automatically resolved items under monitoring for 24 hours, then stop polling them. Record the last check and whether resolution was automatic or user-confirmed. Undo or Reopen restarts tracking. Do not promise to discover edits after monitoring stops.

Slack message events include edit/delete subtypes; a future event-based optimization must handle these rather than copying the mention adapter's ignore rules. Events do not replace the complete refresh needed after gaps. [Slack message events](https://docs.slack.dev/reference/events/message/)

Use durable `next_check_at` jobs with an atomic claim, lease expiry, and bounded concurrency. No database lock is held across network/model calls. On restart, recover interrupted read/analysis jobs and refresh overdue items once. A sleeping/closed machine does not perform checks, and missed checks must not produce a burst of duplicate reminders.

## Interpretation and drafting

Introduce a dedicated `FollowUpAnalyzer` interface with fixture and production adapters. Reuse existing agent/account/capability selection patterns, but do not copy the memory-review invocation unchanged: the current reviewer calls a normal agent adapter with a working directory and does not establish a tool-free execution boundary.

The production analysis route must enforce text-only operation: no filesystem execution, shell, browser, MCP, tools, injected project instructions, attached skills, automatic memory retrieval, or ambient workspace access. Use a supported provider-specific mode and verify it, not merely a prompt saying "do not use tools". Implement and verify one explicit provider/harness route first; expose only verified combinations. If none is available, retain manual tracking and drafting controls but do not call the automated portion of this plan complete.

Inputs contain the user-confirmed outcome, selected target identity, latest complete bounded thread snapshot, relevant previous state, and due time. Thread text is untrusted evidence. It cannot change the destination, select another account, authorize a send, request tools, or change the tracker instructions.

Require strict structured output containing interpretation, existing evidence message IDs, a short rationale, and optional draft text. Reject unknown fields, fabricated evidence IDs, output outside bounds, invalid identity references, and instructions masquerading as status. Keep model output out of canonical state until backend validation succeeds. Do not use a numeric model confidence score alone to decide completion.

Automatic fulfillment requires all of the following:

- A complete recent snapshot for the exact item revision.
- Evidence from the explicitly selected target person.
- A clear statement satisfying the specified outcome, not an acknowledgment, quotation, hypothetical approval, conditional approval with outstanding work, or unrelated approval.
- No later contradictory evidence in the snapshot.
- A supported analysis adapter that passes the curated regression cases below.

If any condition is uncertain, create a resolution question for the user. Automatic resolution is reversible and visibly attributed to its evidence; it is not guaranteed semantic certainty.

Coalesce message bursts and analyze only a changed evidence digest or an explicit due/draft request. Proposed defaults: at most one analysis per item per minute, 10 calls per item per day, and 100 calls per installation per day, with visible usage counts and adjustable limits. Due drafts share the same budget. On exhaustion, show Analysis paused and allow manual handling. Do not run unbounded repair/retry loops or retry through a different provider.

Drafts must stay within the tracked topic and known facts. Do not invent progress, deadlines, or promises. The model supplies text only; the backend binds sender, workspace, channel, and thread from the reviewed item. A plain user-written draft is available when model drafting is disabled or unavailable.

## Inbox and delivery contract

Implement a narrow `review_actions`/outbox service reusable by later Inbox features. V1 supports only a follow-up Slack thread reply and must not expose arbitrary executable payloads supplied by models.

One ready draft exists per item/outcome revision/due revision. Store its evidence digest, exact body, body revision, originating item revision, sender connection, fixed destination, and creation time. Manual edits increment body revision; regeneration creates a new version without overwriting unsaved edits.

Send operates as follows:

1. User clicks Send for a displayed draft revision and sending identity.
2. Claim that revision, then perform a fresh complete thread read within the shared provider budget. If access, pagination, or rate limits prevent a fresh check, retain the draft and explain that sending through this control is waiting for verification.
3. If new relevant messages, a resolution, or item/draft edits invalidate it, return to review. Never silently rewrite and send a different body.
4. Conditionally transition to `sending`, persist an attempt record, and call the existing generic Slack reply action with the fixed destination and exact approved text.
5. On confirmed success, save Slack message identity/permalink, mark sent, update the item, and consume the due revision in one local transaction.

Serialize sends per item, reject duplicate clicks/stale revisions, and cancel an unsubmitted attempt if the item is paused/resolved/stopped. There is no atomic transaction spanning Slack and SQLite: a reply can arrive after the last refresh, and a process can crash after Slack accepts a message. Document that the check reduces stale sends rather than guaranteeing their impossibility.

On an ambiguous timeout or restart from `sending`, mark `delivery_unknown`. Never blind-retry. Show Open thread and a way to reconcile whether the message was sent; record a confirmed message ID when available. A positively rejected request can be retried through the existing action policy, but it must not be confused with unknown delivery. Do not claim exactly-once delivery from a local uniqueness constraint.

Snooze and Dismiss never send a message. Making a draft obsolete preserves its local explanation while removing it from the actionable Inbox. Notifications contain minimal text, respect app notification preferences, and deduplicate by state transition; reading a notification never counts as approval to send.

## Persistence and module boundaries

Suggested tables, each with migrations and appropriate foreign keys/indexes:

| Table | Purpose |
| --- | --- |
| `follow_up_items` | Outcome, target/source identity, responsibility status, monitoring state, due instant/timezone, revisions, and lifecycle timestamps |
| `follow_up_threads` / `follow_up_messages` | Shared tracked-thread identity, complete-snapshot metadata, bounded normalized evidence, and reference counts |
| `follow_up_events` | User/model transitions, evidence references, and undo provenance |
| `follow_up_jobs` | Durable refresh/analysis work, deduplication keys, claims, retries, leases, and stable errors |
| `review_actions` / `review_action_attempts` | Editable draft revisions, fixed destinations, delivery state, and ambiguous-send reconciliation |

Do not store these records as ordinary memories or keep a workflow run open for days. Monitoring jobs have their own lifecycle. An originating workflow/run ID is optional provenance only; deleting that workflow does not silently stop tracking a request the user created separately.

Suggested modules:

- `src-tauri/src/follow_ups/`: state transitions, jobs, thread adapter orchestration, interpretation policy, and commands.
- `src-tauri/src/review_actions/`: draft lifecycle and delivery coordination over existing `ActionRegistry`.
- `src-tauri/src/integrations/slack.rs` or a split `slack/` module: selected-thread read capability, scopes, normalization, and existing send adapter integration.
- `src-tauri/src/db/`: item/evidence/job/outbox persistence and migrations.
- `src/features/follow-ups/`: list/detail/create views, state actions, settings, and scoped state.
- `src/features/inbox/`: draft list/detail/editor and send/reconciliation controls.
- Existing sidebar/App routing: add the two destinations without changing workflow or canvas ownership.

Connections and tokens remain in the existing credential system. Store only bounded selected-thread text and stable identifiers locally; never raw provider payloads, token headers, attachments, or workspace-wide indexes. Do not send tracked content to Plan 042's sharing service or export live follow-up records in templates.

On Delete, cascade local item evidence/drafts/jobs, preserving a shared snapshot only while another item references it. Propose 30-day retention for stopped/resolved content, configurable by the user. After expiry, retain only a minimal status/source record and require a fresh fetch on reopen. Disconnect stops reads and sends; follow the app's explicit local-data deletion controls. Logs and telemetry carry stable codes/counts, not message bodies or participant names.

## Ordered delivery steps

### 1. Build the durable request model and manual Follow-ups UI

- Freeze state transitions, due-time conversion, revisions, retention, and persistence.
- Build create/list/detail/state actions with fixture sources and manual status control.
- Include the Sarah approval scenario and distinguish acknowledgment, requested work, refusal, ambiguity, and fulfillment in fixtures.
- Exit when restart, snooze, pause, stop, delete, reopen, and concurrent edits preserve the user's request without duplicate work.

### 2. Extend Slack for selected-thread access

- Add opt-in history capability, connection upgrade guidance, permalink parsing, thread reads, pagination, shared rate budgets, evidence normalization, and access checks.
- Verify the supported bot/channel modes against an owned test workspace. Use fixture providers for automated tests and never send a test message to a real colleague.
- Update `docs/slack.md` and Plan 012 references to distinguish this narrow capability from deferred broad history search/DM support.
- Exit when complete snapshots and edits/deletions/gaps are handled without pretending incomplete data proves no reply.

### 3. Add monitoring jobs and bounded interpretation

- Implement app-lifecycle scheduling, atomic job claims, crash recovery, read deduplication, and fair due-item refresh.
- Add one verified text-only model route, explicit settings, structured output validation, usage bounds, and regression evaluation.
- Apply policy-driven transitions conditionally on current revisions and evidence.
- Exit when the tracker follows the acceptance story with real authorized thread reads and a verified analysis route, including conservative ambiguity handling.

### 4. Build Inbox drafts and reviewed sending

- Implement draft creation/editing/regeneration/snooze/dismissal, the full-page Inbox UI, and notification deduplication.
- Add fresh-read preflight, fixed destinations, sender identity, conditional claims, confirmed sends, and unknown-delivery recovery.
- Exit when a due request produces one reviewable draft and only an explicit Send action can publish the exact reviewed text.

### 5. Integrate entry points and complete release verification

- Add Track follow-up to supported source/result views, Follow-ups settings, last-check indicators, and reconnect/review guidance.
- Verify packaged Tauri behavior across restart, tray, sleep/wake, app shutdown, and existing active workflow runs.
- Write `docs/follow-ups.md`, update Connected Apps scope descriptions, and record integration evidence and current Slack distribution constraints.
- Complete full checks and the end-to-end matrix below before enabling automatic interpretation or reviewed sending in the supported build.

## Acceptance and evaluation matrix

| Case | Required result |
| --- | --- |
| Sarah acknowledges | Still waiting; no false resolution |
| Sarah asks for changes | Needs me with evidence; old follow-up drafts obsolete |
| User posts an update | Return-to-waiting suggestion; no false identity inference |
| Sarah clearly approves | Conservative resolution with evidence and Undo; unsent drafts obsolete |
| Conditional/quoted/unrelated approval or another person says approved | Ask for review or remain open; never auto-resolve |
| Negative response | Needs me for a decision; no recurring pressure to approve |
| Due while app was closed | Refresh on reopen, then at most one current draft |
| Thread exceeds bounds / page fails / scope revoked | Visible incomplete/unavailable state; no inference of silence |
| Evidence edited/deleted | Invalidate dependent analysis/drafts; review automatic resolution when still monitored |
| Same thread tracked for two outcomes | Shared read, separate outcome interpretation and drafts |
| User changes outcome/target while model runs | Stale result discarded |
| Model outage/budget exhaustion/malformed output | Manual actions remain available; no state corruption or cross-provider retry |
| Malicious thread instructions | No tools, account changes, destination changes, or automatic send |
| Draft edited or new reply before Send | Recheck and return changed context to review; never send stale approved revision |
| Reply arrives just after preflight | Record actual send/result honestly; do not claim an impossible cross-provider atomic guarantee |
| Double click / response lost / restart during send | One local attempt; confirmed sent or delivery unknown, never blind duplicate |
| Pause/stop/delete with in-flight work | Cancellation/revision checks prevent new drafts or unsubmitted sends |
| Read or notification opens | No message sent |
| Bot/app sending identity | Preview and actual author match the chosen connected app |
| Delete/retention/revoke | Correct local cleanup and no orphan worker; no Slack message deletion |

Create a curated evaluation set of at least 40 short threads covering the positive and adversarial cases above, including multiple outcomes and replies in supported languages. Require zero false automatic resolutions on the negative/ambiguous fixtures, complete evidence-ID validation, and explicit manual-review behavior for unsupported/incomplete cases. Record adapter/model versions and observed results; this is a release regression gate, not a general accuracy guarantee.

Run focused state/job/outbox/connector tests with injected clocks and fixture providers, then `bun run check` and `git diff --check`. Manually verify the full workflow in the native Tauri app, including both Inbox and Follow-ups views, keyboard use, draft preservation across navigation, and both themes. A manual-reminder-only slice is useful progress but is not completion of the automatic tracking and reviewed-send experience specified here.
