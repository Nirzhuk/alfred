//! Plan 005 matrix D — offline and restrictive-state behavior.
//!
//! Every boundary here is proven with an injected clock (`FixedClock`) and a
//! loopback mock, never by moving the system clock or touching the network.
//! These tests encode the shipped policy; they must not be relaxed to make a
//! product change pass.

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Response, Server};

use crate::db::{
    CreateMemoryInput, CreateWorkflowInput, Db, OnMissed, StoredLicenseSnapshot,
    UpsertScheduleInput, UpsertTriggerInput,
};

use super::client::{PolarClient, PolarClientError};
use super::config::PolarConfig;
use super::models::{LicenseProduct, LicenseStatus, LicenseStatusDto};
use super::offline::{
    evaluate_cached_state, state_after_transient_failure, OFFLINE_GRACE_DAYS, REFRESH_AFTER_DAYS,
};
use super::service::{LicenseClock, LicenseService};
use super::store::{
    InMemoryLicenseCredentialStore, LicenseCredentialEnvelope, LicenseCredentialStore,
};
use super::update_window::is_in_update_window;

const KEY: &str = "TEST-LICENSE-KEY-SECRET";
const ACTIVATION: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const BENEFIT: &str = "11111111-1111-4111-8111-111111111111";
const UNKNOWN_BENEFIT: &str = "44444444-4444-4444-8444-444444444444";

/// The single injected origin every day offset is measured from.
fn validated_at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap()
}

fn day(offset: i64) -> DateTime<Utc> {
    validated_at() + Duration::days(offset)
}

fn refresh_due() -> DateTime<Utc> {
    day(REFRESH_AFTER_DAYS)
}

fn offline_deadline() -> DateTime<Utc> {
    day(OFFLINE_GRACE_DAYS)
}

/// The transient classes plan 005 names: timeout, DNS/connect failure, 429, 5xx.
const TRANSIENT_ERRORS: [PolarClientError; 4] = [
    PolarClientError::Timeout,
    PolarClientError::Connectivity,
    PolarClientError::RateLimited,
    PolarClientError::ServiceUnavailable,
];

#[derive(Debug)]
struct FixedClock(Mutex<DateTime<Utc>>);

impl FixedClock {
    fn at(time: DateTime<Utc>) -> Self {
        Self(Mutex::new(time))
    }
}

impl LicenseClock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().expect("clock")
    }
}

fn spawn_server(responses: Vec<(u16, String)>) -> (PolarConfig, std::thread::JoinHandle<()>) {
    let server = Server::http(("127.0.0.1", 0)).expect("mock server");
    let port = server.server_addr().to_ip().expect("mock address").port();
    let thread = std::thread::spawn(move || {
        for (status, body) in responses {
            let request = server.recv().expect("request");
            let content_type =
                Header::from_bytes("Content-Type", "application/json").expect("header");
            request
                .respond(
                    Response::from_string(body)
                        .with_status_code(status)
                        .with_header(content_type),
                )
                .expect("response");
        }
    });
    (loopback_config(port), thread)
}

/// A port nothing listens on, standing in for the DNS/connect failure class
/// that `map_reqwest_error` folds into `PolarClientError::Connectivity`.
fn unreachable_config() -> PolarConfig {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("probe socket");
    let port = listener.local_addr().expect("probe address").port();
    drop(listener);
    loopback_config(port)
}

fn loopback_config(port: u16) -> PolarConfig {
    PolarConfig::for_test(url::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap())
}

fn build_service(
    config: PolarConfig,
    store: Arc<dyn LicenseCredentialStore>,
    now: DateTime<Utc>,
) -> LicenseService {
    let client = PolarClient::new(&config).expect("client");
    LicenseService::configured(config, client, store, Arc::new(FixedClock::at(now)))
}

fn validation_body(status: &str, benefit: &str, expires_at: Option<&str>) -> Value {
    json!({
        "benefit_id": benefit,
        "status": status,
        "expires_at": expires_at,
        "key": "raw-response-secret"
    })
}

