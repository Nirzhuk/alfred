//! macOS menu-bar (and desktop tray) icon for Alfred.
//!
//! Menu + title reflect active runs and upcoming schedules.

use crate::agents::active;
use crate::db::{parse_rfc3339, Db};
use crate::runner::{self, RunTrigger};
use chrono::{Local, Utc};
use tauri::{
    image::Image,
    menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime, Wry,
};

const TRAY_ID: &str = "alfred-tray";
const MAX_ACTIVE_ROWS: usize = 5;
const MAX_SCHEDULE_ROWS: usize = 5;

pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn truncate(name: &str, max: usize) -> String {
    let trimmed = name.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

fn format_relative(next: chrono::DateTime<Utc>) -> String {
    let secs = next.signed_duration_since(Utc::now()).num_seconds();
    if secs <= 0 {
        return "due".into();
    }
    let mins = (secs + 59) / 60;
    if mins < 60 {
        format!("{mins}m")
    } else if mins < 60 * 24 {
        format!("{}h", mins / 60)
    } else {
        format!("{}d", mins / (60 * 24))
    }
}

fn format_when(next: chrono::DateTime<Utc>) -> String {
    let local = next.with_timezone(&Local);
    let today = Local::now().date_naive();
    if local.date_naive() == today {
        local.format("%H:%M").to_string()
    } else {
        local.format("%a %H:%M").to_string()
    }
}

struct TraySnapshot {
    active: Vec<active::ActiveRun>,
    upcoming: Vec<(String, String, chrono::DateTime<Utc>)>, // workflow_id, name, next
}

fn snapshot(app: &AppHandle) -> TraySnapshot {
    let active = active::list_active();
    let mut upcoming = Vec::new();
    if let Some(db) = app.try_state::<Db>() {
        if let Ok(schedules) = db.list_enabled_schedules() {
            for schedule in schedules {
                let Some(next) = schedule.next_run_at.as_deref().and_then(parse_rfc3339) else {
                    continue;
                };
                let name = db
                    .get_workflow(&schedule.workflow_id)
                    .ok()
                    .flatten()
                    .map(|w| w.name)
                    .unwrap_or_else(|| "Workflow".into());
                upcoming.push((schedule.workflow_id, name, next));
            }
        }
    }
    upcoming.sort_by_key(|(_, _, next)| *next);
    upcoming.truncate(MAX_SCHEDULE_ROWS);
    TraySnapshot { active, upcoming }
}

fn status_title(snap: &TraySnapshot) -> String {
    if !snap.active.is_empty() {
        if snap.active.len() == 1 {
            return "●".into();
        }
        return snap.active.len().to_string();
    }
    if let Some((_, _, next)) = snap.upcoming.first() {
        let mins = next.signed_duration_since(Utc::now()).num_minutes();
        if mins <= 60 * 24 {
            return format_relative(*next);
        }
    }
    String::new()
}

fn status_tooltip(app: &AppHandle, snap: &TraySnapshot) -> String {
    let mut lines = vec![app.config().product_name.as_deref().unwrap_or("Alfred").to_string()];
    if snap.active.is_empty() {
        lines.push("No active runs".into());
    } else {
        lines.push(format!(
            "{} active run{}",
            snap.active.len(),
            if snap.active.len() == 1 { "" } else { "s" }
        ));
        for run in snap.active.iter().take(MAX_ACTIVE_ROWS) {
            lines.push(format!("• {}", truncate(&run.workflow_name, 40)));
        }
    }
    if let Some((_, name, next)) = snap.upcoming.first() {
        lines.push(format!(
            "Next: {} — {} ({})",
            truncate(name, 28),
            format_when(*next),
            format_relative(*next)
        ));
    } else {
        lines.push("No upcoming schedules".into());
    }
    lines.join("\n")
}

fn status_icon() -> tauri::Result<Image<'static>> {
    // Native tray APIs consume raster images; this is the menu-bar rendition
    // of public/icon-mustache.svg.
    Image::from_bytes(include_bytes!("../icons/trayMustacheTemplate@2x.png"))
}

