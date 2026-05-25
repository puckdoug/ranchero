// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 13 — ZwiftAuth::get_event wiremock tests.
//
// `get_event(id)` fetches `/api/events/{id}` using the protobuf-lite encoding
// (same Accept header as `get_player_state`) and returns the decoded
// `zwift_proto::Event`, which the caller uses to populate the in-memory
// event-subgroup cache.
//
// These tests fail today because ZwiftAuth has no `get_event` method.

use prost::Message as _;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zwift_api::{Config, DEFAULT_SOURCE, DEFAULT_USER_AGENT, TOKEN_PATH, ZwiftAuth};
use zwift_proto::{Event, EventSubgroupProtobuf};

// ---------------------------------------------------------------------------
// Helpers (mirrors profile.rs)
// ---------------------------------------------------------------------------

fn config_for(server: &MockServer) -> Config {
    let base = server.uri();
    Config {
        auth_base: base.clone(),
        api_base:  base,
        source:    DEFAULT_SOURCE.to_string(),
        user_agent: DEFAULT_USER_AGENT.to_string(),
        platform:  "OSX".to_string(),
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

/// Mount standard token + /api/profiles/me mocks, log in, and return a
/// ready-to-use ZwiftAuth.
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

// ==========================================================================
// Step 13 — get_event
// ==========================================================================

/// A successful /api/events/{id} response is decoded into a zwift_proto::Event
/// with the correct id, name, and one subgroup carrying the expected fields.
#[tokio::test]
async fn get_event_parses_subgroups() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    let event = Event {
        id:   99_001,
        name: "Tour de Watopia Stage 1".to_string(),
        category: vec![EventSubgroupProtobuf {
            id:                   7_001,
            route_id:             3_366_225_080,
            course_id:            Some(3),
            distance_in_meters:   Some(20_000.0),
            duration_in_seconds:  Some(3_600),
            tags:                 Some("fenced;race;no_kick_mode".to_string()),
            invited_leaders:      vec![42],
            invited_sweepers:     vec![99],
            ..Default::default()
        }],
        ..Default::default()
    };

    Mock::given(method("GET"))
        .and(path("/api/events/99001"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-protobuf-lite")
                .set_body_bytes(event.encode_to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let result = auth
        .get_event(99_001)
        .await
        .expect("get_event must succeed on a 200 protobuf response");

    assert_eq!(result.id, 99_001, "event id must be decoded correctly");
    assert_eq!(result.name, "Tour de Watopia Stage 1");
    assert_eq!(
        result.category.len(),
        1,
        "one subgroup (category entry) must be decoded",
    );

    let sg = &result.category[0];
    assert_eq!(sg.id, 7_001, "subgroup id must be decoded");
    assert_eq!(sg.course_id, Some(3), "course_id must be decoded");
    assert!(
        (sg.distance_in_meters.unwrap_or(0.0) - 20_000.0).abs() < 0.1,
        "distance_in_meters must be 20000 m",
    );
    assert_eq!(sg.duration_in_seconds, Some(3_600), "duration must be decoded");
    assert_eq!(
        sg.tags.as_deref(),
        Some("fenced;race;no_kick_mode"),
        "tags string must be decoded intact",
    );
    assert_eq!(sg.invited_leaders, vec![42u64], "invited_leaders must be decoded");
    assert_eq!(sg.invited_sweepers, vec![99u64], "invited_sweepers must be decoded");
}

/// A 404 from /api/events/{id} surfaces as an Err (athlete not in any event,
/// or the event id is wrong).  It must not panic or return Ok(empty event).
#[tokio::test]
async fn get_event_404_is_an_error() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    Mock::given(method("GET"))
        .and(path("/api/events/0"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let result = auth.get_event(0).await;
    assert!(result.is_err(), "a 404 from get_event must be an Err, got: {result:?}");
}
