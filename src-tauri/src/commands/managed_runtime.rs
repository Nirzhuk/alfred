//! Shared control plane for Alfred-managed provider runtimes.
//!
//! This module owns the safe command/state boundary around the provider
//! slices.  It deliberately does not enable a provider: package installation,
//! profile creation, and connection start all remain unavailable until the
//! provider's release gates and independent package evidence are present.

use crate::agent_accounts::authorization::AuthorizationAttempt;
use crate::agent_accounts::models::{
    canonical_agent_identity_key, AgentAccount, AgentAccountCommandError, AgentAccountStatus,
    AgentApiKeySecret, AgentAuthMethod, AgentEntitlementState, AgentProductId,
    AuthorizedAgentAccount, CredentialCustodyMode, ManagedRuntimeId,
};
use crate::agent_accounts::runtime_profile::{
    RuntimeEnvironmentVariable, RuntimeProfile, RuntimeProfileBinding, RuntimeProfileRef,
    RuntimeProfileStore,
};
use crate::agent_accounts::service::{
    AgentAccountProvider, AgentAccountsState, AgentProviderError, AgentProviderFailureKind,
    AgentProviderRegistration, ProviderAccountAccess, ProviderAccountGrant,
    ProviderAuthorizationStart, ProviderFuture,
};
use crate::agents::managed_runtime::{ManagedRuntimeCancellation, ManagedRuntimeSupervisor};
use crate::agents::native::providers::claude::{
    ClaudeCodeSubscriptionRuntime, ClaudePublisherVerificationError,
    ClaudePublisherVerificationRequest, ClaudeTerminalError, ClaudeTerminalLaunchSpec,
    ClaudeTerminalSession,
};
use crate::agents::native::providers::codex::{
    launch_codex_sdk_login, open_chatgpt_sign_in_url, purge_logged_out_codex_profile,
    CodexSdkConnection, CodexSdkPackageError, CodexSdkPackageVerifier, CodexSdkVerifierRequest,
};
use crate::agents::native::providers::opencode::OpenCodePackageVerifier;
use crate::agents::native::providers::opencode::OpenCodeServerSession;
use crate::agents::native::providers::{claude, codex, opencode};
use crate::agents::native::{
    HostApprovalBroker, HostApprovalDecision, NativeCancellation, NativeErrorCode,
    NativeRuntimeError, NativeRuntimeRegistry,
};
use crate::agents::publisher_trust::production_platform_verifier;
use crate::agents::runtime_package::{
    verify_runtime_package_with_platform_evidence, RuntimePackageError, RuntimePackageManifest,
    RuntimePackageSelection, RuntimePackageStore, RuntimePackageVerification,
    RuntimePlatformPublisherVerifier, UnavailableRuntimePlatformPublisherVerifier,
};
use crate::agents::{AgentHarness, AgentProvider, OpaqueAgentAccountRef};
use crate::db::Db;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

const CLAUDE_PRODUCT: AgentProductId = AgentProductId::ClaudeCodeSubscription;
const CODEX_PRODUCT: AgentProductId = AgentProductId::ChatgptCodex;
const OPENCODE_PRODUCT: AgentProductId = AgentProductId::OpencodeGo;

/// Exact public product projection consumed by the managed-runtime settings
/// surface.  All fields are descriptive and safe; no profile or credential
/// reference crosses this boundary.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeProductDto {
    pub provider_id: String,
    pub product_id: String,
    pub product_name: String,
    pub runtime_id: String,
    pub runtime_version: String,
    pub install_state: String,
    pub connection_kind: String,
    pub connect_available: bool,
    pub gate_codes: Vec<String>,
    pub billing_source: String,
    pub custody_mode: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeConnectionStartDto {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    // This is intentionally present even when no provider ceremony is
    // active, so the public contract has an explicit nullable expiry.
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeConnectionStatusDto {
    pub provider_id: String,
    pub product_id: String,
    pub install_state: String,
    pub connection_state: String,
    pub account_id: Option<String>,
    pub entitlement_state: String,
    pub last_error_code: Option<String>,
}

/// Public terminal read projection. Provider session ids are opaque handles;
/// provider-specific output framing remains behind this command boundary.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeTerminalReadDto {
    pub session_id: String,
    pub cursor: u64,
    pub output: String,
    pub closed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeCommandError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl ManagedRuntimeCommandError {
    fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
        }
    }
}

impl fmt::Display for ManagedRuntimeCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ManagedRuntimeCommandError {}

#[derive(Debug, Clone)]
struct ManagedProductSpec {
    provider: AgentProvider,
    product: AgentProductId,
    runtime_id: crate::agent_accounts::models::ManagedRuntimeId,
    runtime_version: &'static str,
    connection_kind: &'static str,
    gate_codes: Vec<String>,
    primary_gate_code: &'static str,
}

impl ManagedProductSpec {
    fn dto(
        &self,
        backend_ready: bool,
        extra_gates: &[String],
        install_state: &str,
    ) -> ManagedRuntimeProductDto {
        let mut gate_codes = self.gate_codes.clone();
        gate_codes.extend(extra_gates.iter().cloned());
        if !backend_ready {
            gate_codes.push("managed_runtime_storage_unavailable".into());
        }
        ManagedRuntimeProductDto {
            provider_id: self.provider.as_str().into(),
            product_id: self.product.as_str().into(),
            product_name: self.product.label().into(),
            runtime_id: self.runtime_id.as_str().into(),
            runtime_version: self.runtime_version.into(),
            install_state: install_state.into(),
            connection_kind: self.connection_kind.into(),
            connect_available: backend_ready && gate_codes.is_empty(),
            gate_codes,
            billing_source: self.product.billing_source().into(),
            custody_mode: self.product.custody_mode().as_str().into(),
        }
    }
}

#[derive(Clone)]
struct ManagedRuntimeBackend {
    packages: RuntimePackageStore,
    profiles: RuntimeProfileStore,
    resource_root: Option<PathBuf>,
    catalog_root: PathBuf,
}

#[derive(Debug, Clone, Default)]
struct ManagedConnection {
    kind: String,
    state: String,
    attempt_id: Option<String>,
    authorization_url: Option<String>,
    user_code: Option<String>,
    terminal_session_id: Option<String>,
    account_id: Option<String>,
    profile_ref: Option<String>,
    pending_account_id: Option<String>,
    codex_login_id: Option<String>,
}

#[derive(Default)]
struct OauthConnectFixtures {
    claude: AtomicBool,
    claude_complete: AtomicBool,
    codex: AtomicBool,
    codex_complete: AtomicBool,
}

/// Process-wide state.  The supervisor is long-lived and backend-only; the
/// package/profile stores are initialized from Tauri's app-data directory in
/// setup.  Commands remain safe if setup cannot initialize storage.
pub struct ManagedRuntimeControlPlane {
    supervisor: ManagedRuntimeSupervisor,
    trust_verifier: ManagedRuntimeTrustVerifier,
    backend: Mutex<Option<ManagedRuntimeBackend>>,
    pending_packages: Mutex<Vec<AgentProductId>>,
    connections: Mutex<HashMap<String, ManagedConnection>>,
    terminal_sessions: Mutex<HashMap<String, Arc<ClaudeTerminalSession>>>,
    opencode_selection: Mutex<Option<RuntimePackageSelection>>,
    host_broker: Mutex<Option<Arc<HostApprovalBroker>>>,
    native_registry: Mutex<Option<Arc<NativeRuntimeRegistry>>>,
    opencode_servers: Mutex<Option<Arc<dyn opencode::OpenCodeServerProvider>>>,
    waive_opencode_process_gates: AtomicBool,
    claude_selection: Mutex<Option<RuntimePackageSelection>>,
    codex_selection: Mutex<Option<RuntimePackageSelection>>,
    codex_sessions: Mutex<HashMap<String, CodexSdkConnection>>,
    oauth_fixtures: OauthConnectFixtures,
}

impl ManagedRuntimeControlPlane {
    pub fn new() -> Self {
        Self {
            supervisor: ManagedRuntimeSupervisor::new(),
            trust_verifier: ManagedRuntimeTrustVerifier::new(production_platform_verifier()),
            backend: Mutex::new(None),
            pending_packages: Mutex::new(Vec::new()),
            connections: Mutex::new(HashMap::new()),
            terminal_sessions: Mutex::new(HashMap::new()),
            opencode_selection: Mutex::new(None),
            host_broker: Mutex::new(None),
            native_registry: Mutex::new(None),
            opencode_servers: Mutex::new(None),
            waive_opencode_process_gates: AtomicBool::new(false),
            claude_selection: Mutex::new(None),
            codex_selection: Mutex::new(None),
            codex_sessions: Mutex::new(HashMap::new()),
            oauth_fixtures: OauthConnectFixtures::default(),
        }
    }

