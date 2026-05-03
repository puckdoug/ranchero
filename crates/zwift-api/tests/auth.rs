// SPDX-License-Identifier: AGPL-3.0-only
//
// Behavioral tests for `zwift-api` (STEP-07). These exercise the
// observable contract from docs/plans/STEP-07-auth-and-rest.md and
// docs/ARCHITECTURE-AND-RUST-SPEC.md §3 / §7.5:
//
//   1. Successful login parses Keycloak tokens and schedules a
//      preemptive refresh at `expires_in / 2`.
//   2. A 401 on an authed call triggers an inline refresh and the
//      original request is retried transparently.
//   3. A failed refresh surfaces an error to the caller.
//   4. Request shape — form body fields, `client_id` with literal
//      space, fixed headers (`Source`, `User-Agent`) — matches a
//      real Zwift game-client conversation.
//
// All tests run against a `wiremock::MockServer`; nothing in CI ever
// reaches a real Zwift host. Both `auth_base` and `api_base` are
// pointed at the same mock server because routing happens by URN.
//
// These tests are TDD scaffolding written ahead of the implementation;
// they are expected to fail until `ZwiftAuth`'s `unimplemented!()`
// stubs in `src/lib.rs` are filled in.

use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zwift_api::{
    CLIENT_ID, Config, DEFAULT_SOURCE, DEFAULT_USER_AGENT, Error, TOKEN_PATH, ZwiftAuth,
};

// --- helpers ---------------------------------------------------------

fn config_for(server: &MockServer) -> Config {
    let base = server.uri();
    Config {
        auth_base: base.clone(),
        api_base: base,
        source: DEFAULT_SOURCE.to_string(),
        user_agent: DEFAULT_USER_AGENT.to_string(),
    }
}

/// A canonical Keycloak password-grant success body. `expires_in` is
/// kept short so timing-based tests can advance through the half-life
/// without marathon waits.
fn token_body(access: &str, refresh: &str, expires_in: u64) -> serde_json::Value {
    json!({
        "access_token": access,
        "refresh_token": refresh,
        "expires_in": expires_in,
        "refresh_expires_in": expires_in * 4,
        "token_type": "Bearer",
    })
}

// --- 1. Successful login --------------------------------------------

#[tokio::test]
async fn login_success_parses_tokens_and_exposes_bearer() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2")
        .await
        .expect("login should succeed against mock 200");

    let tokens = auth.tokens().await.expect("tokens stored after login");
    assert_eq!(tokens.access_token, "ATOK");
    assert_eq!(tokens.refresh_token, "RTOK");
    assert_eq!(tokens.expires_in, 600);

    assert_eq!(auth.bearer().await.expect("bearer after login"), "ATOK");
}

// --- 4. Request shape ------------------------------------------------

