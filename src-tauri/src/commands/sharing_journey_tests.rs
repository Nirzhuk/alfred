use super::*;
use crate::agents::models::ProviderModels;
use crate::db::{CreateWorkflowInput, UpsertTriggerInput};
use crate::integrations::actions::*;
use crate::integrations::events::*;
use crate::integrations::models::{AppConnection, UpsertAppConnection};
use crate::integrations::token_store::{CredentialEnvelope, InMemoryTokenStore, TokenStore};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FixtureProvider {
    resources: Mutex<HashMap<String, String>>,
    calls: Mutex<Vec<(String, String)>>,
    execution_enabled: AtomicBool,
    executions: Mutex<Vec<ValidatedActionRequest>>,
}
impl FixtureProvider {
    fn page(
        &self,
        connection: &AppConnection,
        field: &str,
        token: Option<&str>,
    ) -> (Vec<String>, Option<String>) {
        self.calls
            .lock()
            .unwrap()
            .push((connection.id.clone(), field.into()));
        if token.is_none() {
            return (vec!["C-unrelated".into()], Some("next".into()));
        }
        (
            self.resources
                .lock()
                .unwrap()
                .get(&connection.id)
                .cloned()
                .into_iter()
                .collect(),
            None,
        )
    }
}
impl ActionExecutor for FixtureProvider {
    fn execute<'a>(
        &'a self,
        request: &'a ValidatedActionRequest,
        connection: &'a AppConnection,
        _: TokenAccessCapability,
        _: ActionCancellation,
    ) -> ActionFuture<'a> {
        assert!(
            self.execution_enabled.load(Ordering::SeqCst),
            "sharing setup must never execute an action"
        );
        Box::pin(async move {
            assert_eq!(request.connection_id, connection.id);
            assert_eq!(request.provider_id, "slack");
            assert_eq!(request.action_id, "slack.send_message");
            self.executions.lock().unwrap().push(request.clone());
            Ok(ActionResult {
                summary: "Fixture message delivered".into(),
                output: json!({"delivered": true}),
                artifacts: vec![],
                provider_request_id: Some("synthetic-delivery".into()),
            })
        })
    }
    fn list_resources<'a>(
        &'a self,
        _: &'a str,
        field: &'a str,
        query: &'a str,
        token: Option<&'a str>,
        connection: &'a AppConnection,
        _: TokenAccessCapability,
        _: ActionCancellation,
    ) -> ActionResourcesFuture<'a> {
        Box::pin(async move {
            assert_eq!(query, "");
            let (ids, next_page_token) = self.page(connection, field, token);
            Ok(ActionResourcePage {
                items: ids
                    .into_iter()
                    .map(|id| ActionResourceItem {
                        label: id.clone(),
                        id,
                    })
                    .collect(),
                next_page_token,
            })
        })
    }
}
impl AppEventAdapter for FixtureProvider {
    fn poll<'a>(
        &'a self,
        _: &'a AppTriggerConfig,
        _: &'a AppConnection,
        _: Option<&'a str>,
        _: TokenAccessCapability,
        _: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventBatch> {
        panic!("sharing setup must never poll or activate a trigger")
    }
    fn list_filter_resources<'a>(
        &'a self,
        field: &'a str,
        query: &'a str,
        token: Option<&'a str>,
        connection: &'a AppConnection,
        _: TokenAccessCapability,
        _: AppEventCancellation,
    ) -> AppEventFuture<'a, AppEventResourcePage> {
        Box::pin(async move {
            assert_eq!(query, "");
            let (ids, next_page_token) = self.page(connection, field, token);
            Ok(AppEventResourcePage {
                items: ids
                    .into_iter()
                    .map(|id| AppEventResourceItem {
                        label: id.clone(),
                        id,
                    })
                    .collect(),
                next_page_token,
            })
        })
    }
}

fn catalog(harness: Option<AgentHarness>) -> Vec<ProviderModels> {
    if harness != Some(AgentHarness::Cli) {
        return vec![];
    }
    vec![serde_json::from_value(json!({
        "provider": "claude_code", "harness": "cli", "requiresAccount": false,
        "supportsOAuth": false, "supportsApiKey": false, "supportsUsage": false,
        "accountConnected": false, "nativeRuntimeAvailable": false,
        "defaultModel": "sonnet", "models": [{"id": "sonnet", "label": "Sonnet"}],
        "allowCustom": false, "source": "fixture", "available": true
    }))
    .unwrap()]
}

