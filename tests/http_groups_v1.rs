// SPDX-License-Identifier: AGPL-3.0-only
//! 17.8-T — GET /api/groups/v1 returns a JSON array of group objects;
//! each object carries an `athletes` array formatted with the same shape
//! as /api/athlete/v1/:id.
//!
//! Fails to compile until `ranchero::web::AthleteRegistry`,
//! `WebState::with_registry`, and `configure_api` exist.
//!
//! No real socket; no slow marker.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.8-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

#[actix_web::test]
async fn groups_v1_returns_json_array() {
    let registry = AthleteRegistry::new();
    let state = web::Data::new(Arc::new(
        WebState::with_registry(registry, None, None)
    ));

    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/groups/v1").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_array(), "groups must return a JSON array");
}

#[actix_web::test]
async fn groups_v1_each_group_has_athletes_array() {
    let mut registry = AthleteRegistry::new();
    {
        let ad = registry.upsert(1001, 5, 0, 0.0, 0.0);
        ad.group_id = Some(1);
    }
    {
        let ad = registry.upsert(1002, 5, 0, 0.0, 0.0);
        ad.group_id = Some(1);
    }
    registry.touch_group(1, 0.0);

    let state = web::Data::new(Arc::new(
        WebState::with_registry(registry, None, None)
    ));

    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/groups/v1").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let groups = body.as_array().unwrap();
    assert!(!groups.is_empty(), "at least one group must appear");
    for group in groups {
        assert!(group["athletes"].is_array(),
            "each group must have an 'athletes' array; got {group}");
    }
}
