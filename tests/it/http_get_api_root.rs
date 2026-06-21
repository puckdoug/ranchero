// SPDX-License-Identifier: AGPL-3.0-only
//! 17.4-T — GET /api/ returns a JSON directory listing of registered
//! endpoints, status 200, content-type application/json.
//!
//! Fails to compile until `ranchero::web::http::configure_api` exists.
//! No real socket; no slow marker.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.4-T.

use std::sync::Arc;
use actix_web::{test, web, App};
use ranchero::web::{http::configure_api, WebState};

#[actix_web::test]
async fn get_api_root_returns_200_json_array() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

    let ct = resp.headers()
        .get("content-type")
        .expect("content-type header must be present")
        .to_str().unwrap();
    assert!(ct.contains("application/json"),
        "content-type must be application/json; got {ct}");

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_array(), "API directory must be a JSON array");
    assert!(!body.as_array().unwrap().is_empty(),
        "API directory must list at least one endpoint");
}
