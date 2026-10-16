use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use uuid::Uuid;

pub const FORMAT: &str = "alfred.workflow-template";
/// Highest schema version this build writes. Version 2 added conditional
/// connections; a template carrying one must not be read by a build that would
/// ignore the condition and run every branch.
pub const SCHEMA_VERSION: u16 = 2;
/// Oldest version this build still reads. A v1 template simply has no
/// conditions, so already-shared links keep working.
pub const MIN_SCHEMA_VERSION: u16 = 1;
/// The version a conditional connection requires.
const CONDITIONAL_EDGE_SCHEMA_VERSION: u16 = 2;
pub const MAX_BYTES: usize = 256 * 1024;
pub const MAX_NODES: usize = 100;
pub const MAX_EDGES: usize = 300;
pub const MAX_PARAMETERS: usize = 64;
pub const MAX_STARTS: usize = 16;
pub const MAX_DEPTH: usize = 16;
pub const MAX_TITLE: usize = 120;
pub const MAX_DESCRIPTION: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateEnvelope {
    pub format: String,
    pub schema_version: u16,
    pub min_alfred_version: String,
    pub title: String,
    pub description: String,
    pub graph: TemplateGraph,
    pub parameters: Vec<Parameter>,
    pub requirements: Vec<Requirement>,
    pub starts: Vec<StartRecipe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateGraph {
    pub nodes: Vec<TemplateNode>,
    pub edges: Vec<TemplateEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub data: Map<String, Value>,
    pub position: Position,
}

/// A connection's condition (Plan 053). Absent means the connection always
/// fires. This must travel with the template: a shared workflow that arrives
/// with its conditions stripped would run every branch unconditionally, which
/// is a wrong workflow that looks correct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateEdgeCondition {
    pub source: String,
    pub operator: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateEdgeData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<TemplateEdgeCondition>,
}

