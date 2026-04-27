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

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    // Sanity: confirm the constant downstream callers will rely on
    // really does carry the literal space (i.e. nobody silently
    // hyphenated it).
    assert_eq!(CLIENT_ID, "Zwift Game Client");
}

#[tokio::test]
async fn authed_fetch_sends_bearer_source_and_user_agent_headers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .and(header("authorization", "Bearer ATOK"))
        .and(header("source", DEFAULT_SOURCE))
        .and(header("user-agent", DEFAULT_USER_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .expect(1)
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

    // The first GET with stale ATOK1 returns 401; the retry with the
    // freshly-refreshed ATOK2 returns 200.
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
        Error::AuthFailed(_) | Error::Status { .. } => {}
        other => panic!("expected AuthFailed/Status, got {other:?}"),
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
