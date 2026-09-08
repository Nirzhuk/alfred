mod agent_accounts;
mod agents;
mod commands;
mod db;
// Provider-neutral seams are intentionally consumed by follow-on connector plans.
#[allow(dead_code)]
mod integrations;
mod licensing;
#[cfg(target_os = "macos")]
mod macos_titlebar;
#[cfg(target_os = "macos")]
mod native_window_material;
mod notifications;
mod quick_access;
mod runner;
mod scheduler;
mod skills;
mod tray;
mod triggers;

use agent_accounts::{AgentAccountResolver, AgentAccountsState};
use agents::native::{
    DenyAllToolExecutor, HostApprovalBroker, NativeExecutionRouter, NativeRuntimeRegistry,
};
use commands::managed_runtime::ManagedRuntimeControlPlane;
use db::Db;
use integrations::IntegrationsState;
use licensing::LicensingState;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, State, WindowEvent};
use triggers::app::AppEventRuntime;
use triggers::TriggerRuntime;

#[cfg(desktop)]
const QUICK_ACCESS_SHORTCUT: &str = "CmdOrCtrl+Shift+Space";

struct GlobalShortcutPreference(Mutex<String>);

struct StartupTrace(Option<Instant>);

impl StartupTrace {
    fn mark(&self, stage: &str) {
        if let Some(started) = self.0 {
            eprintln!(
                "[alfred-startup] stage={stage} elapsed_ms={}",
                started.elapsed().as_millis()
            );
        }
    }
}

#[tauri::command]
fn main_window_ready(window: tauri::Window, trace: State<'_, StartupTrace>) {
    if window.label() == "main" {
        trace.mark("main_window_ready");
    }
}

impl Default for GlobalShortcutPreference {
    fn default() -> Self {
        Self(Mutex::new(QUICK_ACCESS_SHORTCUT.to_string()))
    }
}

#[tauri::command]
fn set_global_shortcut(
    app: tauri::AppHandle,
    preference: State<'_, GlobalShortcutPreference>,
    shortcut: String,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        let shortcut = shortcut.trim();
        if shortcut.is_empty() {
            return Err("Shortcut cannot be empty".to_string());
        }

        let mut current = preference
            .0
            .lock()
            .map_err(|_| "Global shortcut preference is unavailable".to_string())?;
        if current.as_str() == shortcut {
            if app.global_shortcut().is_registered(shortcut) {
                return Ok(());
            }
            return app
                .global_shortcut()
                .register(shortcut)
                .map_err(|error| error.to_string());
        }

        // Register first so a rejected OS-level shortcut never leaves the user
        // without their previous working binding.
        app.global_shortcut()
            .register(shortcut)
            .map_err(|error| error.to_string())?;

        if let Err(error) = app.global_shortcut().unregister(current.as_str()) {
            let _ = app.global_shortcut().unregister(shortcut);
            return Err(error.to_string());
        }

        *current = shortcut.to_string();
        Ok(())
    }

    #[cfg(not(desktop))]
    {
        let _ = (app, preference, shortcut);
        Err("Global shortcuts are only available on desktop".to_string())
    }
}

#[tauri::command]
fn sync_macos_traffic_lights(window: tauri::Window) {
    #[cfg(target_os = "macos")]
    macos_titlebar::sync_event_window(&window);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

async fn start_background_services(handle: tauri::AppHandle) {
    let plane = Arc::clone(handle.state::<Arc<ManagedRuntimeControlPlane>>().inner());
    handle
        .state::<StartupTrace>()
        .mark("managed_verification_start");
    let event_handle = handle.clone();
    if let Err(error) = plane
        .verify_bundled_packages(move || {
            let _ = event_handle.emit("managed-runtime://products-changed", ());
        })
        .await
    {
        eprintln!("managed runtime verification failed: {error}");
    }
    handle
        .state::<StartupTrace>()
        .mark("managed_verification_complete");

    // Preserve startup ordering for due schedules and event-triggered runs,
    // while allowing the window to open during package verification.
    handle.state::<TriggerRuntime>().mark_ready();
    let app_event_handle = handle.clone();
    // Webhook listener + file watchers. Neither is fatal: manual and
    // scheduled runs still work if a trigger source fails to start.
    match triggers::http::start(handle.clone()) {
        Ok(port) => handle.state::<TriggerRuntime>().set_http_port(port),
        Err(e) => eprintln!("webhook listener not started: {e}"),
    }
    if let Err(e) = triggers::reload(&handle) {
        eprintln!("file triggers not started: {e}");
    }
    handle.state::<StartupTrace>().mark("automation_ready");

    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(20));
        loop {
            ticker.tick().await;
            let Some(db) = handle.try_state::<Db>() else {
                continue;
            };
            match scheduler::tick(&handle, db.inner()) {
                Ok(n) if n > 0 => {
                    let _ = handle.emit("scheduler://fired", n);
                }
                Err(e) => {
                    eprintln!("scheduler tick error: {e}");
                }
                _ => {}
            }
            if let Some(integrations) = handle.try_state::<IntegrationsState>() {
                integrations
                    .refresh
                    .scheduled_health_check(db.inner())
                    .await;
            }
            tray::refresh(&handle);
        }
    });

    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            if app_event_handle
                .try_state::<AppEventRuntime>()
                .is_some_and(|runtime| runtime.is_stopped())
            {
                break;
            }
            if let Err(error) = triggers::app::tick(&app_event_handle).await {
                eprintln!("app event runtime tick error: {error}");
            }
        }
    });
}