#[tokio::test]
async fn login_sends_password_grant_form_body_with_literal_client_id() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(header(
            "content-type",
            "application/x-www-form-urlencoded",
        ))
        // serde_urlencoded encodes the literal space in
        // "Zwift Game Client" as '+' — this is exactly what the real
        // Zwift game client sends, and Keycloak rejects anything else.
        .and(body_string_contains("client_id=Zwift+Game+Client"))
        .and(body_string_contains("grant_type=password"))
        .and(body_string_contains("username=alice"))
        .and(body_string_contains("password=hunter2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    // Sanity: confirm the constant downstream callers will rely on
    // really does carry the literal space (i.e. nobody silently
    // hyphenated it).
    assert_eq!(CLIENT_ID, "Zwift Game Client");
}

#[tokio::test]
async fn authed_post_includes_bearer_source_and_user_agent_headers() {
    // STEP-09 needs POST with a body for the relay endpoints. This
    // pins the same auth + Source + User-Agent + Content-Type
    // contract `fetch()` honors for GETs, against POST.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/users/login"))
        .and(header("authorization", "Bearer ATOK"))
        .and(header("source", DEFAULT_SOURCE))
        .and(header("user-agent", DEFAULT_USER_AGENT))
        .and(header("content-type", "application/x-protobuf-lite"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"server-reply".to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    let resp = auth
        .post(
            "/api/users/login",
            "application/x-protobuf-lite",
            b"client-body".to_vec(),
        )
        .await
        .expect("authed post should succeed");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn authed_fetch_sends_bearer_source_and_user_agent_headers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;

    // login() calls get_profile_me() eagerly, and then the test calls
    // auth.fetch() explicitly — both hit this mock with identical headers,
    // so expect(2).
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK"))
        .and(header("source", DEFAULT_SOURCE))
        .and(header("user-agent", DEFAULT_USER_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .expect(2)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    let resp = auth
        .fetch("/api/profiles/me")
        .await
        .expect("authed fetch should succeed");
    assert_eq!(resp.status(), 200);
}

// --- 2. 401 → inline refresh → retry --------------------------------

#[tokio::test]
async fn authed_fetch_401_triggers_inline_refresh_and_retries() {
    let server = MockServer::start().await;

    // Token endpoint: first call (login) returns ATOK1/RTOK1; second
    // call (refresh) returns ATOK2/RTOK2. wiremock serves mounts in
    // FIFO order when scoped, so we mount two single-shot mocks.
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=password"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK1", "RTOK1", 600)))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=RTOK1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK2", "RTOK2", 600)))
        .expect(1)
        .mount(&server)
        .await;

    // GET /api/profiles/me mocks.  wiremock 0.6 matches in registration
    // order (FIFO) and an exhausted up_to_n_times mock falls through to
    // the next one.  Order matters:
    //   1. ATOK1 → 200 one-shot: consumed by login()'s eager get_profile_me.
    //   2. ATOK1 → 401: served to the subsequent auth.fetch() call.
    //   3. ATOK2 → 200: served to the retry after inline refresh.
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK1"))
        .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    let resp = auth
        .fetch("/api/profiles/me")
        .await
        .expect("retry after refresh should succeed");
    assert_eq!(resp.status(), 200);

    // After the inline refresh, bearer() should reflect the new token.
    assert_eq!(auth.bearer().await.expect("bearer"), "ATOK2");
}

// --- 3. Refresh failure surfaces ------------------------------------

#[tokio::test]
async fn refresh_failure_surfaces_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=password"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Token is not active",
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    let err = auth.refresh().await.expect_err("refresh should fail");
    match err {
        Error::RefreshFailed(_) | Error::Status { .. } => {}
        other => panic!("expected RefreshFailed/Status, got {other:?}"),
    }
}

#[tokio::test]
async fn login_failure_401_surfaces_auth_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Invalid user credentials",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    let err = auth
        .login("alice", "wrong-password")
        .await
        .expect_err("401 from token endpoint should fail login");

    match err {
        Error::AuthFailedUnauthorized(_) | Error::Status { .. } => {}
        other => panic!("expected AuthFailedUnauthorized/Status, got {other:?}"),
    }
    assert!(
        auth.tokens().await.is_none(),
        "no tokens should be stored after a failed login"
    );
}

// --- bearer() preconditions -----------------------------------------

#[tokio::test]
async fn bearer_without_login_returns_not_authenticated() {
    let server = MockServer::start().await;
    let auth = ZwiftAuth::new(config_for(&server));

    match auth.bearer().await {
        Err(Error::NotAuthenticated) => {}
        Ok(t) => panic!("expected NotAuthenticated, got token {t:?}"),
        Err(other) => panic!("expected NotAuthenticated, got {other:?}"),
    }
}

// --- 1 (cont). Preemptive refresh at expires_in / 2 -----------------

