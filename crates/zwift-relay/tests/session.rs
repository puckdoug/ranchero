// SPDX-License-Identifier: AGPL-3.0-only
//
// Behavioral tests for `zwift-relay`'s session module (STEP 09).
// Wiremock-driven; nothing in CI ever reaches a real Zwift host. Both
// the auth host (used by `ZwiftAuth`) and the relay host (used by
// `RelaySessionSupervisor`) point at the same `MockServer::uri()`
// because routing happens by URN.
//
// These tests are TDD scaffolding written ahead of the implementation;
// they fail until `session.rs`'s `unimplemented!()` stubs are filled
// in. See `docs/plans/STEP-09-relay-session.md`.

use std::time::Duration;

use prost::Message;
use serde_json::json;
use tokio::sync::broadcast;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zwift_api::{Config as AuthConfig, DEFAULT_SOURCE, DEFAULT_USER_AGENT, ZwiftAuth};
use zwift_proto::{
    LoginRequest, LoginResponse, PerSessionInfo, RelaySessionRefreshResponse, TcpAddress, TcpConfig,
};
use zwift_relay::{
    LOGIN_PATH, RelaySession, RelaySessionConfig, RelaySessionSupervisor, SESSION_REFRESH_PATH,
    SessionError, SessionEvent, login, refresh,
};

// --- helpers -------------------------------------------------------

fn auth_config_for(server: &MockServer) -> AuthConfig {
    let base = server.uri();
    AuthConfig {
        auth_base: base.clone(),
        api_base: base,
        source: DEFAULT_SOURCE.to_string(),
        user_agent: DEFAULT_USER_AGENT.to_string(),
    }
}

fn relay_config_for(server: &MockServer) -> RelaySessionConfig {
    RelaySessionConfig {
        api_base: server.uri(),
        // refresh_fraction left at default (0.9); supervisor tests
        // override it to keep wall-clock test cost low.
        ..RelaySessionConfig::default()
    }
}

fn token_body(access: &str, refresh: &str, expires_in: u64) -> serde_json::Value {
    json!({
        "access_token": access,
        "refresh_token": refresh,
        "expires_in": expires_in,
        "refresh_expires_in": expires_in * 4,
        "token_type": "Bearer",
    })
}

const TOKEN_PATH: &str = "/auth/realms/zwift/protocol/openid-connect/token";

/// Mount the Keycloak token endpoint with a canned access token. The
/// `ZwiftAuth` instance points at the same wiremock as the relay
/// endpoints, so both flows share one server.
async fn mount_token_endpoint(server: &MockServer, access: &str) {
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body(access, "RTOK", 600)))
        .mount(server)
        .await;
}

/// Build a `LoginResponse` whose TCP server list is exactly `nodes`,
/// with `expiration` minutes and an arbitrary server time.
fn login_response(
    relay_session_id: Option<u32>,
    expiration_minutes: Option<u32>,
    nodes: Vec<TcpAddress>,
) -> LoginResponse {
    LoginResponse {
        session_state: "ok".to_string(),
        info: PerSessionInfo {
            relay_url: "https://us-or-rly101.zwift.com".to_string(),
            apis: None,
            time: Some(1_700_000_000_000),
            nodes: Some(TcpConfig { nodes }),
            max_segm_subscrs: None,
        },
        relay_session_id,
        expiration: expiration_minutes,
        economy_config: None,
    }
}

fn tcp_addr(ip: &str, port: i32, lb_realm: i32, lb_course: i32) -> TcpAddress {
    TcpAddress {
        ip: Some(ip.to_string()),
        port: Some(port),
        lb_realm: Some(lb_realm),
        lb_course: Some(lb_course),
    }
}

/// Pre-loaded `ZwiftAuth` whose stored access token is `access`.
/// Calls `login` against the wiremock token endpoint internally.
async fn authed(server: &MockServer, access: &str) -> ZwiftAuth {
    mount_token_endpoint(server, access).await;
    let auth = ZwiftAuth::new(auth_config_for(server));
    auth.login("alice", "hunter2").await.expect("auth login");
    auth
}

// --- 1. login: body shape ------------------------------------------

