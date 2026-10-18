use std::sync::OnceLock;

static DEVELOPMENT: OnceLock<bool> = OnceLock::new();

/// Initialize only from the identifier in Tauri's resolved configuration.
/// Build profiles do not determine which data or credentials an app can use.
pub(crate) fn initialize(identifier: &str) -> Result<(), &'static str> {
    let development = match identifier {
        "com.nirzhuk.alfred" => false,
        "com.nirzhuk.alfred.dev" => true,
        _ => return Err("unrecognized Alfred application identifier"),
    };
    DEVELOPMENT
        .set(development)
        .map_err(|_| "application identity already initialized")
}

pub(crate) fn is_development() -> bool {
    *DEVELOPMENT
        .get()
        .expect("application identity must be initialized before use")
}

pub(crate) fn credential_service(
    production: &'static str,
    development: &'static str,
) -> &'static str {
    if is_development() {
        development
    } else {
        production
    }
}