// The background scheduler should fire a refresh at half the access
// token's lifetime. We can't use `tokio::time::pause()` here: the
// scheduled refresh wakes a tokio sleep but then needs the reactor to
// drive an HTTP round-trip to wiremock, and on `current_thread` with
// paused virtual time the reactor only gets a turn when the runtime
// parks — which never happens while the test task is busy yielding.
//
// Instead we use a deliberately tiny `expires_in` (2s, half-life 1s)
// and wait real wall-clock time for the rotation to land. The 2s budget
// is comfortably above CI scheduling jitter and still keeps the test
// suite fast.
#[tokio::test]
async fn preemptive_refresh_fires_at_half_expires_in() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=password"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK1", "RTOK1", 2)))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK2", "RTOK2", 60)))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    assert_eq!(
        auth.bearer().await.expect("bearer"),
        "ATOK1",
        "bearer should still be the original token immediately after login"
    );

    // Half-life is 1s. Sleep past it with margin for IO/scheduling.
    tokio::time::sleep(Duration::from_millis(2000)).await;

    assert_eq!(
        auth.bearer().await.expect("bearer"),
        "ATOK2",
        "preemptive refresh at expires_in/2 should have rotated the access token"
    );
}

// ==========================================================================
// Item 2 (STEP-12.10) — eager profile fetch, athlete identity, typed errors
//
// Red state: ZwiftAuth has no athlete_id() method, login() does not call
// GET /api/profiles/me, and the four typed AuthFailed variants do not exist.
// All five tests below fail to compile until the green-state implementation
// lands.
// ==========================================================================

// T2-A
#[tokio::test]
async fn login_eager_fetches_profile_and_caches_id() {
    // login() must call GET /api/profiles/me exactly once and cache the
    // result so that a subsequent athlete_id() call returns the profile id
    // without any further I/O.  The .expect(1) on the profile mock enforces
    // the single-call contract; wiremock panics on drop if the count is wrong.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 12345,
            "firstName": "Test",
            "lastName": "Rider",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("monitor@example.com", "secret")
        .await
        .expect("login with profile fetch must succeed");

    // athlete_id() must read from the cache; the profile mock's .expect(1)
    // above verifies it was not re-fetched here.
    let id = auth
        .athlete_id()
        .await
        .expect("athlete_id must be available after a successful login");

    assert_eq!(id, 12345, "athlete_id must match the id field from the profile response");
}

// T2-B
#[tokio::test]
async fn get_profile_me_401_returns_unauthorized() {
    // When GET /api/profiles/me returns 401, login() must fail with
    // Error::AuthFailedUnauthorized.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    let err = auth
        .login("monitor@example.com", "secret")
        .await
        .expect_err("login must fail when profile endpoint returns 401");

    assert!(
        matches!(err, Error::AuthFailedUnauthorized(_)),
        "expected AuthFailedUnauthorized, got {err:?}",
    );
}

// T2-C
#[tokio::test]
async fn get_profile_me_403_returns_forbidden() {
    // When GET /api/profiles/me returns 403, login() must fail with
    // Error::AuthFailedForbidden.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    let err = auth
        .login("monitor@example.com", "secret")
        .await
        .expect_err("login must fail when profile endpoint returns 403");

    assert!(
        matches!(err, Error::AuthFailedForbidden(_)),
        "expected AuthFailedForbidden, got {err:?}",
    );
}

// T2-D
#[tokio::test]
async fn get_profile_me_200_with_malformed_body_returns_bad_schema() {
    // When GET /api/profiles/me returns 200 but the body has no "id" field,
    // login() must fail with Error::AuthFailedBadSchema.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    let err = auth
        .login("monitor@example.com", "secret")
        .await
        .expect_err("login must fail when profile body has no id field");

    assert!(
        matches!(err, Error::AuthFailedBadSchema(_)),
        "expected AuthFailedBadSchema, got {err:?}",
    );
}