impl TemplateEdgeData {
    fn is_empty(&self) -> bool {
        self.condition.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_handle: Option<String>,
    #[serde(default, skip_serializing_if = "TemplateEdgeData::is_empty")]
    pub data: TemplateEdgeData,
}

/// Enumerated values the runner understands. An unknown value is rejected on
/// import rather than coerced, so a template from a newer build fails loudly
/// instead of quietly running the wrong branch.
const EDGE_CONDITION_SOURCES: [&str; 3] = ["output", "context", "trigger"];
const EDGE_CONDITION_OPERATORS: [&str; 7] = [
    "contains",
    "notContains",
    "equals",
    "notEquals",
    "matches",
    "isEmpty",
    "isNotEmpty",
];
/// Matches `MAX_CONDITION_VALUE_BYTES` in `runner::conditions`.
const MAX_EDGE_CONDITION_VALUE_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Parameter {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub kind: ParameterKind,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_action: Option<String>,
    pub target: ParameterTarget,
    /// Additional complete-field bindings used when repeated local connection
    /// references are intentionally grouped into one setup question.
    #[serde(default)]
    pub targets: Vec<ParameterTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ParameterKind {
    Text,
    Folder,
    File,
    AgentAccount,
    AppConnection,
    AppResource,
    Memory,
    Channel,
    Email,
    SecretBacked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParameterTarget {
    pub kind: TargetKind,
    pub node_id: Option<String>,
    pub field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    Node,
    Start,
    WorkingDirectory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirement {
    pub kind: RequirementKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RequirementKind {
    Agent,
    Harness,
    Model,
    Skill,
    Interpreter,
    AppAction,
    AppEvent,
}
impl RequirementKind {
    pub fn kind_string(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Harness => "harness",
            Self::Model => "model",
            Self::Skill => "skill",
            Self::Interpreter => "interpreter",
            Self::AppAction => "appAction",
            Self::AppEvent => "appEvent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRecipe {
    pub kind: StartKind,
    pub config: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StartKind {
    Schedule,
    File,
    AppEvent,
    Webhook,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TemplateError {
    #[error("template is not valid JSON: {0}")]
    Json(String),
    #[error("template is too large (maximum {MAX_BYTES} bytes)")]
    TooLarge,
    #[error("template is incompatible: {0}")]
    Invalid(String),
    #[error("template contains unsupported private value in {0}")]
    Sensitive(String),
}

impl From<serde_json::Error> for TemplateError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}

const NODE_TYPES: &[&str] = &[
    "input",
    "prompt",
    "agent",
    "customAgent",
    "chooseOutput",
    "output",
    "memory",
    "identity",
    "template",
    "fileInject",
    "gitStatus",
    "shell",
    "script",
    "http",
    "notify",
    "writeFile",
    "gitHost",
    "appAction",
];

fn has_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "sk-",
        "ghp_",
        "github_pat_",
        "xoxb-",
        "bearer ",
        "password=",
        "token=",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || value.contains("/Users/")
        || value.contains("/home/")
        || value.contains("C:\\Users\\")
        || value.contains("\\AppData\\")
}

fn has_secret_url_query(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "token=",
        "secret=",
        "password=",
        "api_key=",
        "apikey=",
        "auth=",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn scan_text(value: &Value, path: &str) -> Result<(), TemplateError> {
    match value {
        Value::String(text) if has_sensitive_value(text) => {
            Err(TemplateError::Sensitive(path.into()))
        }
        Value::Array(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(i, v)| scan_text(v, &format!("{path}[{i}]"))),
        Value::Object(map) => map
            .iter()
            .try_for_each(|(key, v)| scan_text(v, &format!("{path}.{key}"))),
        _ => Ok(()),
    }
}

fn parameter(
    parameters: &mut Vec<Parameter>,
    key: &str,
    label: &str,
    kind: ParameterKind,
    node_id: Option<&str>,
    field: &str,
) {
    let target = ParameterTarget {
        kind: if node_id.is_some() {
            TargetKind::Node
        } else {
            TargetKind::WorkingDirectory
        },
        node_id: node_id.map(str::to_owned),
        field: field.into(),
    };
    if let Some(existing) = parameters.iter_mut().find(|p| p.key == key) {
        if existing.target != target && !existing.targets.contains(&target) {
            existing.targets.push(target);
        }
        return;
    }
    parameters.push(Parameter {
        key: key.into(),
        label: label.into(),
        description: String::new(),
        kind,
        required: true,
        allowed_provider: None,
        allowed_action: None,
        target,
        targets: Vec::new(),
    });
}

fn grouped_reference_key(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}.{suffix}")
}

fn keep(map: &Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    keys.iter()
        .filter_map(|key| map.get(*key).map(|v| ((*key).into(), v.clone())))
        .collect()
}

fn sanitize_node(
    node: &Value,
    parameters: &mut Vec<Parameter>,
    requirements: &mut Vec<Requirement>,
) -> Result<TemplateNode, TemplateError> {
    let object = node
        .as_object()
        .ok_or_else(|| TemplateError::Invalid("graph node must be an object".into()))?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| TemplateError::Invalid("node id is required".into()))?
        .to_owned();
    let node_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| TemplateError::Invalid(format!("node {id} has no type")))?
        .to_owned();
    if !NODE_TYPES.contains(&node_type.as_str()) {
        return Err(TemplateError::Invalid(format!(
            "unsupported node type {node_type}"
        )));
    }
    let position = object
        .get("position")
        .and_then(Value::as_object)
        .ok_or_else(|| TemplateError::Invalid(format!("node {id} has no position")))?;
    let x = position
        .get("x")
        .and_then(Value::as_f64)
        .ok_or_else(|| TemplateError::Invalid(format!("node {id} has invalid x")))?;
    let y = position
        .get("y")
        .and_then(Value::as_f64)
        .ok_or_else(|| TemplateError::Invalid(format!("node {id} has invalid y")))?;
    if !x.is_finite() || !y.is_finite() {
        return Err(TemplateError::Invalid(format!(
            "node {id} has non-finite position"
        )));
    }
    let data = object
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut clean = match node_type.as_str() {
        "input" | "prompt" => {
            let mut result = keep(&data, &["label", "prompt", "blocked"]);
            if let Some(script) = data.get("script") {
                let mut script = script.as_object().cloned().ok_or_else(|| {
                    TemplateError::Invalid(format!("node {id}.script must be a structured script"))
                })?;
                let source = script
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("inline");
                if !matches!(source, "file" | "inline") {
                    return Err(TemplateError::Invalid(format!(
                        "node {id}.script.source is invalid"
                    )));
                }
                for key in script.keys() {
                    if !["source", "path", "body", "interpreter", "message", "run"]
                        .contains(&key.as_str())
                    {
                        return Err(TemplateError::Invalid(format!(
                            "unsupported field {id}.script.{key}"
                        )));
                    }
                }
                if source == "file" {
                    parameter(
                        parameters,
                        &format!("{id}.script.path"),
                        "Script file",
                        ParameterKind::File,
                        Some(&id),
                        "script.path",
                    );
                    script.insert("path".into(), Value::String(String::new()));
                }
                result.insert("script".into(), Value::Object(script));
            }
            if data.get("attachments").is_some() {
                parameter(
                    parameters,
                    &format!("{id}.attachments"),
                    "Files or folders",
                    ParameterKind::Folder,
                    Some(&id),
                    "attachments",
                );
                result.insert("attachments".into(), Value::Array(vec![]));
            }
            result
        }
        "agent" => {
            let mut result = keep(
                &data,
                &[
                    "label",
                    "provider",
                    "harness",
                    "model",
                    "skillNames",
                    "skillName",
                ],
            );
            if let Some(account_ref) = data
                .get("accountRef")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                let key = grouped_reference_key("agent.account", account_ref);
                parameter(
                    parameters,
                    &key,
                    "Agent account",
                    ParameterKind::AgentAccount,
                    Some(&id),
                    "accountRef",
                );
                if let Some(item) = parameters.iter_mut().find(|item| item.key == key) {
                    item.allowed_provider = result
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
            }
            if let Some(provider) = result.get("provider").and_then(Value::as_str) {
                requirements.push(Requirement {
                    kind: RequirementKind::Agent,
                    name: provider.into(),
                    provider: Some(provider.into()),
                    capability: None,
                });
            }
            if let Some(harness) = result.get("harness").and_then(Value::as_str) {
                requirements.push(Requirement {
                    kind: RequirementKind::Harness,
                    name: harness.into(),
                    provider: result
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    capability: None,
                });
            }
            if let Some(model) = result
                .get("model")
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
            {
                requirements.push(Requirement {
                    kind: RequirementKind::Model,
                    name: model.into(),
                    provider: result
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    capability: None,
                });
            }
            if let Some(skills) = result.get("skillNames").and_then(Value::as_array) {
                for skill in skills
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|v| !v.trim().is_empty())
                {
                    requirements.push(Requirement {
                        kind: RequirementKind::Skill,
                        name: skill.into(),
                        provider: None,
                        capability: None,
                    });
                }
            }
            result
        }
        "memory" => {
            let mut result = keep(&data, &["label"]);
            parameter(
                parameters,
                &format!("{id}.memory"),
                "Memory selection",
                ParameterKind::Memory,
                Some(&id),
                "memoryIds",
            );
            result.insert("memoryIds".into(), Value::Array(vec![]));
            result
        }
        "fileInject" => {
            let mut result = keep(&data, &["kind", "label"]);
            parameter(
                parameters,
                &format!("{id}.paths"),
                "Files or folders",
                ParameterKind::Folder,
                Some(&id),
                "paths",
            );
            result.insert("paths".into(), Value::Array(vec![]));
            result
        }
        "writeFile" => {
            let mut result = keep(&data, &["kind", "label", "content"]);
            parameter(
                parameters,
                &format!("{id}.path"),
                "Output file",
                ParameterKind::File,
                Some(&id),
                "path",
            );
            result.insert("path".into(), Value::String(String::new()));
            result
        }
        "appAction" => {
            let mut result = keep(&data, &["type", "label", "providerId", "actionId"]);
            let provider = data
                .get("providerId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let action = data
                .get("actionId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if provider.is_empty() || action.is_empty() {
                return Err(TemplateError::Invalid(format!(
                    "app action {id} is not configured"
                )));
            }
            requirements.push(Requirement {
                kind: RequirementKind::AppAction,
                name: action.into(),
                provider: Some(provider.into()),
                capability: Some("execute".into()),
            });
            let connection_key = data
                .get("connectionId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(|value| grouped_reference_key("app.connection", value))
                .unwrap_or_else(|| format!("{id}.connection"));
            parameter(
                parameters,
                &connection_key,
                "Connected account",
                ParameterKind::AppConnection,
                Some(&id),
                "connectionId",
            );
            if let Some(item) = parameters
                .iter_mut()
                .find(|item| item.key == connection_key)
            {
                item.allowed_provider = Some(provider.into());
                item.allowed_action = Some(action.into());
            }
            let mut input = Map::new();
            if let Some(values) = data.get("input").and_then(Value::as_object) {
                for (key, value) in values {
                    if key.to_ascii_lowercase().contains("secret")
                        || key.to_ascii_lowercase().contains("token")
                    {
                        return Err(TemplateError::Sensitive(format!("{id}.input.{key}")));
                    }
                    let input_key = format!("{id}.input.{key}");
                    parameter(
                        parameters,
                        &input_key,
                        key,
                        ParameterKind::AppResource,
                        Some(&id),
                        &format!("input.{key}"),
                    );
                    if let Some(item) = parameters.iter_mut().find(|item| item.key == input_key) {
                        item.allowed_provider = Some(provider.into());
                        item.allowed_action = Some(action.into());
                    }
                    input.insert(key.clone(), Value::Null);
                    let _ = value;
                }
            }
            result.insert("connectionId".into(), Value::String(String::new()));
            result.insert("input".into(), Value::Object(input));
            result
        }
        "http" => {
            let mut result = keep(&data, &["kind", "label", "method", "body"]);
            if data
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(has_secret_url_query)
            {
                return Err(TemplateError::Sensitive(format!("{id}.url")));
            }
            if data
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|v| !v.trim().is_empty())
            {
                parameter(
                    parameters,
                    &format!("{id}.url"),
                    "HTTP destination",
                    ParameterKind::Text,
                    Some(&id),
                    "url",
                );
                result.insert("url".into(), Value::String(String::new()));
            }
            if let Some(headers) = data.get("headers").and_then(Value::as_str) {
                for line in headers.lines() {
                    let name = line
                        .split_once(':')
                        .map(|(n, _)| n.trim().to_ascii_lowercase())
                        .unwrap_or_default();
                    if [
                        "authorization",
                        "cookie",
                        "x-api-key",
                        "proxy-authorization",
                    ]
                    .contains(&name.as_str())
                    {
                        return Err(TemplateError::Invalid(format!(
                            "authenticated HTTP header at {id}.headers"
                        )));
                    }
                }
                if !headers.trim().is_empty() {
                    parameter(
                        parameters,
                        &format!("{id}.headers"),
                        "HTTP headers",
                        ParameterKind::Text,
                        Some(&id),
                        "headers",
                    );
                    result.insert("headers".into(), Value::String(String::new()));
                }
            }
            result
        }
        "notify" => {
            let mut result = keep(&data, &["kind", "label", "title", "body", "desktop"]);
            if data
                .get("webhookUrl")
                .and_then(Value::as_str)
                .is_some_and(has_secret_url_query)
            {
                return Err(TemplateError::Sensitive(format!("{id}.webhookUrl")));
            }
            if data
                .get("webhookUrl")
                .and_then(Value::as_str)
                .is_some_and(|v| !v.trim().is_empty())
            {
                parameter(
                    parameters,
                    &format!("{id}.webhookUrl"),
                    "Notification destination",
                    ParameterKind::Text,
                    Some(&id),
                    "webhookUrl",
                );
            }
            result.insert("webhookUrl".into(), Value::String(String::new()));
            result
        }
        "script" => {
            let mut result = keep(
                &data,
                &[
                    "kind",
                    "label",
                    "source",
                    "body",
                    "interpreter",
                    "appendOutput",
                ],
            );
            if data
                .get("interpreter")
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .is_some()
            {
                requirements.push(Requirement {
                    kind: RequirementKind::Interpreter,
                    name: data
                        .get("interpreter")
                        .and_then(Value::as_str)
                        .unwrap()
                        .into(),
                    provider: None,
                    capability: None,
                });
            }
            if data.get("source").and_then(Value::as_str) == Some("file") {
                parameter(
                    parameters,
                    &format!("{id}.path"),
                    "Script file",
                    ParameterKind::File,
                    Some(&id),
                    "path",
                );
                result.insert("path".into(), Value::String(String::new()));
            }
            result
        }
        "shell" => keep(&data, &["kind", "label", "command", "appendOutput"]),
        "customAgent" => keep(&data, &["kind", "label", "command", "promptMode"]),
        "identity" => keep(
            &data,
            &["kind", "label", "role", "personality", "instructions"],
        ),
        "template" => keep(&data, &["kind", "label", "template", "mode"]),
        "gitStatus" => keep(&data, &["kind", "label", "includeDiff"]),
        "gitHost" => keep(
            &data,
            &[
                "kind", "label", "action", "repo", "title", "body", "base", "head", "draft",
            ],
        ),
        "chooseOutput" | "output" => keep(
            &data,
            &[
                "label",
                "saveToMemory",
                "pinMemory",
                "memoryTitle",
                "asFinalResult",
                "htmlReport",
                "includeFilesChanged",
            ],
        ),
        _ => unreachable!(),
    };
    scan_text(&Value::Object(clean.clone()), &format!("node {id}"))?;
    if clean.len() > 64 {
        return Err(TemplateError::Invalid(format!(
            "unknown or excessive fields in node {id}"
        )));
    }
    Ok(TemplateNode {
        id,
        node_type,
        data: std::mem::take(&mut clean),
        position: Position { x, y },
    })
}

pub fn export_graph(
    title: &str,
    description: &str,
    graph: &Value,
) -> Result<TemplateEnvelope, TemplateError> {
    export_workflow(title, description, "", graph)
}

pub fn export_workflow(
    title: &str,
    description: &str,
    working_directory: &str,
    graph: &Value,
) -> Result<TemplateEnvelope, TemplateError> {
    export_workflow_with_starts(title, description, working_directory, graph, &[])
}

pub fn export_workflow_with_starts(
    title: &str,
    description: &str,
    working_directory: &str,
    graph: &Value,
    starts: &[StartRecipe],
) -> Result<TemplateEnvelope, TemplateError> {
    if title.trim().is_empty() || title.chars().count() > MAX_TITLE {
        return Err(TemplateError::Invalid(
            "title must be 1-120 characters".into(),
        ));
    }
    if description.chars().count() > MAX_DESCRIPTION {
        return Err(TemplateError::Invalid(
            "description exceeds 2,000 characters".into(),
        ));
    }
    let graph_obj = graph
        .as_object()
        .ok_or_else(|| TemplateError::Invalid("graph must be an object".into()))?;
    let nodes = graph_obj
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| TemplateError::Invalid("graph.nodes must be an array".into()))?;
    let edges = graph_obj
        .get("edges")
        .and_then(Value::as_array)
        .ok_or_else(|| TemplateError::Invalid("graph.edges must be an array".into()))?;
    if nodes.len() > MAX_NODES || edges.len() > MAX_EDGES {
        return Err(TemplateError::Invalid(
            "graph exceeds node or edge limit".into(),
        ));
    }
    let mut parameters = vec![];
    let mut requirements = vec![];
    if !working_directory.trim().is_empty() {
        parameter(
            &mut parameters,
            "workingDirectory",
            "Workflow folder",
            ParameterKind::Folder,
            None,
            "workingDirectory",
        );
    }
    let template_nodes = nodes
        .iter()
        .map(|node| sanitize_node(node, &mut parameters, &mut requirements))
        .collect::<Result<Vec<_>, _>>()?;
    let mut unique_requirements = Vec::with_capacity(requirements.len());
    for requirement in requirements.drain(..) {
        if !unique_requirements.contains(&requirement) {
            unique_requirements.push(requirement);
        }
    }
    requirements.clear();
    requirements.extend(unique_requirements);
    let ids: HashSet<_> = template_nodes.iter().map(|n| n.id.clone()).collect();
    if ids.len() != template_nodes.len() {
        return Err(TemplateError::Invalid("node IDs must be unique".into()));
    }
    let mut edge_ids = HashSet::new();
    let template_edges = edges
        .iter()
        .map(|edge| {
            let object = edge
                .as_object()
                .ok_or_else(|| TemplateError::Invalid("graph edge must be an object".into()))?;
            let source = object
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| TemplateError::Invalid("edge source is required".into()))?;
            let target = object
                .get("target")
                .and_then(Value::as_str)
                .ok_or_else(|| TemplateError::Invalid("edge target is required".into()))?;
            if !ids.contains(source) || !ids.contains(target) {
                return Err(TemplateError::Invalid(
                    "edge references a missing node".into(),
                ));
            }
            let edge_id = object
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_else(|| "edge");
            if !edge_ids.insert(edge_id.to_owned()) {
                return Err(TemplateError::Invalid("edge IDs must be unique".into()));
            }
            Ok(TemplateEdge {
                id: edge_id.into(),
                source: source.into(),
                target: target.into(),
                source_handle: object
                    .get("sourceHandle")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                target_handle: object
                    .get("targetHandle")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                data: TemplateEdgeData {
                    condition: object
                        .get("data")
                        .and_then(Value::as_object)
                        .and_then(|data| data.get("condition"))
                        .map(template_edge_condition)
                        .transpose()?,
                },
            })
        })
        .collect::<Result<Vec<_>, TemplateError>>()?;
    // Emit the lowest version that can carry this graph, so a workflow with no
    // conditions stays readable by builds that predate them.
    let schema_version = if template_edges.iter().any(|edge| edge.data.condition.is_some()) {
        CONDITIONAL_EDGE_SCHEMA_VERSION
    } else {
        MIN_SCHEMA_VERSION
    };
    let template = TemplateEnvelope {
        format: FORMAT.into(),
        schema_version,
        min_alfred_version: "1.0.0".into(),
        title: title.into(),
        description: description.into(),
        graph: TemplateGraph {
            nodes: template_nodes,
            edges: template_edges,
        },
        parameters,
        requirements,
        starts: starts.to_vec(),
    };
    validate_template(&template)?;
    if canonical_bytes(&template)?.len() > MAX_BYTES {
        return Err(TemplateError::TooLarge);
    }
    Ok(template)
}

pub fn validate_template(template: &TemplateEnvelope) -> Result<(), TemplateError> {
    validate_template_inner(template, false)
}

/// Read a condition off a workflow graph during export. Rejects anything the
/// runner would not understand so an invalid template is never published.
fn template_edge_condition(value: &Value) -> Result<TemplateEdgeCondition, TemplateError> {
    let object = value
        .as_object()
        .ok_or_else(|| TemplateError::Invalid("edge condition must be an object".into()))?;
    let condition = TemplateEdgeCondition {
        source: object
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        operator: object
            .get("operator")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        value: object
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        case_sensitive: object
            .get("caseSensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    validate_edge_condition(&condition)?;
    Ok(condition)
}

fn validate_edge_condition(condition: &TemplateEdgeCondition) -> Result<(), TemplateError> {
    if !EDGE_CONDITION_SOURCES.contains(&condition.source.as_str()) {
        return Err(TemplateError::Invalid(
            "edge condition source is not supported".into(),
        ));
    }
    if !EDGE_CONDITION_OPERATORS.contains(&condition.operator.as_str()) {
        return Err(TemplateError::Invalid(
            "edge condition operator is not supported".into(),
        ));
    }
    if condition.value.len() > MAX_EDGE_CONDITION_VALUE_BYTES {
        return Err(TemplateError::Invalid(
            "edge condition value is out of bounds".into(),
        ));
    }
    Ok(())
}

fn validate_template_inner(
    template: &TemplateEnvelope,
    allow_local_refs: bool,
) -> Result<(), TemplateError> {
    if template.format != FORMAT
        || template.schema_version < MIN_SCHEMA_VERSION
        || template.schema_version > SCHEMA_VERSION
    {
        return Err(TemplateError::Invalid(
            "unsupported template format or schema version".into(),
        ));
    }
    let version = parse_version(&template.min_alfred_version)
        .ok_or_else(|| TemplateError::Invalid("invalid minimum Alfred version".into()))?;
    if version > (1, 0, 0) {
        return Err(TemplateError::Invalid(
            "template requires a newer Alfred version".into(),
        ));
    }
    if template.title.trim().is_empty()
        || template.title.chars().count() > MAX_TITLE
        || template.description.chars().count() > MAX_DESCRIPTION
    {
        return Err(TemplateError::Invalid(
            "title or description is out of bounds".into(),
        ));
    }
    if template.graph.nodes.len() > MAX_NODES
        || template.graph.edges.len() > MAX_EDGES
        || template.parameters.len() > MAX_PARAMETERS
        || template.starts.len() > MAX_STARTS
    {
        return Err(TemplateError::Invalid(
            "template exceeds a size limit".into(),
        ));
    }
    if template
        .graph
        .nodes
        .iter()
        .any(|node| !valid_identifier(&node.id, 96))
    {
        return Err(TemplateError::Invalid(
            "node IDs must be bounded identifiers".into(),
        ));
    }
    let ids: HashSet<_> = template.graph.nodes.iter().map(|n| n.id.as_str()).collect();
    if ids.len() != template.graph.nodes.len() {
        return Err(TemplateError::Invalid("node IDs must be unique".into()));
    }
    for node in &template.graph.nodes {
        validate_node_data(node, allow_local_refs)?;
        if !node.position.x.is_finite() || !node.position.y.is_finite() {
            return Err(TemplateError::Invalid(format!(
                "node {} has non-finite position",
                node.id
            )));
        }
    }
    let mut edge_ids = HashSet::new();
    let mut incoming = HashMap::<&str, Vec<&str>>::new();
    for edge in &template.graph.edges {
        if !valid_identifier(&edge.id, 128) || !edge_ids.insert(edge.id.as_str()) {
            return Err(TemplateError::Invalid("edge IDs must be unique".into()));
        }
        if edge.source == edge.target {
            return Err(TemplateError::Invalid(
                "self edges are not supported".into(),
            ));
        }
        if !ids.contains(edge.source.as_str()) || !ids.contains(edge.target.as_str()) {
            return Err(TemplateError::Invalid(
                "edge references a missing node".into(),
            ));
        }
        for handle in [edge.source_handle.as_deref(), edge.target_handle.as_deref()]
            .into_iter()
            .flatten()
        {
            if handle.is_empty() || handle.len() > 96 {
                return Err(TemplateError::Invalid(
                    "edge handle is out of bounds".into(),
                ));
            }
        }
        if let Some(condition) = &edge.data.condition {
            if template.schema_version < CONDITIONAL_EDGE_SCHEMA_VERSION {
                return Err(TemplateError::Invalid(
                    "conditional connections require a newer template version".into(),
                ));
            }
            validate_edge_condition(condition)?;
        }
        incoming
            .entry(edge.target.as_str())
            .or_default()
            .push(edge.source.as_str());
    }
    fn visit<'a>(
        id: &'a str,
        incoming: &HashMap<&'a str, Vec<&'a str>>,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> bool {
        if visiting.contains(id) {
            return false;
        }
        if visited.contains(id) {
            return true;
        }
        visiting.insert(id);
        if incoming.get(id).is_some_and(|parents| {
            parents
                .iter()
                .any(|parent| !visit(parent, incoming, visiting, visited))
        }) {
            return false;
        }
        visiting.remove(id);
        visited.insert(id);
        true
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in &ids {
        if !visit(id, &incoming, &mut visiting, &mut visited) {
            return Err(TemplateError::Invalid("graph must be acyclic".into()));
        }
    }
    let mut parameter_keys = HashSet::new();
    for parameter in &template.parameters {
        if !parameter_keys.insert(parameter.key.as_str())
            || parameter.key.is_empty()
            || !valid_parameter_key(&parameter.key)
            || parameter.key == "__proto__"
        {
            return Err(TemplateError::Invalid(format!(
                "invalid parameter {}",
                parameter.key
            )));
        }
        if parameter.description.chars().count() > 500
            || parameter
                .allowed_provider
                .as_ref()
                .is_some_and(|value| value.chars().count() > 96)
            || parameter
                .allowed_action
                .as_ref()
                .is_some_and(|value| value.chars().count() > 96)
            || parameter.targets.len() > 16
        {
            return Err(TemplateError::Invalid(format!(
                "invalid parameter {} metadata",
                parameter.key
            )));
        }
        for target in std::iter::once(&parameter.target).chain(parameter.targets.iter()) {
            if target.field.is_empty() || target.field.len() > 128 || target.field == "__proto__" {
                return Err(TemplateError::Invalid(format!(
                    "invalid parameter target {}",
                    parameter.key
                )));
            }
            match target.kind {
                TargetKind::Node => {
                    let id = target
                        .node_id
                        .as_deref()
                        .filter(|id| ids.contains(id))
                        .ok_or_else(|| {
                            TemplateError::Invalid(format!(
                                "parameter {} references a missing node",
                                parameter.key
                            ))
                        })?;
                    let node = template
                        .graph
                        .nodes
                        .iter()
                        .find(|node| node.id == id)
                        .unwrap();
                    let field = target.field.split('.').next().unwrap_or_default();
                    let agent_account_binding =
                        node.node_type == "agent" && target.field == "accountRef";
                    if !allowed_fields(&node.node_type).contains(&field) && !agent_account_binding {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} targets unsupported field",
                            parameter.key
                        )));
                    }
                    if target.field.contains('.')
                        && !((node.node_type == "appAction"
                            && target.field.starts_with("input.")
                            && target.field.len() > 6)
                            || ((node.node_type == "input" || node.node_type == "prompt")
                                && target.field == "script.path"))
                    {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} targets unsupported nested field",
                            parameter.key
                        )));
                    }
                    validate_parameter_kind_for_node(parameter, target, node)?;
                }
                TargetKind::Start => {
                    if target.node_id.is_some() {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} has a node on a start target",
                            parameter.key
                        )));
                    }
                    let mut parts = target.field.splitn(3, '.');
                    let index = parts.next().and_then(|n| n.parse::<usize>().ok());
                    let field = parts.next();
                    let nested = parts.next();
                    if index.is_none()
                        || field.is_none_or(|field| field.is_empty())
                        || nested.is_some_and(|field| field.is_empty())
                        || index.unwrap() >= template.starts.len()
                    {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} has an invalid start target",
                            parameter.key
                        )));
                    }
                    if nested.is_some() && field != Some("filters") {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} has an invalid nested start target",
                            parameter.key
                        )));
                    }
                    let start = &template.starts[index.unwrap()];
                    let allowed = match start.kind {
                        StartKind::Schedule => &["cron", "label", "timezone"][..],
                        StartKind::File => &["path", "label", "pattern"][..],
                        StartKind::AppEvent => &[
                            "event",
                            "eventType",
                            "providerId",
                            "connectionId",
                            "label",
                            "filters",
                        ][..],
                        StartKind::Webhook => &["label"][..],
                    };
                    if !allowed.contains(&field.unwrap())
                        || (nested.is_some()
                            && (start.kind != StartKind::AppEvent
                                || !start.config.get("filters").is_some_and(Value::is_object)))
                    {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} targets an unsupported start field",
                            parameter.key
                        )));
                    }
                    validate_parameter_kind_for_start(parameter, target, start)?;
                }
                TargetKind::WorkingDirectory => {
                    if target.node_id.is_some() || target.field != "workingDirectory" {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} has an invalid working directory target",
                            parameter.key
                        )));
                    }
                    if parameter.kind != ParameterKind::Folder {
                        return Err(TemplateError::Invalid(format!(
                            "parameter {} has an incompatible working directory kind",
                            parameter.key
                        )));
                    }
                }
            }
        }
    }
    let mut requirement_keys = HashSet::new();
    for requirement in &template.requirements {
        if requirement.name.trim().is_empty()
            || requirement.name.len() > 120
            || requirement
                .provider
                .as_ref()
                .is_some_and(|value| value.len() > 96)
            || requirement
                .capability
                .as_ref()
                .is_some_and(|value| value.len() > 120)
        {
            return Err(TemplateError::Invalid(
                "requirement name is out of bounds".into(),
            ));
        }
        if !requirement_keys.insert((
            requirement.kind.kind_string(),
            requirement.name.as_str(),
            requirement.provider.as_deref(),
            requirement.capability.as_deref(),
        )) {
            return Err(TemplateError::Invalid("duplicate requirement".into()));
        }
    }
    for (index, start) in template.starts.iter().enumerate() {
        validate_start(start, index, template)?;
    }
    let value = serde_json::to_value(template)?;
    if json_depth(&value, 0) > MAX_DEPTH {
        return Err(TemplateError::Invalid("JSON depth exceeds 16".into()));
    }
    if serde_json::to_vec(template)?.len() > MAX_BYTES {
        return Err(TemplateError::TooLarge);
    }
    Ok(())
}

