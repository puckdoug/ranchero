// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 18 — `ZwiftAuth::get_game_info` wiremock tests.
//
// `get_game_info()` fetches `/api/game_info` as JSON and returns the parsed
// document. Sauce4zwift sends `Accept: application/json` and a custom
// `Zwift-Api-Version: 2.7` header (`zwift.mjs:681`), then walks the segment
// array fixing up integer ids that JSON.parse would otherwise truncate.
//
// These tests fail today because `ZwiftAuth` has no `get_game_info` method.

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zwift_api::{Config, DEFAULT_SOURCE, DEFAULT_USER_AGENT, TOKEN_PATH, ZwiftAuth};

fn config_for(server: &MockServer) -> Config {
    let base = server.uri();
    Config {
        auth_base:  base.clone(),
        api_base:   base,
        source:     DEFAULT_SOURCE.to_string(),
        user_agent: DEFAULT_USER_AGENT.to_string(),
        platform:   "OSX".to_string(),
    }
}

fn token_body(access: &str, refresh: &str, expires_in: u64) -> serde_json::Value {
    json!({
        "access_token":         access,
        "refresh_token":        refresh,
        "expires_in":           expires_in,
        "refresh_expires_in":   expires_in * 4,
        "token_type":           "Bearer",
    })
}

async fn setup_authed_client(server: &MockServer) -> ZwiftAuth {
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            token_body("ATOK", "RTOK", 600),
        ))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(server)
        .await;
    let auth = ZwiftAuth::new(config_for(server));
    auth.login("alice", "hunter2").await.expect("login");
    auth
}

/// A successful /api/game_info response parses into a structure exposing at
/// least one segment with its id intact.
#[tokio::test]
async fn get_game_info_parses() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    // Sauce reads `segments[].id` as a string-of-integer to dodge IEEE-754
    // truncation of 64-bit ids.  A real `/api/game_info` body returns this
    // schema; the mock fixture below mirrors it minimally.
    let body = json!({
        "version": "1.0.123456",
        "segments": [
            { "id": 12_345_678_901_234_567_u64, "name": "Test KOM" },
            { "id":                  98_765_u64, "name": "Test Sprint" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/api/game_info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let info = auth
        .get_game_info()
        .await
        .expect("get_game_info must succeed on a 200 JSON response");

    let segments = info
        .get("segments")
        .and_then(|v| v.as_array())
        .expect("game_info must expose a segments array");
    assert_eq!(segments.len(), 2, "both segments must be parsed");

    // The first segment id is larger than 2^53; if `get_game_info` uses a
    // naive `serde_json::from_str::<Value>`, the id is truncated and this
    // assertion catches it.
    let first_id = segments[0].get("id").expect("segment id must be present");
    assert_eq!(
        first_id.as_u64(),
        Some(12_345_678_901_234_567),
        "segment id must round-trip without truncation; got {first_id}",
    );
}

/// A 401 from /api/game_info surfaces as an Err rather than a panic.
#[tokio::test]
async fn get_game_info_propagates_auth_error() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    Mock::given(method("GET"))
        .and(path("/api/game_info"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorised"))
        .mount(&server)
        .await;

    let result = auth.get_game_info().await;
    assert!(
        result.is_err(),
        "a 401 from get_game_info must surface as Err; got {result:?}",
    );
}