// T2-E
#[tokio::test]
async fn get_profile_me_5xx_returns_unknown() {
    // When GET /api/profiles/me returns a 5xx status, login() must fail
    // with Error::AuthFailedUnknown.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    let err = auth
        .login("monitor@example.com", "secret")
        .await
        .expect_err("login must fail when profile endpoint returns 503");

    assert!(
        matches!(err, Error::AuthFailedUnknown(_)),
        "expected AuthFailedUnknown, got {err:?}",
    );
}

// --- STEP-12.12 Phase 5a: HTTP capture sink + auth tracing ---------
//
// Pinning the contract for Phase 5b: every request and response body
// issued by `ZwiftAuth` must reach an injected `CaptureSink` so the
// `--capture` file can replay the auth/HTTP timeline, and every
// successful or failed exchange must emit a `relay.auth.*` tracing
// event so an operator looking at the daemon log can tell what
// happened without resorting to `tcpdump`. None of these events or
// the sink injection point exist today; every test below fails red.

use std::sync::{Arc, Mutex};

use zwift_api::{CaptureDirection, CaptureSink, CaptureTransport};

#[derive(Default)]
struct RecordingSink {
    records: Mutex<Vec<(CaptureDirection, CaptureTransport, Vec<u8>)>>,
}

impl RecordingSink {
    fn snapshot(&self) -> Vec<(CaptureDirection, CaptureTransport, Vec<u8>)> {
        self.records.lock().expect("sink mutex").clone()
    }
}

impl CaptureSink for RecordingSink {
    fn record(&self, direction: CaptureDirection, transport: CaptureTransport, payload: &[u8]) {
        self.records
            .lock()
            .expect("sink mutex")
            .push((direction, transport, payload.to_vec()));
    }
}

/// Builds a `ZwiftAuth` against `server` with a fresh `RecordingSink`
/// already attached. Use this for tests that want to capture every
/// HTTP exchange a single call makes (login, refresh, etc).
fn auth_with_sink(server: &MockServer) -> (ZwiftAuth, Arc<RecordingSink>) {
    let auth = ZwiftAuth::new(config_for(server));
    let sink = Arc::new(RecordingSink::default());
    auth.set_capture_sink(sink.clone() as Arc<dyn CaptureSink>);
    (auth, sink)
}

/// Builds a `ZwiftAuth` that has already logged in (so `tokens` /
/// `profile` are populated), then attaches a fresh `RecordingSink`
/// AFTER login so the only captures come from operations the test
/// drives explicitly.
async fn authed_with_sink_after_login(
    server: &MockServer,
) -> (ZwiftAuth, Arc<RecordingSink>) {
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(server)
        .await;
    let auth = ZwiftAuth::new(config_for(server));
    auth.login("alice", "hunter2").await.expect("login");
    let sink = Arc::new(RecordingSink::default());
    auth.set_capture_sink(sink.clone() as Arc<dyn CaptureSink>);
    (auth, sink)
}

#[tokio::test]
async fn login_request_body_appears_in_capture_as_http_outbound() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let (auth, sink) = auth_with_sink(&server);
    auth.login("alice", "hunter2").await.expect("login");

    // The token endpoint is form-encoded; assert at least one outbound
    // HTTP capture contains the form-encoded password-grant fields.
    let outbound: Vec<Vec<u8>> = sink
        .snapshot()
        .into_iter()
        .filter(|(d, t, _)| {
            *d == CaptureDirection::Outbound && *t == CaptureTransport::Http
        })
        .map(|(_, _, p)| p)
        .collect();
    let token_request = outbound
        .iter()
        .find(|body| {
            let s = String::from_utf8_lossy(body);
            s.contains("grant_type=password") && s.contains("username=alice")
        })
        .expect(
            "STEP-12.12 Phase 5a: the token POST's form body must appear as an \
             outbound Http capture record",
        );
    let token_body_str = String::from_utf8_lossy(token_request);
    assert!(
        token_body_str.contains("password=hunter2"),
        "captured token request body must include the credentials verbatim; \
         got {token_body_str:?}",
    );
}

