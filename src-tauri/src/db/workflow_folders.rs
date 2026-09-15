use super::{Db, DbError, Workflow};
use chrono::Utc;
use rusqlite::params;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowFolder {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn row_to_folder(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowFolder> {
    Ok(WorkflowFolder {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

impl Db {
    pub fn list_workflow_folders(&self) -> Result<Vec<WorkflowFolder>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, created_at, updated_at
                 FROM workflow_folders
                 ORDER BY sort_order ASC, created_at ASC",
            )?;
            let folders = stmt
                .query_map([], row_to_folder)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(folders)
        })
    }

    pub fn create_workflow_folder(&self, name: &str) -> Result<WorkflowFolder, DbError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DbError::Other("folder name cannot be empty".into()));
        }
        let id = Uuid::new_v4().to_string();
        let created_at = now();
        self.with_conn(|conn| {
            let duplicate: i64 = conn.query_row(
                "SELECT COUNT(*) FROM workflow_folders WHERE name = ?1 COLLATE NOCASE",
                params![name],
                |row| row.get(0),
            )?;
            if duplicate > 0 {
                return Err(DbError::Other(format!(
                    "a folder named ‘{name}’ already exists"
                )));
            }
            let next_order: i64 = conn.query_row(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM workflow_folders",
                [],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO workflow_folders (id, name, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, name, next_order, created_at],
            )?;
            Ok(())
        })?;
        Ok(WorkflowFolder {
            id,
            name: name.to_string(),
            created_at: created_at.clone(),
            updated_at: created_at,
        })
    }

    pub fn rename_workflow_folder(&self, id: &str, name: &str) -> Result<WorkflowFolder, DbError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DbError::Other("folder name cannot be empty".into()));
        }
        let updated_at = now();
        let created_at = self.with_conn(|conn| {
            let duplicate: i64 = conn.query_row(
                "SELECT COUNT(*) FROM workflow_folders
                 WHERE name = ?1 COLLATE NOCASE AND id != ?2",
                params![name, id],
                |row| row.get(0),
            )?;
            if duplicate > 0 {
                return Err(DbError::Other(format!(
                    "a folder named ‘{name}’ already exists"
                )));
            }
            let created_at: String = conn
                .query_row(
                    "SELECT created_at FROM workflow_folders WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|_| DbError::Other(format!("folder not found: {id}")))?;
            conn.execute(
                "UPDATE workflow_folders SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, updated_at, id],
            )?;
            Ok(created_at)
        })?;
        Ok(WorkflowFolder {
            id: id.to_string(),
            name: name.to_string(),
            created_at,
            updated_at,
        })
    }

    pub fn delete_workflow_folder(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE workflows SET folder_id = NULL WHERE folder_id = ?1",
                params![id],
            )?;
            let changed =
                conn.execute("DELETE FROM workflow_folders WHERE id = ?1", params![id])?;
            if changed == 0 {
                return Err(DbError::Other(format!("folder not found: {id}")));
            }
            Ok(())
        })
    }

    pub fn reorder_workflow_folders(&self, ordered_ids: &[String]) -> Result<(), DbError> {
        self.with_conn(|conn| {
            for (index, id) in ordered_ids.iter().enumerate() {
                conn.execute(
                    "UPDATE workflow_folders SET sort_order = ?1 WHERE id = ?2",
                    params![index as i64, id],
                )?;
            }
            Ok(())
        })
    }

    pub fn move_workflow_to_folder(
        &self,
        workflow_id: &str,
        folder_id: Option<&str>,
    ) -> Result<Workflow, DbError> {
        self.with_conn(|conn| {
            if let Some(folder_id) = folder_id {
                let exists: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM workflow_folders WHERE id = ?1",
                    params![folder_id],
                    |row| row.get(0),
                )?;
                if exists == 0 {
                    return Err(DbError::Other(format!("folder not found: {folder_id}")));
                }
            }
            let changed = conn.execute(
                "UPDATE workflows SET folder_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![folder_id, now(), workflow_id],
            )?;
            if changed == 0 {
                return Err(DbError::Other(format!("workflow not found: {workflow_id}")));
            }
            Ok(())
        })?;
        self.get_workflow(workflow_id)?
            .ok_or_else(|| DbError::Other("failed to load moved workflow".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CreateWorkflowInput;
    use rusqlite::Connection;
    use serde_json::json;
    use std::sync::Mutex;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        conn.execute_batch(include_str!("schema.sql"))
            .expect("create schema");
        Db {
            conn: Mutex::new(conn),
            artifact_root: std::env::temp_dir().join(format!(
                "alfred-workflow-folder-test-{}",
                uuid::Uuid::new_v4()
            )),
        }
    }

    #[test]
    fn folders_organize_workflows_without_owning_them() {
        let db = test_db();
        let folder = db
            .create_workflow_folder("Client projects")
            .expect("create folder");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Weekly summary".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: json!({ "nodes": [], "edges": [] }),
            })
            .expect("create workflow");

        let moved = db
            .move_workflow_to_folder(&workflow.id, Some(&folder.id))
            .expect("move workflow");
        assert_eq!(moved.folder_id.as_deref(), Some(folder.id.as_str()));

        db.delete_workflow_folder(&folder.id)
            .expect("delete folder");
        let preserved = db
            .get_workflow(&workflow.id)
            .expect("load workflow")
            .expect("workflow remains");
        assert_eq!(preserved.folder_id, None);
    }

    #[test]
    fn folder_names_are_unique_ignoring_case() {
        let db = test_db();
        db.create_workflow_folder("Research")
            .expect("create folder");
        let error = db
            .create_workflow_folder("research")
            .expect_err("duplicate should fail");
        assert!(error.to_string().contains("already exists"));
    }
}
