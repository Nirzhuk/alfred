use super::process::{
    find_bin, lock_claude_invocation, prefer_stdout, run_cmd, run_cmd_with_stdin,
};
use super::AgentProvider;
use chrono::{DateTime, Datelike, Duration as ChronoDuration, TimeZone, Utc};
use directories::BaseDirs;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageWindow {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<i64>,
    pub reset_description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageSnapshot {
    pub provider: AgentProvider,
    pub connected: bool,
    pub windows: Vec<AgentUsageWindow>,
    pub source: String,
    pub updated_at: String,
    pub error: Option<String>,
}

pub fn list_provider_usage(providers: &[AgentProvider]) -> Vec<AgentUsageSnapshot> {
    thread::scope(|scope| {
        providers
            .iter()
            .copied()
            .map(|provider| (provider, scope.spawn(move || provider_usage(provider))))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(provider, handle)| {
                handle.join().unwrap_or_else(|_| {
                    snapshot(
                        provider,
                        false,
                        vec![],
                        "native_cli",
                        Some("Usage refresh failed".into()),
                    )
                })
            })
            .collect()
    })
}

fn provider_usage(provider: AgentProvider) -> AgentUsageSnapshot {
    match provider {
        AgentProvider::ClaudeCode => claude_usage(),
        AgentProvider::Cursor => cursor_usage(),
        AgentProvider::Codex => codex_usage(),
        AgentProvider::Opencode => opencode_usage(),
        AgentProvider::GithubCopilot => {
            cli_usage(AgentProvider::GithubCopilot, "copilot", "GitHub Copilot")
        }
        AgentProvider::Gemini => cli_usage(AgentProvider::Gemini, "gemini", "Gemini"),
        AgentProvider::Grok => cli_usage(AgentProvider::Grok, "grok", "Grok"),
        AgentProvider::Pi => cli_usage(AgentProvider::Pi, "pi", "Pi"),
        AgentProvider::Omp => cli_usage(AgentProvider::Omp, "omp", "OMP"),
    }
}

fn snapshot(
    provider: AgentProvider,
    connected: bool,
    windows: Vec<AgentUsageWindow>,
    source: &str,
    error: Option<String>,
) -> AgentUsageSnapshot {
    AgentUsageSnapshot {
        provider,
        connected,
        windows,
        source: source.into(),
        updated_at: Utc::now().to_rfc3339(),
        error,
    }
}

fn not_installed(provider: AgentProvider, command: &str) -> AgentUsageSnapshot {
    snapshot(
        provider,
        false,
        vec![],
        "native_cli",
        Some(format!("{command} is not installed")),
    )
}

/// Copilot, Gemini, Grok, pi, and OMP do not expose a stable local quota API
/// through their CLIs. Still report binary availability so the usage rail can explain
/// why no subscription windows are shown instead of treating them as unknown
/// providers.
fn cli_usage(provider: AgentProvider, command: &str, label: &str) -> AgentUsageSnapshot {
    if find_bin(command).is_none() {
        return not_installed(provider, label);
    }
    snapshot(
        provider,
        true,
        vec![],
        &format!("{label} CLI"),
        Some("Subscription usage is unavailable from this CLI".into()),
    )
}

fn claude_usage() -> AgentUsageSnapshot {
    let provider = AgentProvider::ClaudeCode;
    if let Some(snapshot) = recent_claude_usage() {
        return snapshot;
    }
    let Some(bin) = find_bin("claude") else {
        return not_installed(provider, "Claude Code");
    };
    let _invocation = match lock_claude_invocation(None) {
        Ok(guard) => guard,
        Err(error) => return snapshot(provider, false, vec![], "claude /usage", Some(error)),
    };
    // React Strict Mode deliberately repeats mount effects in development.
    // Check again after acquiring the single-flight lock so that the queued
    // request reuses the first result instead of opening another session.
    if let Some(snapshot) = recent_claude_usage() {
        return snapshot;
    }

    let result = claude_usage_uncached(provider, &bin);
    remember_claude_usage(&result);
    result
}

const CLAUDE_USAGE_DEDUP_WINDOW: Duration = Duration::from_secs(10);