#[tokio::test]
async fn login_response_body_appears_in_capture_as_http_inbound() {
    let server = MockServer::start().await;
    let token_resp = token_body("ATOK_RESP", "RTOK_RESP", 600);
    let token_resp_bytes = serde_json::to_vec(&token_resp).expect("serialize token body");
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_resp.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 99})))
        .mount(&server)
        .await;

    let (auth, sink) = auth_with_sink(&server);
    auth.login("alice", "hunter2").await.expect("login");

    let inbound: Vec<Vec<u8>> = sink
        .snapshot()
        .into_iter()
        .filter(|(d, t, _)| {
            *d == CaptureDirection::Inbound && *t == CaptureTransport::Http
        })
        .map(|(_, _, p)| p)
        .collect();
    assert!(
        inbound.iter().any(|body| body == &token_resp_bytes),
        "STEP-12.12 Phase 5a: the token endpoint's response body must appear \
         verbatim as an inbound Http capture record",
    );
}

#[tokio::test]
async fn profile_fetch_request_and_response_appear_in_capture() {
    let server = MockServer::start().await;
    let (auth, sink) = authed_with_sink_after_login(&server).await;
    let profile_body = json!({"id": 1});
    let profile_bytes = serde_json::to_vec(&profile_body).expect("serialize");
    // The mock used during login was a generic 200; mount a fresh,
    // recognizable response for the explicit get_profile_me call.
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(profile_bytes.clone()))
        .mount(&server)
        .await;

    auth.get_profile_me().await.expect("profile fetch");

    let records = sink.snapshot();
    let outbound = records
        .iter()
        .find(|(d, t, _)| {
            *d == CaptureDirection::Outbound && *t == CaptureTransport::Http
        })
        .expect(
            "STEP-12.12 Phase 5a: get_profile_me must produce one outbound \
             Http capture record (empty body for the GET)",
        );
    assert!(
        outbound.2.is_empty(),
        "GET request bodies are empty; captured payload should match",
    );
    let inbound = records
        .iter()
        .find(|(d, t, p)| {
            *d == CaptureDirection::Inbound && *t == CaptureTransport::Http && p == &profile_bytes
        })
        .expect(
            "STEP-12.12 Phase 5a: get_profile_me must produce one inbound \
             Http capture record holding the response body verbatim",
        );
    let _ = inbound;
}

#[tokio::test]
async fn authenticated_post_and_get_paths_record_request_and_response() {
    let server = MockServer::start().await;
    let (auth, sink) = authed_with_sink_after_login(&server).await;

    // POST returns one specific body; GET returns another. We look for
    // both in the captured records to prove each path records its own
    // request + response pair.
    let post_resp = b"post-resp-bytes";
    let get_resp = b"get-resp-bytes";
    Mock::given(method("POST"))
        .and(path("/api/echo"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(post_resp.to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/something"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(get_resp.to_vec()))
        .mount(&server)
        .await;

    auth.post(
        "/api/echo",
        "application/octet-stream",
        b"client-body".to_vec(),
    )
    .await
    .expect("post");
    auth.fetch("/api/something").await.expect("fetch");

    let records = sink.snapshot();

    assert!(
        records.iter().any(|(d, t, p)| {
            *d == CaptureDirection::Outbound
                && *t == CaptureTransport::Http
                && p.as_slice() == b"client-body"
        }),
        "STEP-12.12 Phase 5a: auth.post must record its request body as \
         an outbound Http capture; got {records:?}",
    );
    assert!(
        records.iter().any(|(d, t, p)| {
            *d == CaptureDirection::Inbound
                && *t == CaptureTransport::Http
                && p.as_slice() == post_resp
        }),
        "STEP-12.12 Phase 5a: auth.post must record its response body as \
         an inbound Http capture",
    );
    assert!(
        records.iter().any(|(d, t, p)| {
            *d == CaptureDirection::Inbound
                && *t == CaptureTransport::Http
                && p.as_slice() == get_resp
        }),
        "STEP-12.12 Phase 5a: auth.fetch must record its response body as \
         an inbound Http capture",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn auth_emits_token_requested_and_granted_events() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.auth.token.requested",
        ),
        "STEP-12.12 Phase 5a: relay.auth.token.requested must fire at info \
         before the token POST; not found in tracing log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.auth.token.granted",
        ),
        "STEP-12.12 Phase 5a: relay.auth.token.granted must fire at info on \
         a successful token decode; not found in tracing log",
    );
    for field in ["username=", "grant_type="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.12 Phase 5a: relay.auth.token.requested must carry \
             {field:?} — not present in any captured log line",
        );
    }
    for field in ["expires_in_s=", "refresh_expires_in_s="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.12 Phase 5a: relay.auth.token.granted must carry \
             {field:?} — not present in any captured log line",
        );
    }
}

