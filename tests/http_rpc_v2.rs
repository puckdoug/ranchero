// SPDX-License-Identifier: AGPL-3.0-only
//! 17.13-T — GET /api/rpc/v2/:name/{base64url_arg1}/.../{base64url_argN}
//! decodes each path segment as a separately base64url-encoded JSON value
//! and dispatches to the registered handler with those args.
//!
//! Fails to compile until RpcRegistry and WebState::with_rpc are defined.
//! No real socket; no slow marker.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.13-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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
async fn rpc_v2_dispatches_base64url_encoded_args() {
    let state = echo_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    // Each path segment is the base64url (no padding) encoding of a JSON value.
    let a = URL_SAFE_NO_PAD.encode("1");        // JSON number 1
    let b = URL_SAFE_NO_PAD.encode("true");     // JSON boolean true
    let c = URL_SAFE_NO_PAD.encode("\"foo\"");  // JSON string "foo"
    let uri = format!("/api/rpc/v2/echo/{a}/{b}/{c}");

    let req = test::TestRequest::get().uri(&uri).to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["success"], json!(true),
        "v2 dispatch must succeed; got {body}");
    assert_eq!(body["data"], json!([1, true, "foo"]),
        "each base64url segment must decode to its JSON value; got {body}");
}