    /// Open storage and discover bundled packages without hashing or staging
    /// them. Verification must run separately on a blocking worker.
    pub fn initialize(
        &self,
        app_data_root: &Path,
        resource_root: Option<&Path>,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let mut backend = self.backend.lock().map_err(|_| state_error())?;
        if backend.is_some() {
            return Ok(());
        }
        let packages = RuntimePackageStore::open(app_data_root)
            .map_err(|error| package_command_error(error, true))?;
        let profiles = RuntimeProfileStore::new(app_data_root)
            .map_err(|error| profile_command_error(error.to_string(), true))?;
        let catalog_root = app_data_root
            .join("managed-runtimes")
            .join("catalog")
            .join("opencode_go");
        fs::create_dir_all(&catalog_root).map_err(|_| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_catalog_unavailable",
                "Managed runtime catalog storage is unavailable.",
                true,
            )
        })?;
        let resource_root = resource_root.map(Path::to_path_buf);
        let mut pending = self.pending_packages.lock().map_err(|_| state_error())?;
        for (product, package_root) in [
            (
                OPENCODE_PRODUCT,
                first_opencode_package_root(resource_root.as_deref()),
            ),
            (
                CLAUDE_PRODUCT,
                first_claude_package_root(resource_root.as_deref()),
            ),
            (
                CODEX_PRODUCT,
                first_codex_package_root(resource_root.as_deref()),
            ),
        ] {
            if package_root.is_some() {
                pending.push(product);
            }
        }
        *backend = Some(ManagedRuntimeBackend {
            packages,
            profiles,
            resource_root,
            catalog_root,
        });
        Ok(())
    }

    pub async fn verify_bundled_packages(
        self: Arc<Self>,
        on_change: impl Fn() + Send + 'static,
    ) -> Result<(), ManagedRuntimeCommandError> {
        run_managed_blocking(move || {
            let pending = self
                .pending_packages
                .lock()
                .map_err(|_| state_error())?
                .clone();
            for product in pending {
                if let Err(error) = self.refresh_product_package(product) {
                    eprintln!("managed runtime package unavailable: {error}");
                }
                self.pending_packages
                    .lock()
                    .map_err(|_| state_error())?
                    .retain(|pending| *pending != product);
                on_change();
            }
            Ok(())
        })
        .await
    }

    fn refresh_product_package(
        &self,
        product: AgentProductId,
    ) -> Result<(), ManagedRuntimeCommandError> {
        match product {
            OPENCODE_PRODUCT => self.refresh_opencode_package(),
            CLAUDE_PRODUCT => self.refresh_claude_package(),
            CODEX_PRODUCT => self.refresh_codex_package(),
            _ => Ok(()),
        }
    }

    pub fn bind_native_collaborators(
        &self,
        registry: Arc<NativeRuntimeRegistry>,
        broker: Arc<HostApprovalBroker>,
    ) -> Result<(), ManagedRuntimeCommandError> {
        *self.native_registry.lock().map_err(|_| state_error())? = Some(registry);
        *self.host_broker.lock().map_err(|_| state_error())? = Some(broker);
        let _ = self.register_verified_opencode_runtime();
        Ok(())
    }

    #[cfg(test)]
    pub fn waive_opencode_process_gates_for_test(&self) {
        self.waive_opencode_process_gates.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub fn inject_opencode_servers_for_test(
        &self,
        servers: Arc<dyn opencode::OpenCodeServerProvider>,
    ) {
        *self
            .opencode_servers
            .lock()
            .expect("test opencode server lock") = Some(servers);
    }

    #[cfg(test)]
    pub fn enable_claude_oauth_fixture_for_test(&self) {
        self.oauth_fixtures.claude.store(true, Ordering::SeqCst);
        self.oauth_fixtures.claude_complete.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub fn enable_codex_oauth_fixture_for_test(&self) {
        self.oauth_fixtures.codex.store(true, Ordering::SeqCst);
        self.oauth_fixtures.codex_complete.store(true, Ordering::SeqCst);
    }

    pub fn supervisor(&self) -> &ManagedRuntimeSupervisor {
        &self.supervisor
    }

    pub fn trust_verifier(&self) -> &ManagedRuntimeTrustVerifier {
        &self.trust_verifier
    }

    /// Returns the only bridge allowed to connect an OpenCode supervisor
    /// launch to its fixed-loopback authenticated client. The bridge owns the
    /// one-way password handoff and never exposes the password to commands.
    pub fn supervisor_http_bridge(&self) -> Arc<dyn opencode::OpenCodeSupervisorHttpBridge> {
        Arc::new(ManagedRuntimeSupervisorHttpBridge)
    }

    pub fn provider_handlers(self: &Arc<Self>) -> Vec<Arc<dyn AgentAccountProvider>> {
        managed_products()
            .into_iter()
            .map(|spec| {
                Arc::new(ManagedRuntimeAccountProvider {
                    control_plane: Arc::clone(self),
                    spec,
                }) as Arc<dyn AgentAccountProvider>
            })
            .collect()
    }

    pub fn list_products(
        &self,
    ) -> Result<Vec<ManagedRuntimeProductDto>, ManagedRuntimeCommandError> {
        managed_products()
            .into_iter()
            .map(|spec| self.product_status(&spec))
            .collect()
    }

    pub fn prepare_product(
        &self,
        provider_id: &str,
        product_id: &str,
    ) -> Result<ManagedRuntimeProductDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        let preparing = self
            .pending_packages
            .lock()
            .map_err(|_| state_error())?
            .contains(&spec.product);
        if preparing {
            return self.product_status(&spec);
        }
        self.refresh_product_package(spec.product)?;
        let status = self.product_status(&spec)?;
        if status.install_state == "missing" || status.install_state == "failed" {
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_package_missing",
                "Couldn't start sign-in. This copy of Alfred doesn't include it yet.",
                true,
            ));
        }
        Ok(status)
    }

    pub fn start_connection(
        &self,
        db: &Db,
        provider_id: &str,
        product_id: &str,
    ) -> Result<ManagedRuntimeConnectionStartDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        // Claude Code names its own macOS Keychain item, so a second account
        // overwrites the first and can invalidate a live session. Refuse the
        // second connect instead of silently evicting the connected account.
        if spec.product == CLAUDE_PRODUCT
            && self
                .persisted_managed_account(db, spec.provider, spec.product)
                .is_some()
        {
            return Err(ManagedRuntimeCommandError::new(
                CLAUDE_SINGLE_ACCOUNT_CODE,
                "Claude allows one connected subscription account. Disconnect the current one first.",
                false,
            ));
        }
        let status = self.product_status(&spec)?;
        if status.install_state == "missing" || status.install_state == "failed" {
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_package_missing",
                "Couldn't start sign-in. This copy of Alfred doesn't include it yet.",
                true,
            ));
        }
        if !status.connect_available {
            return Err(ManagedRuntimeCommandError::new(
                spec.primary_gate_code,
                "This managed provider product is blocked by its release gates.",
                false,
            ));
        }
        match spec.product {
            OPENCODE_PRODUCT if spec.connection_kind == "api_key" => {
                self.begin_opencode_api_key_connection()
            }
            CLAUDE_PRODUCT if spec.connection_kind == "terminal" => self.start_claude_oauth(&spec),
            CODEX_PRODUCT if spec.connection_kind == "browser" => self.start_codex_oauth(&spec),
            _ => Err(ManagedRuntimeCommandError::new(
                "managed_runtime_connection_unavailable",
                "The managed runtime connection is not available in this build.",
                false,
            )),
        }
    }

    /// OpenCode Go key intake. The secret is parsed, handed to the isolated
    /// runtime, and dropped. Command DTOs never retain it.
    pub fn connect_api_key(
        &self,
        db: &Db,
        accounts: &AgentAccountsState,
        provider_id: &str,
        product_id: &str,
        api_key: Zeroizing<String>,
    ) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        if spec.product != OPENCODE_PRODUCT {
            drop(api_key);
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_api_key_product_invalid",
                "Managed API-key intake is only available for OpenCode Go.",
                false,
            ));
        }
        let status = self.product_status(&spec)?;
        if !self.opencode_process_gates_waived() && !status.gate_codes.is_empty() {
            drop(api_key);
            return Err(ManagedRuntimeCommandError::new(
                spec.primary_gate_code,
                "OpenCode Go managed API-key intake is blocked by its release gates.",
                false,
            ));
        }
        let parsed = match opencode::OpenCodeGoKey::parse(api_key.to_string()) {
            Ok(parsed) => parsed,
            Err(_) => {
                drop(api_key);
                return Err(ManagedRuntimeCommandError::new(
                    "managed_runtime_api_key_invalid",
                    "That OpenCode Go key could not be accepted.",
                    false,
                ));
            }
        };
        drop(api_key);
        let result = self.connect_opencode_go(db, accounts, parsed);
        match result {
            Ok(status) => Ok(status),
            Err(error) => {
                self.record_opencode_error(&error.code);
                Err(error)
            }
        }
    }

    pub fn connection_status(
        &self,
        db: &Db,
        accounts: &AgentAccountsState,
        provider_id: &str,
        product_id: &str,
    ) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        self.poll_pending_oauth(db, accounts, &spec)?;
        let key = spec.product.as_str().to_owned();
        let connection = self
            .connections
            .lock()
            .map_err(|_| state_error())?
            .get(&key)
            .cloned();
        let product_status = self.product_status(&spec)?;
        let install_state = product_status.install_state;
        let persisted = self.persisted_managed_account(db, spec.provider, spec.product);
        let account_id = connection
            .as_ref()
            .and_then(|connection| connection.account_id.clone())
            .or_else(|| persisted.as_ref().map(|account| account.id.clone()));
        let entitlement_state = persisted
            .as_ref()
            .map(|account| account.entitlement_state.as_str().to_owned())
            .unwrap_or_else(|| "unknown".into());
        let fallback_connection_state = if account_id.is_some() {
            "connected"
        } else if install_state == "blocked" {
            "error"
        } else {
            "disconnected"
        };
        let connection_state = connection
            .as_ref()
            .map(|connection| match connection.state.as_str() {
                "connecting" => "connecting",
                "connected" => "connected",
                "limited" => "limited",
                "disconnected" => "disconnected",
                _ => "error",
            })
            .unwrap_or(fallback_connection_state);
        let last_error_code = match connection_state {
            "connected" | "connecting" => None,
            _ => product_status.gate_codes.first().cloned(),
        };
        Ok(ManagedRuntimeConnectionStatusDto {
            provider_id: spec.provider.as_str().into(),
            product_id: spec.product.as_str().into(),
            install_state,
            connection_state: connection_state.into(),
            account_id,
            entitlement_state,
            last_error_code,
        })
    }

    fn product_status(
        &self,
        spec: &ManagedProductSpec,
    ) -> Result<ManagedRuntimeProductDto, ManagedRuntimeCommandError> {
        let backend_ready = self
            .backend
            .lock()
            .ok()
            .is_some_and(|backend| backend.is_some());
        let mut extra_gates = Vec::new();
        let mut gate_filter: Vec<String> = Vec::new();
        let install_state = if !backend_ready {
            "blocked"
        } else if self
            .pending_packages
            .lock()
            .map_err(|_| state_error())?
            .contains(&spec.product)
        {
            extra_gates.push("managed_runtime_preparing".into());
            "preparing"
        } else if spec.product == OPENCODE_PRODUCT {
            if self.opencode_process_gates_waived() {
                gate_filter.extend([
                    opencode::COMMERCIAL_GATE_CODE.into(),
                    opencode::LIVE_SMOKE_GATE_CODE.into(),
                ]);
            }
            if self.opencode_runtime_ready() {
                "ready"
            } else {
                extra_gates.push(opencode::PACKAGE_GATE_CODE.into());
                "missing"
            }
        } else if spec.product == CLAUDE_PRODUCT {
            gate_filter.extend(claude_connect_gate_filter());
            if self.claude_connect_ready() {
                gate_filter.extend([
                    claude::PACKAGE_INTEGRATION_BLOCKED_CODE.into(),
                    claude::PUBLISHER_VERIFIER_BLOCKED_CODE.into(),
                ]);
                "ready"
            } else {
                "missing"
            }
        } else if spec.product == CODEX_PRODUCT {
            gate_filter.extend(codex_connect_gate_filter());
            if self.codex_connect_ready() {
                gate_filter.push(codex::SEALED_PACKAGE_BLOCKER.into());
                "ready"
            } else {
                "missing"
            }
        } else {
            "missing"
        };
        let mut dto = spec.dto(backend_ready, &extra_gates, install_state);
        if !gate_filter.is_empty() {
            dto.gate_codes
                .retain(|code| !gate_filter.iter().any(|filtered| filtered == code));
            dto.connect_available = backend_ready && dto.gate_codes.is_empty();
        }
        Ok(dto)
    }

    fn connect_opencode_go(
        &self,
        db: &Db,
        accounts: &AgentAccountsState,
        key: opencode::OpenCodeGoKey,
    ) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
        let servers = self.opencode_servers()?;
        let catalog = self.opencode_catalog_root()?;
        let identity = canonical_agent_identity_key(
            AgentProvider::Opencode,
            OPENCODE_PRODUCT,
            AgentHarness::Alfred,
            opencode::OPENCODE_GO_PROVIDER_ID,
            None,
        );
        let existing = db
            .get_agent_account_by_identity(
                AgentProvider::Opencode,
                OPENCODE_PRODUCT,
                AgentHarness::Alfred,
                &identity,
            )
            .map_err(|_| state_error())?;
        if let Some(existing) = existing.as_ref() {
            if existing.runtime_profile_ref.is_some() {
                self.logout_opencode_account(existing)?;
            }
        }
        let account_id = existing
            .as_ref()
            .map(|account| account.id.clone())
            .unwrap_or_else(|| format!("account_{}", Uuid::new_v4().simple()));
        let account_ref = OpaqueAgentAccountRef::parse(&account_id).map_err(|_| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_account_invalid",
                "The managed runtime account identity is invalid.",
                false,
            )
        })?;
        let cancellation = NativeCancellation::new("opencode-connect", Duration::from_secs(60))
            .map_err(native_command_error)?;
        let manager = opencode::OpenCodeAccountManager::new(servers);
        let profile_ref = manager
            .connect(&account_ref, &catalog, key, &cancellation)
            .map_err(native_command_error)?;
        let metadata = opencode_authorized_account(profile_ref.as_str());
        accounts
            .persist_runtime_managed_account(db, &account_id, metadata)
            .map_err(account_command_error)?;
        self.register_verified_opencode_runtime()?;
        self.connections
            .lock()
            .map_err(|_| state_error())?
            .insert(
                OPENCODE_PRODUCT.as_str().into(),
                ManagedConnection {
                    kind: "api_key".into(),
                    state: "connected".into(),
                    attempt_id: None,
                    authorization_url: None,
                    user_code: None,
                    terminal_session_id: None,
                    account_id: Some(account_id),
                    profile_ref: None,
                    pending_account_id: None,
                    codex_login_id: None,
                },
            );
        self.connection_status(db, accounts, "opencode", OPENCODE_PRODUCT.as_str())
    }

    fn logout_opencode_account(
        &self,
        account: &AgentAccount,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let servers = self.opencode_servers()?;
        let catalog = self.opencode_catalog_root()?;
        let Some(profile) = account.runtime_profile_ref.as_deref() else {
            return Ok(());
        };
        let profile_ref = RuntimeProfileRef::parse(profile).map_err(|_| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_profile_invalid",
                "The managed runtime profile reference is invalid.",
                false,
            )
        })?;
        let account_ref = OpaqueAgentAccountRef::parse(&account.id).map_err(|_| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_account_invalid",
                "The managed runtime account identity is invalid.",
                false,
            )
        })?;
        let cancellation = NativeCancellation::new("opencode-logout", Duration::from_secs(60))
            .map_err(native_command_error)?;
        opencode::OpenCodeAccountManager::new(servers)
            .disconnect(&account_ref, &profile_ref, &catalog, &cancellation)
            .map_err(native_command_error)?;
        self.connections
            .lock()
            .map_err(|_| state_error())?
            .insert(
                OPENCODE_PRODUCT.as_str().into(),
                ManagedConnection {
                    kind: "api_key".into(),
                    state: "disconnected".into(),
                    attempt_id: None,
                    authorization_url: None,
                    user_code: None,
                    terminal_session_id: None,
                    account_id: None,
                    profile_ref: None,
                    pending_account_id: None,
                    codex_login_id: None,
                },
            );
        Ok(())
    }

    fn activate_opencode_selection(
        &self,
        selection: RuntimePackageSelection,
        profiles: &RuntimeProfileStore,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let factory = opencode::OpenCodeManagedServerFactory::new(
            selection.clone(),
            profiles.clone(),
            self.supervisor.clone(),
            self.supervisor_http_bridge(),
        )
        .map_err(native_command_error)?;
        *self.opencode_selection.lock().map_err(|_| state_error())? = Some(selection);
        *self.opencode_servers.lock().map_err(|_| state_error())? = Some(Arc::new(factory));
        Ok(())
    }

    fn refresh_opencode_package(&self) -> Result<(), ManagedRuntimeCommandError> {
        let (packages, profiles, package_root) = {
            let backend = self.backend.lock().map_err(|_| state_error())?;
            let Some(backend) = backend.as_ref() else {
                return Ok(());
            };
            let Some(package_root) =
                first_opencode_package_root(backend.resource_root.as_deref())
            else {
                return Ok(());
            };
            (
                backend.packages.clone(),
                backend.profiles.clone(),
                package_root,
            )
        };
        self.trust_verifier
            .bind_opencode_package(packages, package_root)?;
        let selection = match self.trust_verifier.verify_opencode_selection() {
            Ok(selection) => selection,
            Err(error) => {
                eprintln!("OpenCode managed package present but unverified: {error}");
                return Ok(());
            }
        };
        self.activate_opencode_selection(selection, &profiles)?;
        let _ = self.register_verified_opencode_runtime();
        Ok(())
    }

    fn register_verified_opencode_runtime(&self) -> Result<(), ManagedRuntimeCommandError> {
        let Some(registry) = self
            .native_registry
            .lock()
            .map_err(|_| state_error())?
            .clone()
        else {
            return Ok(());
        };
        let Some(broker) = self.host_broker.lock().map_err(|_| state_error())?.clone() else {
            return Ok(());
        };
        let Ok(servers) = self.opencode_servers() else {
            return Ok(());
        };
        let catalog = self.opencode_catalog_root()?;
        let runtime = opencode::OpenCodeNativeRuntime::new(servers, broker, catalog)
            .map_err(native_command_error)?;
        let _ = registry.unregister(AgentProvider::Opencode);
        registry
            .register(Arc::new(runtime))
            .map_err(native_command_error)?;
        Ok(())
    }

    fn opencode_servers(
        &self,
    ) -> Result<Arc<dyn opencode::OpenCodeServerProvider>, ManagedRuntimeCommandError> {
        self.opencode_servers
            .lock()
            .map_err(|_| state_error())?
            .clone()
            .ok_or_else(|| {
                ManagedRuntimeCommandError::new(
                    opencode::PACKAGE_GATE_CODE,
                    "The verified OpenCode Go package is not available in this build.",
                    false,
                )
            })
    }

    fn opencode_catalog_root(&self) -> Result<PathBuf, ManagedRuntimeCommandError> {
        self.backend
            .lock()
            .map_err(|_| state_error())?
            .as_ref()
            .map(|backend| backend.catalog_root.clone())
            .ok_or_else(state_error)
    }

    fn opencode_runtime_ready(&self) -> bool {
        self.opencode_servers
            .lock()
            .ok()
            .is_some_and(|servers| servers.is_some())
            || self
                .opencode_selection
                .lock()
                .ok()
                .is_some_and(|selection| selection.is_some())
    }

    fn opencode_process_gates_waived(&self) -> bool {
        self.waive_opencode_process_gates.load(Ordering::SeqCst)
    }

    fn persisted_managed_account(
        &self,
        db: &Db,
        provider: AgentProvider,
        product: AgentProductId,
    ) -> Option<AgentAccount> {
        db.list_agent_accounts().ok()?.into_iter().find(|account| {
            account.provider == provider
                && account.product == product
                && account.status == AgentAccountStatus::Connected
                && account.runtime_profile_ref.is_some()
        })
    }

    fn record_opencode_error(&self, code: &str) {
        if let Ok(mut connections) = self.connections.lock() {
            connections.insert(
                OPENCODE_PRODUCT.as_str().into(),
                ManagedConnection {
                    kind: "api_key".into(),
                    state: "error".into(),
                    attempt_id: None,
                    authorization_url: None,
                    user_code: None,
                    terminal_session_id: None,
                    account_id: None,
                    profile_ref: None,
                    pending_account_id: None,
                    codex_login_id: None,
                },
            );
        }
        let _ = code;
    }

    fn terminal(
        &self,
        session_id: &str,
    ) -> Result<Arc<ClaudeTerminalSession>, ManagedRuntimeCommandError> {
        if !valid_terminal_session_id(session_id) {
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_terminal_session_invalid",
                "The managed terminal session is invalid.",
                false,
            ));
        }
        self.terminal_sessions
            .lock()
            .map_err(|_| state_error())?
            .get(session_id)
            .cloned()
            .ok_or_else(|| {
                ManagedRuntimeCommandError::new(
                    "managed_runtime_terminal_not_found",
                    "The managed terminal session no longer exists.",
                    false,
                )
            })
    }

    fn read_terminal(
        &self,
        session_id: &str,
        cursor: u64,
    ) -> Result<ManagedRuntimeTerminalReadDto, ManagedRuntimeCommandError> {
        let session = self.terminal(session_id)?;
        // Polling is deliberately non-blocking at the public boundary. The
        // transport timeout is an internal policy, not a command argument.
        let output = session
            .read_output(Duration::ZERO)
            .map_err(terminal_command_error)?;
        let closed = session.snapshot().lifecycle != claude::ClaudeTerminalLifecycle::Running;
        Ok(match output {
            Some(output) => ManagedRuntimeTerminalReadDto {
                session_id: output.session_id.as_str().to_owned(),
                cursor: output.sequence.saturating_add(1),
                output: output.data_base64,
                closed,
            },
            None => ManagedRuntimeTerminalReadDto {
                session_id: session_id.to_owned(),
                cursor,
                output: String::new(),
                closed,
            },
        })
    }

    fn write_terminal(
        &self,
        session_id: &str,
        input: &str,
    ) -> Result<(), ManagedRuntimeCommandError> {
        if input.len() > 128 * 1024 {
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_terminal_input_invalid",
                "The terminal input is too large.",
                false,
            ));
        }
        self.terminal(session_id)?
            .write_input(input.as_bytes())
            .map_err(terminal_command_error)
    }

    fn resize_terminal(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), ManagedRuntimeCommandError> {
        self.terminal(session_id)?
            // Pixel dimensions are provider-internal and fixed at the
            // command boundary; callers can only request terminal cells.
            .resize(cols, rows, 0, 0)
            .map_err(terminal_command_error)
    }

    fn close_terminal(&self, session_id: &str) -> Result<(), ManagedRuntimeCommandError> {
        let session = self.terminal(session_id)?;
        session.cancel().map_err(terminal_command_error)?;
        self.terminal_sessions
            .lock()
            .map_err(|_| state_error())?
            .remove(session_id);
        Ok(())
    }

    /// Backend-only insertion point for the Claude provider session manager.
    /// A session is accepted only with the exact opaque id produced by the
    /// provider PTY implementation.
    pub(crate) fn track_claude_terminal(
        &self,
        session: ClaudeTerminalSession,
    ) -> Result<String, ManagedRuntimeCommandError> {
        let id = session.id().as_str().to_owned();
        self.terminal_sessions
            .lock()
            .map_err(|_| state_error())?
            .insert(id.clone(), Arc::new(session));
        Ok(id)
    }

    /// Starts a provider-owned Claude PTY only after its caller has obtained a
    /// sealed package selection and an active, matching profile. The session
    /// itself remains behind the backend command relay.
    pub(crate) fn start_claude_terminal(
        &self,
        runtime: &ClaudeCodeSubscriptionRuntime,
        package: &RuntimePackageSelection,
        profile: &crate::agent_accounts::runtime_profile::RuntimeProfile,
        spec: ClaudeTerminalLaunchSpec,
    ) -> Result<String, ManagedRuntimeCommandError> {
        let session = runtime
            .start_terminal(package, profile, spec)
            .map_err(terminal_command_error)?;
        self.track_claude_terminal(session)
    }

    fn begin_opencode_api_key_connection(
        &self,
    ) -> Result<ManagedRuntimeConnectionStartDto, ManagedRuntimeCommandError> {
        self.store_connection(
            OPENCODE_PRODUCT,
            ManagedConnection {
                kind: "api_key".into(),
                state: "connecting".into(),
                ..ManagedConnection::default()
            },
        )?;
        Ok(ManagedRuntimeConnectionStartDto {
            kind: "api_key".into(),
            attempt_id: None,
            authorization_url: None,
            user_code: None,
            expires_at: None,
            terminal_session_id: None,
        })
    }

    fn start_claude_oauth(
        &self,
        spec: &ManagedProductSpec,
    ) -> Result<ManagedRuntimeConnectionStartDto, ManagedRuntimeCommandError> {
        let (account_id, account_ref, profile) = self.create_oauth_profile(spec)?;
        let terminal_session_id = if self.claude_oauth_fixture() {
            format!("claude_terminal_{}", Uuid::new_v4().simple())
        } else {
            let selection = self.claude_selection()?;
            let working_directory = profile.launch_home_root().to_path_buf();
            let runtime = ClaudeCodeSubscriptionRuntime::new(self.supervisor.clone());
            let session = runtime
                .start_auth_login(&selection, &profile, &working_directory, 80, 24)
                .map_err(terminal_command_error)?;
            self.track_claude_terminal(session)?
        };
        self.store_connection(
            CLAUDE_PRODUCT,
            ManagedConnection {
                kind: "terminal".into(),
                state: "connecting".into(),
                terminal_session_id: Some(terminal_session_id.clone()),
                pending_account_id: Some(account_id),
                profile_ref: Some(profile.profile_ref().as_str().to_owned()),
                ..ManagedConnection::default()
            },
        )?;
        let _ = account_ref;
        Ok(ManagedRuntimeConnectionStartDto {
            kind: "terminal".into(),
            attempt_id: None,
            authorization_url: None,
            user_code: None,
            expires_at: None,
            terminal_session_id: Some(terminal_session_id),
        })
    }

    fn start_codex_oauth(
        &self,
        spec: &ManagedProductSpec,
    ) -> Result<ManagedRuntimeConnectionStartDto, ManagedRuntimeCommandError> {
        let (account_id, _, profile) = self.create_oauth_profile(spec)?;
        let (authorization_url, login_id) = if self.codex_oauth_fixture() {
            (
                "https://chatgpt.com/auth/codex".to_owned(),
                "login_browser_1".to_owned(),
            )
        } else {
            let selection = self.codex_selection()?;
            let working_directory = profile
                .environment_roots()
                .get(RuntimeEnvironmentVariable::CodexHome)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| profile.launch_home_root().to_path_buf());
            let connection = launch_codex_sdk_login(
                &self.supervisor,
                &selection,
                &profile,
                &working_directory,
                ManagedRuntimeCancellation::new(),
            )
            .map_err(codex_command_error)?;
            let prompt = connection
                .start_chatgpt_login(Duration::from_secs(20))
                .map_err(codex_command_error)?;
            self.codex_sessions
                .lock()
                .map_err(|_| state_error())?
                .insert(CODEX_PRODUCT.as_str().into(), connection);
            open_chatgpt_sign_in_url(&prompt.authorization_url);
            (prompt.authorization_url, prompt.login_id)
        };
        self.store_connection(
            CODEX_PRODUCT,
            ManagedConnection {
                kind: "browser".into(),
                state: "connecting".into(),
                authorization_url: Some(authorization_url.clone()),
                pending_account_id: Some(account_id),
                profile_ref: Some(profile.profile_ref().as_str().to_owned()),
                codex_login_id: Some(login_id),
                ..ManagedConnection::default()
            },
        )?;
        Ok(ManagedRuntimeConnectionStartDto {
            kind: "browser".into(),
            attempt_id: None,
            authorization_url: Some(authorization_url),
            user_code: None,
            expires_at: None,
            terminal_session_id: None,
        })
    }

    fn poll_pending_oauth(
        &self,
        db: &Db,
        accounts: &AgentAccountsState,
        spec: &ManagedProductSpec,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let connection = self
            .connections
            .lock()
            .map_err(|_| state_error())?
            .get(spec.product.as_str())
            .cloned();
        let Some(connection) = connection else {
            return Ok(());
        };
        if connection.state != "connecting" {
            return Ok(());
        }
        match spec.product {
            CLAUDE_PRODUCT => self.complete_claude_oauth(db, accounts, spec, &connection),
            CODEX_PRODUCT => self.complete_codex_oauth(db, accounts, spec, &connection),
            _ => Ok(()),
        }
    }

    fn complete_claude_oauth(
        &self,
        db: &Db,
        accounts: &AgentAccountsState,
        spec: &ManagedProductSpec,
        connection: &ManagedConnection,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let Some(profile_ref) = connection.profile_ref.as_deref() else {
            return Ok(());
        };
        let Some(account_id) = connection.pending_account_id.as_deref() else {
            return Ok(());
        };
        let status = if self.claude_oauth_fixture() {
            if !self.oauth_fixtures.claude_complete.load(Ordering::SeqCst) {
                return Ok(());
            }
            None
        } else {
            let selection = self.claude_selection()?;
            let profile = self.open_oauth_profile(spec, account_id, profile_ref)?;
            let queried = ClaudeCodeSubscriptionRuntime::new(self.supervisor.clone())
                .auth_status(
                    &selection,
                    &profile,
                    ManagedRuntimeCancellation::new(),
                )
                .map_err(|error| {
                    ManagedRuntimeCommandError::new(
                        error.code().as_str(),
                        "Claude subscription status could not be read.",
                        true,
                    )
                })?;
            if queried.api_key_takes_precedence() {
                self.store_connection(
                    CLAUDE_PRODUCT,
                    ManagedConnection {
                        kind: "terminal".into(),
                        state: "limited".into(),
                        terminal_session_id: connection.terminal_session_id.clone(),
                        pending_account_id: Some(account_id.to_owned()),
                        profile_ref: Some(profile_ref.to_owned()),
                        ..ManagedConnection::default()
                    },
                )?;
                return Ok(());
            }
            if !queried.logged_in || !queried.is_subscription_billed() {
                return Ok(());
            }
            Some(queried)
        };
        let _ = status;
        self.persist_oauth_account(
            db,
            accounts,
            spec,
            account_id,
            profile_ref,
            claude_authorized_account(profile_ref),
            connection.terminal_session_id.clone(),
        )
    }

    fn complete_codex_oauth(
        &self,
        db: &Db,
        accounts: &AgentAccountsState,
        spec: &ManagedProductSpec,
        connection: &ManagedConnection,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let Some(profile_ref) = connection.profile_ref.as_deref() else {
            return Ok(());
        };
        let Some(account_id) = connection.pending_account_id.as_deref() else {
            return Ok(());
        };
        if self.codex_oauth_fixture() {
            if !self.oauth_fixtures.codex_complete.load(Ordering::SeqCst) {
                return Ok(());
            }
        } else {
            let Some(login_id) = connection.codex_login_id.as_deref() else {
                return Ok(());
            };
            let sessions = self.codex_sessions.lock().map_err(|_| state_error())?;
            let Some(session) = sessions.get(CODEX_PRODUCT.as_str()) else {
                return Ok(());
            };
            let account = session
                .poll_chatgpt_account(login_id, Duration::from_millis(200))
                .map_err(codex_command_error)?;
            let Some(account) = account else {
                return Ok(());
            };
            if !account.authenticated || account.auth_mode.as_deref() != Some("chatgpt") {
                return Ok(());
            }
        }
        self.persist_oauth_account(
            db,
            accounts,
            spec,
            account_id,
            profile_ref,
            codex_authorized_account(profile_ref),
            None,
        )
    }

    fn persist_oauth_account(
        &self,
        db: &Db,
        accounts: &AgentAccountsState,
        spec: &ManagedProductSpec,
        account_id: &str,
        profile_ref: &str,
        metadata: AuthorizedAgentAccount,
        terminal_session_id: Option<String>,
    ) -> Result<(), ManagedRuntimeCommandError> {
        accounts
            .persist_runtime_managed_account(db, account_id, metadata)
            .map_err(account_command_error)?;
        self.store_connection(
            spec.product,
            ManagedConnection {
                kind: spec.connection_kind.into(),
                state: "connected".into(),
                account_id: Some(account_id.to_owned()),
                profile_ref: Some(profile_ref.to_owned()),
                terminal_session_id,
                ..ManagedConnection::default()
            },
        )
    }

    fn create_oauth_profile(
        &self,
        spec: &ManagedProductSpec,
    ) -> Result<(String, OpaqueAgentAccountRef, RuntimeProfile), ManagedRuntimeCommandError> {
        let account_id = format!("account_{}", Uuid::new_v4().simple());
        let account_ref = OpaqueAgentAccountRef::parse(&account_id).map_err(|_| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_account_invalid",
                "The managed runtime account identity is invalid.",
                false,
            )
        })?;
        let binding = RuntimeProfileBinding::new(
            &account_ref,
            spec.product,
            spec.runtime_id,
            spec.runtime_version,
        )
        .map_err(|error| profile_command_error(error.code().as_str().into(), false))?;
        let profiles = self.profiles()?;
        let profile = profiles
            .create(&binding)
            .map_err(|error| profile_command_error(error.code().as_str().into(), true))?;
        Ok((account_id, account_ref, profile))
    }

    fn open_oauth_profile(
        &self,
        spec: &ManagedProductSpec,
        account_id: &str,
        profile_ref: &str,
    ) -> Result<RuntimeProfile, ManagedRuntimeCommandError> {
        let account_ref = OpaqueAgentAccountRef::parse(account_id).map_err(|_| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_account_invalid",
                "The managed runtime account identity is invalid.",
                false,
            )
        })?;
        let parsed = RuntimeProfileRef::parse(profile_ref)
            .map_err(|error| profile_command_error(error.code().as_str().into(), false))?;
        let binding = RuntimeProfileBinding::new(
            &account_ref,
            spec.product,
            spec.runtime_id,
            spec.runtime_version,
        )
        .map_err(|error| profile_command_error(error.code().as_str().into(), false))?;
        self.profiles()?
            .open(&parsed, &binding)
            .map_err(|error| profile_command_error(error.code().as_str().into(), true))
    }

    fn logout_claude_account(
        &self,
        account: &AgentAccount,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let Some(profile_ref) = account.runtime_profile_ref.as_deref() else {
            return Ok(());
        };
        if !self.claude_oauth_fixture() {
            if let (Ok(selection), Ok(profile)) = (
                self.claude_selection(),
                self.open_oauth_profile(
                    &product_spec(account.provider.as_str(), account.product.as_str())?,
                    &account.id,
                    profile_ref,
                ),
            ) {
                let working_directory = profile.launch_home_root().to_path_buf();
                let runtime = ClaudeCodeSubscriptionRuntime::new(self.supervisor.clone());
                if let Ok(session) = runtime.start_auth_logout(
                    &selection,
                    &profile,
                    &working_directory,
                    80,
                    24,
                ) {
                    let id = self.track_claude_terminal(session)?;
                    let _ = self.close_terminal(&id);
                }
            }
        }
        self.purge_oauth_profile(account)?;
        self.store_connection(
            CLAUDE_PRODUCT,
            ManagedConnection {
                kind: "terminal".into(),
                state: "disconnected".into(),
                ..ManagedConnection::default()
            },
        )
    }

    fn logout_codex_account(
        &self,
        account: &AgentAccount,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let Some(profile_ref) = account.runtime_profile_ref.as_deref() else {
            return Ok(());
        };
        if !self.codex_oauth_fixture() {
            let spec = product_spec(account.provider.as_str(), account.product.as_str())?;
            let profile = self.open_oauth_profile(&spec, &account.id, profile_ref)?;
            if let Some(connection) = self
                .codex_sessions
                .lock()
                .map_err(|_| state_error())?
                .remove(CODEX_PRODUCT.as_str())
            {
                let receipt = connection
                    .logout_and_stop("logout_1", &profile, Duration::from_secs(10))
                    .map_err(codex_command_error)?;
                purge_logged_out_codex_profile(&self.profiles()?, receipt)
                    .map_err(codex_command_error)?;
            } else {
                self.purge_oauth_profile(account)?;
            }
        } else {
            self.purge_oauth_profile(account)?;
        }
        self.store_connection(
            CODEX_PRODUCT,
            ManagedConnection {
                kind: "browser".into(),
                state: "disconnected".into(),
                ..ManagedConnection::default()
            },
        )
    }

    fn purge_oauth_profile(
        &self,
        account: &AgentAccount,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let Some(profile_ref) = account.runtime_profile_ref.as_deref() else {
            return Ok(());
        };
        let spec = product_spec(account.provider.as_str(), account.product.as_str())?;
        let account_ref = OpaqueAgentAccountRef::parse(&account.id).map_err(|_| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_account_invalid",
                "The managed runtime account identity is invalid.",
                false,
            )
        })?;
        let parsed = RuntimeProfileRef::parse(profile_ref)
            .map_err(|error| profile_command_error(error.code().as_str().into(), false))?;
        let binding = RuntimeProfileBinding::new(
            &account_ref,
            spec.product,
            spec.runtime_id,
            spec.runtime_version,
        )
        .map_err(|error| profile_command_error(error.code().as_str().into(), false))?;
        match self.profiles()?.purge(&parsed, &binding) {
            Ok(()) => Ok(()),
            Err(error)
                if error.code()
                    == crate::agent_accounts::runtime_profile::RuntimeProfileErrorCode::ProfileNotFound =>
            {
                Ok(())
            }
            Err(error) => Err(profile_command_error(error.code().as_str().into(), true)),
        }
    }

    fn store_connection(
        &self,
        product: AgentProductId,
        connection: ManagedConnection,
    ) -> Result<(), ManagedRuntimeCommandError> {
        self.connections
            .lock()
            .map_err(|_| state_error())?
            .insert(product.as_str().into(), connection);
        Ok(())
    }

    fn profiles(&self) -> Result<RuntimeProfileStore, ManagedRuntimeCommandError> {
        self.backend
            .lock()
            .map_err(|_| state_error())?
            .as_ref()
            .map(|backend| backend.profiles.clone())
            .ok_or_else(state_error)
    }

    fn claude_selection(&self) -> Result<RuntimePackageSelection, ManagedRuntimeCommandError> {
        self.claude_selection
            .lock()
            .map_err(|_| state_error())?
            .clone()
            .ok_or_else(|| {
                ManagedRuntimeCommandError::new(
                    claude::PACKAGE_INTEGRATION_BLOCKED_CODE,
                    "The verified Claude Code package is not available in this build.",
                    false,
                )
            })
    }

    fn codex_selection(&self) -> Result<RuntimePackageSelection, ManagedRuntimeCommandError> {
        self.codex_selection
            .lock()
            .map_err(|_| state_error())?
            .clone()
            .ok_or_else(|| {
                ManagedRuntimeCommandError::new(
                    codex::SEALED_PACKAGE_BLOCKER,
                    "The verified Codex runtime package is not available in this build.",
                    false,
                )
            })
    }

    fn claude_connect_ready(&self) -> bool {
        self.claude_oauth_fixture()
            || self
                .claude_selection
                .lock()
                .ok()
                .is_some_and(|selection| selection.is_some())
    }

    fn codex_connect_ready(&self) -> bool {
        self.codex_oauth_fixture()
            || self
                .codex_selection
                .lock()
                .ok()
                .is_some_and(|selection| selection.is_some())
    }

    fn claude_oauth_fixture(&self) -> bool {
        self.oauth_fixtures.claude.load(Ordering::SeqCst)
    }

    fn codex_oauth_fixture(&self) -> bool {
        self.oauth_fixtures.codex.load(Ordering::SeqCst)
    }

    fn refresh_claude_package(&self) -> Result<(), ManagedRuntimeCommandError> {
        let (packages, package_root) = {
            let backend = self.backend.lock().map_err(|_| state_error())?;
            let Some(backend) = backend.as_ref() else {
                return Ok(());
            };
            let Some(package_root) =
                first_claude_package_root(backend.resource_root.as_deref())
            else {
                return Ok(());
            };
            (backend.packages.clone(), package_root)
        };
        self.trust_verifier
            .bind_claude_package(packages, package_root)?;
        match self.trust_verifier.verify_claude_selection() {
            Ok(selection) => {
                *self.claude_selection.lock().map_err(|_| state_error())? = Some(selection);
            }
            Err(error) => eprintln!("Claude managed package present but unverified: {error}"),
        }
        Ok(())
    }

    fn refresh_codex_package(&self) -> Result<(), ManagedRuntimeCommandError> {
        let (packages, package_root) = {
            let backend = self.backend.lock().map_err(|_| state_error())?;
            let Some(backend) = backend.as_ref() else {
                return Ok(());
            };
            let Some(package_root) =
                first_codex_package_root(backend.resource_root.as_deref())
            else {
                return Ok(());
            };
            (backend.packages.clone(), package_root)
        };
        self.trust_verifier
            .bind_codex_package(packages, package_root)?;
        match self.trust_verifier.verify_codex_selection() {
            Ok(selection) => {
                *self.codex_selection.lock().map_err(|_| state_error())? = Some(selection);
            }
            Err(error) => eprintln!("ChatGPT managed package present but unverified: {error}"),
        }
        Ok(())
    }
}

