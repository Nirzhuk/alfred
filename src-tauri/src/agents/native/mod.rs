//! Provider-neutral contract for bundled native agent runtimes.
//!
//! Provider modules register one implementation in [`NativeRuntimeRegistry`].
//! The contract deliberately has no provider-specific payload or CLI fallback.

mod context;
mod contract;
mod events;
mod host_approval;
mod memory_tools;
pub mod providers;
mod redaction;
mod registry;
mod tools;

#[cfg(test)]
mod conformance;
#[cfg(test)]
mod fake;

pub use context::{prepare_native_request, NativeContextPolicy};
pub use contract::*;
pub use events::*;
pub use host_approval::*;
pub use memory_tools::*;
pub(crate) use redaction::contains_diagnostic_secret;
pub use redaction::{
    canonical_key, contains_cli_permission_flag, contains_secret_marker, is_secret_key, redact_text,
};
pub use registry::*;
pub use tools::*;

pub const NATIVE_REQUEST_CONTRACT_VERSION: u16 = 2;
pub const NATIVE_EVENT_CONTRACT_VERSION: u16 = 1;
pub const NATIVE_CAPABILITY_CONTRACT_VERSION: u16 = 5;
