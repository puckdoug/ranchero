// SPDX-License-Identifier: AGPL-3.0-only
//
// Failing tests for Step 1 of the STEP-20 implementation plan:
// ZwiftAuth::get_profile / get_profiles and the extended Profile struct.
//
// Red state: Profile has only { id: i64 }; get_profile and get_profiles
// do not exist. All tests below fail to compile until Step 1 implementation
// adds the missing fields and methods.

use prost::Message as _;
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zwift_api::{Config, DEFAULT_SOURCE, DEFAULT_USER_AGENT, TOKEN_PATH, ZwiftAuth};
use zwift_proto::{PlayerProfile, PlayerProfiles};

// --- helpers ---------------------------------------------------------

fn config_for(server: &MockServer) -> Config {
    let base = server.uri();
    Config {
        auth_base: base.clone(),
        api_base: base,
        source: DEFAULT_SOURCE.to_string(),
        user_agent: DEFAULT_USER_AGENT.to_string(),
        platform: "OSX".to_string(),
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

/// Mount the standard token + /api/profiles/me mocks and log in.
/// Returns a ZwiftAuth that is ready to make authenticated calls.
async fn setup_authed_client(server: &MockServer) -> ZwiftAuth {
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
    auth
}

// ==========================================================================
// Step 1 — Profile fetch + extended Profile struct
//
// These four tests are red until Step 1 implementation:
//   a) extends Profile with first_name, last_name, ftp, weight_g
//   b) adds ZwiftAuth::get_profile(id: i64) -> Result<Profile>
//      (GET /api/profiles/{id}, JSON response)
//   c) adds ZwiftAuth::get_profiles(ids: &[i64]) -> Result<Vec<Profile>>
//      (GET /api/profiles?id=…, protobuf PlayerProfiles response)
// ==========================================================================

// S1-A: Individual profile fetch parses all relevant fields.
#[tokio::test]
async fn get_profile_parses_ftp_and_name() {
    let server = MockServer::start().await;
    let auth = setup_authed_client(&server).await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 42,
            "firstName": "Test",
            "lastName": "Rider",
            "ftp": 220,
            "weight": 72000,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let profile = auth
        .get_profile(42)
        .await
        .expect("Step 1: get_profile must succeed on a 200 JSON response");

    assert_eq!(profile.id, 42, "id must match the JSON id field");
    assert_eq!(
        profile.first_name.as_deref(),
        Some("Test"),
        "first_name must be parsed from the JSON firstName field",
    );
    assert_eq!(
        profile.last_name.as_deref(),
        Some("Rider"),
        "last_name must be parsed from the JSON lastName field",
    );
    assert_eq!(
        profile.ftp,
        Some(220),
        "ftp must be parsed from the JSON ftp field (watts)",
    );
    assert_eq!(
        profile.weight_g,
        Some(72_000),
        "weight_g must be parsed from the JSON weight field (grams)",
    );
}

// S1-B: Batch profile fetch returns all requested profiles in request order.
//
// The batch endpoint returns a protobuf-encoded PlayerProfiles message.
// The implementation must reorder the results to match the id slice passed
// by the caller (mirroring sauce4zwift's getProfiles behaviour).
#[tokio::test]
async fn get_profiles_batch_returns_all_in_request_order() {
    let server = MockServer::start().await;
    let auth = setup_authed_client(&server).await;

    let profiles_pb = PlayerProfiles {
        // Intentionally returned in reverse order to verify that the
        // implementation reorders results to match the requested id sequence.
        profiles: vec![
            PlayerProfile {
                id: Some(99),
                first_name: Some("Bob".into()),
                ftp: Some(300),
                ..Default::default()
            },
            PlayerProfile {
                id: Some(42),
                first_name: Some("Alice".into()),
                ftp: Some(200),
                ..Default::default()
            },
        ],
    };

    Mock::given(method("GET"))
        .and(path("/api/profiles"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-protobuf-lite")
                .set_body_bytes(profiles_pb.encode_to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let profiles = auth
        .get_profiles(&[42, 99])
        .await
        .expect("Step 1: get_profiles must succeed on a 200 protobuf response");

    assert_eq!(
        profiles.len(),
        2,
        "get_profiles must return one Profile per requested id",
    );
    assert_eq!(
        profiles[0].id, 42,
        "first result must correspond to the first requested id (42)",
    );
    assert_eq!(
        profiles[1].id, 99,
        "second result must correspond to the second requested id (99)",
    );
    assert_eq!(
        profiles[0].ftp,
        Some(200),
        "ftp must be decoded from the PlayerProfile protobuf for id 42",
    );
}

// S1-C: A profile response that omits ftp must yield ftp == None, not 0.
//
// Zwift sometimes returns profiles without an ftp field (unset riders,
// or privacy-hidden values). The implementation must represent this as
// None, not zero, so callers can distinguish "no FTP on file" from "0 W".
#[tokio::test]
async fn get_profile_missing_ftp_is_none() {
    let server = MockServer::start().await;
    let auth = setup_authed_client(&server).await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 42,
            "firstName": "Ghost",
            "lastName": "Rider",
            // ftp field intentionally absent
        })))
        .mount(&server)
        .await;

    let profile = auth
        .get_profile(42)
        .await
        .expect("Step 1: get_profile must succeed even when ftp is absent");

    assert_eq!(
        profile.ftp,
        None,
        "ftp must be None when the JSON response omits the field; \
         callers must be able to distinguish 'no FTP on file' from '0 W'",
    );
}

// S1-D: A 401 from the batch endpoint must surface as Err, not Ok(vec![]).
//
// Errors from the underlying HTTP call must propagate to the caller.
// This test guards against an implementation that silently returns an
// empty vec on auth failure (which would hide the error from the daemon).
#[tokio::test]
async fn get_profiles_propagates_auth_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=password"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("ATOK", "RTOK", 600)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .mount(&server)
        .await;

    // Inline token refresh also fails, so the 401 on the batch endpoint
    // cannot be recovered and must be surfaced to the caller.
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Token is not active",
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&server)
        .await;

    let auth = ZwiftAuth::new(config_for(&server));
    auth.login("alice", "hunter2").await.expect("login");

    let err = auth
        .get_profiles(&[42])
        .await
        .expect_err("Step 1: a 401 from the batch endpoint must surface as Err, not Ok(vec![])");

    assert!(
        matches!(
            err,
            zwift_api::Error::RefreshFailed(_)
                | zwift_api::Error::AuthFailedUnauthorized(_)
                | zwift_api::Error::Status { .. }
        ),
        "expected an error indicating auth failure (RefreshFailed, \
         AuthFailedUnauthorized, or Status), got {err:?}",
    );
}
