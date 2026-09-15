//! The memory reviewer execution boundary.
//!
//! This module deliberately does not reuse `AgentRequest` or `AgentAdapter`.
//! A review receives only the bounded text prepared by the curator and has no
//! working directory, skills, session id, tool policy, or caller supplied
//! command. Provider implementations live below this boundary and must fail
//! closed when their restriction contract is not verified.

pub mod claude_cli;

use super::AgentProvider;
use std::boxed::Box;
use std::sync::Arc;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;
use thiserror::Error;

pub const REVIEW_CONTRACT_VERSION: &str = "memory-review-restricted-v1";
pub const REVIEW_INPUT_MAX_BYTES: usize = 64 * 1024;
pub const REVIEW_RESPONSE_MAX_BYTES: usize = 32 * 1024;
pub const REVIEW_TIMEOUT: Duration = Duration::from_secs(120);
pub const REVIEW_ACTIVE_LIMIT: usize = 1;
pub const REVIEW_READY_QUEUE_LIMIT: usize = 32;
pub const REVIEW_ERROR_QUEUE_FULL: &str = "review_queue_full";

struct ReviewQueueState {
    queued: usize,
    active: bool,
    drain: Option<Box<dyn FnOnce() + Send + 'static>>,
}

/// A bounded in-process queue. SQLite remains the durable queue for work that
/// does not fit; this guard only limits live review tasks and provider calls.
pub struct ReviewQueuePermit {
    state: Arc<(Mutex<ReviewQueueState>, Condvar)>,
    entered: bool,
}

#[derive(Clone)]
struct ReviewQueue {
    state: Arc<(Mutex<ReviewQueueState>, Condvar)>,
}

impl ReviewQueue {
    fn new() -> Self {
        Self {
            state: Arc::new((
                Mutex::new(ReviewQueueState {
                    queued: 0,
                    active: false,
                    drain: None,
                }),
                Condvar::new(),
            )),
        }
    }

    fn try_enqueue(&self) -> Option<ReviewQueuePermit> {
        let mut guard = self.state.0.lock().ok()?;
        let occupied = guard.queued + usize::from(guard.active);
        if occupied >= REVIEW_READY_QUEUE_LIMIT + REVIEW_ACTIVE_LIMIT {
            return None;
        }
        guard.queued += 1;
        Some(ReviewQueuePermit {
            state: Arc::clone(&self.state),
            entered: false,
        })
    }

    fn register_drain(&self, callback: impl FnOnce() + Send + 'static) {
        let mut run_now = None;
        if let Ok(mut guard) = self.state.0.lock() {
            let occupied = guard.queued + usize::from(guard.active);
            if occupied < REVIEW_READY_QUEUE_LIMIT + REVIEW_ACTIVE_LIMIT {
                run_now = guard
                    .drain
                    .take()
                    .or_else(|| Some(Box::new(callback) as Box<dyn FnOnce() + Send>));
            } else if guard.drain.is_none() {
                guard.drain = Some(Box::new(callback));
            }
        }
        if let Some(callback) = run_now {
            callback();
        }
    }
}

static REVIEW_QUEUE: OnceLock<ReviewQueue> = OnceLock::new();

fn review_queue() -> &'static ReviewQueue {
    REVIEW_QUEUE.get_or_init(ReviewQueue::new)
}

pub fn try_enqueue_review() -> Option<ReviewQueuePermit> {
    review_queue().try_enqueue()
}

/// Register one wakeup for durable pending work that did not fit in the
/// bounded in-memory queue. At most one callback is retained, so overload
/// cannot turn into an unbounded collection of closures.
pub fn register_review_drain(callback: impl FnOnce() + Send + 'static) {
    review_queue().register_drain(callback);
}

impl ReviewQueuePermit {
    pub fn wait_for_turn(&mut self) {
        let Ok(mut guard) = self.state.0.lock() else {
            return;
        };
        while guard.active {
            guard = self
                .state
                .1
                .wait(guard)
                .unwrap_or_else(|error| error.into_inner());
        }
        guard.queued = guard.queued.saturating_sub(1);
        guard.active = true;
        self.entered = true;
    }
}

impl Drop for ReviewQueuePermit {
    fn drop(&mut self) {
        let mut drain = None;
        if let Ok(mut guard) = self.state.0.lock() {
            if self.entered {
                guard.active = false;
            } else {
                guard.queued = guard.queued.saturating_sub(1);
            }
            self.state.1.notify_one();
            drain = guard.drain.take();
        }
        if let Some(callback) = drain {
            callback();
        }
    }
}

