use super::models::CredentialCustodyMode;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(any(test, target_os = "macos"))]
use std::collections::HashMap;
use std::fmt;
#[cfg(test)]
use std::sync::Mutex;
use thiserror::Error;
use zeroize::Zeroize;

#[cfg(target_os = "macos")]
use keyring_core::api::CredentialStoreApi;
#[cfg(target_os = "macos")]
use std::sync::LazyLock;

fn credential_service() -> &'static str {
    crate::app_identity::credential_service(
        "com.alfred.agent-harness",
        "com.alfred.agent-harness.dev",
    )
}

pub struct AgentCredentialEnvelope {
    pub version: u8,
    pub access_token: Option<String>,
    pub runtime_credential_ref: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub provider_fields: BTreeMap<String, String>,
    pub custody_mode: CredentialCustodyMode,
}

impl AgentCredentialEnvelope {
    pub const CURRENT_VERSION: u8 = 1;

    #[allow(dead_code)] // Constructed by provider handlers added in provider plans.
    pub fn alfred_managed(access_token: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            access_token: Some(access_token),
            runtime_credential_ref: None,
            refresh_token: None,
            expires_at: None,
            provider_fields: BTreeMap::new(),
            custody_mode: CredentialCustodyMode::AlfredManaged,
        }
    }

    #[allow(dead_code)] // Constructed by provider handlers added in provider plans.
    pub fn runtime_managed(runtime_credential_ref: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            access_token: None,
            runtime_credential_ref: Some(runtime_credential_ref),
            refresh_token: None,
            expires_at: None,
            provider_fields: BTreeMap::new(),
            custody_mode: CredentialCustodyMode::RuntimeManaged,
        }
    }

    fn is_valid(&self) -> bool {
        if self.version != Self::CURRENT_VERSION {
            return false;
        }
        match self.custody_mode {
            CredentialCustodyMode::AlfredManaged => {
                self.access_token
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                    && self.runtime_credential_ref.is_none()
            }
            CredentialCustodyMode::RuntimeManaged => {
                self.runtime_credential_ref
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                    && self.access_token.is_none()
                    && self.refresh_token.is_none()
            }
        }
    }
}

impl fmt::Debug for AgentCredentialEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentCredentialEnvelope")
            .field("version", &self.version)
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "runtime_credential_ref",
                &self.runtime_credential_ref.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_at", &self.expires_at)
            .field("provider_fields", &"[REDACTED]")
            .field("custody_mode", &self.custody_mode)
            .finish()
    }
}

/// Any accidental diagnostic or command serialization is redacted. The OS
/// store uses the private `StoredAgentCredential` representation below.
impl Serialize for AgentCredentialEnvelope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("AgentCredentialEnvelope", 7)?;
        state.serialize_field("version", &self.version)?;
        state.serialize_field(
            "accessToken",
            &self.access_token.as_ref().map(|_| "[REDACTED]"),
        )?;
        state.serialize_field(
            "runtimeCredentialRef",
            &self.runtime_credential_ref.as_ref().map(|_| "[REDACTED]"),
        )?;
        state.serialize_field(
            "refreshToken",
            &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
        )?;
        state.serialize_field("expiresAt", &self.expires_at)?;
        state.serialize_field("providerFields", "[REDACTED]")?;
        state.serialize_field("custodyMode", &self.custody_mode)?;
        state.end()
    }
}

