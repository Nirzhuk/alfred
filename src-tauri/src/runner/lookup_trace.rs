use crate::agents::native::{
    MemoryLookupTraceSink, NativeMemoryLookupDelivery, NativeMemoryLookupTrace, NativeErrorCode,
    NativeRuntimeError,
};
use crate::db::agent_memory_lookup::{
    LookupSourceKind, LookupSourceState, LookupToolName, LookupTraceClassification,
};
use crate::db::lookup_trace::{
    insert_lookup_trace, LookupTraceInput, LookupTraceSourceInput,
};
use crate::db::Db;
use chrono::Utc;
use std::sync::Arc;


/// Persists the body-free identity of every native memory lookup after the
/// executor has applied its delivery contract. The sink deliberately receives
/// only source identities, versions, states, and page numbers; bodies,
/// queries, paths, and credentials never enter this record.
pub(crate) struct DbMemoryLookupTraceSink {
    db: Arc<Db>,
}

impl DbMemoryLookupTraceSink {
    pub(crate) fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

impl MemoryLookupTraceSink for DbMemoryLookupTraceSink {
    fn record(&self, trace: &NativeMemoryLookupTrace) -> Result<(), NativeRuntimeError> {
        let sources = trace
            .trace
            .sources
            .iter()
            .map(|source| LookupTraceSourceInput {
                reference: &source.reference,
                source_kind: source_kind_text(source.source),
                source_id: &source.source_id,
                version: &source.version,
                state: source_state_text(source.state),
                page: source.page,
            })
            .collect::<Vec<_>>();
        let created_at = Utc::now().to_rfc3339();
        let input = LookupTraceInput {
            turn_id: &trace.turn_id,
            run_id: &trace.run_id,
            node_id: &trace.node_id,
            request_id: &trace.request_id,
            tool: tool_name_text(trace.trace.tool),
            retrieved_bytes: trace.retrieved_bytes,
            returned_bytes: trace.trace.returned_bytes,
            classification: classification_text(trace.trace.classification),
            delivery: delivery_text(trace.delivery),
            created_at: &created_at,
            sources: &sources,
        };
        self.db
            .with_conn(|conn| insert_lookup_trace(conn, &input).map(|_| ()))
            .map_err(|error| {
                NativeRuntimeError::new(
                    NativeErrorCode::ProviderUnavailable,
                    format!("memory lookup trace persistence failed: {error}"),
                    true,
                )
            })
    }
}

fn tool_name_text(tool: LookupToolName) -> &'static str {
    match tool {
        LookupToolName::MemorySearch => "memory.search",
        LookupToolName::MemoryGet => "memory.get",
        LookupToolName::HistorySearch => "history.search",
        LookupToolName::HistoryGet => "history.get",
    }
}

fn source_kind_text(source: LookupSourceKind) -> &'static str {
    match source {
        LookupSourceKind::Memory => "memory",
        LookupSourceKind::RunStep => "run_step",
        LookupSourceKind::Evidence => "evidence",
    }
}

fn source_state_text(state: LookupSourceState) -> &'static str {
    match state {
        LookupSourceState::Current => "current",
        LookupSourceState::Historical => "historical",
        LookupSourceState::Incomplete => "incomplete",
        LookupSourceState::Unavailable => "unavailable",
        LookupSourceState::Deleted => "deleted",
        LookupSourceState::Expired => "expired",
        LookupSourceState::Retracted => "retracted",
        LookupSourceState::Replaced => "replaced",
        LookupSourceState::Unlinked => "unlinked",
    }
}

fn classification_text(classification: LookupTraceClassification) -> &'static str {
    match classification {
        LookupTraceClassification::RetrievedData => "retrieved_data",
    }
}

fn delivery_text(delivery: NativeMemoryLookupDelivery) -> &'static str {
    match delivery {
        NativeMemoryLookupDelivery::Delivered => "delivered",
        NativeMemoryLookupDelivery::RejectedNativeOutputLimit => "rejected_native_output_limit",
        NativeMemoryLookupDelivery::RejectedContextBudget => "rejected_context_budget",
        NativeMemoryLookupDelivery::RejectedInvalidJson => "rejected_invalid_json",
        NativeMemoryLookupDelivery::RejectedCancelled => "rejected_cancelled",
        NativeMemoryLookupDelivery::RejectedTimedOut => "rejected_timeout",
    }
}
