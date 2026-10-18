//! Store-key custody for the WhatsApp protocol store (Plan 023 Step 2).
//!
//! The key that encrypts the protocol database is random, generated once at
//! pairing, and kept in the OS credential store under an **opaque** reference.
//! The reference is a fresh UUID: it is never derived from a phone number, JID,
//! device id, or user password, so the credential entry itself reveals nothing
//! about which account is linked.

use std::path::PathBuf;

use uuid::Uuid;

use super::crypto::{CryptoError, StoreKey};
use crate::db::{app_data_dir, DbError};
use crate::integrations::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};

/// Prefix that scopes this provider's entries inside Alfred's credential
/// service, so a WhatsApp key can never be confused with an OAuth token.
const REF_PREFIX: &str = "whatsapp-protocol-store";

/// Sub-directory of Alfred's app data holding the protocol database. Separate
/// from `app.db` by design.
const STORE_DIR: &str = "whatsapp";
const STORE_FILE: &str = "protocol.db";

#[derive(Debug, thiserror::Error)]
pub enum KeyCustodyError {
    #[error("credential store is unavailable or locked")]
    CredentialStore(#[from] TokenStoreError),
    #[error("stored key material is unusable")]
    Key(#[from] CryptoError),
    #[error("could not resolve the protocol store location")]
    Location(#[from] DbError),
}

/// A freshly provisioned key and the opaque reference it lives under.
pub struct ProvisionedKey {
    pub credential_ref: String,
    pub key: StoreKey,
}

/// Generates a new random store key and files it under a new opaque reference.
///
/// Called once, during pairing. The returned reference is what the connection
/// row persists; the key itself never touches SQLite.
pub fn provision(tokens: &dyn TokenStore) -> Result<ProvisionedKey, KeyCustodyError> {
    let credential_ref = format!("{REF_PREFIX}/{}", Uuid::new_v4());
    let key = StoreKey::generate();
    save(tokens, &credential_ref, &key)?;
    Ok(ProvisionedKey {
        credential_ref,
        key,
    })
}

/// Writes (or replaces) the key held under `credential_ref`.
pub fn save(
    tokens: &dyn TokenStore,
    credential_ref: &str,
    key: &StoreKey,
) -> Result<(), KeyCustodyError> {
    // `CredentialEnvelope` zeroizes its fields on drop, so the base64 key does
    // not linger in the heap after the write.
    let envelope = CredentialEnvelope::new(key.expose_base64());
    tokens.put(credential_ref, &envelope)?;
    Ok(())
}

/// Loads the key held under `credential_ref`.
///
/// A missing entry is [`TokenStoreError::Missing`] and a locked keychain is
/// [`TokenStoreError::Locked`]; the caller must distinguish them, because the
/// first means the connection is unrecoverable and the second is temporary.
pub fn load(tokens: &dyn TokenStore, credential_ref: &str) -> Result<StoreKey, KeyCustodyError> {
    let envelope = tokens.get(credential_ref)?;
    Ok(StoreKey::from_base64(&envelope.access_token)?)
}

/// Removes the key. Part of disconnect, which must clear local state whether or
/// not the remote logout succeeded.
pub fn delete(tokens: &dyn TokenStore, credential_ref: &str) -> Result<(), KeyCustodyError> {
    match tokens.delete(credential_ref) {
        // Already gone is success: disconnect must be idempotent.
        Ok(()) | Err(TokenStoreError::Missing) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Absolute path of the protocol database.
pub fn store_path() -> Result<PathBuf, KeyCustodyError> {
    Ok(app_data_dir()?.join(STORE_DIR).join(STORE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::token_store::InMemoryTokenStore;

    #[test]
    fn provision_yields_a_working_key_under_an_opaque_reference() {
        let tokens = InMemoryTokenStore::default();
        let provisioned = provision(&tokens).unwrap();

        assert!(provisioned.credential_ref.starts_with(REF_PREFIX));
        // Opaque: nothing but the prefix and a UUID.
        let suffix = provisioned
            .credential_ref
            .strip_prefix(&format!("{REF_PREFIX}/"))
            .unwrap();
        assert!(
            Uuid::parse_str(suffix).is_ok(),
            "reference must be a bare UUID"
        );

        let loaded = load(&tokens, &provisioned.credential_ref).unwrap();
        let aad = super::super::crypto::row_aad("session", &[1u8; 32]);
        let sealed = provisioned.key.seal(&aad, b"payload");
        assert_eq!(loaded.open(&aad, &sealed).unwrap(), b"payload");
    }

    #[test]
    fn each_provision_is_independent() {
        let tokens = InMemoryTokenStore::default();
        let first = provision(&tokens).unwrap();
        let second = provision(&tokens).unwrap();

        assert_ne!(first.credential_ref, second.credential_ref);
        let aad = super::super::crypto::row_aad("session", &[1u8; 32]);
        let sealed = first.key.seal(&aad, b"payload");
        assert!(
            second.key.open(&aad, &sealed).is_err(),
            "a second pairing must not be able to read the first one's data"
        );
    }

    #[test]
    fn a_missing_reference_is_reported_as_missing() {
        let tokens = InMemoryTokenStore::default();
        assert!(matches!(
            load(&tokens, "whatsapp-protocol-store/nope"),
            Err(KeyCustodyError::CredentialStore(TokenStoreError::Missing))
        ));
    }

    #[test]
    fn delete_is_idempotent() {
        let tokens = InMemoryTokenStore::default();
        let provisioned = provision(&tokens).unwrap();

        delete(&tokens, &provisioned.credential_ref).unwrap();
        // Disconnect may run twice; the second pass must not fail.
        delete(&tokens, &provisioned.credential_ref).unwrap();
        assert!(load(&tokens, &provisioned.credential_ref).is_err());
    }

    #[test]
    fn corrupt_key_material_is_rejected_rather_than_used() {
        let tokens = InMemoryTokenStore::default();
        let credential_ref = format!("{REF_PREFIX}/{}", Uuid::new_v4());
        tokens
            .put(
                &credential_ref,
                &CredentialEnvelope::new("not-a-key".into()),
            )
            .unwrap();

        assert!(matches!(
            load(&tokens, &credential_ref),
            Err(KeyCustodyError::Key(CryptoError::MalformedKey))
        ));
    }

}