fn build_menu(app: &AppHandle, snap: &TraySnapshot) -> tauri::Result<Menu<Wry>> {
    let product_name = app.config().product_name.as_deref().unwrap_or("Alfred");
    let open = MenuItem::with_id(
        app,
        "tray-open",
        format!("Open {product_name}"),
        true,
        None::<&str>,
    )?;
    let quick_access = MenuItem::with_id(
        app,
        "tray-quick-access",
        "Open Quick Access",
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "tray-settings", "Settings", true, None::<&str>)?;
    let download_latest = MenuItem::with_id(
        app,
        "tray-download-latest",
        "Check for Updates…",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&open, &quick_access, &sep1];

    // Keep owned items alive for the Menu::with_items call.
    let mut owned: Vec<MenuItem<Wry>> = Vec::new();

    let running_header = MenuItem::with_id(
        app,
        "tray-hdr-running",
        if snap.active.is_empty() {
            "Running — none"
        } else {
            "Running"
        },
        false,
        None::<&str>,
    )?;
    owned.push(running_header);

    if snap.active.is_empty() {
        owned.push(MenuItem::with_id(
            app,
            "tray-running-empty",
            "No active runs",
            false,
            None::<&str>,
        )?);
    } else {
        for run in snap.active.iter().take(MAX_ACTIVE_ROWS) {
            owned.push(MenuItem::with_id(
                app,
                format!("tray-open-wf:{}", run.workflow_id),
                format!("Open “{}”", truncate(&run.workflow_name, 28)),
                true,
                None::<&str>,
            )?);
            owned.push(MenuItem::with_id(
                app,
                format!("tray-stop:{}", run.run_id),
                format!("Stop “{}”", truncate(&run.workflow_name, 28)),
                true,
                None::<&str>,
            )?);
        }
    }

    owned.push(MenuItem::with_id(
        app,
        "tray-hdr-upnext",
        if snap.upcoming.is_empty() {
            "Up next — none"
        } else {
            "Up next"
        },
        false,
        None::<&str>,
    )?);

    if snap.upcoming.is_empty() {
        owned.push(MenuItem::with_id(
            app,
            "tray-upnext-empty",
            "No scheduled workflows",
            false,
            None::<&str>,
        )?);
    } else {
        for (workflow_id, name, next) in &snap.upcoming {
            let label = format!(
                "{} — {} ({})",
                truncate(name, 22),
                format_when(*next),
                format_relative(*next)
            );
            owned.push(MenuItem::with_id(
                app,
                format!("tray-open-wf:{workflow_id}"),
                label,
                true,
                None::<&str>,
            )?);
            owned.push(MenuItem::with_id(
                app,
                format!("tray-run-wf:{workflow_id}"),
                format!("    Run “{}” now", truncate(name, 22)),
                true,
                None::<&str>,
            )?);
        }
    }

    for item in &owned {
        items.push(item);
    }
    items.push(&sep2);
    items.push(&settings);
    items.push(&download_latest);
    items.push(&sep3);
    items.push(&quit);

    Menu::with_items(app, &items)
}

fn apply_snapshot(app: &AppHandle, snap: &TraySnapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    if let Ok(menu) = build_menu(app, snap) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(status_tooltip(app, snap)));
    let _ = tray.set_title(Some(status_title(snap)));

    if let Ok(icon) = status_icon() {
        #[cfg(target_os = "macos")]
        {
            let _ = tray.set_icon_as_template(true);
        }
        let _ = tray.set_icon(Some(icon));
    }
}

/// Rebuild tray menu / title / icon from current runs + schedules.
pub fn refresh(app: &AppHandle) {
    let snap = snapshot(app);
    apply_snapshot(app, &snap);
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "tray-open" => show_main_window(app),
        "tray-quick-access" => {
            if let Err(error) = crate::quick_access::show_expanded(app) {
                eprintln!("quick access could not be opened from tray: {error}");
            }
        }
        "tray-settings" => {
            show_main_window(app);
            let _ = app.emit("app://open-settings", ());
        }
        "tray-download-latest" => {
            show_main_window(app);
            let _ = app.emit("app://download-latest", ());
        }
        "tray-quit" => app.exit(0),
        other if other.starts_with("tray-stop:") => {
            let run_id = &other["tray-stop:".len()..];
            if let Some(db) = app.try_state::<Db>() {
                let _ = runner::cancel_run(db.inner(), run_id);
            }
            refresh(app);
        }
        other if other.starts_with("tray-open-wf:") => {
            let workflow_id = &other["tray-open-wf:".len()..];
            show_main_window(app);
            let _ = app.emit("app://open-workflow", workflow_id);
        }
        other if other.starts_with("tray-run-wf:") => {
            let workflow_id = &other["tray-run-wf:".len()..];
            show_main_window(app);
            if let Some(db) = app.try_state::<Db>() {
                let _ = runner::start_run(
                    app.clone(),
                    db.inner(),
                    workflow_id,
                    RunTrigger::Manual,
                    None,
                );
            }
            let _ = app.emit("app://open-workflow", workflow_id);
            let _ = app.emit("app://open-activity", ());
            refresh(app);
        }
        _ => {}
    }
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let snap = snapshot(app);
    let menu = build_menu(app, &snap)?;
    let icon = status_icon()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(status_tooltip(app, &snap))
        .title(status_title(&snap))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }

    let _tray = builder.build(app)?;
    Ok(())
}