fn incompatible_kind(
    parameter: &Parameter,
    target: &ParameterTarget,
    expected: &str,
) -> TemplateError {
    TemplateError::Invalid(format!(
        "parameter {} has an incompatible kind for {} target {}",
        parameter.key, expected, target.field
    ))
}

fn validate_parameter_kind_for_node(
    parameter: &Parameter,
    target: &ParameterTarget,
    node: &TemplateNode,
) -> Result<(), TemplateError> {
    let expected = if node.node_type == "agent" && target.field == "accountRef" {
        Some(("agent account", ParameterKind::AgentAccount))
    } else if node.node_type == "memory" && target.field == "memoryIds" {
        Some(("memory", ParameterKind::Memory))
    } else if node.node_type == "appAction" && target.field == "connectionId" {
        Some(("app connection", ParameterKind::AppConnection))
    } else if node.node_type == "appAction" && target.field.starts_with("input.") {
        if parameter.allowed_action.is_some() && parameter.kind != ParameterKind::AppResource {
            return Err(incompatible_kind(
                parameter,
                target,
                "descriptor-backed app resource",
            ));
        }
        None
    } else if (node.node_type == "fileInject" && target.field == "paths")
        || ((node.node_type == "input" || node.node_type == "prompt")
            && target.field == "attachments")
    {
        Some(("folder", ParameterKind::Folder))
    } else if (node.node_type == "writeFile" && target.field == "path")
        || (node.node_type == "script" && target.field == "path")
        || ((node.node_type == "input" || node.node_type == "prompt")
            && target.field == "script.path")
    {
        Some(("file", ParameterKind::File))
    } else {
        None
    };
    if let Some((label, kind)) = expected {
        if parameter.kind != kind {
            return Err(incompatible_kind(parameter, target, label));
        }
    } else if (target.field == "connectionId" && node.node_type != "appAction")
        || (target.field == "accountRef" && node.node_type != "agent")
        || (target.field == "memoryIds" && node.node_type != "memory")
        || (target.field == "paths"
            && node.node_type != "fileInject"
            && node.node_type != "input"
            && node.node_type != "prompt")
    {
        return Err(incompatible_kind(parameter, target, "execution binding"));
    }
    Ok(())
}

