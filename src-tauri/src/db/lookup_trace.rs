use super::DbError;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) struct LookupTraceSourceInput<'a> {
    pub(crate) reference: &'a str,
    pub(crate) source_kind: &'a str,
    pub(crate) source_id: &'a str,
    pub(crate) version: &'a str,
    pub(crate) state: &'a str,
    pub(crate) page: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LookupTraceInput<'a> {
    pub(crate) turn_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) node_id: &'a str,
    pub(crate) request_id: &'a str,
    pub(crate) tool: &'a str,
    pub(crate) retrieved_bytes: usize,
    pub(crate) returned_bytes: usize,
    pub(crate) classification: &'a str,
    pub(crate) delivery: &'a str,
    pub(crate) created_at: &'a str,
    pub(crate) sources: &'a [LookupTraceSourceInput<'a>],
}

pub(crate) const LOOKUP_TRACE_SCHEMA: &str = r#"
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
"#;

pub(crate) fn create_lookup_trace_schema(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(LOOKUP_TRACE_SCHEMA)?;
    Ok(())
}

pub(crate) fn insert_lookup_trace(
    conn: &Connection,
    input: &LookupTraceInput<'_>,
) -> Result<String, DbError> {
    if input.turn_id.trim().is_empty()
        || input.run_id.trim().is_empty()
        || input.node_id.trim().is_empty()
        || input.request_id.trim().is_empty()
        || input.tool.trim().is_empty()
        || input.classification.trim().is_empty()
        || input.delivery.trim().is_empty()
    {
        return Err(DbError::Other("lookup_trace_identity_missing".into()));
    }
    let retrieved_bytes = i64::try_from(input.retrieved_bytes)
        .map_err(|_| DbError::Other("lookup_trace_bytes_overflow".into()))?;
    let returned_bytes = i64::try_from(input.returned_bytes)
        .map_err(|_| DbError::Other("lookup_trace_bytes_overflow".into()))?;
    let trace_id = Uuid::new_v4().to_string();
    let transaction = conn.unchecked_transaction()?;
    transaction.execute(
        "INSERT INTO memory_lookup_traces
           (id, turn_id, run_id, node_id, request_id, tool, retrieved_bytes,
            returned_bytes, classification, delivery, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            trace_id,
            input.turn_id,
            input.run_id,
            input.node_id,
            input.request_id,
            input.tool,
            retrieved_bytes,
            returned_bytes,
            input.classification,
            input.delivery,
            input.created_at,
        ],
    )?;
    for (ordinal, source) in input.sources.iter().enumerate() {
        let ordinal = i64::try_from(ordinal)
            .map_err(|_| DbError::Other("lookup_trace_source_overflow".into()))?;
        transaction.execute(
            "INSERT INTO memory_lookup_trace_sources
               (trace_id, ordinal, reference, source_kind, source_id, version, state, page)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                trace_id,
                ordinal,
                source.reference,
                source.source_kind,
                source.source_id,
                source.version,
                source.state,
                source.page.map(i64::from),
            ],
        )?;
    }
    transaction.commit()?;
    Ok(trace_id)
}

pub(crate) fn associate_lookup_traces_with_step_conn(
    conn: &Connection,
    run_id: &str,
    node_id: &str,
    step_id: &str,
) -> Result<usize, DbError> {
    let changed = conn.execute(
        "UPDATE memory_lookup_traces
            SET step_id = ?1
          WHERE run_id = ?2 AND node_id = ?3 AND step_id IS NULL",
        params![step_id, run_id, node_id],
    )?;
    Ok(changed)
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LookupTraceRow {
    pub(crate) id: String,
    pub(crate) run_id: String,
    pub(crate) node_id: String,
    pub(crate) step_id: Option<String>,
    pub(crate) tool: String,
    pub(crate) retrieved_bytes: i64,
    pub(crate) returned_bytes: i64,
    pub(crate) classification: String,
    pub(crate) delivery: String,
}

#[cfg(test)]
pub(crate) fn lookup_trace_rows_conn(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<LookupTraceRow>, DbError> {
    let mut statement = conn.prepare(
        "SELECT id, run_id, node_id, step_id, tool, retrieved_bytes,
                returned_bytes, classification, delivery
           FROM memory_lookup_traces
          WHERE run_id = ?1
          ORDER BY created_at, id",
    )?;
    let rows = statement
        .query_map(params![run_id], |row| {
            Ok(LookupTraceRow {
                id: row.get(0)?,
                run_id: row.get(1)?,
                node_id: row.get(2)?,
                step_id: row.get(3)?,
                tool: row.get(4)?,
                retrieved_bytes: row.get(5)?,
                returned_bytes: row.get(6)?,
                classification: row.get(7)?,
                delivery: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