/// Identity persisted with every job. It is intentionally descriptive rather
/// than a credential reference: provider authentication remains CLI-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewExecutionIdentity {
    pub provider: AgentProvider,
    pub product: String,
    pub runtime_version: String,
    pub execution_mode: String,
    pub restriction_contract_version: String,
}

impl ReviewExecutionIdentity {
    pub fn claude_cli(runtime_version: impl Into<String>) -> Self {
        Self {
            provider: AgentProvider::ClaudeCode,
            product: "claude_code".into(),
            runtime_version: runtime_version.into(),
            execution_mode: "restricted_print".into(),
            restriction_contract_version: REVIEW_CONTRACT_VERSION.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLimits {
    pub(crate) input_max_bytes: usize,
    pub(crate) response_max_bytes: usize,
    pub(crate) timeout: Duration,
}

impl Default for ReviewLimits {
    fn default() -> Self {
        Self {
            input_max_bytes: REVIEW_INPUT_MAX_BYTES,
            response_max_bytes: REVIEW_RESPONSE_MAX_BYTES,
            timeout: REVIEW_TIMEOUT,
        }
    }
}

impl ReviewLimits {
    pub(crate) fn validate(&self) -> Result<(), ReviewError> {
        if self.input_max_bytes == 0
            || self.response_max_bytes == 0
            || self.timeout.is_zero()
            || self.input_max_bytes > REVIEW_INPUT_MAX_BYTES
            || self.response_max_bytes > REVIEW_RESPONSE_MAX_BYTES
            || self.timeout > REVIEW_TIMEOUT
        {
            return Err(ReviewError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReviewCancellation(Option<Arc<std::sync::atomic::AtomicBool>>);

impl ReviewCancellation {
    pub fn new() -> (Self, Arc<std::sync::atomic::AtomicBool>) {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        (Self(Some(flag.clone())), flag)
    }

    pub fn is_cancelled(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::SeqCst))
    }
}

/// Typed, bounded input for a restricted review invocation.
#[derive(Debug, Clone)]
pub struct RestrictedReviewRequest {
    pub identity: ReviewExecutionIdentity,
    pub model: Option<String>,
    pub prompt: String,
    pub limits: ReviewLimits,
    pub cancellation: ReviewCancellation,
}

impl RestrictedReviewRequest {
    pub fn new(
        identity: ReviewExecutionIdentity,
        model: Option<String>,
        prompt: String,
    ) -> Result<Self, ReviewError> {
        let limits = ReviewLimits::default();
        Self::with_limits(identity, model, prompt, limits)
    }

    pub fn with_limits(
        identity: ReviewExecutionIdentity,
        model: Option<String>,
        prompt: String,
        limits: ReviewLimits,
    ) -> Result<Self, ReviewError> {
        if prompt.len() > limits.input_max_bytes {
            return Err(ReviewError::InputTooLarge {
                limit: limits.input_max_bytes,
            });
        }
        if model.as_deref().is_some_and(|value| value.len() > 200) {
            return Err(ReviewError::InvalidRequest);
        }
        limits.validate()?;
        Ok(Self {
            identity,
            model,
            prompt,
            limits,
            cancellation: ReviewCancellation::default(),
        })
    }

    pub fn with_cancellation(mut self, cancellation: ReviewCancellation) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), ReviewError> {
        self.limits.validate()?;
        if self.prompt.len() > self.limits.input_max_bytes
            || self.model.as_deref().is_some_and(|value| value.len() > 200)
        {
            return Err(ReviewError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewUsage {
    pub response_bytes: usize,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictedReviewResponse {
    pub output: String,
    pub usage: ReviewUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedReviewCapability {
    pub identity: ReviewExecutionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableReviewCapability {
    pub provider: AgentProvider,
    pub product: String,
    pub runtime_version: Option<String>,
    pub execution_mode: String,
    pub restriction_contract_version: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewAvailability {
    Supported(SupportedReviewCapability),
    Unavailable(UnavailableReviewCapability),
}

impl ReviewAvailability {
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Supported(_))
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Supported(_) => None,
            Self::Unavailable(value) => Some(&value.reason),
        }
    }
}

/// Review errors are stable and safe to map to the durable job error code.
/// Provider diagnostics are deliberately not carried by this type.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ReviewError {
    #[error("restricted review is unavailable")]
    Unavailable(UnavailableReviewCapability),
    #[error("review input exceeds its byte limit")]
    InputTooLarge { limit: usize },
    #[error("review response exceeds its byte limit")]
    ResponseTooLarge { limit: usize },
    #[error("review timed out")]
    Timeout,
    #[error("review cancelled")]
    Cancelled,
    #[error("review requested a tool")]
    ToolRequest,
    #[error("invalid restricted review request")]
    InvalidRequest,
    #[error("restricted review process failed")]
    Process,
    #[error("review authentication is unavailable")]
    Authentication,
    #[error("restricted review internal failure")]
    Internal,
}

pub trait RestrictedReviewAgent: Send + Sync {
    fn run_review(
        &self,
        request: RestrictedReviewRequest,
    ) -> Result<RestrictedReviewResponse, ReviewError>;
}

#[cfg(test)]
mod contract {
    use super::*;

    fn request(prompt: String) -> RestrictedReviewRequest {
        RestrictedReviewRequest::new(ReviewExecutionIdentity::claude_cli("2.1.263"), None, prompt)
            .expect("request")
    }

    #[test]
    fn identity_is_explicit_and_has_no_workspace_or_tool_fields() {
        let value = request("review this bounded digest".into());
        assert_eq!(value.identity.execution_mode, "restricted_print");
        assert_eq!(
            value.identity.restriction_contract_version,
            REVIEW_CONTRACT_VERSION
        );
        assert!(!format!("{value:?}").contains("working_directory"));
    }

    #[test]
    fn input_limit_is_checked_before_the_request_is_constructed() {
        let error = RestrictedReviewRequest::new(
            ReviewExecutionIdentity::claude_cli("2.1.263"),
            None,
            "x".repeat(REVIEW_INPUT_MAX_BYTES + 1),
        )
        .expect_err("oversized input");
        assert_eq!(
            error,
            ReviewError::InputTooLarge {
                limit: REVIEW_INPUT_MAX_BYTES
            }
        );
    }

    #[test]
    fn limits_cannot_be_raised_above_hard_caps() {
        let error = RestrictedReviewRequest::with_limits(
            ReviewExecutionIdentity::claude_cli("2.1.263"),
            None,
            "bounded".into(),
            ReviewLimits {
                input_max_bytes: REVIEW_INPUT_MAX_BYTES + 1,
                response_max_bytes: REVIEW_RESPONSE_MAX_BYTES,
                timeout: REVIEW_TIMEOUT,
            },
        )
        .expect_err("hard input cap");
        assert_eq!(error, ReviewError::InvalidRequest);
    }

    #[test]
    fn cancellation_is_observable_without_provider_access() {
        let (cancellation, flag) = ReviewCancellation::new();
        assert!(!cancellation.is_cancelled());
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn availability_has_stable_supported_and_unavailable_identity() {
        let supported = ReviewAvailability::Supported(SupportedReviewCapability {
            identity: ReviewExecutionIdentity::claude_cli("2.1.263"),
        });
        assert!(supported.is_supported());
        assert_eq!(supported.reason(), None);
        let unavailable = ReviewAvailability::Unavailable(UnavailableReviewCapability {
            provider: AgentProvider::ClaudeCode,
            product: "claude_code".into(),
            runtime_version: Some("future".into()),
            execution_mode: "restricted_print".into(),
            restriction_contract_version: REVIEW_CONTRACT_VERSION.into(),
            reason: "unknown_runtime_version".into(),
        });
        assert!(!unavailable.is_supported());
        assert_eq!(unavailable.reason(), Some("unknown_runtime_version"));
    }

    #[test]
    fn queue_has_one_active_and_thirty_two_ready_slots() {
        let queue = ReviewQueue::new();
        let mut permits: Vec<_> = (0..(REVIEW_ACTIVE_LIMIT + REVIEW_READY_QUEUE_LIMIT))
            .map(|_| queue.try_enqueue().expect("bounded slot"))
            .collect();
        assert!(queue.try_enqueue().is_none(), "queue must remain bounded");
        let mut active = permits.pop().expect("active permit");
        active.wait_for_turn();
        assert!(
            queue.try_enqueue().is_none(),
            "active plus ready remain bounded"
        );
        drop(active);
        drop(permits);
    }
}
