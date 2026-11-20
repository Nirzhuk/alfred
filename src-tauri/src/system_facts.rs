//! Ambient facts a connection condition can compare: clock, machine identity,
//! power, network, files, environment, and the current desktop.
//!
//! The clock and machine identity are captured once per run by the caller so a
//! long run cannot cross midnight and silently change which branch fires. File
//! metadata, power, network, clipboard, and the frontmost app are read when the
//! caller resolves facts, which the runner does at each gate.
//!
//! Conditions never send these values to a model or store them in run history.
//! The editor preview is the only place a resolved value is shown.

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveTime, Timelike, Weekday};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A machine value a condition can name. Kept separate from the operator table
/// in `runner::conditions`, which decides which operators each field accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SystemField {
    Weekday,
    Date,
    Time,
    Timezone,
    Os,
    Hostname,
    Username,
    PowerSource,
    Battery,
    Network,
    Path,
    Env,
    FrontmostApp,
    Clipboard,
}

impl SystemField {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "weekday" => Some(Self::Weekday),
            "date" => Some(Self::Date),
            "time" => Some(Self::Time),
            "timezone" => Some(Self::Timezone),
            "os" => Some(Self::Os),
            "hostname" => Some(Self::Hostname),
            "username" => Some(Self::Username),
            "powerSource" => Some(Self::PowerSource),
            "battery" => Some(Self::Battery),
            "network" => Some(Self::Network),
            "path" => Some(Self::Path),
            "env" => Some(Self::Env),
            "frontmostApp" => Some(Self::FrontmostApp),
            "clipboard" => Some(Self::Clipboard),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Weekday => "day of week",
            Self::Date => "date",
            Self::Time => "time of day",
            Self::Timezone => "time zone",
            Self::Os => "operating system",
            Self::Hostname => "hostname",
            Self::Username => "username",
            Self::PowerSource => "power source",
            Self::Battery => "battery level",
            Self::Network => "network",
            Self::Path => "file",
            Self::Env => "environment variable",
            Self::FrontmostApp => "frontmost app",
            Self::Clipboard => "clipboard",
        }
    }

    /// The subject needs a second input naming what to read.
    pub(crate) fn requires_key(self) -> bool {
        matches!(self, Self::Path | Self::Env)
    }
}

/// Wall clock captured at run start. `time` is truncated to the minute so a
/// comparison never depends on sub-minute timing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClockFacts {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub timezone: String,
}

impl ClockFacts {
    pub(crate) fn now() -> Self {
        let now = Local::now();
        let timezone = iana_time_zone::get_timezone().unwrap_or_else(|_| now.offset().to_string());
        Self::at(now.date_naive(), now.time(), timezone)
    }

    pub(crate) fn at(date: NaiveDate, time: NaiveTime, timezone: impl Into<String>) -> Self {
        let time = NaiveTime::from_hms_opt(time.hour(), time.minute(), 0).unwrap_or(time);
        Self {
            date,
            time,
            timezone: timezone.into(),
        }
    }

    pub(crate) fn weekday(&self) -> Weekday {
        self.date.weekday()
    }

    pub(crate) fn weekday_name(&self) -> &'static str {
        weekday_name(self.weekday())
    }

    pub(crate) fn date_iso(&self) -> String {
        self.date.format("%Y-%m-%d").to_string()
    }

    pub(crate) fn time_hm(&self) -> String {
        self.time.format("%H:%M").to_string()
    }
}

pub(crate) fn weekday_name(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PathState {
    pub exists: bool,
    pub is_file: bool,
    pub is_dir: bool,
    pub modified: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EnvValue {
    pub is_set: bool,
    pub value: String,
}

/// Everything a condition can read, resolved by `resolve` before evaluation.
#[derive(Debug, Clone)]
pub(crate) struct SystemFacts {
    pub clock: ClockFacts,
    pub os: String,
    pub hostname: Option<String>,
    pub username: Option<String>,
    pub power_source: Option<String>,
    pub battery_percent: Option<u8>,
    pub network_online: Option<bool>,
    pub paths: HashMap<String, PathState>,
    pub env: HashMap<String, EnvValue>,
    pub frontmost_app: Option<String>,
    pub clipboard: Option<String>,
    /// Field to a user-facing reason the value could not be read.
    pub unavailable: HashMap<SystemField, String>,
}

impl SystemFacts {
    pub(crate) fn new(clock: ClockFacts) -> Self {
        Self {
            clock,
            os: os_canonical().to_string(),
            hostname: None,
            username: None,
            power_source: None,
            battery_percent: None,
            network_online: None,
            paths: HashMap::new(),
            env: HashMap::new(),
            frontmost_app: None,
            clipboard: None,
            unavailable: HashMap::new(),
        }
    }
}

/// Which facts a workflow actually uses. Resolving only these keeps a run from
/// touching the clipboard or battery on every gate when no condition asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SystemRequirements {
    pub fields: HashSet<SystemField>,
    pub paths: HashSet<String>,
    pub env: HashSet<String>,
}

impl SystemRequirements {
    pub(crate) fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.paths.is_empty() && self.env.is_empty()
    }
}

