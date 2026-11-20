//! Conditional connections: which graph edges fire, and therefore which steps
//! run (Plan 053).
//!
//! The whole feature is three rules, implemented here and in the runner's node
//! loop:
//!
//! 1. An edge fires when its source step produced a result and its condition is
//!    absent or evaluates true.
//! 2. A step runs when it has no incoming edges, or at least one incoming edge
//!    fired. Several firing edges still run it once.
//! 3. A step is skipped when it has incoming edges and none fired. A skipped
//!    step produces no result, so its own edges cannot fire — skipping
//!    propagates down the branch without a separate subtree walk.
//!
//! Evaluation is pure. Ambient values arrive already resolved in
//! [`SystemFacts`], captured once per run for the clock and read live for the
//! environment (see `crate::system_facts`).

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday};
use regex::RegexBuilder;
use serde_json::Value;

use crate::system_facts::{SystemFacts, SystemField, SystemRequirements};

/// Matches `MAX_EDGE_CONDITION_VALUE_BYTES` in `src/features/workflow/types.ts`.
pub const MAX_CONDITION_VALUE_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionSource {
    /// Text output of this edge's own source step.
    Output,
    /// Accumulated run context at evaluation time.
    Context,
    /// The run's trigger payload, or an empty string when there is none.
    Trigger,
    /// A value read from the computer, named by `field`.
    System,
}

impl ConditionSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "output" => Some(Self::Output),
            "context" => Some(Self::Context),
            "trigger" => Some(Self::Trigger),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionOperator {
    Contains,
    NotContains,
    Equals,
    NotEquals,
    Matches,
    IsEmpty,
    IsNotEmpty,
    IsOneOf,
    IsNotOneOf,
    Before,
    After,
    Between,
    NotBetween,
    AtLeast,
    AtMost,
    Exists,
    NotExists,
    IsFile,
    IsDirectory,
    ModifiedBefore,
    ModifiedAfter,
}

impl ConditionOperator {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "contains" => Some(Self::Contains),
            "notContains" => Some(Self::NotContains),
            "equals" => Some(Self::Equals),
            "notEquals" => Some(Self::NotEquals),
            "matches" => Some(Self::Matches),
            "isEmpty" => Some(Self::IsEmpty),
            "isNotEmpty" => Some(Self::IsNotEmpty),
            "isOneOf" => Some(Self::IsOneOf),
            "isNotOneOf" => Some(Self::IsNotOneOf),
            "before" => Some(Self::Before),
            "after" => Some(Self::After),
            "between" => Some(Self::Between),
            "notBetween" => Some(Self::NotBetween),
            "atLeast" => Some(Self::AtLeast),
            "atMost" => Some(Self::AtMost),
            "exists" => Some(Self::Exists),
            "notExists" => Some(Self::NotExists),
            "isFile" => Some(Self::IsFile),
            "isDirectory" => Some(Self::IsDirectory),
            "modifiedBefore" => Some(Self::ModifiedBefore),
            "modifiedAfter" => Some(Self::ModifiedAfter),
            _ => None,
        }
    }

    /// Operators that ignore the comparison value.
    pub fn is_unary(self) -> bool {
        matches!(
            self,
            Self::IsEmpty
                | Self::IsNotEmpty
                | Self::Exists
                | Self::NotExists
                | Self::IsFile
                | Self::IsDirectory
        )
    }

    pub fn needs_second_value(self) -> bool {
        matches!(self, Self::Between | Self::NotBetween)
    }

    /// Operators a run-data source (step output, context, trigger) accepts.
    /// Adding typed operators there would imply comparison semantics the text
    /// values do not have.
    fn supports_run_source(self) -> bool {
        matches!(
            self,
            Self::Contains
                | Self::NotContains
                | Self::Equals
                | Self::NotEquals
                | Self::Matches
                | Self::IsEmpty
                | Self::IsNotEmpty
        )
    }

    fn is_text(self) -> bool {
        matches!(
            self,
            Self::Contains
                | Self::NotContains
                | Self::Equals
                | Self::NotEquals
                | Self::Matches
                | Self::IsEmpty
                | Self::IsNotEmpty
                | Self::IsOneOf
                | Self::IsNotOneOf
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::NotContains => "does not contain",
            Self::Equals => "equals",
            Self::NotEquals => "does not equal",
            Self::Matches => "matches regex",
            Self::IsEmpty => "is empty",
            Self::IsNotEmpty => "is not empty",
            Self::IsOneOf => "is one of",
            Self::IsNotOneOf => "is not one of",
            Self::Before => "is before",
            Self::After => "is after",
            Self::Between => "is between",
            Self::NotBetween => "is not between",
            Self::AtLeast => "is at least",
            Self::AtMost => "is at most",
            Self::Exists => "exists",
            Self::NotExists => "does not exist",
            Self::IsFile => "is a file",
            Self::IsDirectory => "is a directory",
            Self::ModifiedBefore => "was modified before",
            Self::ModifiedAfter => "was modified after",
        }
    }
}

