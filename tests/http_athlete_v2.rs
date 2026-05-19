// SPDX-License-Identifier: AGPL-3.0-only
//! 17.10-T — GET /api/athlete/v2/:id?resource=stats&resource=lap returns
//! only the requested resource fields; omitting the query returns the
//! v1 shape with a version: 2 field added; unknown id returns 404.
//!
//! Fails at runtime (not compile time) until `configure_api` registers
//! /api/athlete/v2/:id. No real socket; no slow marker.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.10-T.

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

#[actix_web::test]
async fn athlete_v2_no_query_returns_v1_shape_with_version() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;

    assert_eq!(body["version"], 2, "v2 response must carry version: 2");
    assert_eq!(body["athleteId"], 1001);
    assert_eq!(body["courseId"], 5);
    assert!(body["lapCount"].is_number());
    assert!(body["stats"].is_object(),
        "no-query v2 response must include stats");
    assert!(body["lap"].is_object(),
        "no-query v2 response must include lap");
}

#[actix_web::test]
async fn athlete_v2_returns_404_for_unknown_id() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/9999")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn athlete_v2_resource_filter_returns_only_requested_keys() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=stats&resource=lap")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object()
        .expect("filtered v2 response must be a JSON object");

    assert!(obj.contains_key("stats"), "requested resource 'stats' must be present");
    assert!(obj.contains_key("lap"),   "requested resource 'lap' must be present");
    assert!(!obj.contains_key("athleteId"),
        "athleteId is not a resource key and must be absent when using resource filter");
}

#[actix_web::test]
async fn athlete_v2_resource_filter_excludes_unrequested_keys() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    // Request stats but not lap.
    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=stats")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object()
        .expect("filtered v2 response must be a JSON object");

    assert!(obj.contains_key("stats"), "requested resource 'stats' must be present");
    assert!(!obj.contains_key("lap"),
        "resource 'lap' was not requested and must be absent");
}