pub fn run() {
    let startup_trace =
        StartupTrace(std::env::var_os("ALFRED_STARTUP_TRACE").map(|_| Instant::now()));
    startup_trace.mark("run_enter");
    let database = Db::open().expect("failed to open sqlite database");

    // Native-agent accounts and runtimes. The registry stays empty until a
    // provider plan registers one; until then Alfred-harness steps surface
    // `native_runtime_unavailable` and never fall back to a CLI adapter.
    let agent_accounts = AgentAccountsState::default();
    let managed_runtime = Arc::new(ManagedRuntimeControlPlane::new());
    // Register all managed product handlers for diagnostics and lifecycle
    // ownership.  Each handler remains explicitly gate-blocked until its
    // provider package, profile, auth, and smoke evidence is complete.
    for handler in managed_runtime.provider_handlers() {
        let _ = agent_accounts.register(handler);
    }
    let native_registry = Arc::new(NativeRuntimeRegistry::default());
    let host_broker = Arc::new(HostApprovalBroker::new());
    // The resolver needs its own handle because the managed `Db` is owned by
    // Tauri state; SQLite is safe to open twice against the same file.
    let resolver_db = Arc::new(Db::open().expect("failed to open sqlite database"));
    let native_router = NativeExecutionRouter::new(
        Arc::clone(&native_registry),
        Arc::new(AgentAccountResolver::new(resolver_db, &agent_accounts)),
        // Alfred-executed tools stay deny-all. OpenCode decides via the host
        // broker and executes inside the isolated runtime.
        Arc::new(DenyAllToolExecutor),
        Arc::clone(&host_broker) as Arc<dyn agents::native::AlfredApprovalHandler>,
    );

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(database)
        .manage(startup_trace)
        .manage(agent_accounts)
        .manage(Arc::clone(&managed_runtime))
        .manage(Arc::clone(&native_registry))
        .manage(Arc::clone(&host_broker))
        .manage(native_router)
        .manage(IntegrationsState::default())
        .manage(LicensingState::default())
        .manage(notifications::NotificationPreferences::default())
        .manage(GlobalShortcutPreference::default())
        .manage(TriggerRuntime::default())
        .manage(AppEventRuntime::default());

    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::ShortcutState;
        use tauri_plugin_window_state::StateFlags;

        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }))
            .plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            if let Err(error) = quick_access::show_expanded(app) {
                                eprintln!("global shortcut could not open quick access: {error}");
                            }
                        }
                    })
                    .build(),
            )
            .plugin(
                tauri_plugin_window_state::Builder::default()
                    .with_state_flags(
                        StateFlags::SIZE
                            | StateFlags::POSITION
                            | StateFlags::MAXIMIZED
                            | StateFlags::FULLSCREEN,
                    )
                    .with_denylist(&[quick_access::WINDOW_LABEL])
                    .build(),
            );
    }

    builder
        .setup(|app| {
            app.state::<StartupTrace>().mark("setup_enter");
            if let Some(root) = app.path().app_data_dir().ok() {
                let resource_root = app.path().resource_dir().ok();
                let plane = app.state::<Arc<ManagedRuntimeControlPlane>>();
                if let Err(error) = plane.initialize(&root, resource_root.as_deref()) {
                    eprintln!("managed runtime state unavailable: {error}");
                }
                if let Err(error) = plane.bind_native_collaborators(
                    Arc::clone(app.state::<Arc<NativeRuntimeRegistry>>().inner()),
                    Arc::clone(app.state::<Arc<HostApprovalBroker>>().inner()),
                ) {
                    eprintln!("managed runtime collaborators unavailable: {error}");
                }
            }
            let approval_handle = app.handle().clone();
            app.state::<Arc<HostApprovalBroker>>().set_listener(Arc::new(
                move |prompt: agents::native::HostApprovalPrompt| {
                    let _ = approval_handle.emit("native://approval-requested", prompt);
                },
            ));
            let resource_root = app.path().resource_dir().ok();
            let capability_manifest = agents::capability_manifest::manifest_for_resource_root(
                agents::capability_manifest::current_platform(),
                agents::capability_manifest::current_build_kind(),
                resource_root.as_deref(),
            );
            if !capability_manifest.is_valid() {
                return Err("agent capability manifest is invalid".into());
            }
            app.manage(capability_manifest);

            #[cfg(target_os = "macos")]
            {
                notifications::prepare_notification_identity(app.handle());
                if let Some(window) = app.get_webview_window("main") {
                    if let Err(error) = native_window_material::install(&window) {
                        eprintln!("native sidebar material could not be applied: {error}");
                    }
                    macos_titlebar::sync_main_window_after_layout(&window);
                }
            }

            // Blocks WhatsApp protocol logging before any client exists. The
            // crate prints raw identity and Signal key material through `log`,
            // including at WARN, so this must be installed first.
            if let Err(error) = integrations::whatsapp::log_guard::install_silent() {
                eprintln!("whatsapp log guard not installed: {error}");
            }

            let handle = app.handle().clone();

            // One WhatsApp runtime for the process lifetime, only when an
            // account is already linked. Never blocks startup.
            {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let (Some(db), Some(integrations)) = (
                        handle.try_state::<Db>(),
                        handle.try_state::<IntegrationsState>(),
                    ) else {
                        return;
                    };
                    integrations.whatsapp.start_stored_runtime(db.inner()).await;
                });
            }

            if let Err(e) = tray::install(app.handle()) {
                eprintln!("tray icon not started: {e}");
            }
            // The window starts hidden to avoid a font flash. If the webview
            // never boots (Vite not up, import error), bun dev would otherwise
            // look like a terminal with no app. Reveal once after a short wait.
            let reveal_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                if let Some(window) = reveal_handle.get_webview_window("main") {
                    let _ = window.show();
                    reveal_handle
                        .state::<StartupTrace>()
                        .mark("main_window_fallback_reveal");
                }
            });
            // Quick Access is NOT built here. Its webview is a second
            // WebContent process (~48 MB), and most of that was paid by users
            // who keep the feature off. `set_quick_access_enabled`, pushed by
            // the frontend moments after the main window boots, builds it when
            // the preference is on; the shortcut and tray build it on demand.

            // Return the cached snapshot through `get_license_status` without
            // a startup network dependency, then refresh stale grants in the
            // background under the licensing single-flight lock.
            let license_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let Some(db) = license_handle.try_state::<Db>() else {
                    return;
                };
                let Some(licensing) = license_handle.try_state::<LicensingState>() else {
                    return;
                };
                if licensing.should_refresh(db.inner()) {
                    let _ = licensing.refresh(db.inner()).await;
                }
            });

            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;

                let shortcut = app
                    .state::<GlobalShortcutPreference>()
                    .0
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_else(|_| QUICK_ACCESS_SHORTCUT.to_string());
                if let Err(error) = app.global_shortcut().register(shortcut.as_str()) {
                    eprintln!("global quick-access shortcut {shortcut} is unavailable: {error}");
                }
            }

            let background_handle = app.handle().clone();
            tauri::async_runtime::spawn(start_background_services(background_handle));
            app.state::<StartupTrace>().mark("setup_complete");
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Alfred running in the menu bar / tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
            #[cfg(target_os = "macos")]
            match event {
                WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Focused(_)
                | WindowEvent::ThemeChanged(_) => {
                    macos_titlebar::sync_event_window(window);
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            main_window_ready,
            commands::activate_license,
            commands::refresh_license,
            commands::deactivate_license,
            commands::get_license_status,
            commands::agent_accounts::list_agent_account_providers,
            commands::agent_accounts::list_agent_accounts,
            commands::agent_accounts::get_agent_account,
            commands::agent_accounts::connect_agent_api_key_account,
            commands::agent_accounts::start_agent_authorization,
            commands::agent_accounts::complete_agent_authorization,
            commands::agent_accounts::cancel_agent_authorization,
            commands::agent_accounts::refresh_agent_account,
            commands::agent_accounts::disconnect_agent_account,
            commands::github_cli::github_cli_repos,
            commands::github_cli::github_cli_branches,
            commands::integrations::list_app_providers,
            commands::integrations::list_app_action_descriptors,
            commands::integrations::list_app_action_resources,
            commands::integrations::list_app_event_descriptors,
            commands::integrations::list_app_event_resources,
            commands::integrations::list_app_connections,
            commands::integrations::get_app_connection,
            commands::integrations::get_app_connection_usage,
            commands::integrations::disconnect_app_connection,
            commands::integrations::connect_slack_private,
            commands::integrations::prepare_github_connection,
            commands::integrations::poll_github_connection,
            commands::integrations::cancel_github_pairing,
            commands::integrations::prepare_gmail_connection,
            commands::integrations::complete_gmail_connection,
            commands::integrations::cancel_gmail_authorization,
            commands::integrations::prepare_microsoft_connection,
            commands::integrations::complete_microsoft_connection,
            commands::integrations::cancel_microsoft_authorization,
            commands::integrations::connect_notion_private,
            commands::integrations::connect_obsidian_vault,
            commands::integrations::connect_linear_private,
            commands::integrations::connect_sentry_private,
            commands::integrations::prepare_telegram_connection,
            commands::integrations::complete_telegram_connection,
            commands::integrations::cancel_telegram_pairing,
            commands::integrations::begin_whatsapp_pairing,
            commands::integrations::whatsapp_pairing_state,
            commands::integrations::send_whatsapp_pairing_test,
            commands::integrations::complete_whatsapp_pairing,
            commands::integrations::cancel_whatsapp_pairing,
            commands::integrations::whatsapp_runtime_status,
            commands::integrations::reconnect_whatsapp_runtime,
            commands::list_workflows,
            commands::get_workflow,
            commands::create_workflow,
            commands::update_workflow,
            commands::delete_workflow,
            commands::reorder_workflows,
            commands::list_workflow_folders,
            commands::create_workflow_folder,
            commands::rename_workflow_folder,
            commands::delete_workflow_folder,
            commands::reorder_workflow_folders,
            commands::move_workflow_to_folder,
            commands::list_agent_providers,
            commands::get_agent_capability_manifest,
            commands::get_agent_harness_diagnostics,
            commands::managed_runtime::list_managed_runtime_products,
            commands::managed_runtime::prepare_managed_runtime_product,
            commands::managed_runtime::start_managed_runtime_connection,
            commands::managed_runtime::connect_managed_runtime_api_key,
            commands::managed_runtime::managed_runtime_connection_status,
            commands::managed_runtime::read_managed_runtime_terminal,
            commands::managed_runtime::write_managed_runtime_terminal,
            commands::managed_runtime::resize_managed_runtime_terminal,
            commands::managed_runtime::close_managed_runtime_terminal,
            commands::managed_runtime::resolve_native_approval,
            commands::list_agent_models,
            commands::get_agent_usage,
            commands::list_skills,
            commands::run_workflow,
            commands::cancel_run,
            commands::list_active_runs,
            commands::list_run_history,
            commands::get_run_history,
            commands::search_history,
            commands::list_schedules,
            commands::get_workflow_schedule,
            commands::upsert_workflow_schedule,
            commands::delete_workflow_schedule,
            commands::list_workflow_triggers,
            commands::list_app_trigger_statuses,
            commands::upsert_workflow_trigger,
            commands::delete_workflow_trigger,
            commands::test_workflow_trigger,
            commands::webhook_base_url,
            commands::list_memories,
            commands::list_linkable_memories,
            commands::link_memory,
            commands::unlink_memory,
            commands::create_memory,
            commands::update_memory,
            commands::delete_memory,
            commands::clear_memories,
            commands::get_memory_review_settings,
            commands::update_memory_review_settings,
            commands::set_workflow_memory_review,
            commands::list_memory_candidates,
            commands::update_memory_candidate,
            commands::approve_memory_candidate,
            commands::reject_memory_candidate,
            commands::retry_memory_review,
            commands::get_memory_review_job,
            commands::list_memory_reviews,
            commands::get_workflow_memory_review,
            commands::clear_decided_memory_candidates,
            quick_access::show_quick_access,
            quick_access::set_quick_access_expanded,
            quick_access::set_quick_access_enabled,
            quick_access::set_quick_access_mode,
            quick_access::set_quick_access_always_on_top,
            quick_access::set_quick_access_fullscreen,
            quick_access::open_quick_access_target,
            notifications::set_notification_sound_cmd,
            notifications::notify_message_cmd,
            notifications::notify_run_finished_cmd,
            set_global_shortcut,
            sync_macos_traffic_lights,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // macOS dock-icon click while the window is hidden sends Reopen,
            // not a fresh launch, so single-instance's re-show never fires.
            // Orderly exit: stop the WhatsApp client and flush protocol state.
            // Alfred sends nothing while it is not running.
            if let tauri::RunEvent::Exit = event {
                if let Some(integrations) = app_handle.try_state::<IntegrationsState>() {
                    tauri::async_runtime::block_on(integrations.whatsapp.shutdown_runtime());
                }
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
}