/// Seeds a snapshot that was last validated successfully at `validated_at()`,
/// so day offsets in the assertions map directly onto the 7/30-day policy.
/// `expires_at` plants Polar's own update deadline in the stored row, the
/// value a later 404 is arbitrated against.
fn seed_validated(
    db: &Db,
    store: &dyn LicenseCredentialStore,
    state: LicenseStatus,
    expires_at: Option<DateTime<Utc>>,
) {
    store
        .put(
            "credential",
            &LicenseCredentialEnvelope::new(KEY.into(), ACTIVATION.into()),
        )
        .expect("seed credential");
    db.put_license_snapshot(&StoredLicenseSnapshot {
        product: "individual".into(),
        status: state.as_db_value().into(),
        masked_key: Some("••••-CRET".into()),
        benefit_id: Some(BENEFIT.into()),
        activation_label: Some("Acceptance Device".into()),
        current_device: true,
        expires_at: expires_at.map(|time| time.to_rfc3339()),
        last_success_at: Some(validated_at().to_rfc3339()),
        refresh_due_at: Some(refresh_due().to_rfc3339()),
        offline_deadline: Some(offline_deadline().to_rfc3339()),
        error_code: None,
        credential_ref: Some("credential".into()),
        updated_at: validated_at().to_rfc3339(),
    })
    .expect("seed snapshot");
}

// === D1–D2: the 7-day refresh boundary ======================================

#[test]
fn cached_active_state_before_day_seven_never_triggers_a_refresh() {
    for offset in 0..REFRESH_AFTER_DAYS {
        let evaluation = evaluate_cached_state(
            LicenseStatus::Active,
            None,
            Some(refresh_due()),
            Some(offline_deadline()),
            day(offset),
        );
        assert!(
            !evaluation.should_refresh,
            "day {offset} must not be due for refresh"
        );
        assert_eq!(evaluation.state, LicenseStatus::Active, "day {offset}");
    }
    assert!(
        !evaluate_cached_state(
            LicenseStatus::Active,
            None,
            Some(refresh_due()),
            Some(offline_deadline()),
            refresh_due() - Duration::nanoseconds(1),
        )
        .should_refresh
    );
}

#[test]
fn refresh_becomes_due_exactly_at_day_seven_and_stays_due() {
    for (offset, expected_due) in [(6, false), (7, true), (8, true)] {
        let evaluation = evaluate_cached_state(
            LicenseStatus::Active,
            None,
            Some(refresh_due()),
            Some(offline_deadline()),
            day(offset),
        );
        assert_eq!(
            evaluation.should_refresh, expected_due,
            "day {offset} refresh due"
        );
        // Being due for refresh never downgrades a cached grant on its own.
        assert_eq!(
            evaluation.state,
            LicenseStatus::Active,
            "day {offset} state"
        );
    }
    assert!(
        evaluate_cached_state(
            LicenseStatus::Active,
            None,
            Some(refresh_due()),
            Some(offline_deadline()),
            refresh_due(),
        )
        .should_refresh
    );
}

// === D3–D4: the 30-day offline grace boundary ===============================

#[test]
fn every_transient_failure_class_yields_offline_grace_through_day_thirty() {
    for error in TRANSIENT_ERRORS {
        assert!(error.is_transient(), "{error:?} must be transient");
        for prior in [LicenseStatus::Active, LicenseStatus::OfflineGrace] {
            for offset in [0, 1, 7, 8, 29, OFFLINE_GRACE_DAYS] {
                assert_eq!(
                    state_after_transient_failure(prior, Some(offline_deadline()), day(offset)),
                    LicenseStatus::OfflineGrace,
                    "{error:?} from {prior:?} on day {offset}"
                );
            }
        }
    }
}

#[test]
fn offline_grace_ends_exactly_after_day_thirty() {
    for (moment, expected, label) in [
        (day(29), LicenseStatus::OfflineGrace, "day 29"),
        (offline_deadline(), LicenseStatus::OfflineGrace, "day 30"),
        (
            offline_deadline() + Duration::nanoseconds(1),
            LicenseStatus::NeedsOnline,
            "day 30 + 1ns",
        ),
        (day(31), LicenseStatus::NeedsOnline, "day 31"),
    ] {
        assert_eq!(
            state_after_transient_failure(
                LicenseStatus::OfflineGrace,
                Some(offline_deadline()),
                moment,
            ),
            expected,
            "transient failure at {label}"
        );
    }
}