impl Default for ManagedRuntimeControlPlane {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct ManagedRuntimeSupervisorHttpBridge;

impl opencode::OpenCodeSupervisorHttpBridge for ManagedRuntimeSupervisorHttpBridge {
    fn launch_authenticated(
        &self,
        supervisor: &ManagedRuntimeSupervisor,
        package: &RuntimePackageSelection,
        profile: &crate::agent_accounts::runtime_profile::RuntimeProfile,
        spec: crate::agents::managed_runtime::ManagedRuntimeLaunchSpec,
        address: std::net::SocketAddr,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        cancellation.checkpoint()?;
        let (handle, password) = supervisor
            .launch_opencode_authenticated(
                package,
                profile,
                spec,
                ManagedRuntimeCancellation::new(),
            )
            .map_err(|error| {
                NativeRuntimeError::new(
                    NativeErrorCode::ProviderUnavailable,
                    format!(
                        "managed OpenCode runtime launch failed: {}",
                        error.code().as_str()
                    ),
                    true,
                )
            })?;
        cancellation.checkpoint()?;
        let password = opencode::OpenCodeServerPassword::new(password)?;
        opencode::ManagedOpenCodeServerSession::new(handle, address, password)
            .map(|session| Box::new(session) as Box<dyn OpenCodeServerSession>)
    }
}

struct ManagedRuntimeAccountProvider {
    control_plane: Arc<ManagedRuntimeControlPlane>,
    spec: ManagedProductSpec,
}

impl AgentAccountProvider for ManagedRuntimeAccountProvider {
    fn registration(&self) -> AgentProviderRegistration {
        AgentProviderRegistration {
            provider: self.spec.provider,
            product: self.spec.product,
            harness: AgentHarness::Alfred,
            auth_method: self.spec.product.auth_method(),
            custody_mode: self.spec.product.custody_mode(),
            managed_runtime_id: Some(self.spec.runtime_id),
            managed_runtime_version: Some(self.spec.runtime_version.into()),
            // Registration is visible for diagnostics, but remains blocked;
            // capability gates are still checked independently by commands.
            gate_code: Some(self.spec.primary_gate_code.into()),
        }
    }