#[tokio::test]
async fn login_posts_protobuf_login_request_with_correct_body() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    let resp = login_response(Some(7), Some(10), vec![tcp_addr("1.1.1.1", 3025, 0, 0)]);
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .and(header("content-type", "application/x-protobuf-lite"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    login(&auth, &relay_config_for(&server))
        .await
        .expect("login should succeed");

    // Inspect the captured request body and assert it decodes as a
    // LoginRequest with a 16-byte AES key field.
    let received = server.received_requests().await.expect("requests captured");
    let login_req = received
        .iter()
        .find(|r| r.url.path() == LOGIN_PATH)
        .expect("login request observed");
    let parsed = LoginRequest::decode(login_req.body.as_slice()).expect("body is a LoginRequest");
    assert_eq!(
        parsed.key.len(),
        16,
        "LoginRequest.key must be the 16-byte AES session key"
    );
}

// --- 2. login: response shape -> RelaySession ----------------------

#[tokio::test]
async fn login_response_decodes_into_relay_session() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    let resp = login_response(
        Some(0xDEAD_BEEF),
        Some(10),
        vec![
            tcp_addr("10.0.0.1", 3025, 0, 0),
            tcp_addr("10.0.0.2", 3025, 0, 0),
        ],
    );
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .mount(&server)
        .await;

    let before = tokio::time::Instant::now();
    let session = login(&auth, &relay_config_for(&server))
        .await
        .expect("login should succeed");

    assert_eq!(session.relay_id, 0xDEAD_BEEF);
    assert_eq!(session.tcp_servers.len(), 2);
    assert_eq!(session.tcp_servers[0].ip, "10.0.0.1");
    assert_eq!(session.server_time_ms, Some(1_700_000_000_000));
    // expires_at should be ~10 minutes from now (give a generous
    // window so the assertion isn't flaky on slow CI).
    let elapsed_to_expiry = session.expires_at.duration_since(before);
    assert!(
        elapsed_to_expiry >= Duration::from_secs(599)
            && elapsed_to_expiry <= Duration::from_secs(605),
        "expires_at should be ~10 minutes ahead, got {:?}",
        elapsed_to_expiry,
    );
}

// --- 3. TCP server filter ------------------------------------------

#[tokio::test]
async fn login_filters_tcp_servers_to_realm_zero_course_zero() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    let resp = login_response(
        Some(1),
        Some(10),
        vec![
            tcp_addr("10.0.0.1", 3025, 0, 0), // generic — keep
            tcp_addr("10.0.0.2", 3025, 0, 5), // course-scoped — drop
            tcp_addr("10.0.0.3", 3025, 1, 0), // realm-scoped — drop
            tcp_addr("10.0.0.4", 3025, 2, 7), // both non-zero — drop
        ],
    );
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .mount(&server)
        .await;

    let session = login(&auth, &relay_config_for(&server))
        .await
        .expect("login");

    let ips: Vec<&str> = session.tcp_servers.iter().map(|s| s.ip.as_str()).collect();
    assert_eq!(ips, vec!["10.0.0.1"], "only (lb_realm=0, lb_course=0) survives");
}

// --- 4. bearer header propagation ----------------------------------

#[tokio::test]
async fn login_includes_bearer_from_zwift_auth() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    let resp = login_response(Some(1), Some(10), vec![tcp_addr("1.1.1.1", 3025, 0, 0)]);
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .and(header("authorization", "Bearer ATOK"))
        .and(header("source", DEFAULT_SOURCE))
        .and(header("user-agent", DEFAULT_USER_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    login(&auth, &relay_config_for(&server))
        .await
        .expect("login should carry bearer + Source + User-Agent");
}

// --- 5/6/7. error surface ------------------------------------------

#[tokio::test]
async fn login_surfaces_http_error_status() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal err"))
        .mount(&server)
        .await;

    let err = login(&auth, &relay_config_for(&server))
        .await
        .expect_err("500 must surface as an error");
    match err {
        SessionError::Status { status: 500, .. } => {}
        other => panic!("expected Status {{ status: 500, .. }}, got {other:?}"),
    }
}

#[tokio::test]
async fn login_surfaces_protobuf_decode_error() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not-a-protobuf".to_vec()))
        .mount(&server)
        .await;

    let err = login(&auth, &relay_config_for(&server))
        .await
        .expect_err("garbage body must surface as Decode");
    assert!(
        matches!(err, SessionError::Decode(_)),
        "expected Decode, got {err:?}",
    );
}

#[tokio::test]
async fn login_surfaces_missing_required_field() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    // LoginResponse with relay_session_id = None.
    let resp = login_response(None, Some(10), vec![tcp_addr("1.1.1.1", 3025, 0, 0)]);
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .mount(&server)
        .await;

    let err = login(&auth, &relay_config_for(&server))
        .await
        .expect_err("missing relay_session_id must error");
    assert!(
        matches!(err, SessionError::MissingField("relay_session_id")),
        "expected MissingField(\"relay_session_id\"), got {err:?}",
    );
}