/// Which operators each machine field accepts. The editor mirrors this table;
/// a template or graph that names anything else is rejected rather than
/// evaluated with surprising semantics.
pub(crate) fn field_supports(field: SystemField, operator: ConditionOperator) -> bool {
    match field {
        SystemField::Weekday => matches!(
            operator,
            ConditionOperator::IsOneOf | ConditionOperator::IsNotOneOf
        ),
        SystemField::Date => matches!(
            operator,
            ConditionOperator::Before | ConditionOperator::After | ConditionOperator::Equals
        ),
        SystemField::Time => matches!(
            operator,
            ConditionOperator::Before
                | ConditionOperator::After
                | ConditionOperator::Between
                | ConditionOperator::NotBetween
        ),
        SystemField::Timezone
        | SystemField::Hostname
        | SystemField::Username
        | SystemField::FrontmostApp
        | SystemField::Clipboard
        | SystemField::Env => operator.is_text(),
        SystemField::Os | SystemField::PowerSource | SystemField::Network => matches!(
            operator,
            ConditionOperator::Equals | ConditionOperator::NotEquals
        ),
        SystemField::Battery => matches!(
            operator,
            ConditionOperator::AtLeast
                | ConditionOperator::AtMost
                | ConditionOperator::Between
                | ConditionOperator::NotBetween
        ),
        SystemField::Path => matches!(
            operator,
            ConditionOperator::Exists
                | ConditionOperator::NotExists
                | ConditionOperator::IsFile
                | ConditionOperator::IsDirectory
                | ConditionOperator::ModifiedBefore
                | ConditionOperator::ModifiedAfter
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeCondition {
    pub source: ConditionSource,
    /// Present only for [`ConditionSource::System`].
    pub field: Option<SystemField>,
    /// Environment variable name or file path for the fields that need one.
    pub key: Option<String>,
    pub operator: ConditionOperator,
    pub value: String,
    /// Upper bound for `between` / `notBetween`.
    pub value2: Option<String>,
    pub case_sensitive: bool,
}

fn truncate_on_boundary(mut value: String, limit: usize) -> String {
    if value.len() <= limit {
        return value;
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

impl EdgeCondition {
    /// Read a condition off untrusted graph JSON. Anything that is not a fully
    /// valid condition reads as `None`, i.e. an unconditional edge — the same
    /// result as a graph written before this feature existed.
    pub fn parse(edge: &Value) -> Option<Self> {
        Self::parse_value(edge.get("data")?.get("condition")?)
    }

    /// Parse the condition object itself. Shared with the template contract so
    /// export, import, and the runner agree on what is valid.
    pub(crate) fn parse_value(raw: &Value) -> Option<Self> {
        let source = ConditionSource::parse(raw.get("source")?.as_str()?)?;
        let operator = ConditionOperator::parse(raw.get("operator")?.as_str()?)?;

        let field = match source {
            ConditionSource::System => SystemField::parse(raw.get("field")?.as_str()?),
            _ => {
                if raw.get("field").is_some_and(|value| !value.is_null()) {
                    return None;
                }
                None
            }
        };
        if let Some(field) = field {
            if !field_supports(field, operator) {
                return None;
            }
        } else if !operator.supports_run_source() {
            return None;
        }

        let mut value = raw
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        value = truncate_on_boundary(value, MAX_CONDITION_VALUE_BYTES);

        let value2 = raw
            .get("value2")
            .and_then(Value::as_str)
            .map(|text| truncate_on_boundary(text.to_string(), MAX_CONDITION_VALUE_BYTES))
            .filter(|text| !text.is_empty());
        let key = raw
            .get("key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| truncate_on_boundary(text.to_string(), MAX_CONDITION_VALUE_BYTES));

        match field {
            Some(field) if field.requires_key() => {
                key.as_ref()?;
            }
            _ => {
                if key.is_some() {
                    return None;
                }
            }
        }
        if operator.needs_second_value() && value2.is_none() {
            return None;
        }
        if !operator.needs_second_value() && value2.is_some() {
            return None;
        }

        Some(Self {
            source,
            field,
            key,
            operator,
            value,
            value2,
            case_sensitive: raw
                .get("caseSensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    pub(crate) fn system_field(&self) -> Option<SystemField> {
        self.field
    }

    /// Chip-style text for run events and skipped-step notes.
    pub fn summary(&self) -> String {
        match self.source {
            ConditionSource::Output => self.text_summary("step output"),
            ConditionSource::Context => self.text_summary("run context"),
            ConditionSource::Trigger => self.text_summary("trigger payload"),
            ConditionSource::System => self.system_summary(),
        }
    }

    fn text_summary(&self, subject: &str) -> String {
        if self.operator.is_unary() {
            format!("{subject} {}", self.operator.label())
        } else {
            format!("{subject} {} \"{}\"", self.operator.label(), self.value)
        }
    }

    fn operator_label(&self, field: SystemField) -> &'static str {
        match (field, self.operator) {
            (SystemField::Date, ConditionOperator::Equals) => "is on",
            (
                SystemField::Os
                | SystemField::PowerSource
                | SystemField::Network
                | SystemField::Timezone,
                ConditionOperator::Equals,
            ) => "is",
            (
                SystemField::Os
                | SystemField::PowerSource
                | SystemField::Network
                | SystemField::Timezone,
                ConditionOperator::NotEquals,
            ) => "is not",
            _ => self.operator.label(),
        }
    }

    fn system_summary(&self) -> String {
        let Some(field) = self.field else {
            return "computer value".into();
        };
        let subject = field.label();
        let operator = self.operator_label(field);
        match field {
            SystemField::Weekday => format!(
                "{subject} {operator} {}",
                weekday_short_labels(&self.value).join(", ")
            ),
            SystemField::Path => {
                let key = self.key.as_deref().unwrap_or_default();
                if self.operator.is_unary() {
                    format!("{subject} \"{key}\" {operator}")
                } else {
                    format!("{subject} \"{key}\" {operator} \"{}\"", self.value)
                }
            }
            SystemField::Env => format!(
                "{subject} \"{}\" {operator} \"{}\"",
                self.key.as_deref().unwrap_or_default(),
                self.value
            ),
            SystemField::Battery => {
                if self.operator.needs_second_value() {
                    format!(
                        "{subject} {operator} {}% and {}%",
                        self.value,
                        self.value2.as_deref().unwrap_or_default()
                    )
                } else {
                    format!("{subject} {operator} {}%", self.value)
                }
            }
            SystemField::Os => format!("{subject} {operator} {}", os_display(&self.value)),
            SystemField::PowerSource => {
                format!("{subject} {operator} {}", power_display(&self.value))
            }
            SystemField::Network => {
                format!("{subject} {operator} {}", network_display(&self.value))
            }
            _ => {
                if self.operator.needs_second_value() {
                    format!(
                        "{subject} {operator} \"{}\" and \"{}\"",
                        self.value,
                        self.value2.as_deref().unwrap_or_default()
                    )
                } else if self.operator.is_unary() {
                    format!("{subject} {operator}")
                } else {
                    format!("{subject} {operator} \"{}\"", self.value)
                }
            }
        }
    }
}

/// Reject values the editor would not let a user save. The runner treats a bad
/// value as false; templates must fail loudly instead.
pub(crate) fn validate_condition_values(condition: &EdgeCondition) -> Result<(), &'static str> {
    if condition.value.len() > MAX_CONDITION_VALUE_BYTES
        || condition
            .value2
            .as_deref()
            .is_some_and(|value| value.len() > MAX_CONDITION_VALUE_BYTES)
        || condition
            .key
            .as_deref()
            .is_some_and(|value| value.len() > MAX_CONDITION_VALUE_BYTES)
    {
        return Err("edge condition value is out of bounds");
    }
    if condition.source != ConditionSource::System {
        if condition.field.is_some() {
            return Err("edge condition field is not supported for this source");
        }
        if condition.key.is_some() || condition.value2.is_some() {
            return Err("edge condition key or bounds are not supported for this source");
        }
        if !condition.operator.supports_run_source() {
            return Err("edge condition operator is not supported for this source");
        }
        return Ok(());
    }

    let Some(field) = condition.field else {
        return Err("edge condition field is required for a system source");
    };
    if !field_supports(field, condition.operator) {
        return Err("edge condition operator is not supported for this field");
    }
    if field.requires_key() {
        if condition
            .key
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Err("edge condition key is required");
        }
    } else if condition.key.is_some() {
        return Err("edge condition key is not supported for this field");
    }
    if condition.operator.needs_second_value()
        && condition
            .value2
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return Err("edge condition upper bound is required");
    }
    if condition.operator.is_unary() {
        return Ok(());
    }

    match field {
        SystemField::Weekday => {
            if weekday_selection(&condition.value).is_none() {
                return Err("edge condition weekday list is invalid");
            }
        }
        SystemField::Date => {
            if parse_date(&condition.value).is_none() {
                return Err("edge condition date is invalid");
            }
        }
        SystemField::Time => {
            if parse_time_minutes(&condition.value).is_none()
                || (condition.operator.needs_second_value()
                    && condition
                        .value2
                        .as_deref()
                        .is_none_or(|value| parse_time_minutes(value).is_none()))
            {
                return Err("edge condition time is invalid");
            }
        }
        SystemField::Battery => {
            if parse_percent(&condition.value).is_none()
                || (condition.operator.needs_second_value()
                    && condition
                        .value2
                        .as_deref()
                        .is_none_or(|value| parse_percent(value).is_none()))
            {
                return Err("edge condition battery level is invalid");
            }
        }
        SystemField::Path
            if matches!(
                condition.operator,
                ConditionOperator::ModifiedBefore | ConditionOperator::ModifiedAfter
            ) && parse_local_datetime(&condition.value).is_none() =>
        {
            return Err("edge condition modification time is invalid");
        }
        _ => {}
    }
    Ok(())
}

/// Compile a user-authored pattern exactly as the runner will evaluate it.
///
/// `multi_line` is on because the subject is almost always a multi-line step
/// output: without it `^FAIL` only matches when the whole output starts with
/// FAIL, so a line anchor silently never fires. `.` still stops at a newline,
/// matching what people expect from a line-oriented search.
pub fn compile_pattern(pattern: &str, case_sensitive: bool) -> Result<regex::Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .multi_line(true)
        .size_limit(1 << 20)
        .build()
}

/// Values a condition can compare against, gathered once per evaluation.
pub struct ConditionSubjects<'a> {
    pub output: &'a str,
    pub context: &'a str,
    pub trigger: &'a str,
    pub system: &'a SystemFacts,
}

enum SubjectValue<'a> {
    Text(&'a str),
    /// The value is available and empty: an unset environment variable, an
    /// empty clipboard, or no frontmost app.
    Empty,
    Unavailable,
}

impl<'a> SubjectValue<'a> {
    fn text(&self) -> Option<&'a str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Empty => Some(""),
            Self::Unavailable => None,
        }
    }
}

/// Evaluate one condition. An uncompilable regex is false, never an error: a
/// bad pattern must not end an otherwise healthy run. An unreadable machine
/// value is false too, including `isEmpty`, so a missing fact never satisfies a
/// branch by accident.
pub fn evaluate_condition(condition: &EdgeCondition, subjects: &ConditionSubjects<'_>) -> bool {
    match condition.source {
        ConditionSource::Output => evaluate_text(condition, SubjectValue::Text(subjects.output)),
        ConditionSource::Context => evaluate_text(condition, SubjectValue::Text(subjects.context)),
        ConditionSource::Trigger => evaluate_text(condition, SubjectValue::Text(subjects.trigger)),
        ConditionSource::System => evaluate_system(condition, subjects.system),
    }
}

fn evaluate_text(condition: &EdgeCondition, subject: SubjectValue<'_>) -> bool {
    let Some(subject) = subject.text() else {
        return false;
    };

    match condition.operator {
        ConditionOperator::IsEmpty => return subject.trim().is_empty(),
        ConditionOperator::IsNotEmpty => return !subject.trim().is_empty(),
        ConditionOperator::Matches => {
            return compile_pattern(&condition.value, condition.case_sensitive)
                .map(|re| re.is_match(subject))
                .unwrap_or(false);
        }
        ConditionOperator::IsOneOf | ConditionOperator::IsNotOneOf => {
            let candidate = if condition.case_sensitive {
                subject.to_string()
            } else {
                subject.to_lowercase()
            };
            let hit = condition.value.split(',').map(str::trim).any(|value| {
                if value.is_empty() {
                    return false;
                }
                let value = if condition.case_sensitive {
                    value.to_string()
                } else {
                    value.to_lowercase()
                };
                value == candidate
            });
            return match condition.operator {
                ConditionOperator::IsOneOf => hit,
                ConditionOperator::IsNotOneOf => !hit,
                _ => unreachable!("matched above"),
            };
        }
        _ => {}
    }

    // Text operators. Case-insensitive comparison lowercases both sides; this
    // is ASCII-simple folding, which is what users expect from a "contains" box.
    let (haystack, needle) = if condition.case_sensitive {
        (subject.to_string(), condition.value.clone())
    } else {
        (subject.to_lowercase(), condition.value.to_lowercase())
    };

    match condition.operator {
        ConditionOperator::Contains => haystack.contains(&needle),
        ConditionOperator::NotContains => !haystack.contains(&needle),
        ConditionOperator::Equals => haystack.trim() == needle.trim(),
        ConditionOperator::NotEquals => haystack.trim() != needle.trim(),
        ConditionOperator::Matches
        | ConditionOperator::IsEmpty
        | ConditionOperator::IsNotEmpty
        | ConditionOperator::IsOneOf
        | ConditionOperator::IsNotOneOf => unreachable!("handled above"),
        _ => false,
    }
}

fn evaluate_system(condition: &EdgeCondition, facts: &SystemFacts) -> bool {
    let Some(field) = condition.field else {
        return false;
    };
    match field {
        SystemField::Weekday => {
            let Some(days) = weekday_selection(&condition.value) else {
                return false;
            };
            let hit = days.contains(&facts.clock.weekday());
            match condition.operator {
                ConditionOperator::IsOneOf => hit,
                ConditionOperator::IsNotOneOf => !hit,
                _ => false,
            }
        }
        SystemField::Date => {
            let Some(bound) = parse_date(&condition.value) else {
                return false;
            };
            match condition.operator {
                ConditionOperator::Before => facts.clock.date < bound,
                ConditionOperator::After => facts.clock.date > bound,
                ConditionOperator::Equals => facts.clock.date == bound,
                _ => false,
            }
        }
        SystemField::Time => {
            let Some(subject) = time_minutes(facts.clock.time) else {
                return false;
            };
            let Some(bound) = parse_time_minutes(&condition.value) else {
                return false;
            };
            match condition.operator {
                ConditionOperator::Before => subject < bound,
                ConditionOperator::After => subject > bound,
                ConditionOperator::Between => condition
                    .value2
                    .as_deref()
                    .and_then(parse_time_minutes)
                    .is_some_and(|end| in_window(subject, bound, end)),
                ConditionOperator::NotBetween => condition
                    .value2
                    .as_deref()
                    .and_then(parse_time_minutes)
                    .is_some_and(|end| !in_window(subject, bound, end)),
                _ => false,
            }
        }
        SystemField::Timezone => {
            evaluate_text(condition, SubjectValue::Text(&facts.clock.timezone))
        }
        SystemField::Os => evaluate_enum(condition, &facts.os, normalize_os),
        SystemField::Hostname => {
            evaluate_text(condition, text_field_subject(field, condition, facts))
        }
        SystemField::Username => {
            evaluate_text(condition, text_field_subject(field, condition, facts))
        }
        SystemField::PowerSource => match &facts.power_source {
            Some(source) => evaluate_enum(condition, source, normalize_power),
            None => false,
        },
        SystemField::Battery => {
            let Some(level) = facts.battery_percent else {
                return false;
            };
            let Some(value) = parse_percent(&condition.value) else {
                return false;
            };
            match condition.operator {
                ConditionOperator::AtLeast => level >= value,
                ConditionOperator::AtMost => level <= value,
                ConditionOperator::Between => condition
                    .value2
                    .as_deref()
                    .and_then(parse_percent)
                    .is_some_and(|end| value <= level && level <= end),
                ConditionOperator::NotBetween => condition
                    .value2
                    .as_deref()
                    .and_then(parse_percent)
                    .is_some_and(|end| !(value <= level && level <= end)),
                _ => false,
            }
        }
        SystemField::Network => {
            let Some(online) = facts.network_online else {
                return false;
            };
            evaluate_enum(
                condition,
                if online { "online" } else { "offline" },
                normalize_network,
            )
        }
        SystemField::Path => evaluate_path(condition, facts),
        SystemField::Env => evaluate_text(condition, text_field_subject(field, condition, facts)),
        SystemField::FrontmostApp => {
            evaluate_text(condition, text_field_subject(field, condition, facts))
        }
        SystemField::Clipboard => {
            evaluate_text(condition, text_field_subject(field, condition, facts))
        }
    }
}

fn text_field_subject<'a>(
    field: SystemField,
    condition: &EdgeCondition,
    facts: &'a SystemFacts,
) -> SubjectValue<'a> {
    if facts.unavailable.contains_key(&field) {
        return SubjectValue::Unavailable;
    }
    match field {
        SystemField::Hostname => facts
            .hostname
            .as_deref()
            .map(SubjectValue::Text)
            .unwrap_or(SubjectValue::Empty),
        SystemField::Username => facts
            .username
            .as_deref()
            .map(SubjectValue::Text)
            .unwrap_or(SubjectValue::Empty),
        SystemField::Env => {
            let key = condition.key.as_deref().unwrap_or_default();
            match facts.env.get(key) {
                Some(value) if value.is_set => SubjectValue::Text(&value.value),
                Some(_) => SubjectValue::Empty,
                None => SubjectValue::Unavailable,
            }
        }
        SystemField::FrontmostApp => facts
            .frontmost_app
            .as_deref()
            .map(SubjectValue::Text)
            .unwrap_or(SubjectValue::Empty),
        SystemField::Clipboard => facts
            .clipboard
            .as_deref()
            .map(SubjectValue::Text)
            .unwrap_or(SubjectValue::Empty),
        _ => SubjectValue::Unavailable,
    }
}