fn validate_parameter_kind_for_start(
    parameter: &Parameter,
    target: &ParameterTarget,
    start: &StartRecipe,
) -> Result<(), TemplateError> {
    let field = target.field.splitn(3, '.').nth(1).unwrap_or_default();
    let expected = match (start.kind, field) {
        (StartKind::AppEvent, "connectionId") => {
            Some(("app connection", ParameterKind::AppConnection))
        }
        (StartKind::AppEvent, "filters") => {
            Some(("descriptor-backed app resource", ParameterKind::AppResource))
        }
        (StartKind::File, "path") => Some(("folder", ParameterKind::Folder)),
        _ => None,
    };
    if let Some((label, kind)) = expected {
        if parameter.kind != kind {
            return Err(incompatible_kind(parameter, target, label));
        }
    } else if !matches!(
        parameter.kind,
        ParameterKind::Text | ParameterKind::Email | ParameterKind::Channel
    ) {
        return Err(incompatible_kind(parameter, target, "text"));
    }
    Ok(())
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next().unwrap_or("0").parse().ok()?,
    );
    if parts.next().is_some() {
        return None;
    }
    Some(parsed)
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn valid_parameter_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(byte) if byte.is_ascii_lowercase())
        && value.len() <= 64
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn is_public_event_filter_field(field: &str) -> bool {
    let field = field.to_ascii_lowercase();
    ![
        "id",
        "ids",
        "ref",
        "resource",
        "connection",
        "channel",
        "conversation",
        "repository",
        "project",
        "workspace",
        "account",
        "folder",
        "file",
    ]
    .iter()
    .any(|marker| {
        field == *marker
            || field.ends_with(marker)
            || (!matches!(*marker, "id" | "ids" | "ref") && field.contains(marker))
    })
}