impl Drop for AgentCredentialEnvelope {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.runtime_credential_ref.zeroize();
        self.refresh_token.zeroize();
        for value in self.provider_fields.values_mut() {
            value.zeroize();
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAgentCredential {
    version: u8,
    access_token: Option<String>,
    runtime_credential_ref: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<String>,
    #[serde(default)]
    provider_fields: BTreeMap<String, String>,
    custody_mode: CredentialCustodyMode,
}

impl StoredAgentCredential {
    fn from_envelope(value: &AgentCredentialEnvelope) -> Self {
        Self {
            version: value.version,
            access_token: value.access_token.clone(),
            runtime_credential_ref: value.runtime_credential_ref.clone(),
            refresh_token: value.refresh_token.clone(),
            expires_at: value.expires_at.clone(),
            provider_fields: value.provider_fields.clone(),
            custody_mode: value.custody_mode,
        }
    }

    fn into_envelope(mut self) -> AgentCredentialEnvelope {
        AgentCredentialEnvelope {
            version: self.version,
            access_token: self.access_token.take(),
            runtime_credential_ref: self.runtime_credential_ref.take(),
            refresh_token: self.refresh_token.take(),
            expires_at: self.expires_at.take(),
            provider_fields: std::mem::take(&mut self.provider_fields),
            custody_mode: self.custody_mode,
        }
    }
}

impl Drop for StoredAgentCredential {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.runtime_credential_ref.zeroize();
        self.refresh_token.zeroize();
        for value in self.provider_fields.values_mut() {
            value.zeroize();
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AgentCredentialStoreError {
    #[error("agent credential was not found")]
    Missing,
    #[error("agent credential storage is locked or unavailable")]
    Locked,
    #[error("agent credential payload is invalid")]
    Invalid,
    #[error("agent credential operation failed")]
    Failed,
}

pub trait AgentCredentialStore: Send + Sync {
    fn put(
        &self,
        credential_ref: &str,
        credential: &AgentCredentialEnvelope,
    ) -> Result<(), AgentCredentialStoreError>;
    fn get(
        &self,
        credential_ref: &str,
    ) -> Result<AgentCredentialEnvelope, AgentCredentialStoreError>;
    fn delete(&self, credential_ref: &str) -> Result<(), AgentCredentialStoreError>;
}

#[derive(Debug, Default)]
pub struct OsAgentCredentialStore;

impl AgentCredentialStore for OsAgentCredentialStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &AgentCredentialEnvelope,
    ) -> Result<(), AgentCredentialStoreError> {
        if !credential.is_valid() {
            return Err(AgentCredentialStoreError::Invalid);
        }
        let stored = StoredAgentCredential::from_envelope(credential);
        let mut payload =
            serde_json::to_vec(&stored).map_err(|_| AgentCredentialStoreError::Invalid)?;
        let result = write_secret(credential_ref, &payload);
        payload.zeroize();
        result
    }

    fn get(
        &self,
        credential_ref: &str,
    ) -> Result<AgentCredentialEnvelope, AgentCredentialStoreError> {
        let mut payload = read_secret(credential_ref)?;
        let result = decode_envelope(&payload);
        payload.zeroize();
        result
    }

    fn delete(&self, credential_ref: &str) -> Result<(), AgentCredentialStoreError> {
        delete_secret(credential_ref)
    }
}

fn decode_envelope(payload: &[u8]) -> Result<AgentCredentialEnvelope, AgentCredentialStoreError> {
    let stored = serde_json::from_slice::<StoredAgentCredential>(payload)
        .map_err(|_| AgentCredentialStoreError::Invalid)?;
    let credential = stored.into_envelope();
    if !credential.is_valid() {
        return Err(AgentCredentialStoreError::Invalid);
    }
    Ok(credential)
}

#[cfg(not(target_os = "macos"))]
fn platform_entry(credential_ref: &str) -> Result<keyring::Entry, AgentCredentialStoreError> {
    keyring::Entry::new(credential_service(), credential_ref).map_err(map_keyring_error)
}

#[cfg(not(target_os = "macos"))]
fn write_secret(credential_ref: &str, payload: &[u8]) -> Result<(), AgentCredentialStoreError> {
    platform_entry(credential_ref)?
        .set_secret(payload)
        .map_err(map_keyring_error)
}

#[cfg(not(target_os = "macos"))]
fn read_secret(credential_ref: &str) -> Result<Vec<u8>, AgentCredentialStoreError> {
    platform_entry(credential_ref)?
        .get_secret()
        .map_err(map_keyring_error)
}

#[cfg(not(target_os = "macos"))]
fn delete_secret(credential_ref: &str) -> Result<(), AgentCredentialStoreError> {
    platform_entry(credential_ref)?
        .delete_credential()
        .map_err(map_keyring_error)
}

#[cfg(target_os = "macos")]
fn protected_store() -> &'static std::sync::Arc<apple_native_keyring_store::protected::Store> {
    static STORE: LazyLock<std::sync::Arc<apple_native_keyring_store::protected::Store>> =
        LazyLock::new(|| {
            apple_native_keyring_store::protected::Store::new().expect("protected keychain store")
        });
    &STORE
}

#[cfg(target_os = "macos")]
fn protected_entry(credential_ref: &str) -> Result<keyring::Entry, AgentCredentialStoreError> {
    let modifiers = HashMap::from([("access-policy", "after-first-unlock-this-device-only")]);
    let inner = protected_store()
        .build(credential_service(), credential_ref, Some(&modifiers))
        .map_err(map_keyring_error)?;
    Ok(keyring::Entry { inner })
}

#[cfg(target_os = "macos")]
fn write_secret(credential_ref: &str, payload: &[u8]) -> Result<(), AgentCredentialStoreError> {
    protected_entry(credential_ref)?
        .set_secret(payload)
        .map_err(map_keyring_error)
}

#[cfg(target_os = "macos")]
fn read_secret(credential_ref: &str) -> Result<Vec<u8>, AgentCredentialStoreError> {
    protected_entry(credential_ref)?
        .get_secret()
        .map_err(map_keyring_error)
}

#[cfg(target_os = "macos")]
fn delete_secret(credential_ref: &str) -> Result<(), AgentCredentialStoreError> {
    protected_entry(credential_ref)?
        .delete_credential()
        .map_err(map_keyring_error)
}

fn map_keyring_error(error: keyring::Error) -> AgentCredentialStoreError {
    match error {
        keyring::Error::NoEntry => AgentCredentialStoreError::Missing,
        keyring::Error::NoStorageAccess(_) | keyring::Error::NoDefaultStore => {
            AgentCredentialStoreError::Locked
        }
        _ => AgentCredentialStoreError::Failed,
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct InMemoryAgentCredentialStore {
    entries: Mutex<HashMap<String, Vec<u8>>>,
    put_failure: Mutex<Option<AgentCredentialStoreError>>,
    put_after_write_failure: Mutex<Option<AgentCredentialStoreError>>,
    delete_failure: Mutex<Option<AgentCredentialStoreError>>,
}

#[cfg(test)]
impl InMemoryAgentCredentialStore {
    pub fn fail_next_put(&self, error: AgentCredentialStoreError) {
        *self.put_failure.lock().expect("put failure lock") = Some(error);
    }

    pub fn fail_next_put_after_write(&self, error: AgentCredentialStoreError) {
        *self
            .put_after_write_failure
            .lock()
            .expect("put after write failure lock") = Some(error);
    }

    pub fn fail_next_delete(&self, error: AgentCredentialStoreError) {
        *self.delete_failure.lock().expect("delete failure lock") = Some(error);
    }

    pub fn entry_count(&self) -> usize {
        self.entries.lock().expect("entries lock").len()
    }
}

#[cfg(test)]
impl AgentCredentialStore for InMemoryAgentCredentialStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &AgentCredentialEnvelope,
    ) -> Result<(), AgentCredentialStoreError> {
        if let Some(error) = self
            .put_failure
            .lock()
            .map_err(|_| AgentCredentialStoreError::Failed)?
            .take()
        {
            return Err(error);
        }
        if !credential.is_valid() {
            return Err(AgentCredentialStoreError::Invalid);
        }
        let stored = StoredAgentCredential::from_envelope(credential);
        let payload =
            serde_json::to_vec(&stored).map_err(|_| AgentCredentialStoreError::Invalid)?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| AgentCredentialStoreError::Failed)?;
        if let Some(mut previous) = entries.insert(credential_ref.into(), payload) {
            previous.zeroize();
        }
        if let Some(error) = self
            .put_after_write_failure
            .lock()
            .map_err(|_| AgentCredentialStoreError::Failed)?
            .take()
        {
            return Err(error);
        }
        Ok(())
    }

