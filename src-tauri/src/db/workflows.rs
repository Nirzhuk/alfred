use super::memories::remove_output_deletion_files;
use super::output_persistence::{tombstone_output_receipts_for_workflow, OutputArtifactStore};
use super::{Db, DbError};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::path::Path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub working_directory: String,
    pub folder_id: Option<String>,
    pub memory_retrieval_enabled: bool,
    pub memory_lookup_enabled: bool,
    pub graph: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkflowInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default = "empty_graph")]
    pub graph: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkflowInput {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub working_directory: Option<String>,
    pub memory_retrieval_enabled: Option<bool>,
    pub memory_lookup_enabled: Option<bool>,
    pub graph: Option<Value>,
}

fn empty_graph() -> Value {
    serde_json::json!({ "nodes": [], "edges": [] })
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn row_to_workflow(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workflow> {
    let graph_json: String = row.get(7)?;
    Ok(Workflow {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        working_directory: row.get(3)?,
        folder_id: row.get(4)?,
        memory_retrieval_enabled: row.get::<_, i64>(5)? != 0,
        memory_lookup_enabled: row.get::<_, i64>(6)? != 0,
        graph: serde_json::from_str(&graph_json).unwrap_or_else(|_| empty_graph()),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

impl Db {
    pub fn list_workflows(&self) -> Result<Vec<Workflow>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, description, working_directory, folder_id,
                        memory_retrieval_enabled, memory_lookup_enabled, graph_json,
                        created_at, updated_at
                 FROM workflows
                 ORDER BY sort_order ASC, updated_at DESC",
            )?;

            let rows = stmt
                .query_map([], row_to_workflow)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(rows)
        })
    }

    pub fn get_workflow(&self, id: &str) -> Result<Option<Workflow>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, description, working_directory, folder_id,
                        memory_retrieval_enabled, memory_lookup_enabled, graph_json,
                        created_at, updated_at
                 FROM workflows
                 WHERE id = ?1",
            )?;

            let mut rows = stmt.query_map(params![id], row_to_workflow)?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn create_workflow(&self, input: CreateWorkflowInput) -> Result<Workflow, DbError> {
        let id = Uuid::new_v4().to_string();
        let created_at = now();
        let graph_json =
            serde_json::to_string(&input.graph).map_err(|e| DbError::Other(e.to_string()))?;

        self.with_conn(|conn| {
            if let Some(folder_id) = input.folder_id.as_deref() {
                let exists: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM workflow_folders WHERE id = ?1",
                    params![folder_id],
                    |row| row.get(0),
                )?;
                if exists == 0 {
                    return Err(DbError::Other(format!("folder not found: {folder_id}")));
                }
            }
            let next_order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MIN(sort_order), 0) - 1 FROM workflows",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(-1);
            conn.execute(
                "INSERT INTO workflows
                   (id, name, description, working_directory, folder_id, sort_order,
                    memory_retrieval_enabled, memory_lookup_enabled, graph_json,
                    created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 0, ?7, ?8, ?8)",
                params![
                    id,
                    input.name,
                    input.description,
                    input.working_directory,
                    input.folder_id,
                    next_order,
                    graph_json,
                    created_at
                ],
            )?;
            Ok(())
        })?;

        self.get_workflow(&id)?
            .ok_or_else(|| DbError::Other("failed to load created workflow".into()))
    }

    /// Persist sidebar order. `ordered_ids` is top → bottom.
    pub fn reorder_workflows(&self, ordered_ids: &[String]) -> Result<(), DbError> {
        if ordered_ids.is_empty() {
            return Ok(());
        }

        self.with_conn(|conn| {
            // Held under the Db mutex — treat as one logical write.
            for (index, id) in ordered_ids.iter().enumerate() {
                conn.execute(
                    "UPDATE workflows SET sort_order = ?1 WHERE id = ?2",
                    params![index as i64, id],
                )?;
            }
            Ok(())
        })
    }

    pub fn update_workflow(&self, input: UpdateWorkflowInput) -> Result<Workflow, DbError> {
        let graph_json = input
            .graph
            .map(|graph| serde_json::to_string(&graph))
            .transpose()
            .map_err(|error| DbError::Other(error.to_string()))?;
        let updated_at = now();
        let workflow_id = input.id.clone();
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let changed = transaction.execute(
                "UPDATE workflows
                    SET name = COALESCE(?1, name),
                        description = COALESCE(?2, description),
                        working_directory = COALESCE(?3, working_directory),
                        memory_retrieval_enabled =
                            COALESCE(?4, memory_retrieval_enabled),
                        memory_lookup_enabled =
                            COALESCE(?5, memory_lookup_enabled),
                        graph_json = COALESCE(?6, graph_json),
                        updated_at = ?7
                  WHERE id = ?8",
                params![
                    input.name,
                    input.description,
                    input.working_directory,
                    input
                        .memory_retrieval_enabled
                        .map(|enabled| if enabled { 1_i64 } else { 0_i64 }),
                    input
                        .memory_lookup_enabled
                        .map(|enabled| if enabled { 1_i64 } else { 0_i64 }),
                    graph_json,
                    updated_at,
                    &workflow_id,
                ],
            )?;
            if changed == 0 {
                return Err(DbError::Other(format!("workflow not found: {workflow_id}")));
            }
            let updated = {
                let mut statement = transaction.prepare(
                    "SELECT id, name, description, working_directory, folder_id,
                            memory_retrieval_enabled, memory_lookup_enabled, graph_json,
                            created_at, updated_at
                     FROM workflows
                     WHERE id = ?1",
                )?;
                statement
                    .query_row(params![&workflow_id], row_to_workflow)
                    .optional()?
                    .ok_or_else(|| {
                        DbError::Other(format!("workflow not found: {workflow_id}"))
                    })?
            };
            transaction.commit()?;
            Ok(updated)
        })
    }

    pub fn delete_workflow(&self, id: &str) -> Result<(), DbError> {
        let deleted_at = Utc::now().to_rfc3339();
        let (changed, deleted_artifacts, output_cleanups) = self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let output_cleanups =
                tombstone_output_receipts_for_workflow(&transaction, id, &deleted_at)?;
            let deleted_artifacts = {
                let mut statement = transaction.prepare(
                    "SELECT artifact_path FROM memories
                     WHERE workflow_id = ?1 AND scope_type = 'workflow' AND artifact_path IS NOT NULL",
                )?;
                let rows = statement
                    .query_map(params![id], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let preserved_memory_ids = {
                let mut statement = transaction.prepare(
                    "SELECT id FROM memories
                     WHERE workflow_id = ?1 AND scope_type IN ('user', 'workspace')",
                )?;
                let rows = statement
                    .query_map(params![id], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            // Explicit cleanup so delete still works if CASCADE isn't active
            // on an older database connection.
            transaction.execute(
                "DELETE FROM memory_fts WHERE memory_id IN (
                   SELECT id FROM memories WHERE workflow_id = ?1 AND scope_type = 'workflow'
                 )",
                params![id],
            )?;
            transaction.execute("DELETE FROM run_step_fts WHERE workflow_id = ?1", params![id])?;
            transaction.execute(
                "DELETE FROM memories WHERE workflow_id = ?1 AND scope_type = 'workflow'",
                params![id],
            )?;
            transaction.execute(
                "UPDATE memories SET workflow_id = NULL
                 WHERE workflow_id = ?1 AND scope_type IN ('user', 'workspace')",
                params![id],
            )?;
            for memory_id in preserved_memory_ids {
                super::history::index_memory(&transaction, &memory_id)?;
            }
            transaction.execute("DELETE FROM schedules WHERE workflow_id = ?1", params![id])?;
            super::memory_evidence::delete_workflow_evidence_sources_conn(&transaction, id)?;
            transaction.execute(
                "DELETE FROM run_steps WHERE run_id IN (SELECT id FROM runs WHERE workflow_id = ?1)",
                params![id],
            )?;
            transaction.execute("DELETE FROM runs WHERE workflow_id = ?1", params![id])?;
            let changed = transaction.execute("DELETE FROM workflows WHERE id = ?1", params![id])?;
            transaction.commit()?;
            Ok((changed, deleted_artifacts, output_cleanups))
        })?;

        if changed == 0 {
            return Err(DbError::Other(format!("workflow not found: {id}")));
        }

        for artifact in deleted_artifacts {
            if let Some(path) = self.trusted_memory_artifact_path(Path::new(&artifact)) {
                let _ = std::fs::remove_file(path);
            }
        }
        let output_store = self.output_artifact_store();
        remove_output_deletion_files(self, &output_store, output_cleanups);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_partial_updates_preserve_unrelated_fields_after_reload() {
        let db = Arc::new(Db::open_in_memory().expect("open isolated database"));
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Concurrent".into(),
                description: "Keep me".into(),
                working_directory: "/tmp/project".into(),
                folder_id: None,
                graph: json!({"nodes": [{"id": "keep"}], "edges": []}),
            })
            .expect("create workflow");
        let workflow_id = workflow.id;
        let barrier = Arc::new(Barrier::new(3));

        std::thread::scope(|scope| {
            let lookup_db = Arc::clone(&db);
            let lookup_barrier = Arc::clone(&barrier);
            let lookup_id = workflow_id.clone();
            let lookup = scope.spawn(move || {
                lookup_barrier.wait();
                lookup_db
                    .update_workflow(UpdateWorkflowInput {
                        id: lookup_id,
                        name: None,
                        description: None,
                        working_directory: None,
                        memory_retrieval_enabled: None,
                        memory_lookup_enabled: Some(true),
                        graph: None,
                    })
                    .expect("lookup update");
            });

            let retrieval_db = Arc::clone(&db);
            let retrieval_barrier = Arc::clone(&barrier);
            let retrieval_id = workflow_id.clone();
            let retrieval = scope.spawn(move || {
                retrieval_barrier.wait();
                retrieval_db
                    .update_workflow(UpdateWorkflowInput {
                        id: retrieval_id,
                        name: None,
                        description: None,
                        working_directory: None,
                        memory_retrieval_enabled: Some(false),
                        memory_lookup_enabled: None,
                        graph: None,
                    })
                    .expect("retrieval update");
            });

            barrier.wait();
            lookup.join().expect("lookup thread");
            retrieval.join().expect("retrieval thread");
        });

        let reloaded = db
            .get_workflow(&workflow_id)
            .expect("reload workflow")
            .expect("workflow exists");
        assert!(reloaded.memory_lookup_enabled);
        assert!(!reloaded.memory_retrieval_enabled);
        assert_eq!(reloaded.name, "Concurrent");
        assert_eq!(reloaded.description, "Keep me");
        assert_eq!(reloaded.working_directory, "/tmp/project");
        assert_eq!(reloaded.graph, json!({"nodes": [{"id": "keep"}], "edges": []}));
    }
}
