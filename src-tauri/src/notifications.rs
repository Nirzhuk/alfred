//! Desktop notifications with shared branding, sounds, and click-to-open.
//!
//! The stock Tauri notification plugin cannot report clicks on desktop, so
//! notifications are sent through `notify_rust` and share one native settings
//! state. This also keeps scheduled runs and Notify nodes in sync with the UI.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{path::BaseDirectory, AppHandle, Emitter, Manager, Runtime, State};

const MAX_NOTIFICATION_OPEN_BODY_BYTES: usize = 256 * 1024;
const NOTIFICATION_BODY_TRUNCATED_SUFFIX: &str =
    "\n\n[… output truncated in notification; the complete run remains stored by Alfred]";

fn bounded_notification_body(mut body: String) -> String {
    if body.len() <= MAX_NOTIFICATION_OPEN_BODY_BYTES {
        return body;
    }

    let budget =
        MAX_NOTIFICATION_OPEN_BODY_BYTES.saturating_sub(NOTIFICATION_BODY_TRUNCATED_SUFFIX.len());
    let mut end = budget.min(body.len());
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    body.truncate(end);
    body.push_str(NOTIFICATION_BODY_TRUNCATED_SUFFIX);
    body
}

fn run_notification_copy(workflow_name: &str, ok: bool) -> (String, &'static str) {
    if ok {
        (format!("{workflow_name} finished"), "Click to view output.")
    } else {
        (
            format!("{workflow_name} failed"),
            "Click to view error details.",
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum NotificationSound {
    #[default]
    System,
    Chime,
    Ping,
    Pop,
    None,
}

impl NotificationSound {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "chime" => Some(Self::Chime),
            "ping" => Some(Self::Ping),
            "pop" => Some(Self::Pop),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    #[cfg(target_os = "macos")]
    fn native_name(self) -> Option<&'static str> {
        match self {
            Self::System => Some("default"),
            Self::Chime => Some("Glass"),
            Self::Ping => Some("Ping"),
            Self::Pop => Some("Pop"),
            Self::None => None,
        }
    }

    #[cfg(target_os = "windows")]
    fn native_name(self) -> Option<&'static str> {
        match self {
            Self::System => Some("Default"),
            Self::Chime => Some("Mail"),
            Self::Ping => Some("IM"),
            Self::Pop => Some("SMS"),
            Self::None => Some("Silent"),
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn native_name(self) -> Option<&'static str> {
        match self {
            Self::System => Some("default"),
            Self::Chime => Some("complete"),
            Self::Ping => Some("message-new-instant"),
            Self::Pop => Some("button-pressed"),
            Self::None => None,
        }
    }
}

pub struct NotificationPreferences {
    sound: Mutex<NotificationSound>,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            sound: Mutex::new(NotificationSound::default()),
        }
    }
}

impl NotificationPreferences {
    fn sound(&self) -> NotificationSound {
        *self
            .sound
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_sound(&self, sound: NotificationSound) {
        *self
            .sound
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = sound;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRunOutputPayload {
    pub workflow_id: String,
    pub title: String,
    pub body: String,
    pub ok: bool,
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn selected_sound<R: Runtime>(app: &AppHandle<R>) -> NotificationSound {
    app.try_state::<NotificationPreferences>()
        .map(|preferences| preferences.sound())
        .unwrap_or_default()
}

fn notification_icon_path<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    app.path()
        .resolve("notification-icon.png", BaseDirectory::Resource)
        .ok()
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn configure_notification<R: Runtime>(
    app: &AppHandle<R>,
    notification: &mut notify_rust::Notification,
) {
    notification.appname(app.config().product_name.as_deref().unwrap_or("Alfred"));
    let sound = selected_sound(app);

    #[cfg(all(unix, not(target_os = "macos")))]
    if sound == NotificationSound::None {
        notification.hint(notify_rust::Hint::SuppressSound(true));
    }

    if let Some(name) = sound.native_name() {
        notification.sound_name(name);
    }

    if let Some(icon_path) = notification_icon_path(app) {
        #[cfg(all(unix, not(target_os = "macos")))]
        notification.icon(&icon_path);

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        notification.image_path(&icon_path);
    }

    #[cfg(target_os = "windows")]
    if !tauri::is_dev() {
        notification.app_id(&app.config().identifier);
    }
}

/// Register an Alfred-shaped app bundle for development notifications.
///
/// Tauri normally attributes development notifications to Terminal because the
/// debug executable is not inside an app bundle. Launch Services only needs a
/// registered bundle to provide the correct name and icon, so create a tiny
/// identity bundle in Alfred's local data directory and keep the real app as
/// the notification delegate.
#[cfg(target_os = "macos")]
fn install_development_notification_identity<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let identifier = app.config().identifier.as_str();
    let product_name = app.config().product_name.as_deref().unwrap_or("Alfred");
    let bundle = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("notification-identity")
        .join(format!("{product_name}.app"));
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos).map_err(|error| error.to_string())?;
    fs::create_dir_all(&resources).map_err(|error| error.to_string())?;
    let display_name = product_name
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");

    let version = env!("CARGO_PKG_VERSION");
    let info = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>{display_name}</string>
  <key>CFBundleExecutable</key><string>AlfredNotificationIdentity</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleIdentifier</key><string>{identifier}</string>
  <key>CFBundleName</key><string>{display_name}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSBackgroundOnly</key><true/>
  <key>LSUIElement</key><true/>
</dict>
</plist>
"#
    );
    fs::write(contents.join("Info.plist"), info).map_err(|error| error.to_string())?;
    fs::write(
        resources.join("icon.icns"),
        include_bytes!("../icons/icon.icns"),
    )
    .map_err(|error| error.to_string())?;

    let executable = macos.join("AlfredNotificationIdentity");
    fs::write(&executable, b"#!/bin/sh\nexit 0\n").map_err(|error| error.to_string())?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;

    let status = Command::new(
        "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
    )
    .arg("-f")
    .arg(&bundle)
    .status()
    .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Launch Services registration exited with {status}"))
    }
}

