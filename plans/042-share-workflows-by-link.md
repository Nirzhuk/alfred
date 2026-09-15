# Plan 042: Share workflow templates by link

## Status and scope

- Status: IN PROGRESS. High review and scoped browser checks accepted the local backend, setup, and public preview. Shared integration tests, packaged verification, and hosted release gates remain; no deployment has been performed.
- Priority: P1. Effort: L, spanning the desktop, a small hosted service, and packaged desktop verification.
- Requested on: 2026-09-08.
- Planning baseline: desktop commit `85f0d64`, with unrelated uncommitted canvas, identity-node, runner, style, and menu changes. Preserve them. Recheck the working tree before implementation.
- Depends on: existing workflow persistence, Connected Apps descriptors/selectors, agent capability discovery, and OS credential storage.
- Independent of: Plan 011's OAuth/webhook relay and the licensing API. Do not revive the rejected custom licensing service to host this feature.

Build the experience the user selected:

**Share workflow → review reusable template → create HTTPS link → recipient opens the page → Open in Alfred → choose local settings → create a workflow copy.**

The first release uses links, a publishing service, and one reusable preview page. It requires no publisher or recipient account signup. It includes copying an HTTPS link into Alfred as a fallback when browser-to-app opening is unavailable. File export is not a prerequisite.

Schedule nodes, a public gallery, ratings, paid templates, author profiles, cloud execution, live collaboration, and automatic template updates are separate work. Existing schedule/trigger configuration stays in place. A template can include disabled start settings without moving them onto the canvas.

## Intended result

Someone shares a flow that summarizes selected notes and sends an update to Slack. The recipient sees its steps and requirements, selects their own notes source, agent, Slack connection, and channel, then creates an editable local copy. The publisher's accounts, machine paths, memories, and previous results never become the recipient's configuration.

Successful import does not run the flow, contact its action destinations, install a skill, or enable a trigger. The recipient starts the first run through Alfred's normal Run control after reviewing the configured flow.

## What exists today

| Area | Observed implementation | Consequence for this plan |
| --- | --- | --- |
| Saved workflows | `src/features/workflow/types.ts`, `src-tauri/src/db/workflows.rs` store a graph plus local working directory and folder identity | A saved workflow is not a portable template; introduce a separate contract |
| Node data | Prompts, identity instructions, inline commands, HTTP headers, paths, memory IDs, account references, and app connection/resource IDs | Export needs explicit field handling and a preview of the exact publication |
| Connected Apps | `src-tauri/src/integrations/actions.rs` declares fields, required scopes, resource selectors, and interpolation support | Reuse these descriptors for setup; add portability metadata where field intent is insufficient |
| Schedule/trigger data | Separate persistence and APIs in `src-tauri/src/db/` and `src/features/workflow/api.ts` | Read these separately during export and create new disabled records during import |
| Memory default | `Db::create_workflow` currently enables automatic retrieval | The import transaction must explicitly start with recall off; the recipient can opt in |
| App lifecycle | `src-tauri/src/lib.rs` already has a single-instance plugin that reveals the main window | Integrate cold and warm deep links without breaking tray, quick access, or existing window behavior |
| Link handling | No sharing protocol is registered in `tauri.conf.json`; no sharing import/export API was found | Build and verify these paths; do not assume the existing app can receive a link |
| UI | Workflow context menu, canvas controls, modal components, and feature CSS already exist | Add Share and Import from link through those patterns |

Read `docs/design-system.md` before implementing UI. Keep new styling in the sharing feature rather than expanding the shared stylesheet.

## Product decisions for the first release