fn evaluate_enum(
    condition: &EdgeCondition,
    actual: &str,
    normalize: impl Fn(&str) -> String,
) -> bool {
    let expected = normalize(&condition.value);
    let actual = normalize(actual);
    match condition.operator {
        ConditionOperator::Equals => expected == actual,
        ConditionOperator::NotEquals => expected != actual,
        _ => false,
    }
}

fn evaluate_path(condition: &EdgeCondition, facts: &SystemFacts) -> bool {
    let key = condition.key.as_deref().unwrap_or_default();
    let Some(state) = facts.paths.get(key) else {
        return false;
    };
    match condition.operator {
        ConditionOperator::Exists => state.exists,
        ConditionOperator::NotExists => !state.exists,
        ConditionOperator::IsFile => state.is_file,
        ConditionOperator::IsDirectory => state.is_dir,
        ConditionOperator::ModifiedBefore => {
            let Some(bound) = parse_local_datetime(&condition.value) else {
                return false;
            };
            state.modified.is_some_and(|modified| modified < bound)
        }
        ConditionOperator::ModifiedAfter => {
            let Some(bound) = parse_local_datetime(&condition.value) else {
                return false;
            };
            state.modified.is_some_and(|modified| modified > bound)
        }
        _ => false,
    }
}

/// Rule 1. `source_output` is `None` when the source step never produced a
/// result — it was skipped, or has not run — which makes the edge dead.
pub fn edge_fires(
    edge: &Value,
    source_output: Option<&str>,
    context: &str,
    trigger: &str,
    system: &SystemFacts,
) -> bool {
    let Some(output) = source_output else {
        return false;
    };
    match EdgeCondition::parse(edge) {
        None => true,
        Some(condition) => evaluate_condition(
            &condition,
            &ConditionSubjects {
                output,
                context,
                trigger,
                system,
            },
        ),
    }
}

