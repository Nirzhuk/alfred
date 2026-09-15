PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS workflow_folders (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflows (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  working_directory TEXT NOT NULL DEFAULT '',
  folder_id TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  memory_retrieval_enabled INTEGER NOT NULL DEFAULT 0,
  memory_lookup_enabled INTEGER NOT NULL DEFAULT 0,
  graph_json TEXT NOT NULL DEFAULT '{"nodes":[],"edges":[]}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agents (
  id TEXT PRIMARY KEY NOT NULL,
  provider TEXT NOT NULL CHECK (provider IN ('claude_code', 'cursor', 'codex', 'opencode', 'github_copilot', 'gemini', 'grok', 'pi', 'omp')),
  name TEXT NOT NULL,
  config_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- ponytail: no CHECK on trigger_kind — every new trigger source would need a
-- table rebuild (SQLite can't ALTER a CHECK). Values come from RunTrigger.
CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  trigger_kind TEXT NOT NULL DEFAULT 'manual',
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed', 'cancelled')),
  payload_json TEXT,
  error TEXT,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run_steps (
  id TEXT PRIMARY KEY NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  agent_provider TEXT,
  skill_name TEXT,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed', 'skipped')),
  input_json TEXT NOT NULL DEFAULT '{}',
  output_json TEXT NOT NULL DEFAULT '{}',
  error TEXT,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL
);

-- ponytail: no CHECK on on_missed — a new policy value would need a table
-- rebuild (SQLite can't ALTER a CHECK). Values come from the `OnMissed` enum:
-- 'run' catches up on open, 'skip' drops the missed run, 'ask' waits for the user.
CREATE TABLE IF NOT EXISTS schedules (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  cron TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  next_run_at TEXT,
  on_missed TEXT NOT NULL DEFAULT 'run',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Event triggers: one workflow can have many (unlike schedules, which are 1:1).
-- `source` is free-form so new sources need no migration; `config_json` holds
-- the source-specific shape (file: {path, pattern, debounceMs}).
CREATE TABLE IF NOT EXISTS triggers (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  label TEXT NOT NULL DEFAULT '',
  config_json TEXT NOT NULL DEFAULT '{}',
  secret TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_fired_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memories (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT REFERENCES workflows(id) ON DELETE SET NULL,
  run_id TEXT,
  node_id TEXT,
  scope_type TEXT NOT NULL DEFAULT 'workflow'
    CHECK (scope_type IN ('user', 'workspace', 'workflow')),
  scope_key TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'text'
    CHECK (kind IN ('text', 'note', 'artifact')),
  memory_type TEXT NOT NULL DEFAULT 'output'
    CHECK (memory_type IN (
      'preference', 'fact', 'decision', 'constraint', 'lesson', 'episode',
      'checkpoint', 'note', 'output', 'artifact'
    )),
  source TEXT NOT NULL DEFAULT 'run'
    CHECK (source IN ('run', 'manual', 'import', 'review')),
  provenance_state TEXT NOT NULL DEFAULT 'legacy_unknown'
    CHECK (provenance_state IN ('legacy_unknown','manual','evidence_verified','source_removed')),
  evidence_subject TEXT
    CHECK (evidence_subject IS NULL OR evidence_subject IN ('user_statement','workflow_configuration','recorded_observation')),
  evidence_is_inference INTEGER NOT NULL DEFAULT 0
    CHECK (evidence_is_inference IN (0, 1)),
  title TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  artifact_path TEXT,
  artifact_content_digest TEXT,
  pinned INTEGER NOT NULL DEFAULT 0,
  confidence REAL NOT NULL DEFAULT 1.0 CHECK (confidence >= 0 AND confidence <= 1),
  salience INTEGER NOT NULL DEFAULT 50 CHECK (salience >= 0 AND salience <= 100),
  status TEXT NOT NULL DEFAULT 'active'
    CHECK (status IN ('active', 'superseded', 'retracted')),
  semantic_revision INTEGER NOT NULL DEFAULT 1 CHECK (semantic_revision >= 1),
  supersedes_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
  fact_key TEXT,
  observed_at TEXT,
  last_confirmed_at TEXT,
  valid_from TEXT,
  expires_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (scope_type = 'workflow' AND workflow_id IS NOT NULL AND scope_key = workflow_id)
    OR (scope_type = 'workspace' AND length(trim(scope_key)) > 0)
    OR (scope_type = 'user' AND scope_key = 'local-user')
  )
);

-- Canonical artifact verification metadata. Writers populate the complete
-- fixed-size manifest while the body is already in memory. Legacy rows are
-- explicit and remain unavailable to bounded lookup until reindexed safely.
CREATE TABLE IF NOT EXISTS artifact_chunk_manifests (
  memory_id TEXT PRIMARY KEY NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  content_digest TEXT NOT NULL,
  digest_version INTEGER NOT NULL,
  byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
  chunk_size INTEGER NOT NULL CHECK (chunk_size > 0),
  chunk_count INTEGER NOT NULL CHECK (chunk_count >= 0),
  verification_state TEXT NOT NULL CHECK (
    verification_state IN ('verified', 'legacy_unverified')
  ),
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS artifact_chunk_digests (
  memory_id TEXT NOT NULL REFERENCES artifact_chunk_manifests(memory_id) ON DELETE CASCADE,
  chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
  digest TEXT NOT NULL CHECK (length(digest) = 64),
  PRIMARY KEY (memory_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_artifact_chunk_digests_memory
  ON artifact_chunk_digests(memory_id, chunk_index);

CREATE TABLE IF NOT EXISTS output_receipts (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  step_id TEXT NOT NULL UNIQUE REFERENCES run_steps(id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  save_requested INTEGER NOT NULL CHECK (save_requested IN (0, 1)),
  pin_memory INTEGER NOT NULL CHECK (pin_memory IN (0, 1)),
  memory_title_override TEXT NOT NULL,
  resolved_memory_title TEXT NOT NULL,
  as_final_result INTEGER NOT NULL CHECK (as_final_result IN (0, 1)),
  html_report INTEGER NOT NULL CHECK (html_report IN (0, 1)),
  include_files_changed INTEGER NOT NULL CHECK (include_files_changed IN (0, 1)),
  state TEXT NOT NULL CHECK (state IN (
    'not_requested', 'skipped_empty', 'pending', 'saving', 'saved', 'failed', 'deleted'
  )),
  storage_id TEXT UNIQUE,
  output_sha256 TEXT,
  artifact_required INTEGER NOT NULL DEFAULT 0 CHECK (artifact_required IN (0, 1)),
  memory_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
  error_code TEXT CHECK (error_code IS NULL OR error_code IN (
    'invalid_disposition', 'step_output_unavailable', 'artifact_stage_io',
    'artifact_finalize_io', 'artifact_content_mismatch', 'database_write', 'internal'
  )),
  saving_started_at TEXT,
  claim_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  PRIMARY KEY (run_id, node_id),
  CHECK (
    (state = 'not_requested' AND save_requested = 0 AND storage_id IS NULL
      AND output_sha256 IS NULL AND memory_id IS NULL AND error_code IS NULL)
    OR (state = 'skipped_empty' AND save_requested = 1 AND storage_id IS NULL
      AND output_sha256 IS NULL AND memory_id IS NULL AND error_code IS NULL)
    OR (state IN ('pending', 'saving') AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NULL AND error_code IS NULL)
    OR (state = 'saved' AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NOT NULL AND memory_id = storage_id AND error_code IS NULL)
    OR (state = 'failed' AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NULL AND error_code IS NOT NULL)
    OR (state = 'deleted' AND save_requested = 1 AND storage_id IS NOT NULL
      AND output_sha256 IS NOT NULL AND length(output_sha256) = 64
      AND memory_id IS NULL AND error_code IS NULL)
  ),
  CHECK ((state = 'deleted') = (deleted_at IS NOT NULL)),
  CHECK (
    (state = 'saving' AND saving_started_at IS NOT NULL AND claim_id IS NOT NULL)
    OR (state != 'saving' AND saving_started_at IS NULL AND claim_id IS NULL)
  )
);
CREATE INDEX IF NOT EXISTS idx_output_receipts_recovery
  ON output_receipts(state, saving_started_at, updated_at);
CREATE INDEX IF NOT EXISTS idx_output_receipts_memory_id
  ON output_receipts(memory_id) WHERE memory_id IS NOT NULL;

-- Durable cleanup intents for output final/staging artifacts. Rows remain
-- until checked removal (or a retained-canonical decision) is acknowledged.
CREATE TABLE IF NOT EXISTS output_cleanup_journal (
  storage_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL DEFAULT '',
  generation INTEGER NOT NULL CHECK (generation > 0),
  remove_final INTEGER NOT NULL CHECK (remove_final IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (storage_id, attempt_id)
);
CREATE INDEX IF NOT EXISTS idx_output_cleanup_journal_updated
  ON output_cleanup_journal(updated_at, storage_id, attempt_id);

-- Cross-workflow memory links: consumer workflow → memory owned elsewhere.
CREATE TABLE IF NOT EXISTS memory_links (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  UNIQUE (workflow_id, memory_id)
);

CREATE TABLE IF NOT EXISTS run_memory_uses (
  id TEXT PRIMARY KEY NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  rank INTEGER NOT NULL,
  score REAL NOT NULL,
  reason TEXT NOT NULL CHECK (reason IN ('explicit', 'lexical', 'recent', 'pinned')),
  rendered_bytes INTEGER NOT NULL,
  ranking_version TEXT NOT NULL DEFAULT 'lexical_v1',
  created_at TEXT NOT NULL,
  UNIQUE (run_id, node_id, memory_id)
);

CREATE TABLE IF NOT EXISTS run_memory_context_summaries (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  budget_bytes INTEGER NOT NULL,
  memory_bytes INTEGER NOT NULL,
  included_count INTEGER NOT NULL,
  explicit_omitted_count INTEGER NOT NULL,
  pinned_omitted_count INTEGER NOT NULL,
  automatic_omitted_count INTEGER NOT NULL,
  unavailable_count INTEGER NOT NULL,
  repeated_count INTEGER NOT NULL,
  trace_available INTEGER NOT NULL CHECK (trace_available IN (0, 1)),
  created_at TEXT NOT NULL,
  PRIMARY KEY (run_id, node_id)
);

-- Body-free durable lookup traces. A trace may be recorded before the
-- runner's step UUID exists; insert_step associates it in the same commit.
-- Source rows intentionally do not foreign-key source_id: deletion preserves
-- the historical identity/state without retaining body or path material.
CREATE TABLE IF NOT EXISTS memory_lookup_traces (
  id TEXT PRIMARY KEY NOT NULL,
  turn_id TEXT NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  step_id TEXT REFERENCES run_steps(id) ON DELETE CASCADE,
  request_id TEXT NOT NULL,
  tool TEXT NOT NULL CHECK (
    tool IN ('memory.search', 'memory.get', 'history.search', 'history.get')
  ),
  retrieved_bytes INTEGER NOT NULL CHECK (retrieved_bytes >= 0),
  returned_bytes INTEGER NOT NULL CHECK (returned_bytes >= 0),
  classification TEXT NOT NULL CHECK (classification IN ('retrieved_data')),
  delivery TEXT NOT NULL CHECK (
    delivery IN (
      'delivered',
      'rejected_native_output_limit',
      'rejected_context_budget',
      'rejected_invalid_json',
      'rejected_cancelled',
      'rejected_timeout'
    )
  ),
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_lookup_traces_run_node
  ON memory_lookup_traces(run_id, node_id, created_at);
CREATE INDEX IF NOT EXISTS idx_memory_lookup_traces_step
  ON memory_lookup_traces(step_id);

CREATE TABLE IF NOT EXISTS memory_lookup_trace_sources (
  trace_id TEXT NOT NULL REFERENCES memory_lookup_traces(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  reference TEXT NOT NULL,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('memory', 'run_step', 'evidence')),
  source_id TEXT NOT NULL,
  version TEXT NOT NULL,
  state TEXT NOT NULL CHECK (
    state IN (
      'current', 'historical', 'incomplete', 'unavailable', 'deleted',
      'expired', 'retracted', 'replaced', 'unlinked'
    )
  ),
  page INTEGER CHECK (page IS NULL OR page >= 0),
  PRIMARY KEY (trace_id, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_memory_lookup_trace_sources_source
  ON memory_lookup_trace_sources(source_kind, source_id);

CREATE TABLE IF NOT EXISTS schema_meta (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
);

-- Disposable local search indexes. Canonical text remains in memories and
-- run_steps; these tables can be cleared and rebuilt at any time.
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
  memory_id UNINDEXED,
  workflow_id UNINDEXED,
  title,
  body,
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE IF NOT EXISTS run_step_fts USING fts5(
  step_id UNINDEXED,
  run_id UNINDEXED,
  workflow_id UNINDEXED,
  node_id UNINDEXED,
  input_text,
  output_text,
  error_text,
  tokenize = 'unicode61 remove_diacritics 2'
);

-- Connected-app metadata only. OAuth credentials live in the OS credential
-- store and are addressed by the opaque `credential_ref`.
CREATE TABLE IF NOT EXISTS app_connections (
  id TEXT PRIMARY KEY NOT NULL,
  provider_id TEXT NOT NULL,
  display_name TEXT,
  external_account_id TEXT,
  external_tenant_id TEXT,
  connection_mode TEXT NOT NULL,
  identity_key TEXT NOT NULL,
  scopes_json TEXT NOT NULL DEFAULT '[]',
  provider_metadata_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'connected' CHECK (status IN ('connected', 'expired', 'error', 'revoked')),
  expires_at TEXT,
  last_checked_at TEXT,
  last_error_code TEXT,
  credential_ref TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Native-agent account metadata only. Provider/product/runtime/billing are
-- independent. Product and runtime ids stay as TEXT and Rust registries
-- validate them so adding a product does not require a SQL enum migration.
CREATE TABLE IF NOT EXISTS agent_accounts (
  id TEXT PRIMARY KEY NOT NULL,
  provider_id TEXT NOT NULL,
  product_id TEXT NOT NULL,
  harness TEXT NOT NULL,
  identity_key TEXT NOT NULL,
  display_name TEXT,
  external_account_id TEXT,
  external_workspace_id TEXT,
  auth_method TEXT NOT NULL,
  custody_mode TEXT NOT NULL,
  managed_runtime_id TEXT,
  managed_runtime_version TEXT,
  runtime_profile_ref TEXT UNIQUE,
  scopes_json TEXT NOT NULL DEFAULT '[]',
  billing_source TEXT NOT NULL,
  billing_owner TEXT NOT NULL,
  entitlement_state TEXT NOT NULL DEFAULT 'unknown'
    CHECK (entitlement_state IN ('unknown', 'eligible', 'limited', 'exhausted', 'ineligible')),
  entitlement_source TEXT NOT NULL,
  entitlement_observed_at TEXT,
  status TEXT NOT NULL DEFAULT 'error' CHECK (status IN ('connected', 'expired', 'error', 'revoked', 'disconnect_pending')),
  expires_at TEXT,
  last_checked_at TEXT,
  last_error_code TEXT,
  credential_ref TEXT UNIQUE,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK ((managed_runtime_id IS NULL) = (managed_runtime_version IS NULL)),
  CHECK (runtime_profile_ref IS NULL OR managed_runtime_id IS NOT NULL),
  CHECK (runtime_profile_ref IS NOT NULL OR credential_ref IS NOT NULL)
);

-- Migration-only credential references stay reachable until the account
-- service deletes the corresponding legacy OS credential. This table has no
-- foreign key because the agent_accounts contract is rebuilt in place.
CREATE TABLE IF NOT EXISTS agent_account_credential_cleanup (
  account_id TEXT PRIMARY KEY NOT NULL,
  credential_ref TEXT NOT NULL UNIQUE,
  cleanup_owner TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- Provider-neutral app event delivery. Normalized payloads are bounded and
-- minimized before entering this queue; provider bodies and headers never do.
CREATE TABLE IF NOT EXISTS app_trigger_state (
  trigger_id TEXT PRIMARY KEY NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  cursor TEXT,
  subscription_id TEXT,
  expires_at TEXT,
  last_polled_at TEXT,
  last_success_at TEXT,
  last_error_code TEXT,
  next_attempt_at TEXT,
  retry_count INTEGER NOT NULL DEFAULT 0,
  overrun_count INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_event_receipts (
  trigger_id TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  external_event_id TEXT NOT NULL,
  received_at TEXT NOT NULL,
  disposition TEXT NOT NULL CHECK (disposition IN ('queued', 'enqueued', 'dropped_overrun', 'rejected_invalid')),
  run_id TEXT,
  reason_code TEXT,
  PRIMARY KEY (trigger_id, external_event_id)
);

CREATE TABLE IF NOT EXISTS app_event_queue (
  id TEXT PRIMARY KEY NOT NULL,
  trigger_id TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  external_event_id TEXT NOT NULL,
  normalized_event_json TEXT NOT NULL,
  enqueued_at TEXT NOT NULL,
  started_at TEXT,
  UNIQUE (trigger_id, external_event_id)
);

-- Post-run memory review: explicit opt-in settings (singleton), one review job
-- per run, and model-proposed memory candidates that never touch canonical
-- memories until a user approves them.
CREATE TABLE IF NOT EXISTS memory_review_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  enabled INTEGER NOT NULL DEFAULT 0,
  provider TEXT,
  model TEXT,
  max_candidates INTEGER NOT NULL DEFAULT 5 CHECK (max_candidates BETWEEN 1 AND 5),
  updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO memory_review_settings (id, enabled, provider, model, max_candidates, updated_at)
VALUES (1, 0, NULL, NULL, 5, '1970-01-01T00:00:00Z');

CREATE TABLE IF NOT EXISTS workflow_memory_review (
  workflow_id TEXT PRIMARY KEY REFERENCES workflows(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

-- Review job metadata only. Raw provider errors, prompts, responses, and run
-- transcripts never enter this table; failures carry stable codes alone.
CREATE TABLE IF NOT EXISTS memory_reviews (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (
    status IN ('pending','running','completed','failed','skipped')
  ),
  provider TEXT NOT NULL,
  provider_product TEXT,
  execution_mode TEXT,
  capability_version TEXT,
  restriction_contract_version TEXT,
  model TEXT,
  error_code TEXT,
  candidate_count INTEGER NOT NULL DEFAULT 0,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_candidates (
  id TEXT PRIMARY KEY NOT NULL,
  review_run_id TEXT NOT NULL REFERENCES memory_reviews(run_id) ON DELETE CASCADE,
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  source_node_id TEXT,
  operation TEXT NOT NULL CHECK (operation IN ('create','supersede','retract')),
  target_memory_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
  scope_type TEXT NOT NULL CHECK (scope_type IN ('user','workspace','workflow')),
  scope_key TEXT NOT NULL,
  memory_type TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  rationale TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  candidate_revision INTEGER NOT NULL DEFAULT 1 CHECK (candidate_revision >= 1),
  expected_target_memory_id TEXT,
  expected_target_semantic_revision INTEGER,
  expected_target_content_digest TEXT,
  target_complete INTEGER CHECK (target_complete IS NULL OR target_complete IN (0, 1)),
  fact_key TEXT,
  observed_at TEXT,
  last_confirmed_at TEXT,
  valid_from TEXT,
  expires_at TEXT,
  possible_conflicts_json TEXT NOT NULL DEFAULT '[]',
  curation_contract_version INTEGER NOT NULL DEFAULT 0,
  evidence_state TEXT NOT NULL DEFAULT 'legacy_unavailable'
    CHECK (evidence_state IN ('verified','legacy_unavailable','needs_reconfirmation','source_removed')),
  evidence_subject TEXT CHECK (evidence_subject IN ('user_statement','workflow_configuration','recorded_observation')),
  is_inference INTEGER NOT NULL DEFAULT 0 CHECK (is_inference IN (0, 1)),
  claim_digest TEXT,
  status TEXT NOT NULL CHECK (
    status IN ('pending','approved','rejected','blocked')
  ),
  blocked_code TEXT,
  created_at TEXT NOT NULL,
  decided_at TEXT,
  UNIQUE (review_run_id, content_hash)
);

CREATE INDEX IF NOT EXISTS idx_memory_candidates_status_workflow
  ON memory_candidates(status, workflow_id);
CREATE INDEX IF NOT EXISTS idx_memory_candidates_created ON memory_candidates(created_at);
CREATE INDEX IF NOT EXISTS idx_memory_reviews_status ON memory_reviews(status);
CREATE INDEX IF NOT EXISTS idx_memory_reviews_created ON memory_reviews(created_at);

-- Revision indexes and triggers are installed by the idempotent migration
-- after compatibility columns exist. `schema.sql` also runs against existing
-- databases whose `CREATE TABLE IF NOT EXISTS` leaves the older table shape in
-- place, so new-column objects cannot safely be created here.

-- Original evidence is captured before prompt rendering. Candidate evidence is
-- disposable with its queue row; approved evidence is copied to the canonical
-- memory so clearing decided suggestions does not erase provenance.
CREATE TABLE IF NOT EXISTS run_evidence_sources (
  id TEXT PRIMARY KEY NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  step_id TEXT REFERENCES run_steps(id) ON DELETE CASCADE,
  node_id TEXT,
  origin TEXT NOT NULL CHECK (origin IN (
    'authored_task','configured_workflow_instruction','external_trigger_data',
    'host_recorded_observation','model_output','prior_memory','retrieved_historical_summary'
  )),
  producer_identity TEXT,
  source_label TEXT,
  observed_at TEXT NOT NULL,
  normalization_version INTEGER NOT NULL,
  normalized_text TEXT NOT NULL,
  capture_state TEXT NOT NULL CHECK (capture_state IN ('complete','truncated','omitted')),
  omission_reason TEXT CHECK (omission_reason IN ('run_byte_limit','sensitive_content','observation_unavailable')),
  original_bytes INTEGER NOT NULL,
  captured_bytes INTEGER NOT NULL,
  content_digest TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_run_evidence_sources_run
  ON run_evidence_sources(run_id, observed_at, id);
CREATE INDEX IF NOT EXISTS idx_run_evidence_sources_step
  ON run_evidence_sources(step_id);

CREATE TABLE IF NOT EXISTS memory_candidate_evidence (
  id TEXT PRIMARY KEY NOT NULL,
  candidate_id TEXT NOT NULL REFERENCES memory_candidates(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  source_id TEXT REFERENCES run_evidence_sources(id) ON DELETE SET NULL,
  fragment_id TEXT NOT NULL,
  source_run_id TEXT NOT NULL,
  source_step_id TEXT,
  source_node_id TEXT,
  origin TEXT NOT NULL,
  producer_identity TEXT,
  source_label TEXT,
  quote TEXT,
  byte_start INTEGER,
  byte_end INTEGER,
  normalization_version INTEGER,
  fragment_digest TEXT,
  source_removed INTEGER NOT NULL DEFAULT 0 CHECK (source_removed IN (0, 1)),
  created_at TEXT NOT NULL,
  UNIQUE (candidate_id, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_memory_candidate_evidence_source
  ON memory_candidate_evidence(source_id);

CREATE TABLE IF NOT EXISTS memory_evidence (
  id TEXT PRIMARY KEY NOT NULL,
  memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  source_id TEXT REFERENCES run_evidence_sources(id) ON DELETE SET NULL,
  fragment_id TEXT NOT NULL,
  source_run_id TEXT NOT NULL,
  source_step_id TEXT,
  source_node_id TEXT,
  origin TEXT NOT NULL,
  producer_identity TEXT,
  source_label TEXT,
  quote TEXT,
  byte_start INTEGER,
  byte_end INTEGER,
  normalization_version INTEGER,
  fragment_digest TEXT,
  source_removed INTEGER NOT NULL DEFAULT 0 CHECK (source_removed IN (0, 1)),
  created_at TEXT NOT NULL,
  UNIQUE (memory_id, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_memory_evidence_source ON memory_evidence(source_id);

CREATE TRIGGER IF NOT EXISTS scrub_evidence_source_before_delete
BEFORE DELETE ON run_evidence_sources
BEGIN
  UPDATE memory_candidates
     SET status = CASE WHEN status = 'pending' THEN 'blocked' ELSE status END,
         blocked_code = CASE WHEN status = 'pending' THEN 'source_removed' ELSE blocked_code END,
         evidence_state = 'source_removed',
         claim_digest = NULL,
         rationale = ''
   WHERE id IN (
     SELECT candidate_id FROM memory_candidate_evidence WHERE source_id = OLD.id
   );
  UPDATE memories
     SET provenance_state = 'source_removed'
   WHERE id IN (
     SELECT memory_id FROM memory_evidence WHERE source_id = OLD.id
   );
  UPDATE memory_candidate_evidence
     SET source_id = NULL, quote = NULL, byte_start = NULL, byte_end = NULL,
         normalization_version = NULL, fragment_digest = NULL, source_removed = 1
   WHERE source_id = OLD.id;
  UPDATE memory_evidence
     SET source_id = NULL, quote = NULL, byte_start = NULL, byte_end = NULL,
         normalization_version = NULL, fragment_digest = NULL, source_removed = 1
   WHERE source_id = OLD.id;
END;

-- Safe licensing snapshot only. The full key and Polar activation ID live
-- together in the OS credential store under `credential_ref`.
CREATE TABLE IF NOT EXISTS license_snapshot (
  id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  product TEXT NOT NULL,
  status TEXT NOT NULL,
  masked_key TEXT,
  benefit_id TEXT,
  activation_label TEXT,
  current_device INTEGER NOT NULL DEFAULT 0,
  expires_at TEXT,
  last_success_at TEXT,
  refresh_due_at TEXT,
  offline_deadline TEXT,
  error_code TEXT,
  credential_ref TEXT UNIQUE,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runs_workflow_id ON runs(workflow_id);
CREATE INDEX IF NOT EXISTS idx_run_steps_run_id ON run_steps(run_id);
CREATE INDEX IF NOT EXISTS idx_schedules_workflow_id ON schedules(workflow_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_schedules_workflow_unique ON schedules(workflow_id);
CREATE INDEX IF NOT EXISTS idx_triggers_workflow_id ON triggers(workflow_id);
CREATE INDEX IF NOT EXISTS idx_triggers_enabled ON triggers(enabled);
CREATE INDEX IF NOT EXISTS idx_memories_workflow_id ON memories(workflow_id);
CREATE INDEX IF NOT EXISTS idx_memories_workflow_pinned ON memories(workflow_id, pinned);
CREATE INDEX IF NOT EXISTS idx_memory_links_workflow_id ON memory_links(workflow_id);
CREATE INDEX IF NOT EXISTS idx_memory_links_memory_id ON memory_links(memory_id);
CREATE INDEX IF NOT EXISTS idx_run_memory_uses_run_node_rank
  ON run_memory_uses(run_id, node_id, rank);
CREATE INDEX IF NOT EXISTS idx_run_memory_uses_memory_id ON run_memory_uses(memory_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_app_connections_identity
  ON app_connections(provider_id, connection_mode, identity_key);
CREATE INDEX IF NOT EXISTS idx_app_connections_provider_id ON app_connections(provider_id);
CREATE INDEX IF NOT EXISTS idx_app_event_queue_trigger ON app_event_queue(trigger_id, enqueued_at);
CREATE INDEX IF NOT EXISTS idx_app_event_receipts_received ON app_event_receipts(received_at);