fn claude_usage_cache() -> &'static Mutex<Option<(Instant, AgentUsageSnapshot)>> {
    static CACHE: OnceLock<Mutex<Option<(Instant, AgentUsageSnapshot)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn recent_claude_usage() -> Option<AgentUsageSnapshot> {
    let cache = claude_usage_cache().lock().ok()?;
    let (cached_at, snapshot) = cache.as_ref()?;
    (cached_at.elapsed() < CLAUDE_USAGE_DEDUP_WINDOW).then(|| snapshot.clone())
}

fn remember_claude_usage(snapshot: &AgentUsageSnapshot) {
    if let Ok(mut cache) = claude_usage_cache().lock() {
        *cache = Some((Instant::now(), snapshot.clone()));
    }
}

fn claude_usage_uncached(provider: AgentProvider, bin: &Path) -> AgentUsageSnapshot {
    let auth = run_cmd(
        bin,
        &["auth".into(), "status".into()],
        None,
        Duration::from_secs(8),
        None,
        None,
    );
    let connected = auth
        .ok()
        .and_then(|output| serde_json::from_str::<Value>(&prefer_stdout(&output)).ok())
        .and_then(|value| value.get("loggedIn").and_then(Value::as_bool))
        .unwrap_or(false);
    if !connected {
        return snapshot(
            provider,
            false,
            vec![],
            "claude /usage",
            Some("Not connected".into()),
        );
    }

    // `/usage` is a local Claude Code command (`num_turns: 0`) and does not
    // ask a model to estimate the account limit.
    let output = run_cmd(
        bin,
        &[
            "-p".into(),
            "/usage".into(),
            "--safe-mode".into(),
            "--output-format".into(),
            "json".into(),
            "--no-session-persistence".into(),
            "--max-turns".into(),
            "1".into(),
        ],
        None,
        Duration::from_secs(35),
        None,
        None,
    );
    let Ok(output) = output else {
        return snapshot(
            provider,
            true,
            vec![],
            "claude /usage",
            Some("Could not refresh subscription usage".into()),
        );
    };
    let parsed: Value = match serde_json::from_str(&prefer_stdout(&output)) {
        Ok(value) => value,
        Err(_) => {
            return snapshot(
                provider,
                true,
                vec![],
                "claude /usage",
                Some("Claude returned no subscription windows".into()),
            )
        }
    };
    let mut windows = parse_claude_rate_limits(&parsed);
    if windows.is_empty() {
        windows = parse_claude_usage_text(
            parsed
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    }
    let error = windows
        .is_empty()
        .then(|| "Subscription usage is unavailable for this Claude account".into());
    snapshot(provider, true, windows, "claude /usage", error)
}

fn parse_claude_rate_limits(value: &Value) -> Vec<AgentUsageWindow> {
    let rate_limits = value.get("rate_limits").or_else(|| value.get("rateLimits"));
    let Some(rate_limits) = rate_limits else {
        return vec![];
    };
    [
        ("five_hour", "5-hour"),
        ("seven_day", "7-day"),
        ("seven_day_opus", "7-day Opus"),
        ("seven_day_sonnet", "7-day Sonnet"),
    ]
    .into_iter()
    .filter_map(|(key, label)| {
        let camel = to_camel_case(key);
        let window = rate_limits
            .get(key)
            .or_else(|| rate_limits.get(camel.as_str()))?;
        let used_percent = number(window, &["used_percentage", "usedPercentage"])
            .map(clamp_percent)
            .or_else(|| {
                number(window, &["utilization"])
                    .map(|utilization| clamp_percent(utilization * 100.0))
            })?;
        Some(AgentUsageWindow {
            label: label.into(),
            used_percent,
            resets_at: timestamp(window, &["resets_at", "resetsAt"]),
            reset_description: None,
        })
    })
    .collect()
}

fn parse_claude_usage_text(text: &str) -> Vec<AgentUsageWindow> {
    let mut windows = Vec::new();
    let mut pending_label: Option<&str> = None;
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("5-hour") || lower.contains("5 hour") || lower.contains("current session")
        {
            pending_label = Some("5-hour");
        } else if lower.contains("7-day") || lower.contains("7 day") || lower.contains("weekly") {
            pending_label = Some("7-day");
        }
        let Some(percent) = percent_in_text(line) else {
            continue;
        };
        let Some(label) = pending_label else {
            continue;
        };
        let reset_description = lower
            .find("reset")
            .map(|index| line[index..].trim().to_string());
        windows.push(AgentUsageWindow {
            label: label.into(),
            used_percent: clamp_percent(percent),
            resets_at: None,
            reset_description,
        });
        pending_label = None;
    }
    windows
}

