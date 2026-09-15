//! Sharing transport and canonical link lifecycle. Network and owner
//! capabilities stay in Rust; the React feature receives only DTOs.
mod links;
mod owner;
mod service;

pub use links::{parse_share_link, ShareLink, TrustedShareOrigin};
pub use owner::{
    ensure_owner_capability, KeyringOwnerStore, MemoryOwnerStore, OwnerCapabilityStore,
    OWNER_CREDENTIAL_REF,
};
pub use service::{Publication, PublicationRequest, SharedTemplate, SharingClient, SharingError};

/// Application-owned transport state. The service origin is explicit so a
/// source build cannot silently publish to an operator's production host.
pub struct SharingState(pub Option<service::SharingClient>);
impl SharingState {
    pub fn disabled() -> Self {
        Self(None)
    }
}
impl SharingState {
    pub fn from_environment() -> Result<Option<Self>, String> {
        let configured_origin = std::env::var_os("ALFRED_SHARE_ORIGIN")
            .or_else(|| option_env!("ALFRED_SHARE_ORIGIN").map(std::ffi::OsString::from));
        let Some(origin) = configured_origin else {
            return Ok(None);
        };
        let origin = TrustedShareOrigin::parse(&origin.to_string_lossy(), cfg!(debug_assertions))
            .map_err(|error| error.to_string())?;
        Ok(Some(Self(Some(
            service::SharingClient::new(origin, Box::new(KeyringOwnerStore::default()))
                .map_err(|error| error.to_string())?,
        ))))
    }
}