/// Resolve the requested facts. `working_directory` is the workflow's directory;
/// relative file paths resolve from it, or from the process directory when the
/// workflow has none.
pub(crate) fn resolve(
    requirements: &SystemRequirements,
    working_directory: Option<&Path>,
    clock: &ClockFacts,
) -> SystemFacts {
    let mut facts = SystemFacts::new(clock.clone());
    if requirements.is_empty() {
        return facts;
    }

    if requirements.fields.contains(&SystemField::Hostname) {
        match whoami::hostname() {
            Ok(value) if !value.trim().is_empty() => facts.hostname = Some(value),
            Ok(_) => {
                facts
                    .unavailable
                    .insert(SystemField::Hostname, "No hostname is configured.".into());
            }
            Err(error) => {
                facts.unavailable.insert(
                    SystemField::Hostname,
                    format!("The hostname could not be read ({error})."),
                );
            }
        }
    }
    if requirements.fields.contains(&SystemField::Username) {
        match whoami::username() {
            Ok(value) if !value.trim().is_empty() => facts.username = Some(value),
            Ok(_) => {
                facts
                    .unavailable
                    .insert(SystemField::Username, "No username is configured.".into());
            }
            Err(error) => {
                facts.unavailable.insert(
                    SystemField::Username,
                    format!("The username could not be read ({error})."),
                );
            }
        }
    }
    if requirements.fields.contains(&SystemField::PowerSource)
        || requirements.fields.contains(&SystemField::Battery)
    {
        let power = probe_power();
        facts.power_source = power.source;
        facts.battery_percent = power.percent;
        if let Some(reason) = power.source_error {
            facts.unavailable.insert(SystemField::PowerSource, reason);
        }
        if let Some(reason) = power.battery_error {
            facts.unavailable.insert(SystemField::Battery, reason);
        }
    }
    if requirements.fields.contains(&SystemField::Network) {
        facts.network_online = Some(probe_network());
    }
    for path in &requirements.paths {
        facts
            .paths
            .insert(path.clone(), probe_path(path, working_directory));
    }
    for key in &requirements.env {
        facts.env.insert(key.clone(), probe_env(key));
    }
    if requirements.fields.contains(&SystemField::FrontmostApp) {
        match probe_frontmost_app() {
            Ok(value) => facts.frontmost_app = Some(value),
            Err(error) => {
                facts.unavailable.insert(SystemField::FrontmostApp, error);
            }
        }
    }
    if requirements.fields.contains(&SystemField::Clipboard) {
        match probe_clipboard() {
            Ok(value) => facts.clipboard = Some(value),
            Err(error) => {
                facts.unavailable.insert(SystemField::Clipboard, error);
            }
        }
    }

    facts
}

pub(crate) fn os_canonical() -> &'static str {
    std::env::consts::OS
}

pub(crate) fn os_label(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "macos" => "macOS".into(),
        "windows" => "Windows".into(),
        "linux" => "Linux".into(),
        other => other.into(),
    }
}

struct PowerProbe {
    source: Option<String>,
    percent: Option<u8>,
    source_error: Option<String>,
    battery_error: Option<String>,
}

impl PowerProbe {
    fn unavailable(reason: &str) -> Self {
        Self {
            source: None,
            percent: None,
            source_error: Some(reason.into()),
            battery_error: Some(reason.into()),
        }
    }
}

