pub(crate) mod artifact_chunks;
mod agent_accounts;
pub(crate) mod lookup_trace;
pub(crate) mod agent_memory_lookup;
mod app_connections;
mod app_events;
mod history;
mod license;
mod memories;
mod memory_conflicts;
mod memory_curation;
#[cfg(test)]
mod memory_eval;
mod memory_evidence;
mod memory_query;
mod memory_retrieval;
pub(crate) mod memory_validity;
mod migrate;
mod output_persistence;
mod schedules;
mod template_shares;
mod triggers;
mod workflow_folders;
mod workflows;

pub use agent_accounts::AgentAccountReadDiagnostic;
pub use app_events::*;
pub use history::*;
pub use license::*;
pub use memories::*;
pub(crate) use memory_conflicts::*;
pub use memory_curation::*;
pub use memory_evidence::*;
pub use memory_retrieval::*;
pub use output_persistence::*;
pub use schedules::*;
pub use template_shares::*;
pub use triggers::*;
pub use workflow_folders::*;
pub use workflows::*;

use directories::ProjectDirs;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not resolve app data directory")]
    NoAppDir,
    #[error("{0}")]
    Other(String),
}

pub struct Db {
    conn: Mutex<Connection>,
    artifact_root: PathBuf,
}

impl Db {
    pub fn open() -> Result<Self, DbError> {
        let root = app_data_dir()?;
        fs::create_dir_all(&root)?;
        let path = root.join("app.db");

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(include_str!("schema.sql"))?;
        migrate::apply_migrations(&conn)?;

        let database = Self {
            conn: Mutex::new(conn),
            artifact_root: root,
        };
        database.verify_legacy_artifacts(8)?;
        Ok(database)
    }

    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| DbError::Other("database lock poisoned".into()))?;
        f(&conn)
    }

    pub(crate) fn output_artifact_store(&self) -> output_persistence::OutputArtifactStore {
        output_persistence::OutputArtifactStore::new(self.artifact_root.clone())
    }

    pub(crate) fn memory_artifact_dir(&self, workflow_id: &str) -> Result<PathBuf, DbError> {
        if workflow_id.trim().is_empty() {
            return Err(DbError::Other("memory artifact workflow is missing".into()));
        }
        let dir = self.artifact_root.join("artifacts").join(workflow_id);
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub(crate) fn artifact_root(&self) -> PathBuf {
        self.artifact_root.clone()
    }

    pub(crate) fn trusted_memory_artifact_path(&self, stored_path: &Path) -> Option<PathBuf> {
        let root = self.artifact_root.join("artifacts");
        let canonical = artifact_chunks::trusted_artifact_path(&root, stored_path)?;
        let canonical_root = fs::canonicalize(root).ok()?;
        let mut components = canonical.strip_prefix(canonical_root).ok()?.components();
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            .then_some(())?;
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            .then_some(())?;
        components.next().is_none().then_some(canonical)
    }
    /// Repair legacy artifact proof in a small startup batch. This is the only
    /// production path that reads an unindexed artifact: it validates the
    /// trusted root/path and canonical full digest while retaining only fixed
    /// chunk hashes. Lookup calls never invoke this repair.
    pub(crate) fn verify_legacy_artifacts(&self, limit: usize) -> Result<usize, DbError> {
        let limit = limit.clamp(1, 32);
        // ponytail: rows that can never be indexed (missing file, digest
        // mismatch, untrusted path) are rescanned each startup; scanning 8x
        // the batch keeps them from starving newer rows. Persist a failed
        // state if that set can outgrow the scan.
        let scan = limit * 8;
        let root = self.artifact_root.join("artifacts");
        self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT m.id, m.artifact_content_digest, m.artifact_path
                   FROM memories m
                   LEFT JOIN artifact_chunk_manifests ac ON ac.memory_id = m.id
                  WHERE m.artifact_path IS NOT NULL
                    AND m.artifact_content_digest IS NOT NULL
                    AND (ac.memory_id IS NULL OR ac.verification_state != 'verified')
                  ORDER BY m.updated_at ASC, m.id ASC
                  LIMIT ?1",
            )?;
            let candidates = statement
                .query_map([scan as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(statement);

            let mut indexed = 0usize;
            for (memory_id, content_digest, path) in candidates {
                if indexed == limit {
                    break;
                }
                if artifact_chunks::index_legacy_artifact_file_conn(
                    conn,
                    &memory_id,
                    &content_digest,
                    &root,
                    Path::new(&path),
                    &chrono::Utc::now().to_rfc3339(),
                )? {
                    indexed += 1;
                }
            }
            Ok(indexed)
        })
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory_with_artifact_root(
        root: impl Into<PathBuf>,
    ) -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(include_str!("schema.sql"))?;
        migrate::apply_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            artifact_root: root.into(),
        })
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Result<Self, DbError> {
        let root = std::env::temp_dir()
            .join("alfred-test-artifacts")
            .join(Uuid::new_v4().to_string());
        Self::open_in_memory_with_artifact_root(root)
    }
}


pub fn app_data_dir() -> Result<PathBuf, DbError> {
    let dirs = ProjectDirs::from("com", "nirzhuk", "alfred").ok_or(DbError::NoAppDir)?;
    Ok(dirs.data_dir().to_path_buf())
}

#[cfg(test)]
mod legacy_artifact_tests {
    use super::memories::{write_canonical_memory, CanonicalMemoryRecord};
    use super::memory_validity::artifact_content_digest;
    use super::{Db, MemoryScopeType, MemoryStatus, MemoryType};
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn unindexable_legacy_rows_do_not_starve_later_artifacts() {
        let db = Db::open_in_memory().expect("db");
        let dir = db.artifact_root().join("artifacts").join("legacy");
        fs::create_dir_all(&dir).expect("artifact dir");
        let body = "legacy artifact body";
        // The three oldest rows point at missing files; only the newest indexes.
        for (index, id) in ["missing-a", "missing-b", "missing-c", "present"]
            .into_iter()
            .enumerate()
        {
            let path = dir.join(format!("{}.txt", Uuid::new_v4()));
            if id == "present" {
                fs::write(&path, body).expect("artifact file");
            }
            let record = CanonicalMemoryRecord {
                id: id.into(),
                workflow_id: None,
                run_id: None,
                node_id: None,
                scope_type: MemoryScopeType::User,
                scope_key: "local-user".into(),
                kind: "artifact".into(),
                memory_type: MemoryType::Artifact,
                source: "run".into(),
                title: id.into(),
                body: body.into(),
                artifact_path: Some(path.to_string_lossy().into_owned()),
                artifact_content_digest: Some(artifact_content_digest(body.as_bytes())),
                pinned: false,
                confidence: 1.0,
                salience: 50,
                status: MemoryStatus::Active,
                supersedes_id: None,
                fact_key: None,
                observed_at: None,
                last_confirmed_at: None,
                valid_from: None,
                expires_at: None,
                created_at: format!("2026-08-18T10:0{index}:00Z"),
            };
            db.with_conn(|conn| write_canonical_memory(conn, &record))
                .expect("memory");
        }
        assert_eq!(db.verify_legacy_artifacts(1).expect("verify"), 1);
    }
}