    fn start_authorization(&self) -> Result<ProviderAuthorizationStart, AgentProviderError> {
        Err(self.blocked_provider_error())
    }

    fn complete_authorization<'a>(
        &'a self,
        _attempt: AuthorizationAttempt,
    ) -> ProviderFuture<'a, ProviderAccountGrant> {
        Box::pin(async move { Err(self.blocked_provider_error()) })
    }

    fn refresh<'a>(
        &'a self,
        _account: &'a AgentAccount,
        _access: ProviderAccountAccess,
    ) -> ProviderFuture<'a, ProviderAccountAccess> {
        Box::pin(async move { Err(self.blocked_provider_error()) })
    }

    fn revoke<'a>(
        &'a self,
        account: &'a AgentAccount,
        _access: ProviderAccountAccess,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            match self.spec.product {
                OPENCODE_PRODUCT => self
                    .control_plane
                    .logout_opencode_account(account)
                    .map_err(|error| {
                        AgentProviderError::new(&error.code, AgentProviderFailureKind::Retryable)
                    }),
                CLAUDE_PRODUCT => self
                    .control_plane
                    .logout_claude_account(account)
                    .map_err(|error| {
                        AgentProviderError::new(&error.code, AgentProviderFailureKind::Retryable)
                    }),
                CODEX_PRODUCT => self
                    .control_plane
                    .logout_codex_account(account)
                    .map_err(|error| {
                        AgentProviderError::new(&error.code, AgentProviderFailureKind::Retryable)
                    }),
                _ => Err(self.blocked_provider_error()),
            }
        })
    }
}

