use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Mutex;
use thiserror::Error;
use zeroize::Zeroize;

#[cfg(target_os = "macos")]
use keyring_core::api::CredentialStoreApi;
#[cfg(target_os = "macos")]
use std::sync::LazyLock;

fn credential_service() -> &'static str {
    crate::app_identity::credential_service(
        "com.alfred.connected-apps",
        "com.alfred.connected-apps.dev",
    )
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEnvelope {
    pub version: u8,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub provider_fields: BTreeMap<String, String>,
}

impl CredentialEnvelope {
    pub const CURRENT_VERSION: u8 = 1;

    pub fn new(access_token: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            access_token,
            refresh_token: None,
            expires_at: None,
            provider_fields: BTreeMap::new(),
        }
    }
}

impl fmt::Debug for CredentialEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialEnvelope")
            .field("version", &self.version)
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_at", &self.expires_at)
            .field("provider_fields", &"[REDACTED]")
            .finish()
    }
}

impl Drop for CredentialEnvelope {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        for value in self.provider_fields.values_mut() {
            value.zeroize();
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum TokenStoreError {
    #[error("credential was not found")]
    Missing,
    #[error("credential storage is locked or unavailable")]
    Locked,
    #[error("credential payload is invalid")]
    Invalid,
    #[error("credential operation failed")]
    Failed,
}

pub trait TokenStore: Send + Sync {
    fn put(
        &self,
        credential_ref: &str,
        credential: &CredentialEnvelope,
    ) -> Result<(), TokenStoreError>;
    fn get(&self, credential_ref: &str) -> Result<CredentialEnvelope, TokenStoreError>;
    fn delete(&self, credential_ref: &str) -> Result<(), TokenStoreError>;
}

#[derive(Debug, Default)]
pub struct OsTokenStore;

impl TokenStore for OsTokenStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &CredentialEnvelope,
    ) -> Result<(), TokenStoreError> {
        let mut payload = serde_json::to_vec(credential).map_err(|_| TokenStoreError::Invalid)?;
        let result = write_secret(credential_ref, &payload);
        payload.zeroize();
        result
    }

    fn get(&self, credential_ref: &str) -> Result<CredentialEnvelope, TokenStoreError> {
        let mut payload = read_secret(credential_ref)?;
        let result = decode_envelope(&payload);
        payload.zeroize();
        result
    }

    fn delete(&self, credential_ref: &str) -> Result<(), TokenStoreError> {
        delete_secret(credential_ref)
    }
}

fn decode_envelope(payload: &[u8]) -> Result<CredentialEnvelope, TokenStoreError> {
    let credential = serde_json::from_slice::<CredentialEnvelope>(payload)
        .map_err(|_| TokenStoreError::Invalid)?;
    if credential.version != CredentialEnvelope::CURRENT_VERSION {
        return Err(TokenStoreError::Invalid);
    }
    Ok(credential)
}

/// Prefer the canonical store. If it has no entry, copy from the fallback
/// store so callers keep working while the secret is rewritten once.
fn read_secret_with_fallback(
    primary: impl FnOnce() -> Result<Vec<u8>, TokenStoreError>,
    fallback: impl FnOnce() -> Result<Vec<u8>, TokenStoreError>,
    persist_primary: impl FnOnce(&[u8]) -> Result<(), TokenStoreError>,
    delete_fallback: impl FnOnce() -> Result<(), TokenStoreError>,
) -> Result<Vec<u8>, TokenStoreError> {
    match primary() {
        Ok(secret) => Ok(secret),
        Err(TokenStoreError::Missing) => {
            let secret = fallback()?;
            if persist_primary(&secret).is_ok() {
                let _ = delete_fallback();
            }
            Ok(secret)
        }
        Err(error) => Err(error),
    }
}

/// Protected is canonical. A leftover login-keychain copy is best-effort.
fn delete_secret_from_both(
    primary: Result<(), TokenStoreError>,
    fallback: Result<(), TokenStoreError>,
) -> Result<(), TokenStoreError> {
    match (primary, fallback) {
        (Ok(()), _) | (Err(TokenStoreError::Missing), Ok(())) => Ok(()),
        (Err(TokenStoreError::Missing), Err(TokenStoreError::Missing)) => {
            Err(TokenStoreError::Missing)
        }
        (Err(TokenStoreError::Locked), _)
        | (Err(TokenStoreError::Missing), Err(TokenStoreError::Locked)) => {
            Err(TokenStoreError::Locked)
        }
        _ => Err(TokenStoreError::Failed),
    }
}

