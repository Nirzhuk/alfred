use super::{Db, DbError};
use crate::integrations::catalog::ProviderCatalog;
use crate::integrations::models::{
    AppConnection, AppConnectionUsage, ConnectionStatus, ConnectionUsageItem, UpsertAppConnection,
};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::collections::BTreeMap;
use std::str::FromStr;
use uuid::Uuid;

/// Surfaced when a single-account provider already holds a different account.
/// Disconnecting the existing one is the only way forward.
pub const SINGLE_CONNECTION_MESSAGE: &str =
    "this provider supports one linked account; disconnect the existing one first";

const COLUMNS: &str = "id, provider_id, display_name, external_account_id, \
external_tenant_id, connection_mode, identity_key, scopes_json, provider_metadata_json, \
status, expires_at, last_checked_at, last_error_code, credential_ref, created_at, updated_at";

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn map_connection(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppConnection> {
    let scopes_json: String = row.get(7)?;
    let provider_metadata_json: String = row.get(8)?;
    let status: String = row.get(9)?;
    let scopes = serde_json::from_str(&scopes_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let status = ConnectionStatus::from_str(&status).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            9,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })?;
    let provider_metadata = serde_json::from_str::<BTreeMap<String, String>>(
        &provider_metadata_json,
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(AppConnection {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        display_name: row.get(2)?,
        external_account_id: row.get(3)?,
        external_tenant_id: row.get(4)?,
        connection_mode: row.get(5)?,
        identity_key: row.get(6)?,
        scopes,
        provider_metadata,
        status,
        expires_at: row.get(10)?,
        last_checked_at: row.get(11)?,
        last_error_code: row.get(12)?,
        credential_ref: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

impl Db {
    pub fn list_app_connections(&self) -> Result<Vec<AppConnection>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM app_connections ORDER BY provider_id, created_at"
            ))?;
            let rows = stmt
                .query_map([], map_connection)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_app_connection(&self, id: &str) -> Result<Option<AppConnection>, DbError> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {COLUMNS} FROM app_connections WHERE id = ?1"),
                    params![id],
                    map_connection,
                )
                .optional()?)
        })
    }

    #[allow(dead_code)] // Called by provider plans after they validate external identity.
    pub fn upsert_app_connection(
        &self,
        mut input: UpsertAppConnection,
    ) -> Result<AppConnection, DbError> {
        let catalog = ProviderCatalog::default();
        if !catalog.contains(&input.provider_id) {
            return Err(DbError::Other("unknown connected-app provider".into()));
        }
        let single_connection = catalog.is_single_connection(&input.provider_id);
        if input.provider_id.trim().is_empty()
            || input.connection_mode.trim().is_empty()
            || input.identity_key.trim().is_empty()
            || input.credential_ref.trim().is_empty()
        {
            return Err(DbError::Other(
                "provider, connection mode, identity, and credential reference are required".into(),
            ));
        }
        input.scopes.sort();
        input.scopes.dedup();
        let scopes_json = serde_json::to_string(&input.scopes)
            .map_err(|error| DbError::Other(error.to_string()))?;
        let provider_metadata_json = serde_json::to_string(&input.provider_metadata)
            .map_err(|error| DbError::Other(error.to_string()))?;
        let updated_at = now();

        let id = self.with_conn(|conn| {
            let existing: Option<(String, String)> = conn
                .query_row(
                    "SELECT id, credential_ref FROM app_connections
                     WHERE provider_id = ?1 AND connection_mode = ?2 AND identity_key = ?3",
                    params![input.provider_id, input.connection_mode, input.identity_key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            // Single-account providers (currently the experimental WhatsApp
            // linked device) may relink the same account, but never add a
            // second one. Enforced here rather than only in the connect UI.
            if existing.is_none() && single_connection {
                let other_accounts: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM app_connections
                     WHERE provider_id = ?1 AND identity_key != ?2",
                    params![input.provider_id, input.identity_key],
                    |row| row.get(0),
                )?;
                if other_accounts > 0 {
                    return Err(DbError::Other(SINGLE_CONNECTION_MESSAGE.into()));
                }
            }

            if let Some((id, _credential_ref)) = existing {
                conn.execute(
                    "UPDATE app_connections SET
                       display_name = ?1, external_account_id = ?2, external_tenant_id = ?3,
                       scopes_json = ?4, provider_metadata_json = ?5,
                       status = 'connected', expires_at = ?6,
                       last_checked_at = ?7, last_error_code = NULL,
                       updated_at = ?7
                     WHERE id = ?8",
                    params![
                        input.display_name,
                        input.external_account_id,
                        input.external_tenant_id,
                        scopes_json,
                        provider_metadata_json,
                        input.expires_at,
                        updated_at,
                        id
                    ],
                )?;
                return Ok(id);
            }

            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO app_connections (
                   id, provider_id, display_name, external_account_id, external_tenant_id,
                   connection_mode, identity_key, scopes_json, provider_metadata_json,
                   status, expires_at, last_checked_at, credential_ref, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'connected', ?10, ?11, ?12, ?11, ?11)",
                params![
                    id,
                    input.provider_id,
                    input.display_name,
                    input.external_account_id,
                    input.external_tenant_id,
                    input.connection_mode,
                    input.identity_key,
                    scopes_json,
                    provider_metadata_json,
                    input.expires_at,
                    updated_at,
                    input.credential_ref
                ],
            )?;
            Ok(id)
        })?;

        self.get_app_connection(&id)?
            .ok_or_else(|| DbError::Other("failed to load saved app connection".into()))
    }

    pub fn get_app_connection_by_identity(
        &self,
        provider_id: &str,
        connection_mode: &str,
        identity_key: &str,
    ) -> Result<Option<AppConnection>, DbError> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM app_connections
                         WHERE provider_id = ?1 AND connection_mode = ?2 AND identity_key = ?3"
                    ),
                    params![provider_id, connection_mode, identity_key],
                    map_connection,
                )
                .optional()?)
        })
    }

    pub fn set_app_connection_refresh_state(
        &self,
        id: &str,
        status: ConnectionStatus,
        expires_at: Option<&str>,
        last_error_code: Option<&str>,
    ) -> Result<(), DbError> {
        let checked_at = now();
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE app_connections
                 SET status = ?1, expires_at = ?2, last_checked_at = ?3,
                     last_error_code = ?4, updated_at = ?3
                 WHERE id = ?5 AND status != 'revoked'",
                params![status.as_str(), expires_at, checked_at, last_error_code, id],
            )?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("connection not found or revoked".into()));
        }
        Ok(())
    }

    pub fn mark_app_connection_revoked(&self, id: &str) -> Result<(), DbError> {
        let updated_at = now();
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE app_connections SET status = 'revoked', updated_at = ?1 WHERE id = ?2",
                params![updated_at, id],
            )?)
        })?;
        if changed == 0 {
            return Err(DbError::Other("connection not found".into()));
        }
        Ok(())
    }

    pub fn delete_app_connection_metadata(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM app_connections WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn get_app_connection_usage(&self, id: &str) -> Result<AppConnectionUsage, DbError> {
        let workflows = self.list_workflows()?;
        let referenced_workflows = workflows
            .into_iter()
            .filter(|workflow| app_action_references(&workflow.graph, id, false))
            .map(|workflow| ConnectionUsageItem {
                id: workflow.id,
                label: workflow.name,
                enabled: true,
            })
            .collect::<Vec<_>>();
        let workflow_ids = referenced_workflows
            .iter()
            .map(|workflow| workflow.id.as_str())
            .collect::<std::collections::HashSet<_>>();

        let schedules = self
            .list_all_schedules()?
            .into_iter()
            .filter(|schedule| workflow_ids.contains(schedule.workflow_id.as_str()))
            .map(|schedule| ConnectionUsageItem {
                id: schedule.id,
                label: schedule.workflow_name,
                enabled: schedule.enabled,
            })
            .collect();

        let triggers = self
            .list_all_triggers()?
            .into_iter()
            .filter(|trigger| value_references(&trigger.config, id))
            .map(|trigger| ConnectionUsageItem {
                id: trigger.id,
                label: if trigger.label.is_empty() {
                    trigger.source
                } else {
                    trigger.label
                },
                enabled: trigger.enabled,
            })
            .collect();

        Ok(AppConnectionUsage {
            workflows: referenced_workflows,
            schedules,
            triggers,
        })
    }
}