#[cfg(target_os = "macos")]
mod power {
    use super::PowerProbe;
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFNumberRef = *const c_void;
    type CFIndex = isize;
    type Boolean = u8;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
        fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: CFTypeRef,
            c_str: *const std::ffi::c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> CFTypeRef;
        fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
        fn CFNumberGetValue(number: CFNumberRef, the_type: i32, value: *mut c_void) -> Boolean;
        fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> Boolean;
        fn CFRelease(value: CFTypeRef);
    }

    const CF_NUMBER_SINT64_TYPE: i32 = 4;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    /// CoreFoundation objects returned under the Copy rule, released on drop.
    struct CfOwned(CFTypeRef);

    impl Drop for CfOwned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }

    /// The IOPS dictionary keys are C string macros, not exported CFString
    /// symbols, so they are created for this call and released with `CfOwned`.
    fn cf_string(value: &'static [u8]) -> CfOwned {
        CfOwned(unsafe {
            CFStringCreateWithCString(
                std::ptr::null(),
                value.as_ptr().cast(),
                CF_STRING_ENCODING_UTF8,
            )
        })
    }

    /// `battery` 0.7 cannot read Apple Silicon packs (`DesignCapacity` is no
    /// longer exposed), so macOS reads IOPowerSources directly. This is the
    /// same API the menu-bar battery item uses.
    pub(super) fn probe_power() -> PowerProbe {
        unsafe {
            let state_key = cf_string(b"Power Source State\0");
            let current_key = cf_string(b"Current Capacity\0");
            let max_key = cf_string(b"Max Capacity\0");
            let ac_value = cf_string(b"AC Power\0");
            let battery_value = cf_string(b"Battery Power\0");
            if [
                state_key.0,
                current_key.0,
                max_key.0,
                ac_value.0,
                battery_value.0,
            ]
            .iter()
            .any(|value| value.is_null())
            {
                return PowerProbe::unavailable("Power information is unavailable.");
            }

            let blob = CfOwned(IOPSCopyPowerSourcesInfo());
            if blob.0.is_null() {
                return PowerProbe::unavailable("Power information is unavailable.");
            }
            let list = CfOwned(IOPSCopyPowerSourcesList(blob.0));
            if list.0.is_null() {
                return PowerProbe::unavailable("Power information is unavailable.");
            }
            if CFArrayGetCount(list.0) <= 0 {
                return PowerProbe {
                    source: Some("AC".into()),
                    percent: None,
                    source_error: None,
                    battery_error: Some("This computer has no battery.".into()),
                };
            }
            let description =
                IOPSGetPowerSourceDescription(blob.0, CFArrayGetValueAtIndex(list.0, 0));
            let (source, source_error) = if description.is_null() {
                (
                    None,
                    Some("The power source could not be determined.".to_string()),
                )
            } else {
                let state = CFDictionaryGetValue(description, state_key.0);
                if state.is_null() {
                    (
                        None,
                        Some("The power source could not be determined.".to_string()),
                    )
                } else if CFEqual(state, ac_value.0) != 0 {
                    (Some("AC".to_string()), None)
                } else if CFEqual(state, battery_value.0) != 0 {
                    (Some("Battery".to_string()), None)
                } else {
                    (
                        None,
                        Some("The power source could not be determined.".to_string()),
                    )
                }
            };
            let capacity = |key: CFStringRef| -> Option<i64> {
                let value = CFDictionaryGetValue(description, key);
                if value.is_null() {
                    return None;
                }
                let mut number: i64 = 0;
                (CFNumberGetValue(
                    value as CFNumberRef,
                    CF_NUMBER_SINT64_TYPE,
                    &mut number as *mut i64 as *mut c_void,
                ) != 0)
                    .then_some(number)
            };
            let percent = match (capacity(current_key.0), capacity(max_key.0)) {
                (Some(current), Some(max)) if max > 0 => {
                    Some(((current.saturating_mul(100) + max / 2) / max).clamp(0, 100) as u8)
                }
                _ => None,
            };
            let battery_error = percent
                .is_none()
                .then(|| "The battery level could not be read.".to_string());
            PowerProbe {
                source,
                percent,
                source_error,
                battery_error,
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn probe_power() -> PowerProbe {
    power::probe_power()
}

#[cfg(not(target_os = "macos"))]
fn probe_power() -> PowerProbe {
    let manager = match battery::Manager::new() {
        Ok(manager) => manager,
        Err(error) => {
            let reason = format!("Power information is unavailable ({error}).");
            return PowerProbe {
                source: None,
                percent: None,
                source_error: Some(reason.clone()),
                battery_error: Some(reason),
            };
        }
    };
    let mut batteries = match manager.batteries() {
        Ok(batteries) => batteries,
        Err(error) => {
            let reason = format!("Power information is unavailable ({error}).");
            return PowerProbe {
                source: None,
                percent: None,
                source_error: Some(reason.clone()),
                battery_error: Some(reason),
            };
        }
    };
    let Some(battery) = batteries.next() else {
        // A desktop is unambiguously on wall power; it simply has no level.
        return PowerProbe {
            source: Some("AC".into()),
            percent: None,
            source_error: None,
            battery_error: Some("This computer has no battery.".into()),
        };
    };
    let battery = match battery {
        Ok(battery) => battery,
        Err(error) => {
            let reason = format!("Battery information is unavailable ({error}).");
            return PowerProbe {
                source: None,
                percent: None,
                source_error: Some(reason.clone()),
                battery_error: Some(reason),
            };
        }
    };
    let percent = (battery.state_of_charge().value * 100.0)
        .round()
        .clamp(0.0, 100.0) as u8;
    let (source, source_error) = match battery.state() {
        battery::State::Charging | battery::State::Full => (Some("AC".to_string()), None),
        battery::State::Discharging | battery::State::Empty => (Some("Battery".to_string()), None),
        _ => (
            None,
            Some("The power source could not be determined.".to_string()),
        ),
    };
    PowerProbe {
        source,
        percent: Some(percent),
        source_error,
        battery_error: None,
    }
}

/// A default route is the honest local definition of "online": it needs no
/// outbound packet and no DNS lookup, and it answers the question automations
/// actually ask. Connecting a UDP socket performs no network I/O.
fn probe_network() -> bool {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| socket.connect("1.1.1.1:80"))
        .is_ok()
        || std::net::UdpSocket::bind("[::]:0")
            .and_then(|socket| socket.connect("[2606:4700:4700::1111]:80"))
            .is_ok()
}

fn probe_env(key: &str) -> EnvValue {
    match std::env::var_os(key) {
        Some(value) => EnvValue {
            is_set: true,
            value: value.to_string_lossy().into_owned(),
        },
        None => EnvValue::default(),
    }
}

fn resolved_path(raw: &str, working_directory: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    let base = working_directory
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    base.map(|base| base.join(path.clone())).unwrap_or(path)
}

fn probe_path(raw: &str, working_directory: Option<&Path>) -> PathState {
    let path = resolved_path(raw, working_directory);
    match std::fs::metadata(path) {
        Ok(metadata) => PathState {
            exists: true,
            is_file: metadata.is_file(),
            is_dir: metadata.is_dir(),
            modified: metadata.modified().ok().map(DateTime::<Local>::from),
        },
        Err(_) => PathState::default(),
    }
}

fn probe_clipboard() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("The clipboard is unavailable ({error})."))?;
    match clipboard.get_text() {
        Ok(text) => Ok(text),
        Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
        Err(error) => Err(format!("The clipboard could not be read ({error}).")),
    }
}