fn validate_start(
    start: &StartRecipe,
    index: usize,
    template: &TemplateEnvelope,
) -> Result<(), TemplateError> {
    let allowed: &[&str] = match start.kind {
        StartKind::Schedule => &["cron", "label", "timezone"],
        StartKind::File => &["path", "label", "pattern"],
        StartKind::AppEvent => &[
            "event",
            "eventType",
            "providerId",
            "connectionId",
            "label",
            "filters",
        ],
        StartKind::Webhook => &["label"],
    };
    for key in start.config.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(TemplateError::Invalid(format!(
                "unsupported start field {key}"
            )));
        }
    }
    for (key, value) in &start.config {
        if key == "filters" {
            if !value.is_object() {
                return Err(TemplateError::Invalid(format!(
                    "start {key} filters must be an object"
                )));
            }
        } else if !value.is_string() {
            return Err(TemplateError::Invalid(format!(
                "start {key} value must be a string"
            )));
        }
    }
    if start.kind == StartKind::AppEvent {
        let has_binding = |field: &str, kind: ParameterKind| {
            template.parameters.iter().any(|parameter| {
                parameter.kind == kind
                    && std::iter::once(&parameter.target)
                        .chain(parameter.targets.iter())
                        .any(|target| {
                            target.kind == TargetKind::Start
                                && target.field == format!("{index}.{field}")
                        })
            })
        };
        if start
            .config
            .get("connectionId")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
            && !has_binding("connectionId", ParameterKind::AppConnection)
        {
            return Err(TemplateError::Invalid(format!(
                "app event start {index} retains a local connection"
            )));
        }
        if let Some(filters) = start.config.get("filters").and_then(Value::as_object) {
            for (field, value) in filters {
                if !value.is_null()
                    && value.as_str().is_some_and(|text| !text.trim().is_empty())
                    && !has_binding(&format!("filters.{field}"), ParameterKind::AppResource)
                    && !is_public_event_filter_field(field)
                {
                    return Err(TemplateError::Invalid(format!(
                        "app event start {index} retains local resource {field}"
                    )));
                }
            }
        }
    }
    let required = match start.kind {
        StartKind::Schedule => "cron",
        StartKind::File => "path",
        StartKind::AppEvent => "event",
        StartKind::Webhook => "label",
    };
    let event_ok = start.kind == StartKind::AppEvent
        && ["event", "eventType"].iter().any(|field| {
            start
                .config
                .get(*field)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        });
    let required_parameterized = template.parameters.iter().any(|parameter| {
        std::iter::once(&parameter.target)
            .chain(parameter.targets.iter())
            .any(|target| {
                target.kind == TargetKind::Start && target.field == format!("{index}.{required}")
            })
    });
    if !event_ok
        && !required_parameterized
        && start
            .config
            .get(required)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(TemplateError::Invalid(format!(
            "start {required} is required"
        )));
    }
    Ok(())
}

fn json_depth(value: &Value, depth: usize) -> usize {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|v| json_depth(v, depth + 1))
            .max()
            .unwrap_or(depth),
        Value::Object(map) => map
            .values()
            .map(|v| json_depth(v, depth + 1))
            .max()
            .unwrap_or(depth),
        _ => depth,
    }
}