#[cfg(not(target_os = "macos"))]
fn write_secret(credential_ref: &str, payload: &[u8]) -> Result<(), TokenStoreError> {
    platform_entry(credential_ref)?
        .set_secret(payload)
        .map_err(map_keyring_error)
}

#[cfg(not(target_os = "macos"))]
fn read_secret(credential_ref: &str) -> Result<Vec<u8>, TokenStoreError> {
    platform_entry(credential_ref)?
        .get_secret()
        .map_err(map_keyring_error)
}

#[cfg(not(target_os = "macos"))]
fn delete_secret(credential_ref: &str) -> Result<(), TokenStoreError> {
    platform_entry(credential_ref)?
        .delete_credential()
        .map_err(map_keyring_error)
}

#[cfg(not(target_os = "macos"))]
fn platform_entry(credential_ref: &str) -> Result<keyring::Entry, TokenStoreError> {
    keyring::Entry::new(credential_service(), credential_ref).map_err(map_keyring_error)
}

/// macOS data-protection keychain. Access follows the app's code signature, so
/// the login-keychain ACL dialog does not appear on every launch.
#[cfg(target_os = "macos")]
fn write_secret(credential_ref: &str, payload: &[u8]) -> Result<(), TokenStoreError> {
    match protected_entry(credential_ref)?.set_secret(payload) {
        Ok(()) => {
            let _ = delete_legacy_secret(credential_ref);
            Ok(())
        }
        Err(error) => {
            let mapped = map_keyring_error(error);
            if mapped == TokenStoreError::Locked || mapped == TokenStoreError::Failed {
                return legacy_entry(credential_ref)?
                    .set_secret(payload)
                    .map_err(map_keyring_error);
            }
            Err(mapped)
        }
    }
}

#[cfg(target_os = "macos")]
fn read_secret(credential_ref: &str) -> Result<Vec<u8>, TokenStoreError> {
    read_secret_with_fallback(
        || {
            protected_entry(credential_ref)?
                .get_secret()
                .map_err(map_keyring_error)
        },
        || {
            legacy_entry(credential_ref)?
                .get_secret()
                .map_err(map_keyring_error)
        },
        |secret| {
            protected_entry(credential_ref)?
                .set_secret(secret)
                .map_err(map_keyring_error)
        },
        || delete_legacy_secret(credential_ref),
    )
}

#[cfg(target_os = "macos")]
fn delete_secret(credential_ref: &str) -> Result<(), TokenStoreError> {
    delete_secret_from_both(
        protected_entry(credential_ref)?
            .delete_credential()
            .map_err(map_keyring_error),
        delete_legacy_secret(credential_ref),
    )
}

#[cfg(target_os = "macos")]
fn delete_legacy_secret(credential_ref: &str) -> Result<(), TokenStoreError> {
    legacy_entry(credential_ref)?
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
fn legacy_store() -> &'static std::sync::Arc<apple_native_keyring_store::keychain::Store> {
    static STORE: LazyLock<std::sync::Arc<apple_native_keyring_store::keychain::Store>> =
        LazyLock::new(|| {
            apple_native_keyring_store::keychain::Store::new().expect("login keychain store")
        });
    &STORE
}

#[cfg(target_os = "macos")]
fn protected_entry(credential_ref: &str) -> Result<keyring::Entry, TokenStoreError> {
    // After first unlock, so a linked WhatsApp runtime can start at login
    // without a prompt. This-device-only keeps tokens out of iCloud backups.
    let modifiers = HashMap::from([("access-policy", "after-first-unlock-this-device-only")]);
    let inner = protected_store()
        .build(credential_service(), credential_ref, Some(&modifiers))
        .map_err(map_keyring_error)?;
    Ok(keyring::Entry { inner })
}

#[cfg(target_os = "macos")]
fn legacy_entry(credential_ref: &str) -> Result<keyring::Entry, TokenStoreError> {
    let inner = legacy_store()
        .build(credential_service(), credential_ref, None)
        .map_err(map_keyring_error)?;
    Ok(keyring::Entry { inner })
}

fn map_keyring_error(error: keyring::Error) -> TokenStoreError {
    match error {
        keyring::Error::NoEntry => TokenStoreError::Missing,
        keyring::Error::NoStorageAccess(_) | keyring::Error::NoDefaultStore => {
            TokenStoreError::Locked
        }
        _ => TokenStoreError::Failed,
    }
}