// --- 8. refresh: body shape ----------------------------------------

#[tokio::test]
async fn refresh_posts_relay_session_refresh_request_body() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    let resp = RelaySessionRefreshResponse {
        relay_session_id: 42,
        expiration: 30,
    };
    Mock::given(method("POST"))
        .and(path(SESSION_REFRESH_PATH))
        .and(header("content-type", "application/x-protobuf-lite"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    refresh(&auth, &relay_config_for(&server), 42)
        .await
        .expect("refresh");

    let received = server.received_requests().await.expect("requests");
    let refresh_req = received
        .iter()
        .find(|r| r.url.path() == SESSION_REFRESH_PATH)
        .expect("refresh request observed");
    // RelaySessionRefreshRequest is missing from the upstream proto
    // (see STEP-09 plan "Open verification points" §1). The body is
    // a single varint field: tag = (field_number << 3) | wire_type =
    // (1 << 3) | 0 = 0x08, then the varint-encoded relay_session_id.
    // For relay_id = 42 that's [0x08, 0x2A].
    assert_eq!(
        refresh_req.body,
        vec![0x08, 0x2A],
        "RelaySessionRefreshRequest body must be tag=1 varint relay_session_id",
    );
}

// --- 9. refresh: returns expiration --------------------------------

#[tokio::test]
async fn refresh_returns_new_expiration_minutes() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    let resp = RelaySessionRefreshResponse {
        relay_session_id: 7,
        expiration: 30,
    };
    Mock::given(method("POST"))
        .and(path(SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .mount(&server)
        .await;

    let new_expiration = refresh(&auth, &relay_config_for(&server), 7)
        .await
        .expect("refresh");
    assert_eq!(new_expiration, 30);
}

// --- supervisor helpers --------------------------------------------

fn fast_relay_config_for(server: &MockServer) -> RelaySessionConfig {
    // Tighten the refresh schedule so supervisor tests don't pay 54 s
    // of wall clock per scenario. With expiration = 1 minute (60 s)
    // and refresh_fraction = 0.05, the next refresh fires at 3 s —
    // tolerable in CI without the multi-thread + paused-time
    // gymnastics that STEP 07 §20.1 ran into.
    RelaySessionConfig {
        api_base: server.uri(),
        source: DEFAULT_SOURCE.to_string(),
        user_agent: DEFAULT_USER_AGENT.to_string(),
        min_refresh_interval: Duration::from_millis(50),
        refresh_fraction: 0.05,
    }
}

async fn mount_login(server: &MockServer, relay_id: u32, expiration_min: u32, ip: &str) {
    let resp = login_response(
        Some(relay_id),
        Some(expiration_min),
        vec![tcp_addr(ip, 3025, 0, 0)],
    );
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .mount(server)
        .await;
}

async fn drain_until<F: Fn(&SessionEvent) -> bool>(
    rx: &mut broadcast::Receiver<SessionEvent>,
    pred: F,
    timeout: Duration,
) -> SessionEvent {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) if pred(&ev) => return ev,
            Ok(Ok(_)) => continue,
            Ok(Err(_)) => panic!("event channel closed before predicate matched"),
            Err(_) => panic!("timed out waiting for event"),
        }
    }
}

// --- 10. supervisor: initial login emits LoggedIn ------------------

#[tokio::test]
async fn supervisor_initial_login_emits_logged_in_event() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;
    mount_login(&server, 11, 10, "1.1.1.1").await;

    let supervisor = RelaySessionSupervisor::start(auth, fast_relay_config_for(&server))
        .await
        .expect("supervisor start");
    let mut events = supervisor.events();

    // The LoggedIn event should arrive immediately after start().
    let ev = drain_until(
        &mut events,
        |e| matches!(e, SessionEvent::LoggedIn(_)),
        Duration::from_secs(1),
    )
    .await;

    let logged_in = match ev {
        SessionEvent::LoggedIn(s) => s,
        _ => unreachable!(),
    };
    let snapshot = supervisor.current().await;
    assert_eq!(logged_in.relay_id, snapshot.relay_id);
    assert_eq!(logged_in.aes_key, snapshot.aes_key);
}

// --- 11. supervisor: refresh fires at refresh_fraction × expires ---