fn allowed_fields(node_type: &str) -> &'static [&'static str] {
    match node_type {
        "input" | "prompt" => &["label", "prompt", "blocked", "script", "attachments"],
        "agent" => &[
            "label",
            "provider",
            "harness",
            "model",
            "skillNames",
            "skillName",
            "accountRef",
        ],
        "memory" => &["label", "memoryIds"],
        "identity" => &["kind", "label", "role", "personality", "instructions"],
        "template" => &["kind", "label", "template", "mode"],
        "fileInject" => &["kind", "label", "paths"],
        "gitStatus" => &["kind", "label", "includeDiff"],
        "shell" => &["kind", "label", "command", "appendOutput"],
        "script" => &[
            "kind",
            "label",
            "source",
            "path",
            "body",
            "interpreter",
            "appendOutput",
        ],
        "http" => &["kind", "label", "method", "url", "body", "headers"],
        "notify" => &["kind", "label", "title", "body", "desktop", "webhookUrl"],
        "writeFile" => &["kind", "label", "path", "content"],
        "gitHost" => &[
            "kind", "label", "action", "repo", "title", "body", "base", "head", "draft",
        ],
        "customAgent" => &["kind", "label", "command", "promptMode"],
        "chooseOutput" | "output" => &[
            "label",
            "saveToMemory",
            "pinMemory",
            "memoryTitle",
            "asFinalResult",
            "htmlReport",
            "includeFilesChanged",
        ],
        "appAction" => &[
            "type",
            "label",
            "providerId",
            "actionId",
            "connectionId",
            "input",
        ],
        _ => &[],
    }
}

fn validate_node_data(node: &TemplateNode, allow_local_refs: bool) -> Result<(), TemplateError> {
    if !NODE_TYPES.contains(&node.node_type.as_str()) {
        return Err(TemplateError::Invalid(format!(
            "unsupported node type {}",
            node.node_type
        )));
    }
    for key in node.data.keys() {
        if !allowed_fields(&node.node_type).contains(&key.as_str()) {
            return Err(TemplateError::Invalid(format!(
                "unsupported execution field {}.{}",
                node.id, key
            )));
        }
    }
    for key in [
        "label",
        "prompt",
        "provider",
        "harness",
        "model",
        "skillName",
        "kind",
        "role",
        "personality",
        "instructions",
        "template",
        "mode",
        "command",
        "source",
        "path",
        "body",
        "interpreter",
        "message",
        "method",
        "url",
        "headers",
        "title",
        "webhookUrl",
        "content",
        "action",
        "repo",
        "base",
        "head",
        "type",
        "providerId",
        "actionId",
        "connectionId",
        "memoryTitle",
    ] {
        if node.data.get(key).is_some_and(|value| !value.is_string()) {
            return Err(TemplateError::Invalid(format!(
                "node {}.{} must be a string",
                node.id, key
            )));
        }
    }
    if node.node_type == "agent"
        && !allow_local_refs
        && node
            .data
            .get("accountRef")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(TemplateError::Invalid(format!(
            "agent {} retains a local account",
            node.id
        )));
    }
    for key in ["skillNames", "memoryIds", "paths"] {
        if node.data.get(key).is_some_and(|value| {
            value
                .as_array()
                .is_none_or(|items| items.iter().any(|item| !item.is_string()))
        }) {
            return Err(TemplateError::Invalid(format!(
                "node {}.{} must be a string array",
                node.id, key
            )));
        }
    }
    for key in [
        "blocked",
        "appendOutput",
        "includeDiff",
        "desktop",
        "draft",
        "saveToMemory",
        "pinMemory",
        "asFinalResult",
        "htmlReport",
        "includeFilesChanged",
        "run",
    ] {
        if node.data.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(TemplateError::Invalid(format!(
                "node {}.{} must be boolean",
                node.id, key
            )));
        }
    }
    if let Some(script) = node.data.get("script") {
        let object = script.as_object().ok_or_else(|| {
            TemplateError::Invalid(format!(
                "node {}.script must be a structured script",
                node.id
            ))
        })?;
        if object.keys().any(|key| {
            !["source", "path", "body", "interpreter", "message", "run"].contains(&key.as_str())
        }) {
            return Err(TemplateError::Invalid(format!(
                "node {}.script contains an unsupported field",
                node.id
            )));
        }
        if !matches!(
            object.get("source").and_then(Value::as_str),
            Some("file") | Some("inline")
        ) {
            return Err(TemplateError::Invalid(format!(
                "node {}.script.source is invalid",
                node.id
            )));
        }
        for key in ["path", "body", "interpreter", "message"] {
            if object.get(key).is_some_and(|value| !value.is_string()) {
                return Err(TemplateError::Invalid(format!(
                    "node {}.script.{} must be a string",
                    node.id, key
                )));
            }
        }
        if object.get("run").is_some_and(|value| !value.is_boolean()) {
            return Err(TemplateError::Invalid(format!(
                "node {}.script.run must be boolean",
                node.id
            )));
        }
    }
    if node.node_type == "appAction"
        && node
            .data
            .get("input")
            .is_some_and(|value| value.as_object().is_none())
    {
        return Err(TemplateError::Invalid(format!(
            "node {}.input must be an object",
            node.id
        )));
    }
    if node.node_type == "input" || node.node_type == "prompt" {
        if let Some(attachments) = node.data.get("attachments") {
            let Some(items) = attachments.as_array() else {
                return Err(TemplateError::Invalid(format!(
                    "node {}.attachments must be an array",
                    node.id
                )));
            };
            for attachment in items {
                let object = attachment.as_object().ok_or_else(|| {
                    TemplateError::Invalid(format!(
                        "node {}.attachments contains an invalid item",
                        node.id
                    ))
                })?;
                if object
                    .keys()
                    .any(|key| !["id", "path", "kind"].contains(&key.as_str()))
                    || object
                        .get("id")
                        .and_then(Value::as_str)
                        .is_none_or(|value| value.is_empty())
                    || object
                        .get("path")
                        .and_then(Value::as_str)
                        .is_none_or(|value| value.is_empty())
                    || !matches!(
                        object.get("kind").and_then(Value::as_str),
                        Some("file") | Some("folder")
                    )
                {
                    return Err(TemplateError::Invalid(format!(
                        "node {}.attachments contains an invalid item",
                        node.id
                    )));
                }
            }
        }
    }
    if !allow_local_refs {
        if node.node_type == "appAction"
            && node
                .data
                .get("connectionId")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        {
            return Err(TemplateError::Invalid(format!(
                "app action {} retains a local connection",
                node.id
            )));
        }
        if node.node_type == "memory"
            && node
                .data
                .get("memoryIds")
                .is_some_and(|value| value.as_array().is_some_and(|items| !items.is_empty()))
        {
            return Err(TemplateError::Invalid(format!(
                "node {} retains a local reference",
                node.id
            )));
        }
        if node.node_type == "fileInject"
            && node
                .data
                .get("paths")
                .is_some_and(|value| value.as_array().is_some_and(|items| !items.is_empty()))
        {
            return Err(TemplateError::Invalid(format!(
                "node {} retains a local reference",
                node.id
            )));
        }
    }
    Ok(())
}

pub fn canonical_bytes(template: &TemplateEnvelope) -> Result<Vec<u8>, TemplateError> {
    validate_template(template)?;
    let mut bytes = Vec::new();
    write_canonical_json(&serde_json::to_value(template)?, &mut bytes)?;
    Ok(bytes)
}

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> Result<(), TemplateError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(value) => output.extend_from_slice(if *value { b"true" } else { b"false" }),
        Value::String(value) => output.extend_from_slice(serde_json::to_string(value)?.as_bytes()),
        Value::Number(value) => output.extend_from_slice(canonical_number(value)?.as_bytes()),
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                output.extend_from_slice(serde_json::to_string(key)?.as_bytes());
                output.push(b':');
                write_canonical_json(&values[key], output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// Match JSON.stringify's fixed/exponent threshold while using the same
/// shortest IEEE-754 representation as serde_json's ryu formatter.
fn canonical_number(value: &serde_json::Number) -> Result<String, TemplateError> {
    let float = value
        .as_f64()
        .ok_or_else(|| TemplateError::Invalid("number is not finite".into()))?;
    if float == 0.0 {
        return Ok("0".into());
    }
    let raw = serde_json::Number::from_f64(float)
        .ok_or_else(|| TemplateError::Invalid("number is not finite".into()))?
        .to_string()
        .replace('E', "e");
    let (negative, raw) = raw
        .strip_prefix('-')
        .map_or((false, raw.as_str()), |value| (true, value));
    let (coefficient, exponent) = raw
        .split_once('e')
        .map_or((raw, 0_i32), |(coefficient, exponent)| {
            (coefficient, exponent.parse::<i32>().unwrap_or(0))
        });
    let mut digits = coefficient.replace('.', "");
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let decimal_exponent = exponent + coefficient.find('.').unwrap_or(coefficient.len()) as i32 - 1;
    let mut result = if (-6..21).contains(&decimal_exponent) {
        let point = decimal_exponent + 1;
        if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}", digits, "0".repeat(point as usize - digits.len()))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        }
    } else {
        let exponent_text = if decimal_exponent >= 0 {
            format!("+{}", decimal_exponent)
        } else {
            decimal_exponent.to_string()
        };
        if digits.len() == 1 {
            format!("{}e{}", digits, exponent_text)
        } else {
            format!("{}.{}e{}", &digits[..1], &digits[1..], exponent_text)
        }
    };
    if negative {
        result.insert(0, '-');
    }
    Ok(result)
}
pub fn digest(template: &TemplateEnvelope) -> Result<String, TemplateError> {
    Ok(format!("{:x}", Sha256::digest(canonical_bytes(template)?)))
}

