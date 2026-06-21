// SPDX-License-Identifier: AGPL-3.0-only
//! 17.12-T — GET /api/rpc/v1/:name?arg=1&arg=true&arg=foo dispatches to
//! the registered handler with type-coerced args [1, true, "foo"].
//! POST /api/rpc/v1/:name with body [1, true, "foo"] dispatches with the
//! same args. Unknown handler returns 404. The built-in getVersion handler
//! is pre-registered and returns a string.
//!
//! Fails to compile until RpcRegistry and WebState::with_rpc are defined.
//! No real socket; no slow marker.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.12-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use serde_json::{json, Value};
use ranchero::web::{http::configure_api, RpcRegistry, WebState};

fn echo_state() -> web::Data<Arc<WebState>> {
    let mut rpc = RpcRegistry::new();
    rpc.register("echo", |args: Vec<Value>| async move {
        Ok::<Value, String>(json!(args))
    });
    web::Data::new(Arc::new(WebState::with_rpc(rpc)))
}

#[actix_web::test]
async fn rpc_v1_get_dispatches_with_type_coerced_args() {
    let state = echo_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/rpc/v1/echo?arg=1&arg=true&arg=foo")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["success"], json!(true),
        "success must be true; got {body}");
    assert_eq!(body["data"], json!([1, true, "foo"]),
        "args must be type-coerced: 1→number, true→boolean, foo→string; got {body}");
}

#[actix_web::test]
async fn rpc_v1_post_dispatches_with_json_body() {
    let state = echo_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::post()
        .uri("/api/rpc/v1/echo")
        .set_json(json!([1, true, "foo"]))
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["success"], json!(true),
        "success must be true; got {body}");
    assert_eq!(body["data"], json!([1, true, "foo"]),
        "POST body args must be forwarded as-is; got {body}");
}

#[actix_web::test]
async fn rpc_v1_unknown_handler_returns_404() {
    let state = echo_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::post()
        .uri("/api/rpc/v1/nonexistent")
        .set_json(json!([]))
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn rpc_v1_get_version_builtin_returns_string() {
    let state = web::Data::new(Arc::new(WebState::with_rpc(RpcRegistry::new())));
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::post()
        .uri("/api/rpc/v1/getVersion")
        .set_json(json!([]))
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["success"], json!(true),
        "getVersion must succeed; got {body}");
    assert!(body["data"].is_string(),
        "getVersion must return a string; got {body}");
}
