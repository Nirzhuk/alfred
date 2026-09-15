use super::memory_evidence::{list_run_evidence_sources_conn, EvidenceSource};
use super::output_persistence::{list_output_receipts_for_run_conn, OutputReceipt};
#[cfg(test)]
use super::Db;
use super::DbError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const SEARCHABLE_JSON_MAX_BYTES: usize = 32 * 1024;
const MAX_SEARCH_TERMS: usize = 12;
const MAX_SEARCH_TERM_CHARS: usize = 64;
const FINAL_OUTPUT_PREVIEW_MAX_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHistoryItem {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub trigger: String,
    pub status: String,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub step_count: i64,
    pub final_output_preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHistoryStep {
    pub id: String,
    pub node_id: String,
    pub agent_provider: Option<String>,
    pub skill_name: Option<String>,
    pub status: String,
    pub input: Value,
    pub output: Value,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHistoryDetail {
    pub run: RunHistoryItem,
    pub steps: Vec<RunHistoryStep>,
    pub memory_uses: Vec<RunHistoryMemoryUse>,
    pub memory_context_summaries: Vec<RunHistoryMemoryContextSummary>,
    pub evidence_sources: Vec<EvidenceSource>,
    pub output_receipts: Vec<OutputReceipt>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHistoryMemoryContextSummary {
    pub node_id: String,
    pub budget_bytes: i64,
    pub memory_bytes: i64,
    pub included_count: i64,
    pub explicit_omitted_count: i64,
    pub pinned_omitted_count: i64,
    pub automatic_omitted_count: i64,
    pub unavailable_count: i64,
    pub repeated_count: i64,
    pub trace_available: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHistoryMemoryUse {
    pub node_id: String,
    pub memory_id: String,
    pub memory_title: String,
    pub scope_type: String,
    pub memory_type: String,
    pub rank: i64,
    pub score: f64,
    pub reason: String,
    pub rendered_bytes: i64,
    pub ranking_version: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySearchInput {
    pub query: String,
    pub workflow_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySearchHit {
    pub kind: String,
    pub source_id: String,
    pub run_id: Option<String>,
    pub workflow_id: Option<String>,
    pub workflow_name: String,
    pub title: String,
    pub snippet: String,
    pub timestamp: String,
    pub rank: f64,
}

pub(crate) fn plain_text_fts_query(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .filter_map(|term| {
            let bounded: String = term.chars().take(MAX_SEARCH_TERM_CHARS).collect();
            if bounded.is_empty() {
                None
            } else {
                Some(format!("\"{}\"*", bounded.replace('"', "\"\"")))
            }
        })
        .take(MAX_SEARCH_TERMS)
        .collect::<Vec<_>>();

    (!terms.is_empty()).then(|| terms.join(" AND "))
}

fn parse_json_or_empty(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| serde_json::json!({}))
}

fn row_to_run_history_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunHistoryItem> {
    let final_output_json: Option<String> = row.get(10)?;
    let final_output_preview = final_output_json
        .as_deref()
        .map(parse_json_or_empty)
        .map(|value| searchable_json_text(&value, FINAL_OUTPUT_PREVIEW_MAX_BYTES))
        .unwrap_or_default();
    Ok(RunHistoryItem {
        id: row.get(0)?,
        workflow_id: row.get(1)?,
        workflow_name: row.get(2)?,
        trigger: row.get(3)?,
        status: row.get(4)?,
        error: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        created_at: row.get(8)?,
        step_count: row.get(9)?,
        final_output_preview,
    })
}

const RUN_HISTORY_SELECT: &str =
    "SELECT r.id, r.workflow_id, w.name, r.trigger_kind, r.status, r.error,
            r.started_at, r.finished_at, r.created_at,
            (SELECT COUNT(*) FROM run_steps counted WHERE counted.run_id = r.id),
            (SELECT latest.output_json FROM run_steps latest
             WHERE latest.run_id = r.id
             ORDER BY latest.created_at DESC, latest.rowid DESC LIMIT 1)
     FROM runs r
     JOIN workflows w ON w.id = r.workflow_id";

impl super::Db {
    pub fn list_run_history(
        &self,
        workflow_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RunHistoryItem>, DbError> {
        let limit = limit.clamp(1, 100);
        self.with_conn(|conn| {
            let sql = format!(
                "{RUN_HISTORY_SELECT}
                 WHERE (?1 IS NULL OR r.workflow_id = ?1)
                 ORDER BY r.created_at DESC, r.rowid DESC
                 LIMIT ?2 OFFSET ?3"
            );
            let mut statement = conn.prepare(&sql)?;
            let rows = statement
                .query_map(
                    params![workflow_id, limit as i64, offset as i64],
                    row_to_run_history_item,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_run_history(&self, run_id: &str) -> Result<Option<RunHistoryDetail>, DbError> {
        self.with_conn(|conn| {
            let sql = format!("{RUN_HISTORY_SELECT} WHERE r.id = ?1");
            let run = conn
                .query_row(&sql, params![run_id], row_to_run_history_item)
                .optional()?;
            let Some(run) = run else {
                return Ok(None);
            };

            let mut statement = conn.prepare(
                "SELECT id, node_id, agent_provider, skill_name, status, input_json,
                        output_json, error, started_at, finished_at, created_at
                 FROM run_steps
                 WHERE run_id = ?1
                 ORDER BY created_at ASC, rowid ASC",
            )?;
            let steps = statement
                .query_map(params![run_id], |row| {
                    let input_json: String = row.get(5)?;
                    let output_json: String = row.get(6)?;
                    Ok(RunHistoryStep {
                        id: row.get(0)?,
                        node_id: row.get(1)?,
                        agent_provider: row.get(2)?,
                        skill_name: row.get(3)?,
                        status: row.get(4)?,
                        input: parse_json_or_empty(&input_json),
                        output: parse_json_or_empty(&output_json),
                        error: row.get(7)?,
                        started_at: row.get(8)?,
                        finished_at: row.get(9)?,
                        created_at: row.get(10)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut memory_statement = conn.prepare(
                "SELECT u.node_id, u.memory_id, m.title, m.scope_type, m.memory_type,
                        u.rank, u.score, u.reason, u.rendered_bytes, u.ranking_version,
                        u.created_at
                 FROM run_memory_uses u
                 JOIN memories m ON m.id = u.memory_id
                 WHERE u.run_id = ?1
                 ORDER BY u.created_at ASC, u.node_id ASC, u.rank ASC, u.memory_id ASC",
            )?;
            let memory_uses = memory_statement
                .query_map(params![run_id], |row| {
                    let score: f64 = row.get(6)?;
                    Ok(RunHistoryMemoryUse {
                        node_id: row.get(0)?,
                        memory_id: row.get(1)?,
                        memory_title: row.get(2)?,
                        scope_type: row.get(3)?,
                        memory_type: row.get(4)?,
                        rank: row.get(5)?,
                        score: (score * 100.0).round() / 100.0,
                        reason: row.get(7)?,
                        rendered_bytes: row.get(8)?,
                        ranking_version: row.get(9)?,
                        created_at: row.get(10)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut summary_statement = conn.prepare(
                "SELECT node_id, budget_bytes, memory_bytes, included_count,
                        explicit_omitted_count, pinned_omitted_count,
                        automatic_omitted_count, unavailable_count, repeated_count,
                        trace_available, created_at
                 FROM run_memory_context_summaries
                 WHERE run_id = ?1
                 ORDER BY created_at ASC, node_id ASC",
            )?;
            let memory_context_summaries = summary_statement
                .query_map(params![run_id], |row| {
                    Ok(RunHistoryMemoryContextSummary {
                        node_id: row.get(0)?,
                        budget_bytes: row.get(1)?,
                        memory_bytes: row.get(2)?,
                        included_count: row.get(3)?,
                        explicit_omitted_count: row.get(4)?,
                        pinned_omitted_count: row.get(5)?,
                        automatic_omitted_count: row.get(6)?,
                        unavailable_count: row.get(7)?,
                        repeated_count: row.get(8)?,
                        trace_available: row.get::<_, i64>(9)? != 0,
                        created_at: row.get(10)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let evidence_sources = list_run_evidence_sources_conn(conn, run_id)?;
            let output_receipts = list_output_receipts_for_run_conn(conn, run_id)?;
            Ok(Some(RunHistoryDetail {
                run,
                steps,
                memory_uses,
                memory_context_summaries,
                evidence_sources,
                output_receipts,
            }))
        })
    }

    pub fn search_history(
        &self,
        input: HistorySearchInput,
    ) -> Result<Vec<HistorySearchHit>, DbError> {
        let fts_query = plain_text_fts_query(&input.query)
            .ok_or_else(|| DbError::Other("history search query must not be empty".into()))?;
        let limit = input.limit.unwrap_or(25).clamp(1, 50);
        let context = input
            .workflow_id
            .as_deref()
            .map(|workflow_id| self.memory_context(workflow_id))
            .transpose()?;
        let workflow_id = context.as_ref().map(|value| value.workflow_id.as_str());
        let workspace_key = context
            .as_ref()
            .and_then(|value| value.working_directory.as_deref());
        self.with_conn(|conn| {
            let mut hits = Vec::with_capacity(limit.saturating_mul(2));
            let mut run_statement = conn.prepare(
                "SELECT run_step_fts.step_id, run_step_fts.run_id,
                        run_step_fts.workflow_id, w.name,
                        COALESCE(NULLIF(rs.skill_name, ''), NULLIF(rs.agent_provider, ''), rs.node_id),
                        snippet(run_step_fts, -1, '[', ']', '…', 24),
                        rs.created_at,
                        bm25(run_step_fts, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
                 FROM run_step_fts
                 JOIN run_steps rs ON rs.id = run_step_fts.step_id
                 JOIN workflows w ON w.id = run_step_fts.workflow_id
                 WHERE run_step_fts MATCH ?1
                   AND (?2 IS NULL OR run_step_fts.workflow_id = ?2)
                 ORDER BY bm25(run_step_fts), rs.created_at DESC, run_step_fts.step_id
                 LIMIT ?3",
            )?;
            let run_hits = run_statement
                .query_map(
                    params![fts_query, workflow_id, limit as i64],
                    |row| {
                        Ok(HistorySearchHit {
                            kind: "run_step".into(),
                            source_id: row.get(0)?,
                            run_id: Some(row.get(1)?),
                            workflow_id: Some(row.get(2)?),
                            workflow_name: row.get(3)?,
                            title: row.get(4)?,
                            snippet: row.get(5)?,
                            timestamp: row.get(6)?,
                            rank: row.get(7)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            hits.extend(run_hits);

            // History is an audit surface: retained superseded/retracted rows stay
            // searchable, while scope visibility still follows the active context.
            let mut memory_statement = conn.prepare(
                "SELECT memory_fts.memory_id, m.run_id, m.workflow_id,
                        CASE m.scope_type
                          WHEN 'user' THEN 'User memory'
                          WHEN 'workspace' THEN 'Workspace memory'
                          ELSE COALESCE(w.name, 'Workflow memory')
                        END,
                        m.title, snippet(memory_fts, -1, '[', ']', '…', 24),
                        m.updated_at, bm25(memory_fts, 0.0, 0.0, 1.0, 1.0)
                 FROM memory_fts
                 JOIN memories m ON m.id = memory_fts.memory_id
                 LEFT JOIN workflows w ON w.id = m.workflow_id
                 WHERE memory_fts MATCH ?1
                   AND (
                     ?2 IS NULL
                     OR (m.scope_type = 'user' AND m.scope_key = 'local-user')
                     OR (m.scope_type = 'workspace' AND m.scope_key = ?3)
                     OR (m.scope_type = 'workflow' AND (
                       m.scope_key = ?2
                       OR EXISTS (
                         SELECT 1 FROM memory_links l
                         WHERE l.workflow_id = ?2 AND l.memory_id = m.id
                       )
                     ))
                   )
                 ORDER BY bm25(memory_fts), m.updated_at DESC, memory_fts.memory_id
                 LIMIT ?4",
            )?;
            let memory_hits = memory_statement
                .query_map(
                    params![fts_query, workflow_id, workspace_key, limit as i64],
                    |row| {
                        Ok(HistorySearchHit {
                            kind: "memory".into(),
                            source_id: row.get(0)?,
                            run_id: row.get(1)?,
                            workflow_id: row.get(2)?,
                            workflow_name: row.get(3)?,
                            title: row.get(4)?,
                            snippet: row.get(5)?,
                            timestamp: row.get(6)?,
                            rank: row.get(7)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            hits.extend(memory_hits);

            hits.sort_by(|left, right| {
                left.rank
                    .total_cmp(&right.rank)
                    .then_with(|| right.timestamp.cmp(&left.timestamp))
                    .then_with(|| left.kind.cmp(&right.kind))
                    .then_with(|| left.source_id.cmp(&right.source_id))
            });
            hits.truncate(limit);
            Ok(hits)
        })
    }
}

/// Flatten JSON leaves into bounded searchable text without indexing object
/// keys. The output is always valid UTF-8 and excludes non-whitespace control
/// characters.
pub(crate) fn searchable_json_text(value: &Value, max_bytes: usize) -> String {
    fn append_leaf(output: &mut String, leaf: &str, max_bytes: usize) {
        if output.len() >= max_bytes {
            return;
        }
        if !output.is_empty() && output.len() < max_bytes {
            output.push(' ');
        }
        for character in leaf.chars() {
            if character.is_control() && character != '\n' && character != '\t' {
                continue;
            }
            if output.len() + character.len_utf8() > max_bytes {
                break;
            }
            output.push(character);
        }
    }

    fn walk(value: &Value, output: &mut String, max_bytes: usize) {
        if output.len() >= max_bytes {
            return;
        }
        match value {
            Value::Null => {}
            Value::Bool(value) => append_leaf(output, &value.to_string(), max_bytes),
            Value::Number(value) => append_leaf(output, &value.to_string(), max_bytes),
            Value::String(value) => append_leaf(output, value, max_bytes),
            Value::Array(values) => {
                for value in values {
                    walk(value, output, max_bytes);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    walk(value, output, max_bytes);
                }
            }
        }
    }

    if max_bytes == 0 {
        return String::new();
    }
    let mut output = String::new();
    walk(value, &mut output, max_bytes);
    output
}

pub(crate) fn index_run_step(
    conn: &Connection,
    step_id: &str,
    run_id: &str,
    workflow_id: &str,
    node_id: &str,
    input: &Value,
    output: &Value,
    error: Option<&str>,
) -> Result<(), DbError> {
    delete_run_step_index(conn, step_id)?;
    let input_text = searchable_json_text(input, SEARCHABLE_JSON_MAX_BYTES);
    let output_text = searchable_json_text(output, SEARCHABLE_JSON_MAX_BYTES);
    let error_text = error
        .map(|value| {
            searchable_json_text(&Value::String(value.to_owned()), SEARCHABLE_JSON_MAX_BYTES)
        })
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO run_step_fts
         (step_id, run_id, workflow_id, node_id, input_text, output_text, error_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            step_id,
            run_id,
            workflow_id,
            node_id,
            input_text,
            output_text,
            error_text
        ],
    )?;
    Ok(())
}

pub(crate) fn index_memory(conn: &Connection, memory_id: &str) -> Result<(), DbError> {
    let memory: Option<(Option<String>, String, String)> = conn
        .query_row(
            "SELECT workflow_id, title, body FROM memories WHERE id = ?1",
            params![memory_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    delete_memory_index(conn, memory_id)?;
    if let Some((workflow_id, title, body)) = memory {
        conn.execute(
            "INSERT INTO memory_fts(memory_id, workflow_id, title, body)
             VALUES (?1, ?2, ?3, ?4)",
            params![memory_id, workflow_id, title, body],
        )?;
    }
    Ok(())
}

pub(crate) fn delete_run_step_index(conn: &Connection, step_id: &str) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM run_step_fts WHERE step_id = ?1",
        params![step_id],
    )?;
    Ok(())
}

pub(crate) fn delete_memory_index(conn: &Connection, memory_id: &str) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM memory_fts WHERE memory_id = ?1",
        params![memory_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        CreateMemoryInput, CreateWorkflowInput, MemoryScopeType, MemoryStatus, MemoryType,
        UpdateMemoryInput,
    };
    use rusqlite::params;
    use serde_json::json;

    fn create_workflow(db: &Db, name: &str) -> String {
        create_workflow_at(db, name, "")
    }

    fn create_workflow_at(db: &Db, name: &str, working_directory: &str) -> String {
        db.create_workflow(CreateWorkflowInput {
            name: name.to_owned(),
            description: String::new(),
            working_directory: working_directory.to_owned(),
            folder_id: None,
            graph: json!({ "nodes": [], "edges": [] }),
        })
        .expect("create workflow")
        .id
    }

    fn insert_run_fixture(
        db: &Db,
        workflow_id: &str,
        run_id: &str,
        created_at: &str,
        steps: &[(&str, &str, Value, Value)],
    ) {
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs
                 (id, workflow_id, trigger_kind, status, started_at, finished_at, created_at)
                 VALUES (?1, ?2, 'manual', 'completed', ?3, ?3, ?3)",
                params![run_id, workflow_id, created_at],
            )?;
            for (step_id, node_id, input, output) in steps {
                conn.execute(
                    "INSERT INTO run_steps
                     (id, run_id, node_id, agent_provider, skill_name, status,
                      input_json, output_json, started_at, finished_at, created_at)
                     VALUES (?1, ?2, ?3, 'codex', 'planner', 'completed', ?4, ?5,
                             ?6, ?6, ?6)",
                    params![
                        step_id,
                        run_id,
                        node_id,
                        serde_json::to_string(input).expect("serialize input"),
                        serde_json::to_string(output).expect("serialize output"),
                        created_at,
                    ],
                )?;
                index_run_step(
                    conn,
                    step_id,
                    run_id,
                    workflow_id,
                    node_id,
                    input,
                    output,
                    None,
                )?;
            }
            Ok(())
        })
        .expect("insert run fixture");
    }

    #[test]
    fn searchable_json_is_bounded_and_omits_keys_and_controls() {
        let value = json!({
            "secretKeyName": ["first\u{0000}", 42, true, "ééé"]
        });
        let text = searchable_json_text(&value, 18);

        assert_eq!(text, "first 42 true éé");
        assert!(!text.contains("secretKeyName"));
        assert!(text.len() <= 18);
        assert!(text.is_char_boundary(text.len()));
    }

    #[test]
    fn memory_index_tracks_create_update_and_delete() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Memory workflow");
        let memory = db
            .create_memory(CreateMemoryInput {
                workflow_id,
                title: "Original title".into(),
                body: "original body".into(),
                run_id: None,
                node_id: None,
                kind: None,
                scope_type: None,
                memory_type: None,
                source: Some("manual".into()),
                pinned: None,
                confidence: None,
                salience: None,
                status: None,
                supersedes_id: None,
                fact_key: None,
                observed_at: None,
                last_confirmed_at: None,
                valid_from: None,
                expires_at: None,
                id: Some("memory-sync".into()),
            })
            .expect("create memory");

        db.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH '\"original\"*'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .expect("query created index");

        db.update_memory(UpdateMemoryInput {
            id: memory.id.clone(),
            expected_semantic_revision: memory.semantic_revision,
            context_workflow_id: None,
            title: Some("Updated title".into()),
            body: Some("replacement body".into()),
            pinned: None,
            kind: None,
            scope_type: None,
            memory_type: None,
            confidence: None,
            salience: None,
            status: None,
            supersedes_id: None,
            fact_key: None,
            observed_at: None,
            last_confirmed_at: None,
            valid_from: None,
            expires_at: None,
        })
        .expect("update memory");
        db.with_conn(|conn| {
            let old_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH '\"original\"*'",
                [],
                |row| row.get(0),
            )?;
            let new_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH '\"replacement\"*'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!((old_count, new_count), (0, 1));
            Ok(())
        })
        .expect("query updated index");

        db.delete_memory(&memory.id).expect("delete memory");
        db.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_id = ?1",
                params![memory.id],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0);
            Ok(())
        })
        .expect("query deleted index");
    }

    #[test]
    fn run_step_index_tracks_insert_and_delete() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Run workflow");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs(id, workflow_id, trigger_kind, status, created_at)
                 VALUES ('run-sync', ?1, 'manual', 'completed', '2026-08-18T10:00:00Z')",
                params![workflow_id],
            )?;
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, status, input_json, output_json, created_at)
                 VALUES ('step-sync', 'run-sync', 'agent-1', 'completed', '{}', '{}',
                         '2026-08-18T10:01:00Z')",
                [],
            )?;
            index_run_step(
                &transaction,
                "step-sync",
                "run-sync",
                &workflow_id,
                "agent-1",
                &json!({ "prompt": "find the launch decision" }),
                &json!({ "answer": "ship on Friday" }),
                None,
            )?;
            transaction.commit()?;

            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE run_step_fts MATCH '\"launch\"*'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);

            let transaction = conn.unchecked_transaction()?;
            delete_run_step_index(&transaction, "step-sync")?;
            transaction.execute("DELETE FROM run_steps WHERE id = 'step-sync'", [])?;
            transaction.commit()?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE step_id = 'step-sync'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0);
            Ok(())
        })
        .expect("synchronize run-step index");
    }

    #[test]
    fn legacy_rows_backfill_once_without_duplicates() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Legacy workflow");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO memories
                 (id, workflow_id, scope_type, scope_key, kind, memory_type,
                  source, title, body, created_at, updated_at)
                 VALUES ('legacy-memory', ?1, 'workflow', ?1, 'text', 'output',
                         'import', 'Legacy note',
                         'remember unicode café', '2026-08-18T09:00:00Z', '2026-08-18T09:00:00Z')",
                params![workflow_id],
            )?;
            conn.execute(
                "INSERT INTO runs(id, workflow_id, trigger_kind, status, created_at)
                 VALUES ('legacy-run', ?1, 'manual', 'completed', '2026-08-18T09:00:00Z')",
                params![workflow_id],
            )?;
            conn.execute(
                "INSERT INTO run_steps
                 (id, run_id, node_id, status, input_json, output_json, created_at)
                 VALUES ('legacy-step', 'legacy-run', 'node-1', 'completed',
                         '{not valid json', '{\"answer\":\"legacy output\"}',
                         '2026-08-18T09:01:00Z')",
                [],
            )?;
            conn.execute("DELETE FROM memory_fts", [])?;
            conn.execute("DELETE FROM run_step_fts", [])?;
            conn.execute(
                "DELETE FROM schema_meta WHERE key = 'search_fts_backfill_v1'",
                [],
            )?;

            crate::db::migrate::rebuild_search_indexes(conn)?;
            crate::db::migrate::rebuild_search_indexes(conn)?;

            let memory_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE memory_id = 'legacy-memory'",
                [],
                |row| row.get(0),
            )?;
            let step_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE step_id = 'legacy-step'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!((memory_count, step_count), (1, 1));
            Ok(())
        })
        .expect("backfill legacy rows");
    }

    #[test]
    fn workflow_delete_cleans_both_indexes() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Disposable workflow");
        db.create_memory(CreateMemoryInput {
            workflow_id: workflow_id.clone(),
            title: "Delete me".into(),
            body: "memory cleanup".into(),
            run_id: None,
            node_id: None,
            kind: None,
            scope_type: None,
            memory_type: None,
            source: None,
            pinned: None,
            confidence: None,
            salience: None,
            status: None,
            supersedes_id: None,
            fact_key: None,
            observed_at: None,
            last_confirmed_at: None,
            valid_from: None,
            expires_at: None,
            id: Some("delete-memory".into()),
        })
        .expect("create memory");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runs(id, workflow_id, trigger_kind, status, created_at)
                 VALUES ('delete-run', ?1, 'manual', 'completed', '2026-08-18T11:00:00Z')",
                params![workflow_id],
            )?;
            conn.execute(
                "INSERT INTO run_steps(id, run_id, node_id, status, created_at)
                 VALUES ('delete-step', 'delete-run', 'node-1', 'completed',
                         '2026-08-18T11:01:00Z')",
                [],
            )?;
            index_run_step(
                conn,
                "delete-step",
                "delete-run",
                &workflow_id,
                "node-1",
                &json!({ "input": "cleanup" }),
                &json!({ "output": "cleanup" }),
                None,
            )?;
            Ok(())
        })
        .expect("create run fixture");

        db.delete_workflow(&workflow_id).expect("delete workflow");
        db.with_conn(|conn| {
            let memories: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE workflow_id = ?1",
                params![workflow_id],
                |row| row.get(0),
            )?;
            let steps: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_step_fts WHERE workflow_id = ?1",
                params![workflow_id],
                |row| row.get(0),
            )?;
            assert_eq!((memories, steps), (0, 0));
            Ok(())
        })
        .expect("query cleaned indexes");
    }

    #[test]
    fn plain_text_queries_quote_user_terms_without_exposing_fts_syntax() {
        assert_eq!(
            plain_text_fts_query("  release OR “café”  "),
            Some("\"release\"* AND \"OR\"* AND \"“café”\"*".into())
        );
        assert_eq!(
            plain_text_fts_query("say \"ship\""),
            Some("\"say\"* AND \"\"\"ship\"\"\"*".into())
        );
        assert_eq!(plain_text_fts_query("   \n\t  "), None);

        let long_term = "x".repeat(80);
        let query = plain_text_fts_query(&format!("{long_term} a b c d e f g h i j k l m"))
            .expect("bounded query");
        assert_eq!(query.matches(" AND ").count(), 11);
        assert_eq!(query.split(" AND ").next().unwrap().chars().count(), 67);
    }

    #[test]
    fn browses_newest_runs_and_loads_exact_ordered_detail() {
        let db = Db::open_in_memory().expect("open database");
        let first_workflow = create_workflow(&db, "First workflow");
        let second_workflow = create_workflow(&db, "Second workflow");
        insert_run_fixture(
            &db,
            &first_workflow,
            "run-old",
            "2026-08-18T08:00:00Z",
            &[
                (
                    "step-a",
                    "node-a",
                    json!({ "prompt": "start" }),
                    json!({ "draft": "one" }),
                ),
                (
                    "step-b",
                    "node-b",
                    json!({ "prompt": "finish" }),
                    json!({ "answer": "final answer" }),
                ),
            ],
        );
        insert_run_fixture(
            &db,
            &second_workflow,
            "run-new",
            "2026-08-18T09:00:00Z",
            &[("step-c", "node-c", json!({}), json!({ "answer": "newest" }))],
        );

        let all = db.list_run_history(None, 100, 0).expect("list all runs");
        assert_eq!(
            all.iter().map(|run| run.id.as_str()).collect::<Vec<_>>(),
            ["run-new", "run-old"]
        );
        assert_eq!(all[1].workflow_name, "First workflow");
        assert_eq!(all[1].step_count, 2);
        assert_eq!(all[1].final_output_preview, "final answer");

        let filtered = db
            .list_run_history(Some(&first_workflow), 0, 0)
            .expect("list filtered runs");
        assert_eq!(filtered.len(), 1, "zero limit clamps to one");
        assert_eq!(filtered[0].id, "run-old");

        let detail = db
            .get_run_history("run-old")
            .expect("load run detail")
            .expect("known run");
        assert_eq!(
            detail
                .steps
                .iter()
                .map(|step| step.id.as_str())
                .collect::<Vec<_>>(),
            ["step-a", "step-b"]
        );
        assert_eq!(detail.steps[1].output, json!({ "answer": "final answer" }));
        assert!(db
            .get_run_history("missing-run")
            .expect("load missing run")
            .is_none());
    }

    #[test]
    fn history_exposes_explicit_version_and_context_summary_without_changing_step_input() {
        let db = Db::open_in_memory().expect("open database");
        let workflow_id = create_workflow(&db, "Versioned memory history");
        insert_run_fixture(
            &db,
            &workflow_id,
            "versioned-run",
            "2026-08-18T08:00:00Z",
            &[(
                "versioned-step",
                "agent",
                json!({
                    "prompt": "release",
                    "memoryRecallStatus": "included",
                    "memoryRecallOmittedCount": 0
                }),
                json!({ "answer": "done" }),
            )],
        );
        db.create_memory(CreateMemoryInput {
            workflow_id,
            title: "Release decision".into(),
            body: "Use the release checklist".into(),
            run_id: None,
            node_id: None,
            kind: None,
            scope_type: None,
            memory_type: Some(MemoryType::Decision),
            source: Some("manual".into()),
            pinned: None,
            confidence: None,
            salience: None,
            status: None,
            supersedes_id: None,
            fact_key: None,
            observed_at: None,
            last_confirmed_at: None,
            valid_from: None,
            expires_at: None,
            id: Some("versioned-memory".into()),
        })
        .expect("create memory");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO run_memory_uses
                   (id, run_id, node_id, memory_id, rank, score, reason, rendered_bytes,
                    ranking_version, created_at)
                 VALUES ('versioned-use', 'versioned-run', 'agent', 'versioned-memory', 1, 220.0,
                         'explicit', 120, 'explicit_v1', '2026-08-18T08:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO run_memory_context_summaries
                   (run_id, node_id, budget_bytes, memory_bytes, included_count,
                    explicit_omitted_count, pinned_omitted_count,
                    automatic_omitted_count, unavailable_count, repeated_count,
                    trace_available, created_at)
                 VALUES ('versioned-run', 'agent', 12000, 120, 1, 2, 1, 3, 4, 5, 0,
                         '2026-08-18T08:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .expect("insert memory use");

        let detail = db
            .get_run_history("versioned-run")
            .expect("load history")
            .expect("known run");
        assert_eq!(detail.memory_uses.len(), 1);
        assert_eq!(detail.memory_uses[0].ranking_version, "explicit_v1");
        assert_eq!(detail.memory_uses[0].reason, "explicit");
        assert_eq!(detail.memory_context_summaries.len(), 1);
        let summary = &detail.memory_context_summaries[0];
        assert_eq!(summary.budget_bytes, 12_000);
        assert_eq!(summary.memory_bytes, 120);
        assert_eq!(summary.explicit_omitted_count, 2);
        assert_eq!(summary.pinned_omitted_count, 1);
        assert_eq!(summary.automatic_omitted_count, 3);
        assert_eq!(summary.unavailable_count, 4);
        assert_eq!(summary.repeated_count, 5);
        assert!(!summary.trace_available);
        assert_eq!(detail.steps[0].input["memoryRecallStatus"], "included");
        assert_eq!(detail.steps[0].input["memoryRecallOmittedCount"], 0);

        db.with_conn(|conn| {
            conn.execute("DELETE FROM runs WHERE id = 'versioned-run'", [])?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM run_memory_context_summaries WHERE run_id = 'versioned-run'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0);
            Ok(())
        })
        .expect("summary cascades with run");
    }

    #[test]
    fn searches_memory_and_run_documents_with_scope_ranking_and_safe_queries() {
        let db = Db::open_in_memory().expect("open database");
        let first_workflow = create_workflow(&db, "Search workflow");
        let second_workflow = create_workflow(&db, "Other workflow");
        insert_run_fixture(
            &db,
            &first_workflow,
            "search-run",
            "2026-08-18T10:00:00Z",
            &[(
                "search-step",
                "research-node",
                json!({ "prompt": "find café launch" }),
                json!({ "answer": "cafe launch launch launch decision" }),
            )],
        );
        for (id, workflow_id, title, body) in [
            (
                "search-memory",
                &first_workflow,
                "Launch note",
                "The café launch decision",
            ),
            (
                "other-memory",
                &second_workflow,
                "Other launch",
                "A launch elsewhere",
            ),
        ] {
            db.create_memory(CreateMemoryInput {
                workflow_id: workflow_id.clone(),
                title: title.into(),
                body: body.into(),
                run_id: None,
                node_id: None,
                kind: None,
                scope_type: None,
                memory_type: None,
                source: Some("manual".into()),
                pinned: None,
                confidence: None,
                salience: None,
                status: None,
                supersedes_id: None,
                fact_key: None,
                observed_at: None,
                last_confirmed_at: None,
                valid_from: None,
                expires_at: None,
                id: Some(id.into()),
            })
            .expect("create searchable memory");
        }

        let hits = db
            .search_history(HistorySearchInput {
                query: "launch".into(),
                workflow_id: None,
                limit: Some(50),
            })
            .expect("search all workflows");
        assert_eq!(hits.len(), 3);
        assert!(hits.windows(2).all(|pair| pair[0].rank <= pair[1].rank));
        assert!(hits
            .iter()
            .any(|hit| hit.kind == "run_step" && hit.source_id == "search-step"));
        assert!(hits
            .iter()
            .any(|hit| hit.kind == "memory" && hit.source_id == "search-memory"));
        assert!(hits
            .iter()
            .all(|hit| hit.snippet.contains('[') && hit.snippet.contains(']')));

        let scoped = db
            .search_history(HistorySearchInput {
                query: "café".into(),
                workflow_id: Some(first_workflow.clone()),
                limit: Some(50),
            })
            .expect("search current workflow");
        assert_eq!(scoped.len(), 2);
        assert!(scoped
            .iter()
            .all(|hit| hit.workflow_id.as_deref() == Some(first_workflow.as_str())));

        let limited = db
            .search_history(HistorySearchInput {
                query: "launch".into(),
                workflow_id: None,
                limit: Some(0),
            })
            .expect("search with clamped limit");
        assert_eq!(limited.len(), 1);

        for query in ["!!!", "launch OR memory", "say \"ship\""] {
            db.search_history(HistorySearchInput {
                query: query.into(),
                workflow_id: None,
                limit: None,
            })
            .expect("plain text must not produce FTS syntax errors");
        }
        assert!(db
            .search_history(HistorySearchInput {
                query: " \n ".into(),
                workflow_id: None,
                limit: None,
            })
            .is_err());
    }

    #[test]
    fn scoped_memory_search_survives_provenance_deletion_and_respects_context() {
        let db = Db::open_in_memory().expect("open database");
        let origin = create_workflow_at(&db, "Origin", "/projects/shared/./app");
        let current = create_workflow_at(&db, "Current", "/projects/shared/app");
        let unrelated = create_workflow_at(&db, "Unrelated", "/projects/private");

        let create =
            |id: &str, workflow_id: &str, scope_type: MemoryScopeType, status: MemoryStatus| {
                db.create_memory(CreateMemoryInput {
                    workflow_id: workflow_id.into(),
                    title: id.into(),
                    body: format!("scopeprobe {id}"),
                    run_id: None,
                    node_id: None,
                    kind: None,
                    scope_type: Some(scope_type),
                    memory_type: Some(MemoryType::Fact),
                    source: Some("manual".into()),
                    pinned: None,
                    confidence: None,
                    salience: None,
                    status: Some(status),
                    supersedes_id: None,
                    fact_key: None,
                    observed_at: None,
                    last_confirmed_at: None,
                    valid_from: None,
                    expires_at: None,
                    id: Some(id.into()),
                })
                .expect("create scoped search memory")
            };

        create(
            "visible-user",
            &origin,
            MemoryScopeType::User,
            MemoryStatus::Active,
        );
        create(
            "visible-workspace-history",
            &origin,
            MemoryScopeType::Workspace,
            MemoryStatus::Retracted,
        );
        create(
            "unrelated-workspace",
            &unrelated,
            MemoryScopeType::Workspace,
            MemoryStatus::Active,
        );
        let linked = create(
            "visible-linked",
            &unrelated,
            MemoryScopeType::Workflow,
            MemoryStatus::Active,
        );
        db.link_memory(&current, &linked.id)
            .expect("link visible memory");
        create(
            "unrelated-workflow",
            &unrelated,
            MemoryScopeType::Workflow,
            MemoryStatus::Active,
        );

        db.delete_workflow(&origin)
            .expect("delete provenance workflow");

        let all = db
            .search_history(HistorySearchInput {
                query: "scopeprobe".into(),
                workflow_id: None,
                limit: Some(50),
            })
            .expect("search all memory history");
        for id in ["visible-user", "visible-workspace-history"] {
            let hit = all
                .iter()
                .find(|hit| hit.source_id == id)
                .expect("preserved inherited memory hit");
            assert_eq!(hit.workflow_id, None);
            assert_eq!(
                hit.workflow_name,
                if id == "visible-user" {
                    "User memory"
                } else {
                    "Workspace memory"
                }
            );
        }

        let current_hits = db
            .search_history(HistorySearchInput {
                query: "scopeprobe".into(),
                workflow_id: Some(current),
                limit: Some(50),
            })
            .expect("search current memory scope");
        let ids = current_hits
            .iter()
            .map(|hit| hit.source_id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"visible-user"));
        assert!(ids.contains(&"visible-workspace-history"));
        assert!(ids.contains(&"visible-linked"));
        assert!(!ids.contains(&"unrelated-workspace"));
        assert!(!ids.contains(&"unrelated-workflow"));
    }
}