/// Resolve complete supported fields and reject missing required values. This
/// performs the same narrow binding used by the import transaction; it never
/// performs string replacement in scripts, prompts, or arbitrary JSON paths.
pub fn apply_parameter_values(
    template: &TemplateEnvelope,
    values: &BTreeMap<String, Value>,
) -> Result<TemplateEnvelope, TemplateError> {
    validate_template(template)?;
    let mut result = template.clone();
    for parameter in &template.parameters {
        let Some(value) = values.get(&parameter.key) else {
            if parameter.required {
                return Err(TemplateError::Invalid(format!(
                    "missing setup value {}",
                    parameter.key
                )));
            } else {
                continue;
            }
        };
        if parameter.required && value_is_missing(value) {
            return Err(TemplateError::Invalid(format!(
                "missing setup value {}",
                parameter.key
            )));
        }
        validate_binding_value(parameter, value)?;
        for target in std::iter::once(&parameter.target).chain(parameter.targets.iter()) {
            match target.kind {
                TargetKind::Node => {
                    let node_id = target.node_id.as_deref().unwrap();
                    let node = result
                        .graph
                        .nodes
                        .iter_mut()
                        .find(|n| n.id == node_id)
                        .unwrap();
                    let field = target.field.rsplit('.').next().unwrap_or(&target.field);
                    let adjusted = if field == "attachments"
                        && matches!(node.node_type.as_str(), "input" | "prompt")
                    {
                        let paths = match value {
                            Value::String(path) => vec![path.clone()],
                            Value::Array(items) => items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect(),
                            _ => vec![],
                        };
                        Value::Array(paths.into_iter().map(|path| serde_json::json!({"id": Uuid::new_v4().to_string(), "path": path, "kind": "folder"})).collect())
                    } else if matches!(field, "paths" | "memoryIds") && value.is_string() {
                        Value::Array(vec![value.clone()])
                    } else {
                        value.clone()
                    };
                    set_node_field(&mut node.data, &target.field, adjusted)?;
                }
                TargetKind::Start => {
                    let mut parts = target.field.splitn(3, '.');
                    let index = parts.next().and_then(|n| n.parse::<usize>().ok()).unwrap();
                    let field = parts.next().unwrap();
                    if let Some(nested) = parts.next() {
                        if field != "filters" || nested.is_empty() {
                            return Err(TemplateError::Invalid(format!(
                                "parameter {} has an invalid nested start target",
                                parameter.key
                            )));
                        }
                        let filters = result.starts[index]
                            .config
                            .entry(field.to_string())
                            .or_insert_with(|| Value::Object(Map::new()))
                            .as_object_mut()
                            .ok_or_else(|| {
                                TemplateError::Invalid(format!(
                                    "parameter {} start filters are not an object",
                                    parameter.key
                                ))
                            })?;
                        filters.insert(nested.into(), value.clone());
                    } else {
                        result.starts[index]
                            .config
                            .insert(field.into(), value.clone());
                    }
                }
                TargetKind::WorkingDirectory => {}
            }
        }
    }
    validate_template_inner(&result, true)?;
    Ok(result)
}

fn validate_binding_value(parameter: &Parameter, value: &Value) -> Result<(), TemplateError> {
    let valid = match parameter.kind {
        ParameterKind::Text
        | ParameterKind::Folder
        | ParameterKind::File
        | ParameterKind::AgentAccount
        | ParameterKind::AppConnection
        | ParameterKind::AppResource
        | ParameterKind::Channel
        | ParameterKind::Email
        | ParameterKind::SecretBacked => value.is_string(),
        ParameterKind::Memory => {
            value.is_array() && value.as_array().unwrap().iter().all(Value::is_string)
        }
    };
    if !valid {
        return Err(TemplateError::Invalid(format!(
            "setup value {} has the wrong type",
            parameter.key
        )));
    }
    Ok(())
}