impl ManagedRuntimeAccountProvider {
    fn blocked_provider_error(&self) -> AgentProviderError {
        let _ = &self.control_plane;
        AgentProviderError::new(
            self.spec.primary_gate_code,
            AgentProviderFailureKind::PolicyDenied,
        )
    }
}

/// Shared provider verifier implementation.  Provider modules own their
/// pinned manifests and call their own request builders; this type is the
/// only production adapter that can turn independent platform evidence into
/// a sealed package verification capability.
pub struct ManagedRuntimeTrustVerifier {
    platform: Arc<dyn RuntimePlatformPublisherVerifier>,
    opencode_package: Mutex<Option<(RuntimePackageStore, PathBuf)>>,
    claude_package: Mutex<Option<(RuntimePackageStore, PathBuf)>>,
    codex_package: Mutex<Option<(RuntimePackageStore, PathBuf)>>,
}

impl ManagedRuntimeTrustVerifier {
    pub fn new(platform: Arc<dyn RuntimePlatformPublisherVerifier>) -> Self {
        Self {
            platform,
            opencode_package: Mutex::new(None),
            claude_package: Mutex::new(None),
            codex_package: Mutex::new(None),
        }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableRuntimePlatformPublisherVerifier))
    }

    pub fn with_opencode_package(
        self,
        store: RuntimePackageStore,
        package_root: impl Into<PathBuf>,
    ) -> Self {
        if let Ok(mut slot) = self.opencode_package.lock() {
            *slot = Some((store, package_root.into()));
        }
        self
    }

    fn bind_claude_package(
        &self,
        store: RuntimePackageStore,
        package_root: PathBuf,
    ) -> Result<(), ManagedRuntimeCommandError> {
        *self.claude_package.lock().map_err(|_| state_error())? = Some((store, package_root));
        Ok(())
    }

    fn bind_codex_package(
        &self,
        store: RuntimePackageStore,
        package_root: PathBuf,
    ) -> Result<(), ManagedRuntimeCommandError> {
        *self.codex_package.lock().map_err(|_| state_error())? = Some((store, package_root));
        Ok(())
    }

    fn verify_claude_selection(&self) -> Result<RuntimePackageSelection, NativeRuntimeError> {
        let target = host_runtime_target().ok_or_else(native_claude_package_gate)?;
        let bound = self
            .claude_package
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        let Some((store, package_root)) = bound else {
            return Err(native_claude_package_gate());
        };
        let (manifest, signature) = load_claude_publisher_evidence(&package_root)?;
        let verification = claude::verify_package_for_install(
            &package_root,
            target,
            &manifest,
            &signature,
            self,
        )
        .map_err(|error| {
            package_gate("claude:verify_package_for_install", error, native_claude_package_gate)
        })?;
        claude::stage_and_select_verified_package(&store, &package_root, &verification)
            .map_err(|error| package_gate("claude:stage_and_select", error, native_claude_package_gate))
    }

    fn verify_codex_selection(&self) -> Result<RuntimePackageSelection, NativeRuntimeError> {
        let target = host_runtime_target().ok_or_else(native_codex_package_gate)?;
        let bound = self
            .codex_package
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        let Some((store, package_root)) = bound else {
            return Err(native_codex_package_gate());
        };
        let inputs = load_codex_package_inputs(&package_root)?;
        let verification = codex::verify_codex_sdk_package(
            &package_root,
            target,
            &inputs.source_manifest,
            &inputs.target_sbom,
            &inputs.license,
            &inputs.notice,
            self,
        )
        .map_err(|error| {
            package_gate("codex:verify_codex_sdk_package", error, native_codex_package_gate)
        })?;
        store
            .stage_and_activate(&package_root, &verification, None)
            .map_err(|error| package_gate("codex:stage_and_activate", error, native_codex_package_gate))?;
        store
            .select_active(&verification)
            .map_err(|error| package_gate("codex:select_active", error, native_codex_package_gate))
    }

    fn bind_opencode_package(
        &self,
        store: RuntimePackageStore,
        package_root: PathBuf,
    ) -> Result<(), ManagedRuntimeCommandError> {
        *self.opencode_package.lock().map_err(|_| state_error())? = Some((store, package_root));
        Ok(())
    }

    fn verify_opencode_selection(&self) -> Result<RuntimePackageSelection, NativeRuntimeError> {
        let target = opencode::current_runtime_target().ok_or_else(native_package_gate)?;
        opencode::select_verified_package(self, target)
    }
}

impl Default for ManagedRuntimeTrustVerifier {
    fn default() -> Self {
        Self::unavailable()
    }
}

impl claude::ClaudePublisherVerifier for ManagedRuntimeTrustVerifier {
    fn verify(
        &self,
        request: ClaudePublisherVerificationRequest<'_>,
    ) -> Result<RuntimePackageVerification, ClaudePublisherVerificationError> {
        let Ok(target) = request
            .package_manifest
            .select_target(request.expectation.target())
        else {
            return Err(ClaudePublisherVerificationError);
        };
        if request.artifact.runtime_target != request.expectation.target()
            || target.executable.sha256 != request.artifact.sha256
            || target.publisher_verification.publisher != request.artifact.publisher
            || request.signing_key_fingerprint != claude::CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT
        {
            return Err(ClaudePublisherVerificationError);
        }
        verify_runtime_package_with_platform_evidence(
            request.package_root,
            request.package_manifest,
            request.expectation,
            request.publisher_release_manifest,
            request.detached_manifest_signature,
            self.platform.as_ref(),
        )
        .map_err(|_| ClaudePublisherVerificationError)
    }
}

impl CodexSdkPackageVerifier for ManagedRuntimeTrustVerifier {
    fn verify(
        &self,
        request: CodexSdkVerifierRequest<'_>,
    ) -> Result<RuntimePackageVerification, CodexSdkPackageError> {
        if request.sdk_source_commit != codex::CODEX_SDK_SOURCE_COMMIT
            || request.sdk_wheel_sha256 != codex::CODEX_SDK_WHEEL_SHA256
            || request.cli_wheel.target != request.expectation.target()
            || request.expectation.product() != CODEX_PRODUCT
            || request.expectation.runtime_id() != ManagedRuntimeId::CodexPythonSdk
            || request.expectation.runtime_version() != codex::CODEX_SDK_RUNTIME_VERSION
        {
            return Err(CodexSdkPackageError::SelectionMismatch);
        }
        let manifest = codex::package_manifest();
        verify_runtime_package_with_platform_evidence(
            request.package_root,
            &manifest,
            request.expectation,
            request.target_sbom,
            request.cli_wheel.sha256.as_bytes(),
            self.platform.as_ref(),
        )
        .map_err(|_| CodexSdkPackageError::SealedVerificationUnavailable)
    }
}