#[cfg(target_os = "macos")]
pub fn prepare_notification_identity<R: Runtime>(app: &AppHandle<R>) {
    let identifier = app.config().identifier.as_str();
    let sender = if !tauri::is_dev() || install_development_notification_identity(app).is_ok() {
        identifier
    } else {
        // Preserve working notifications if Launch Services is unavailable.
        "com.apple.Terminal"
    };

    if let Err(error) = notify_rust::set_application(sender) {
        eprintln!("notification identity not configured: {error}");
    }
}

/// Show a run-finished notification. Clicking it focuses Alfred and emits
/// `app://open-run-output` so the UI can open the output modal.
pub fn notify_run_finished<R: Runtime>(
    app: &AppHandle<R>,
    workflow_id: String,
    workflow_name: String,
    ok: bool,
    title: String,
    body: String,
) {
    let app = app.clone();
    let (summary, message) = run_notification_copy(&workflow_name, ok);

    // Notification action handlers may wait for minutes or hours. Retain a
    // bounded body in each waiting thread rather than an arbitrary model result.
    let open_payload = OpenRunOutputPayload {
        workflow_id,
        title,
        body: bounded_notification_body(body),
        ok,
    };

    std::thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification
            .summary(&summary)
            .body(message)
            // Lets Linux/XDG treat a banner click as the default action.
            .action("default", "View output");
        configure_notification(&app, &mut notification);

        let Ok(handle) = notification.show() else {
            return;
        };

        handle.wait_for_action(|action| {
            // macOS default click -> "default"; Linux/Windows are similar.
            if action == "default" || action == "clicked" {
                show_main_window(&app);
                let _ = app.emit("app://open-run-output", &open_payload);
            }
        });
    });
}

#[tauri::command]
pub fn set_notification_sound_cmd(
    preferences: State<'_, NotificationPreferences>,
    sound: String,
) -> Result<(), String> {
    let sound = NotificationSound::parse(&sound)
        .ok_or_else(|| format!("unsupported notification sound: {sound}"))?;
    preferences.set_sound(sound);
    Ok(())
}

#[tauri::command]
pub fn notify_run_finished_cmd(
    app: AppHandle,
    workflow_id: String,
    workflow_name: String,
    ok: bool,
    title: String,
    body: String,
) -> Result<(), String> {
    notify_run_finished(&app, workflow_id, workflow_name, ok, title, body);
    Ok(())
}

/// Fire-and-forget branded desktop banner (including Notify nodes and tests).
pub fn notify_message<R: Runtime>(app: &AppHandle<R>, title: String, body: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        let preview: String = body.chars().take(220).collect();
        let mut notification = notify_rust::Notification::new();
        notification.summary(&title).body(&preview);
        configure_notification(&app, &mut notification);
        let _ = notification.show();
    });
}

#[tauri::command]
pub fn notify_message_cmd(app: AppHandle, title: String, body: String) -> Result<(), String> {
    notify_message(&app, title, body);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_notification_body, run_notification_copy, NotificationSound,
        MAX_NOTIFICATION_OPEN_BODY_BYTES,
    };

    #[test]
    fn parses_supported_sound_ids() {
        assert_eq!(
            NotificationSound::parse("system"),
            Some(NotificationSound::System)
        );
        assert_eq!(
            NotificationSound::parse("chime"),
            Some(NotificationSound::Chime)
        );
        assert_eq!(
            NotificationSound::parse("ping"),
            Some(NotificationSound::Ping)
        );
        assert_eq!(
            NotificationSound::parse("pop"),
            Some(NotificationSound::Pop)
        );
        assert_eq!(
            NotificationSound::parse("none"),
            Some(NotificationSound::None)
        );
        assert_eq!(NotificationSound::parse("unknown"), None);
    }

    #[test]
    fn bounds_output_retained_by_notification_action_threads() {
        let body = "é".repeat(MAX_NOTIFICATION_OPEN_BODY_BYTES);
        let bounded = bounded_notification_body(body);

        assert!(bounded.len() <= MAX_NOTIFICATION_OPEN_BODY_BYTES);
        assert!(bounded.contains("output truncated in notification"));
        assert!(bounded.is_char_boundary(bounded.len()));
    }

    #[test]
    fn run_notification_prompts_the_user_to_open_the_result() {
        assert_eq!(
            run_notification_copy("Potato", true),
            ("Potato finished".to_string(), "Click to view output.")
        );
        assert_eq!(
            run_notification_copy("Potato", false),
            ("Potato failed".to_string(), "Click to view error details.")
        );
    }
}
