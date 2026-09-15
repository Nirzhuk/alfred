use super::models::{AppConnection, ConnectionStatus};
use super::refresh::{RefreshService, RefreshServiceError};
use super::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_OUTPUT_DEPTH: usize = 8;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const RESOURCE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESOURCE_QUERY_BYTES: usize = 200;
const MAX_RESOURCE_PAGE_TOKEN_BYTES: usize = 512;
const MAX_RESOURCE_ITEMS: usize = 100;
const MAX_RESOURCE_CACHE_ENTRIES: usize = 128;
const RESOURCE_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionFieldKind {
    Text,
    Textarea,
    Boolean,
    Enum,
    ResourceSelector,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionFieldDescriptor {
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: ActionFieldKind,
    pub required: bool,
    pub default: Option<Value>,
    /// Always false in the public contract. Secret input fields are forbidden.
    pub secret: bool,
    pub option_source: Option<String>,
    pub options: Vec<ActionOption>,
    pub supports_interpolation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDescriptor {
    pub provider_id: String,
    pub action_id: String,
    pub label: String,
    pub description: String,
    pub fields: Vec<ActionFieldDescriptor>,
    pub required_scopes: Vec<String>,
    pub output_schema_version: u16,
    /// Marks provider text that must enter downstream prompts as untrusted
    /// external data rather than workflow instructions.
    pub output_is_untrusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequest {
    pub connection_id: String,
    pub provider_id: String,
    pub action_id: String,
    pub input: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct ValidatedActionRequest {
    pub connection_id: String,
    pub provider_id: String,
    pub action_id: String,
    pub input: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionArtifact {
    pub kind: String,
    pub label: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub summary: String,
    pub output: Value,
    pub artifacts: Vec<ActionArtifact>,
    pub provider_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResourceItem {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResourcePage {
    pub items: Vec<ActionResourceItem>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionErrorCode {
    ActionNotFound,
    ConnectionRequired,
    ScopeMissing,
    RateLimited,
    ProviderUnauthorized,
    ProviderUnavailable,
    InvalidInput,
    OutputTooLarge,
    OutputInvalid,
    TimedOut,
    DeliveryUnknown,
    Cancelled,
}

impl ActionErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ActionNotFound => "action_not_found",
            Self::ConnectionRequired => "connection_required",
            Self::ScopeMissing => "scope_missing",
            Self::RateLimited => "rate_limited",
            Self::ProviderUnauthorized => "provider_unauthorized",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::InvalidInput => "invalid_input",
            Self::OutputTooLarge => "output_too_large",
            Self::OutputInvalid => "output_invalid",
            Self::TimedOut => "timed_out",
            Self::DeliveryUnknown => "delivery_unknown",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionError {
    pub code: ActionErrorCode,
    pub message: String,
    pub retry_after_seconds: Option<u64>,
    pub provider_request_id: Option<String>,
}

impl fmt::Debug for ActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActionError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("retry_after_seconds", &self.retry_after_seconds)
            .field("provider_request_id", &self.provider_request_id)
            .finish()
    }
}

impl fmt::Display for ActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ActionError {}

impl ActionError {
    pub fn new(code: ActionErrorCode) -> Self {
        Self {
            code,
            message: safe_message(code).into(),
            retry_after_seconds: None,
            provider_request_id: None,
        }
    }

    pub fn rate_limited(retry_after_seconds: Option<u64>) -> Self {
        let mut error = Self::new(ActionErrorCode::RateLimited);
        error.retry_after_seconds = retry_after_seconds.map(|seconds| seconds.min(86_400));
        error
    }

    pub fn with_request_id(mut self, request_id: &str) -> Self {
        self.provider_request_id = sanitize_request_id(request_id);
        self
    }

    pub fn is_unauthorized(&self) -> bool {
        self.code == ActionErrorCode::ProviderUnauthorized
    }
}

fn safe_message(code: ActionErrorCode) -> &'static str {
    match code {
        ActionErrorCode::ActionNotFound => "This app action is not available.",
        ActionErrorCode::ConnectionRequired => "Choose a healthy connected app.",
        ActionErrorCode::ScopeMissing => "The connection does not grant the required access.",
        ActionErrorCode::RateLimited => {
            "The provider is rate limiting this action. Try again later."
        }
        ActionErrorCode::ProviderUnauthorized => {
            "The provider rejected the connection. Reconnect and try again."
        }
        ActionErrorCode::ProviderUnavailable => "The provider is temporarily unavailable.",
        ActionErrorCode::InvalidInput => "The app action configuration is invalid.",
        ActionErrorCode::OutputTooLarge => "The provider result exceeds the safe output limit.",
        ActionErrorCode::OutputInvalid => "The provider returned an invalid result.",
        ActionErrorCode::TimedOut => "The app action timed out.",
        ActionErrorCode::DeliveryUnknown => {
            "The provider may have accepted this action. Check the target before retrying."
        }
        ActionErrorCode::Cancelled => "The app action was cancelled.",
    }
}

fn sanitize_request_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ActionLimits {
    pub max_output_bytes: usize,
    pub max_output_depth: usize,
    pub timeout: Duration,
}

impl Default for ActionLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_output_depth: DEFAULT_MAX_OUTPUT_DEPTH,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[derive(Clone)]
pub struct ActionCancellation {
    cancelled: Arc<AtomicBool>,
}

impl ActionCancellation {
    pub fn new(cancelled: Arc<AtomicBool>) -> Self {
        Self { cancelled }
    }

    pub fn never() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)))
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    async fn wait(&self) {
        while !self.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }
}

#[derive(Clone)]
pub struct TokenAccessCapability {
    credential: Arc<Mutex<CredentialEnvelope>>,
}

impl fmt::Debug for TokenAccessCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TokenAccessCapability([REDACTED])")
    }
}

