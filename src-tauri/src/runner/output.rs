use crate::db::{
    Db, OutputArtifactStore, OutputClaimMode, OutputPersistenceEvent, OutputPersistenceEventSink,
    OutputReceipt, OutputReceiptKey,
};
use tauri::{AppHandle, Emitter};

pub(crate) const OUTPUT_PERSISTENCE_EVENT: &str = "output-persistence://receipt";
const RECOVERY_BATCH_SIZE: usize = 32;

struct TauriOutputPersistenceEventSink<'a> {
    app: &'a AppHandle,
}

impl OutputPersistenceEventSink for TauriOutputPersistenceEventSink<'_> {
    fn emit(&self, event: &OutputPersistenceEvent) -> Result<(), String> {
        self.app
            .emit(OUTPUT_PERSISTENCE_EVENT, event)
            .map_err(|error| error.to_string())
    }
}

fn artifact_store(db: &Db) -> OutputArtifactStore {
    db.output_artifact_store()
}

pub(crate) fn persist_completed_output(app: &AppHandle, db: &Db, run_id: &str, node_id: &str) {
    let key = match OutputReceiptKey::new(run_id, node_id) {
        Ok(key) => key,
        Err(error) => {
            eprintln!("invalid completed output receipt key: {error}");
            return;
        }
    };
    let artifacts = artifact_store(db);
    let sink = TauriOutputPersistenceEventSink { app };
    if let Err(error) =
        db.persist_output_receipt(&key, OutputClaimMode::Initial, &artifacts, Some(&sink))
    {
        eprintln!(
            "completed output persistence failed for {}/{}: {error}",
            key.run_id(),
            key.node_id()
        );
        match db.requeue_output_recovery_item(&key) {
            Ok(true) => eprintln!(
                "deferred completed output persistence recovery for {}/{}",
                key.run_id(),
                key.node_id()
            ),
            Ok(false) => {}
            Err(requeue_error) => eprintln!(
                "failed to defer completed output persistence for {}/{}: {requeue_error}",
                key.run_id(),
                key.node_id()
            ),
        }
    }
}

pub(crate) fn retry_output_persistence(
    app: &AppHandle,
    db: &Db,
    run_id: &str,
    node_id: &str,
) -> Result<OutputReceipt, String> {
    let key = OutputReceiptKey::new(run_id, node_id).map_err(|error| error.to_string())?;
    let artifacts = artifact_store(db);
    let sink = TauriOutputPersistenceEventSink { app };
    let outcome = match db.persist_output_receipt(
        &key,
        OutputClaimMode::ExplicitRetry,
        &artifacts,
        Some(&sink),
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Err(requeue_error) = db.requeue_output_recovery_item(&key) {
                return Err(format!("{error}; failed to defer recovery: {requeue_error}"));
            }
            return Err(error.to_string());
        }
    };
    outcome
        .into_receipt()
        .ok_or_else(|| "output receipt is no longer available".to_string())
}

pub(crate) fn recover_output_persistence(app: &AppHandle, db: &Db) -> Result<usize, String> {
    let artifacts = artifact_store(db);
    let sink = TauriOutputPersistenceEventSink { app };
    loop {
        let cleaned = db
            .process_output_cleanup_journal(&artifacts, RECOVERY_BATCH_SIZE)
            .map_err(|error| {
                eprintln!("output cleanup journal batch failed: {error}");
                error.to_string()
            })?;
        if cleaned < RECOVERY_BATCH_SIZE {
            break;
        }
    }

    let mut processed = 0usize;
    loop {
        let (outcomes, has_more) = db
            .recover_and_persist_output_snapshot_batch(
                RECOVERY_BATCH_SIZE,
                &artifacts,
                Some(&sink),
            )
            .map_err(|error| error.to_string())?;
        processed = processed.saturating_add(outcomes.len());
        if !has_more {
            break;
        }
    }
    Ok(processed)
}