    fn get(
        &self,
        credential_ref: &str,
    ) -> Result<AgentCredentialEnvelope, AgentCredentialStoreError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| AgentCredentialStoreError::Failed)?;
        decode_envelope(
            entries
                .get(credential_ref)
                .ok_or(AgentCredentialStoreError::Missing)?,
        )
    }

    fn delete(&self, credential_ref: &str) -> Result<(), AgentCredentialStoreError> {
        if let Some(error) = self
            .delete_failure
            .lock()
            .map_err(|_| AgentCredentialStoreError::Failed)?
            .take()
        {
            return Err(error);
        }
        let mut removed = self
            .entries
            .lock()
            .map_err(|_| AgentCredentialStoreError::Failed)?
            .remove(credential_ref);
        if let Some(payload) = removed.as_mut() {
            payload.zeroize();
            Ok(())
        } else {
            Err(AgentCredentialStoreError::Missing)
        }
    }
}

#[cfg(test)]
impl Drop for InMemoryAgentCredentialStore {
    fn drop(&mut self) {
        if let Ok(entries) = self.entries.get_mut() {
            for payload in entries.values_mut() {
                payload.zeroize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(secret: &str) -> AgentCredentialEnvelope {
        let mut credential = AgentCredentialEnvelope::alfred_managed(secret.into());
        credential.refresh_token = Some(format!("refresh-{secret}"));
        credential
            .provider_fields
            .insert("proof".into(), format!("proof-{secret}"));
        credential
    }

    #[test]
    fn round_trip_overwrite_delete_and_missing() {
        let store = InMemoryAgentCredentialStore::default();
        store.put("account:a", &fixture("first")).expect("put");
        assert_eq!(
            store.get("account:a").expect("get").access_token.as_deref(),
            Some("first")
        );
        store
            .put("account:a", &fixture("second"))
            .expect("overwrite");
        assert_eq!(
            store.get("account:a").expect("get").access_token.as_deref(),
            Some("second")
        );
        store.delete("account:a").expect("delete");
        assert_eq!(
            store.get("account:a").unwrap_err(),
            AgentCredentialStoreError::Missing
        );
    }

    #[test]
    fn malformed_wrong_version_and_custody_mismatch_are_rejected() {
        assert_eq!(
            decode_envelope(b"not-json").unwrap_err(),
            AgentCredentialStoreError::Invalid
        );
        let wrong = serde_json::json!({
            "version": 2,
            "accessToken": "secret",
            "runtimeCredentialRef": null,
            "refreshToken": null,
            "expiresAt": null,
            "providerFields": {},
            "custodyMode": "alfred_managed"
        });
        assert_eq!(
            decode_envelope(&serde_json::to_vec(&wrong).unwrap()).unwrap_err(),
            AgentCredentialStoreError::Invalid
        );
        let invalid = AgentCredentialEnvelope::runtime_managed(String::new());
        assert_eq!(
            InMemoryAgentCredentialStore::default()
                .put("x", &invalid)
                .unwrap_err(),
            AgentCredentialStoreError::Invalid
        );
    }

    #[test]
    fn debug_redacts_every_secret_class() {
        let credential = fixture("access-secret-fixture");
        let output = format!("{credential:?}");
        let serialized = serde_json::to_string(&credential).expect("serialize redacted envelope");
        assert!(output.contains("[REDACTED]"));
        for secret in [
            "access-secret-fixture",
            "refresh-access-secret-fixture",
            "proof-access-secret-fixture",
        ] {
            assert!(!output.contains(secret));
            assert!(!serialized.contains(secret));
        }
        assert!(serialized.contains("[REDACTED]"));
    }
}
