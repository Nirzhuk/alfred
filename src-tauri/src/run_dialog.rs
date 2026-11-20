//! Dedicated window for answering a workflow's `{{ask:…}}` placeholders.
//!
//! The main window opens it instead of blocking the canvas with an in-app
//! modal. It loads the workflow itself, so the answers always match the graph
//! that was saved just before the Run button was pressed. Submitting hands the
//! answers back to the main window, which owns run state and the event stream.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const WINDOW_LABEL: &str = "run-dialog";
const WINDOW_TITLE: &str = "Run workflow";
const WINDOW_WIDTH: f64 = 560.0;
const WINDOW_HEIGHT: f64 = 680.0;
const MIN_WIDTH: f64 = 420.0;
const MIN_HEIGHT: f64 = 420.0;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubmitPayload<'a> {
    workflow_id: &'a str,
    answers: &'a HashMap<String, String>,
}

fn dialog_url(workflow_id: &str) -> String {
    // Workflow ids are UUIDs, but encoding keeps any future id from splitting
    // the query string or escaping the app origin.
    let encoded: String = workflow_id
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect();
    format!("index.html?runDialog={encoded}")
}

fn open(app: &AppHandle, workflow_id: &str) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        // Reusing the window keeps one dialog in the OS window list. The
        // frontend reloads when the workflow differs from the one on screen.
        window.emit("run-dialog://open", workflow_id)?;
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        WebviewUrl::App(dialog_url(workflow_id).into()),
    )
    .title(WINDOW_TITLE)
    .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
    .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
    .resizable(true)
    .center()
    .build()?;
    window.set_focus()?;
    Ok(())
}

/// Opens (or focuses) the run dialog for a workflow.
#[tauri::command]
pub fn open_run_dialog(app: AppHandle, workflow_id: String) -> Result<(), String> {
    open(&app, &workflow_id).map_err(|error| error.to_string())
}

/// Hands submitted answers to the main window and brings Alfred forward.
#[tauri::command]
pub fn submit_run_dialog(
    app: AppHandle,
    workflow_id: String,
    answers: HashMap<String, String>,
) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "Alfred's main window is unavailable.".to_string())?;
    main.emit(
        "run-dialog://submit",
        SubmitPayload {
            workflow_id: &workflow_id,
            answers: &answers,
        },
    )
    .map_err(|error| error.to_string())?;
    // The run has already been handed off; a window that fails to come forward
    // must not turn that into an error the run dialog would report.
    let _ = main.show();
    let _ = main.unminimize();
    let _ = main.set_focus();
    Ok(())
}