1. Links are **unlisted, accessible to anyone who has the link**. They are not private invitations. State this beside Create link. The hosting service receives the reviewed template content.
2. A link identifies one immutable publication. Editing a local flow does not edit its shared copy. Publishing changed content creates a new link.
3. Imported flows are independent local copies. Preserve their source link and content digest for reference, without fetching or applying future changes automatically.
4. The publishing installation can revoke a link without signup. Revocation prevents future retrieval; it cannot remove copies already imported or downloaded by other people.
5. No creator identity is verified in v1. Omit author badges and account/profile UI.
6. Templates include requirements, not installed dependencies. Detect missing agents, unavailable harnesses, skills, interpreters, and connector capabilities during setup. Never switch accounts, providers, harnesses, or billing products silently.
7. Start settings import disabled. Use the existing schedule/trigger controls to enable them after setup, respecting existing entitlements and the requirement that Alfred stays open.
8. Keep publishing out of the execution path. Existing workflows and previously imported copies remain usable if the sharing service is unavailable.

## Desktop publishing experience

Add **Share…** to the workflow context menu and a discoverable action in the workflow toolbar. It opens a large dialog with three steps:

1. **Describe.** Name, short description, and a read-only graph preview. Read the saved graph only after the current editor save has completed. Do not publish an older version while newer edits are visible.
2. **Make reusable.** Suggest setup fields for accounts, channels, recipients, repositories, local paths, and selected documents. The publisher chooses which reusable text/settings to retain. Group repeated uses of the same local connection into one setup question while preserving intentionally separate connections.
3. **Review and share.** Show the final public description, graph, prompts, identity instructions, scripts/commands, destinations, requirements, and start settings. List removed or replaced values. Create link publishes exactly this reviewed snapshot.

Example setup fields:

| Source value | Public setup label | Recipient control |
| --- | --- | --- |
| Local Slack connection ID | Slack account | Compatible connected-account picker |
| Channel ID and display cache | Update channel | Live channel selector for the chosen account |
| Local notes path | Notes folder | Native folder picker |
| Native agent account reference | Agent account | Compatible local account picker |
| Email recipient | Send updates to | Address input unless the publisher deliberately retains a public example |

After publication, show Copy link and Open preview. A workflow's Share dialog also lists publications created from that workflow with Copy link and Revoke link. Keep publication management accessible from settings if the original workflow is deleted. Deleting a local workflow does not revoke its links automatically.

While preparing a template, edits apply to the publication draft only. Closing the dialog leaves the original workflow unchanged. Disable duplicate submission; retry an ambiguous upload using the same durable publication request ID.

## Recipient experience

The HTTPS page contains the name, description, node/edge preview, required apps and tools, and an **Open in Alfred** button. It also offers Copy link and Download Alfred. Use the existing official installer destination for Download Alfred.

Opening a link in Alfred enters a setup dialog:

1. **Preview.** Fetch and validate the template, show its graph and executable content, and explain its required actions. No workflow is created yet.
2. **Set up.** Resolve only missing/local inputs. Accounts first, then resources from those accounts. Permit different accounts for intentionally separate roles. Offer existing connection setup when necessary, then return to this dialog.
3. **Create workflow.** Validate again in Rust and atomically create a fresh workflow plus its disabled start settings and provenance. Reveal the imported canvas with an ordinary Run action.

Do not persist an executable workflow with unresolved placeholders in v1. The user may leave the setup dialog open, or cancel and reopen the same link. Setup values stay local and never return to the sharing service. Cancellation creates no workflow or schedule; an account deliberately connected during setup remains connected.

Opening a second link while setup is active must not discard entered values. Queue a bounded number of pending links and offer them after the current setup closes. Duplicate delivery of the same link must not create duplicate dialogs or workflows. A later deliberate import of the same template may create another copy, with an existing-copy notice.

## Portable template contract

Add a versioned JSON contract and shared acceptance fixtures under `contracts/workflow-template/`. Rust is authoritative for desktop export/import; the service independently validates uploaded documents against the same contract and fixtures.

The envelope contains:

- `format: "alfred.workflow-template"`, `schemaVersion: 1`, and `minAlfredVersion`.
- Public `title` and `description`.
- `graph`: allow-listed node data, edges, finite positions, and template-local IDs.
- `parameters`: typed setup requirements with stable keys and explicit allowed targets.
- `requirements`: agent/harness/model suggestions, skill names, interpreter names, and app action/event identifiers with compatibility metadata.
- `starts`: optional schedule, file, app-event, or webhook setup recipes. These are configuration recipes, not live subscriptions.

Use typed field references rather than global text replacement. A parameter binds to an allowed node field, start setting, or working-directory field. It cannot write arbitrary JSON paths or prototype keys. Do not confuse setup parameters with the runner's existing `{{output}}`, `{{context}}`, and `{{cwd}}` expressions. V1 setup can replace complete supported fields; arbitrary substitutions inside shell commands or scripts are outside scope.

Suggested initial limits: 256 KiB UTF-8 serialized template, 100 nodes, 300 edges, 64 setup parameters, 16 start recipes, JSON depth 16, 120-character title, and 2,000-character description. Apply the same limits before upload, in the service, and while fetching/importing. Reject oversize documents rather than truncating executable content.

Validate unique IDs, allowed node types and fields, existing edge endpoints, supported handles, finite coordinates, valid parameter references, and an acyclic graph compatible with the current runner. Unknown schema versions, node types, or execution-affecting fields produce an actionable compatibility error; do not drop them or generate a partial flow.

### Export rules by data category

| Category | Handling |
| --- | --- |
| Workflow/folder/run IDs, timestamps, runtime output, traces, viewport selection | Exclude; retain only template-local node/edge identity and useful layout |
| Prompt, identity, template, output, and notification text | Retain only in the reviewed publication; render as text during review |
| Working directory, attachments, file-inject paths, write paths, file-backed scripts | Replace with recipient setup fields; do not upload referenced file content |
| Agent account references | Remove and create local account requirements; preserve provider/harness intent |
| Skills and interpreters | Declare requirements by name/capability; do not bundle skills, install tools, or read arbitrary local skill files |
| Memory nodes | Replace memory IDs with local memory-selection requirements; exclude memory bodies, history, and suggested memories |
| App actions | Remove connection IDs, account metadata, and resource display caches. Bind resource selectors to recipient choices. Require portability rules for every input field |
| HTTP and webhook nodes | Allow credential-free HTTP with explicitly classified headers and recipient-selected destinations. Auth headers, cookies, and secret-bearing webhook URLs require an existing credential-backed action equivalent or an explicit unsupported-node error |
| Inline scripts, shell commands, custom agents, and runnable input scripts | Preserve only as explicit reviewed executable text; list required interpreter/platform and local path dependencies. Never execute during preview/setup |
| Start recipes | Remove trigger IDs, secrets, external subscriptions, cursors, receipts, and health state. Create fresh disabled local records; webhook endpoints/secrets are generated locally |
| Existing cron schedules | Preserve the actual time basis, currently UTC in `scheduler/mod.rs`, and show it explicitly. Do not label the existing cron as the publisher's local timezone. Named-timezone scheduling is separate work |

Use explicit per-node adapters and descriptor-backed action/event portability metadata. The current generic field kind does not distinguish all private recipient/address/resource values, so add that information rather than guessing from input keys. Unknown actions/events or unclassified fields block publication until a portability rule exists. Include the in-progress identity node if it exists at execution time; do not undo its implementation.

Setup is not a new generic secret-entry system. Do not ask recipients to paste tokens into graph fields. For unsupported authenticated HTTP/webhook actions, identify the node and let the publisher replace it with a supported Connected Apps action in the source workflow before sharing. Never remove an unsupported action and publish a flow with different behavior.

Run a best-effort local scan of free text for known credential formats and machine-specific paths. Identified issues require editing or parameterization before publishing. State in the review UI that automatic checks cannot recognize every private detail; the exact outgoing content remains inspectable. Export must never access credential values merely to populate a template.

