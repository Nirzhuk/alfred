//! Redacted local publication and provenance metadata. The owner capability is
//! held in the OS credential store; this table intentionally stores only its
//! opaque credential reference.
use super::memory_validity::{current_memory_validity, ValidityState};
use super::{Db, DbError};
use crate::templates::contract::{StartKind, StartRecipe, TemplateGraph};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path};
use uuid::Uuid;

#[cfg(test)]
#[path = "template_shares_tests.rs"]
mod tests;

/// Migration fragment for the shared database migration owner to apply. Keep
/// this separate from the plan's memory migrations so it can be reviewed and
/// merged without rewriting schema history.
pub const TEMPLATE_SHARES_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS template_shares (id TEXT PRIMARY KEY NOT NULL, workflow_id TEXT REFERENCES workflows(id) ON DELETE SET NULL, request_id TEXT NOT NULL, digest TEXT NOT NULL, credential_ref TEXT NOT NULL, created_at TEXT NOT NULL, revoked_at TEXT, UNIQUE(credential_ref, request_id)); CREATE INDEX IF NOT EXISTS idx_template_shares_workflow ON template_shares(workflow_id); CREATE TABLE IF NOT EXISTS template_imports (attempt_id TEXT PRIMARY KEY NOT NULL, workflow_id TEXT NOT NULL UNIQUE REFERENCES workflows(id) ON DELETE CASCADE, source_link TEXT NOT NULL, digest TEXT NOT NULL, created_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS template_publication_requests (request_id TEXT PRIMARY KEY NOT NULL, workflow_id TEXT REFERENCES workflows(id) ON DELETE SET NULL, digest TEXT NOT NULL, credential_ref TEXT NOT NULL, share_id TEXT, template_json TEXT, created_at TEXT NOT NULL, UNIQUE(credential_ref, request_id));";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateShareRecord {
    pub id: String,
    pub workflow_id: Option<String>,
    pub request_id: String,
    pub digest: String,
    pub credential_ref: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

impl Db {
    pub fn get_template_publication_request(
        &self,
        request_id: &str,
    ) -> Result<Option<PublicationRequestRecord>, DbError> {
        self.with_conn(|conn| Ok(conn.query_row("SELECT request_id, workflow_id, digest, credential_ref, share_id, template_json, created_at FROM template_publication_requests WHERE request_id = ?1", params![request_id], |row| Ok(PublicationRequestRecord { request_id: row.get(0)?, workflow_id: row.get(1)?, digest: row.get(2)?, credential_ref: row.get(3)?, share_id: row.get(4)?, template_json: row.get(5)?, created_at: row.get(6)? })).optional()?))
    }

    pub fn list_pending_template_publications(
        &self,
    ) -> Result<Vec<PublicationRequestRecord>, DbError> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare("SELECT request_id, workflow_id, digest, credential_ref, share_id, template_json, created_at FROM template_publication_requests WHERE share_id IS NULL ORDER BY created_at DESC")?;
            let records = statement.query_map([], |row| Ok(PublicationRequestRecord { request_id: row.get(0)?, workflow_id: row.get(1)?, digest: row.get(2)?, credential_ref: row.get(3)?, share_id: row.get(4)?, template_json: row.get(5)?, created_at: row.get(6)? }))?.collect::<Result<Vec<_>, _>>()?;
            Ok(records)
        })
    }

    pub fn begin_template_publication(
        &self,
        workflow_id: &str,
        request_id: &str,
        digest: &str,
        credential_ref: &str,
        template_json: &str,
    ) -> Result<Option<PublicationRequestRecord>, DbError> {
        self.with_conn(|conn| {
            let existing = conn.query_row("SELECT request_id, workflow_id, digest, credential_ref, share_id, template_json, created_at FROM template_publication_requests WHERE request_id = ?1 AND credential_ref = ?2", params![request_id, credential_ref], |row| Ok(PublicationRequestRecord { request_id: row.get(0)?, workflow_id: row.get(1)?, digest: row.get(2)?, credential_ref: row.get(3)?, share_id: row.get(4)?, template_json: row.get(5)?, created_at: row.get(6)? })).optional()?;
            if let Some(record) = existing { if record.workflow_id.as_deref() != Some(workflow_id) || record.digest != digest { return Err(DbError::Other("publication request conflicts with existing content".into())); } return Ok(Some(record)); }
            conn.execute("INSERT INTO template_publication_requests (request_id, workflow_id, digest, credential_ref, template_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![request_id, workflow_id, digest, credential_ref, template_json, Utc::now().to_rfc3339()])?; Ok(None)
        })
    }

    pub fn complete_template_publication(
        &self,
        workflow_id: Option<&str>,
        request_id: &str,
        digest: &str,
        credential_ref: &str,
        share_id: &str,
    ) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let durable: Option<(Option<String>, String, String, Option<String>)> = tx
                .query_row(
                    "SELECT workflow_id, digest, credential_ref, share_id
                     FROM template_publication_requests WHERE request_id = ?1",
                    params![request_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;
            let Some((durable_workflow_id, durable_digest, durable_credential, accepted_id)) =
                durable
            else {
                return Err(DbError::Other("publication request not found".into()));
            };
            if durable_digest != digest
                || durable_credential != credential_ref
                || durable_workflow_id.as_deref() != workflow_id
            {
                return Err(DbError::Other(
                    "publication completion does not match the durable request".into(),
                ));
            }
            if let Some(accepted_id) = accepted_id {
                if accepted_id != share_id {
                    return Err(DbError::Other(
                        "publication request is already bound to a different share ID".into(),
                    ));
                }
                tx.commit()?;
                return Ok(());
            }
            tx.execute(
                "INSERT INTO template_shares
                    (id, workflow_id, request_id, digest, credential_ref, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    share_id,
                    workflow_id,
                    request_id,
                    digest,
                    credential_ref,
                    Utc::now().to_rfc3339()
                ],
            )?;
            let changed = tx.execute(
                "UPDATE template_publication_requests
                 SET share_id = ?1
                 WHERE request_id = ?2 AND credential_ref = ?3 AND digest = ?4
                   AND share_id IS NULL",
                params![share_id, request_id, credential_ref, digest],
            )?;
            if changed != 1 {
                return Err(DbError::Other(
                    "publication completion did not update the durable request".into(),
                ));
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Import all local records under the database mutex and one SQLite
    /// transaction. Reusing an attempt ID returns the original copy, so a
    /// lost response or double click cannot create a second workflow.
    pub fn import_template_transaction(
        &self,
        attempt_id: &str,
        title: &str,
        description: &str,
        working_directory: &str,
        graph: &TemplateGraph,
        starts: &[StartRecipe],
        source_link: &str,
        digest: &str,
    ) -> Result<TemplateShareImport, DbError> {
        if attempt_id.trim().is_empty() || source_link.trim().is_empty() || digest.len() != 64 {
            return Err(DbError::Other("invalid import provenance".into()));
        }
        let graph_json =
            serde_json::to_string(&serde_json::json!({"nodes": graph.nodes, "edges": graph.edges}))
                .map_err(|error| DbError::Other(error.to_string()))?;
        let (workflow_id, reused) = self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            if let Ok(existing) = tx.query_row("SELECT workflow_id, source_link, digest FROM template_imports WHERE attempt_id = ?1", params![attempt_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))) {
                if existing.1 != source_link || existing.2 != digest { return Err(DbError::Other("import attempt conflicts with existing content".into())); }
                tx.commit()?; return Ok((existing.0, true));
            }
            let workflow_id = Uuid::new_v4().to_string(); let at = Utc::now().to_rfc3339();
            tx.execute("INSERT INTO workflows (id, name, description, working_directory, folder_id, sort_order, memory_retrieval_enabled, memory_lookup_enabled, graph_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, NULL, (SELECT COALESCE(MIN(sort_order), 0) - 1 FROM workflows), 0, 0, ?5, ?6, ?6)", params![workflow_id, title, description, working_directory, graph_json, at])?;
            tx.execute("INSERT OR REPLACE INTO workflow_memory_review (workflow_id, enabled, updated_at) VALUES (?1, 0, ?2)", params![workflow_id, at])?;
            // Memory IDs in a template are references to the recipient's local
            // library. Make every selected reference visible to the imported
            // workflow in this same transaction, and reject stale/retracted
            // records before the workflow can become observable.
            for node in &graph.nodes {
                let Some(ids) = node.data.get("memoryIds").and_then(Value::as_array) else { continue; };
                for memory_id in ids.iter().filter_map(Value::as_str) {
                    let memory: Option<(String, String, Option<String>, String, Option<String>, Option<String>)> = tx.query_row(
                        "SELECT scope_type, scope_key, workflow_id, status, valid_from, expires_at
                         FROM memories WHERE id = ?1",
                        params![memory_id],
                        |row| Ok((
                            row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                            row.get(4)?, row.get(5)?
                        )),
                    ).optional()?;
                    let Some((scope_type, scope_key, owner_workflow, status, valid_from, expires_at)) = memory else {
                        return Err(DbError::Other(format!("selected memory {memory_id} is unavailable")));
                    };
                    let current = matches!(
                        current_memory_validity(
                            status == "active",
                            valid_from.as_deref(),
                            expires_at.as_deref(),
                            Utc::now(),
                        ),
                        Ok(ValidityState::Current)
                    );
                    if !current {
                        return Err(DbError::Other(format!("selected memory {memory_id} is expired or unavailable")));
                    }
                    match scope_type.as_str() {
                        // Workflow memories are shared deliberately by creating
                        // the visible link inside the same transaction.
                        "workflow" if owner_workflow.as_deref() != Some(workflow_id.as_str()) => {
                            tx.execute("INSERT OR IGNORE INTO memory_links (id, workflow_id, memory_id, created_at) VALUES (?1, ?2, ?3, ?4)", params![Uuid::new_v4().to_string(), workflow_id, memory_id, at])?;
                        }
                        // User memories are visible to every workflow and must
                        // not acquire a workflow link.
                        "user" if scope_key == "local-user" => {}
                        // Workspace memories are visible only when the imported
                        // copy keeps the exact normalized workspace context.
                        "workspace" if normalize_workspace_key(working_directory)
                            .is_some_and(|path| path == scope_key) => {}
                        _ => {
                            return Err(DbError::Other(format!(
                                "selected memory {memory_id} is unavailable in this workflow context"
                            )));
                        }
                    }
                }
            }
            for start in starts {
                let config = serde_json::to_string(&start.config).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
                match start.kind {
                    StartKind::Schedule => { let cron = start.config.get("cron").and_then(Value::as_str).ok_or_else(|| rusqlite::Error::InvalidParameterName("schedule.cron".into()))?; tx.execute("INSERT INTO schedules (id, workflow_id, cron, enabled, next_run_at, created_at, updated_at) VALUES (?1, ?2, ?3, 0, NULL, ?4, ?4)", params![Uuid::new_v4().to_string(), workflow_id, cron, at])?; }
                    StartKind::File | StartKind::AppEvent | StartKind::Webhook => { let source = match start.kind { StartKind::File => "file", StartKind::AppEvent => "app", StartKind::Webhook => "webhook", StartKind::Schedule => unreachable!() }; let secret = (source == "webhook").then(|| Uuid::new_v4().simple().to_string()); tx.execute("INSERT INTO triggers (id, workflow_id, source, label, config_json, secret, enabled, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)", params![Uuid::new_v4().to_string(), workflow_id, source, start.config.get("label").and_then(Value::as_str).unwrap_or("Imported start"), config, secret, at])?; }
                }
            }
            tx.execute("INSERT INTO template_imports (attempt_id, workflow_id, source_link, digest, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![attempt_id, workflow_id, source_link, digest, at])?;
            tx.commit()?; Ok((workflow_id, false))
        })?;
        self.get_workflow(&workflow_id)?
            .map(|workflow| TemplateShareImport {
                workflow_id,
                workflow,
                reused,
            })
            .ok_or_else(|| DbError::Other("failed to load imported workflow".into()))
    }

    pub fn list_template_shares(
        &self,
        workflow_id: Option<&str>,
    ) -> Result<Vec<TemplateShareRecord>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, workflow_id, request_id, digest, credential_ref, created_at, revoked_at FROM template_shares WHERE (?1 IS NULL OR workflow_id = ?1) ORDER BY created_at DESC")?;
            let rows = stmt.query_map(params![workflow_id], |row| Ok(TemplateShareRecord { id: row.get(0)?, workflow_id: row.get(1)?, request_id: row.get(2)?, digest: row.get(3)?, credential_ref: row.get(4)?, created_at: row.get(5)?, revoked_at: row.get(6)? }))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn revoke_template_share(&self, id: &str) -> Result<bool, DbError> {
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE template_shares SET revoked_at = COALESCE(revoked_at, ?1) WHERE id = ?2",
                params![Utc::now().to_rfc3339(), id],
            )?)
        })?;
        Ok(changed > 0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateShareImport {
    pub workflow_id: String,
    pub workflow: super::Workflow,
    pub reused: bool,
}

fn normalize_workspace_key(value: &str) -> Option<String> {
    let path = Path::new(value.trim());
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = std::path::PathBuf::new();
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(part) => {
                normalized.push(part);
                depth += 1;
            }
            Component::ParentDir => {
                if depth == 0 {
                    return None;
                }
                normalized.pop();
                depth -= 1;
            }
        }
    }
    normalized
        .is_absolute()
        .then(|| normalized.to_string_lossy().into_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicationRequestRecord {
    pub request_id: String,
    pub workflow_id: Option<String>,
    pub digest: String,
    pub credential_ref: String,
    pub share_id: Option<String>,
    pub template_json: Option<String>,
    pub created_at: String,
}
