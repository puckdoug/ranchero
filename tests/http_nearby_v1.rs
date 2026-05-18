// SPDX-License-Identifier: AGPL-3.0-only
//! 17.7-T — GET /api/nearby/v1 returns a JSON array of nearby athletes,
//! each formatted with the same shape as /api/athlete/v1/:id.
//!
//! Fails to compile until `ranchero::web::AthleteRegistry`,
//! `WebState::with_registry`, and `configure_api` exist.
//!
//! No real socket; no slow marker.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.7-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

#[actix_web::test]
async fn nearby_v1_returns_all_registered_athletes() {
    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    registry.upsert(1002, 5, 0, 0.0, 0.0);
    let state = web::Data::new(Arc::new(
        WebState::with_registry(registry, Some(1001), None)
    ));

    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/nearby/v1").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_array(), "nearby must return a JSON array");
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2, "both registered athletes must appear");
    for entry in arr {
        assert!(entry["athleteId"].is_number(),
            "each nearby entry must carry an athleteId");
    }
}

#[actix_web::test]
async fn nearby_v1_returns_empty_array_when_no_athletes() {
    let registry = AthleteRegistry::new();
    let state = web::Data::new(Arc::new(
        WebState::with_registry(registry, None, None)
    ));

    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/nearby/v1").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body, serde_json::json!([]));
}