#[test]
fn cached_reads_expose_needs_online_only_after_the_day_thirty_deadline() {
    for (moment, expected, label) in [
        (day(29), LicenseStatus::OfflineGrace, "day 29"),
        (offline_deadline(), LicenseStatus::OfflineGrace, "day 30"),
        (
            offline_deadline() + Duration::nanoseconds(1),
            LicenseStatus::NeedsOnline,
            "day 30 + 1ns",
        ),
        (day(31), LicenseStatus::NeedsOnline, "day 31"),
    ] {
        let evaluation = evaluate_cached_state(
            LicenseStatus::OfflineGrace,
            None,
            Some(refresh_due()),
            Some(offline_deadline()),
            moment,
        );
        assert_eq!(evaluation.state, expected, "cached read at {label}");
        assert!(
            evaluation.should_refresh,
            "cached read at {label} must retry"
        );
    }
}

// === D5: no grace without a prior confirmed grant ===========================

#[test]
fn a_key_that_was_never_validated_receives_no_offline_grace() {
    for prior in [
        LicenseStatus::Unlicensed,
        LicenseStatus::NeedsOnline,
        LicenseStatus::DeviceLimit,
        LicenseStatus::SecureStorageUnavailable,
        LicenseStatus::NotConfigured,
    ] {
        // Neither a missing deadline nor a generously seeded one grants grace.
        for deadline in [None, Some(offline_deadline())] {
            for offset in [0, 1, 7, 29, OFFLINE_GRACE_DAYS, 31] {
                assert_eq!(
                    state_after_transient_failure(prior, deadline, day(offset),),
                    prior,
                    "{prior:?} on day {offset} must not gain grace"
                );
            }
        }
        assert_eq!(
            evaluate_cached_state(
                prior,
                None,
                Some(refresh_due()),
                Some(offline_deadline()),
                day(31)
            )
            .state,
            prior,
            "{prior:?} cached read must not be rewritten"
        );
    }
}

#[tokio::test]
async fn an_unknown_benefit_key_receives_no_offline_grace() {
    let (config, thread) = spawn_server(vec![(
        200,
        validation_body("granted", UNKNOWN_BENEFIT, None).to_string(),
    )]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::Active, None);
    let service = build_service(config, store.clone(), day(7));

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::Disabled);
    assert_eq!(status.error_code.as_deref(), Some("unsupported_product"));
    thread.join().expect("server");

    // A later outage cannot promote that unsupported product into grace.
    let service = build_service(unreachable_config(), store, day(8));
    let status = service.refresh(&db).await.expect("offline refresh");
    assert_eq!(status.state, LicenseStatus::Disabled);
    assert_ne!(status.state, LicenseStatus::OfflineGrace);
}

// === D6: a confirmed restrictive answer beats remaining grace ===============

#[tokio::test]
async fn a_confirmed_restrictive_response_overrides_remaining_grace_immediately() {
    for (body, expected_state, expected_code) in [
        (
            validation_body("revoked", BENEFIT, None),
            LicenseStatus::Revoked,
            "license_revoked",
        ),
        (
            validation_body("disabled", BENEFIT, None),
            LicenseStatus::Disabled,
            "license_disabled",
        ),
    ] {
        let (config, thread) = spawn_server(vec![(200, body.to_string())]);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let db = Db::open_in_memory().expect("database");
        seed_validated(&db, store.as_ref(), LicenseStatus::OfflineGrace, None);
        // Day 8 of a 30-day window: 22 days of grace are still unspent.
        let service = build_service(config, store, day(8));

        let status = service.refresh(&db).await.expect("refresh");
        assert_eq!(status.state, expected_state);
        assert_eq!(status.error_code.as_deref(), Some(expected_code));
        assert_ne!(status.state, LicenseStatus::OfflineGrace);

        // The restriction persists on the next cached read, still inside grace.
        let cached = build_service(
            loopback_config(1),
            Arc::new(InMemoryLicenseCredentialStore::default()),
            day(9),
        )
        .get_status(&db)
        .expect("cached status");
        assert_eq!(cached.state, expected_state);
        thread.join().expect("server");
    }
}