fn cursor_usage() -> AgentUsageSnapshot {
    let provider = AgentProvider::Cursor;
    let Some(bin) = find_bin("cursor-agent").or_else(|| find_bin("agent")) else {
        return not_installed(provider, "Cursor Agent");
    };
    match query_cursor_usage(&bin) {
        Ok((connected, windows, error)) => snapshot(
            provider,
            connected,
            windows,
            "cursor internal usage helper",
            error,
        ),
        Err(error) => snapshot(
            provider,
            false,
            vec![],
            "cursor internal usage helper",
            Some(error),
        ),
    }
}

const CURSOR_USAGE_SCRIPT: &str = r#"
(async () => {
  try {
    const fs = require('fs');
    const vm = require('vm');
    const path = require('path');
    const { createRequire } = require('module');
    const versionDir = process.argv[2];
    const mainPath = path.join(versionDir, 'index.js');
    const chunkPath = path.join(versionDir, '6260.index.js');
    const bundleRequire = createRequire(mainPath);
    let code = fs.readFileSync(mainPath, 'utf8');
    code = code.replace('var __webpack_exports__=__webpack_require__("./src/main.tsx")})();', 'globalThis.__cursor_wreq=__webpack_require__;})();');
    const g = globalThis;
    const context = {
      require: bundleRequire,
      process: { ...process, argv: ['node', 'cursor-agent', '--help'], exit: () => {} },
      console,
      Buffer,
      setTimeout,
      clearTimeout,
      setInterval,
      clearInterval,
      URL,
      URLSearchParams,
      TextEncoder,
      TextDecoder,
      AbortController,
      AbortSignal,
      fetch,
      Headers,
      Request,
      Response,
      ReadableStream,
      WritableStream,
      TransformStream,
      Blob,
      File,
      FormData,
      crypto: g.crypto,
      performance: g.performance,
      structuredClone: g.structuredClone,
      queueMicrotask,
      atob,
      btoa,
      Event,
      EventTarget,
      MessageChannel,
      MessageEvent,
      DOMException,
      __filename: mainPath,
      __dirname: versionDir,
      exports: {},
      module: { exports: {} },
    };
    context.global = context;
    context.globalThis = context;
    vm.runInNewContext(code, context, { filename: mainPath });
    const wreq = context.__cursor_wreq;
    const chunkExports = require(chunkPath);
    Object.assign(wreq.m, chunkExports.modules);
    const createDash = wreq('./src/dashboard-client.ts').m;
    const creds = wreq('../cli-credentials/dist/index.js').jo({ domain: 'cursor' });
    const urls = wreq('./src/utils/service-urls.ts');
    const usageData = wreq('./src/usage/usage-data.ts');
    const client = createDash({ endpoint: urls.e9(), credentialManager: creds });
    const result = await usageData.d({ client, locale: 'en-US' });
    console.log(JSON.stringify(result));
  } catch (error) {
    console.log(JSON.stringify({
      kind: 'error',
      message: error && error.message ? error.message : String(error),
    }));
  }
})();
"#;