struct Fixture {
    db: Db,
    integrations: IntegrationsState,
    provider: Arc<FixtureProvider>,
    connections: [String; 2],
    folder: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}
fn fixture(name: &str) -> Fixture {
    let db = Db::open_in_memory().unwrap();
    let store = Arc::new(InMemoryTokenStore::default());
    let mut integrations = IntegrationsState::new(store.clone());
    let mut action = integrations
        .actions
        .descriptor("slack", "slack.send_message")
        .unwrap();
    action.fields.push(ActionFieldDescriptor {
        key: "visibility".into(),
        label: "Visibility".into(),
        description: String::new(),
        kind: ActionFieldKind::Enum,
        required: false,
        default: None,
        secret: false,
        option_source: None,
        options: vec![ActionOption {
            id: "public".into(),
            label: "Public".into(),
        }],
        supports_interpolation: false,
    });
    let event = integrations
        .events
        .descriptor("slack", "slack.app_mention")
        .unwrap();
    integrations.actions = ActionRegistry::default();
    integrations.events = AppEventRegistry::default();
    let provider = Arc::new(FixtureProvider::default());
    integrations
        .actions
        .register(action, ActionLimits::default(), provider.clone())
        .unwrap();
    integrations
        .events
        .register(event, provider.clone())
        .unwrap();
    let connections = [0, 1].map(|index| {
        let credential_ref = format!("fixture-{name}-{index}");
        store
            .put(
                &credential_ref,
                &CredentialEnvelope::new("synthetic-fixture-token".into()),
            )
            .unwrap();
        let connection = db
            .upsert_app_connection(UpsertAppConnection {
                provider_id: "slack".into(),
                display_name: Some(format!("{name}-{index}")),
                external_account_id: None,
                external_tenant_id: None,
                connection_mode: "bot".into(),
                identity_key: format!("{name}-{index}"),
                scopes: vec![
                    "chat:write".into(),
                    "channels:read".into(),
                    "app_mentions:read".into(),
                    "connections:write".into(),
                ],
                provider_metadata: BTreeMap::new(),
                expires_at: None,
                credential_ref,
            })
            .unwrap();
        provider
            .resources
            .lock()
            .unwrap()
            .insert(connection.id.clone(), format!("C-{name}"));
        connection.id
    });
    let folder =
        std::env::temp_dir().join(format!("alfred-sharing-journey-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    Fixture {
        db,
        integrations,
        provider,
        connections,
        folder,
    }
}

fn source_draft(source: &Fixture) -> PublicationDraft {
    let workflow = source.db.create_workflow(CreateWorkflowInput {
        name: "Notes to Slack".into(), description: "Review notes and prepare two messages".into(),
        working_directory: source.folder.to_string_lossy().into(), folder_id: None,
        graph: json!({"nodes": [
            {"id":"notes","type":"input","data":{"label":"Notes","prompt":"Summarize selected notes","attachments":[{"id":"folder","path":source.folder,"kind":"folder"}]},"position":{"x":0,"y":0}},
            {"id":"agent","type":"agent","data":{"label":"CLI summary","provider":"claude_code","harness":"cli","model":"sonnet","skillNames":[]},"position":{"x":200,"y":0}},
            {"id":"send-a","type":"appAction","data":{"label":"Team update","providerId":"slack","actionId":"slack.send_message","connectionId":source.connections[0],"input":{"conversation":"C-source","text":"{{output}}"}},"position":{"x":400,"y":0}},
            {"id":"send-b","type":"appAction","data":{"label":"Partner update","providerId":"slack","actionId":"slack.send_message","connectionId":source.connections[1],"input":{"conversation":"C-source","text":"{{output}}"}},"position":{"x":600,"y":0}}
        ],"edges":[{"id":"e1","source":"notes","target":"agent"},{"id":"e2","source":"agent","target":"send-a"},{"id":"e3","source":"send-a","target":"send-b"}]}),
    }).unwrap();
    source
        .db
        .upsert_trigger(UpsertTriggerInput {
            id: None,
            workflow_id: workflow.id.clone(),
            source: "app".into(),
            label: "Team mention".into(),
            enabled: true,
            config: serde_json::to_value(AppTriggerConfig {
                provider_id: "slack".into(),
                event_type: "slack.app_mention".into(),
                connection_id: source.connections[0].clone(),
                filters: BTreeMap::from([("channelId".into(), json!("C-source"))]),
                descriptor_version: 1,
            })
            .unwrap(),
        })
        .unwrap();
    let mut draft =
        prepare_publication_from_db(&source.db, &source.integrations, &workflow.id).unwrap();
    // The public contract permits one resource question used by independent
    // connection roles. Group it explicitly, then verify each target at import.
    let resource_b = draft
        .template
        .parameters
        .iter()
        .position(|parameter| {
            parameter.kind == ParameterKind::AppResource
                && parameter.target.node_id.as_deref() == Some("send-b")
        })
        .unwrap();
    let secondary = draft.template.parameters.remove(resource_b);
    let resource_a = draft
        .template
        .parameters
        .iter_mut()
        .find(|parameter| {
            parameter.kind == ParameterKind::AppResource
                && parameter.target.node_id.as_deref() == Some("send-a")
        })
        .unwrap();
    resource_a.targets.push(secondary.target);
    crate::templates::validate_template(&draft.template).unwrap();
    draft
}
fn input_for(
    draft: &PublicationDraft,
    recipient: &Fixture,
    attempt: &str,
    app_alias: bool,
) -> ImportInput {
    let values = draft
        .template
        .parameters
        .iter()
        .map(|parameter| {
            let value = match parameter.kind {
                ParameterKind::Folder => json!(recipient.folder),
                ParameterKind::AppConnection => json!(
                    recipient.connections
                        [usize::from(parameter.target.node_id.as_deref() == Some("send-b"))]
                ),
                ParameterKind::AppResource => json!(recipient
                    .provider
                    .resources
                    .lock()
                    .unwrap()
                    .get(&recipient.connections[0])
                    .unwrap()),
                _ => panic!("unexpected setup parameter {}", parameter.key),
            };
            (parameter.key.clone(), value)
        })
        .collect();
    let id = "a".repeat(32);
    ImportInput {
        template: draft.template.clone(),
        values,
        source_link: if app_alias {
            format!("alfred-workflows://import/{id}")
        } else {
            format!("https://share.example/s/{id}")
        },
        digest: crate::templates::digest(&draft.template).unwrap(),
        import_attempt_id: attempt.into(),
    }
}

#[tokio::test]
async fn production_prepare_setup_import_round_trips_two_profiles_and_saved_event() {
    let source = fixture("source");
    let a = fixture("recipient-a");
    let b = fixture("recipient-b");
    let draft = source_draft(&source);
    let serialized = serde_json::to_string(&draft.template).unwrap();
    assert!(!serialized.contains(&source.connections[0]));
    assert!(!serialized.contains(source.folder.to_str().unwrap()));
    assert!(!serialized.contains("descriptorVersion"));
    assert_eq!(
        draft
            .template
            .parameters
            .iter()
            .filter(|parameter| parameter.kind == ParameterKind::AppConnection)
            .count(),
        2
    );
    let origin = TrustedShareOrigin::parse("https://share.example/", false).unwrap();
    let first = import_checked(
        &a.db,
        &a.integrations,
        &origin,
        input_for(&draft, &a, "attempt-a", false),
        catalog,
    )
    .await
    .unwrap();
    let second = import_checked(
        &b.db,
        &b.integrations,
        &origin,
        input_for(&draft, &b, "attempt-b", false),
        catalog,
    )
    .await
    .unwrap();
    assert_ne!(first["workflowId"], second["workflowId"]);
    assert_ne!(first["workflow"]["graph"], second["workflow"]["graph"]);
    let retry = import_checked(
        &a.db,
        &a.integrations,
        &origin,
        input_for(&draft, &a, "attempt-a", true),
        catalog,
    )
    .await
    .unwrap();
    assert_eq!(retry["workflowId"], first["workflowId"]);
    assert_eq!(retry["existingCopy"], true);
    for (recipient, result) in [(&a, first), (&b, second)] {
        let workflow_id = result["workflowId"].as_str().unwrap();
        assert_eq!(
            result["workflow"]["workingDirectory"],
            recipient.folder.to_str().unwrap()
        );
        assert_eq!(result["workflow"]["memoryRetrievalEnabled"], false);
        let nodes = result["workflow"]["graph"]["nodes"].as_array().unwrap();
        for (label, connection) in [
            ("Team update", &recipient.connections[0]),
            ("Partner update", &recipient.connections[1]),
        ] {
            let node = nodes
                .iter()
                .find(|node| node.pointer("/data/label").and_then(Value::as_str) == Some(label))
                .unwrap();
            assert_eq!(node["data"]["connectionId"], *connection);
            assert_eq!(
                node["data"]["input"]["conversation"],
                *recipient
                    .provider
                    .resources
                    .lock()
                    .unwrap()
                    .get(connection)
                    .unwrap()
            );
        }

        let triggers = recipient.db.list_triggers(workflow_id).unwrap();
        assert_eq!(triggers.len(), 1);
        assert!(!triggers[0].enabled);
        let config: AppTriggerConfig = serde_json::from_value(triggers[0].config.clone()).unwrap();
        recipient
            .integrations
            .validate_app_trigger(&recipient.db, &config)
            .unwrap();
        assert_eq!(config.connection_id, recipient.connections[0]);
        assert_eq!(config.descriptor_version, 1);
        let calls = recipient.provider.calls.lock().unwrap();
        for (connection, field) in [
            (&recipient.connections[0], "conversation"),
            (&recipient.connections[1], "conversation"),
            (&recipient.connections[0], "channelId"),
        ] {
            assert!(
                calls.contains(&(connection.clone(), field.into())),
                "missing actual provider read"
            );
        }
    }
}

#[tokio::test]
async fn imported_action_executes_with_recipient_bindings_and_fixture_summary() {
    let source = fixture("source");
    let recipient = fixture("recipient");
    let draft = source_draft(&source);
    let origin = TrustedShareOrigin::parse("https://share.example/", false).unwrap();
    let imported = import_checked(
        &recipient.db,
        &recipient.integrations,
        &origin,
        input_for(&draft, &recipient, "fixture-execution", false),
        catalog,
    )
    .await
    .unwrap();
    assert!(recipient.provider.executions.lock().unwrap().is_empty());
    let workflow = recipient
        .db
        .get_workflow(imported["workflowId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let data = &workflow.graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["data"]["label"] == "Team update")
        .unwrap()["data"];
    let descriptor = recipient
        .integrations
        .actions
        .descriptor("slack", "slack.send_message")
        .unwrap();
    // The summary is injected. This exercises the production action boundary
    // after import; it does not claim a native runner or real agent execution.
    let summary = "Fixture summary: reviewed notes from the recipient folder.";
    let request = crate::runner::prepare_action_request(
        data,
        &descriptor,
        "Summarize selected notes",
        summary,
        &workflow.working_directory,
    )
    .unwrap();
    recipient
        .provider
        .execution_enabled
        .store(true, Ordering::SeqCst);
    let result = recipient
        .integrations
        .execute_action(&recipient.db, request, ActionCancellation::never())
        .await
        .unwrap();
    assert_eq!(result.output, json!({"delivered": true}));
    let executions = recipient.provider.executions.lock().unwrap();
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].connection_id, recipient.connections[0]);
    assert_eq!(executions[0].input["conversation"], "C-recipient");
    assert_eq!(executions[0].input["text"], summary);
    assert!(!recipient
        .db
        .list_triggers(&workflow.id)
        .unwrap()
        .iter()
        .any(|trigger| trigger.enabled));
}

#[tokio::test]
async fn commit_rechecks_secondary_resource_after_picker_cache_and_rejects_kind_spoofing() {
    let source = fixture("source");
    let recipient = fixture("recipient");
    let draft = source_draft(&source);
    let origin = TrustedShareOrigin::parse("https://share.example/", false).unwrap();
    let second_connection = &recipient.connections[1];
    for token in [None, Some("next")] {
        recipient
            .integrations
            .list_action_resources(
                &recipient.db,
                second_connection,
                "slack",
                "slack.send_message",
                "conversation",
                "",
                token,
            )
            .await
            .unwrap();
    }
    recipient
        .provider
        .resources
        .lock()
        .unwrap()
        .remove(second_connection);
    let error = import_checked(
        &recipient.db,
        &recipient.integrations,
        &origin,
        input_for(&draft, &recipient, "revoked-resource", false),
        catalog,
    )
    .await
    .unwrap_err();
    assert!(error.contains("send-b"), "{error}");
    assert_eq!(recipient.db.list_workflows().unwrap().len(), 0);

    let mut spoofed = input_for(&draft, &recipient, "spoofed-kind", false);
    let parameter = spoofed
        .template
        .parameters
        .iter_mut()
        .find(|parameter| {
            parameter.kind == ParameterKind::AppResource && parameter.target.node_id.is_some()
        })
        .unwrap();
    parameter.kind = ParameterKind::Text;
    parameter.allowed_action = None;
    spoofed.digest = crate::templates::digest(&spoofed.template).unwrap();
    let error = import_checked(
        &recipient.db,
        &recipient.integrations,
        &origin,
        spoofed,
        catalog,
    )
    .await
    .unwrap_err();
    assert!(error.contains("setup type"), "{error}");
    assert_eq!(recipient.db.list_workflows().unwrap().len(), 0);
}

#[tokio::test]
async fn actual_setup_rejects_invalid_enum_oversized_text_and_unknown_event_filter() {
    let source = fixture("source");
    let recipient = fixture("recipient");
    let draft = source_draft(&source);
    let origin = TrustedShareOrigin::parse("https://share.example/", false).unwrap();
    for kind in ["enum", "text", "filter"] {
        let mut input = input_for(&draft, &recipient, kind, false);
        match kind {
            "enum" => {
                input
                    .template
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == "send-a")
                    .unwrap()
                    .data
                    .get_mut("input")
                    .unwrap()["visibility"] = json!("unregistered-choice")
            }
            "text" => {
                input
                    .template
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == "send-a")
                    .unwrap()
                    .data
                    .get_mut("input")
                    .unwrap()["text"] = json!("x".repeat(32 * 1024 + 1))
            }
            _ => {
                input.template.starts[0].config.get_mut("filters").unwrap()["action"] =
                    json!("created")
            }
        }
        input.digest = crate::templates::digest(&input.template).unwrap();
        let error = import_checked(
            &recipient.db,
            &recipient.integrations,
            &origin,
            input,
            catalog,
        )
        .await
        .unwrap_err();
        assert!(error.contains("not ready"), "{kind}: {error}");
        assert_eq!(recipient.db.list_workflows().unwrap().len(), 0);
    }
    assert!(recipient.provider.calls.lock().unwrap().is_empty());
}