impl OpenCodePackageVerifier for ManagedRuntimeTrustVerifier {
    fn select_verified_active(
        &self,
        manifest: &RuntimePackageManifest,
        expectation: &crate::agents::runtime_package::RuntimePackageExpectation,
        release: opencode::OpenCodeReleaseArtifact,
    ) -> Result<RuntimePackageSelection, crate::agents::native::NativeRuntimeError> {
        if expectation.product() != OPENCODE_PRODUCT
            || expectation.runtime_id()
                != crate::agent_accounts::models::ManagedRuntimeId::OpencodeServer
            || expectation.runtime_version() != opencode::OPENCODE_RUNTIME_VERSION
            || manifest != &opencode::package_manifest()
        {
            return Err(native_package_gate());
        }
        let bound = self
            .opencode_package
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        let Some((store, package_root)) = bound else {
            return Err(native_package_gate());
        };
        let target = manifest
            .select_target(expectation.target())
            .map_err(|_| native_package_gate())?;
        if target.executable.sha256 != release.executable_sha256 {
            return Err(native_package_gate());
        }
        let (proof, signature) = load_opencode_publisher_evidence(&package_root)?;
        let verification = verify_runtime_package_with_platform_evidence(
            &package_root,
            manifest,
            expectation,
            &proof,
            &signature,
            self.platform.as_ref(),
        )
        .map_err(|_| native_package_gate())?;
        store
            .stage_and_activate(&package_root, &verification, None)
            .map_err(|_| native_package_gate())?;
        store
            .select_active(&verification)
            .map_err(|_| native_package_gate())
    }
}

fn native_package_gate() -> crate::agents::native::NativeRuntimeError {
    crate::agents::native::NativeRuntimeError::new(
        crate::agents::native::NativeErrorCode::ProviderUnavailable,
        opencode::PACKAGE_GATE_CODE,
        false,
    )
}

fn load_opencode_publisher_evidence(
    package_root: &Path,
) -> Result<(Vec<u8>, Vec<u8>), NativeRuntimeError> {
    let evidence_root = package_root
        .parent()
        .ok_or_else(native_package_gate)?
        .join("publisher-evidence");
    let proof = fs::read(evidence_root.join("publisher-verification.json"))
        .map_err(|_| native_package_gate())?;
    let signature = fs::read(evidence_root.join("publisher.sig")).map_err(|_| native_package_gate())?;
    if proof.is_empty() || proof.len() > 1024 * 1024 || signature.len() != 64 {
        return Err(native_package_gate());
    }
    Ok((proof, signature))
}

fn managed_runtime_bundle_roots(resource_root: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = resource_root {
        roots.push(root.join("managed-runtimes"));
        roots.push(root.join("sidecars").join("managed-runtimes"));
    }
    #[cfg(all(debug_assertions, not(test)))]
    {
        let sidecar = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("sidecars")
            .join("managed-runtimes");
        if !roots.iter().any(|existing| existing == &sidecar) {
            roots.push(sidecar);
        }
    }
    roots
}

fn first_opencode_package_root(resource_root: Option<&Path>) -> Option<PathBuf> {
    let target = opencode::current_runtime_target()?;
    managed_runtime_bundle_roots(resource_root)
        .into_iter()
        .map(|root| {
            root.join("opencode_server")
                .join(target)
                .join(opencode::OPENCODE_RUNTIME_VERSION)
                .join("package")
        })
        .find(|package_root| package_root.is_dir())
}

fn first_claude_package_root(resource_root: Option<&Path>) -> Option<PathBuf> {
    let target = host_runtime_target()?;
    managed_runtime_bundle_roots(resource_root)
        .into_iter()
        .map(|root| {
            root.join("claude_code_managed")
                .join(target)
                .join(claude::CLAUDE_CODE_RUNTIME_VERSION)
                .join("package")
        })
        .find(|package_root| package_root.is_dir())
}

fn first_codex_package_root(resource_root: Option<&Path>) -> Option<PathBuf> {
    let target = host_runtime_target()?;
    managed_runtime_bundle_roots(resource_root)
        .into_iter()
        .map(|root| {
            root.join("codex_python_sdk")
                .join(target)
                .join(codex::CODEX_SDK_RUNTIME_VERSION)
                .join("package")
        })
        .find(|package_root| package_root.is_dir())
}

fn host_runtime_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

pub const CLAUDE_SINGLE_ACCOUNT_CODE: &str = "claude_single_account_required";

fn claude_connect_gate_filter() -> Vec<String> {
    vec![
        claude::COMMERCIAL_TERMS_BLOCKED_CODE.into(),
        claude::PACKAGED_NO_CLI_SMOKE_BLOCKED_CODE.into(),
        claude::WORKFLOW_RENDERER_APPROVAL_BLOCKED_CODE.into(),
    ]
}

fn codex_connect_gate_filter() -> Vec<String> {
    vec![
        codex::PUBLIC_CAPABILITY_AUDIT_BLOCKER.into(),
        codex::CODEX_SDK_HOST_APPROVAL_BLOCKER.into(),
        codex::KNOWN_CLIENT_ENTERPRISE_BLOCKER.into(),
        codex::PACKAGED_SMOKE_BLOCKER.into(),
    ]
}

fn claude_authorized_account(profile_ref: &str) -> AuthorizedAgentAccount {
    AuthorizedAgentAccount {
        provider: AgentProvider::ClaudeCode,
        product: CLAUDE_PRODUCT,
        harness: AgentHarness::Alfred,
        display_name: Some("Claude Code subscription".into()),
        external_account_id: "claude.ai".into(),
        external_workspace_id: None,
        auth_method: AgentAuthMethod::Runtime,
        custody_mode: CredentialCustodyMode::RuntimeManaged,
        managed_runtime_id: Some(ManagedRuntimeId::ClaudeCodeManaged),
        managed_runtime_version: Some(claude::CLAUDE_CODE_RUNTIME_VERSION.into()),
        runtime_profile_ref: Some(profile_ref.into()),
        scopes: Vec::new(),
        billing_source: CLAUDE_PRODUCT.billing_source().into(),
        billing_owner: CLAUDE_PRODUCT.billing_owner().into(),
        entitlement_state: AgentEntitlementState::Unknown,
        entitlement_source: "not_observed".into(),
        entitlement_observed_at: None,
        expires_at: None,
    }
}

fn codex_authorized_account(profile_ref: &str) -> AuthorizedAgentAccount {
    AuthorizedAgentAccount {
        provider: AgentProvider::Codex,
        product: CODEX_PRODUCT,
        harness: AgentHarness::Alfred,
        display_name: Some("ChatGPT Codex".into()),
        external_account_id: "chatgpt.com".into(),
        external_workspace_id: None,
        auth_method: AgentAuthMethod::OAuthPkce,
        custody_mode: CredentialCustodyMode::RuntimeManaged,
        managed_runtime_id: Some(ManagedRuntimeId::CodexPythonSdk),
        managed_runtime_version: Some(codex::CODEX_SDK_RUNTIME_VERSION.into()),
        runtime_profile_ref: Some(profile_ref.into()),
        scopes: Vec::new(),
        billing_source: CODEX_PRODUCT.billing_source().into(),
        billing_owner: CODEX_PRODUCT.billing_owner().into(),
        entitlement_state: AgentEntitlementState::Unknown,
        entitlement_source: "not_observed".into(),
        entitlement_observed_at: None,
        expires_at: None,
    }
}

fn load_claude_publisher_evidence(
    package_root: &Path,
) -> Result<(Vec<u8>, Vec<u8>), NativeRuntimeError> {
    let evidence_root = package_root
        .parent()
        .ok_or_else(native_claude_package_gate)?
        .join("publisher-evidence");
    let manifest = fs::read(evidence_root.join("manifest.json")).map_err(|_| native_claude_package_gate())?;
    let signature = fs::read(evidence_root.join("manifest.json.sig"))
        .map_err(|_| native_claude_package_gate())?;
    if manifest.is_empty() || manifest.len() > 1024 * 1024 || signature.is_empty() {
        return Err(native_claude_package_gate());
    }
    Ok((manifest, signature))
}

struct CodexPackageInputs {
    source_manifest: Vec<u8>,
    target_sbom: Vec<u8>,
    license: Vec<u8>,
    notice: Vec<u8>,
}

fn load_codex_package_inputs(package_root: &Path) -> Result<CodexPackageInputs, NativeRuntimeError> {
    let evidence_root = package_root
        .parent()
        .ok_or_else(native_codex_package_gate)?;
    let source_manifest = fs::read(evidence_root.join("runtime-package.source.json"))
        .or_else(|_| fs::read(package_root.join("runtime-package.source.json")))
        .map_err(|_| native_codex_package_gate())?;
    let target_sbom = fs::read(package_root.join("legal/sbom.cdx.json"))
        .or_else(|_| fs::read(evidence_root.join("sbom.cdx.json")))
        .map_err(|_| native_codex_package_gate())?;
    let license = fs::read(package_root.join("legal/openai-codex/LICENSE"))
        .map_err(|_| native_codex_package_gate())?;
    let notice = fs::read(package_root.join("legal/openai-codex/NOTICE"))
        .map_err(|_| native_codex_package_gate())?;
    Ok(CodexPackageInputs {
        source_manifest,
        target_sbom,
        license,
        notice,
    })
}

/// The public gate code is deliberately opaque so DTOs never carry package
/// paths or provider detail. That also hides the cause from whoever has to fix
/// a staging failure, so keep the real error on stderr in debug builds.
fn package_gate<E: std::fmt::Debug>(
    stage: &str,
    error: E,
    gate: fn() -> NativeRuntimeError,
) -> NativeRuntimeError {
    #[cfg(debug_assertions)]
    eprintln!("managed runtime package rejected at {stage}: {error:?}");
    #[cfg(not(debug_assertions))]
    let _ = (stage, error);
    gate()
}

fn native_claude_package_gate() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        claude::PACKAGE_INTEGRATION_BLOCKED_CODE,
        false,
    )
}

fn native_codex_package_gate() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        codex::SEALED_PACKAGE_BLOCKER,
        false,
    )
}

fn codex_command_error(error: codex::CodexSdkRuntimeError) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        error.code().as_str(),
        "The managed ChatGPT Codex runtime operation failed.",
        true,
    )
}

fn opencode_authorized_account(profile_ref: &str) -> AuthorizedAgentAccount {
    AuthorizedAgentAccount {
        provider: AgentProvider::Opencode,
        product: OPENCODE_PRODUCT,
        harness: AgentHarness::Alfred,
        display_name: Some("OpenCode Go".into()),
        external_account_id: opencode::OPENCODE_GO_PROVIDER_ID.into(),
        external_workspace_id: None,
        auth_method: AgentAuthMethod::ApiKey,
        custody_mode: CredentialCustodyMode::RuntimeManaged,
        managed_runtime_id: Some(ManagedRuntimeId::OpencodeServer),
        managed_runtime_version: Some(opencode::OPENCODE_RUNTIME_VERSION.into()),
        runtime_profile_ref: Some(profile_ref.into()),
        scopes: Vec::new(),
        billing_source: OPENCODE_PRODUCT.billing_source().into(),
        billing_owner: OPENCODE_PRODUCT.billing_owner().into(),
        entitlement_state: AgentEntitlementState::Unknown,
        entitlement_source: "not_observed".into(),
        entitlement_observed_at: None,
        expires_at: None,
    }
}

fn native_command_error(error: NativeRuntimeError) -> ManagedRuntimeCommandError {
    let code = if is_stable_command_code(&error.message) {
        error.message
    } else {
        match error.code {
            NativeErrorCode::AccountUnavailable => "managed_runtime_api_key_invalid".into(),
            NativeErrorCode::InvalidRequest => "managed_runtime_account_invalid".into(),
            NativeErrorCode::ProviderUnavailable => opencode::PACKAGE_GATE_CODE.into(),
            _ => "managed_runtime_connection_failed".into(),
        }
    };
    ManagedRuntimeCommandError::new(
        code,
        "The managed OpenCode runtime operation failed.",
        error.retryable,
    )
}

fn account_command_error(error: AgentAccountCommandError) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        error.code,
        "Managed runtime account metadata could not be stored.",
        error.recoverable,
    )
}