/// Plan 007: `expired` is "entitled, update window closed", not a loss of
/// access. It is the one state that must NOT be folded in with the two
/// verdicts above, so it gets its own case rather than a row in that loop.
#[tokio::test]
async fn a_closed_update_window_keeps_entitlement_while_revoked_and_disabled_end_it() {
    let closed = validation_body("granted", BENEFIT, Some("2026-08-03T12:00:00Z"));
    let (config, thread) = spawn_server(vec![(200, closed.to_string())]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::OfflineGrace, None);
    let service = build_service(config, store, day(8));

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::Expired);
    assert!(
        status.state.is_entitled(),
        "an expired key still proves a completed purchase"
    );
    assert_eq!(status.product, LicenseProduct::Individual);
    // Re-serialized from Polar's value, so the offset is spelled out.
    assert_eq!(
        status.update_deadline.as_deref(),
        Some("2026-08-03T12:00:00+00:00")
    );
    thread.join().expect("server");

    // It stays expired-and-entitled on every later cached read, and never
    // decays into `needsOnline` however long the app sits offline.
    for offset in [9, 31, 400] {
        let cached = build_service(
            loopback_config(1),
            Arc::new(InMemoryLicenseCredentialStore::default()),
            day(offset),
        )
        .get_status(&db)
        .expect("cached status");
        assert_eq!(cached.state, LicenseStatus::Expired, "day {offset}");
        assert!(cached.state.is_entitled(), "day {offset}");
    }

    // The two verdicts that do end entitlement keep doing so.
    for state in [LicenseStatus::Revoked, LicenseStatus::Disabled] {
        assert!(!state.is_entitled(), "{state:?} must end entitlement");
    }
}

/// An in-window build keeps its features forever; only a build released after
/// the deadline falls outside what the customer bought.
#[test]
fn the_update_window_is_decided_by_the_build_release_date_alone() {
    let deadline = Some("2027-01-15T09:30:00Z");
    assert!(is_in_update_window(Some("2026-08-20"), deadline));
    assert!(is_in_update_window(Some("2027-01-15"), deadline));
    assert!(!is_in_update_window(Some("2027-01-16"), deadline));
    // A source build has no release date and is never locked.
    assert!(is_in_update_window(None, deadline));
}

// === Matrix D, update-window rows W1/W2/W5–W8 ===============================

/// W1 — a build released inside the key's update window is entitled: a
/// `granted` answer with a live deadline keeps pro features on.
#[tokio::test]
async fn w1_an_in_window_build_is_active_and_entitled() {
    // A deadline a year out from the injected clock: day(7) is inside it.
    let (config, thread) = spawn_server(vec![(
        200,
        validation_body("granted", BENEFIT, Some(day(365).to_rfc3339()).as_deref()).to_string(),
    )]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::Active, None);
    let service = build_service(config, store, day(7));

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::Active);
    assert!(status.state.is_entitled(), "an in-window grant is entitled");
    assert!(status.in_update_window);
    thread.join().expect("server");
}

/// W2 — exactly at the deadline is IN window: the rule is `<=`, never `<`.
/// The pure window rule proves it for the release date; at service level the
/// same instant on Polar's own clock (`now >= expires_at`) reads as
/// expired-but-entitled with the window still open, and never as revoked.
#[tokio::test]
async fn w2_exactly_at_the_deadline_is_in_window_and_never_ends_entitlement() {
    // Release day == deadline day is inside the window.
    assert!(is_in_update_window(
        Some("2026-08-01"),
        Some("2026-08-01T12:00:00Z"),
    ));

    let (config, thread) = spawn_server(vec![(
        200,
        validation_body(
            "granted",
            BENEFIT,
            Some(validated_at().to_rfc3339()).as_deref(),
        )
        .to_string(),
    )]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::Active, None);
    // The injected clock sits exactly on the key's deadline.
    let service = build_service(config, store, validated_at());

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(
        status.state,
        LicenseStatus::Expired,
        "Polar's own rule is now >= expires_at, so equality lapses the key"
    );
    assert!(
        status.state.is_entitled(),
        "equality never ends entitlement"
    );
    assert!(
        status.in_update_window,
        "a build of the deadline day is still inside the window"
    );
    assert_eq!(
        status.error_code.as_deref(),
        Some(super::service::UPDATE_WINDOW_CLOSED)
    );
    thread.join().expect("server");
}