/// Incoming edges per target node, in graph order. Edges referencing a node
/// that is not in the graph are dropped, matching `topological_order`.
pub fn incoming_edges(nodes: &[Value], edges: &[Value]) -> HashMap<String, Vec<Value>> {
    let ids: HashSet<&str> = nodes
        .iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str))
        .collect();

    let mut incoming: HashMap<String, Vec<Value>> = HashMap::new();
    for edge in edges {
        let source = edge.get("source").and_then(Value::as_str);
        let target = edge.get("target").and_then(Value::as_str);
        if let (Some(source), Some(target)) = (source, target) {
            if ids.contains(source) && ids.contains(target) {
                incoming
                    .entry(target.to_string())
                    .or_default()
                    .push(edge.clone());
            }
        }
    }
    incoming
}

/// Collect the machine fields and keys this graph actually tests, so a run
/// never reads the clipboard or probes the battery for conditions it does not
/// have.
pub fn system_requirements(edges: &[Value]) -> SystemRequirements {
    let mut requirements = SystemRequirements::default();
    for edge in edges {
        let Some(condition) = EdgeCondition::parse(edge) else {
            continue;
        };
        let Some(field) = condition.field else {
            continue;
        };
        requirements.fields.insert(field);
        if let Some(key) = &condition.key {
            match field {
                SystemField::Path => {
                    requirements.paths.insert(key.clone());
                }
                SystemField::Env => {
                    requirements.env.insert(key.clone());
                }
                _ => {}
            }
        }
    }
    requirements
}

