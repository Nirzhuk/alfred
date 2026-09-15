use crate::agent_accounts::models::AgentAccountStatus;
use crate::agents::{AgentHarness, AgentProvider};
use crate::db::{is_current_at, Db};
use crate::integrations::actions::ActionFieldKind;
use crate::integrations::models::ConnectionStatus;
use crate::integrations::IntegrationsState;
use crate::sharing::{
    parse_share_link, Publication, SharedTemplate, SharingError, SharingState, TrustedShareOrigin,
    OWNER_CREDENTIAL_REF,
};
use crate::templates::{
    apply_parameter_values, export_workflow_with_starts, Parameter, ParameterKind, ParameterTarget,
    StartKind, StartRecipe, TargetKind, TemplateEnvelope,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationDraft {
    pub title: String,
    pub description: String,
    pub template: TemplateEnvelope,
    pub removed: Vec<String>,
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPublication {
    pub request_id: String,
    pub workflow_id: Option<String>,
    pub digest: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishInput {
    pub workflow_id: String,
    pub request_id: String,
    pub template: TemplateEnvelope,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupInput {
    pub template: TemplateEnvelope,
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportInput {
    pub template: TemplateEnvelope,
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
    pub source_link: String,
    pub digest: String,
    pub import_attempt_id: String,
}

fn map_error(error: SharingError) -> String {
    error.to_string()
}

fn canonical_import_source_link(link: &str, origin: &TrustedShareOrigin) -> Result<String, String> {
    let parsed = parse_share_link(link, origin)
        .map_err(|error| format!("invalid import source link: {error}"))?;
    origin
        .preview_url(&parsed.share_id)
        .map(|url| url.to_string())
        .map_err(|error| format!("could not canonicalize import source link: {error}"))
}

#[tauri::command]
pub fn prepare_workflow_publication(
    db: State<'_, Db>,
    integrations: State<'_, IntegrationsState>,
    workflow_id: String,
) -> Result<PublicationDraft, String> {
    prepare_publication_from_db(db.inner(), integrations.inner(), &workflow_id)
}

fn prepare_publication_from_db(
    db: &Db,
    integrations: &IntegrationsState,
    workflow_id: &str,
) -> Result<PublicationDraft, String> {
    let workflow = db
        .get_workflow(&workflow_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
    let mut starts = Vec::new();
    let mut start_connections = BTreeMap::new();
    if let Some(schedule) = db
        .get_schedule_for_workflow(&workflow_id)
        .map_err(|error| error.to_string())?
    {
        let mut config = serde_json::Map::new();
        config.insert("cron".into(), Value::String(schedule.cron));
        config.insert("timezone".into(), Value::String("UTC".into()));
        starts.push(StartRecipe {
            kind: StartKind::Schedule,
            config,
        });
    }
    for trigger in db
        .list_triggers(&workflow_id)
        .map_err(|error| error.to_string())?
    {
        let kind = match trigger.source.as_str() {
            "file" => StartKind::File,
            "app" | "appEvent" => StartKind::AppEvent,
            "webhook" => StartKind::Webhook,
            _ => continue,
        };
        let mut config = trigger.config.as_object().cloned().unwrap_or_default();
        if kind == StartKind::AppEvent {
            if let Some(connection) = config.get("connectionId").and_then(Value::as_str) {
                start_connections.insert(starts.len(), connection.to_string());
            }
        }
        config.remove("connectionId");
        config.remove("secret");
        config.remove("descriptorVersion");
        if !trigger.label.trim().is_empty() {
            config.insert("label".into(), Value::String(trigger.label.clone()));
        }
        if kind == StartKind::AppEvent && !config.contains_key("event") {
            if let Some(event) = config.get("eventType").cloned() {
                config.insert("event".into(), event);
            } else {
                config.insert("event".into(), Value::String(trigger.label.clone()));
            }
        }
        if kind == StartKind::AppEvent {
            let provider = config
                .get("providerId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let event_type = config
                .get("eventType")
                .and_then(Value::as_str)
                .or_else(|| config.get("event").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            let descriptor = integrations
                .events
                .descriptor(provider, &event_type)
                .ok_or_else(|| {
                    format!("app event {provider}:{event_type} has no portability descriptor")
                })?;
            let source_filters = config
                .get("filters")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut filters = serde_json::Map::new();
            for (key, value) in source_filters {
                let field = descriptor
                    .filter_fields
                    .iter()
                    .find(|field| field.key == key)
                    .ok_or_else(|| {
                        format!("app event {provider}:{event_type} has unsupported filter {key}")
                    })?;
                if field.kind == ActionFieldKind::ResourceSelector {
                    filters.insert(key, Value::Null);
                } else {
                    filters.insert(key, value);
                }
            }
            config.insert("providerId".into(), Value::String(provider.into()));
            config.insert("eventType".into(), Value::String(event_type));
            config.insert("connectionId".into(), Value::String(String::new()));
            config.insert("filters".into(), Value::Object(filters));
        }
        if kind == StartKind::Webhook && !config.contains_key("label") {
            config.insert("label".into(), Value::String("Imported webhook".into()));
        }
        starts.push(StartRecipe { kind, config });
    }
    let mut draft = build_publication_draft(
        &workflow.name,
        &workflow.description,
        &workflow.working_directory,
        &workflow.graph,
        &starts,
        integrations,
    )?;
    for (index, source_connection) in start_connections {
        let key = format!("start.{index}.connection");
        let Some(start_parameter) = draft
            .template
            .parameters
            .iter()
            .find(|parameter| parameter.key == key)
            .cloned()
        else {
            continue;
        };
        let existing = draft.template.parameters.iter().position(|parameter| {
            parameter.key != key
                && parameter.kind == ParameterKind::AppConnection
                && parameter.allowed_provider == start_parameter.allowed_provider
                && std::iter::once(&parameter.target)
                    .chain(&parameter.targets)
                    .any(|target| {
                        target.kind == TargetKind::Node
                            && workflow
                                .graph
                                .get("nodes")
                                .and_then(Value::as_array)
                                .is_some_and(|nodes| {
                                    nodes.iter().any(|node| {
                                        node.get("id").and_then(Value::as_str)
                                            == target.node_id.as_deref()
                                            && node
                                                .pointer("/data/connectionId")
                                                .and_then(Value::as_str)
                                                == Some(source_connection.as_str())
                                    })
                                })
                    })
        });
        if let Some(existing) = existing {
            draft.template.parameters[existing]
                .targets
                .push(start_parameter.target);
            draft
                .template
                .parameters
                .retain(|parameter| parameter.key != key);
        }
    }
    crate::templates::validate_template(&draft.template).map_err(|error| error.to_string())?;
    Ok(draft)
}

fn build_publication_draft(
    name: &str,
    description: &str,
    working_directory: &str,
    graph: &Value,
    starts: &[StartRecipe],
    integrations: &IntegrationsState,
) -> Result<PublicationDraft, String> {
    let mut template =
        export_workflow_with_starts(name, description, working_directory, graph, starts)
            .map_err(|error| error.to_string())?;
    add_start_portability_parameters(&mut template, integrations)?;
    apply_action_descriptor_portability(&mut template, graph, integrations)?;
    let removed = template
        .parameters
        .iter()
        .map(|parameter| parameter.key.clone())
        .collect();
    let requirements = template
        .requirements
        .iter()
        .map(|requirement| format!("{}: {}", requirement.kind.kind_string(), requirement.name))
        .collect();
    Ok(PublicationDraft {
        title: template.title.clone(),
        description: template.description.clone(),
        template,
        removed,
        requirements,
    })
}

fn add_start_portability_parameters(
    template: &mut TemplateEnvelope,
    integrations: &IntegrationsState,
) -> Result<(), String> {
    for index in 0..template.starts.len() {
        let start = template.starts[index].clone();
        match start.kind {
            StartKind::File => {
                if start.config.contains_key("path") {
                    template.starts[index]
                        .config
                        .insert("path".into(), Value::String(String::new()));
                    template.parameters.push(Parameter {
                        key: format!("start.{index}.path"),
                        label: "Watched folder".into(),
                        description: "Choose a local folder for this file trigger.".into(),
                        kind: ParameterKind::Folder,
                        required: true,
                        allowed_provider: None,
                        allowed_action: None,
                        target: ParameterTarget {
                            kind: TargetKind::Start,
                            node_id: None,
                            field: format!("{index}.path"),
                        },
                        targets: Vec::new(),
                    });
                }
            }
            StartKind::AppEvent => {
                let provider = start
                    .config
                    .get("providerId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let event_type = start
                    .config
                    .get("eventType")
                    .and_then(Value::as_str)
                    .or_else(|| start.config.get("event").and_then(Value::as_str))
                    .unwrap_or_default();
                let descriptor = integrations
                    .events
                    .descriptor(provider, event_type)
                    .ok_or_else(|| {
                        format!("app event {provider}:{event_type} has no portability descriptor")
                    })?;
                template.parameters.push(Parameter {
                    key: format!("start.{index}.connection"),
                    label: "Event connection".into(),
                    description: "Choose the connected app for this event trigger.".into(),
                    kind: ParameterKind::AppConnection,
                    required: true,
                    allowed_provider: Some(provider.into()),
                    allowed_action: None,
                    target: ParameterTarget {
                        kind: TargetKind::Start,
                        node_id: None,
                        field: format!("{index}.connectionId"),
                    },
                    targets: Vec::new(),
                });
                let filters = start
                    .config
                    .get("filters")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                for field in descriptor.filter_fields.iter().filter(|field| {
                    field.kind == ActionFieldKind::ResourceSelector
                        && (field.required || filters.contains_key(&field.key))
                }) {
                    template.parameters.push(Parameter {
                        key: format!("start.{index}.filter.{}", field.key),
                        label: field.label.clone(),
                        description: field.description.clone(),
                        kind: ParameterKind::AppResource,
                        required: field.required || filters.contains_key(&field.key),
                        allowed_provider: Some(provider.into()),
                        allowed_action: Some(event_type.into()),
                        target: ParameterTarget {
                            kind: TargetKind::Start,
                            node_id: None,
                            field: format!("{index}.filters.{}", field.key),
                        },
                        targets: Vec::new(),
                    });
                }
            }
            StartKind::Schedule | StartKind::Webhook => {}
        }
    }
    crate::templates::validate_template(template).map_err(|error| error.to_string())
}

/// The generic graph exporter cannot infer whether an app action input is a
/// resource selector, public text, or an execution-required value. Reconcile
/// its provisional bindings against the registered descriptor before showing
/// the review snapshot. Unknown fields and secret fields block publication;
/// resource selectors remain typed setup values while ordinary public inputs
/// stay in the reusable template.
fn apply_action_descriptor_portability(
    template: &mut TemplateEnvelope,
    source_graph: &Value,
    integrations: &IntegrationsState,
) -> Result<(), String> {
    let source_nodes = source_graph
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "workflow graph nodes are required".to_string())?;
    let mut source_by_id = std::collections::HashMap::new();
    for node in source_nodes {
        if let Some(object) = node.as_object() {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                source_by_id.insert(id, object);
            }
        }
    }
    for node in template
        .graph
        .nodes
        .iter_mut()
        .filter(|node| node.node_type == "appAction")
    {
        let source = source_by_id
            .get(node.id.as_str())
            .ok_or_else(|| format!("app action {} has no source node", node.id))?;
        let provider = node
            .data
            .get("providerId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let action = node
            .data
            .get("actionId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let descriptor = integrations
            .action_descriptors(Some(provider))
            .into_iter()
            .find(|item| item.action_id == action)
            .ok_or_else(|| {
                format!("app action {provider}:{action} has no portability descriptor")
            })?;
        let source_input = source
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("input"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let fields: std::collections::HashMap<_, _> = descriptor
            .fields
            .iter()
            .map(|field| (field.key.as_str(), field))
            .collect();
        for key in source_input
            .keys()
            .filter(|key| !key.ends_with("__display"))
        {
            let field = fields.get(key.as_str()).ok_or_else(|| {
                format!("app action {provider}:{action} has unsupported input {key}")
            })?;
            if field.secret {
                return Err(format!(
                    "app action {provider}:{action} contains a secret input {key}"
                ));
            }
        }
        let node_id = node.id.clone();
        let mut parameters = template
            .parameters
            .iter()
            .filter(|parameter| {
                !(parameter.target.node_id.as_deref() == Some(node_id.as_str())
                    && parameter.target.field.starts_with("input."))
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut input = serde_json::Map::new();
        for field in &descriptor.fields {
            let value = source_input
                .get(&field.key)
                .cloned()
                .or_else(|| field.default.clone());
            let value_is_blank = value.as_ref().is_none_or(|value| match value {
                Value::Null => true,
                Value::String(text) => text.trim().is_empty(),
                _ => false,
            });
            if field.required && value_is_blank && field.kind != ActionFieldKind::ResourceSelector {
                return Err(format!(
                    "required app action input {provider}:{action}.{} has no value",
                    field.key
                ));
            }
            if let Some(Value::String(text)) = value.as_ref() {
                if contains_private_text(text) {
                    return Err(format!(
                        "app action {provider}:{action} contains a likely private value in {}",
                        field.key
                    ));
                }
            }
            if let Some(value) = value.as_ref().filter(|_| !value_is_blank) {
                let valid_type = match field.kind {
                    ActionFieldKind::Text
                    | ActionFieldKind::Textarea
                    | ActionFieldKind::Enum
                    | ActionFieldKind::ResourceSelector => value.is_string(),
                    ActionFieldKind::Boolean => value.is_boolean(),
                };
                if !valid_type {
                    return Err(format!(
                        "app action input {provider}:{action}.{} has the wrong type",
                        field.key
                    ));
                }
            }
            if field.kind == ActionFieldKind::ResourceSelector {
                if field.required || !value_is_blank {
                    let key = format!("{}.input.{}", node_id, field.key);
                    parameters.push(crate::templates::Parameter {
                        key: key.clone(),
                        label: field.label.clone(),
                        description: field.description.clone(),
                        kind: crate::templates::ParameterKind::AppResource,
                        required: field.required || !value_is_blank,
                        allowed_provider: Some(provider.to_string()),
                        allowed_action: Some(action.to_string()),
                        target: crate::templates::ParameterTarget {
                            kind: crate::templates::TargetKind::Node,
                            node_id: Some(node_id.clone()),
                            field: format!("input.{}", field.key),
                        },
                        targets: Vec::new(),
                    });
                    input.insert(field.key.clone(), Value::Null);
                }
            } else if matches!(
                field.kind,
                ActionFieldKind::Text | ActionFieldKind::Textarea
            ) && matches!(
                field.key.as_str(),
                "to" | "cc" | "bcc" | "email" | "recipient" | "recipients" | "recipientEmail"
            ) && (field.required || !value_is_blank)
            {
                parameters.push(Parameter {
                    key: format!("{}.input.{}", node_id, field.key),
                    label: field.label.clone(),
                    description: field.description.clone(),
                    kind: ParameterKind::Email,
                    required: field.required || !value_is_blank,
                    allowed_provider: Some(provider.into()),
                    allowed_action: None,
                    target: ParameterTarget {
                        kind: TargetKind::Node,
                        node_id: Some(node_id.clone()),
                        field: format!("input.{}", field.key),
                    },
                    targets: Vec::new(),
                });
                input.insert(field.key.clone(), Value::Null);
            } else if let Some(value) = value.filter(|_| !value_is_blank) {
                input.insert(field.key.clone(), value);
            }
        }
        node.data.insert("input".into(), Value::Object(input));
        template.parameters = parameters;
    }
    crate::templates::validate_template(template).map_err(|error| error.to_string())
}

fn contains_private_text(value: &str) -> bool {
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

fn validate_action_descriptor_inputs(
    template: &TemplateEnvelope,
    integrations: &IntegrationsState,
) -> Vec<String> {
    let mut errors = Vec::new();
    for node in template
        .graph
        .nodes
        .iter()
        .filter(|node| node.node_type == "appAction")
    {
        let provider = node
            .data
            .get("providerId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let action = node
            .data
            .get("actionId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(descriptor) = integrations
            .action_descriptors(Some(provider))
            .into_iter()
            .find(|item| item.action_id == action)
        else {
            errors.push(format!(
                "app action {provider}:{action} is unavailable locally"
            ));
            continue;
        };
        let input = node
            .data
            .get("input")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let fields: std::collections::HashMap<_, _> = descriptor
            .fields
            .iter()
            .map(|field| (field.key.as_str(), field))
            .collect();
        for key in input.keys() {
            if !fields.contains_key(key.as_str()) {
                errors.push(format!(
                    "app action {provider}:{action} has unsupported input {key}"
                ));
            }
        }
        for field in &descriptor.fields {
            for parameter in &template.parameters {
                let binds = std::iter::once(&parameter.target)
                    .chain(&parameter.targets)
                    .any(|target| {
                        target.kind == TargetKind::Node
                            && target.node_id.as_deref() == Some(node.id.as_str())
                            && target.field == format!("input.{}", field.key)
                    });
                if binds
                    && ((field.kind == ActionFieldKind::ResourceSelector)
                        != (parameter.kind == ParameterKind::AppResource))
                {
                    errors.push(format!(
                        "{} must use the descriptor's setup type for {}",
                        parameter.label, field.label
                    ));
                }
            }
            let Some(value) = input.get(&field.key) else {
                if field.required {
                    errors.push(format!("{} is required for execution", field.label));
                }
                continue;
            };
            if field.secret {
                errors.push(format!(
                    "app action {provider}:{action} contains secret input {}",
                    field.key
                ));
                continue;
            }
            if value.is_null() {
                let bound = template.parameters.iter().any(|parameter| {
                    ((field.kind == ActionFieldKind::ResourceSelector
                        && parameter.kind == ParameterKind::AppResource)
                        || (matches!(
                            field.kind,
                            ActionFieldKind::Text | ActionFieldKind::Textarea
                        ) && matches!(
                            parameter.kind,
                            ParameterKind::Text | ParameterKind::Email
                        )))
                        && std::iter::once(&parameter.target)
                            .chain(parameter.targets.iter())
                            .any(|target| {
                                target.kind == crate::templates::TargetKind::Node
                                    && target.node_id.as_deref() == Some(node.id.as_str())
                                    && target.field == format!("input.{}", field.key)
                            })
                });
                if bound {
                    // Exported templates intentionally carry a null selector;
                    // the typed recipient value is applied before the commit
                    // boundary check below.
                    continue;
                }
                if field.required {
                    errors.push(format!("{} is required for execution", field.label));
                }
                continue;
            }
            let valid_type = match field.kind {
                ActionFieldKind::Text
                | ActionFieldKind::Textarea
                | ActionFieldKind::Enum
                | ActionFieldKind::ResourceSelector => value.is_string(),
                ActionFieldKind::Boolean => value.is_boolean(),
            };
            if !valid_type {
                errors.push(format!(
                    "app action input {provider}:{action}.{} has the wrong type",
                    field.key
                ));
            } else if field.required && value_is_blank(value) {
                errors.push(format!("{} is required for execution", field.label));
            }
        }
    }
    errors
}

fn validate_runtime_requirements(
    db: &Db,
    integrations: &IntegrationsState,
    template: &TemplateEnvelope,
    values: &BTreeMap<String, Value>,
    models: fn(Option<AgentHarness>) -> Vec<crate::agents::models::ProviderModels>,
) -> Vec<String> {
    let accounts = template
        .parameters
        .iter()
        .filter(|parameter| parameter.kind == crate::templates::ParameterKind::AgentAccount)
        .filter_map(|parameter| values.get(&parameter.key).and_then(Value::as_str))
        .filter_map(|id| db.get_agent_account(id).ok().flatten())
        .collect::<Vec<_>>();
    let agent_nodes = template
        .graph
        .nodes
        .iter()
        .filter(|node| node.node_type == "agent")
        .filter_map(|node| {
            let provider = node.data.get("provider").and_then(Value::as_str)?;
            let harness = AgentHarness::parse_persisted(node.data.get("harness")).ok()?;
            Some((provider, harness))
        })
        .collect::<Vec<_>>();
    let cli_available = |provider: &str| {
        models(Some(AgentHarness::Cli))
            .into_iter()
            .any(|item| item.provider == provider && item.available)
    };
    let connected_account = |account: &&crate::agent_accounts::models::AgentAccount| {
        account.status == AgentAccountStatus::Connected
            && expiry_is_current(account.expires_at.as_deref())
    };
    let mut errors = Vec::new();
    for requirement in &template.requirements {
        match requirement.kind {
            crate::templates::RequirementKind::Agent => {
                let provider = requirement.provider.as_deref().unwrap_or(&requirement.name);
                let account_ok = accounts.iter().any(|account| {
                    connected_account(&account)
                        && (account.provider.as_str() == requirement.name
                            || account.provider.as_str() == provider)
                });
                let cli_ok = agent_nodes.iter().any(|(node_provider, harness)| {
                    *harness == AgentHarness::Cli
                        && (*node_provider == provider || *node_provider == requirement.name)
                        && cli_available(node_provider)
                });
                if !account_ok && !cli_ok {
                    errors.push(format!(
                        "required agent {} is unavailable locally",
                        requirement.name
                    ));
                }
            }
            crate::templates::RequirementKind::Harness => {
                let account_ok = accounts.iter().any(|account| {
                    connected_account(&account) && account.harness.as_str() == requirement.name
                });
                let cli_ok = requirement.name == AgentHarness::Cli.as_str()
                    && agent_nodes.iter().any(|(provider, harness)| {
                        *harness == AgentHarness::Cli && cli_available(provider)
                    });
                if !account_ok && !cli_ok {
                    errors.push(format!(
                        "required harness {} is unavailable locally",
                        requirement.name
                    ));
                }
            }
            crate::templates::RequirementKind::Model => {
                let provider = requirement.provider.as_deref();
                let harness = accounts
                    .iter()
                    .find(|account| {
                        connected_account(account)
                            && provider.is_none_or(|provider| account.provider.as_str() == provider)
                    })
                    .map(|account| account.harness)
                    .or_else(|| {
                        provider.and_then(|provider| {
                            agent_nodes
                                .iter()
                                .find(|(node_provider, _)| *node_provider == provider)
                                .map(|(_, harness)| *harness)
                        })
                    });
                let models = models(harness);
                let known = models.iter().any(|item| {
                    item.provider == provider.unwrap_or_default()
                        && (item.models.iter().any(|model| model.id == requirement.name)
                            || item.allow_custom)
                });
                if !known {
                    errors.push(format!(
                        "required model {} is unavailable locally",
                        requirement.name
                    ));
                }
            }
            crate::templates::RequirementKind::Skill => match crate::skills::list_skills(None) {
                Ok(skills) if skills.iter().any(|skill| skill.name == requirement.name) => {}
                Ok(_) => errors.push(format!(
                    "required skill {} is unavailable locally",
                    requirement.name
                )),
                Err(error) => errors.push(format!(
                    "could not verify skill {}: {error}",
                    requirement.name
                )),
            },
            crate::templates::RequirementKind::Interpreter => {
                let found = executable_available(&requirement.name);
                if !found {
                    errors.push(format!(
                        "required interpreter {} is unavailable locally",
                        requirement.name
                    ));
                }
            }
            crate::templates::RequirementKind::AppAction => {
                let provider = requirement.provider.as_deref().unwrap_or_default();
                if !integrations
                    .action_descriptors(Some(provider))
                    .iter()
                    .any(|descriptor| descriptor.action_id == requirement.name)
                {
                    errors.push(format!(
                        "required app action {} is unavailable locally",
                        requirement.name
                    ));
                }
            }
            crate::templates::RequirementKind::AppEvent => {
                let provider = requirement.provider.as_deref().unwrap_or_default();
                if !integrations
                    .events
                    .descriptors(Some(provider))
                    .iter()
                    .any(|descriptor| descriptor.event_type == requirement.name)
                {
                    errors.push(format!(
                        "required app event {} is unavailable locally",
                        requirement.name
                    ));
                }
            }
        }
    }
    errors
}

fn expiry_is_current(expires_at: Option<&str>) -> bool {
    expires_at.is_none_or(|value| {
        DateTime::parse_from_rfc3339(value)
            .map(|parsed| parsed.with_timezone(&Utc) > Utc::now())
            .unwrap_or(false)
    })
}

fn executable_available(name: &str) -> bool {
    let candidates = std::iter::once(Path::new(name).to_path_buf()).chain(
        std::env::var_os("PATH").into_iter().flat_map(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join(name))
                .collect::<Vec<_>>()
        }),
    );
    candidates.into_iter().any(|candidate| {
        let Ok(metadata) = fs::metadata(candidate) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    })
}

/// Validate every resolved executable node against the exact local capability
/// it will use. Template requirements are aggregate for display; this second
/// pass is per-node so one account cannot satisfy a different node's provider,
/// harness, model, or expiry requirements.
#[cfg(test)]
fn validate_resolved_execution_capabilities(db: &Db, template: &TemplateEnvelope) -> Vec<String> {
    validate_resolved_execution_with_catalog(
        db,
        template,
        crate::agents::models::list_provider_models,
    )
}

fn validate_resolved_execution_with_catalog(
    db: &Db,
    template: &TemplateEnvelope,
    models: fn(Option<AgentHarness>) -> Vec<crate::agents::models::ProviderModels>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for node in &template.graph.nodes {
        if node.node_type == "agent" {
            let provider = node
                .data
                .get("provider")
                .and_then(Value::as_str)
                .and_then(AgentProvider::from_str);
            let harness = match AgentHarness::parse_persisted(node.data.get("harness")) {
                Ok(value) => value,
                Err(error) => {
                    errors.push(format!("agent {} has invalid harness: {error}", node.id));
                    continue;
                }
            };
            let account_ref = node
                .data
                .get("accountRef")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if harness == AgentHarness::Alfred && account_ref.is_none() {
                errors.push(format!(
                    "agent {} requires a selected local account for the Alfred harness",
                    node.id
                ));
            }
            if let Some(account_ref) = account_ref {
                match db.get_agent_account(account_ref) {
                    Ok(Some(account)) => {
                        if account.status != AgentAccountStatus::Connected {
                            errors.push(format!(
                                "agent {} selected account is not connected",
                                node.id
                            ));
                        }
                        if !expiry_is_current(account.expires_at.as_deref()) {
                            errors.push(format!("agent {} selected account has expired", node.id));
                        }
                        if provider.is_some_and(|expected| account.provider != expected) {
                            errors.push(format!(
                                "agent {} selected account does not match its provider",
                                node.id
                            ));
                        }
                        if account.harness != harness {
                            errors.push(format!(
                                "agent {} selected account does not support its harness",
                                node.id
                            ));
                        }
                    }
                    Ok(None) => errors.push(format!(
                        "agent {} selected account is unavailable locally",
                        node.id
                    )),
                    Err(error) => errors.push(format!(
                        "could not verify agent {} selected account: {error}",
                        node.id
                    )),
                }
            }
            let Some(provider) = provider else {
                errors.push(format!("agent {} has no supported provider", node.id));
                continue;
            };
            let model = node.data.get("model").and_then(Value::as_str).unwrap_or("");
            let catalog = models(Some(harness))
                .into_iter()
                .find(|item| item.provider == provider.as_str());
            match catalog {
                Some(catalog) if catalog.available => {
                    if !model.trim().is_empty()
                        && !catalog.models.iter().any(|item| item.id == model)
                        && !catalog.allow_custom
                    {
                        errors.push(format!(
                            "agent {} model {} is unavailable for its selected provider",
                            node.id, model
                        ));
                    }
                }
                Some(_) | None => errors.push(format!(
                    "agent {} runtime {} is unavailable locally",
                    node.id,
                    provider.as_str()
                )),
            }
            if let Some(skills) = node
                .data
                .get("skillNames")
                .and_then(Value::as_array)
                .filter(|skills| !skills.is_empty())
            {
                match crate::skills::list_skills(None) {
                    Ok(available) => {
                        for skill in skills.iter().filter_map(Value::as_str) {
                            if !available.iter().any(|item| item.name == skill) {
                                errors.push(format!(
                                    "agent {} skill {} is unavailable locally",
                                    node.id, skill
                                ));
                            }
                        }
                    }
                    Err(error) => errors.push(format!(
                        "could not verify skills for agent {}: {error}",
                        node.id
                    )),
                }
            }
        }
        if node.node_type == "memory" {
            if let Some(ids) = node.data.get("memoryIds").and_then(Value::as_array) {
                for id in ids.iter().filter_map(Value::as_str) {
                    match db.get_memory(id) {
                        Ok(Some(memory)) if is_current_at(&memory, Utc::now()) => {}
                        Ok(Some(_)) | Ok(None) => errors.push(format!(
                            "memory node {} references an expired or unavailable memory",
                            node.id
                        )),
                        Err(error) => errors
                            .push(format!("could not verify memory node {}: {error}", node.id)),
                    }
                }
            }
        }
        let interpreter = node
            .data
            .get("interpreter")
            .and_then(Value::as_str)
            .or_else(|| {
                node.data
                    .get("script")
                    .and_then(Value::as_object)
                    .and_then(|script| script.get("interpreter"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(interpreter) = interpreter {
            if !executable_available(interpreter) {
                errors.push(format!(
                    "node {} interpreter {} is unavailable or not executable",
                    node.id, interpreter
                ));
            }
        }
    }
    errors
}

/// Check the connection and descriptor target for every resolved action and
/// event start, including targets that were not represented by a setup
/// parameter. The review envelope may be hand-authored, so parameter checks
/// alone cannot establish that the execution target is still usable.
fn local_app_start(
    start: &StartRecipe,
    integrations: &IntegrationsState,
) -> Result<crate::integrations::events::AppTriggerConfig, String> {
    let provider = start
        .config
        .get("providerId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let event = start
        .config
        .get("eventType")
        .or_else(|| start.config.get("event"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let descriptor = integrations
        .events
        .descriptor(provider, event)
        .ok_or_else(|| format!("app event {provider}:{event} is unavailable locally"))?;
    Ok(crate::integrations::events::AppTriggerConfig {
        provider_id: provider.into(),
        event_type: event.into(),
        connection_id: start
            .config
            .get("connectionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        filters: match start.config.get("filters") {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| "invalid app event filters".to_string())?,
            None => BTreeMap::new(),
        },
        descriptor_version: descriptor.descriptor_version,
    })
}

fn validate_resolved_app_targets(
    db: &Db,
    integrations: &IntegrationsState,
    template: &TemplateEnvelope,
) -> Vec<String> {
    let mut errors = Vec::new();
    for node in template
        .graph
        .nodes
        .iter()
        .filter(|node| node.node_type == "appAction")
    {
        let input = match node.data.get("input") {
            Some(value) => match serde_json::from_value(value.clone()) {
                Ok(input) => input,
                Err(_) => {
                    errors.push(format!("app action {} has invalid input", node.id));
                    continue;
                }
            },
            None => BTreeMap::new(),
        };
        let request = crate::integrations::actions::ActionRequest {
            connection_id: node
                .data
                .get("connectionId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            provider_id: node
                .data
                .get("providerId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            action_id: node
                .data
                .get("actionId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            input,
        };
        if db
            .get_app_connection(&request.connection_id)
            .ok()
            .flatten()
            .is_some_and(|connection| !expiry_is_current(connection.expires_at.as_deref()))
        {
            errors.push(format!("app action {} connection has expired", node.id));
        }
        if let Err(error) = integrations.actions.validate_setup(db, request) {
            errors.push(format!("app action {} is not ready: {error}", node.id));
        }
    }
    for (index, start) in template
        .starts
        .iter()
        .enumerate()
        .filter(|(_, start)| start.kind == StartKind::AppEvent)
    {
        match local_app_start(start, integrations) {
            Ok(config) => {
                if db
                    .get_app_connection(&config.connection_id)
                    .ok()
                    .flatten()
                    .is_some_and(|connection| !expiry_is_current(connection.expires_at.as_deref()))
                {
                    errors.push(format!("app event start {index} connection has expired"));
                }
                if let Err(error) = integrations.validate_app_trigger(db, &config) {
                    errors.push(format!("app event start {index} is not ready: {error}"));
                }
            }
            Err(error) => errors.push(error),
        }
    }
    errors
}

#[tauri::command]
pub async fn publish_workflow_template(
    db: State<'_, Db>,
    sharing: State<'_, SharingState>,
    input: PublishInput,
) -> Result<Publication, String> {
    let sharing = sharing
        .0
        .as_ref()
        .ok_or_else(|| "sharing service is not configured".to_string())?;
    if input.request_id.trim().is_empty() {
        return Err("publication request ID is required".into());
    }
    if db
        .get_workflow(&input.workflow_id)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err("workflow not found".into());
    }
    let expected = crate::templates::digest(&input.template).map_err(|error| error.to_string())?;
    let snapshot = serde_json::to_string(&input.template).map_err(|error| error.to_string())?;
    if let Some(existing) = db
        .begin_template_publication(
            &input.workflow_id,
            &input.request_id,
            &expected,
            OWNER_CREDENTIAL_REF,
            &snapshot,
        )
        .map_err(|error| error.to_string())?
    {
        if let Some(share_id) = existing.share_id {
            // Repair metadata if an older interrupted implementation recorded
            // the service response before inserting the local management row.
            // The operation is idempotent and keeps retries reviewable.
            db.complete_template_publication(
                Some(&input.workflow_id),
                &input.request_id,
                &existing.digest,
                OWNER_CREDENTIAL_REF,
                &share_id,
            )
            .map_err(|error| error.to_string())?;
            let url = sharing
                .origin()
                .preview_url(&share_id)
                .map_err(|error| error.to_string())?;
            return Ok(Publication {
                share_id,
                url: url.to_string(),
                digest: existing.digest,
            });
        }
    }
    let publication = sharing
        .publish(&input.template, Some(&input.request_id))
        .await
        .map_err(map_error)?;
    db.complete_template_publication(
        Some(&input.workflow_id),
        &input.request_id,
        &publication.digest,
        OWNER_CREDENTIAL_REF,
        &publication.share_id,
    )
    .map_err(|error| error.to_string())?;
    Ok(publication)
}

#[tauri::command]
pub fn list_workflow_publications(
    db: State<'_, Db>,
    sharing: State<'_, SharingState>,
    workflow_id: Option<String>,
) -> Result<Vec<Publication>, String> {
    let sharing = sharing
        .0
        .as_ref()
        .ok_or_else(|| "sharing service is not configured".to_string())?;
    db.list_template_shares(workflow_id.as_deref())
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|record| record.revoked_at.is_none())
        .map(|record| {
            let url = sharing
                .origin()
                .preview_url(&record.id)
                .map_err(|error| error.to_string())?;
            Ok(Publication {
                share_id: record.id,
                url: url.to_string(),
                digest: record.digest,
            })
        })
        .collect()
}

#[tauri::command]
pub fn list_pending_workflow_publications(
    db: State<'_, Db>,
) -> Result<Vec<PendingPublication>, String> {
    db.list_pending_template_publications()
        .map(|records| {
            records
                .into_iter()
                .map(|record| PendingPublication {
                    request_id: record.request_id,
                    workflow_id: record.workflow_id,
                    digest: record.digest,
                    created_at: record.created_at,
                })
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resume_workflow_publication(
    db: State<'_, Db>,
    sharing: State<'_, SharingState>,
    request_id: String,
) -> Result<Publication, String> {
    let sharing = sharing
        .0
        .as_ref()
        .ok_or_else(|| "sharing service is not configured".to_string())?;
    let pending = db
        .get_template_publication_request(&request_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "publication request not found".to_string())?;
    if pending.credential_ref != OWNER_CREDENTIAL_REF {
        return Err("publication owner capability is unavailable".into());
    }
    let snapshot = pending
        .template_json
        .ok_or_else(|| "publication snapshot is unavailable".to_string())?;
    let template: TemplateEnvelope =
        serde_json::from_str(&snapshot).map_err(|error| error.to_string())?;
    let publication = sharing
        .publish(&template, Some(&request_id))
        .await
        .map_err(map_error)?;
    db.complete_template_publication(
        pending.workflow_id.as_deref(),
        &request_id,
        &publication.digest,
        OWNER_CREDENTIAL_REF,
        &publication.share_id,
    )
    .map_err(|error| error.to_string())?;
    Ok(publication)
}

#[tauri::command]
pub async fn revoke_workflow_publication(
    sharing: State<'_, SharingState>,
    db: State<'_, Db>,
    share_id: String,
) -> Result<(), String> {
    let sharing = sharing
        .0
        .as_ref()
        .ok_or_else(|| "sharing service is not configured".to_string())?;
    sharing.revoke(&share_id).await.map_err(map_error)?;
    let _ = db
        .revoke_template_share(&share_id)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn resolve_shared_workflow_link(
    sharing: State<'_, SharingState>,
    link: String,
) -> Result<SharedTemplate, String> {
    let sharing = sharing
        .0
        .as_ref()
        .ok_or_else(|| "sharing service is not configured".to_string())?;
    let origin = sharing.origin();
    let parsed = parse_share_link(&link, origin).map_err(|error| error.to_string())?;
    sharing.fetch(&parsed).await.map_err(map_error)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupCheck {
    pub complete: bool,
    pub errors: Vec<String>,
}

fn capability_errors(
    db: &Db,
    template: &TemplateEnvelope,
    values: &BTreeMap<String, Value>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for parameter in &template.parameters {
        let mandatory = matches!(
            parameter.kind,
            crate::templates::ParameterKind::AgentAccount
                | crate::templates::ParameterKind::AppConnection
                | crate::templates::ParameterKind::AppResource
                | crate::templates::ParameterKind::Memory
                | crate::templates::ParameterKind::Folder
                | crate::templates::ParameterKind::File
                | crate::templates::ParameterKind::Channel
        );
        if mandatory
            && values
                .get(&parameter.key)
                .is_none_or(|value| value.is_null() || value_is_blank(value))
        {
            errors.push(format!("{} is required for execution", parameter.label));
        }
    }
    let resolved = match apply_parameter_values(template, values) {
        Ok(value) => value,
        Err(error) => return vec![error.to_string()],
    };
    for parameter in &template.parameters {
        let Some(value) = values.get(&parameter.key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        match parameter.kind {
            crate::templates::ParameterKind::AgentAccount => {
                let Some(id) = value.as_str().filter(|item| !item.trim().is_empty()) else {
                    continue;
                };
                match db.get_agent_account(id) {
                    Ok(Some(account)) if account.status == AgentAccountStatus::Connected => {
                        if let Some(provider) = parameter.allowed_provider.as_deref() {
                            if account.provider.as_str() != provider {
                                errors.push(format!(
                                    "{} is not compatible with provider {}",
                                    parameter.label, provider
                                ));
                            }
                        }
                    }
                    Ok(Some(_)) => errors.push(format!("{} is not connected", parameter.label)),
                    Ok(None) => errors.push(format!(
                        "{} does not name a local agent account",
                        parameter.label
                    )),
                    Err(error) => {
                        errors.push(format!("could not verify {}: {error}", parameter.label))
                    }
                }
            }
            crate::templates::ParameterKind::AppConnection => {
                let Some(id) = value.as_str().filter(|item| !item.trim().is_empty()) else {
                    continue;
                };
                match db.get_app_connection(id) {
                    Ok(Some(connection))
                        if connection.status == ConnectionStatus::Connected
                            && expiry_is_current(connection.expires_at.as_deref()) =>
                    {
                        if let Some(provider) = parameter.allowed_provider.as_deref() {
                            if connection.provider_id != provider {
                                errors.push(format!(
                                    "{} is not compatible with provider {}",
                                    parameter.label, provider
                                ));
                            }
                        }
                    }
                    Ok(Some(connection)) if connection.status == ConnectionStatus::Connected => {
                        errors.push(format!("{} has expired", parameter.label))
                    }
                    Ok(Some(_)) => errors.push(format!("{} is not connected", parameter.label)),
                    Ok(None) => errors.push(format!(
                        "{} does not name a local connected app",
                        parameter.label
                    )),
                    Err(error) => {
                        errors.push(format!("could not verify {}: {error}", parameter.label))
                    }
                }
            }
            crate::templates::ParameterKind::Memory => {
                let ids = value
                    .as_array()
                    .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                    .unwrap_or_default();
                for id in ids {
                    match db.get_memory(id) {
                        Ok(Some(memory)) if is_current_at(&memory, Utc::now()) => {}
                        Ok(Some(_)) | Ok(None) => errors.push(format!(
                            "{} references an unavailable memory",
                            parameter.label
                        )),
                        Err(error) => {
                            errors.push(format!("could not verify {}: {error}", parameter.label))
                        }
                    }
                }
            }
            crate::templates::ParameterKind::Folder | crate::templates::ParameterKind::File => {
                let paths = if let Some(path) = value.as_str() {
                    vec![path]
                } else {
                    value
                        .as_array()
                        .map(|items| items.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default()
                };
                for path in paths {
                    let item = Path::new(path);
                    if !item.is_absolute() {
                        errors.push(format!(
                            "{} must use an absolute local path",
                            parameter.label
                        ));
                    } else if !item.exists() {
                        errors.push(format!("{} does not exist", parameter.label));
                    } else if parameter.kind == crate::templates::ParameterKind::Folder
                        && !item.is_dir()
                    {
                        errors.push(format!("{} must be a folder", parameter.label));
                    } else if parameter.kind == crate::templates::ParameterKind::File
                        && !item.is_file()
                    {
                        errors.push(format!("{} must be a file", parameter.label));
                    }
                }
            }
            crate::templates::ParameterKind::AppResource => {
                if parameter.target.kind == crate::templates::TargetKind::Start {
                    if value.as_str().is_none_or(|item| item.trim().is_empty()) {
                        errors.push(format!("{} is required", parameter.label));
                    }
                    continue;
                }
                let Some(node_id) = parameter.target.node_id.as_deref() else {
                    errors.push(format!("{} has no action target", parameter.label));
                    continue;
                };
                let connection_id = resolved
                    .graph
                    .nodes
                    .iter()
                    .find(|node| node.id == node_id)
                    .and_then(|node| node.data.get("connectionId"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if connection_id.is_empty() {
                    errors.push(format!(
                        "{} requires a connected app before resources can be selected",
                        parameter.label
                    ));
                } else if !db
                    .get_app_connection(connection_id)
                    .ok()
                    .flatten()
                    .is_some_and(|connection| {
                        connection.status == ConnectionStatus::Connected
                            && expiry_is_current(connection.expires_at.as_deref())
                    })
                {
                    errors.push(format!(
                        "{} is unavailable for the selected connected app",
                        parameter.label
                    ));
                }
                if value.as_str().is_none_or(|item| item.trim().is_empty()) {
                    errors.push(format!("{} is required", parameter.label));
                }
            }
            _ => {}
        }
    }
    if let Some(path) = values
        .get("workingDirectory")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        let item = Path::new(path);
        if !item.is_absolute() || !item.is_dir() {
            errors.push("Workflow folder must be an existing absolute directory".into());
        }
    }
    errors
}

fn value_is_blank(value: &Value) -> bool {
    match value {
        Value::String(item) => item.trim().is_empty(),
        Value::Array(items) => {
            items.is_empty()
                || items
                    .iter()
                    .any(|item| item.as_str().is_some_and(|text| text.trim().is_empty()))
        }
        _ => false,
    }
}

/// Recheck provider metadata at the commit boundary. The setup UI's resource
/// list is only a convenience and may be stale or have been revoked; this
/// resolver asks the selected connected provider whether the chosen resource
/// is still visible. It never executes the workflow action.
async fn capability_errors_at_commit(
    db: &Db,
    integrations: &IntegrationsState,
    template: &TemplateEnvelope,
    values: &BTreeMap<String, Value>,
) -> Vec<String> {
    capability_errors_with_catalog(
        db,
        integrations,
        template,
        values,
        crate::agents::models::list_provider_models,
    )
    .await
}

#[derive(Clone)]
struct ResourceCheck {
    event: bool,
    connection_id: String,
    provider: String,
    capability: String,
    field: String,
    resource_id: String,
    label: String,
}

async fn capability_errors_with_catalog(
    db: &Db,
    integrations: &IntegrationsState,
    template: &TemplateEnvelope,
    values: &BTreeMap<String, Value>,
    models: fn(Option<AgentHarness>) -> Vec<crate::agents::models::ProviderModels>,
) -> Vec<String> {
    let mut errors = capability_errors(db, template, values);
    errors.extend(validate_action_descriptor_inputs(template, integrations));
    errors.extend(validate_runtime_requirements(
        db,
        integrations,
        template,
        values,
        models,
    ));
    let Ok(resolved) = apply_parameter_values(template, values) else {
        return errors;
    };
    errors.extend(validate_action_descriptor_inputs(&resolved, integrations));
    errors.extend(validate_resolved_execution_with_catalog(
        db, &resolved, models,
    ));
    errors.extend(validate_resolved_app_targets(db, integrations, &resolved));
    if !errors.is_empty() {
        return errors;
    }

    let mut checks = Vec::new();
    for node in resolved
        .graph
        .nodes
        .iter()
        .filter(|node| node.node_type == "appAction")
    {
        let provider = node
            .data
            .get("providerId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let capability = node
            .data
            .get("actionId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(descriptor) = integrations.actions.descriptor(provider, capability) else {
            continue;
        };
        for field in descriptor
            .fields
            .iter()
            .filter(|field| field.kind == ActionFieldKind::ResourceSelector)
        {
            let Some(resource) = node
                .data
                .get("input")
                .and_then(|input| input.get(&field.key))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            checks.push(ResourceCheck {
                event: false,
                connection_id: node
                    .data
                    .get("connectionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                provider: provider.into(),
                capability: capability.into(),
                field: field.key.clone(),
                resource_id: resource.into(),
                label: format!("{} for step {}", field.label, node.id),
            });
        }
    }
    for (index, start) in resolved
        .starts
        .iter()
        .enumerate()
        .filter(|(_, start)| start.kind == StartKind::AppEvent)
    {
        let Ok(config) = local_app_start(start, integrations) else {
            continue;
        };
        let Some(descriptor) = integrations
            .events
            .descriptor(&config.provider_id, &config.event_type)
        else {
            continue;
        };
        for field in descriptor
            .filter_fields
            .iter()
            .filter(|field| field.kind == ActionFieldKind::ResourceSelector)
        {
            let Some(resource) = config
                .filters
                .get(&field.key)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            checks.push(ResourceCheck {
                event: true,
                connection_id: config.connection_id.clone(),
                provider: config.provider_id.clone(),
                capability: config.event_type.clone(),
                field: field.key.clone(),
                resource_id: resource.into(),
                label: format!("{} for start {index}", field.label),
            });
        }
    }
    for check in checks {
        let mut token: Option<String> = None;
        let mut seen = std::collections::HashSet::new();
        let mut found = false;
        for _ in 0..10 {
            let page = if check.event {
                integrations
                    .list_app_event_resources(
                        db,
                        &check.connection_id,
                        &check.provider,
                        &check.capability,
                        &check.field,
                        "",
                        token.as_deref(),
                    )
                    .await
                    .map(|page| {
                        (
                            page.items
                                .into_iter()
                                .map(|item| item.id)
                                .collect::<Vec<_>>(),
                            page.next_page_token,
                        )
                    })
                    .map_err(|error| error.to_string())
            } else {
                integrations
                    .list_action_resources_for_setup(
                        db,
                        &check.connection_id,
                        &check.provider,
                        &check.capability,
                        &check.field,
                        token.as_deref(),
                    )
                    .await
                    .map(|page| {
                        (
                            page.items
                                .into_iter()
                                .map(|item| item.id)
                                .collect::<Vec<_>>(),
                            page.next_page_token,
                        )
                    })
                    .map_err(|error| error.to_string())
            };
            match page {
                Ok((items, next)) => {
                    if items.iter().any(|id| id == &check.resource_id) {
                        found = true;
                        break;
                    }
                    match next {
                        Some(next) if seen.insert(next.clone()) => token = Some(next),
                        _ => break,
                    }
                }
                Err(error) => {
                    errors.push(format!("could not verify {}: {error}", check.label));
                    break;
                }
            }
        }
        if !found {
            errors.push(format!(
                "{} is unavailable or its resource list is incomplete",
                check.label
            ));
        }
    }
    errors
}

#[tauri::command]
pub async fn check_workflow_template_setup(
    db: State<'_, Db>,
    integrations: State<'_, IntegrationsState>,
    input: SetupInput,
) -> Result<SetupCheck, String> {
    let errors = capability_errors_at_commit(
        db.inner(),
        integrations.inner(),
        &input.template,
        &input.values,
    )
    .await;
    Ok(SetupCheck {
        complete: errors.is_empty(),
        errors,
    })
}

/// Import is kept behind one command so the production implementation can
/// replace the final section with the database's single transaction helper.
/// Validation and fresh ID remapping happen before any local write.
#[tauri::command]
pub async fn import_workflow_template(
    db: State<'_, Db>,
    integrations: State<'_, IntegrationsState>,
    sharing: State<'_, SharingState>,
    input: ImportInput,
) -> Result<Value, String> {
    let service = sharing
        .0
        .as_ref()
        .ok_or_else(|| "sharing service is not configured".to_string())?;
    import_checked(
        db.inner(),
        integrations.inner(),
        service.origin(),
        input,
        crate::agents::models::list_provider_models,
    )
    .await
}

async fn import_checked(
    db: &Db,
    integrations: &IntegrationsState,
    origin: &TrustedShareOrigin,
    input: ImportInput,
    models: fn(Option<AgentHarness>) -> Vec<crate::agents::models::ProviderModels>,
) -> Result<Value, String> {
    if input.import_attempt_id.trim().is_empty()
        || input.source_link.trim().is_empty()
        || input.digest.len() != 64
    {
        return Err("invalid import provenance".into());
    }
    // HTTPS and alfred-workflows aliases identify the same immutable
    // publication. Persist one canonical HTTPS identity so retrying an
    // interrupted import through the other transport remains idempotent.
    let canonical_source_link = canonical_import_source_link(&input.source_link, origin)?;
    let expected_digest =
        crate::templates::digest(&input.template).map_err(|error| error.to_string())?;
    if !expected_digest.eq_ignore_ascii_case(&input.digest) {
        return Err("template digest does not match the reviewed snapshot".into());
    }
    let resolved = apply_parameter_values(&input.template, &input.values)
        .map_err(|error| error.to_string())?;
    let errors =
        capability_errors_with_catalog(db, integrations, &input.template, &input.values, models)
            .await;
    if !errors.is_empty() {
        return Err(errors.join(" "));
    }
    let workdir = input
        .values
        .get("workingDirectory")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut starts = resolved.starts.clone();
    for start in starts
        .iter_mut()
        .filter(|start| start.kind == StartKind::AppEvent)
    {
        let config = local_app_start(start, integrations)?;
        let label = start.config.get("label").cloned();
        start.config = serde_json::to_value(config)
            .map_err(|error| error.to_string())?
            .as_object()
            .cloned()
            .ok_or("invalid local event configuration")?;
        if let Some(label) = label {
            start.config.insert("label".into(), label);
        }
    }
    let imported = crate::templates::remap_graph_ids(&resolved.graph);
    let imported = db
        .import_template_transaction(
            &input.import_attempt_id,
            &resolved.title,
            &resolved.description,
            workdir,
            &imported.graph,
            &starts,
            &canonical_source_link,
            &input.digest,
        )
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "workflowId": imported.workflow_id,
        "workflow": imported.workflow,
        "existingCopy": imported.reused,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::models::{
        AgentAuthMethod, AgentEntitlementState, AgentProductId, AuthorizedAgentAccount,
        CredentialCustodyMode,
    };
    use crate::agents::{AgentHarness, AgentProvider};
    use crate::db::CreateWorkflowInput;
    use crate::templates::contract::{Position, FORMAT, SCHEMA_VERSION};
    use crate::templates::{TemplateGraph, TemplateNode};

    fn account_fixture() -> AuthorizedAgentAccount {
        AuthorizedAgentAccount {
            provider: AgentProvider::Codex,
            product: AgentProductId::ChatgptCodex,
            harness: AgentHarness::Alfred,
            display_name: Some("Sharing test account".into()),
            external_account_id: "sharing-test-account".into(),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::DeviceCode,
            custody_mode: CredentialCustodyMode::RuntimeManaged,
            managed_runtime_id: AgentProductId::ChatgptCodex.managed_runtime(),
            managed_runtime_version: AgentProductId::ChatgptCodex
                .managed_runtime_version()
                .map(str::to_owned),
            runtime_profile_ref: Some("sharing-test-profile".into()),
            scopes: vec!["models:read".into()],
            billing_source: "provider_subscription".into(),
            billing_owner: "subscription_account".into(),
            entitlement_state: AgentEntitlementState::Eligible,
            entitlement_source: "test_fixture".into(),
            entitlement_observed_at: Some(Utc::now().to_rfc3339()),
            expires_at: None,
        }
    }

    fn agent_template(account_ref: &str) -> TemplateEnvelope {
        TemplateEnvelope {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            min_alfred_version: "0.1.0".into(),
            title: "Account capability fixture".into(),
            description: String::new(),
            graph: TemplateGraph {
                nodes: vec![TemplateNode {
                    id: "agent-1".into(),
                    node_type: "agent".into(),
                    data: serde_json::from_value(json!({
                        "label": "Agent",
                        "provider": "codex",
                        "harness": "alfred",
                        "accountRef": account_ref,
                        "skillNames": []
                    }))
                    .expect("agent data"),
                    position: Position { x: 0.0, y: 0.0 },
                }],
                edges: vec![],
            },
            parameters: vec![],
            requirements: vec![],
            starts: vec![],
        }
    }

    #[test]
    fn resolved_agent_checks_the_selected_account_expiry_and_identity() {
        let db = Db::open_in_memory().expect("database");
        let account = db
            .upsert_runtime_managed_account("sharing-account", account_fixture())
            .expect("account fixture");
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE agent_accounts SET status = 'connected', expires_at = ?1 WHERE id = ?2",
                rusqlite::params!["2020-01-01T00:00:00Z", account.id],
            )?;
            Ok(())
        })
        .expect("connect fixture");
        let errors = validate_resolved_execution_capabilities(&db, &agent_template(&account.id));
        assert!(errors.iter().any(|error| error.contains("has expired")));
    }

    #[test]
    fn interpreter_check_requires_an_executable_file() {
        assert!(executable_available("/bin/sh"));
        assert!(!executable_available("/etc/hosts"));
    }

    #[test]
    fn action_descriptor_adapter_preserves_public_text_and_types_resources() {
        let integrations = IntegrationsState::default();
        let graph = json!({
            "nodes": [{
                "id": "send",
                "type": "appAction",
                "data": {
                    "type": "appAction",
                    "label": "Send",
                    "providerId": "slack",
                    "actionId": "slack.send_message",
                    "connectionId": "connection-local",
                    "input": { "conversation": "C123", "text": "Hello team" }
                },
                "position": { "x": 0, "y": 0 }
            }],
            "edges": []
        });
        let mut template =
            crate::templates::export_workflow("Send", "", "", &graph).expect("generic export");
        apply_action_descriptor_portability(&mut template, &graph, &integrations)
            .expect("descriptor-backed export");
        let node = &template.graph.nodes[0];
        assert_eq!(node.data["input"]["text"], "Hello team");
        assert!(node.data["input"]["conversation"].is_null());
        assert!(template.parameters.iter().any(|parameter| {
            parameter.kind == ParameterKind::AppResource
                && parameter.target.field == "input.conversation"
                && parameter.required
        }));
        assert!(template.parameters.iter().any(|parameter| {
            parameter.kind == ParameterKind::AppConnection
                && parameter.target.field == "connectionId"
        }));
    }

    #[test]
    fn event_start_adapter_parameterizes_connection_and_resource_filter() {
        let integrations = IntegrationsState::default();
        let mut config = serde_json::Map::new();
        config.insert("providerId".into(), json!("slack"));
        config.insert("eventType".into(), json!("slack.app_mention"));
        config.insert("connectionId".into(), Value::String(String::new()));
        config.insert(
            "filters".into(),
            json!({ "channelId": null, "action": "created" }),
        );
        let mut template = TemplateEnvelope {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            min_alfred_version: "0.1.0".into(),
            title: "Mention trigger".into(),
            description: String::new(),
            graph: TemplateGraph {
                nodes: vec![],
                edges: vec![],
            },
            parameters: vec![],
            requirements: vec![],
            starts: vec![StartRecipe {
                kind: StartKind::AppEvent,
                config,
            }],
        };
        add_start_portability_parameters(&mut template, &integrations)
            .expect("event start adapter");
        assert!(template.parameters.iter().any(|parameter| {
            parameter.kind == ParameterKind::AppConnection
                && parameter.target.field == "0.connectionId"
        }));
        assert!(template.parameters.iter().any(|parameter| {
            parameter.kind == ParameterKind::AppResource
                && parameter.target.field == "0.filters.channelId"
                && parameter.allowed_action.as_deref() == Some("slack.app_mention")
        }));
    }

    #[test]
    fn required_unparameterized_resource_selector_is_rejected() {
        let integrations = IntegrationsState::default();
        let template: TemplateEnvelope = serde_json::from_value(json!({
            "format": FORMAT,
            "schemaVersion": SCHEMA_VERSION,
            "minAlfredVersion": "1.0.0",
            "title": "Unbound action",
            "description": "",
            "graph": {"nodes": [{"id": "action", "type": "appAction", "data": {
                "type": "appAction", "providerId": "slack", "actionId": "slack.send_message",
                "connectionId": "local", "input": {"conversation": null, "text": "Hello"}
            }, "position": {"x": 0, "y": 0}}], "edges": []},
            "parameters": [], "requirements": [], "starts": []
        }))
        .expect("template");
        let errors = validate_action_descriptor_inputs(&template, &integrations);
        assert!(errors.iter().any(|error| error.contains("Conversation")));
    }

    #[test]
    fn https_and_app_aliases_have_one_durable_import_identity() {
        let origin = TrustedShareOrigin::parse("https://share.example/", false).expect("origin");
        let id = "a".repeat(32);
        let https = canonical_import_source_link(&format!("https://share.example/s/{id}"), &origin)
            .expect("https link");
        let app = canonical_import_source_link(&format!("alfred-workflows://import/{id}"), &origin)
            .expect("app link");
        assert_eq!(https, app);
        assert_eq!(https, format!("https://share.example/s/{id}"));
    }
}

#[cfg(test)]
#[path = "sharing_journey_tests.rs"]
mod journey_tests;