fn query_cursor_usage(bin: &Path) -> Result<(bool, Vec<AgentUsageWindow>, Option<String>), String> {
    let output = run_cmd(
        bin,
        &["status".into()],
        None,
        Duration::from_secs(8),
        None,
        None,
    )?;
    let status_text = prefer_stdout(&output);
    let status_lower = status_text.to_lowercase();
    let connected = !status_lower.contains("not logged in")
        && !status_lower.contains("sign in")
        && !status_lower.is_empty();
    if !connected {
        return Ok((false, vec![], Some("Cursor CLI login required".into())));
    }

    let version_dir = std::fs::canonicalize(bin)
        .map_err(|error| format!("Could not resolve Cursor Agent: {error}"))?
        .parent()
        .map(Path::to_path_buf)
        .ok_or("Cursor Agent installation is missing its version directory")?;
    let node_bin = version_dir.join("node");
    if !node_bin.is_file() {
        return Err("Cursor Agent installation is missing its bundled Node runtime".into());
    }

    let args = vec!["-".into(), version_dir.to_string_lossy().into_owned()];
    let output = run_cmd_with_stdin(
        &node_bin,
        &args,
        None,
        Duration::from_secs(20),
        None,
        None,
        CURSOR_USAGE_SCRIPT,
    )?;
    let parsed = serde_json::from_str::<Value>(&prefer_stdout(&output))
        .map_err(|error| format!("Could not parse Cursor usage: {error}"))?;
    parse_cursor_usage_response(&parsed)
}

fn parse_cursor_usage_response(
    value: &Value,
) -> Result<(bool, Vec<AgentUsageWindow>, Option<String>), String> {
    match value.get("kind").and_then(Value::as_str) {
        Some("error") => Err(value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Could not load Cursor usage")
            .to_string()),
        Some("unavailable") => Ok((
            true,
            vec![],
            Some(
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Cursor usage is unavailable for this account")
                    .to_string(),
            ),
        )),
        Some("available") => Ok(cursor_windows_from_usage_model(value)),
        Some(other) => Err(format!("Unsupported Cursor usage payload: {other}")),
        None => Err("Cursor usage payload did not include a kind".into()),
    }
}

fn cursor_windows_from_usage_model(value: &Value) -> (bool, Vec<AgentUsageWindow>, Option<String>) {
    let model = value.get("model").unwrap_or(&Value::Null);
    let reset_description = model
        .get("resetLabel")
        .and_then(Value::as_str)
        .map(str::to_string);
    match model.get("kind").and_then(Value::as_str) {
        Some("standard") => {
            let included = model.get("included").unwrap_or(&Value::Null);
            let mut windows = vec![
                cursor_window(
                    "Included",
                    number(included, &["totalPercentUsed"]).unwrap_or(0.0),
                    reset_description.clone(),
                ),
                cursor_window(
                    "Auto",
                    number(included, &["autoPercentUsed"]).unwrap_or(0.0),
                    reset_description.clone(),
                ),
                cursor_window(
                    "API",
                    number(included, &["apiPercentUsed"]).unwrap_or(0.0),
                    reset_description.clone(),
                ),
            ];
            if let Some(window) = cursor_on_demand_window(
                model.get("onDemand").unwrap_or(&Value::Null),
                reset_description.clone(),
            ) {
                windows.push(window);
            }
            let error = windows
                .is_empty()
                .then(|| "Cursor returned no subscription windows".into());
            (true, windows, error)
        }
        Some("spend") => (
            true,
            vec![],
            Some("Cursor CLI only exposes spend-mode usage for this plan".into()),
        ),
        Some(other) => (
            true,
            vec![],
            Some(format!(
                "Cursor returned an unsupported usage model: {other}"
            )),
        ),
        None => (
            true,
            vec![],
            Some("Cursor returned an incomplete usage model".into()),
        ),
    }
}

fn cursor_window(
    label: &str,
    used_percent: f64,
    reset_description: Option<String>,
) -> AgentUsageWindow {
    AgentUsageWindow {
        label: label.into(),
        used_percent: clamp_percent(used_percent),
        resets_at: None,
        reset_description,
    }
}

fn cursor_on_demand_window(
    value: &Value,
    reset_description: Option<String>,
) -> Option<AgentUsageWindow> {
    match value.get("kind").and_then(Value::as_str) {
        Some("fixed") => Some(cursor_window(
            "On-Demand",
            number(value, &["percent"]).or_else(|| {
                let used = number(value, &["usedDollars"])?;
                let limit = number(value, &["limitDollars"])?;
                (limit > 0.0).then_some(used / limit * 100.0)
            })?,
            reset_description,
        )),
        Some("unlimited") | Some("disabled") | Some("unavailable") | None => None,
        Some(_) => None,
    }
}

