// SPDX-License-Identifier: AGPL-3.0-only
//! 17.23-T — Static file serving under /pages.
//!
//! `GET /pages/index.html` returns the file with status 200, content-type
//! `text/html`, and `Access-Control-Allow-Origin: *`.
//! `GET /pages/missing.html` returns 404.
//!
//! Fails to compile until `ranchero::web::http::configure_static` is defined
//! (17.23-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.23-T.

use std::path::PathBuf;
use std::sync::Arc;

use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_static, WebState};

fn pages_root() -> PathBuf {
    PathBuf::from("pages")
}

#[actix_web::test]
async fn pages_index_html_returns_200_with_text_html_and_cors() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_static(pages_root()))
    ).await;

    let req = test::TestRequest::get()
        .uri("/pages/index.html")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK,
        "GET /pages/index.html must return 200");

    let ct = resp.headers()
        .get("content-type")
        .expect("content-type must be present")
        .to_str().unwrap();
    assert!(ct.contains("text/html"),
        "content-type must be text/html; got {ct}");

    let acao = resp.headers()
        .get("access-control-allow-origin")
        .expect("access-control-allow-origin must be present")
        .to_str().unwrap();
    assert_eq!(acao, "*",
        "Access-Control-Allow-Origin must be *");
}

#[actix_web::test]
async fn pages_missing_file_returns_404() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_static(pages_root()))
    ).await;

    let req = test::TestRequest::get()
        .uri("/pages/missing.html")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND,
        "GET /pages/missing.html must return 404");
}
