// SPDX-License-Identifier: AGPL-3.0-only
//! 19.1-T — HTTP serving of the ranchero widget pages.
//!
//! Asserts that each new widget page and its companion ES module are served
//! with the correct status, MIME type, and CORS header.  Also checks the
//! shared WebSocket client module under `/pages/shared/`.
//!
//! Pages under test:
//!   /pages/watching.html  + /pages/watching.mjs   — live stats widget
//!   /pages/nearby.html    + /pages/nearby.mjs     — nearby riders list
//!   /pages/groups.html    + /pages/groups.mjs     — groups view
//!   /pages/map.html       + /pages/map.mjs        — position/map widget
//!   /pages/shared/ws-client.mjs                   — shared WebSocket client
//!
//! Every test fails until 19.1-I creates the corresponding file under `pages/`.
//!
//! See docs/planning/STEP-19-compatibility-tests.md, item 19.1-T.

use std::path::PathBuf;
use std::sync::Arc;

use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_static, WebState};

fn pages_root() -> PathBuf {
    PathBuf::from("pages")
}

async fn assert_page(uri: &str) {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_static(pages_root())),
    )
    .await;

    let req = test::TestRequest::get().uri(uri).to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK, "GET {uri} must return 200");

    let ct = resp
        .headers()
        .get("content-type")
        .unwrap_or_else(|| panic!("GET {uri}: content-type header absent"))
        .to_str()
        .unwrap();
    assert!(
        ct.contains("text/html"),
        "GET {uri}: content-type must be text/html; got {ct}"
    );

    let acao = resp
        .headers()
        .get("access-control-allow-origin")
        .unwrap_or_else(|| panic!("GET {uri}: access-control-allow-origin header absent"))
        .to_str()
        .unwrap();
    assert_eq!(acao, "*", "GET {uri}: Access-Control-Allow-Origin must be *");
}

async fn assert_module(uri: &str) {
    let state = web::Data::new(Arc::new(WebState::new()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(configure_static(pages_root())),
    )
    .await;

    let req = test::TestRequest::get().uri(uri).to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK, "GET {uri} must return 200");

    let ct = resp
        .headers()
        .get("content-type")
        .unwrap_or_else(|| panic!("GET {uri}: content-type header absent"))
        .to_str()
        .unwrap();
    assert_eq!(
        ct, "text/javascript",
        "GET {uri}: content-type for .mjs must be text/javascript; got {ct}"
    );

    let acao = resp
        .headers()
        .get("access-control-allow-origin")
        .unwrap_or_else(|| panic!("GET {uri}: access-control-allow-origin header absent"))
        .to_str()
        .unwrap();
    assert_eq!(acao, "*", "GET {uri}: Access-Control-Allow-Origin must be *");
}

// --- watching widget ---

#[actix_web::test]
async fn watching_html_returns_200_text_html() {
    assert_page("/pages/watching.html").await;
}

#[actix_web::test]
async fn watching_mjs_returns_200_text_javascript() {
    assert_module("/pages/watching.mjs").await;
}

// --- nearby list ---

#[actix_web::test]
async fn nearby_html_returns_200_text_html() {
    assert_page("/pages/nearby.html").await;
}

#[actix_web::test]
async fn nearby_mjs_returns_200_text_javascript() {
    assert_module("/pages/nearby.mjs").await;
}

// --- groups view ---

#[actix_web::test]
async fn groups_html_returns_200_text_html() {
    assert_page("/pages/groups.html").await;
}

#[actix_web::test]
async fn groups_mjs_returns_200_text_javascript() {
    assert_module("/pages/groups.mjs").await;
}

// --- map / position widget ---

#[actix_web::test]
async fn map_html_returns_200_text_html() {
    assert_page("/pages/map.html").await;
}

#[actix_web::test]
async fn map_mjs_returns_200_text_javascript() {
    assert_module("/pages/map.mjs").await;
}

// --- shared WebSocket client ---

#[actix_web::test]
async fn shared_ws_client_mjs_returns_200_text_javascript() {
    assert_module("/pages/shared/ws-client.mjs").await;
}