fn opencode_usage() -> AgentUsageSnapshot {
    let provider = AgentProvider::Opencode;
    let Some(bin) = find_bin("opencode") else {
        return not_installed(provider, "OpenCode");
    };
    let auth = run_cmd(
        &bin,
        &["auth".into(), "list".into()],
        None,
        Duration::from_secs(10),
        None,
        None,
    )
    .ok()
    .map(|output| prefer_stdout(&output).to_lowercase())
    .unwrap_or_default();
    let has_go = auth.contains("opencode go");

    if has_go {
        return match opencode_go_local_windows() {
            Ok(windows) if !windows.is_empty() => snapshot(
                provider,
                true,
                windows,
                "OpenCode Go local history estimate",
                None,
            ),
            Ok(_) => snapshot(
                provider,
                true,
                vec![],
                "opencode local history",
                Some("Run an OpenCode Go workflow to estimate usage".into()),
            ),
            Err(error) => snapshot(
                provider,
                true,
                vec![],
                "opencode local history",
                Some(error),
            ),
        };
    }

    snapshot(
        provider,
        true,
        vec![],
        "opencode auth list",
        Some("OpenCode routes multiple providers and has no unified quota".into()),
    )
}

#[derive(Debug, Clone, Copy)]
struct OpenCodeGoUsageRow {
    created_ms: i64,
    cost_usd: f64,
}

const OPENCODE_GO_FIVE_HOUR_LIMIT_USD: f64 = 12.0;
const OPENCODE_GO_WEEKLY_LIMIT_USD: f64 = 30.0;
const OPENCODE_GO_MONTHLY_LIMIT_USD: f64 = 60.0;

fn opencode_go_local_windows() -> Result<Vec<AgentUsageWindow>, String> {
    let base_dirs = BaseDirs::new().ok_or("OpenCode data directory unavailable")?;
    let database_path = base_dirs
        .home_dir()
        .join(".local/share/opencode/opencode.db");
    if !database_path.is_file() {
        return Ok(vec![]);
    }

    let connection = Connection::open_with_flags(
        &database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("Could not read OpenCode Go usage: {error}"))?;
    connection
        .busy_timeout(Duration::from_millis(250))
        .map_err(|error| format!("Could not read OpenCode Go usage: {error}"))?;

    let has_part_table = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'part')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    let sql = if has_part_table {
        OPENCODE_GO_MESSAGE_AND_PART_USAGE_SQL
    } else {
        OPENCODE_GO_MESSAGE_USAGE_SQL
    };
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("Could not read OpenCode Go usage: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(OpenCodeGoUsageRow {
                created_ms: row.get(0)?,
                cost_usd: row.get(1)?,
            })
        })
        .map_err(|error| format!("Could not read OpenCode Go usage: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Could not read OpenCode Go usage: {error}"))?
        .into_iter()
        .filter(|row| row.created_ms > 0 && row.cost_usd.is_finite() && row.cost_usd >= 0.0)
        .collect::<Vec<_>>();

    Ok(opencode_go_windows_from_rows(&rows, Utc::now()))
}

const OPENCODE_GO_MESSAGE_USAGE_SQL: &str = r#"
    SELECT
        CAST(COALESCE(json_extract(data, '$.time.created'), time_created) AS INTEGER),
        CAST(json_extract(data, '$.cost') AS REAL)
    FROM message
    WHERE json_valid(data)
      AND json_extract(data, '$.providerID') = 'opencode-go'
      AND json_extract(data, '$.role') = 'assistant'
      AND json_type(data, '$.cost') IN ('integer', 'real')
"#;

