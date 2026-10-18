use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub const OWNER_CREDENTIAL_REF: &str = "workflow-share-owner-capability-v1";

fn credential_service() -> &'static str {
    crate::app_identity::credential_service(
        "com.nirzhuk.alfred.workflow-sharing",
        "com.nirzhuk.alfred.workflow-sharing.dev",
    )
}

pub trait OwnerCapabilityStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, String>;
    fn put(&self, value: &str) -> Result<(), String>;
}

/// OS-backed owner storage. The capability is generated before the first
/// upload and remains outside SQLite, URLs, React state, and logs.
#[derive(Default)]
pub struct KeyringOwnerStore;
impl OwnerCapabilityStore for KeyringOwnerStore {
    fn get(&self) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(credential_service(), OWNER_CREDENTIAL_REF)
            .map_err(|error| error.to_string())?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    fn put(&self, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(credential_service(), OWNER_CREDENTIAL_REF)
            .map_err(|error| error.to_string())?;
        entry.set_password(value).map_err(|error| error.to_string())
    }
}

#[derive(Clone, Default)]
pub struct MemoryOwnerStore(Arc<Mutex<HashMap<String, String>>>);
impl OwnerCapabilityStore for MemoryOwnerStore {
    fn get(&self) -> Result<Option<String>, String> {
        self.0
            .lock()
            .map_err(|_| "owner store unavailable".into())
            .map(|store| store.get(OWNER_CREDENTIAL_REF).cloned())
    }
    fn put(&self, value: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "owner store unavailable".into())
            .map(|mut store| {
                store.insert(OWNER_CREDENTIAL_REF.into(), value.into());
            })
    }
}

pub fn ensure_owner_capability(store: &dyn OwnerCapabilityStore) -> Result<String, String> {
    static CREATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = CREATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "owner store unavailable".to_string())?;
    if let Some(existing) = store.get()? {
        if existing.len() >= 32 {
            return Ok(existing);
        }
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let capability = URL_SAFE_NO_PAD.encode(bytes);
    store.put(&capability)?;
    Ok(capability)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generates_and_reuses_capability() {
        let store = MemoryOwnerStore::default();
        let first = ensure_owner_capability(&store).unwrap();
        assert_eq!(first, ensure_owner_capability(&store).unwrap());
        assert!(first.len() >= 43);
    }
}