#[cfg(target_os = "macos")]
fn probe_frontmost_app() -> Result<String, String> {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    match workspace.frontmostApplication() {
        Some(application) => match application.localizedName() {
            Some(name) => Ok(name.to_string()),
            None => Err("The frontmost app has no name.".into()),
        },
        // No app is frontmost while the desktop has focus; that is an empty
        // subject, not an error.
        None => Ok(String::new()),
    }
}

#[cfg(not(target_os = "macos"))]
fn probe_frontmost_app() -> Result<String, String> {
    Err("The frontmost app is only available on macOS.".into())
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldPreview {
    pub value: Option<String>,
    pub detail: Option<String>,
    pub unavailable: Option<String>,
}

/// Resolve one field for the editor. This intentionally reads the value live so
/// the author sees what the condition would compare right now.
pub(crate) fn preview(
    field: SystemField,
    key: Option<&str>,
    working_directory: Option<&Path>,
    clock: &ClockFacts,
) -> FieldPreview {
    let unavailable = |reason: String| FieldPreview {
        value: None,
        detail: None,
        unavailable: Some(reason),
    };
    match field {
        SystemField::Weekday => FieldPreview {
            value: Some(clock.weekday_name().into()),
            ..Default::default()
        },
        SystemField::Date => FieldPreview {
            value: Some(clock.date_iso()),
            ..Default::default()
        },
        SystemField::Time => FieldPreview {
            value: Some(clock.time_hm()),
            ..Default::default()
        },
        SystemField::Timezone => FieldPreview {
            value: Some(clock.timezone.clone()),
            ..Default::default()
        },
        SystemField::Os => FieldPreview {
            value: Some(os_label(os_canonical())),
            ..Default::default()
        },
        SystemField::Hostname => match whoami::hostname() {
            Ok(value) => FieldPreview {
                value: Some(value),
                ..Default::default()
            },
            Err(error) => unavailable(format!("The hostname could not be read ({error}).")),
        },
        SystemField::Username => match whoami::username() {
            Ok(value) => FieldPreview {
                value: Some(value),
                ..Default::default()
            },
            Err(error) => unavailable(format!("The username could not be read ({error}).")),
        },
        SystemField::PowerSource | SystemField::Battery => {
            let power = probe_power();
            if field == SystemField::PowerSource {
                match power.source {
                    Some(source) => FieldPreview {
                        value: Some(source.clone()),
                        detail: power.percent.map(|percent| format!("{percent}%")),
                        ..Default::default()
                    },
                    None => unavailable(
                        power
                            .source_error
                            .unwrap_or_else(|| "The power source could not be read.".into()),
                    ),
                }
            } else {
                match power.percent {
                    Some(percent) => FieldPreview {
                        value: Some(format!("{percent}%")),
                        detail: power.source.clone(),
                        ..Default::default()
                    },
                    None => unavailable(
                        power
                            .battery_error
                            .unwrap_or_else(|| "The battery level could not be read.".into()),
                    ),
                }
            }
        }
        SystemField::Network => {
            let online = probe_network();
            FieldPreview {
                value: Some(if online { "online" } else { "offline" }.into()),
                detail: Some(if online {
                    "A default network route exists.".into()
                } else {
                    "No default network route exists.".into()
                }),
                ..Default::default()
            }
        }
        SystemField::Path => {
            let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) else {
                return FieldPreview::default();
            };
            let resolved = resolved_path(key, working_directory);
            let state = probe_path(key, working_directory);
            let kind = if state.is_dir {
                "Folder"
            } else if state.is_file {
                "File"
            } else if state.exists {
                "Item"
            } else {
                "Not found"
            };
            let mut details = vec![resolved.display().to_string()];
            if let Some(modified) = state.modified {
                details.push(format!("modified {}", modified.format("%Y-%m-%d %H:%M")));
            }
            FieldPreview {
                value: Some(kind.into()),
                detail: Some(details.join(" · ")),
                ..Default::default()
            }
        }
        SystemField::Env => {
            let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) else {
                return FieldPreview::default();
            };
            let value = probe_env(key);
            if value.is_set {
                FieldPreview {
                    value: Some(value.value),
                    ..Default::default()
                }
            } else {
                FieldPreview {
                    value: None,
                    detail: Some("Not set".into()),
                    unavailable: None,
                }
            }
        }
        SystemField::FrontmostApp => match probe_frontmost_app() {
            Ok(value) => FieldPreview {
                value: if value.is_empty() { None } else { Some(value) },
                detail: None,
                unavailable: None,
            },
            Err(error) => unavailable(error),
        },
        SystemField::Clipboard => match probe_clipboard() {
            Ok(text) => {
                let characters = text.chars().count();
                FieldPreview {
                    value: Some(truncate_chars(&text, 500)),
                    detail: Some(format!(
                        "{characters} character{}",
                        if characters == 1 { "" } else { "s" }
                    )),
                    unavailable: None,
                }
            }
            Err(error) => unavailable(error),
        },
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut truncated: String = value.chars().take(limit).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> ClockFacts {
        ClockFacts::at(
            NaiveDate::from_ymd_opt(2026, 11, 20).expect("date"),
            NaiveTime::from_hms_opt(9, 5, 0).expect("time"),
            "America/New_York",
        )
    }

    #[test]
    fn clock_values_are_stable_and_minute_precise() {
        let clock = clock();
        assert_eq!(clock.weekday_name(), "Friday");
        assert_eq!(clock.date_iso(), "2026-11-20");
        assert_eq!(clock.time_hm(), "09:05");
    }

    #[test]
    fn preview_reads_the_selected_field() {
        let clock = clock();
        assert_eq!(
            preview(SystemField::Weekday, None, None, &clock).value,
            Some("Friday".into())
        );
        assert_eq!(
            preview(SystemField::Time, None, None, &clock).value,
            Some("09:05".into())
        );
    }

    #[test]
    fn relative_paths_resolve_under_the_working_directory() {
        let root = std::env::temp_dir();
        assert_eq!(
            resolved_path("reports/latest.md", Some(root.as_path())),
            root.join("reports/latest.md")
        );
    }
}
