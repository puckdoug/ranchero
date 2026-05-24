// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 3 — failing tests for self-identity wiring.
//
// Red state: `src/daemon/runtime.rs` (TODO 17.36-I) sets `watching_id` from
// the config but leaves `self_athlete_id` as `None`.  Both tests below express
// the desired post-fix behaviour and currently fail because `self_athlete_id`
// is never populated.
//
// The fix is a single line in `runtime.rs`:
//   s.self_athlete_id = cfg.watched_athlete_id.map(|id| id as u32);
//
// After that line is added:
//   - `resolve_athlete_id("self", &state)` returns `Some(watching_id)` instead
//     of `None`.
//   - `route_player_state` passes `self_athlete_id` (not `0`) to
//     `apply_event_state`, so the watched athlete is correctly exempt from
//     event-privacy flags.

use std::sync::Arc;

use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

/// Build a state that mirrors what `runtime.rs` currently produces:
/// `watching_id` is set, `self_athlete_id` is left `None` (the bug).
fn buggy_state(athlete_id: u32) -> web::Data<Arc<WebState>> {
    let mut registry = AthleteRegistry::new();
    registry.upsert(athlete_id, 5, 0, 0.0, 0.0);
    web::Data::new(Arc::new(WebState::with_registry(
        registry,
        Some(athlete_id), // watching_id — set by runtime
        None,             // self_athlete_id — NOT set by runtime (the bug)
    )))
}

// S3-A: GET /api/athlete/v1/self must resolve to the watched athlete.
// Currently fails with 404 because `resolve_athlete_id("self")` returns `None`
// when `self_athlete_id` is `None`.  After the runtime fix, `self_athlete_id`
// equals `watching_id` and the response is 200.
#[actix_web::test]
async fn web_state_self_id_equals_watched() {
    let state = buggy_state(42);
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v1/self")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /athlete/self must return 200 when watching_id is set; \
         fails until runtime.rs sets self_athlete_id = watching_id (TODO 17.36-I)",
    );
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(
        body["athleteId"],
        serde_json::json!(42),
        "response body must contain the watched athlete's id",
    );
}

// S3-B: The self alias must set the `self` flag in the response body, and
// `apply_event_state` must identify the watched athlete as self (not fall back
// to id 0).
//
// Part 1 (HTTP): GET /api/athlete/v1/self must return a body with `self: true`.
// Part 2 (apply_event_state): When `self_athlete_id = 0` (the current fallback),
// the watched athlete is treated as a non-self athlete; with the correct id it
// is exempt from event-privacy restrictions.
#[actix_web::test]
async fn self_alias_resolves_to_watched() {
    // --- Part 1: HTTP alias ---
    let state = buggy_state(99);
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v1/self")
        .to_request();
    let resp = test::call_service(&app, req).await;

    // Fails: 404 because self_athlete_id is None.
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /athlete/self must return 200; \
         fails until self_athlete_id is populated in the runtime",
    );
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(
        body["self"],
        serde_json::json!(true),
        "response body must have self: true for the watched athlete",
    );

    // --- Part 2: apply_event_state self-comparison ---
    // When `state.self_athlete_id.unwrap_or(0)` returns 0 (current broken state),
    // athlete 99 is treated as non-self and event-privacy tags are applied to it.
    // After the fix, `unwrap_or(0)` returns 99, and the self athlete is exempt.
    use std::collections::HashMap;
    use zwift_stats::{apply_event_state, AthleteData, EventBehavior, MostRecentState};
    use zwift_stats::athlete::EventSubgroup;

    let mut ad = AthleteData::new(99, 42, 1, 0.0, 10.0);
    let sg = EventSubgroup {
        id:            7,
        course_id:     1,
        all_tags:      vec!["hideftp".to_owned()],
        end_ts:        0,
        end_distance:  0.0,
    };
    let mut sgs = HashMap::new();
    sgs.insert(7u32, sg);

    let state_view = MostRecentState {
        world_time:        5000.0,
        speed:             0.0,
        power:             0.0,
        heartrate:         0,
        cadence:           0,
        draft:             0.0,
        distance:          0.0,
        altitude:          0.0,
        lat:               0.0,
        lng:               0.0,
        course_id:         1,
        road_id:           1,
        road_time:         5000.0,
        reverse:           false,
        event_subgroup_id: 7,
        group_id:          0,
        time:              5.0,
        event_distance:    0.0,
    };

    // Pass self_athlete_id = 0 (the current broken fallback when None is unwrapped).
    // Athlete 99 != 0 so it is treated as non-self → hideftp flag is applied.
    apply_event_state(
        &mut ad, &state_view, 0, &sgs,
        EventBehavior { auto_reset: false, auto_lap: false },
        10.0, 0,
    );

    // Fails: hide_ftp is true because athlete 99 was treated as non-self (id 0 != 99).
    // After fix: self_athlete_id = Some(99) → unwrap_or(0) = 99 → exempt → hide_ftp = false.
    assert!(
        !ad.event_privacy.hide_ftp,
        "the watched/self athlete must be exempt from hideftp; \
         fails when self_athlete_id falls back to 0 instead of the watched id",
    );
}