fn app_action_references(value: &Value, connection_id: &str, inside_app_action: bool) -> bool {
    match value {
        Value::Object(object) => {
            let inside = inside_app_action
                || object
                    .get("type")
                    .or_else(|| object.get("kind"))
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind == "appAction");
            if inside
                && object
                    .get("connectionId")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == connection_id)
            {
                return true;
            }
            object
                .values()
                .any(|child| app_action_references(child, connection_id, inside))
        }
        Value::Array(values) => values
            .iter()
            .any(|child| app_action_references(child, connection_id, inside_app_action)),
        _ => false,
    }
}

fn value_references(value: &Value, connection_id: &str) -> bool {
    match value {
        Value::Object(object) => {
            if object
                .get("connectionId")
                .and_then(Value::as_str)
                .is_some_and(|value| value == connection_id)
            {
                return true;
            }
            object
                .values()
                .any(|child| value_references(child, connection_id))
        }
        Value::Array(values) => values
            .iter()
            .any(|child| value_references(child, connection_id)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CreateWorkflowInput, OnMissed, UpsertScheduleInput, UpsertTriggerInput};
    use crate::integrations::models::canonical_identity_key;

    fn input(tenant: Option<&str>, mode: &str, credential_ref: &str) -> UpsertAppConnection {
        let parts = [tenant.unwrap_or("account"), "user"];
        UpsertAppConnection {
            provider_id: "slack".into(),
            display_name: None,
            external_account_id: Some("user".into()),
            external_tenant_id: tenant.map(str::to_owned),
            connection_mode: mode.into(),
            identity_key: canonical_identity_key("slack", mode, &parts),
            scopes: vec!["write".into(), "read".into(), "read".into()],
            provider_metadata: BTreeMap::new(),
            expires_at: None,
            credential_ref: credential_ref.into(),
        }
    }

    fn whatsapp_input(own_jid: &str, credential_ref: &str) -> UpsertAppConnection {
        use crate::integrations::whatsapp::provider;
        UpsertAppConnection {
            provider_id: provider::PROVIDER_ID.into(),
            display_name: Some(provider::display_name()),
            external_account_id: None,
            external_tenant_id: None,
            connection_mode: provider::CONNECTION_MODE.into(),
            identity_key: provider::identity_key(own_jid),
            scopes: Vec::new(),
            provider_metadata: provider::connection_metadata(
                own_jid,
                "/tmp/whatsapp/protocol.db",
                "2026-08-19",
            ),
            expires_at: None,
            credential_ref: credential_ref.into(),
        }
    }

    #[test]
    fn a_single_account_provider_rejects_a_second_account() {
        let db = Db::open_in_memory().expect("database");
        db.upsert_app_connection(whatsapp_input("34600123456@s.whatsapp.net", "ref-a"))
            .expect("first account links");

        let error = db
            .upsert_app_connection(whatsapp_input("34600999999@s.whatsapp.net", "ref-b"))
            .expect_err("a second WhatsApp account must be refused");
        assert!(error.to_string().contains("one linked account"));
        assert_eq!(db.list_app_connections().expect("list").len(), 1);
    }

    #[test]
    fn a_single_account_provider_still_allows_relinking_the_same_account() {
        let db = Db::open_in_memory().expect("database");
        let first = db
            .upsert_app_connection(whatsapp_input("34600123456@s.whatsapp.net", "ref-a"))
            .expect("first link");
        let relinked = db
            .upsert_app_connection(whatsapp_input("34600123456@s.whatsapp.net", "ref-a"))
            .expect("relinking the same account must succeed");

        assert_eq!(first.id, relinked.id);
        assert_eq!(db.list_app_connections().expect("list").len(), 1);
    }

    #[test]
    fn the_single_account_rule_does_not_constrain_other_providers() {
        let db = Db::open_in_memory().expect("database");
        db.upsert_app_connection(input(Some("tenant-a"), "native_oauth", "ref-a"))
            .expect("first Slack workspace");
        db.upsert_app_connection(input(Some("tenant-b"), "native_oauth", "ref-b"))
            .expect("a second Slack workspace is allowed");

        assert_eq!(db.list_app_connections().expect("list").len(), 2);
    }

    #[test]
    fn a_whatsapp_connection_stores_no_raw_identity() {
        let db = Db::open_in_memory().expect("database");
        let own_jid = "34600123456@s.whatsapp.net";
        let connection = db
            .upsert_app_connection(whatsapp_input(own_jid, "ref-a"))
            .expect("link");

        // Everything the row carries, serialized together.
        let serialized = format!(
            "{:?}{:?}{:?}{:?}{:?}",
            connection.display_name,
            connection.external_account_id,
            connection.external_tenant_id,
            connection.identity_key,
            connection.provider_metadata
        );
        assert!(
            !serialized.contains("34600123456"),
            "the connection row must never hold a full phone number or JID"
        );
        assert!(serialized.contains("***56@s.whatsapp.net"));
    }

    #[test]
    fn reconnect_upgrades_existing_metadata_and_preserves_id() {
        let db = Db::open_in_memory().expect("database");
        let first = db
            .upsert_app_connection(input(Some("tenant-a"), "native_oauth", "first-ref"))
            .expect("insert");
        let mut reconnect = input(Some("tenant-a"), "native_oauth", "rotated-ref");
        reconnect.display_name = Some("Workspace A".into());
        reconnect.scopes.push("admin".into());
        let second = db.upsert_app_connection(reconnect).expect("reconnect");

        assert_eq!(first.id, second.id);
        assert_eq!(second.credential_ref, "first-ref");
        assert_eq!(second.display_name.as_deref(), Some("Workspace A"));
        assert_eq!(second.scopes, vec!["admin", "read", "write"]);
        assert_eq!(db.list_app_connections().expect("list").len(), 1);
    }

    #[test]
    fn identity_separates_tenants_modes_and_nullable_display_metadata() {
        let db = Db::open_in_memory().expect("database");
        let a = db
            .upsert_app_connection(input(Some("tenant-a"), "native_oauth", "ref-a"))
            .expect("tenant a");
        let b = db
            .upsert_app_connection(input(Some("tenant-b"), "native_oauth", "ref-b"))
            .expect("tenant b");
        let webhook = db
            .upsert_app_connection(input(Some("tenant-a"), "incoming_webhook", "ref-c"))
            .expect("webhook");
        let mut other_account = input(Some("tenant-a"), "native_oauth", "ref-d");
        other_account.external_account_id = Some("different-user".into());
        other_account.identity_key =
            canonical_identity_key("slack", "native_oauth", &["tenant-a", "different-user"]);
        let other_account = db
            .upsert_app_connection(other_account)
            .expect("different account");

        assert_ne!(a.id, b.id);
        assert_ne!(a.id, webhook.id);
        assert_ne!(a.id, other_account.id);
        assert!(a.display_name.is_none());
        assert_eq!(db.list_app_connections().expect("list").len(), 4);
    }

    #[test]
    fn usage_finds_direct_workflow_trigger_and_transitive_schedule() {
        let db = Db::open_in_memory().expect("database");
        let workflow = db
            .create_workflow(CreateWorkflowInput {
                name: "Notify team".into(),
                description: String::new(),
                working_directory: String::new(),
                folder_id: None,
                graph: serde_json::json!({
                    "nodes": [{"type": "appAction", "data": {"connectionId": "connection-a"}}],
                    "edges": []
                }),
            })
            .expect("workflow");
        db.upsert_schedule(
            UpsertScheduleInput {
                workflow_id: workflow.id.clone(),
                cron: "0 * * * * *".into(),
                enabled: false,
                on_missed: OnMissed::Run,
            },
            None,
        )
        .expect("schedule");
        db.upsert_trigger(UpsertTriggerInput {
            id: None,
            workflow_id: workflow.id,
            source: "app_event".into(),
            label: "Slack event".into(),
            config: serde_json::json!({"connectionId": "connection-a"}),
            enabled: true,
        })
        .expect("trigger");

        let usage = db.get_app_connection_usage("connection-a").expect("usage");
        assert_eq!(usage.workflows.len(), 1);
        assert_eq!(usage.schedules.len(), 1);
        assert!(!usage.schedules[0].enabled);
        assert_eq!(usage.triggers.len(), 1);
    }

    #[test]
    fn rejects_provider_ids_outside_the_rust_catalog() {
        let db = Db::open_in_memory().expect("database");
        let mut unknown = input(None, "native_oauth", "unknown-ref");
        unknown.provider_id = "unregistered".into();
        unknown.identity_key = canonical_identity_key("unregistered", "native_oauth", &["account"]);
        assert!(db.upsert_app_connection(unknown).is_err());
    }
}
