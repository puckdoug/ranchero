// SPDX-License-Identifier: AGPL-3.0-only
//! 17.24-T — GET / returns pages/index.html.
//!
//! `GET /` must return status 200 with content-type `text/html` and a
//! non-empty body whose content matches `pages/index.html`.
//! `Files::new` mounted at `/pages` does not serve the bare-root path,
//! so an explicit route is required (17.24-I).
//!
//! Fails to compile until `ranchero::web::http::configure_static` is defined
//! (17.23-I / 17.24-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.24-T.

use std::path::PathBuf;
use std::sync::Arc;

use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_static, WebState};

fn pages_root() -> PathBuf {
    PathBuf::from("pages")
}

#[actix_web::test]
async fn get_root_returns_pages_index_html() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_static(pages_root()))
    ).await;

    let req = test::TestRequest::get().uri("/").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK,
        "GET / must return 200");

    let ct = resp.headers()
        .get("content-type")
        .expect("content-type must be present")
        .to_str().unwrap();
    assert!(ct.contains("text/html"),
        "content-type must be text/html; got {ct}");

    let body = test::read_body(resp).await;
    assert!(!body.is_empty(), "body must not be empty");

    // The expected content is from pages/index.html; verify a known string.
    let body_str = std::str::from_utf8(&body).expect("body must be valid UTF-8");
    assert!(body_str.contains("<!DOCTYPE html"),
        "body must be the contents of pages/index.html; got: {body_str:.200}");
}