The review snapshot gets a digest. Publish that snapshot, not a fresh read of a graph that may have changed. Once published, the service stores and serves the accepted canonical bytes and a SHA-256 digest. Web preview and desktop import refer to this same immutable document. The digest identifies content; it is not proof of author identity.

## Hosted service and the small web page

Recommended implementation: a standalone TypeScript Cloudflare Worker with D1 and bundled static assets, in `share-service/` inside the desktop repository. Keep its package, migrations, configuration, and tests independent of the Tauri frontend and the sibling licensing project.

Workers can serve static assets alongside Worker code, and D1's documented row limit accommodates the proposed 256 KiB template bound. These are implementation recommendations, not a claim that hosting has been configured or that a particular paid plan is required. See [Workers static assets](https://developers.cloudflare.com/workers/static-assets/) and [D1 limits](https://developers.cloudflare.com/d1/platform/limits/).

Production URLs use an operator-configured origin. `https://<share-host>/s/<shareId>` below is a placeholder, not an existing domain.

| Endpoint | Behavior |
| --- | --- |
| `POST /api/v1/shares` | Validate and store an immutable template; accept a publication request ID and owner bearer capability; return share ID, HTTPS URL, and digest |
| `GET /api/v1/shares/:id` | Return the bounded versioned template and digest, or a stable unavailable response |
| `DELETE /api/v1/shares/:id` | Authenticate the owner capability, revoke idempotently, and remove the published payload |
| `GET /s/:id` | Render the reusable preview page for that publication |

Each share ID contains at least 128 random bits. Use an independent 256-bit owner capability generated in Rust and saved to the OS credential store before the first upload. It never appears in a share URL, page, React state, or logs. The server stores only its hash. Scope the durable publication request ID to that hash so retries return the same publication after an interrupted response; changed payload under the same request ID returns conflict. The local database retains redacted publication metadata and its opaque credential reference.

There is no account recovery in v1. If the owner credential is lost, the installation cannot revoke the share through self-service. Document this in link management. Revocation retains only the minimal tombstone and owner hash needed for idempotent deletion; remove content from read paths immediately and disclose the finite backup-retention policy.

Start with configurable upload/body limits, create/read rate limits, and a global publication/storage cap. Add a disable-publishing switch that preserves reads and local imports. IP limits are only one abuse control, not proof of identity. Before public availability, exercise concurrent quota enforcement, establish the operator's takedown path, and provide a visible support/report link on previews. Avoid a general listing endpoint, browser uploads, third-party scripts, tracking pixels, arbitrary remote asset fetching, or executing/rendering submitted HTML.

Use server-owned preview components with escaped text and an allow-listed graph renderer. Link unfurls should show reviewed title/description only, never raw prompts, code, or resource identifiers. Return restrictive CSP, `Referrer-Policy: no-referrer`, and `X-Robots-Tag: noindex, nofollow`. Unlisted links are not access control. Use `Cache-Control: no-store` for template and preview responses so revocation is not undermined by a long public cache lifetime.

## Opening Alfred and the link fallback

Register the dedicated scheme `alfred-workflows://import/<shareId>`. Avoid the broad `alfred://` name. The page invokes the scheme only when the user clicks Open in Alfred.

The link contains only the validated share ID. It cannot provide an arbitrary download URL, command, working directory, account selection, or run flag. Rust constructs the download URL from the configured trusted HTTPS origin. Reject redirects outside that origin, credentials in URLs, malformed IDs, unexpected ports/routes/query parameters, and unexpected document types. Development HTTP origins are explicit development-only configuration and must not ship in release builds.

Add the Tauri deep-link plugin, register the scheme statically for packaged apps, and enable the existing single-instance plugin's `deep-link` feature. Move its registration to the documented first-plugin position while preserving its current window-reveal behavior. Handle both initial launch URLs and links received while running; retain pending links until the main UI is ready. Tauri documents these lifecycle and platform requirements in [Deep Linking](https://v2.tauri.app/plugin/deep-linking/).