fn is_stable_command_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn managed_products() -> Vec<ManagedProductSpec> {
    vec![
        ManagedProductSpec {
            provider: AgentProvider::ClaudeCode,
            product: CLAUDE_PRODUCT,
            runtime_id: CLAUDE_PRODUCT.managed_runtime().expect("pinned runtime"),
            runtime_version: claude::CLAUDE_CODE_RUNTIME_VERSION,
            connection_kind: "terminal",
            gate_codes: claude::subscription_release_gates()
                .into_iter()
                .filter_map(|gate| match gate.status {
                    crate::agents::native::CapabilityReportStatus::Supported => None,
                    crate::agents::native::CapabilityReportStatus::Unsupported
                    | crate::agents::native::CapabilityReportStatus::Blocked => {
                        Some(gate.evidence.into())
                    }
                })
                .collect(),
            primary_gate_code: claude::COMMERCIAL_TERMS_BLOCKED_CODE,
        },
        ManagedProductSpec {
            provider: AgentProvider::Codex,
            product: CODEX_PRODUCT,
            runtime_id: CODEX_PRODUCT.managed_runtime().expect("pinned runtime"),
            runtime_version: codex::CODEX_SDK_RUNTIME_VERSION,
            connection_kind: "browser",
            gate_codes: codex::codex_sdk_release_gates()
                .iter()
                .map(|gate| gate.evidence.into())
                .collect(),
            primary_gate_code: codex::PUBLIC_CAPABILITY_AUDIT_BLOCKER,
        },
        ManagedProductSpec {
            provider: AgentProvider::Opencode,
            product: OPENCODE_PRODUCT,
            runtime_id: OPENCODE_PRODUCT.managed_runtime().expect("pinned runtime"),
            runtime_version: opencode::OPENCODE_RUNTIME_VERSION,
            connection_kind: "api_key",
            gate_codes: opencode::native_release_gate()
                .blockers
                .iter()
                .map(|(code, _)| (*code).into())
                .collect(),
            primary_gate_code: opencode::COMMERCIAL_GATE_CODE,
        },
    ]
}

fn product_spec(
    provider_id: &str,
    product_id: &str,
) -> Result<ManagedProductSpec, ManagedRuntimeCommandError> {
    let provider = AgentProvider::from_str(provider_id).ok_or_else(|| {
        ManagedRuntimeCommandError::new(
            "provider_not_found",
            "The managed runtime provider is unknown.",
            false,
        )
    })?;
    let product = product_id.parse::<AgentProductId>().map_err(|_| {
        ManagedRuntimeCommandError::new(
            "product_not_found",
            "The managed runtime product is unknown.",
            false,
        )
    })?;
    managed_products()
        .into_iter()
        .find(|spec| spec.provider == provider && spec.product == product)
        .ok_or_else(|| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_product_not_supported",
                "That provider/product is not a managed runtime product.",
                false,
            )
        })
}

fn valid_terminal_session_id(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("claude_terminal_") else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn state_error() -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        "managed_runtime_state_unavailable",
        "Managed runtime state is temporarily unavailable.",
        true,
    )
}

fn package_command_error(
    error: RuntimePackageError,
    recoverable: bool,
) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        error.code().as_str(),
        "Managed runtime package storage is unavailable.",
        recoverable,
    )
}

fn profile_command_error(code: String, recoverable: bool) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        code,
        "Managed runtime profile storage is unavailable.",
        recoverable,
    )
}

fn terminal_command_error(error: ClaudeTerminalError) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        error.code().as_str(),
        "Managed runtime terminal operation failed.",
        true,
    )
}

fn managed_runtime_task_failed() -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        "managed_runtime_operation_failed",
        "The managed runtime operation could not be completed. Try again.",
        true,
    )
}

async fn run_managed_blocking<T, F>(work: F) -> Result<T, ManagedRuntimeCommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ManagedRuntimeCommandError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| Err(managed_runtime_task_failed()))
}

/// Tauri command: enumerate exactly the managed provider products known to
/// this release.  It never probes PATH or a provider CLI.
///
/// Login/start/prepare stay off the UI thread. A sync command would freeze
/// Alfred while Claude's PTY or the ChatGPT sidecar starts.
#[tauri::command]
pub async fn list_managed_runtime_products(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
) -> Result<Vec<ManagedRuntimeProductDto>, ManagedRuntimeCommandError> {
    let plane = Arc::clone(state.inner());
    run_managed_blocking(move || plane.list_products()).await
}

#[tauri::command]
pub async fn prepare_managed_runtime_product(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
) -> Result<ManagedRuntimeProductDto, ManagedRuntimeCommandError> {
    let plane = Arc::clone(state.inner());
    run_managed_blocking(move || plane.prepare_product(&provider_id, &product_id)).await
}

#[tauri::command]
pub async fn start_managed_runtime_connection(
    db: tauri::State<'_, Db>,
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
) -> Result<ManagedRuntimeConnectionStartDto, ManagedRuntimeCommandError> {
    tokio::task::block_in_place(|| {
        state
            .inner()
            .start_connection(db.inner(), &provider_id, &product_id)
    })
}

#[tauri::command]
pub async fn connect_managed_runtime_api_key(
    db: tauri::State<'_, Db>,
    accounts: tauri::State<'_, AgentAccountsState>,
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
    api_key: AgentApiKeySecret,
) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
    tokio::task::block_in_place(|| {
        state.inner().connect_api_key(
            db.inner(),
            &accounts,
            &provider_id,
            &product_id,
            api_key.into_zeroizing(),
        )
    })
}

#[tauri::command]
pub async fn managed_runtime_connection_status(
    db: tauri::State<'_, Db>,
    accounts: tauri::State<'_, AgentAccountsState>,
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
    tokio::task::block_in_place(|| {
        state
            .inner()
            .connection_status(db.inner(), &accounts, &provider_id, &product_id)
    })
}

#[tauri::command]
pub fn resolve_native_approval(
    broker: tauri::State<'_, Arc<HostApprovalBroker>>,
    request_id: String,
    decision: String,
) -> Result<(), ManagedRuntimeCommandError> {
    let decision = HostApprovalDecision::parse(&decision).map_err(native_command_error)?;
    broker
        .resolve(&request_id, decision)
        .map_err(native_command_error)
}

#[tauri::command]
pub async fn read_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
    cursor: u64,
) -> Result<ManagedRuntimeTerminalReadDto, ManagedRuntimeCommandError> {
    let plane = Arc::clone(state.inner());
    run_managed_blocking(move || plane.read_terminal(&session_id, cursor)).await
}

#[tauri::command]
pub async fn write_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
    input: String,
) -> Result<(), ManagedRuntimeCommandError> {
    let plane = Arc::clone(state.inner());
    run_managed_blocking(move || plane.write_terminal(&session_id, &input)).await
}

#[tauri::command]
pub async fn resize_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), ManagedRuntimeCommandError> {
    let plane = Arc::clone(state.inner());
    run_managed_blocking(move || plane.resize_terminal(&session_id, cols, rows)).await
}