#[test]
fn publisher_email_recipients_are_setup_fields_until_explicitly_retained() {
    let integrations = IntegrationsState::new(Arc::new(InMemoryTokenStore::default()));
    let graph = json!({"nodes": [{"id":"mail","type":"appAction","position":{"x":0,"y":0},"data":{
        "providerId":"gmail","actionId":"gmail.send_email","connectionId":"publisher-gmail",
        "input":{"to":"private@example.test","cc":"colleague@example.test","subject":"Release update","body":"Reviewed public example"}
    }}],"edges":[]});
    let draft =
        build_publication_draft("Email update", "", "", &graph, &[], &integrations).unwrap();
    let serialized = serde_json::to_string(&draft.template).unwrap();
    assert!(!serialized.contains("private@example.test"));
    assert!(!serialized.contains("colleague@example.test"));
    assert_eq!(
        draft
            .template
            .parameters
            .iter()
            .filter(|parameter| parameter.kind == ParameterKind::Email)
            .count(),
        2
    );
    assert!(validate_action_descriptor_inputs(&draft.template, &integrations).is_empty());
    let mut retained = draft.template;
    retained
        .parameters
        .retain(|parameter| parameter.target.field != "input.to");
    retained.graph.nodes[0].data.get_mut("input").unwrap()["to"] = json!("public@example.test");
    crate::templates::validate_template(&retained).unwrap();
    assert!(validate_action_descriptor_inputs(&retained, &integrations).is_empty());
}