/// Why a step is not running. `Some` is the reason to record on the skipped
/// step; `None` means the step runs.
pub struct SkipReason(pub String);

/// The user-facing reason a machine value could not be read, if any.
pub fn condition_unavailable<'a>(
    condition: &EdgeCondition,
    system: &'a SystemFacts,
) -> Option<&'a str> {
    system
        .unavailable
        .get(&condition.system_field()?)
        .map(String::as_str)
}

/// Rules 2 and 3. Returns `None` when the step should run.
pub fn skip_reason(
    node_id: &str,
    incoming: &HashMap<String, Vec<Value>>,
    node_outputs: &HashMap<String, String>,
    context: &str,
    trigger: &str,
    system: &SystemFacts,
) -> Option<SkipReason> {
    let edges = incoming.get(node_id)?;
    if edges.is_empty() {
        // A root, or every incoming edge pointed at a missing node.
        return None;
    }

    let mut unmet: Vec<String> = Vec::new();
    for edge in edges {
        let source = edge
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source_output = node_outputs.get(source).map(String::as_str);
        if edge_fires(edge, source_output, context, trigger, system) {
            return None;
        }
        unmet.push(match EdgeCondition::parse(edge) {
            Some(condition) if source_output.is_some() => {
                let mut text = format!("{source}: {}", condition.summary());
                if let Some(reason) = condition_unavailable(&condition, system) {
                    text.push_str(&format!(" ({reason})"));
                }
                text
            }
            _ => format!("{source}: did not run"),
        });
    }

    Some(SkipReason(format!(
        "Skipped — no incoming connection fired ({})",
        unmet.join("; ")
    )))
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

fn parse_time_minutes(value: &str) -> Option<u32> {
    let time = NaiveTime::parse_from_str(value.trim(), "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(value.trim(), "%H:%M:%S"))
        .ok()?;
    time_minutes(time)
}

fn time_minutes(time: NaiveTime) -> Option<u32> {
    Some(time.hour() * 60 + time.minute())
}

/// Inclusive window. A start later than the end wraps across midnight, so
/// `22:00 – 06:00` means an overnight window rather than an impossible one.
fn in_window(subject: u32, start: u32, end: u32) -> bool {
    if start <= end {
        subject >= start && subject <= end
    } else {
        subject >= start || subject <= end
    }
}

fn parse_percent(value: &str) -> Option<u8> {
    let value = value.trim().trim_end_matches('%').trim();
    value.parse::<u8>().ok().filter(|value| *value <= 100)
}

fn parse_local_datetime(value: &str) -> Option<DateTime<Local>> {
    let value = value.trim();
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"))
        .ok()?;
    Local.from_local_datetime(&naive).earliest()
}

fn parse_weekday(value: &str) -> Option<Weekday> {
    match value.trim().to_ascii_lowercase().as_str() {
        "monday" | "mon" => Some(Weekday::Mon),
        "tuesday" | "tue" | "tues" => Some(Weekday::Tue),
        "wednesday" | "wed" => Some(Weekday::Wed),
        "thursday" | "thu" | "thur" | "thurs" => Some(Weekday::Thu),
        "friday" | "fri" => Some(Weekday::Fri),
        "saturday" | "sat" => Some(Weekday::Sat),
        "sunday" | "sun" => Some(Weekday::Sun),
        _ => None,
    }
}

fn weekday_selection(value: &str) -> Option<Vec<Weekday>> {
    let mut days = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let weekday = parse_weekday(part)?;
        if !days.contains(&weekday) {
            days.push(weekday);
        }
    }
    if days.is_empty() {
        None
    } else {
        Some(days)
    }
}

fn weekday_short_labels(value: &str) -> Vec<&'static str> {
    value
        .split(',')
        .filter_map(parse_weekday)
        .map(|weekday| match weekday {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun",
        })
        .collect()
}

fn normalize_os(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "macos" | "mac os" | "mac" | "os x" => "macos".into(),
        "windows" | "win" => "windows".into(),
        "linux" => "linux".into(),
        other => other.into(),
    }
}

fn normalize_power(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "ac" | "ac power" | "wall" | "plugged in" => "ac".into(),
        "battery" | "battery power" => "battery".into(),
        other => other.into(),
    }
}

fn normalize_network(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "online" | "connected" | "up" => "online".into(),
        "offline" | "disconnected" | "down" => "offline".into(),
        other => other.into(),
    }
}