#[tokio::test]
#[tracing_test::traced_test]
async fn auth_emits_profile_ok_on_success_and_profile_failed_on_error() {
    // Success scenario: 200 from /api/profiles/me must produce
    // `relay.auth.profile.ok` at debug.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;
    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");
    drop(auth);
    drop(server);

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.auth.profile.ok"),
        "STEP-12.12 Phase 5a: relay.auth.profile.ok must fire at debug on \
         a 200 from /api/profiles/me; not found in tracing log",
    );

    // Failure scenario: 401 must produce `relay.auth.profile.failed` at
    // warn carrying the status. We need the token endpoint to succeed
    // first so login() reaches get_profile_me().
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
        .mount(&server)
        .await;
    let auth = ZwiftAuth::new(config_for(&server));
    let _ = auth.login("alice", "hunter2").await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.auth.profile.failed",
        ),
        "STEP-12.12 Phase 5a: relay.auth.profile.failed must fire at warn \
         on a non-200 from /api/profiles/me; not found in tracing log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "status="),
        "STEP-12.12 Phase 5a: relay.auth.profile.failed must carry status= \
         in its fields",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn auth_emits_http_request_and_response_at_debug() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/echo"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/get-it"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");
    auth.post("/api/echo", "application/octet-stream", b"x".to_vec())
        .await
        .expect("post");
    auth.fetch("/api/get-it").await.expect("fetch");

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.auth.http.request"),
        "STEP-12.12 Phase 5a: relay.auth.http.request must fire at debug \
         for both auth.post and auth.fetch; not found",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.auth.http.response"),
        "STEP-12.12 Phase 5a: relay.auth.http.response must fire at debug \
         for both auth.post and auth.fetch; not found",
    );
    for field in ["method=", "urn=", "status="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.12 Phase 5a: relay.auth.http.request/response must \
             carry field {field:?} — not present in any captured log line",
        );
    }
}

#[tokio::test]
#[tracing_test::traced_test]
async fn auth_emits_http_retry_event_on_401_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=password"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK1", "RTOK1", 600)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK2", "RTOK2", 600)))
        .mount(&server)
        .await;
    // First profile call (during login) succeeds with ATOK1.
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    // Subsequent call with ATOK1 → 401 (forces refresh + retry).
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK1"))
        .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");
    let _ = auth.fetch("/api/profiles/me").await.expect("retry");

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.auth.http.retry"),
        "STEP-12.12 Phase 5a: relay.auth.http.retry must fire at info on \
         the inline 401-refresh-and-retry path; not found in tracing log",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn auth_emits_refresh_completed_event() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=password"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(token_body("ATOK2", "RTOK2", 1200)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");
    auth.refresh().await.expect("refresh");

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.auth.refresh.completed",
        ),
        "STEP-12.12 Phase 5a: relay.auth.refresh.completed must fire at info \
         after a successful refresh; not found in tracing log",
    );
    for field in ["expires_in_s=", "next_refresh_in_s="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.12 Phase 5a: relay.auth.refresh.completed must carry \
             field {field:?} — not present in any captured log line",
        );
    }
}