/// W5 — Polar answers 404 once a key lapses and never returns a 200 again.
/// Arbitrated against the locally persisted deadline that has since passed,
/// the license reads `Expired` — entitlement intact, permanently — and not
/// revoked. The body is deliberately prose-free: the decision must not
/// depend on Polar's English `detail` strings.
#[tokio::test]
async fn w5_a_404_against_a_past_stored_deadline_keeps_entitlement_as_expired() {
    let (config, thread) = spawn_server(vec![(404, "{}".into())]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    // The stored deadline passed one day after the last successful validation;
    // the app only talks to Polar again on day 7, when the key has lapsed.
    seed_validated(&db, store.as_ref(), LicenseStatus::Active, Some(day(1)));
    let service = build_service(config, store, day(7));

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::Expired);
    assert!(
        status.state.is_entitled(),
        "an expiry date is not a loss of purchase"
    );
    assert_eq!(status.product, LicenseProduct::Individual);
    assert_eq!(
        status.error_code.as_deref(),
        Some(super::service::UPDATE_WINDOW_CLOSED)
    );
    thread.join().expect("server");

    // It stays expired-and-entitled on later cached reads.
    let cached = build_service(
        loopback_config(1),
        Arc::new(InMemoryLicenseCredentialStore::default()),
        day(8),
    )
    .get_status(&db)
    .expect("cached status");
    assert_eq!(cached.state, LicenseStatus::Expired);
    assert!(cached.state.is_entitled());
}

/// The other side of the W5 arbitration: a 404 while the persisted deadline
/// is still in the future cannot be an expiry, so it stays a revocation.
#[tokio::test]
async fn a_404_before_the_stored_deadline_still_revokes() {
    let (config, thread) = spawn_server(vec![(404, "{}".into())]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::Active, Some(day(365)));
    let service = build_service(config, store, day(7));

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::Revoked);
    assert_eq!(status.error_code.as_deref(), Some("invalid_license"));
    assert!(!status.state.is_entitled());
    thread.join().expect("server");
}

/// W6/W7 — a confirmed `revoked` or `disabled` answer ends entitlement
/// immediately, whatever the stored deadline says. These are real verdicts
/// on a 200 body, so no arbitration may soften them.
#[tokio::test]
async fn w6_w7_confirmed_revoked_and_disabled_end_entitlement_even_with_a_future_deadline() {
    for (body, expected_state, expected_code) in [
        (
            validation_body("revoked", BENEFIT, Some(day(365).to_rfc3339()).as_deref()),
            LicenseStatus::Revoked,
            "license_revoked",
        ),
        (
            validation_body("disabled", BENEFIT, Some(day(365).to_rfc3339()).as_deref()),
            LicenseStatus::Disabled,
            "license_disabled",
        ),
    ] {
        let (config, thread) = spawn_server(vec![(200, body.to_string())]);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let db = Db::open_in_memory().expect("database");
        seed_validated(&db, store.as_ref(), LicenseStatus::Active, None);
        let service = build_service(config, store, day(7));

        let status = service.refresh(&db).await.expect("refresh");
        assert_eq!(status.state, expected_state);
        assert_eq!(status.error_code.as_deref(), Some(expected_code));
        assert!(
            !status.state.is_entitled(),
            "{expected_code} must end entitlement immediately"
        );
        thread.join().expect("server");
    }
}

/// W8 — an unset `ALFRED_RELEASE_DATE` is a source build: no window
/// comparison ever runs, so even a long-lapsed deadline locks nothing.
#[test]
fn w8_an_unset_release_date_never_locks_anything() {
    assert!(is_in_update_window(None, Some("2020-01-01T00:00:00Z")));
    assert!(is_in_update_window(None, None));

    // This checkout carries no baked release date, so the stored-contract
    // path must report the window open however far back the deadline sits.
    if super::update_window::BUILD_RELEASE_DATE.is_none() {
        let mut dto = LicenseStatusDto::unlicensed();
        dto.set_update_deadline(Some("2020-01-01T00:00:00Z".into()));
        assert!(dto.in_update_window);
    }
}

#[tokio::test]
async fn a_confirmed_invalid_license_revokes_without_consuming_grace() {
    let (config, thread) = spawn_server(vec![(404, "{}".into())]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::Active, None);
    let service = build_service(config, store, day(7));

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::Revoked);
    assert_eq!(status.error_code.as_deref(), Some("invalid_license"));
    assert!(!PolarClientError::InvalidLicense.is_transient());
    thread.join().expect("server");
}

// === D7: an outage is never rendered as a revocation ========================