fn os_display(value: &str) -> String {
    match normalize_os(value).as_str() {
        "macos" => "macOS".into(),
        "windows" => "Windows".into(),
        "linux" => "Linux".into(),
        other => other.into(),
    }
}

fn power_display(value: &str) -> String {
    match normalize_power(value).as_str() {
        "ac" => "AC power".into(),
        "battery" => "Battery".into(),
        other => other.into(),
    }
}

fn network_display(value: &str) -> String {
    match normalize_network(value).as_str() {
        "online" => "online".into(),
        "offline" => "offline".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_facts::{ClockFacts, EnvValue, PathState};
    use serde_json::json;
    use std::sync::LazyLock;

    fn clock() -> ClockFacts {
        ClockFacts::at(
            NaiveDate::from_ymd_opt(2026, 11, 20).expect("date"),
            NaiveTime::from_hms_opt(14, 30, 0).expect("time"),
            "America/New_York",
        )
    }

    fn facts() -> &'static SystemFacts {
        static FACTS: LazyLock<SystemFacts> = LazyLock::new(|| SystemFacts::new(clock()));
        &FACTS
    }

    fn subjects<'a>(output: &'a str) -> ConditionSubjects<'a> {
        ConditionSubjects {
            output,
            context: "",
            trigger: "",
            system: facts(),
        }
    }

    fn condition(operator: &str, value: &str) -> EdgeCondition {
        EdgeCondition {
            source: ConditionSource::Output,
            field: None,
            key: None,
            operator: ConditionOperator::parse(operator).expect("operator"),
            value: value.into(),
            value2: None,
            case_sensitive: false,
        }
    }

    fn system_condition(
        field: SystemField,
        operator: &str,
        value: &str,
        value2: Option<&str>,
    ) -> EdgeCondition {
        EdgeCondition {
            source: ConditionSource::System,
            field: Some(field),
            key: if field.requires_key() {
                Some(match field {
                    SystemField::Path => "report.md".into(),
                    SystemField::Env => "CI".into(),
                    _ => unreachable!(),
                })
            } else {
                None
            },
            operator: ConditionOperator::parse(operator).expect("operator"),
            value: value.into(),
            value2: value2.map(str::to_owned),
            case_sensitive: false,
        }
    }

    fn system_subjects<'a>(facts: &'a SystemFacts) -> ConditionSubjects<'a> {
        ConditionSubjects {
            output: "",
            context: "",
            trigger: "",
            system: facts,
        }
    }

    #[test]
    fn text_operators_cover_both_directions() {
        assert!(evaluate_condition(
            &condition("contains", "fail"),
            &subjects("Tests FAILED")
        ));
        assert!(!evaluate_condition(
            &condition("notContains", "fail"),
            &subjects("Tests FAILED")
        ));
        assert!(evaluate_condition(
            &condition("equals", "done"),
            &subjects("  DONE  ")
        ));
        assert!(evaluate_condition(
            &condition("notEquals", "done"),
            &subjects("not done")
        ));
    }

    #[test]
    fn case_sensitivity_is_respected() {
        let mut sensitive = condition("contains", "FAIL");
        sensitive.case_sensitive = true;
        assert!(evaluate_condition(&sensitive, &subjects("a FAIL here")));
        assert!(!evaluate_condition(&sensitive, &subjects("a fail here")));
        assert!(evaluate_condition(
            &condition("contains", "FAIL"),
            &subjects("a fail here")
        ));
    }

    #[test]
    fn empty_operators_ignore_the_value_and_whitespace() {
        assert!(evaluate_condition(
            &condition("isEmpty", "ignored"),
            &subjects("   \n ")
        ));
        assert!(!evaluate_condition(
            &condition("isEmpty", ""),
            &subjects("x")
        ));
        assert!(evaluate_condition(
            &condition("isNotEmpty", ""),
            &subjects("x")
        ));
    }

    #[test]
    fn line_anchors_match_line_starts_in_multi_line_output() {
        // Step output is nearly always multi-line. Without multi_line this is
        // false and the branch silently never runs.
        assert!(evaluate_condition(
            &condition("matches", "^FAIL"),
            &subjects("tests ran\nFAIL: 3 suites")
        ));
        assert!(evaluate_condition(
            &condition("matches", "suites$"),
            &subjects("FAIL: 3 suites\nsee log")
        ));
    }

    #[test]
    fn a_dot_still_stops_at_a_line_break() {
        // Line-oriented search: `.` spanning lines would surprise.
        assert!(!evaluate_condition(
            &condition("matches", "FAIL.*ERROR"),
            &subjects("FAIL\nERROR")
        ));
    }

    #[test]
    fn look_around_is_unsupported_and_must_be_reported_not_silently_false() {
        // The Rust engine has no look-around. The editor must surface this
        // instead of saving a pattern that can never fire.
        assert!(compile_pattern("FAIL(?!ED)", false).is_err());
        assert!(compile_pattern("(?=FAIL)", false).is_err());
        // Inline flags are supported here but rejected by JavaScript's engine.
        assert!(compile_pattern("(?i)fail", true).is_ok());
    }

    #[test]
    fn regex_matches_and_bad_patterns_are_false() {
        assert!(evaluate_condition(
            &condition("matches", "FAIL|error"),
            &subjects("an error occurred")
        ));
        // Unbalanced group: must be false, never a panic or a run failure.
        assert!(!evaluate_condition(
            &condition("matches", "FAIL("),
            &subjects("FAIL(")
        ));
    }

    #[test]
    fn each_source_reads_its_own_subject() {
        let subjects = ConditionSubjects {
            output: "out",
            context: "ctx",
            trigger: "trg",
            system: facts(),
        };
        for (source, hit) in [
            (ConditionSource::Output, "out"),
            (ConditionSource::Context, "ctx"),
            (ConditionSource::Trigger, "trg"),
        ] {
            let mut condition = EdgeCondition {
                source,
                field: None,
                key: None,
                operator: ConditionOperator::Equals,
                value: hit.into(),
                value2: None,
                case_sensitive: false,
            };
            condition.value = hit.into();
            assert!(evaluate_condition(&condition, &subjects), "{hit}");
        }
    }

    #[test]
    fn missing_or_malformed_conditions_read_as_unconditional() {
        assert!(EdgeCondition::parse(&json!({"source": "a", "target": "b"})).is_none());
        assert!(EdgeCondition::parse(&json!({"data": {}})).is_none());
        assert!(EdgeCondition::parse(
            &json!({"data": {"condition": {"source": "nope", "operator": "contains"}}})
        )
        .is_none());
        assert!(EdgeCondition::parse(
            &json!({"data": {"condition": {"source": "output", "operator": "nope"}}})
        )
        .is_none());
        // Typed operators are only valid with a machine field.
        assert!(EdgeCondition::parse(
            &json!({"data": {"condition": {"source": "output", "operator": "before", "value": "x"}}})
        )
        .is_none());
    }

    #[test]
    fn oversized_values_are_truncated_on_a_char_boundary() {
        let long = "é".repeat(MAX_CONDITION_VALUE_BYTES);
        let parsed = EdgeCondition::parse(
            &json!({"data": {"condition": {"source": "output", "operator": "contains", "value": long}}}),
        )
        .expect("condition");
        assert!(parsed.value.len() <= MAX_CONDITION_VALUE_BYTES);
        assert!(parsed.value.chars().all(|c| c == 'é'));
    }

    #[test]
    fn an_edge_from_a_step_that_never_ran_is_dead() {
        let edge = json!({"source": "a", "target": "b"});
        assert!(!edge_fires(&edge, None, "", "", facts()));
        assert!(edge_fires(&edge, Some(""), "", "", facts()));
    }

    #[test]
    fn weekday_conditions_use_the_run_start_clock() {
        // 2026-11-20 is a Friday.
        let condition = system_condition(SystemField::Weekday, "isOneOf", "Saturday, Sunday", None);
        assert!(!evaluate_condition(&condition, &system_subjects(facts())));

        let condition = system_condition(SystemField::Weekday, "isOneOf", "Friday", None);
        assert!(evaluate_condition(&condition, &system_subjects(facts())));

        let condition =
            system_condition(SystemField::Weekday, "isNotOneOf", "Saturday, Sunday", None);
        assert!(evaluate_condition(&condition, &system_subjects(facts())));
    }

    #[test]
    fn date_and_time_comparisons_are_inclusive_and_wrap_at_midnight() {
        let before = system_condition(SystemField::Date, "before", "2026-11-21", None);
        assert!(evaluate_condition(&before, &system_subjects(facts())));
        let after = system_condition(SystemField::Date, "after", "2026-11-20", None);
        assert!(!evaluate_condition(&after, &system_subjects(facts())));

        let business_hours = system_condition(SystemField::Time, "between", "09:00", Some("17:00"));
        assert!(evaluate_condition(
            &business_hours,
            &system_subjects(facts())
        ));

        let overnight = system_condition(SystemField::Time, "between", "22:00", Some("06:00"));
        assert!(!evaluate_condition(&overnight, &system_subjects(facts())));

        let clock = ClockFacts::at(
            NaiveDate::from_ymd_opt(2026, 11, 20).expect("date"),
            NaiveTime::from_hms_opt(23, 10, 0).expect("time"),
            "America/New_York",
        );
        let night_facts = SystemFacts::new(clock);
        assert!(evaluate_condition(
            &overnight,
            &system_subjects(&night_facts)
        ));
    }

    #[test]
    fn battery_network_power_and_os_are_typed() {
        let mut facts = facts().clone();
        facts.battery_percent = Some(15);
        facts.power_source = Some("Battery".into());
        facts.network_online = Some(true);
        facts.os = "macos".into();

        assert!(evaluate_condition(
            &system_condition(SystemField::Battery, "atLeast", "10", None),
            &system_subjects(&facts)
        ));
        assert!(!evaluate_condition(
            &system_condition(SystemField::Battery, "atLeast", "20", None),
            &system_subjects(&facts)
        ));
        assert!(evaluate_condition(
            &system_condition(SystemField::Battery, "between", "10", Some("20")),
            &system_subjects(&facts)
        ));
        assert!(evaluate_condition(
            &system_condition(SystemField::PowerSource, "equals", "Battery", None),
            &system_subjects(&facts)
        ));
        assert!(evaluate_condition(
            &system_condition(SystemField::Network, "equals", "online", None),
            &system_subjects(&facts)
        ));
        assert!(evaluate_condition(
            &system_condition(SystemField::Os, "equals", "macOS", None),
            &system_subjects(&facts)
        ));
    }

    #[test]
    fn unset_env_is_empty_but_unreadable_facts_are_never_empty() {
        let mut facts = facts().clone();
        facts.env.insert("CI".into(), EnvValue::default());
        assert!(evaluate_condition(
            &system_condition(SystemField::Env, "isEmpty", "", None),
            &system_subjects(&facts)
        ));
        assert!(evaluate_condition(
            &system_condition(SystemField::Env, "equals", "", None),
            &system_subjects(&facts)
        ));

        let mut facts = facts.clone();
        facts.unavailable.insert(
            SystemField::Clipboard,
            "The clipboard is unavailable.".into(),
        );
        assert!(!evaluate_condition(
            &system_condition(SystemField::Clipboard, "isEmpty", "", None),
            &system_subjects(&facts)
        ));
    }

    #[test]
    fn path_conditions_use_resolved_metadata() {
        let mut facts = facts().clone();
        facts.paths.insert(
            "report.md".into(),
            PathState {
                exists: true,
                is_file: true,
                is_dir: false,
                modified: Some(
                    Local
                        .with_ymd_and_hms(2026, 11, 19, 10, 0, 0)
                        .earliest()
                        .expect("local time"),
                ),
            },
        );
        assert!(evaluate_condition(
            &system_condition(SystemField::Path, "exists", "", None),
            &system_subjects(&facts)
        ));
        assert!(evaluate_condition(
            &system_condition(SystemField::Path, "isFile", "", None),
            &system_subjects(&facts)
        ));
        assert!(!evaluate_condition(
            &system_condition(SystemField::Path, "isDirectory", "", None),
            &system_subjects(&facts)
        ));
        assert!(evaluate_condition(
            &system_condition(
                SystemField::Path,
                "modifiedBefore",
                "2026-11-20T00:00",
                None
            ),
            &system_subjects(&facts)
        ));
        assert!(!evaluate_condition(
            &system_condition(SystemField::Path, "modifiedAfter", "2026-11-20T00:00", None),
            &system_subjects(&facts)
        ));
    }

    #[test]
    fn a_system_condition_requires_a_field_and_a_key_when_needed() {
        assert!(EdgeCondition::parse_value(&json!({
            "source": "system", "operator": "contains", "value": "x"
        }))
        .is_none());
        assert!(EdgeCondition::parse_value(&json!({
            "source": "system", "field": "env", "operator": "contains", "value": "x"
        }))
        .is_none());
        assert!(EdgeCondition::parse_value(&json!({
            "source": "system", "field": "env", "key": "CI", "operator": "contains", "value": "x"
        }))
        .is_some());
        assert!(EdgeCondition::parse_value(&json!({
            "source": "output", "field": "weekday", "operator": "contains", "value": "x"
        }))
        .is_none());
    }

    #[test]
    fn system_summaries_name_the_subject_and_bounds() {
        let weekday = system_condition(SystemField::Weekday, "isOneOf", "Saturday, Sunday", None);
        assert_eq!(weekday.summary(), "day of week is one of Sat, Sun");

        let time = system_condition(SystemField::Time, "between", "09:00", Some("17:00"));
        assert_eq!(
            time.summary(),
            "time of day is between \"09:00\" and \"17:00\""
        );

        let path = system_condition(SystemField::Path, "exists", "", None);
        assert_eq!(path.summary(), "file \"report.md\" exists");
    }

    #[test]
    fn requirements_collect_only_machine_conditions() {
        let edges = vec![
            json!({"id":"a","source":"a","target":"b","data":{"condition":{
                "source":"system","field":"weekday","operator":"isOneOf","value":"Monday"
            }}}),
            json!({"id":"b","source":"b","target":"c","data":{"condition":{
                "source":"system","field":"env","key":"CI","operator":"equals","value":"true"
            }}}),
            json!({"id":"c","source":"c","target":"d","data":{"condition":{
                "source":"output","operator":"contains","value":"x"
            }}}),
        ];
        let requirements = system_requirements(&edges);
        assert!(requirements.fields.contains(&SystemField::Weekday));
        assert!(requirements.fields.contains(&SystemField::Env));
        assert!(requirements.env.contains("CI"));
        assert!(!requirements.fields.contains(&SystemField::Clipboard));
    }

    fn node(id: &str) -> Value {
        json!({"id": id, "type": "agent", "data": {}})
    }

    fn conditional_edge(source: &str, target: &str, needle: &str) -> Value {
        json!({
            "id": format!("{source}-{target}"),
            "source": source,
            "target": target,
            "data": {"condition": {
                "source": "output",
                "operator": "contains",
                "value": needle,
            }},
        })
    }

    #[test]
    fn a_root_always_runs() {
        let nodes = vec![node("a"), node("b")];
        let edges = vec![json!({"id": "e", "source": "a", "target": "b"})];
        let incoming = incoming_edges(&nodes, &edges);
        assert!(skip_reason("a", &incoming, &HashMap::new(), "", "", facts()).is_none());
    }

    #[test]
    fn one_firing_edge_is_enough_at_a_join() {
        let nodes = vec![node("a"), node("b"), node("join")];
        let edges = vec![
            conditional_edge("a", "join", "yes"),
            conditional_edge("b", "join", "yes"),
        ];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), "yes".to_string());
        outputs.insert("b".to_string(), "no".to_string());
        assert!(skip_reason("join", &incoming, &outputs, "", "", facts()).is_none());
    }

    #[test]
    fn a_step_with_no_firing_edge_is_skipped_with_a_reason() {
        let nodes = vec![node("a"), node("b")];
        let edges = vec![conditional_edge("a", "b", "yes")];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), "no".to_string());
        let reason = skip_reason("b", &incoming, &outputs, "", "", facts()).expect("skipped");
        assert!(reason.0.contains("step output contains"));
        assert!(reason.0.starts_with("Skipped"));
    }

    #[test]
    fn an_unavailable_machine_value_is_named_in_the_skip_reason() {
        let nodes = vec![node("a"), node("b")];
        let edges = vec![json!({
            "id": "ab", "source": "a", "target": "b",
            "data": {"condition": {
                "source": "system", "field": "clipboard",
                "operator": "contains", "value": "secret"
            }}
        })];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), "ran".to_string());
        let mut unavailable = facts().clone();
        unavailable.unavailable.insert(
            SystemField::Clipboard,
            "The clipboard is unavailable.".into(),
        );
        let reason = skip_reason("b", &incoming, &outputs, "", "", &unavailable).expect("skipped");
        assert!(reason.0.contains("The clipboard is unavailable."));
    }

    #[test]
    fn skipping_propagates_because_a_skipped_step_has_no_output() {
        let nodes = vec![node("a"), node("b"), node("c")];
        let edges = vec![
            conditional_edge("a", "b", "yes"),
            json!({"id": "bc", "source": "b", "target": "c"}),
        ];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), "no".to_string());

        assert!(skip_reason("b", &incoming, &outputs, "", "", facts()).is_some());
        // `b` recorded no output, so the unconditional `b → c` edge is dead too.
        let reason = skip_reason("c", &incoming, &outputs, "", "", facts()).expect("skipped");
        assert!(reason.0.contains("did not run"));
    }

    #[test]
    fn a_graph_with_no_conditions_never_skips_a_reached_step() {
        let nodes = vec![node("a"), node("b"), node("c")];
        let edges = vec![
            json!({"id": "ab", "source": "a", "target": "b"}),
            json!({"id": "bc", "source": "b", "target": "c"}),
        ];
        let incoming = incoming_edges(&nodes, &edges);
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), String::new());
        assert!(skip_reason("b", &incoming, &outputs, "", "", facts()).is_none());
        outputs.insert("b".to_string(), String::new());
        assert!(skip_reason("c", &incoming, &outputs, "", "", facts()).is_none());
    }

    #[test]
    fn edges_pointing_at_missing_nodes_are_dropped() {
        let nodes = vec![node("a")];
        let edges = vec![json!({"id": "e", "source": "ghost", "target": "a"})];
        let incoming = incoming_edges(&nodes, &edges);
        assert!(incoming.is_empty());
        assert!(skip_reason("a", &incoming, &HashMap::new(), "", "", facts()).is_none());
    }
}
