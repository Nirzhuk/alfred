pub mod agent_accounts;
pub mod github_cli;
pub mod integrations;
pub mod managed_runtime;

use crate::agents;
use crate::db::{
    CreateMemoryInput, CreateWorkflowInput, Db, HistorySearchHit, HistorySearchInput,
    ListMemoryCandidatesInput, Memory, MemoryCandidate, MemoryReviewJob, MemoryReviewSettings,
    MemoryWithOrigin, RunHistoryDetail, RunHistoryItem, Schedule, ScheduleListItem, Trigger,
    UpdateMemoryCandidateInput, UpdateMemoryInput, UpdateMemoryReviewSettingsInput,
    UpdateWorkflowInput, UpsertTriggerInput, Workflow, WorkflowFolder, WorkflowMemoryReview,
};
use crate::integrations::events::{
    AppTriggerConfig, NormalizedAppEvent, NORMALIZED_APP_EVENT_SCHEMA_VERSION,
};
use crate::integrations::IntegrationsState;
use crate::licensing::{LicenseCommandError, LicenseStatusDto, LicensingState};
use crate::runner::{self, RunSummary, RunTrigger};
use crate::scheduler;
use crate::skills::{self, SkillRef};
use crate::triggers::{self, TriggerRuntime};
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub async fn activate_license(
    db: State<'_, Db>,
    licensing: State<'_, LicensingState>,
    license_key: String,
    device_label: String,
) -> Result<LicenseStatusDto, LicenseCommandError> {
    licensing
        .activate(db.inner(), license_key, device_label)
        .await
}

#[tauri::command]
pub async fn refresh_license(
    db: State<'_, Db>,
    licensing: State<'_, LicensingState>,
) -> Result<LicenseStatusDto, LicenseCommandError> {
    licensing.refresh(db.inner()).await
}

#[tauri::command]
pub async fn deactivate_license(
    db: State<'_, Db>,
    licensing: State<'_, LicensingState>,
) -> Result<LicenseStatusDto, LicenseCommandError> {
    licensing.deactivate(db.inner()).await
}

#[tauri::command]
pub fn get_license_status(
    db: State<'_, Db>,
    licensing: State<'_, LicensingState>,
) -> Result<LicenseStatusDto, LicenseCommandError> {
    // This command is intentionally local-only; it never waits for Polar.
    licensing.get_status(db.inner())
}

#[tauri::command]
pub fn list_workflows(db: State<'_, Db>) -> Result<Vec<Workflow>, String> {
    let mut workflows = db.list_workflows().map_err(|e| e.to_string())?;
    for workflow in &mut workflows {
        agents::normalize_agent_nodes_in_graph(&mut workflow.graph)
            .map_err(|error| error.to_string())?;
    }
    Ok(workflows)
}

#[tauri::command]
pub fn get_workflow(db: State<'_, Db>, id: String) -> Result<Option<Workflow>, String> {
    let mut workflow = db.get_workflow(&id).map_err(|e| e.to_string())?;
    if let Some(workflow) = &mut workflow {
        agents::normalize_agent_nodes_in_graph(&mut workflow.graph)
            .map_err(|error| error.to_string())?;
    }
    Ok(workflow)
}

