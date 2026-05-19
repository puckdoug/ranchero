// SPDX-License-Identifier: AGPL-3.0-only
//! 17.25-T — MIME type correctness for static files.
//!
//! `GET /pages/main.css` must return content-type `text/css`.
//! `GET /pages/app.mjs` must return content-type `text/javascript`.
//!
//! The `.mjs` case is non-trivial: some `mime_guess` versions return
//! `application/javascript` for `.mjs` files.  The implementation must
//! override this to match the `text/javascript` value sauce4zwift's
//! `src/mime.mjs` returns (17.25-I).
//!
//! Fails to compile until `ranchero::web::http::configure_static` is defined
//! (17.23-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.25-T.

use std::path::PathBuf;
use std::sync::Arc;

use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_static, WebState};

fn pages_root() -> PathBuf {
    PathBuf::from("pages")
}

#[actix_web::test]
async fn css_file_returns_text_css() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_static(pages_root()))
    ).await;

    let req = test::TestRequest::get().uri("/pages/main.css").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK,
        "GET /pages/main.css must return 200");

    let ct = resp.headers()
        .get("content-type")
        .expect("content-type must be present")
        .to_str().unwrap();
    assert!(ct.contains("text/css"),
        "content-type must be text/css; got {ct}");
}

#[actix_web::test]
async fn mjs_file_returns_text_javascript() {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_static(pages_root()))
    ).await;

    let req = test::TestRequest::get().uri("/pages/app.mjs").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK,
        "GET /pages/app.mjs must return 200");

    let ct = resp.headers()
        .get("content-type")
        .expect("content-type must be present")
        .to_str().unwrap();
    assert_eq!(ct, "text/javascript",
        "content-type for .mjs must be text/javascript, not application/javascript; got {ct}");
}