const OPENCODE_GO_MESSAGE_AND_PART_USAGE_SQL: &str = r#"
    WITH provider_messages AS (
        SELECT
            id AS message_id,
            CAST(COALESCE(json_extract(data, '$.time.created'), time_created) AS INTEGER) AS created_ms,
            CAST(json_extract(data, '$.cost') AS REAL) AS cost,
            json_type(data, '$.cost') IN ('integer', 'real') AS has_cost
        FROM message
        WHERE json_valid(data)
          AND json_extract(data, '$.providerID') = 'opencode-go'
          AND json_extract(data, '$.role') = 'assistant'
    )
    SELECT
        CAST(COALESCE(json_extract(p.data, '$.time.created'), p.time_created, m.created_ms) AS INTEGER),
        CAST(json_extract(p.data, '$.cost') AS REAL)
    FROM part p
    JOIN provider_messages m ON m.message_id = p.message_id
    WHERE json_valid(p.data)
      AND json_extract(p.data, '$.type') = 'step-finish'
      AND json_type(p.data, '$.cost') IN ('integer', 'real')
    UNION ALL
    SELECT created_ms, cost
    FROM provider_messages m
    WHERE has_cost
      AND NOT EXISTS (
          SELECT 1
          FROM part p
          WHERE p.message_id = m.message_id
            AND json_valid(p.data)
            AND json_extract(p.data, '$.type') = 'step-finish'
            AND json_type(p.data, '$.cost') IN ('integer', 'real')
      )
"#;

fn opencode_go_windows_from_rows(
    rows: &[OpenCodeGoUsageRow],
    now: DateTime<Utc>,
) -> Vec<AgentUsageWindow> {
    if rows.is_empty() {
        return vec![];
    }

    let now_ms = now.timestamp_millis();
    let five_hour_start_ms = now_ms - ChronoDuration::hours(5).num_milliseconds();
    let week_start = now
        .date_naive()
        .checked_sub_days(chrono::Days::new(
            now.weekday().num_days_from_monday().into(),
        ))
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| Utc.from_utc_datetime(&date))
        .unwrap_or(now);
    let month_start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    let (next_month_year, next_month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    let month_end = Utc
        .with_ymd_and_hms(next_month_year, next_month, 1, 0, 0, 0)
        .single()
        .unwrap_or(now);

    let mut five_hour_cost = 0.0;
    let mut weekly_cost = 0.0;
    let mut monthly_cost = 0.0;
    let mut oldest_five_hour_ms = None;
    for row in rows {
        if row.created_ms >= five_hour_start_ms && row.created_ms <= now_ms {
            five_hour_cost += row.cost_usd;
            oldest_five_hour_ms = Some(
                oldest_five_hour_ms
                    .map_or(row.created_ms, |oldest: i64| oldest.min(row.created_ms)),
            );
        }
        if row.created_ms >= week_start.timestamp_millis() && row.created_ms <= now_ms {
            weekly_cost += row.cost_usd;
        }
        if row.created_ms >= month_start.timestamp_millis() && row.created_ms <= now_ms {
            monthly_cost += row.cost_usd;
        }
    }

    vec![
        AgentUsageWindow {
            label: "5-hour local estimate".into(),
            used_percent: usage_percent(five_hour_cost, OPENCODE_GO_FIVE_HOUR_LIMIT_USD),
            resets_at: oldest_five_hour_ms
                .map(|timestamp| timestamp / 1000 + ChronoDuration::hours(5).num_seconds()),
            reset_description: None,
        },
        AgentUsageWindow {
            label: "Weekly local estimate".into(),
            used_percent: usage_percent(weekly_cost, OPENCODE_GO_WEEKLY_LIMIT_USD),
            resets_at: Some((week_start + ChronoDuration::days(7)).timestamp()),
            reset_description: None,
        },
        AgentUsageWindow {
            label: "Monthly local estimate".into(),
            used_percent: usage_percent(monthly_cost, OPENCODE_GO_MONTHLY_LIMIT_USD),
            resets_at: Some(month_end.timestamp()),
            reset_description: None,
        },
    ]
}

fn usage_percent(used: f64, limit: f64) -> f64 {
    ((used / limit * 100.0).clamp(0.0, 100.0) * 10.0).round() / 10.0
}