#[derive(Debug, Default)]
pub struct InMemoryTokenStore {
    entries: Mutex<HashMap<String, Vec<u8>>>,
}

impl TokenStore for InMemoryTokenStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &CredentialEnvelope,
    ) -> Result<(), TokenStoreError> {
        let payload = serde_json::to_vec(credential).map_err(|_| TokenStoreError::Invalid)?;
        let mut entries = self.entries.lock().map_err(|_| TokenStoreError::Failed)?;
        if let Some(mut previous) = entries.insert(credential_ref.to_owned(), payload) {
            previous.zeroize();
        }
        Ok(())
    }

    fn get(&self, credential_ref: &str) -> Result<CredentialEnvelope, TokenStoreError> {
        let entries = self.entries.lock().map_err(|_| TokenStoreError::Failed)?;
        let payload = entries
            .get(credential_ref)
            .ok_or(TokenStoreError::Missing)?;
        serde_json::from_slice(payload).map_err(|_| TokenStoreError::Invalid)
    }

    fn delete(&self, credential_ref: &str) -> Result<(), TokenStoreError> {
        let mut removed = self
            .entries
            .lock()
            .map_err(|_| TokenStoreError::Failed)?
            .remove(credential_ref);
        if let Some(payload) = removed.as_mut() {
            payload.zeroize();
        } else {
            return Err(TokenStoreError::Missing);
        }
        Ok(())
    }
}

impl Drop for InMemoryTokenStore {
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

    fn credential(value: &str) -> CredentialEnvelope {
        let mut envelope = CredentialEnvelope::new(value.into());
        envelope.refresh_token = Some(format!("refresh-{value}"));
        envelope
    }

    #[test]
    fn fake_store_round_trip_overwrite_delete_and_missing() {
        let store = InMemoryTokenStore::default();
        store.put("opaque", &credential("first")).expect("put");
        assert_eq!(store.get("opaque").expect("get").access_token, "first");

        store
            .put("opaque", &credential("second"))
            .expect("overwrite");
        assert_eq!(store.get("opaque").expect("get").access_token, "second");

        store.delete("opaque").expect("delete");
        assert_eq!(store.get("opaque").unwrap_err(), TokenStoreError::Missing);
        assert_eq!(
            store.delete("opaque").unwrap_err(),
            TokenStoreError::Missing
        );
    }

    #[test]
    fn debug_output_is_redacted() {
        let fixture = credential("access-secret-fixture");
        let output = format!("{fixture:?}");
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("access-secret-fixture"));
        assert!(!output.contains("refresh-access-secret-fixture"));
    }

    #[test]
    fn decode_envelope_rejects_wrong_version() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "version": 99,
            "accessToken": "secret",
        }))
        .expect("payload");
        assert_eq!(
            decode_envelope(&payload).unwrap_err(),
            TokenStoreError::Invalid
        );
    }

    #[test]
    fn fallback_read_migrates_once_then_forgets_the_copy() {
        let migrated = Mutex::new(false);
        let deleted = Mutex::new(false);
        let secret = read_secret_with_fallback(
            || Err(TokenStoreError::Missing),
            || Ok(b"copied".to_vec()),
            |_| {
                *migrated.lock().expect("migrated") = true;
                Ok(())
            },
            || {
                *deleted.lock().expect("deleted") = true;
                Ok(())
            },
        )
        .expect("fallback");
        assert_eq!(secret, b"copied");
        assert!(*migrated.lock().expect("migrated"));
        assert!(*deleted.lock().expect("deleted"));
    }

    #[test]
    fn locked_primary_store_does_not_consult_fallback() {
        let error = read_secret_with_fallback(
            || Err(TokenStoreError::Locked),
            || panic!("fallback must not run when the primary store is locked"),
            |_| panic!("migrate must not run when the primary store is locked"),
            || panic!("delete must not run when the primary store is locked"),
        )
        .unwrap_err();
        assert_eq!(error, TokenStoreError::Locked);
    }

    #[test]
    fn delete_treats_protected_success_as_done() {
        assert_eq!(
            delete_secret_from_both(Ok(()), Err(TokenStoreError::Locked)),
            Ok(())
        );
        assert_eq!(
            delete_secret_from_both(Err(TokenStoreError::Missing), Err(TokenStoreError::Missing)),
            Err(TokenStoreError::Missing)
        );
        assert_eq!(
            delete_secret_from_both(Err(TokenStoreError::Locked), Ok(())),
            Err(TokenStoreError::Locked)
        );
    }
}
