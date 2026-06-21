// SPDX-License-Identifier: AGPL-3.0-only
//! 18.11-T — `/api/athlete/laps/v1/:id`, `/api/athlete/segments/v1/:id`,
//! `/api/athlete/events/v1/:id` return formatted slice arrays; 404 for an
//! unknown id; the `self` alias resolves to the logged-in athlete.
//!
//! Each route is called with the `{active: true}` filter (matching
//! `webserver.mjs:323-336`): open slices are included in the response.
//!
//! Fails at runtime (not compile time) until 18.11-I registers the three
//! routes in `configure_api`.  The unregistered routes fall through to the
//! API-directory default service, which returns a 1-element array containing
//! the directory object.  That object lacks `active`/`courseId`/etc., so the
//! content assertions below are the primary RED signal; 404 assertions also
//! fail because the default service always returns 200.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.11-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

fn seeded_state() -> web::Data<Arc<WebState>> {
    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    web::Data::new(Arc::new(WebState::with_registry(
        registry,
        Some(1001),
        Some(1001),
    )))
}

// ---------------------------------------------------------------------------
// laps/v1
// ---------------------------------------------------------------------------

/// Route must return an array of slice objects (each with `active`, `courseId`).
/// The seeded athlete has one initial open lap slice, so the array has 1 element.
///
/// Currently fails: the default service returns the API-directory array, whose
/// first element is an object with URL keys, not `active`/`courseId`.
#[actix_web::test]
async fn laps_v1_returns_slice_array_for_known_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/laps/v1/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let arr = body.as_array().expect("laps response must be a JSON array");
    assert_eq!(arr.len(), 1, "seeded athlete has exactly one lap slice");
    assert_eq!(arr[0]["active"], true, "initial slice is open");
    assert!(arr[0]["courseId"].is_number(), "slice must have courseId field");
}

#[actix_web::test]
async fn laps_v1_returns_404_for_unknown_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/laps/v1/9999")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// The `self` alias must resolve to athlete 1001 and return its laps.
///
/// Currently fails: default service returns the API-directory array, not slices.
#[actix_web::test]
async fn laps_v1_self_alias_resolves() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/laps/v1/self")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let arr = body.as_array().expect("self alias must return a JSON array");
    assert_eq!(arr.len(), 1, "self resolves to athlete 1001 with one lap slice");
    assert!(arr[0]["courseId"].is_number(), "slice must have courseId field");
}

// ---------------------------------------------------------------------------
// segments/v1
// ---------------------------------------------------------------------------

/// The seeded athlete has no segment slices, so the route must return an empty
/// array.  Currently fails: default service returns a 1-element directory array.
#[actix_web::test]
async fn segments_v1_returns_empty_array_for_athlete_with_no_segments() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/segments/v1/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let arr = body.as_array().expect("segments response must be a JSON array");
    assert!(arr.is_empty(), "seeded athlete has no segment slices");
}

#[actix_web::test]
async fn segments_v1_returns_404_for_unknown_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/segments/v1/9999")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// events/v1
// ---------------------------------------------------------------------------

/// The seeded athlete has no event slices, so the route must return an empty
/// array.  Currently fails: default service returns a 1-element directory array.
#[actix_web::test]
async fn events_v1_returns_empty_array_for_athlete_with_no_events() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/events/v1/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let arr = body.as_array().expect("events response must be a JSON array");
    assert!(arr.is_empty(), "seeded athlete has no event slices");
}

#[actix_web::test]
async fn events_v1_returns_404_for_unknown_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/events/v1/9999")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