The page cannot reliably detect installation. Keep Open in Alfred, Copy link, Download Alfred, and brief instructions visible instead of claiming a timeout proves the app is absent. On mobile, explain that Alfred is a desktop app and offer Copy link for use on a computer.

Add **Import from link…** inside Alfred. Pasting the canonical HTTPS link enters the same parser, fetch, validation, and setup flow as a deep link. This is the complete non-file fallback for browser restrictions and old installers. An installed version without the importer needs an update; downloading Alfred does not automatically resume the browser's link, so the page instructs the user to reopen or paste it after installation.

## Implementation boundaries

Suggested modules, relative to the desktop repository:

| Location | Responsibility |
| --- | --- |
| `contracts/workflow-template/` | Versioned schema, node/action portability contract, and shared valid/invalid fixtures |
| `src-tauri/src/templates/` | Export adapters, import validation, parameter resolution, compatibility, and sanitized preview DTOs |
| `src-tauri/src/sharing/` | Trusted-origin client, publication owner storage, upload/revoke retries, and link parsing/lifecycle |
| `src-tauri/src/db/template_shares.rs` | Local publication/provenance metadata and atomic import helpers |
| `src/features/sharing/` | Share dialog, publication management, preview, setup wizard, link import, and styles |
| `share-service/` | Worker endpoints, D1 migrations, minimal page/assets, local development, and deployment runbook |

Expose narrow commands for preparing a publication, publishing a reviewed draft, listing/revoking local publications, resolving a shared link, checking setup bindings, and committing an import. Keep network requests and ownership credentials in Rust. Do not let the web page invoke Tauri or let setup instantiate live node components that fetch private data or run actions.

Use a dedicated import transaction rather than calling several existing create/upsert commands in sequence. Generate new workflow/node/edge/start IDs; remap all references; explicitly set memory retrieval and background memory review off; resolve recipient values; validate required capabilities; insert disabled starts; and store source ID/digest in the same transaction. Persist a local import-attempt ID to prevent duplicate creation after a double click or an interrupted response.

Backend validation must recheck selected connection identity, scope/capability, resource access, local paths, and supported harness at commit time. Capability checks may fetch metadata, but must never send the flow's messages, run scripts, create events, or fire a sample trigger. Disconnected or unavailable providers keep setup incomplete with an actionable message.

No plan-wide runner rewrite is expected. Ordinary execution continues through the existing runner after import. If an unsupported template construct would require new execution semantics, reject it explicitly and handle that change in a separate plan.

## Ordered delivery steps

### 1. Freeze the portable contract and export rules

- Inventory all current node types and action/event fields, including changes made after the planning baseline.
- Implement the schema, Rust model, export adapters, portability metadata, setup references, and shared fixtures.
- Cover local IDs/paths/credentials, free-text review, legacy prompt/output/skill fields, unknown fields, graph validation, and limits.
- Deliver an exact sanitized preview without uploading anything.
- Exit when every supported construct either exports with documented setup requirements or produces a specific unsupported-field error. Nothing is silently omitted.

### 2. Implement local setup and transactional import

- Build the recipient preview/setup dialog using validated local fixtures first.
- Resolve connections before dependent resources; detect unavailable tools and unsupported providers without substitution.
- Implement fresh-ID remapping, disabled start creation, memory defaults, provenance, and idempotent atomic commit.
- Exit when two isolated test profiles can use the same template with different accounts/paths and no publisher-specific references survive.

### 3. Build the service and preview page locally

- Scaffold `share-service/`, D1 schema, request validation, immutable storage, owner authentication, retry semantics, limits, revocation, and escaped preview rendering.
- Add deterministic local service commands: `bun run dev`, `bun run test`, and `bun run check` in that package. No production deployment is needed for this step.
- Use the same contract fixtures as the desktop and exercise requests against the local Worker/D1 runtime.
- Exit when publish → read → preview → revoke works locally, with equivalent Rust/service validation results.

