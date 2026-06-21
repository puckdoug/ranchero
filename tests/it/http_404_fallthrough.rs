// SPDX-License-Identifier: AGPL-3.0-only
//! 17.9-T — GET /api/<unknown> returns 200 with the API directory
//! listing rather than a 404 error body, matching the fallthrough
//! handler in sauce4zwift `webserver.mjs:495`.
//!
//! Fails to compile until `ranchero::web::http::configure_api` exists.
//! No real socket; no slow marker.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.9-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, WebState};

#[actix_web::test]
async fn unknown_api_path_returns_200_not_404() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/not-a-real-endpoint")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK,
        "unmatched /api/* must fall through to the directory, not a 404");

    let ct = resp.headers()
        .get("content-type")
        .expect("content-type must be present")
        .to_str().unwrap();
    assert!(ct.contains("application/json"),
        "fallthrough response must be JSON; got {ct}");

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_array(),
        "body must be the API directory array, not an error object");
}

#[actix_web::test]
async fn unknown_api_path_body_matches_api_root() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let root_body: serde_json::Value = {
        let req = test::TestRequest::get().uri("/api/").to_request();
        test::read_body_json(test::call_service(&app, req).await).await
    };

    let fallthrough_body: serde_json::Value = {
        let req = test::TestRequest::get()
            .uri("/api/this-does-not-exist")
            .to_request();
        test::read_body_json(test::call_service(&app, req).await).await
    };

    assert_eq!(root_body, fallthrough_body,
        "fallthrough body must equal the /api/ directory listing");
}
