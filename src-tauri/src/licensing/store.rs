use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use zeroize::Zeroize;

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

fn credential_service() -> &'static str {
    crate::app_identity::credential_service("com.alfred.licensing", "com.alfred.licensing.dev")
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseCredentialEnvelope {
    pub version: u8,
    pub license_key: String,
    /// `None` means the Polar benefit does not use device activations. The
    /// license key is still validated online, but there is no device instance
    /// to deactivate or bind to this installation.
    #[serde(default)]
    pub activation_id: Option<String>,
}

impl LicenseCredentialEnvelope {
    const CURRENT_VERSION: u8 = 1;

    pub fn new(license_key: String, activation_id: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            license_key,
            activation_id: Some(activation_id),
        }
    }

    pub fn without_activation(license_key: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            license_key,
            activation_id: None,
        }
    }
}

impl fmt::Debug for LicenseCredentialEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LicenseCredentialEnvelope")
            .field("version", &self.version)
            .field("license_key", &"[REDACTED]")
            .field("activation_id", &"[REDACTED]")
            .finish()
    }
}

impl Drop for LicenseCredentialEnvelope {
    fn drop(&mut self) {
        self.license_key.zeroize();
        if let Some(activation_id) = &mut self.activation_id {
            activation_id.zeroize();
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum LicenseStoreError {
    #[error("license credential was not found")]
    Missing,
    #[error("license credential storage is locked or unavailable")]
    Locked,
    #[error("license credential payload is invalid")]
    Invalid,
    #[error("license credential operation failed")]
    Failed,
}

pub trait LicenseCredentialStore: Send + Sync {
    fn put(
        &self,
        credential_ref: &str,
        credential: &LicenseCredentialEnvelope,
    ) -> Result<(), LicenseStoreError>;
    fn get(&self, credential_ref: &str) -> Result<LicenseCredentialEnvelope, LicenseStoreError>;
    fn delete(&self, credential_ref: &str) -> Result<(), LicenseStoreError>;
}

#[derive(Debug, Default)]
pub struct OsLicenseCredentialStore;

impl OsLicenseCredentialStore {
    fn entry(credential_ref: &str) -> Result<keyring::Entry, LicenseStoreError> {
        keyring::Entry::new(credential_service(), credential_ref).map_err(map_keyring_error)
    }
}

impl LicenseCredentialStore for OsLicenseCredentialStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &LicenseCredentialEnvelope,
    ) -> Result<(), LicenseStoreError> {
        let mut payload = serde_json::to_vec(credential).map_err(|_| LicenseStoreError::Invalid)?;
        let result = match Self::entry(credential_ref) {
            Ok(entry) => entry.set_secret(&payload).map_err(map_keyring_error),
            Err(error) => Err(error),
        };
        payload.zeroize();
        result
    }

    fn get(&self, credential_ref: &str) -> Result<LicenseCredentialEnvelope, LicenseStoreError> {
        let mut payload = Self::entry(credential_ref)?
            .get_secret()
            .map_err(map_keyring_error)?;
        let result = serde_json::from_slice::<LicenseCredentialEnvelope>(&payload)
            .map_err(|_| LicenseStoreError::Invalid);
        payload.zeroize();
        let credential = result?;
        if credential.version != LicenseCredentialEnvelope::CURRENT_VERSION {
            return Err(LicenseStoreError::Invalid);
        }
        Ok(credential)
    }

    fn delete(&self, credential_ref: &str) -> Result<(), LicenseStoreError> {
        Self::entry(credential_ref)?
            .delete_credential()
            .map_err(map_keyring_error)
    }
}

fn map_keyring_error(error: keyring::Error) -> LicenseStoreError {
    match error {
        keyring::Error::NoEntry => LicenseStoreError::Missing,
        keyring::Error::NoStorageAccess(_) | keyring::Error::NoDefaultStore => {
            LicenseStoreError::Locked
        }
        _ => LicenseStoreError::Failed,
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct InMemoryLicenseCredentialStore {
    entries: Mutex<HashMap<String, Vec<u8>>>,
}

#[cfg(test)]
impl LicenseCredentialStore for InMemoryLicenseCredentialStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &LicenseCredentialEnvelope,
    ) -> Result<(), LicenseStoreError> {
        let payload = serde_json::to_vec(credential).map_err(|_| LicenseStoreError::Invalid)?;
        let mut entries = self.entries.lock().map_err(|_| LicenseStoreError::Failed)?;
        if let Some(mut old_payload) = entries.insert(credential_ref.to_owned(), payload) {
            old_payload.zeroize();
        }
        Ok(())
    }

    fn get(&self, credential_ref: &str) -> Result<LicenseCredentialEnvelope, LicenseStoreError> {
        let entries = self.entries.lock().map_err(|_| LicenseStoreError::Failed)?;
        let payload = entries
            .get(credential_ref)
            .ok_or(LicenseStoreError::Missing)?;
        let credential: LicenseCredentialEnvelope =
            serde_json::from_slice(payload).map_err(|_| LicenseStoreError::Invalid)?;
        if credential.version != LicenseCredentialEnvelope::CURRENT_VERSION {
            return Err(LicenseStoreError::Invalid);
        }
        Ok(credential)
    }

    fn delete(&self, credential_ref: &str) -> Result<(), LicenseStoreError> {
        let mut removed = self
            .entries
            .lock()
            .map_err(|_| LicenseStoreError::Failed)?
            .remove(credential_ref);
        if let Some(payload) = removed.as_mut() {
            payload.zeroize();
            Ok(())
        } else {
            Err(LicenseStoreError::Missing)
        }
    }
}

#[cfg(test)]
impl Drop for InMemoryLicenseCredentialStore {
    fn drop(&mut self) {
        if let Ok(entries) = self.entries.get_mut() {
            for payload in entries.values_mut() {
                payload.zeroize();
            }
        }
    }
}

#[cfg(test)]
impl InMemoryLicenseCredentialStore {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries
            .lock()
            .map(|entries| entries.is_empty())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_store_round_trip_overwrite_delete_and_missing() {
        let store = InMemoryLicenseCredentialStore::default();
        store
            .put(
                "opaque",
                &LicenseCredentialEnvelope::new("first-key".into(), "first-id".into()),
            )
            .expect("put");
        assert_eq!(store.get("opaque").expect("get").license_key, "first-key");

        store
            .put(
                "opaque",
                &LicenseCredentialEnvelope::new("second-key".into(), "second-id".into()),
            )
            .expect("overwrite");
        let stored = store.get("opaque").expect("get overwritten");
        assert_eq!(stored.license_key, "second-key");
        assert_eq!(stored.activation_id.as_deref(), Some("second-id"));

        store
            .put(
                "unbound",
                &LicenseCredentialEnvelope::without_activation("third-key".into()),
            )
            .expect("put unbound");
        assert_eq!(
            store.get("unbound").expect("get unbound").activation_id,
            None
        );

        store.delete("opaque").expect("delete");
        assert_eq!(store.get("opaque").unwrap_err(), LicenseStoreError::Missing);
    }

    #[test]
    fn envelope_debug_output_redacts_both_secrets() {
        let envelope = LicenseCredentialEnvelope::new(
            "license-key-secret".into(),
            "activation-id-secret".into(),
        );
        let output = format!("{envelope:?}");
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("license-key-secret"));
        assert!(!output.contains("activation-id-secret"));
    }
}