fn codex_usage() -> AgentUsageSnapshot {
    let provider = AgentProvider::Codex;
    let Some(bin) = find_bin("codex") else {
        return not_installed(provider, "Codex");
    };
    match query_codex_app_server(&bin) {
        Ok((connected, windows)) => {
            let error = if !connected {
                Some("Not connected".into())
            } else if windows.is_empty() {
                Some("Codex returned no subscription windows".into())
            } else {
                None
            };
            snapshot(
                provider,
                connected,
                windows,
                "codex account/rateLimits/read",
                error,
            )
        }
        Err(error) => snapshot(provider, false, vec![], "codex app-server", Some(error)),
    }
}

fn query_codex_app_server(bin: &Path) -> Result<(bool, Vec<AgentUsageWindow>), String> {
    let mut child = Command::new(bin)
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start Codex: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("Codex stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("Codex stdout unavailable")?;
    writeln!(
        stdin,
        "{}",
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "alfred", "title": "Alfred", "version": env!("CARGO_PKG_VERSION") }
            }
        })
    )
    .map_err(|error| error.to_string())?;
    stdin.flush().map_err(|error| error.to_string())?;

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    });

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut account_seen = false;
    let mut connected = false;
    let mut windows: Option<Vec<AgentUsageWindow>> = None;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = receiver.recv_timeout(remaining.min(Duration::from_millis(500))) else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match message.get("id").and_then(Value::as_i64) {
            Some(1) => {
                for request in [
                    json!({ "method": "initialized", "params": {} }),
                    json!({ "method": "account/read", "id": 2, "params": { "refreshToken": false } }),
                    json!({ "method": "account/rateLimits/read", "id": 3, "params": {} }),
                ] {
                    writeln!(stdin, "{request}").map_err(|error| error.to_string())?;
                }
                stdin.flush().map_err(|error| error.to_string())?;
            }
            Some(2) => {
                account_seen = true;
                connected = message
                    .pointer("/result/account")
                    .is_some_and(|account| !account.is_null());
            }
            Some(3) => {
                windows = Some(parse_codex_rate_limits(&message));
            }
            _ => {}
        }
        if account_seen && windows.is_some() {
            break;
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    if !account_seen {
        return Err("Codex account status timed out".into());
    }
    Ok((connected, windows.unwrap_or_default()))
}

fn parse_codex_rate_limits(message: &Value) -> Vec<AgentUsageWindow> {
    let result = match message.get("result") {
        Some(value) => value,
        None => return vec![],
    };
    let limits = result
        .pointer("/rateLimitsByLimitId/codex")
        .or_else(|| result.get("rateLimits"));
    let Some(limits) = limits else {
        return vec![];
    };
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| {
            let window = limits.get(key)?.as_object()?;
            let duration = window
                .get("windowDurationMins")
                .or_else(|| window.get("window_minutes"))
                .and_then(Value::as_i64);
            Some(AgentUsageWindow {
                label: duration.map(window_label).unwrap_or_else(|| key.into()),
                used_percent: clamp_percent(number_from_object(
                    window,
                    &["usedPercent", "used_percent"],
                )?),
                resets_at: integer_from_object(window, &["resetsAt", "resets_at"]),
                reset_description: None,
            })
        })
        .collect()
}

fn window_label(minutes: i64) -> String {
    match minutes {
        300 => "5-hour".into(),
        10080 => "7-day".into(),
        value if value >= 1440 => format!("{}-day", value / 1440),
        value if value >= 60 => format!("{}-hour", value / 60),
        value => format!("{value}m"),
    }
}

fn number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_f64))
}

fn timestamp(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        let value = value.get(key)?;
        value
            .as_i64()
            .or_else(|| value.as_str()?.parse().ok())
            .or_else(|| {
                DateTime::parse_from_rfc3339(value.as_str()?)
                    .ok()
                    .map(|timestamp| timestamp.timestamp())
            })
    })
}

fn number_from_object(value: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

fn integer_from_object(value: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_i64))
}