impl TokenAccessCapability {
    pub(crate) async fn load(
        store: Arc<dyn TokenStore>,
        credential_ref: String,
    ) -> Result<Self, ActionError> {
        let credential = tauri::async_runtime::spawn_blocking(move || store.get(&credential_ref))
            .await
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?
            .map_err(map_token_error)?;
        Ok(Self {
            credential: Arc::new(Mutex::new(credential)),
        })
    }

    /// Providers can copy only the values needed to build a backend request.
    /// The credential reference and envelope never cross the command boundary.
    pub fn with_credential<T>(
        &self,
        use_credential: impl FnOnce(&CredentialEnvelope) -> T,
    ) -> Result<T, ActionError> {
        let credential = self
            .credential
            .lock()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?;
        Ok(use_credential(&credential))
    }

    pub(crate) fn contains_secret(&self, serialized: &str) -> bool {
        let Ok(credential) = self.credential.lock() else {
            return true;
        };
        std::iter::once(credential.access_token.as_str())
            .chain(credential.refresh_token.as_deref())
            .chain(credential.provider_fields.values().map(String::as_str))
            .filter(|value| value.len() >= 8)
            .any(|value| serialized.contains(value))
    }
}

fn map_token_error(error: TokenStoreError) -> ActionError {
    match error {
        TokenStoreError::Missing | TokenStoreError::Invalid => {
            ActionError::new(ActionErrorCode::ConnectionRequired)
        }
        TokenStoreError::Locked | TokenStoreError::Failed => {
            ActionError::new(ActionErrorCode::ProviderUnavailable)
        }
    }
}

pub type ActionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ActionResult, ActionError>> + Send + 'a>>;
pub type ActionResourcesFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ActionResourcePage, ActionError>> + Send + 'a>>;

pub trait ActionExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        request: &'a ValidatedActionRequest,
        connection: &'a AppConnection,
        tokens: TokenAccessCapability,
        cancellation: ActionCancellation,
    ) -> ActionFuture<'a>;

    fn list_resources<'a>(
        &'a self,
        _source: &'a str,
        _field_key: &'a str,
        _query: &'a str,
        _page_token: Option<&'a str>,
        _connection: &'a AppConnection,
        _tokens: TokenAccessCapability,
        _cancellation: ActionCancellation,
    ) -> ActionResourcesFuture<'a> {
        Box::pin(async { Err(ActionError::new(ActionErrorCode::InvalidInput)) })
    }
}

