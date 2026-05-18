// SPDX-License-Identifier: AGPL-3.0-only
//! 17.5-T — OPTIONS requests on /api/* return 204 with
//! `Access-Control-Allow-Origin: *` and `Access-Control-Allow-Headers: *`.
//!
//! Fails to compile until `ranchero::web::http::configure_api` exists.
//! No real socket; no slow marker.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.5-T.

use std::sync::Arc;
use actix_web::{http, test, web, App};
use ranchero::web::{http::configure_api, WebState};

#[actix_web::test]
async fn options_api_returns_204_with_cors_headers() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_api)
    ).await;

    let req = test::TestRequest::default()
        .method(http::Method::OPTIONS)
        .uri("/api/athlete/v1/1")
        .insert_header(("Origin", "http://localhost:3000"))
        .insert_header(("Access-Control-Request-Method", "GET"))
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), http::StatusCode::NO_CONTENT);

    let acao = resp.headers()
        .get("access-control-allow-origin")
        .expect("access-control-allow-origin must be present")
        .to_str().unwrap();
    assert_eq!(acao, "*");

    let acah = resp.headers()
        .get("access-control-allow-headers")
        .expect("access-control-allow-headers must be present")
        .to_str().unwrap();
    assert_eq!(acah, "*");
}
