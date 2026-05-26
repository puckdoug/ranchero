// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 15 — ZwiftAuth segment-leaderboard fetchers (wiremock tests).
//
// Three protobuf-lite endpoints, ported from sauce4zwift's
// `zwift.mjs:633-678`:
//
// * `get_live_segment_leaders()`           → GET /live-segment-results-service/leaders
// * `get_live_segment_leaderboard(seg_id)` → GET /live-segment-results-service/leaderboard/{seg_id}
// * `get_segment_results(seg_ids, opts)`   → GET /api/segment-results?world_id=1&segment_id=…
//
// Each returns a decoded `zwift_proto::SegmentResults` (or its inner
// `Vec<SegmentResult>`). Tests fail today because the three methods do not
// exist on `ZwiftAuth`.

use prost::Message as _;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zwift_api::{Config, DEFAULT_SOURCE, DEFAULT_USER_AGENT, TOKEN_PATH, ZwiftAuth};
use zwift_proto::{SegmentResult, SegmentResults};

// ---------------------------------------------------------------------------
// Helpers (mirrors event.rs)
// ---------------------------------------------------------------------------

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

fn sample_result(player_id: u64, segment_id: i64, elapsed_ms: u64) -> SegmentResult {
    SegmentResult {
        player_id,
        segment_id: Some(segment_id),
        first_name: "A".to_string(),
        last_name:  "Rider".to_string(),
        elapsed_ms,
        ..Default::default()
    }
}

// ==========================================================================
// get_live_segment_leaders
// ==========================================================================

/// `get_live_segment_leaders()` issues GET against
/// `/live-segment-results-service/leaders` with the protobuf-lite Accept
/// header, decodes a `SegmentResults` body, and returns the inner results.
/// Sauce filters out entries whose `id` is falsy (`zwift.mjs:635`); this test
/// asserts that filter is applied.
#[tokio::test]
async fn get_live_segment_leaders_parses() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    let body = SegmentResults {
        server_realm: 1,
        segment_id:   0,
        event_subgroup_id: None,
        segment_results: vec![
            SegmentResult { id: Some(7_001), ..sample_result(10, 5_555, 90_000) },
            SegmentResult { id: Some(7_002), ..sample_result(11, 5_555, 91_500) },
            // id == 0 must be filtered out (sauce convertSegmentResultProtobuf).
            SegmentResult { id: Some(0),     ..sample_result(12, 5_555, 92_000) },
        ],
    };

    Mock::given(method("GET"))
        .and(path("/live-segment-results-service/leaders"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-protobuf-lite")
                .set_body_bytes(body.encode_to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let leaders = auth
        .get_live_segment_leaders()
        .await
        .expect("get_live_segment_leaders must succeed on a 200 protobuf response");

    assert_eq!(
        leaders.len(),
        2,
        "id==0 entries must be filtered out (sauce zwift.mjs:635)",
    );
    assert_eq!(leaders[0].id, Some(7_001));
    assert_eq!(leaders[0].player_id, 10);
    assert_eq!(leaders[0].elapsed_ms, 90_000);
    assert_eq!(leaders[1].id, Some(7_002));
}

// ==========================================================================
// get_live_segment_leaderboard
// ==========================================================================

/// `get_live_segment_leaderboard(seg_id)` issues GET against
/// `/live-segment-results-service/leaderboard/{seg_id}` and returns the
/// decoded `SegmentResults` body's `segment_results`. Unlike the `leaders`
/// fetcher, sauce does **not** filter on `id` here (`zwift.mjs:641`).
#[tokio::test]
async fn get_live_segment_leaderboard_parses() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    let body = SegmentResults {
        server_realm: 1,
        segment_id:   5_555,
        event_subgroup_id: None,
        segment_results: vec![
            sample_result(10, 5_555, 88_000),
            sample_result(11, 5_555, 93_500),
        ],
    };

    Mock::given(method("GET"))
        .and(path("/live-segment-results-service/leaderboard/5555"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-protobuf-lite")
                .set_body_bytes(body.encode_to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let board = auth
        .get_live_segment_leaderboard(5_555)
        .await
        .expect("get_live_segment_leaderboard must succeed on 200");

    assert_eq!(board.len(), 2);
    assert_eq!(board[0].player_id, 10);
    assert_eq!(board[0].elapsed_ms, 88_000);
    assert_eq!(board[1].player_id, 11);
}

// ==========================================================================
// get_segment_results
// ==========================================================================

/// `get_segment_results(&[seg_id, ...])` issues GET against
/// `/api/segment-results?world_id=1&segment_id=...&segment_id=...`, decodes
/// a `SegmentResults` body, sorts by `elapsed_ms` ascending, and returns the
/// sorted list. Sauce sorts by elapsed in `zwift.mjs:677`; this test verifies
/// both query-param encoding and the sort.
#[tokio::test]
async fn get_segment_results_parses() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    // Server returns results out of order; client must sort ascending by
    // elapsed_ms.
    let body = SegmentResults {
        server_realm: 1,
        segment_id:   5_555,
        event_subgroup_id: None,
        segment_results: vec![
            sample_result(11, 5_555, 95_000),
            sample_result(10, 5_555, 88_000),
            sample_result(12, 6_666, 91_000),
        ],
    };

    Mock::given(method("GET"))
        .and(path("/api/segment-results"))
        .and(query_param("world_id", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-protobuf-lite")
                .set_body_bytes(body.encode_to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let results = auth
        .get_segment_results(&[5_555, 6_666])
        .await
        .expect("get_segment_results must succeed on a 200 protobuf response");

    assert_eq!(results.len(), 3, "all three results must be returned");
    assert_eq!(
        results[0].elapsed_ms, 88_000,
        "results must be sorted by elapsed_ms ascending (sauce zwift.mjs:677)",
    );
    assert_eq!(results[1].elapsed_ms, 91_000);
    assert_eq!(results[2].elapsed_ms, 95_000);
}

// ==========================================================================
// Error surfacing
// ==========================================================================

/// A non-2xx response from the leaderboard endpoint surfaces as `Err`, never
/// `Ok(empty)`. Parity with `get_event`'s 404 behaviour and matches sauce's
/// `fetchPB` (which throws on non-OK).
#[tokio::test]
async fn get_live_segment_leaderboard_404_is_an_error() {
    let server = MockServer::start().await;
    let auth   = setup_authed_client(&server).await;

    Mock::given(method("GET"))
        .and(path("/live-segment-results-service/leaderboard/9999"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let result = auth.get_live_segment_leaderboard(9_999).await;
    assert!(
        result.is_err(),
        "404 from get_live_segment_leaderboard must be Err, got: {result:?}",
    );
}