struct RegisteredAction {
    descriptor: ActionDescriptor,
    limits: ActionLimits,
    executor: Arc<dyn ActionExecutor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResourceCacheKey {
    connection_id: String,
    provider_id: String,
    action_id: String,
    field_key: String,
    query: String,
    page_token: Option<String>,
}

struct ResourceCacheEntry {
    inserted_at: Instant,
    page: ActionResourcePage,
}

#[derive(Default)]
pub struct ActionRegistry {
    actions: RwLock<HashMap<String, Arc<RegisteredAction>>>,
    resource_cache: Mutex<HashMap<ResourceCacheKey, ResourceCacheEntry>>,
}

impl ActionRegistry {
    pub fn register(
        &self,
        descriptor: ActionDescriptor,
        limits: ActionLimits,
        executor: Arc<dyn ActionExecutor>,
    ) -> Result<(), ActionError> {
        validate_descriptor(&descriptor)?;
        validate_limits(limits)?;
        let mut actions = self
            .actions
            .write()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?;
        if actions.contains_key(&descriptor.action_id) {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        actions.insert(
            descriptor.action_id.clone(),
            Arc::new(RegisteredAction {
                descriptor,
                limits,
                executor,
            }),
        );
        Ok(())
    }

    pub fn descriptors(&self, provider_id: Option<&str>) -> Vec<ActionDescriptor> {
        let Ok(actions) = self.actions.read() else {
            return Vec::new();
        };
        let mut descriptors = actions
            .values()
            .filter(|registration| {
                provider_id.is_none_or(|provider| registration.descriptor.provider_id == provider)
            })
            .map(|registration| registration.descriptor.clone())
            .collect::<Vec<_>>();
        descriptors.sort_by(|left, right| left.action_id.cmp(&right.action_id));
        descriptors
    }

    pub fn descriptor(&self, provider_id: &str, action_id: &str) -> Option<ActionDescriptor> {
        self.actions
            .read()
            .ok()
            .and_then(|actions| actions.get(action_id).cloned())
            .filter(|registration| registration.descriptor.provider_id == provider_id)
            .map(|registration| registration.descriptor.clone())
    }

    pub(crate) fn validate_setup(
        &self,
        db: &Db,
        request: ActionRequest,
    ) -> Result<(), ActionError> {
        let registration = self.registration(&request.provider_id, &request.action_id)?;
        load_compatible_connection(db, &request, &registration.descriptor)?;
        validate_request(request, &registration.descriptor)?;
        Ok(())
    }

    pub async fn execute(
        &self,
        db: &Db,
        refresh: &RefreshService,
        store: Arc<dyn TokenStore>,
        request: ActionRequest,
        cancellation: ActionCancellation,
    ) -> Result<ActionResult, ActionError> {
        let registration = self.registration(&request.provider_id, &request.action_id)?;
        let connection = load_compatible_connection(db, &request, &registration.descriptor)?;
        let validated = validate_request(request, &registration.descriptor)?;

        let mut tokens =
            TokenAccessCapability::load(store.clone(), connection.credential_ref.clone()).await?;
        let first = run_executor(
            registration.clone(),
            &validated,
            &connection,
            tokens.clone(),
            cancellation.clone(),
        )
        .await;
        let mut prior_tokens = None;
        let result = match first {
            Err(error) if error.is_unauthorized() && !cancellation.is_cancelled() => {
                // Retain a redaction capability for the credential rejected by
                // the provider. A buggy executor must not be able to return the
                // pre-refresh token after the retry succeeds.
                prior_tokens = Some(tokens.clone());
                refresh
                    .refresh_on_demand(db, &connection.id)
                    .await
                    .map_err(map_refresh_error)?;
                tokens =
                    TokenAccessCapability::load(store, connection.credential_ref.clone()).await?;
                run_executor(
                    registration.clone(),
                    &validated,
                    &connection,
                    tokens.clone(),
                    cancellation,
                )
                .await?
            }
            other => other?,
        };
        validate_result(&result, registration.limits, &tokens, prior_tokens.as_ref())?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_resources(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        connection_id: &str,
        provider_id: &str,
        action_id: &str,
        field_key: &str,
        query: &str,
        page_token: Option<&str>,
    ) -> Result<ActionResourcePage, ActionError> {
        self.list_resources_with_cache(
            db,
            store,
            connection_id,
            provider_id,
            action_id,
            field_key,
            query,
            page_token,
            true,
        )
        .await
    }

    pub(crate) async fn list_resources_for_setup(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        connection_id: &str,
        provider_id: &str,
        action_id: &str,
        field_key: &str,
        page_token: Option<&str>,
    ) -> Result<ActionResourcePage, ActionError> {
        self.list_resources_with_cache(
            db,
            store,
            connection_id,
            provider_id,
            action_id,
            field_key,
            "",
            page_token,
            false,
        )
        .await
    }

    async fn list_resources_with_cache(
        &self,
        db: &Db,
        store: Arc<dyn TokenStore>,
        connection_id: &str,
        provider_id: &str,
        action_id: &str,
        field_key: &str,
        query: &str,
        page_token: Option<&str>,
        use_cache: bool,
    ) -> Result<ActionResourcePage, ActionError> {
        if query.len() > MAX_RESOURCE_QUERY_BYTES
            || page_token.is_some_and(|token| token.len() > MAX_RESOURCE_PAGE_TOKEN_BYTES)
        {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        let registration = self.registration(provider_id, action_id)?;
        let field = registration
            .descriptor
            .fields
            .iter()
            .find(|field| field.key == field_key)
            .filter(|field| field.kind == ActionFieldKind::ResourceSelector)
            .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
        let source = field
            .option_source
            .as_deref()
            .ok_or_else(|| ActionError::new(ActionErrorCode::InvalidInput))?;
        let request = ActionRequest {
            connection_id: connection_id.into(),
            provider_id: provider_id.into(),
            action_id: action_id.into(),
            input: BTreeMap::new(),
        };
        let connection = load_compatible_connection(db, &request, &registration.descriptor)?;
        let cache_key = ResourceCacheKey {
            connection_id: connection_id.into(),
            provider_id: provider_id.into(),
            action_id: action_id.into(),
            field_key: field_key.into(),
            query: query.into(),
            page_token: page_token.map(str::to_owned),
        };
        if use_cache {
            if let Some(page) = self.cached_resource_page(&cache_key) {
                return Ok(page);
            }
        }

        let tokens = TokenAccessCapability::load(store, connection.credential_ref.clone()).await?;
        let cancellation = ActionCancellation::never();
        let result = tokio::select! {
            page = registration.executor.list_resources(
                source,
                field_key,
                query,
                page_token,
                &connection,
                tokens.clone(),
                cancellation.clone(),
            ) => page,
            _ = tokio::time::sleep(RESOURCE_TIMEOUT) => Err(ActionError::new(ActionErrorCode::TimedOut)),
            _ = cancellation.wait() => Err(ActionError::new(ActionErrorCode::Cancelled)),
        }?;
        validate_resource_page(&result, &tokens)?;
        if use_cache {
            self.cache_resource_page(cache_key, result.clone());
        }
        Ok(result)
    }

    fn registration(
        &self,
        provider_id: &str,
        action_id: &str,
    ) -> Result<Arc<RegisteredAction>, ActionError> {
        self.actions
            .read()
            .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?
            .get(action_id)
            .filter(|registration| registration.descriptor.provider_id == provider_id)
            .cloned()
            .ok_or_else(|| ActionError::new(ActionErrorCode::ActionNotFound))
    }

    fn cached_resource_page(&self, key: &ResourceCacheKey) -> Option<ActionResourcePage> {
        let mut cache = self.resource_cache.lock().ok()?;
        cache.retain(|_, entry| entry.inserted_at.elapsed() <= RESOURCE_CACHE_TTL);
        cache.get(key).map(|entry| entry.page.clone())
    }

    fn cache_resource_page(&self, key: ResourceCacheKey, page: ActionResourcePage) {
        let Ok(mut cache) = self.resource_cache.lock() else {
            return;
        };
        cache.retain(|_, entry| entry.inserted_at.elapsed() <= RESOURCE_CACHE_TTL);
        if cache.len() >= MAX_RESOURCE_CACHE_ENTRIES {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            key,
            ResourceCacheEntry {
                inserted_at: Instant::now(),
                page,
            },
        );
    }
}

async fn run_executor(
    registration: Arc<RegisteredAction>,
    request: &ValidatedActionRequest,
    connection: &AppConnection,
    tokens: TokenAccessCapability,
    cancellation: ActionCancellation,
) -> Result<ActionResult, ActionError> {
    if cancellation.is_cancelled() {
        return Err(ActionError::new(ActionErrorCode::Cancelled));
    }
    tokio::select! {
        result = registration.executor.execute(
            request,
            connection,
            tokens,
            cancellation.clone(),
        ) => result,
        _ = tokio::time::sleep(registration.limits.timeout) => Err(ActionError::new(ActionErrorCode::TimedOut)),
        _ = cancellation.wait() => Err(ActionError::new(ActionErrorCode::Cancelled)),
    }
}

fn load_compatible_connection(
    db: &Db,
    request: &ActionRequest,
    descriptor: &ActionDescriptor,
) -> Result<AppConnection, ActionError> {
    let connection = db
        .get_app_connection(&request.connection_id)
        .map_err(|_| ActionError::new(ActionErrorCode::ProviderUnavailable))?
        .ok_or_else(|| ActionError::new(ActionErrorCode::ConnectionRequired))?;
    if connection.provider_id != request.provider_id
        || descriptor.provider_id != request.provider_id
        || connection.status != ConnectionStatus::Connected
    {
        return Err(ActionError::new(ActionErrorCode::ConnectionRequired));
    }
    let scopes = connection.scopes.iter().collect::<HashSet<_>>();
    if descriptor
        .required_scopes
        .iter()
        .any(|scope| !scopes.contains(scope))
    {
        return Err(ActionError::new(ActionErrorCode::ScopeMissing));
    }
    Ok(connection)
}

fn validate_descriptor(descriptor: &ActionDescriptor) -> Result<(), ActionError> {
    if !valid_identifier(&descriptor.provider_id)
        || !valid_action_id(&descriptor.action_id, &descriptor.provider_id)
        || descriptor.label.trim().is_empty()
        || descriptor.label.len() > 120
        || descriptor.description.len() > 500
        || descriptor.fields.len() > 32
        || descriptor.required_scopes.len() > 64
        || descriptor.output_schema_version == 0
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    let mut scopes = HashSet::new();
    if descriptor.required_scopes.iter().any(|scope| {
        scope.trim().is_empty()
            || scope.len() > 200
            || scope.bytes().any(|byte| byte.is_ascii_control())
            || !scopes.insert(scope.as_str())
    }) {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    let mut keys = HashSet::new();
    for field in &descriptor.fields {
        let mut option_ids = HashSet::new();
        let invalid_options = field.options.iter().any(|option| {
            option.id.trim().is_empty()
                || option.id.len() > 512
                || option.label.trim().is_empty()
                || option.label.len() > 120
                || !option_ids.insert(option.id.as_str())
        });
        if !valid_identifier(&field.key)
            || sensitive_identifier(&field.key)
            || !keys.insert(field.key.as_str())
            || field.label.trim().is_empty()
            || field.label.len() > 120
            || field.description.len() > 500
            || field.secret
            || (field.kind == ActionFieldKind::ResourceSelector
                && field
                    .option_source
                    .as_deref()
                    .is_none_or(|source| !valid_identifier(source) || sensitive_identifier(source)))
            || (field.kind != ActionFieldKind::ResourceSelector && field.option_source.is_some())
            || (field.kind == ActionFieldKind::Enum && field.options.is_empty())
            || (field.kind != ActionFieldKind::Enum && !field.options.is_empty())
            || invalid_options
            || (!matches!(
                field.kind,
                ActionFieldKind::Text
                    | ActionFieldKind::Textarea
                    | ActionFieldKind::ResourceSelector
            ) && field.supports_interpolation)
        {
            return Err(ActionError::new(ActionErrorCode::InvalidInput));
        }
        if let Some(default) = &field.default {
            validate_field_value(field, default)?;
            if default.as_str().is_some_and(looks_like_secret_default) {
                return Err(ActionError::new(ActionErrorCode::InvalidInput));
            }
        }
    }
    Ok(())
}

fn sensitive_identifier(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|part| value.contains(part))
}

fn looks_like_secret_default(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("Bearer ")
        || value.starts_with("Basic ")
        || value.starts_with("sk-")
        || value.starts_with("ghp_")
        || value.starts_with("xox")
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_action_id(value: &str, provider_id: &str) -> bool {
    value
        .strip_prefix(&format!("{provider_id}."))
        .is_some_and(valid_identifier)
}

fn validate_limits(limits: ActionLimits) -> Result<(), ActionError> {
    if limits.max_output_bytes == 0
        || limits.max_output_bytes > DEFAULT_MAX_OUTPUT_BYTES
        || limits.max_output_depth == 0
        || limits.max_output_depth > DEFAULT_MAX_OUTPUT_DEPTH
        || limits.timeout.is_zero()
        || limits.timeout > DEFAULT_TIMEOUT
    {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    Ok(())
}

fn validate_request(
    request: ActionRequest,
    descriptor: &ActionDescriptor,
) -> Result<ValidatedActionRequest, ActionError> {
    let fields = descriptor
        .fields
        .iter()
        .map(|field| (field.key.as_str(), field))
        .collect::<HashMap<_, _>>();
    let display_snapshot_keys = descriptor
        .fields
        .iter()
        .filter(|field| field.kind == ActionFieldKind::ResourceSelector)
        .map(|field| format!("{}__display", field.key))
        .collect::<HashSet<_>>();
    if request.input.keys().any(|key| {
        !fields.contains_key(key.as_str()) && !display_snapshot_keys.contains(key.as_str())
    }) {
        return Err(ActionError::new(ActionErrorCode::InvalidInput));
    }
    let mut input = request.input;
    for field in &descriptor.fields {
        if !input.contains_key(&field.key) {
            if let Some(default) = &field.default {
                input.insert(field.key.clone(), default.clone());
            } else if field.required {
                return Err(ActionError::new(ActionErrorCode::InvalidInput));
            }
        }
        if let Some(value) = input.get(&field.key) {
            validate_field_value(field, value)?;
        }
        if field.kind == ActionFieldKind::ResourceSelector {
            if let Some(snapshot) = input.get(&format!("{}__display", field.key)) {
                let valid = snapshot.as_str().is_some_and(|value| {
                    !value.trim().is_empty()
                        && value.len() <= 512
                        && !value.chars().any(|character| {
                            character.is_control() && !matches!(character, '\n' | '\t')
                        })
                });
                if !valid {
                    return Err(ActionError::new(ActionErrorCode::InvalidInput));
                }
            }
        }
    }
    Ok(ValidatedActionRequest {
        connection_id: request.connection_id,
        provider_id: request.provider_id,
        action_id: request.action_id,
        input,
    })
}

fn validate_field_value(field: &ActionFieldDescriptor, value: &Value) -> Result<(), ActionError> {
    let valid = match field.kind {
        ActionFieldKind::Text | ActionFieldKind::Textarea | ActionFieldKind::ResourceSelector => {
            value.as_str().is_some_and(|value| {
                value.len() <= 32 * 1024 && (!field.required || !value.trim().is_empty())
            })
        }
        ActionFieldKind::Boolean => value.is_boolean(),
        ActionFieldKind::Enum => value
            .as_str()
            .is_some_and(|value| field.options.iter().any(|option| option.id == value)),
    };
    if valid {
        Ok(())
    } else {
        Err(ActionError::new(ActionErrorCode::InvalidInput))
    }
}

fn validate_result(
    result: &ActionResult,
    limits: ActionLimits,
    tokens: &TokenAccessCapability,
    prior_tokens: Option<&TokenAccessCapability>,
) -> Result<(), ActionError> {
    if result.summary.len() > 1_000
        || result.artifacts.len() > 32
        || result
            .provider_request_id
            .as_deref()
            .is_some_and(|request_id| {
                sanitize_request_id(request_id).as_deref() != Some(request_id)
            })
        || result.artifacts.iter().any(|artifact| {
            artifact.kind.len() > 64 || artifact.label.len() > 256 || artifact.uri.len() > 2_048
        })
    {
        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
    }
    if json_depth(&result.output) > limits.max_output_depth {
        return Err(ActionError::new(ActionErrorCode::OutputTooLarge));
    }
    let serialized = serde_json::to_string(result)
        .map_err(|_| ActionError::new(ActionErrorCode::OutputInvalid))?;
    if serialized.len() > limits.max_output_bytes {
        return Err(ActionError::new(ActionErrorCode::OutputTooLarge));
    }
    if tokens.contains_secret(&serialized)
        || prior_tokens.is_some_and(|prior| prior.contains_secret(&serialized))
    {
        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
    }
    Ok(())
}

fn validate_resource_page(
    page: &ActionResourcePage,
    tokens: &TokenAccessCapability,
) -> Result<(), ActionError> {
    if page.items.len() > MAX_RESOURCE_ITEMS
        || page
            .next_page_token
            .as_deref()
            .is_some_and(|token| token.len() > MAX_RESOURCE_PAGE_TOKEN_BYTES)
        || page.items.iter().any(|item| {
            item.id.is_empty()
                || item.id.len() > 512
                || item.label.is_empty()
                || item.label.len() > 512
        })
    {
        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
    }
    let serialized = serde_json::to_string(page)
        .map_err(|_| ActionError::new(ActionErrorCode::OutputInvalid))?;
    if serialized.len() > DEFAULT_MAX_OUTPUT_BYTES || tokens.contains_secret(&serialized) {
        return Err(ActionError::new(ActionErrorCode::OutputInvalid));
    }
    Ok(())
}

fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(values) => 1 + values.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(values) => 1 + values.values().map(json_depth).max().unwrap_or(0),
        _ => 1,
    }
}

fn map_refresh_error(error: RefreshServiceError) -> ActionError {
    match error {
        RefreshServiceError::Revoked
        | RefreshServiceError::NotFound
        | RefreshServiceError::TokenStore(TokenStoreError::Missing)
        | RefreshServiceError::TokenStore(TokenStoreError::Invalid) => {
            ActionError::new(ActionErrorCode::ConnectionRequired)
        }
        RefreshServiceError::Provider(provider_error)
            if provider_error.kind() == super::refresh::RefreshFailureKind::Terminal =>
        {
            ActionError::new(ActionErrorCode::ProviderUnauthorized)
        }
        _ => ActionError::new(ActionErrorCode::ProviderUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::{canonical_identity_key, UpsertAppConnection};
    use crate::integrations::refresh::{RefreshFuture, RefreshHandler};
    use crate::integrations::token_store::InMemoryTokenStore;
    use chrono::{DateTime, Utc};
    use std::sync::atomic::AtomicUsize;

    fn descriptor() -> ActionDescriptor {
        ActionDescriptor {
            provider_id: "slack".into(),
            action_id: "slack.send_message".into(),
            label: "Send message".into(),
            description: "Send a message to a channel".into(),
            fields: vec![
                ActionFieldDescriptor {
                    key: "channel".into(),
                    label: "Channel".into(),
                    description: String::new(),
                    kind: ActionFieldKind::ResourceSelector,
                    required: true,
                    default: None,
                    secret: false,
                    option_source: Some("channels".into()),
                    options: vec![],
                    supports_interpolation: false,
                },
                ActionFieldDescriptor {
                    key: "message".into(),
                    label: "Message".into(),
                    description: String::new(),
                    kind: ActionFieldKind::Textarea,
                    required: true,
                    default: None,
                    secret: false,
                    option_source: None,
                    options: vec![],
                    supports_interpolation: true,
                },
            ],
            required_scopes: vec!["chat:write".into()],
            output_schema_version: 1,
            output_is_untrusted: false,
        }
    }

    fn request() -> ActionRequest {
        ActionRequest {
            connection_id: "connection".into(),
            provider_id: "slack".into(),
            action_id: "slack.send_message".into(),
            input: BTreeMap::from([
                ("channel".into(), Value::String("C123".into())),
                ("message".into(), Value::String("Hello".into())),
            ]),
        }
    }

    struct FakeExecutor {
        calls: Arc<AtomicUsize>,
        behavior: FakeBehavior,
    }

    enum FakeBehavior {
        Success(Value),
        UnauthorizedOnce,
        UnauthorizedThen(Value),
        Sleep(Duration, Arc<AtomicUsize>),
    }

    impl ActionExecutor for FakeExecutor {
        fn execute<'a>(
            &'a self,
            request: &'a ValidatedActionRequest,
            _connection: &'a AppConnection,
            tokens: TokenAccessCapability,
            _cancellation: ActionCancellation,
        ) -> ActionFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                match &self.behavior {
                    FakeBehavior::Success(output) => Ok(ActionResult {
                        summary: "Sent".into(),
                        output: output.clone(),
                        artifacts: vec![],
                        provider_request_id: Some("request-1".into()),
                    }),
                    FakeBehavior::UnauthorizedOnce | FakeBehavior::UnauthorizedThen(_)
                        if call == 0 =>
                    {
                        Err(ActionError::new(ActionErrorCode::ProviderUnauthorized))
                    }
                    FakeBehavior::UnauthorizedOnce => {
                        let token = tokens.with_credential(|value| value.access_token.clone())?;
                        Ok(ActionResult {
                            summary: "Retried".into(),
                            output: serde_json::json!({"message": request.input["message"], "credentialWasUsed": !token.is_empty()}),
                            artifacts: vec![],
                            provider_request_id: None,
                        })
                    }
                    FakeBehavior::UnauthorizedThen(output) => Ok(ActionResult {
                        summary: "Retried".into(),
                        output: output.clone(),
                        artifacts: vec![],
                        provider_request_id: None,
                    }),
                    FakeBehavior::Sleep(duration, completions) => {
                        tokio::time::sleep(*duration).await;
                        completions.fetch_add(1, Ordering::SeqCst);
                        Ok(ActionResult {
                            summary: "Late".into(),
                            output: Value::Null,
                            artifacts: vec![],
                            provider_request_id: None,
                        })
                    }
                }
            })
        }

        fn list_resources<'a>(
            &'a self,
            _source: &'a str,
            _field_key: &'a str,
            query: &'a str,
            _page_token: Option<&'a str>,
            _connection: &'a AppConnection,
            _tokens: TokenAccessCapability,
            _cancellation: ActionCancellation,
        ) -> ActionResourcesFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let items = if query == "oversized" {
                    (0..=MAX_RESOURCE_ITEMS)
                        .map(|index| ActionResourceItem {
                            id: format!("C{index}"),
                            label: format!("Channel {index}"),
                        })
                        .collect()
                } else {
                    vec![ActionResourceItem {
                        id: "C123".into(),
                        label: format!("{query} channel"),
                    }]
                };
                Ok(ActionResourcePage {
                    items,
                    next_page_token: None,
                })
            })
        }
    }

    struct RefreshToken;

    impl RefreshHandler for RefreshToken {
        fn needs_refresh(&self, _connection: &AppConnection, _now: DateTime<Utc>) -> bool {
            true
        }

        fn refresh<'a>(
            &'a self,
            _connection: &'a AppConnection,
            mut credential: CredentialEnvelope,
        ) -> RefreshFuture<'a> {
            Box::pin(async move {
                credential.access_token = "rotated-secret-token".into();
                Ok(credential)
            })
        }
    }

    fn fixture() -> (Db, Arc<InMemoryTokenStore>, RefreshService, AppConnection) {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        store
            .put(
                "credential",
                &CredentialEnvelope::new("initial-secret-token".into()),
            )
            .expect("credential");
        let connection = db
            .upsert_app_connection(UpsertAppConnection {
                provider_id: "slack".into(),
                display_name: Some("Workspace".into()),
                external_account_id: None,
                external_tenant_id: None,
                connection_mode: "native_oauth".into(),
                identity_key: canonical_identity_key("slack", "native_oauth", &["workspace"]),
                scopes: vec!["chat:write".into()],
                provider_metadata: BTreeMap::new(),
                expires_at: None,
                credential_ref: "credential".into(),
            })
            .expect("connection");
        let refresh = RefreshService::new(store.clone());
        refresh
            .register("slack", Arc::new(RefreshToken))
            .expect("refresh handler");
        (db, store, refresh, connection)
    }

    fn register(
        registry: &ActionRegistry,
        behavior: FakeBehavior,
        limits: ActionLimits,
    ) -> Arc<AtomicUsize> {
        let calls = Arc::new(AtomicUsize::new(0));
        registry
            .register(
                descriptor(),
                limits,
                Arc::new(FakeExecutor {
                    calls: calls.clone(),
                    behavior,
                }),
            )
            .expect("register action");
        calls
    }

    #[test]
    fn rejects_duplicates_and_secret_fields_and_serializes_descriptors() {
        let registry = ActionRegistry::default();
        let executor = Arc::new(FakeExecutor {
            calls: Arc::new(AtomicUsize::new(0)),
            behavior: FakeBehavior::Success(Value::Null),
        });
        registry
            .register(descriptor(), ActionLimits::default(), executor.clone())
            .expect("first registration");
        assert_eq!(
            registry
                .register(descriptor(), ActionLimits::default(), executor)
                .unwrap_err()
                .code,
            ActionErrorCode::InvalidInput
        );
        let serialized = serde_json::to_string(&registry.descriptors(None)).expect("serialize");
        assert!(serialized.contains("slack.send_message"));
        assert!(!serialized.contains("token"));

        let mut unsafe_descriptor = descriptor();
        unsafe_descriptor.action_id = "slack.unsafe".into();
        unsafe_descriptor.fields[0].secret = true;
        assert!(registry
            .register(
                unsafe_descriptor,
                ActionLimits::default(),
                Arc::new(FakeExecutor {
                    calls: Arc::new(AtomicUsize::new(0)),
                    behavior: FakeBehavior::Success(Value::Null),
                })
            )
            .is_err());
    }

    #[test]
    fn validates_missing_unknown_and_typed_inputs() {
        let mut missing = request();
        missing.input.remove("message");
        assert_eq!(
            validate_request(missing, &descriptor()).unwrap_err().code,
            ActionErrorCode::InvalidInput
        );
        let mut unknown = request();
        unknown
            .input
            .insert("futureField".into(), Value::Bool(true));
        assert_eq!(
            validate_request(unknown, &descriptor()).unwrap_err().code,
            ActionErrorCode::InvalidInput
        );
        let mut wrong_type = request();
        wrong_type.input.insert("message".into(), Value::Bool(true));
        assert!(validate_request(wrong_type, &descriptor()).is_err());

        let mut with_snapshot = request();
        with_snapshot.input.insert(
            "channel__display".into(),
            Value::String("Engineering".into()),
        );
        assert_eq!(
            validate_request(with_snapshot, &descriptor())
                .expect("resource display snapshot")
                .input["channel__display"],
            Value::String("Engineering".into())
        );

        let mut unsafe_snapshot = request();
        unsafe_snapshot.input.insert(
            "channel__display".into(),
            Value::String("bad\u{0000}label".into()),
        );
        assert!(validate_request(unsafe_snapshot, &descriptor()).is_err());
    }

    #[tokio::test]
    async fn executes_success_and_resource_lookup_without_leaking_credentials() {
        let (db, store, refresh, connection) = fixture();
        let registry = ActionRegistry::default();
        let calls = register(
            &registry,
            FakeBehavior::Success(serde_json::json!({"ok": true})),
            ActionLimits::default(),
        );
        let mut action_request = request();
        action_request.connection_id = connection.id.clone();
        let result = registry
            .execute(
                &db,
                &refresh,
                store.clone(),
                action_request,
                ActionCancellation::never(),
            )
            .await
            .expect("execute");
        assert_eq!(result.output, serde_json::json!({"ok": true}));
        assert!(!serde_json::to_string(&result)
            .expect("serialize")
            .contains("initial-secret-token"));

        let page = registry
            .list_resources(
                &db,
                store.clone(),
                &connection.id,
                "slack",
                "slack.send_message",
                "channel",
                "eng",
                None,
            )
            .await
            .expect("resources");
        assert_eq!(page.items[0].label, "eng channel");
        let cached = registry
            .list_resources(
                &db,
                store,
                &connection.id,
                "slack",
                "slack.send_message",
                "channel",
                "eng",
                None,
            )
            .await
            .expect("cached resources");
        assert_eq!(cached, page);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn resource_lookup_enforces_query_and_item_bounds() {
        let (db, store, _refresh, connection) = fixture();
        let registry = ActionRegistry::default();
        register(
            &registry,
            FakeBehavior::Success(Value::Null),
            ActionLimits::default(),
        );
        let too_long = "x".repeat(MAX_RESOURCE_QUERY_BYTES + 1);
        let error = registry
            .list_resources(
                &db,
                store.clone(),
                &connection.id,
                "slack",
                "slack.send_message",
                "channel",
                &too_long,
                None,
            )
            .await
            .expect_err("query bound");
        assert_eq!(error.code, ActionErrorCode::InvalidInput);

        let error = registry
            .list_resources(
                &db,
                store,
                &connection.id,
                "slack",
                "slack.send_message",
                "channel",
                "oversized",
                None,
            )
            .await
            .expect_err("item bound");
        assert_eq!(error.code, ActionErrorCode::OutputInvalid);
    }

    #[tokio::test]
    async fn rejects_provider_mismatch_missing_scope_and_secret_output() {
        let (db, store, refresh, connection) = fixture();
        let registry = ActionRegistry::default();
        register(
            &registry,
            FakeBehavior::Success(Value::String("initial-secret-token".into())),
            ActionLimits::default(),
        );
        store
            .put(
                "gmail-credential",
                &CredentialEnvelope::new("gmail-secret-token".into()),
            )
            .expect("mismatched credential");
        let mismatched_connection = db
            .upsert_app_connection(UpsertAppConnection {
                provider_id: "gmail".into(),
                display_name: Some("Mailbox".into()),
                external_account_id: None,
                external_tenant_id: None,
                connection_mode: "native_oauth".into(),
                identity_key: canonical_identity_key("gmail", "native_oauth", &["mailbox"]),
                scopes: vec!["chat:write".into()],
                provider_metadata: BTreeMap::new(),
                expires_at: None,
                credential_ref: "gmail-credential".into(),
            })
            .expect("mismatched connection");
        let mut mismatched = request();
        mismatched.connection_id = mismatched_connection.id;
        assert_eq!(
            registry
                .execute(
                    &db,
                    &refresh,
                    store.clone(),
                    mismatched,
                    ActionCancellation::never(),
                )
                .await
                .unwrap_err()
                .code,
            ActionErrorCode::ConnectionRequired
        );

        db.with_conn(|conn| {
            conn.execute(
                "UPDATE app_connections SET scopes_json = '[]' WHERE id = ?1",
                rusqlite::params![connection.id],
            )?;
            Ok(())
        })
        .expect("remove scope");
        let mut missing_scope = request();
        missing_scope.connection_id = connection.id.clone();
        assert_eq!(
            registry
                .execute(
                    &db,
                    &refresh,
                    store.clone(),
                    missing_scope,
                    ActionCancellation::never(),
                )
                .await
                .unwrap_err()
                .code,
            ActionErrorCode::ScopeMissing
        );

        db.with_conn(|conn| {
            conn.execute(
                "UPDATE app_connections SET scopes_json = '[\"chat:write\"]' WHERE id = ?1",
                rusqlite::params![connection.id],
            )?;
            Ok(())
        })
        .expect("restore scope");
        let mut leaks = request();
        leaks.connection_id = connection.id;
        assert_eq!(
            registry
                .execute(&db, &refresh, store, leaks, ActionCancellation::never(),)
                .await
                .unwrap_err()
                .code,
            ActionErrorCode::OutputInvalid
        );
    }

    #[tokio::test]
    async fn rejects_output_over_byte_and_depth_limits() {
        for output in [
            Value::String("x".repeat(2_000)),
            serde_json::json!({"a":{"b":{"c":{"d":true}}}}),
        ] {
            let (db, store, refresh, connection) = fixture();
            let registry = ActionRegistry::default();
            register(
                &registry,
                FakeBehavior::Success(output),
                ActionLimits {
                    max_output_bytes: 1_024,
                    max_output_depth: 3,
                    timeout: Duration::from_secs(1),
                },
            );
            let mut action_request = request();
            action_request.connection_id = connection.id;
            let error = registry
                .execute(
                    &db,
                    &refresh,
                    store,
                    action_request,
                    ActionCancellation::never(),
                )
                .await
                .expect_err("limit rejection");
            assert_eq!(error.code, ActionErrorCode::OutputTooLarge);
        }
    }

    #[tokio::test]
    async fn timeout_and_cancellation_drop_the_inflight_future() {
        for cancelled in [false, true] {
            let (db, store, refresh, connection) = fixture();
            let registry = ActionRegistry::default();
            let completions = Arc::new(AtomicUsize::new(0));
            register(
                &registry,
                FakeBehavior::Sleep(Duration::from_millis(100), completions.clone()),
                ActionLimits {
                    timeout: Duration::from_millis(20),
                    ..ActionLimits::default()
                },
            );
            let flag = Arc::new(AtomicBool::new(false));
            if cancelled {
                flag.store(true, Ordering::SeqCst);
            }
            let mut action_request = request();
            action_request.connection_id = connection.id;
            let error = registry
                .execute(
                    &db,
                    &refresh,
                    store,
                    action_request,
                    ActionCancellation::new(flag),
                )
                .await
                .expect_err("interrupted");
            assert_eq!(
                error.code,
                if cancelled {
                    ActionErrorCode::Cancelled
                } else {
                    ActionErrorCode::TimedOut
                }
            );
            tokio::time::sleep(Duration::from_millis(120)).await;
            assert_eq!(completions.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn unauthorized_refreshes_and_retries_exactly_once() {
        let (db, store, refresh, connection) = fixture();
        let registry = ActionRegistry::default();
        let calls = register(
            &registry,
            FakeBehavior::UnauthorizedOnce,
            ActionLimits::default(),
        );
        let mut action_request = request();
        action_request.connection_id = connection.id;
        let result = registry
            .execute(
                &db,
                &refresh,
                store,
                action_request,
                ActionCancellation::never(),
            )
            .await
            .expect("retry success");
        assert_eq!(result.summary, "Retried");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retry_result_cannot_leak_the_pre_refresh_token() {
        let (db, store, refresh, connection) = fixture();
        let registry = ActionRegistry::default();
        register(
            &registry,
            FakeBehavior::UnauthorizedThen(Value::String("initial-secret-token".into())),
            ActionLimits::default(),
        );
        let mut action_request = request();
        action_request.connection_id = connection.id;
        let error = registry
            .execute(
                &db,
                &refresh,
                store,
                action_request,
                ActionCancellation::never(),
            )
            .await
            .expect_err("old token leak");
        assert_eq!(error.code, ActionErrorCode::OutputInvalid);
    }

    #[test]
    fn rejects_secret_like_descriptor_keys_and_defaults() {
        let executor = || {
            Arc::new(FakeExecutor {
                calls: Arc::new(AtomicUsize::new(0)),
                behavior: FakeBehavior::Success(Value::Null),
            })
        };
        let registry = ActionRegistry::default();
        let mut secret_key = descriptor();
        secret_key.action_id = "slack.secret_key".into();
        secret_key.fields[1].key = "api_token".into();
        assert_eq!(
            registry
                .register(secret_key, ActionLimits::default(), executor())
                .expect_err("secret-like key")
                .code,
            ActionErrorCode::InvalidInput
        );

        let mut secret_default = descriptor();
        secret_default.action_id = "slack.secret_default".into();
        secret_default.fields[1].default = Some(Value::String("Bearer fixture".into()));
        assert_eq!(
            registry
                .register(secret_default, ActionLimits::default(), executor())
                .expect_err("secret-like default")
                .code,
            ActionErrorCode::InvalidInput
        );
    }

    #[test]
    fn action_errors_never_accept_raw_provider_text() {
        let error = ActionError::new(ActionErrorCode::ProviderUnavailable)
            .with_request_id("Bearer secret fixture");
        let serialized = serde_json::to_string(&error).expect("serialize");
        assert!(!serialized.contains("Bearer secret fixture"));
        assert!(error.provider_request_id.is_none());
    }
}