fn clamp_percent(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn percent_in_text(text: &str) -> Option<f64> {
    let percent = text.find('%')?;
    let prefix = &text[..percent];
    let value = prefix
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .filter(|part| !part.is_empty())
        .next_back()?;
    value.parse().ok()
}

fn to_camel_case(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase = false;
    for character in value.chars() {
        if character == '_' {
            uppercase = true;
        } else if uppercase {
            result.extend(character.to_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_one_percent_stays_one_percent() {
        let message = json!({
            "result": {
                "rateLimitsByLimitId": {
                    "codex": {
                        "primary": {
                            "usedPercent": 1,
                            "windowDurationMins": 300,
                            "resetsAt": 1_786_400_000_i64
                        }
                    }
                }
            }
        });

        let windows = parse_codex_rate_limits(&message);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].used_percent, 1.0);
        assert_eq!(windows[0].label, "5-hour");
    }

    #[test]
    fn claude_percentage_and_fraction_have_distinct_semantics() {
        let value = json!({
            "rate_limits": {
                "five_hour": {
                    "used_percentage": 1,
                    "resets_at": 1_786_400_000_i64
                },
                "seven_day": {
                    "utilization": 0.42,
                    "resets_at": "2026-08-18T08:00:00Z"
                }
            }
        });

        let windows = parse_claude_rate_limits(&value);

        assert_eq!(windows[0].used_percent, 1.0);
        assert_eq!(windows[1].used_percent, 42.0);
        assert_eq!(windows[1].resets_at, Some(1_787_040_000));
    }

    #[test]
    fn claude_usage_text_keeps_literal_percentages() {
        let windows = parse_claude_usage_text(
            "Current session\n1% used · resets in 4h 18m\nWeekly limit\n13% used · resets in 3d 9h",
        );

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].used_percent, 1.0);
        assert_eq!(windows[1].used_percent, 13.0);
    }

    #[test]
    fn cursor_standard_usage_maps_to_windows() {
        let payload = json!({
            "kind": "available",
            "model": {
                "kind": "standard",
                "planName": "Pro",
                "resetLabel": "Resets Sep 1",
                "included": {
                    "totalPercentUsed": 53.7855,
                    "autoPercentUsed": 56.6833,
                    "apiPercentUsed": 34.4666
                },
                "onDemand": {
                    "kind": "disabled",
                    "usedDollars": 0
                }
            }
        });

        let (connected, windows, error) =
            parse_cursor_usage_response(&payload).expect("cursor payload should parse");

        assert!(connected);
        assert!(error.is_none());
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].label, "Included");
        assert_eq!(windows[0].used_percent, 53.7855);
        assert_eq!(
            windows[0].reset_description.as_deref(),
            Some("Resets Sep 1")
        );
        assert_eq!(windows[1].label, "Auto");
        assert_eq!(windows[2].label, "API");
    }

    #[test]
    fn cursor_usage_error_payload_surfaces_message() {
        let payload = json!({
            "kind": "error",
            "message": "token expired"
        });

        let error = parse_cursor_usage_response(&payload).expect_err("error payload should fail");

        assert_eq!(error, "token expired");
    }

    #[test]
    fn opencode_go_local_costs_map_to_documented_limits() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 12, 12, 0, 0)
            .single()
            .expect("valid date");
        let rows = vec![
            OpenCodeGoUsageRow {
                created_ms: (now - ChronoDuration::hours(1)).timestamp_millis(),
                cost_usd: 6.0,
            },
            OpenCodeGoUsageRow {
                created_ms: Utc
                    .with_ymd_and_hms(2026, 8, 10, 1, 0, 0)
                    .single()
                    .expect("valid date")
                    .timestamp_millis(),
                cost_usd: 3.0,
            },
            OpenCodeGoUsageRow {
                created_ms: Utc
                    .with_ymd_and_hms(2026, 7, 10, 1, 0, 0)
                    .single()
                    .expect("valid date")
                    .timestamp_millis(),
                cost_usd: 60.0,
            },
        ];

        let windows = opencode_go_windows_from_rows(&rows, now);

        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].used_percent, 50.0);
        assert_eq!(windows[1].used_percent, 30.0);
        assert_eq!(windows[2].used_percent, 15.0);
        assert_eq!(
            windows[0].resets_at,
            Some((now + ChronoDuration::hours(4)).timestamp())
        );
    }
}