fn value_is_missing(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn set_node_field(
    data: &mut Map<String, Value>,
    field: &str,
    value: Value,
) -> Result<(), TemplateError> {
    if let Some((parent, child)) = field.split_once('.') {
        if !matches!(parent, "input" | "script") || child.is_empty() {
            return Err(TemplateError::Invalid(format!(
                "unsupported parameter field {field}"
            )));
        }
        let object = data
            .get_mut(parent)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                TemplateError::Invalid(format!("parameter parent {parent} is missing"))
            })?;
        object.insert(child.into(), value);
    } else {
        data.insert(field.into(), value);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportGraph {
    pub graph: TemplateGraph,
    pub memory_retrieval_enabled: bool,
}

pub fn remap_graph_ids(graph: &TemplateGraph) -> ImportGraph {
    let ids: HashMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), Uuid::new_v4().to_string()))
        .collect();
    let nodes = graph
        .nodes
        .iter()
        .map(|node| TemplateNode {
            id: ids[&node.id].clone(),
            node_type: node.node_type.clone(),
            data: node.data.clone(),
            position: node.position.clone(),
        })
        .collect();
    let edges = graph
        .edges
        .iter()
        .map(|edge| TemplateEdge {
            id: Uuid::new_v4().to_string(),
            source: ids[&edge.source].clone(),
            target: ids[&edge.target].clone(),
            source_handle: edge.source_handle.clone(),
            target_handle: edge.target_handle.clone(),
            data: edge.data.clone(),
        })
        .collect();
    ImportGraph {
        graph: TemplateGraph { nodes, edges },
        memory_retrieval_enabled: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/valid-note-summary.json"
        ))
        .unwrap()
    }
    #[test]
    fn exports_and_hashes_fixture() {
        let value = fixture();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        validate_template(&template).unwrap();
        assert_eq!(
            digest(&template).unwrap(),
            "d9db7d11405c97a44909e5031ff785cd5c0ec7129652d5e1734373f4484de462"
        );
    }
    #[test]
    fn canonical_unicode_and_numeric_defaults_match_shared_fixture() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/unicode-defaults.json"
        ))
        .unwrap();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        assert_eq!(
            digest(&template).unwrap(),
            "8e3404540176b29ce866f5d74ef0b69dad24217b60813c23fe9e34e65abc371b"
        );
    }
    #[test]
    fn accepts_structured_input_script_and_nested_file_binding() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/input-script.json"
        ))
        .unwrap();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        validate_template(&template).unwrap();
        let mut values = BTreeMap::new();
        values.insert(
            "input.script.path".into(),
            Value::String("/tmp/example.sh".into()),
        );
        let resolved = apply_parameter_values(&template, &values).unwrap();
        assert_eq!(
            resolved.graph.nodes[0].data["script"]["path"],
            Value::String("/tmp/example.sh".into())
        );
    }
    #[test]
    fn accepts_grouped_agent_account_bindings_and_rejects_local_values() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/agent-account-binding.json"
        ))
        .unwrap();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        validate_template(&template).unwrap();
        let graph = serde_json::json!({
            "nodes": [
                {"id":"agent-a","type":"agent","data":{"label":"First","provider":"claude_code","harness":"cli","model":"sonnet","skillNames":[],"accountRef":"local"},"position":{"x":0,"y":0}},
                {"id":"agent-b","type":"agent","data":{"label":"Second","provider":"claude_code","harness":"cli","model":"sonnet","skillNames":[]},"position":{"x":220,"y":0}}
            ], "edges": []
        });
        assert!(export_graph("Local", "", &graph).is_ok());
        assert!(validate_template(&serde_json::from_value::<TemplateEnvelope>(serde_json::json!({
            "format":"alfred.workflow-template","schemaVersion":1,"minAlfredVersion":"1.0.0","title":"Local","description":"","graph":graph,"parameters":[],"requirements":[],"starts":[]
        })).unwrap()).is_err());
    }
    #[test]
    fn notes_to_slack_fixture_resolves_two_isolated_recipient_profiles() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/notes-to-slack.json"
        ))
        .unwrap();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        validate_template(&template).unwrap();
        for (suffix, folder, account, connection, channel) in [
            ("a", "/profiles/a/notes", "agent-a", "slack-a", "C123"),
            ("b", "/profiles/b/notes", "agent-b", "slack-b", "C456"),
        ] {
            let mut values = BTreeMap::new();
            values.insert("notes.attachments".into(), Value::String(folder.into()));
            values.insert("agent.account".into(), Value::String(account.into()));
            values.insert("slack.connection".into(), Value::String(connection.into()));
            values.insert("slack.conversation".into(), Value::String(channel.into()));
            let resolved = apply_parameter_values(&template, &values).unwrap();
            assert_eq!(
                resolved.graph.nodes[0].data["attachments"][0]["path"],
                Value::String(folder.into())
            );
            assert_eq!(
                resolved.graph.nodes[2].data["connectionId"],
                Value::String(connection.into())
            );
            assert_eq!(
                resolved.graph.nodes[2].data["input"]["conversation"],
                Value::String(channel.into())
            );
            assert_eq!(
                resolved.graph.nodes[1].data["accountRef"],
                Value::String(account.into())
            );
            assert!(!suffix.is_empty());
        }
    }
    #[test]
    fn accepts_parameterized_start_fixture() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/parameterized-start.json"
        ))
        .unwrap();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        validate_template(&template).unwrap();
    }
    #[test]
    fn canonical_numeric_and_unicode_boundaries_match_shared_fixture() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/canonical-boundaries.json"
        ))
        .unwrap();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        assert_eq!(
            digest(&template).unwrap(),
            "66486eb47b6594137092f3dd9f97b4d3884bea740721ef1c76fad35e49fd8966"
        );
    }
    #[test]
    fn rejects_cycles() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/invalid-cycle.json"
        ))
        .unwrap();
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        assert!(validate_template(&template).is_err());
    }
    #[test]
    fn rejects_shared_malformed_fixtures() {
        for raw in [
            include_str!("../../../contracts/workflow-template/fixtures/invalid-cycle.json"),
            include_str!(
                "../../../contracts/workflow-template/fixtures/invalid-unknown-field.json"
            ),
            include_str!(
                "../../../contracts/workflow-template/fixtures/invalid-duplicate-edge.json"
            ),
            include_str!(
                "../../../contracts/workflow-template/fixtures/invalid-duplicate-requirement.json"
            ),
            include_str!(
                "../../../contracts/workflow-template/fixtures/invalid-parameter-reference.json"
            ),
            include_str!("../../../contracts/workflow-template/fixtures/invalid-start.json"),
            include_str!("../../../contracts/workflow-template/fixtures/invalid-edge-handle.json"),
            include_str!("../../../contracts/workflow-template/fixtures/invalid-min-version.json"),
            include_str!(
                "../../../contracts/workflow-template/fixtures/invalid-numeric-requirement-name.json"
            ),
            include_str!(
                "../../../contracts/workflow-template/fixtures/invalid-edge-condition.json"
            ),
        ] {
            let value: Value = serde_json::from_str(raw).unwrap();
            let result = serde_json::from_value::<TemplateEnvelope>(value).and_then(|template| {
                validate_template(&template).map_err(|error| {
                    serde_json::Error::io(std::io::Error::other(error.to_string()))
                })
            });
            assert!(result.is_err());
        }
    }
    // ---- Conditional connections (Plan 053) ----

    fn conditional_fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../contracts/workflow-template/fixtures/conditional-edge.json"
        ))
        .unwrap()
    }

    #[test]
    fn a_conditional_template_validates_and_keeps_its_conditions() {
        let template: TemplateEnvelope = serde_json::from_value(conditional_fixture()).unwrap();
        validate_template(&template).unwrap();
        let triage = template
            .graph
            .edges
            .iter()
            .find(|edge| edge.id == "agent-triage")
            .expect("conditional edge");
        let condition = triage.data.condition.as_ref().expect("condition");
        assert_eq!(condition.source, "output");
        assert_eq!(condition.operator, "matches");
        assert_eq!(condition.value, "FAIL|error");
    }

    #[test]
    fn conditions_survive_export_and_import() {
        let graph = serde_json::json!({
            "nodes": [
                {"id":"a","type":"agent","data":{"label":"a","provider":"codex"},"position":{"x":0,"y":0}},
                {"id":"b","type":"agent","data":{"label":"b","provider":"codex"},"position":{"x":200,"y":0}}
            ],
            "edges": [{
                "id":"ab","source":"a","target":"b",
                "data":{"condition":{"source":"output","operator":"contains","value":"FAIL","caseSensitive":true}}
            }]
        });
        let exported = export_graph("x", "", &graph).unwrap();
        let condition = exported.graph.edges[0]
            .data
            .condition
            .as_ref()
            .expect("condition survived export");
        assert_eq!(condition.value, "FAIL");
        assert!(condition.case_sensitive);

        // Node ids are remapped on import; the condition must ride along.
        let imported = remap_graph_ids(&exported.graph);
        assert_eq!(
            imported.graph.edges[0].data.condition.as_ref(),
            Some(condition)
        );
    }

    #[test]
    fn a_conditional_template_declares_the_newer_schema_version() {
        let plain = serde_json::json!({
            "nodes": [
                {"id":"a","type":"agent","data":{"label":"a","provider":"codex"},"position":{"x":0,"y":0}},
                {"id":"b","type":"agent","data":{"label":"b","provider":"codex"},"position":{"x":200,"y":0}}
            ],
            "edges": [{"id":"ab","source":"a","target":"b"}]
        });
        // No conditions: still the original version, so older builds keep reading it.
        assert_eq!(
            export_graph("x", "", &plain).unwrap().schema_version,
            MIN_SCHEMA_VERSION
        );

        let mut conditional = plain.clone();
        conditional["edges"][0]["data"] = serde_json::json!({
            "condition": {"source": "output", "operator": "isNotEmpty"}
        });
        assert_eq!(
            export_graph("x", "", &conditional).unwrap().schema_version,
            CONDITIONAL_EDGE_SCHEMA_VERSION
        );
    }

    #[test]
    fn a_condition_smuggled_into_an_older_version_is_rejected() {
        let mut value = conditional_fixture();
        value["schemaVersion"] = serde_json::json!(MIN_SCHEMA_VERSION);
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        assert!(validate_template(&template).is_err());
    }

    #[test]
    fn templates_from_a_future_version_are_refused() {
        let mut value = conditional_fixture();
        value["schemaVersion"] = serde_json::json!(SCHEMA_VERSION + 1);
        let template: TemplateEnvelope = serde_json::from_value(value).unwrap();
        assert!(validate_template(&template).is_err());
    }

    #[test]
    fn existing_version_one_templates_still_load() {
        let template: TemplateEnvelope = serde_json::from_value(fixture()).unwrap();
        assert_eq!(template.schema_version, MIN_SCHEMA_VERSION);
        validate_template(&template).unwrap();
        assert!(template.graph.edges.iter().all(|e| e.data.condition.is_none()));
    }

    #[test]
    fn strips_paths_and_accounts() {
        let graph = serde_json::json!({"nodes":[{"id":"a","type":"agent","data":{"label":"a","provider":"codex","accountRef":"local"},"position":{"x":0,"y":0}}],"edges":[]});
        let result = export_graph("x", "", &graph).unwrap();
        assert!(result.graph.nodes[0].data.get("accountRef").is_none());
        assert!(result
            .parameters
            .iter()
            .any(|p| p.kind == ParameterKind::AgentAccount));
    }
    #[test]
    fn remaps_ids_and_disables_memory() {
        let template: TemplateEnvelope = serde_json::from_value(fixture()).unwrap();
        let imported = remap_graph_ids(&template.graph);
        assert!(!imported.memory_retrieval_enabled);
        assert_ne!(imported.graph.nodes[0].id, "input");
    }
}
