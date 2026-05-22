// SPDX-License-Identifier: AGPL-3.0-only
//! 18.13-T — GET /api/athlete/streams/v1/{id} returns the stream object;
//! 404 for an unknown id; the `self` alias resolves to the logged-in athlete.
//!
//! Fails at runtime (not compile time) until 18.13-I registers the route in
//! `configure_api`.  Unregistered paths fall through to the API-directory
//! default service, which returns a JSON array, not an object.  That makes
//! the object-shape assertions below the primary RED signal; the 404 assertion
//! also fails because the default service always returns 200.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.13-T.

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

/// Route must return a JSON object with `time`, `power`, and `active` keys.
/// The seeded athlete has no recorded samples, so the arrays are empty.
///
/// Currently fails: the default service returns the API-directory array, not
/// a streams object.
#[actix_web::test]
async fn streams_v1_returns_object_for_known_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/streams/v1/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object()
        .expect("streams response must be a JSON object");
    assert!(obj.contains_key("time"),   "streams object must have 'time' key");
    assert!(obj.contains_key("power"),  "streams object must have 'power' key");
    assert!(obj.contains_key("active"), "streams object must have 'active' key");
}

#[actix_web::test]
async fn streams_v1_returns_404_for_unknown_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/streams/v1/9999")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// The `self` alias must resolve to athlete 1001 and return its streams.
///
/// Currently fails: default service returns the API-directory array.
#[actix_web::test]
async fn streams_v1_self_alias_resolves() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/streams/v1/self")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_object(), "self alias must return a streams object");
}