#[tauri::command]
pub fn create_workflow(
    db: State<'_, Db>,
    mut input: CreateWorkflowInput,
) -> Result<Workflow, String> {
    agents::normalize_agent_nodes_in_graph(&mut input.graph).map_err(|error| error.to_string())?;
    db.create_workflow(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_workflow(
    db: State<'_, Db>,
    mut input: UpdateWorkflowInput,
) -> Result<Workflow, String> {
    if let Some(graph) = &mut input.graph {
        agents::normalize_agent_nodes_in_graph(graph).map_err(|error| error.to_string())?;
    }
    db.update_workflow(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_workflow(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_workflow(&id).map_err(|e| e.to_string())
}

/// Reorder workflows in the sidebar (`orderedIds` is top → bottom).
#[tauri::command]
pub fn reorder_workflows(db: State<'_, Db>, ordered_ids: Vec<String>) -> Result<(), String> {
    db.reorder_workflows(&ordered_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_workflow_folders(db: State<'_, Db>) -> Result<Vec<WorkflowFolder>, String> {
    db.list_workflow_folders().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_workflow_folder(db: State<'_, Db>, name: String) -> Result<WorkflowFolder, String> {
    db.create_workflow_folder(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_workflow_folder(
    db: State<'_, Db>,
    id: String,
    name: String,
) -> Result<WorkflowFolder, String> {
    db.rename_workflow_folder(&id, &name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_workflow_folder(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_workflow_folder(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_workflow_folders(db: State<'_, Db>, ordered_ids: Vec<String>) -> Result<(), String> {
    db.reorder_workflow_folders(&ordered_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn move_workflow_to_folder(
    db: State<'_, Db>,
    workflow_id: String,
    folder_id: Option<String>,
) -> Result<Workflow, String> {
    db.move_workflow_to_folder(&workflow_id, folder_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_agent_providers(
    manifest: State<'_, agents::capability_manifest::AgentCapabilityManifest>,
    harness: Option<agents::AgentHarness>,
) -> Vec<serde_json::Value> {
    agents::list_providers(manifest.inner(), harness)
}

/// One versioned source for runner gates and UI availability. Missing entries
/// are disabled by the consumer; this command never probes provider CLIs.
#[tauri::command]
pub fn get_agent_capability_manifest(
    manifest: State<'_, agents::capability_manifest::AgentCapabilityManifest>,
) -> agents::capability_manifest::AgentCapabilityManifest {
    manifest.inner().clone()
}

/// Bounded support snapshot. It intentionally excludes prompts, outputs,
/// provider payloads, credential references, raw identities, and file paths.
#[tauri::command]
pub fn get_agent_harness_diagnostics(
    db: State<'_, Db>,
    manifest: State<'_, agents::capability_manifest::AgentCapabilityManifest>,
    native_registry: State<'_, std::sync::Arc<agents::native::NativeRuntimeRegistry>>,
    provider: Option<agents::AgentProvider>,
    harness: Option<agents::AgentHarness>,
) -> Result<agents::capability_manifest::AgentHarnessDiagnostics, String> {
    let account_list = db
        .list_agent_accounts_with_diagnostics()
        .map_err(|_| "agent_harness_diagnostics_unavailable".to_string())?;
    Ok(agents::capability_manifest::support_diagnostics(
        manifest.inner(),
        &account_list.accounts,
        &account_list.diagnostics,
        native_registry.inner().as_ref(),
        provider,
        harness,
    ))
}

/// Return one harness-scoped catalog per provider. Omission preserves the
/// historical CLI-only response; native-only requests never probe a CLI.
#[tauri::command]
pub async fn list_agent_models(
    manifest: State<'_, agents::capability_manifest::AgentCapabilityManifest>,
    harness: Option<agents::AgentHarness>,
) -> Result<Vec<agents::ProviderModels>, String> {
    let manifest = manifest.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        agents::models::list_provider_models_with_manifest(harness, &manifest)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Read subscription quota windows from each provider's own local CLI.
#[tauri::command]
pub async fn get_agent_usage(
    provider_ids: Vec<String>,
) -> Result<Vec<agents::AgentUsageSnapshot>, String> {
    let providers: Vec<agents::AgentProvider> = provider_ids
        .iter()
        .filter_map(|provider| agents::AgentProvider::from_str(provider))
        .collect();
    tauri::async_runtime::spawn_blocking(move || agents::list_provider_usage(&providers))
        .await
        .map_err(|error| error.to_string())
}

/// Discover SKILL.md packages from project + user skill directories.
#[tauri::command]
pub fn list_skills(project_root: Option<String>) -> Result<Vec<SkillRef>, String> {
    skills::list_skills(project_root.as_deref()).map_err(|e| e.to_string())
}

/// Manually trigger a workflow automation and stream run events to the UI.
#[tauri::command]
pub fn run_workflow(
    app: AppHandle,
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<RunSummary, String> {
    runner::start_run(app, &db, &workflow_id, RunTrigger::Manual, None).map_err(|e| e.to_string())
}

/// Stop an in-flight run (kills the CLI child process).
#[tauri::command]
pub fn cancel_run(app: AppHandle, db: State<'_, Db>, run_id: String) -> Result<bool, String> {
    let cancelled = runner::cancel_run(&db, &run_id).map_err(|e| e.to_string())?;
    crate::tray::refresh(&app);
    Ok(cancelled)
}

/// Runs remain alive when the window is hidden, so expose them when the UI reloads.
#[tauri::command]
pub fn list_active_runs() -> Vec<agents::active::ActiveRun> {
    agents::active::list_active()
}

#[tauri::command]
pub fn list_run_history(
    db: State<'_, Db>,
    workflow_id: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<RunHistoryItem>, String> {
    db.list_run_history(
        workflow_id.as_deref(),
        limit.unwrap_or(25),
        offset.unwrap_or(0),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_run_history(
    db: State<'_, Db>,
    run_id: String,
) -> Result<Option<RunHistoryDetail>, String> {
    db.get_run_history(&run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn search_history(
    db: State<'_, Db>,
    input: HistorySearchInput,
) -> Result<Vec<HistorySearchHit>, String> {
    db.search_history(input).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_schedules(db: State<'_, Db>) -> Result<Vec<ScheduleListItem>, String> {
    db.list_all_schedules().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_workflow_schedule(
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<Option<Schedule>, String> {
    db.get_schedule_for_workflow(&workflow_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_workflow_schedule(
    app: AppHandle,
    db: State<'_, Db>,
    workflow_id: String,
    cron: String,
    enabled: bool,
) -> Result<Schedule, String> {
    let schedule =
        scheduler::upsert_schedule(&db, workflow_id, cron, enabled).map_err(|e| e.to_string())?;
    crate::tray::refresh(&app);
    let _ = app.emit("schedules://changed", ());
    Ok(schedule)
}

#[tauri::command]
pub fn delete_workflow_schedule(
    app: AppHandle,
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<(), String> {
    db.delete_schedule_for_workflow(&workflow_id)
        .map_err(|e| e.to_string())?;
    crate::tray::refresh(&app);
    let _ = app.emit("schedules://changed", ());
    Ok(())
}

#[tauri::command]
pub fn list_workflow_triggers(
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<Vec<Trigger>, String> {
    db.list_triggers(&workflow_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_app_trigger_statuses(
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<Vec<crate::db::AppTriggerStatus>, String> {
    db.list_app_trigger_statuses(&workflow_id)
        .map_err(|error| error.to_string())
}

/// Create or update a trigger, then rebind the file watchers.
#[tauri::command]
pub fn upsert_workflow_trigger(
    app: AppHandle,
    db: State<'_, Db>,
    integrations: State<'_, IntegrationsState>,
    input: UpsertTriggerInput,
) -> Result<Trigger, String> {
    if input.source == "app" && input.enabled {
        let config: AppTriggerConfig =
            serde_json::from_value(input.config.clone()).map_err(|_| "invalid_input".to_owned())?;
        integrations
            .validate_app_trigger(db.inner(), &config)
            .map_err(|error| error.code.as_str().to_owned())?;
    }
    let trigger = db.upsert_trigger(input).map_err(|e| e.to_string())?;
    triggers::reload(&app)?;
    Ok(trigger)
}

#[tauri::command]
pub fn delete_workflow_trigger(
    app: AppHandle,
    db: State<'_, Db>,
    id: String,
) -> Result<(), String> {
    db.delete_trigger(&id).map_err(|e| e.to_string())?;
    triggers::reload(&app)?;
    Ok(())
}

/// Webhook base URL, e.g. `http://127.0.0.1:8787`. `None` when the listener
/// could not bind (port in use).
#[tauri::command]
pub fn webhook_base_url(runtime: State<'_, TriggerRuntime>) -> Option<String> {
    runtime
        .http_port()
        .map(|port| format!("http://127.0.0.1:{port}"))
}

/// Run a workflow as if its trigger fired — for testing a trigger's payload.
#[tauri::command]
pub fn test_workflow_trigger(
    app: AppHandle,
    db: State<'_, Db>,
    integrations: State<'_, IntegrationsState>,
    id: String,
) -> Result<String, String> {
    let trigger = db
        .get_trigger(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("trigger not found: {id}"))?;

    let payload = if trigger.source == "app" {
        let config: AppTriggerConfig = serde_json::from_value(trigger.config.clone())
            .map_err(|_| "invalid_input".to_owned())?;
        integrations
            .validate_app_trigger(db.inner(), &config)
            .map_err(|error| error.code.as_str().to_owned())?;
        serde_json::to_value(NormalizedAppEvent {
            schema_version: NORMALIZED_APP_EVENT_SCHEMA_VERSION,
            provider_id: config.provider_id,
            event_type: config.event_type,
            connection_id: config.connection_id,
            external_event_id: format!("test-{}", uuid::Uuid::new_v4()),
            occurred_at: chrono::Utc::now().to_rfc3339(),
            subject: Some("Test connected-app event".into()),
            actor: None,
            resource_url: None,
            preview: Some("This is a locally generated test event.".into()),
            attributes: std::collections::BTreeMap::new(),
        })
        .map_err(|_| "event_invalid".to_owned())?
    } else {
        serde_json::json!({
            "source": trigger.source,
            "triggerId": trigger.id,
            "test": true,
        })
    };
    triggers::fire(&app, &db, &trigger, payload)
}

#[tauri::command]
pub fn list_memories(
    db: State<'_, Db>,
    workflow_id: String,
    include_history: Option<bool>,
) -> Result<Vec<MemoryWithOrigin>, String> {
    let context = db.memory_context(&workflow_id).map_err(|e| e.to_string())?;
    db.list_memories_for_context(&context, include_history.unwrap_or(false))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_linkable_memories(
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<Vec<MemoryWithOrigin>, String> {
    db.list_linkable_memories(&workflow_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn link_memory(
    db: State<'_, Db>,
    workflow_id: String,
    memory_id: String,
) -> Result<MemoryWithOrigin, String> {
    db.link_memory(&workflow_id, &memory_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unlink_memory(
    db: State<'_, Db>,
    workflow_id: String,
    memory_id: String,
) -> Result<(), String> {
    db.unlink_memory(&workflow_id, &memory_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_memory(db: State<'_, Db>, input: CreateMemoryInput) -> Result<Memory, String> {
    db.create_memory(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_memory(db: State<'_, Db>, input: UpdateMemoryInput) -> Result<Memory, String> {
    db.update_memory(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_memory(
    db: State<'_, Db>,
    id: String,
    context_workflow_id: Option<String>,
) -> Result<(), String> {
    db.delete_memory_for_context(&id, context_workflow_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_memories(db: State<'_, Db>, workflow_id: String) -> Result<usize, String> {
    db.clear_memories(&workflow_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_memory_review_settings(db: State<'_, Db>) -> Result<MemoryReviewSettings, String> {
    db.get_memory_review_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_memory_review_settings(
    db: State<'_, Db>,
    input: UpdateMemoryReviewSettingsInput,
) -> Result<MemoryReviewSettings, String> {
    db.update_memory_review_settings(input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_workflow_memory_review(
    db: State<'_, Db>,
    workflow_id: String,
    enabled: bool,
) -> Result<WorkflowMemoryReview, String> {
    db.set_workflow_memory_review(&workflow_id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_memory_candidates(
    db: State<'_, Db>,
    input: ListMemoryCandidatesInput,
) -> Result<Vec<MemoryCandidate>, String> {
    db.list_memory_candidates(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_memory_candidate(
    db: State<'_, Db>,
    input: UpdateMemoryCandidateInput,
) -> Result<MemoryCandidate, String> {
    db.update_memory_candidate(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn approve_memory_candidate(
    app: AppHandle,
    db: State<'_, Db>,
    id: String,
) -> Result<MemoryCandidate, String> {
    let candidate = db
        .approve_memory_candidate(&id)
        .map_err(|e| e.to_string())?;
    emit_candidates_changed(&app, &db, &candidate.workflow_id);
    Ok(candidate)
}

#[tauri::command]
pub fn reject_memory_candidate(
    app: AppHandle,
    db: State<'_, Db>,
    id: String,
) -> Result<MemoryCandidate, String> {
    let candidate = db.reject_memory_candidate(&id).map_err(|e| e.to_string())?;
    emit_candidates_changed(&app, &db, &candidate.workflow_id);
    Ok(candidate)
}

#[tauri::command]
pub fn retry_memory_review(
    app: AppHandle,
    db: State<'_, Db>,
    run_id: String,
) -> Result<MemoryReviewJob, String> {
    let job = db.retry_memory_review(&run_id).map_err(|e| e.to_string())?;
    // The reset job is pending; the runner machinery claims and executes it.
    runner::spawn_retry_memory_review(&app, &run_id);
    emit_candidates_changed(&app, &db, &job.workflow_id);
    Ok(job)
}

#[tauri::command]
pub fn get_memory_review_job(
    db: State<'_, Db>,
    run_id: String,
) -> Result<Option<MemoryReviewJob>, String> {
    db.get_memory_review_job(&run_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_memory_reviews(
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<Vec<MemoryReviewJob>, String> {
    db.list_memory_reviews(&workflow_id, 25)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_workflow_memory_review(
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<Option<WorkflowMemoryReview>, String> {
    db.get_workflow_memory_review(&workflow_id)
        .map_err(|e| e.to_string())
}

/// Data-settings action: physically delete decided candidates. Pending
/// suggestions stay; canonical memory is never touched here.
#[tauri::command]
pub fn clear_decided_memory_candidates(
    app: AppHandle,
    db: State<'_, Db>,
    workflow_id: String,
) -> Result<usize, String> {
    let cleared = db
        .clear_decided_memory_candidates(&workflow_id)
        .map_err(|e| e.to_string())?;
    if cleared > 0 {
        emit_candidates_changed(&app, &db, &workflow_id);
    }
    Ok(cleared)
}

/// Post-commit notification for the Suggestions queue. Carries only the
/// workflow id and pending count — never candidate text or provider output.
fn emit_candidates_changed(app: &AppHandle, db: &State<'_, Db>, workflow_id: &str) {
    let pending = db.count_pending_memory_candidates(workflow_id).unwrap_or(0);
    let _ = app.emit(
        "memory://candidates-changed",
        serde_json::json!({ "workflowId": workflow_id, "pendingCount": pending }),
    );
}