#[tauri::command]
pub async fn close_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
) -> Result<(), ManagedRuntimeCommandError> {
    let plane = Arc::clone(state.inner());
    run_managed_blocking(move || plane.close_terminal(&session_id)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RejectPackageVerifier(std::sync::atomic::AtomicUsize);

    impl RuntimePlatformPublisherVerifier for RejectPackageVerifier {
        fn verify(
            &self,
            _request: crate::agents::runtime_package::RuntimePlatformPublisherRequest<'_>,
        ) -> Result<
            crate::agents::runtime_package::RuntimePlatformPublisherEvidence,
            crate::agents::runtime_package::RuntimePackageErrorCode,
        > {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(crate::agents::runtime_package::RuntimePackageErrorCode::PublisherUnverified)
        }
    }

    fn bundled_opencode_fixture(root: &Path) {
        let package_parent = root
            .join("managed-runtimes/opencode_server")
            .join(opencode::current_runtime_target().expect("supported host"))
            .join(opencode::OPENCODE_RUNTIME_VERSION);
        fs::create_dir_all(package_parent.join("package")).expect("package root");
        let evidence = package_parent.join("publisher-evidence");
        fs::create_dir_all(&evidence).expect("evidence root");
        fs::write(evidence.join("publisher-verification.json"), b"{}").expect("proof");
        fs::write(evidence.join("publisher.sig"), [0u8; 64]).expect("signature");
    }

    #[test]
    fn startup_initializes_storage_without_verifying_bundled_packages() {
        let root = std::env::temp_dir().join(format!("alfred-startup-{}", Uuid::new_v4()));
        let resources = root.join("resources");
        bundled_opencode_fixture(&resources);
        let verifier = Arc::new(RejectPackageVerifier(std::sync::atomic::AtomicUsize::new(
            0,
        )));
        let mut plane = ManagedRuntimeControlPlane::new();
        plane.trust_verifier = ManagedRuntimeTrustVerifier::new(verifier.clone());

        plane
            .initialize(&root.join("data"), Some(&resources))
            .expect("storage");

        assert_eq!(
            verifier.0.load(Ordering::SeqCst),
            0,
            "setup must not enter package verification"
        );
        let product = plane
            .list_products()
            .expect("products")
            .into_iter()
            .find(|product| product.product_id == "opencode_go")
            .expect("opencode");
        assert_eq!(product.install_state, "preparing");
        assert!(!product.connect_available);
        assert!(!product
            .gate_codes
            .iter()
            .any(|gate| gate == "managed_runtime_storage_unavailable"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn pending_verification_keeps_commands_responsive_and_rejects_untrusted_packages() {
        struct WaitingVerifier {
            entered: Mutex<Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl RuntimePlatformPublisherVerifier for WaitingVerifier {
            fn verify(
                &self,
                _request: crate::agents::runtime_package::RuntimePlatformPublisherRequest<'_>,
            ) -> Result<
                crate::agents::runtime_package::RuntimePlatformPublisherEvidence,
                crate::agents::runtime_package::RuntimePackageErrorCode,
            > {
                self.entered
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(std::thread::current().id())
                    .unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                Err(crate::agents::runtime_package::RuntimePackageErrorCode::PublisherUnverified)
            }
        }

        let root = std::env::temp_dir().join(format!("alfred-background-{}", Uuid::new_v4()));
        let resources = root.join("resources");
        bundled_opencode_fixture(&resources);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut plane = ManagedRuntimeControlPlane::new();
        plane.trust_verifier = ManagedRuntimeTrustVerifier::new(Arc::new(WaitingVerifier {
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(release_rx),
        }));
        plane
            .initialize(&root.join("data"), Some(&resources))
            .expect("storage");
        let plane = Arc::new(plane);
        let changes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let event_changes = changes.clone();
        let caller = std::thread::current().id();
        let verification = tokio::spawn(plane.clone().verify_bundled_packages(move || {
            event_changes.fetch_add(1, Ordering::SeqCst);
        }));
        let worker = tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .expect("verification starts off-thread")
            .expect("worker entered");
        assert_ne!(worker, caller, "hashing must leave the calling thread");

        let product = plane
            .prepare_product("opencode", "opencode_go")
            .expect("pending prepare");
        assert_eq!(product.install_state, "preparing");
        assert!(!product.connect_available);
        assert_eq!(changes.load(Ordering::SeqCst), 0);
        let db = Db::open_in_memory().expect("db");
        assert!(plane
            .start_connection(&db, "opencode", "opencode_go")
            .is_err());
        assert!(plane.opencode_selection.lock().unwrap().is_none());

        release_tx.send(()).expect("release verification");
        verification
            .await
            .expect("task")
            .expect("verification pass completed");
        assert_eq!(changes.load(Ordering::SeqCst), 1);
        let product = plane
            .list_products()
            .expect("products")
            .into_iter()
            .find(|product| product.product_id == "opencode_go")
            .expect("opencode");
        assert_eq!(product.install_state, "missing");
        assert!(!product.connect_available);
        assert!(plane.opencode_selection.lock().unwrap().is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn product_projection_is_exact_and_fail_closed_before_setup() {
        let control_plane = ManagedRuntimeControlPlane::new();
        let products = control_plane.list_products().expect("product projection");
        assert_eq!(
            products
                .iter()
                .map(|product| product.product_id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude_code_subscription", "chatgpt_codex", "opencode_go"]
        );
        assert!(products.iter().all(|product| {
            !product.connect_available
                && product.install_state == "blocked"
                && product
                    .gate_codes
                    .iter()
                    .any(|code| code == "managed_runtime_storage_unavailable")
        }));
    }

    #[test]
    fn provider_product_pairs_cannot_cross_route() {
        let control_plane = ManagedRuntimeControlPlane::new();
        let error = control_plane
            .prepare_product("claude_code", "chatgpt_codex")
            .expect_err("cross-provider product must be rejected");
        assert_eq!(error.code, "managed_runtime_product_not_supported");
    }

    #[test]
    fn prepare_without_a_verified_package_is_reported() {
        let root = std::env::temp_dir().join(format!(
            "alfred-managed-runtime-prepare-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let control_plane = ManagedRuntimeControlPlane::new();
        control_plane
            .initialize(&root, None)
            .expect("initialize");
        let error = control_plane
            .prepare_product("claude_code", "claude_code_subscription")
            .expect_err("missing Claude package must fail closed");
        assert_eq!(error.code, "managed_runtime_package_missing");
        let error = control_plane
            .prepare_product("codex", "chatgpt_codex")
            .expect_err("missing ChatGPT package must fail closed");
        assert_eq!(error.code, "managed_runtime_package_missing");
        let probe_db = Db::open_in_memory().expect("db");
        let error = control_plane
            .start_connection(&probe_db, "claude_code", "claude_code_subscription")
            .expect_err("missing Claude package must not start OAuth");
        assert_eq!(error.code, "managed_runtime_package_missing");
        let error = control_plane
            .start_connection(&probe_db, "codex", "chatgpt_codex")
            .expect_err("missing ChatGPT package must not start OAuth");
        assert_eq!(error.code, "managed_runtime_package_missing");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn opencode_stays_gated_after_setup_without_publisher_evidence() {
        let root = std::env::temp_dir().join(format!(
            "alfred-opencode-control-plane-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let control_plane = ManagedRuntimeControlPlane::new();
        control_plane
            .initialize(&root, None)
            .expect("initialize");
        let products = control_plane.list_products().expect("products");
        let opencode = products
            .iter()
            .find(|product| product.product_id == "opencode_go")
            .expect("opencode");
        assert_eq!(opencode.install_state, "missing");
        assert!(!opencode.connect_available);
        assert!(opencode
            .gate_codes
            .contains(&opencode::COMMERCIAL_GATE_CODE.into()));
        assert!(opencode
            .gate_codes
            .contains(&opencode::LIVE_SMOKE_GATE_CODE.into()));
        assert!(opencode
            .gate_codes
            .contains(&opencode::PACKAGE_GATE_CODE.into()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn waived_opencode_connect_persists_account_without_exposing_the_key() {
        let root = std::env::temp_dir().join(format!(
            "alfred-opencode-control-plane-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let sidecar = opencode::fake_server::FakeOpenCodeSidecar::start(
            root.to_string_lossy().into_owned(),
            opencode::fake_server::fixture_go_catalog(),
            Vec::new(),
        );
        let servers = Arc::new(opencode::fake_server::FixtureOpenCodeProvider::new(&sidecar));
        let control_plane = Arc::new(ManagedRuntimeControlPlane::new());
        control_plane.initialize(&root, None).expect("initialize");
        control_plane.waive_opencode_process_gates_for_test();
        control_plane.inject_opencode_servers_for_test(servers.clone());
        let registry = Arc::new(NativeRuntimeRegistry::default());
        let broker = Arc::new(HostApprovalBroker::new());
        control_plane
            .bind_native_collaborators(Arc::clone(&registry), Arc::clone(&broker))
            .expect("bind");

        let products = control_plane.list_products().expect("products");
        let opencode_product = products
            .iter()
            .find(|product| product.product_id == "opencode_go")
            .expect("opencode");
        assert_eq!(opencode_product.install_state, "ready");
        assert!(opencode_product.connect_available);
        assert!(opencode_product.gate_codes.is_empty());

        let db = Db::open_in_memory().expect("db");
        let started = control_plane
            .start_connection(&db, "opencode", "opencode_go")
            .expect("start");
        assert_eq!(started.kind, "api_key");

        let accounts = AgentAccountsState::default();
        let status = control_plane
            .connect_api_key(
                &db,
                &accounts,
                "opencode",
                "opencode_go",
                Zeroizing::new(opencode::fake_server::FIXTURE_GO_KEY.into()),
            )
            .expect("connect");
        assert_eq!(status.connection_state, "connected");
        let account_id = status.account_id.clone().expect("account id");
        assert!(account_id.starts_with("account_"));
        let encoded = serde_json::to_string(&status).expect("status json");
        assert!(!encoded.contains(opencode::fake_server::FIXTURE_GO_KEY));
        assert!(!encoded.contains("runtime_profile_"));
        assert!(registry.contains(AgentProvider::Opencode));

        let persisted = db.get_agent_account(&account_id).expect("load").expect("row");
        assert!(persisted.credential_ref.is_none());
        assert!(persisted.runtime_profile_ref.is_some());

        let account = persisted;
        control_plane
            .logout_opencode_account(&account)
            .expect("logout");
        assert_eq!(servers.purges(), 1);
        let _ = fs::remove_dir_all(&root);
        drop(sidecar);
    }

    #[test]
    fn claude_and_codex_oauth_connect_is_ready_with_fixtures() {
        let root = std::env::temp_dir().join(format!(
            "alfred-oauth-control-plane-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let control_plane = ManagedRuntimeControlPlane::new();
        control_plane.initialize(&root, None).expect("initialize");
        let products = control_plane.list_products().expect("products");
        let claude = products
            .iter()
            .find(|product| product.product_id == "claude_code_subscription")
            .expect("claude");
        let codex = products
            .iter()
            .find(|product| product.product_id == "chatgpt_codex")
            .expect("codex");
        assert_eq!(claude.install_state, "missing");
        assert!(!claude.connect_available);
        assert!(claude
            .gate_codes
            .contains(&claude::PACKAGE_INTEGRATION_BLOCKED_CODE.into()));
        assert!(!claude
            .gate_codes
            .contains(&claude::COMMERCIAL_TERMS_BLOCKED_CODE.into()));
        assert!(!claude
            .gate_codes
            .contains(&claude::WORKFLOW_RENDERER_APPROVAL_BLOCKED_CODE.into()));
        assert_eq!(codex.install_state, "missing");
        assert!(!codex.connect_available);
        assert!(codex
            .gate_codes
            .contains(&codex::SEALED_PACKAGE_BLOCKER.into()));
        assert!(!codex
            .gate_codes
            .contains(&codex::PUBLIC_CAPABILITY_AUDIT_BLOCKER.into()));
        assert!(!codex
            .gate_codes
            .contains(&codex::CODEX_SDK_HOST_APPROVAL_BLOCKER.into()));

        control_plane.enable_claude_oauth_fixture_for_test();
        control_plane.enable_codex_oauth_fixture_for_test();
        let products = control_plane.list_products().expect("ready products");
        let claude = products
            .iter()
            .find(|product| product.product_id == "claude_code_subscription")
            .expect("claude");
        let codex = products
            .iter()
            .find(|product| product.product_id == "chatgpt_codex")
            .expect("codex");
        assert_eq!(claude.install_state, "ready");
        assert!(claude.connect_available);
        assert!(claude.gate_codes.is_empty());
        assert_eq!(claude.connection_kind, "terminal");
        assert_eq!(codex.install_state, "ready");
        assert!(codex.connect_available);
        assert!(codex.gate_codes.is_empty());
        assert_eq!(codex.connection_kind, "browser");

        let db = Db::open_in_memory().expect("db");
        let claude_started = control_plane
            .start_connection(&db, "claude_code", "claude_code_subscription")
            .expect("claude start");
        assert_eq!(claude_started.kind, "terminal");
        assert!(claude_started
            .terminal_session_id
            .as_deref()
            .is_some_and(|id| id.starts_with("claude_terminal_")));
        assert!(claude_started.authorization_url.is_none());

        let codex_started = control_plane
            .start_connection(&db, "codex", "chatgpt_codex")
            .expect("codex start");
        assert_eq!(codex_started.kind, "browser");
        assert_eq!(
            codex_started.authorization_url.as_deref(),
            Some("https://chatgpt.com/auth/codex")
        );
        assert!(codex_started.terminal_session_id.is_none());

        let db = Db::open_in_memory().expect("db");
        let accounts = AgentAccountsState::default();
        let claude_status = control_plane
            .connection_status(
                &db,
                &accounts,
                "claude_code",
                "claude_code_subscription",
            )
            .expect("claude status");
        assert_eq!(claude_status.connection_state, "connected");
        let claude_account_id = claude_status.account_id.clone().expect("claude account");
        let claude_json = serde_json::to_string(&claude_status).expect("claude json");
        assert!(!claude_json.contains("sk-ant"));
        assert!(!claude_json.contains("runtime_profile_"));
        assert!(!claude_json.contains("oauth"));

        let codex_status = control_plane
            .connection_status(&db, &accounts, "codex", "chatgpt_codex")
            .expect("codex status");
        assert_eq!(codex_status.connection_state, "connected");
        let codex_account_id = codex_status.account_id.clone().expect("codex account");
        let codex_json = serde_json::to_string(&codex_status).expect("codex json");
        assert!(!codex_json.contains("chatgpt.com/auth"));
        assert!(!codex_json.contains("runtime_profile_"));
        assert!(!codex_json.contains("access_token"));

        let claude_account = db
            .get_agent_account(&claude_account_id)
            .expect("load claude")
            .expect("claude row");
        assert!(claude_account.credential_ref.is_none());
        assert!(claude_account.runtime_profile_ref.is_some());
        assert_eq!(claude_account.auth_method, AgentAuthMethod::Runtime);
        control_plane
            .logout_claude_account(&claude_account)
            .expect("claude logout");

        let codex_account = db
            .get_agent_account(&codex_account_id)
            .expect("load codex")
            .expect("codex row");
        assert!(codex_account.credential_ref.is_none());
        assert!(codex_account.runtime_profile_ref.is_some());
        assert_eq!(codex_account.auth_method, AgentAuthMethod::OAuthPkce);
        control_plane
            .logout_codex_account(&codex_account)
            .expect("codex logout");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_claude_account_is_refused_while_one_is_connected() {
        let root = std::env::temp_dir().join(format!("alfred-claude-single-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp root");
        let control_plane = ManagedRuntimeControlPlane::new();
        control_plane.initialize(&root, None).expect("initialize");
        control_plane.enable_claude_oauth_fixture_for_test();
        let db = Db::open_in_memory().expect("db");
        let accounts = AgentAccountsState::default();

        control_plane
            .start_connection(&db, "claude_code", "claude_code_subscription")
            .expect("first claude connect starts");
        let status = control_plane
            .connection_status(&db, &accounts, "claude_code", "claude_code_subscription")
            .expect("claude status");
        assert_eq!(status.connection_state, "connected");

        // Claude Code owns one shared Keychain item, so a second account would
        // silently evict the first rather than coexist with it.
        let error = control_plane
            .start_connection(&db, "claude_code", "claude_code_subscription")
            .expect_err("a second claude account must be refused");
        assert_eq!(error.code, CLAUDE_SINGLE_ACCOUNT_CODE);
        assert!(!error.recoverable);

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn prepared_host_sidecars_verify_and_offer_sign_in() {
        let resource_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let claude_pkg = first_claude_package_root(Some(&resource_root));
        let codex_pkg = first_codex_package_root(Some(&resource_root));
        if claude_pkg.is_none() && codex_pkg.is_none() {
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "alfred-prepared-sidecars-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let control_plane = Arc::new(ManagedRuntimeControlPlane::new());
        control_plane
            .initialize(&root, Some(&resource_root))
            .expect("initialize");
        control_plane
            .clone()
            .verify_bundled_packages(|| {})
            .await
            .expect("verify");
        let products = control_plane.list_products().expect("products");

        if claude_pkg.is_some() {
            let claude = products
                .iter()
                .find(|product| product.product_id == "claude_code_subscription")
                .expect("claude");
            assert_eq!(claude.install_state, "ready");
            assert!(claude.connect_available);
            assert_eq!(claude.connection_kind, "terminal");
            assert!(claude.gate_codes.is_empty());
        }
        if codex_pkg.is_some() {
            let codex = products
                .iter()
                .find(|product| product.product_id == "chatgpt_codex")
                .expect("codex");
            assert_eq!(codex.install_state, "ready");
            assert!(codex.connect_available);
            assert_eq!(codex.connection_kind, "browser");
            assert!(codex.gate_codes.is_empty());
        }

        let _ = fs::remove_dir_all(&root);
    }
}