#[tokio::test]
async fn supervisor_refresh_fires_at_configured_fraction_of_expiration() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;
    mount_login(&server, 11, 1, "1.1.1.1").await; // 1-minute expiration

    let resp = RelaySessionRefreshResponse {
        relay_session_id: 11,
        expiration: 1,
    };
    Mock::given(method("POST"))
        .and(path(SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(resp.encode_to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let supervisor = RelaySessionSupervisor::start(auth, fast_relay_config_for(&server))
        .await
        .expect("supervisor start");
    let mut events = supervisor.events();

    // refresh_fraction = 0.05 of 60 s = ~3 s. Wait up to 10 s as a
    // generous margin against CI scheduling jitter.
    let refreshed = drain_until(
        &mut events,
        |e| matches!(e, SessionEvent::Refreshed { .. }),
        Duration::from_secs(10),
    )
    .await;
    match refreshed {
        SessionEvent::Refreshed { relay_id, .. } => assert_eq!(relay_id, 11),
        _ => unreachable!(),
    }
    // wiremock asserts the `expect(1)` count on its drop.
}

// --- 12. supervisor: refresh failure → re-login --------------------

#[tokio::test]
async fn supervisor_refresh_failure_triggers_relogin() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;
    mount_login(&server, 11, 1, "1.1.1.1").await;

    Mock::given(method("POST"))
        .and(path(SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .expect(1)
        .mount(&server)
        .await;

    let supervisor = RelaySessionSupervisor::start(auth, fast_relay_config_for(&server))
        .await
        .expect("initial login");
    let mut events = supervisor.events();

    let _ = drain_until(
        &mut events,
        |e| matches!(e, SessionEvent::RefreshFailed(_)),
        Duration::from_secs(10),
    )
    .await;

    // After RefreshFailed, the supervisor falls back to a full
    // re-login → LoggedIn event again.
    let _ = drain_until(
        &mut events,
        |e| matches!(e, SessionEvent::LoggedIn(_)),
        Duration::from_secs(10),
    )
    .await;
}

// --- 13. supervisor: re-login failure with attempt count ----------

#[tokio::test]
async fn supervisor_relogin_failure_emits_login_failed_with_attempt_count() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;

    // First login (during start()) succeeds; subsequent logins fail.
    let initial_resp =
        login_response(Some(11), Some(1), vec![tcp_addr("1.1.1.1", 3025, 0, 0)]);
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(initial_resp.encode_to_vec()))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .mount(&server)
        .await;

    let supervisor = RelaySessionSupervisor::start(auth, fast_relay_config_for(&server))
        .await
        .expect("initial login");
    let mut events = supervisor.events();

    // Expect refresh failure, then login failure with attempt = 1.
    let _ = drain_until(
        &mut events,
        |e| matches!(e, SessionEvent::RefreshFailed(_)),
        Duration::from_secs(10),
    )
    .await;
    let attempt1 = drain_until(
        &mut events,
        |e| matches!(e, SessionEvent::LoginFailed { .. }),
        Duration::from_secs(10),
    )
    .await;
    match attempt1 {
        SessionEvent::LoginFailed { attempt, .. } => assert_eq!(attempt, 1),
        _ => unreachable!(),
    }

    // Backoff fires; login fails again with attempt = 2.
    let attempt2 = drain_until(
        &mut events,
        |e| matches!(e, SessionEvent::LoginFailed { attempt: 2, .. }),
        Duration::from_secs(10),
    )
    .await;
    match attempt2 {
        SessionEvent::LoginFailed { attempt, .. } => assert_eq!(attempt, 2),
        _ => unreachable!(),
    }
}

// --- 14. supervisor: shutdown cancels pending refresh -------------

#[tokio::test]
async fn supervisor_shutdown_cancels_pending_refresh() {
    let server = MockServer::start().await;
    let auth = authed(&server, "ATOK").await;
    mount_login(&server, 11, 1, "1.1.1.1").await;

    // expect(0): if any refresh request hits the endpoint, wiremock
    // panics on Mock drop at end of test.
    Mock::given(method("POST"))
        .and(path(SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let supervisor = RelaySessionSupervisor::start(auth, fast_relay_config_for(&server))
        .await
        .expect("initial login");
    supervisor.shutdown();

    // Wait well past the would-be refresh deadline (~3 s) to confirm
    // no refresh request was issued.
    tokio::time::sleep(Duration::from_secs(5)).await;
}

// --- compile-time wiring sanity (cheap) ----------------------------

#[test]
fn relay_session_is_clone_for_supervisor_snapshots() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<RelaySession>();
}