#[tokio::test]
async fn network_failure_alone_never_renders_as_revoked() {
    let outages: Vec<(Option<(u16, String)>, &str)> = vec![
        (Some((429, "{}".into())), "polar_rate_limited"),
        (Some((500, "{}".into())), "polar_unavailable"),
        (Some((503, "{}".into())), "polar_unavailable"),
        (None, "polar_connectivity"),
    ];
    for (response, expected_code) in outages {
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let db = Db::open_in_memory().expect("database");
        seed_validated(&db, store.as_ref(), LicenseStatus::Active, None);
        let (config, thread) = match response {
            Some(response) => {
                let (config, thread) = spawn_server(vec![response]);
                (config, Some(thread))
            }
            None => (unreachable_config(), None),
        };
        let service = build_service(config, store, day(8));

        let status = service.refresh(&db).await.expect("refresh");
        assert_eq!(status.state, LicenseStatus::OfflineGrace, "{expected_code}");
        assert_eq!(status.error_code.as_deref(), Some(expected_code));
        for forbidden in [
            LicenseStatus::Revoked,
            LicenseStatus::Disabled,
            LicenseStatus::Expired,
        ] {
            assert_ne!(status.state, forbidden, "{expected_code}");
        }
        if let Some(thread) = thread {
            thread.join().expect("server");
        }
    }
}

#[tokio::test]
async fn an_outage_past_day_thirty_needs_online_rather_than_revoked() {
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::OfflineGrace, None);
    let service = build_service(unreachable_config(), store, day(31));

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::NeedsOnline);
    assert_eq!(status.error_code.as_deref(), Some("polar_connectivity"));
}

// === D8: no restrictive state may gate local data ===========================

fn seed_local_data(db: &Db) -> String {
    let workflow = db
        .create_workflow(CreateWorkflowInput {
            name: "Acceptance workflow".into(),
            description: "matrix D".into(),
            working_directory: String::new(),
            folder_id: None,
            graph: json!({ "nodes": [], "edges": [] }),
        })
        .expect("workflow");
    db.create_memory(CreateMemoryInput {
        workflow_id: workflow.id.clone(),
        title: "Acceptance memory".into(),
        body: "local only".into(),
        run_id: None,
        node_id: None,
        kind: None,
        source: None,
        pinned: Some(true),
        id: None,
        scope_type: None,
        memory_type: None,
        salience: None,
        confidence: None,
        status: None,
        supersedes_id: None,
        fact_key: None,
        observed_at: None,
        last_confirmed_at: None,
        valid_from: None,
        expires_at: None,
    })
    .expect("memory");
    db.upsert_schedule(
        UpsertScheduleInput {
            workflow_id: workflow.id.clone(),
            cron: "0 9 * * *".into(),
            enabled: true,
            on_missed: OnMissed::Run,
        },
        None,
    )
    .expect("schedule");
    db.upsert_trigger(UpsertTriggerInput {
        id: None,
        workflow_id: workflow.id.clone(),
        source: "webhook".into(),
        label: "Acceptance trigger".into(),
        config: json!({}),
        enabled: true,
    })
    .expect("trigger");
    workflow.id
}

fn assert_local_data_usable(db: &Db, workflow_id: &str, label: &str) {
    assert_eq!(
        db.list_workflows().expect(label).len(),
        1,
        "workflows {label}"
    );
    let context = db.memory_context(workflow_id).expect(label);
    assert_eq!(
        db.list_memories_for_context(&context, true)
            .expect(label)
            .len(),
        1,
        "memories {label}"
    );
    assert!(
        db.get_schedule_for_workflow(workflow_id)
            .expect(label)
            .is_some_and(|schedule| schedule.enabled),
        "schedule {label}"
    );
    assert_eq!(
        db.list_enabled_triggers(None).expect(label).len(),
        1,
        "triggers {label}"
    );
    // Writes keep working too: no restrictive state may turn the app read-only.
    let extra = db
        .create_memory(CreateMemoryInput {
            workflow_id: workflow_id.to_owned(),
            title: format!("written while {label}"),
            body: "still writable".into(),
            run_id: None,
            node_id: None,
            kind: None,
            source: None,
            pinned: None,
            id: None,
            scope_type: None,
            memory_type: None,
            salience: None,
            confidence: None,
            status: None,
            supersedes_id: None,
            fact_key: None,
            observed_at: None,
            last_confirmed_at: None,
            valid_from: None,
            expires_at: None,
        })
        .unwrap_or_else(|_| panic!("memory write blocked while {label}"));
    db.delete_memory(&extra.id).expect("cleanup");
}