### 4. Connect desktop publishing and link import

- Add Share entry points, reusable-field editing, the exact outgoing preview, Copy link, publication management, and actionable upload errors.
- Add the trusted-origin Rust client and Import from link; use the local service from Step 3.
- Exit when publisher profile A creates a link and recipient profile B configures a new copy without file exchange.

### 5. Add and verify Open in Alfred

- Integrate the deep-link plugin and existing single-instance handling; add the user-clicked page button and desktop/mobile fallback copy.
- Verify cold launch, warm launch, tray-hidden app, UI startup races, duplicate delivery, and active import preservation.
- Smoke packaged macOS, Windows, and Linux installers. Development browser rendering alone does not establish desktop scheme registration.
- Exit when direct opening and pasted-link import reach the same validated setup flow on each supported platform. Keep paste fallback visible where a package/browser needs it.

### 6. Release the link-sharing experience

- Prepare the production origin, isolated service configuration, quota settings, rollback/disable controls, content/backup retention statement, and abuse-contact route.
- Add `docs/workflow-sharing.md`, service deployment instructions, and packaged acceptance evidence.
- Supply two small example templates as fixtures for evaluation, not a public gallery dependency.
- Release only after the end-to-end acceptance cases below pass. Keep desktop publishing unavailable until its configured service is healthy; local workflows remain unaffected.

## Acceptance and verification

Use automated tests for the meaningful data, execution, and lifecycle boundaries. Use isolated profiles and local fixture providers for actions that would otherwise contact real people.

| Case | Required result |
| --- | --- |
| Share a note-summary-to-Slack flow | Recipient selects their own source/account/channel, creates a copy, and can execute it against a fixture provider |
| Schedule/app/file/webhook starts | Start intent is visible, all imported starts are disabled, fresh webhook identities are generated, and no run is enqueued |
| Secrets and local references | Seed known credential/path/account/resource/memory fixtures across supported nodes; outgoing data contains none of the excluded values |
| Inline code and malicious preview text | Code remains visible as text, external image/HTML/JS payloads do not execute or make preview network requests |
| Missing connection/model/skill | Specific setup requirement; no silent replacement and no partially runnable workflow |
| Broken or future template | Unsupported version/type/field, dangling edge, cycle, size/depth violation, or invalid binding rejects before persistence |
| Failure during import | Roll back workflow, start, and provenance writes together; retry cannot create duplicate copies |
| Upload timeout or restart | Reusing the durable request ID returns the same link; a different payload under that ID is rejected |
| Revocation | Wrong owner cannot revoke; repeated owner deletion is safe; all subsequent page/API reads are unavailable; existing local copies still work |
| App launch lifecycle | Cold/warm/tray opening reaches one setup dialog and preserves current canvas and other in-progress setup |
| Browser cannot launch app | Copy link → Import from link completes without downloading or transferring a file |
| Service unavailable | Useful retry state; existing local workflows and imported copies continue to run |
| Public-service limits | Concurrent create requests cannot bypass storage caps; rejected requests do not leave payloads or leaked credentials in logs |

Run focused Rust/frontend/service tests during their steps, then the existing desktop `bun run check`, service `bun run check`, and `git diff --check` after integration. Reconcile any baseline failures against the current working tree; do not weaken existing tests to accommodate the feature. Record packaged link-opening evidence separately from unit tests.

## Remaining release configuration

These details do not block local implementation: the production hostname and hosting account, the live quota/storage budget, the abuse/support destination, and the published backup-retention wording. The Worker/D1 choice and no-signup ownership model are proposed defaults in this plan, not resources that have already been provisioned.

The completed feature is a working shared-link journey with recipient setup and revocation. A gallery, public author system, schedule-node redesign, and cloud workflow execution are not required to call this plan done.
