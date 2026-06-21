// SPDX-License-Identifier: AGPL-3.0-only
//! 17.14-T — GET /api/rpc/v1 and GET /api/rpc/v2 both return a JSON array
//! of registered handler name strings. The built-in getVersion handler
//! must appear in both lists. A custom-registered handler also appears.
//!
//! Fails to compile until RpcRegistry and WebState::with_rpc are defined.
//! No real socket; no slow marker.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.14-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use serde_json::{json, Value};
use ranchero::web::{http::configure_api, RpcRegistry, WebState};

fn state_with_custom_handler() -> web::Data<Arc<WebState>> {
    let mut rpc = RpcRegistry::new();
    rpc.register("myHandler", |_args: Vec<Value>| async move {
        Ok::<Value, String>(json!(null))
    });
    web::Data::new(Arc::new(WebState::with_rpc(rpc)))
}

#[actix_web::test]
async fn rpc_discovery_v1_lists_registered_handlers() {
    let state = state_with_custom_handler();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/rpc/v1").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    let names = body.as_array()
        .expect("rpc/v1 discovery must return a JSON array");

    let name_strs: Vec<&str> = names.iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(name_strs.contains(&"getVersion"),
        "built-in getVersion must appear in discovery list; got {body}");
    assert!(name_strs.contains(&"myHandler"),
        "registered myHandler must appear in discovery list; got {body}");
}

#[actix_web::test]
async fn rpc_discovery_v2_lists_registered_handlers() {
    let state = state_with_custom_handler();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/rpc/v2").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    let names = body.as_array()
        .expect("rpc/v2 discovery must return a JSON array");

    let name_strs: Vec<&str> = names.iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(name_strs.contains(&"getVersion"),
        "built-in getVersion must appear in discovery list; got {body}");
    assert!(name_strs.contains(&"myHandler"),
        "registered myHandler must appear in discovery list; got {body}");
}