#[test]
fn every_restrictive_license_state_leaves_local_data_usable() {
    for state in [
        LicenseStatus::Unlicensed,
        LicenseStatus::OfflineGrace,
        LicenseStatus::NeedsOnline,
        LicenseStatus::Expired,
        LicenseStatus::Revoked,
        LicenseStatus::Disabled,
        LicenseStatus::DeviceLimit,
        LicenseStatus::SecureStorageUnavailable,
        LicenseStatus::NotConfigured,
    ] {
        let db = Db::open_in_memory().expect("database");
        let workflow_id = seed_local_data(&db);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        seed_validated(&db, store.as_ref(), state, None);
        let service = build_service(loopback_config(1), store, day(31));

        let status = service.get_status(&db).expect("status");
        let label = format!("{:?}", status.state);
        assert_local_data_usable(&db, &workflow_id, &label);
    }
}

#[tokio::test]
async fn a_refresh_past_grace_never_touches_local_data() {
    let db = Db::open_in_memory().expect("database");
    let workflow_id = seed_local_data(&db);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    seed_validated(&db, store.as_ref(), LicenseStatus::OfflineGrace, None);
    let service = build_service(unreachable_config(), store, day(31));

    assert_eq!(
        service.refresh(&db).await.expect("refresh").state,
        LicenseStatus::NeedsOnline
    );
    assert_local_data_usable(&db, &workflow_id, "needsOnline");
}

/// The licensing surface is the whole kill-switch budget: status, refresh,
/// activate, deactivate. Nothing in it can reach workflows, memories,
/// schedules, or triggers, and the DTO the frontend sees carries no gate flag.
#[test]
fn the_licensing_contract_exposes_no_kill_switch_over_local_features() {
    let source = include_str!("service.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production half of service.rs");
    let mut touched: Vec<&str> = production
        .match_indices("db.")
        .map(|(index, _)| {
            let tail = &production[index + 3..];
            let end = tail
                .find(|character: char| !character.is_alphanumeric() && character != '_')
                .unwrap_or(tail.len());
            &tail[..end]
        })
        .collect();
    touched.sort_unstable();
    touched.dedup();
    assert_eq!(
        touched,
        [
            "delete_license_snapshot",
            "get_license_snapshot",
            "put_license_snapshot"
        ],
        "the license service may only reach its own snapshot row"
    );
    let dto = serde_json::to_value(LicenseStatusDto::unlicensed()).expect("DTO");
    let fields: Vec<&str> = dto
        .as_object()
        .expect("DTO object")
        .keys()
        .map(String::as_str)
        .collect();
    for forbidden in [
        "locked",
        "disabledFeatures",
        "readOnly",
        "gate",
        "killSwitch",
    ] {
        assert!(
            !fields.iter().any(|field| field.contains(forbidden)),
            "the license DTO must not expose `{forbidden}`"
        );
    }
}

// === D9: the policy constants themselves ====================================

#[tokio::test]
async fn a_successful_validation_rearms_the_documented_seven_and_thirty_day_policy() {
    assert_eq!(REFRESH_AFTER_DAYS, 7);
    assert_eq!(OFFLINE_GRACE_DAYS, 30);

    let (config, thread) = spawn_server(vec![(
        200,
        validation_body("granted", BENEFIT, None).to_string(),
    )]);
    let store = Arc::new(InMemoryLicenseCredentialStore::default());
    let db = Db::open_in_memory().expect("database");
    seed_validated(&db, store.as_ref(), LicenseStatus::OfflineGrace, None);
    let now = day(8);
    let service = build_service(config, store, now);

    let status = service.refresh(&db).await.expect("refresh");
    assert_eq!(status.state, LicenseStatus::Active);
    assert_eq!(
        status.last_successful_validation.as_deref(),
        Some(now.to_rfc3339()).as_deref()
    );
    assert_eq!(
        status.next_refresh.as_deref(),
        Some((now + Duration::days(REFRESH_AFTER_DAYS)).to_rfc3339()).as_deref()
    );
    assert_eq!(
        status.offline_deadline.as_deref(),
        Some((now + Duration::days(OFFLINE_GRACE_DAYS)).to_rfc3339()).as_deref()
    );
    thread.join().expect("server");
}
