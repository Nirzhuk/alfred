fn main() {
    load_dotenv();

    // option_env!/env! reads aren't tracked by Cargo's fingerprint by default,
    // so a value change alone won't trigger a rebuild without these.
    println!("cargo:rerun-if-env-changed=ALFRED_GITHUB_APP_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=ALFRED_GITHUB_APP_INSTALL_URL");
    println!("cargo:rerun-if-env-changed=ALFRED_GMAIL_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=ALFRED_GMAIL_OAUTH_PORT");
    println!("cargo:rerun-if-env-changed=ALFRED_MICROSOFT_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=ALFRED_MICROSOFT_TENANT");
    println!("cargo:rerun-if-env-changed=ALFRED_MICROSOFT_OAUTH_PORT");
    println!("cargo:rerun-if-env-changed=ALFRED_POLAR_ENVIRONMENT");
    println!("cargo:rerun-if-env-changed=ALFRED_POLAR_ORGANIZATION_ID");
    println!("cargo:rerun-if-env-changed=ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID");
    println!("cargo:rerun-if-env-changed=ALFRED_POLAR_TEAMS_BENEFIT_ID");
    println!("cargo:rerun-if-env-changed=ALFRED_RELEASE_DATE");
    println!("cargo:rerun-if-env-changed=ALFRED_DISABLE_LICENSE_GATING");
    tauri_build::build()
}

/// Loads the workspace `.env` so `bun run dev` and `tauri build` both bake
/// the same publisher `ALFRED_*` values without exporting them per shell.
/// An explicit shell export still wins over the file.
fn load_dotenv() {
    const KEYS: &[&str] = &[
        "ALFRED_GITHUB_APP_CLIENT_ID",
        "ALFRED_GITHUB_APP_INSTALL_URL",
        "ALFRED_GMAIL_CLIENT_ID",
        "ALFRED_GMAIL_OAUTH_PORT",
        "ALFRED_MICROSOFT_CLIENT_ID",
        "ALFRED_MICROSOFT_TENANT",
        "ALFRED_MICROSOFT_OAUTH_PORT",
        "ALFRED_POLAR_ENVIRONMENT",
        "ALFRED_POLAR_ORGANIZATION_ID",
        "ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID",
        "ALFRED_POLAR_TEAMS_BENEFIT_ID",
        // Opt-out for self-compiled builds without Polar: `1` disables all
        // license gating in the frontend (`appLicenseGatingDisabled`).
        "ALFRED_DISABLE_LICENSE_GATING",
        // The release workflow supplies this as an ISO 8601 `YYYY-MM-DD` date.
        // Unset means a source build, which never closes an update window.
        "ALFRED_RELEASE_DATE",
    ];
    let Some(workspace_root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        return;
    };
    let dotenv_path = workspace_root.join(".env");
    println!("cargo:rerun-if-changed={}", dotenv_path.display());
    let Ok(contents) = std::fs::read_to_string(&dotenv_path) else {
        return;
    };
    for line in contents.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if line.starts_with('#') || !KEYS.contains(&key) || std::env::var(key).is_ok() {
            continue;
        }
        println!("cargo:rustc-env={key}={}", value.trim());
    }
}
