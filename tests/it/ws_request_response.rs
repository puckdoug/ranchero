// SPDX-License-Identifier: AGPL-3.0-only
//! 17.16-T — WebSocket accept-liberal / emit-strict rule.
//!
//! (a) nested wire form dispatches and produces a matching uid response;
//! (b) flat spec form produces the same response;
//! (c) an extra unknown field is silently ignored;
//! (d) an unknown RPC name produces success:false with an error message;
//! (e) a structurally malformed frame (missing method) produces
//!     success:false with the echoed uid where available, or -1.
//!
//! Fails at runtime (not compile time) until the /api/ws/events route
//! and the frame codec are implemented.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.16-T.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use ranchero::web::{start, RpcRegistry, WebState};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn ws_send(ws: &mut WsStream, payload: Value) {
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .expect("ws send must succeed");
}

async fn ws_recv(ws: &mut WsStream) -> Value {
    loop {
        let msg = ws.next().await
            .expect("ws stream must not end")
            .expect("ws frame must be valid");
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text)
                .expect("response frame must be valid JSON");
        }
        // skip pings and other non-text frames
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_nested_form_dispatches_rpc() {
    let cfg   = super::common::test_config("ws-rr");
    let state = Arc::new(WebState::with_rpc(RpcRegistry::new()));
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws = connect_async(&url).await.expect("ws connect").0;

    let nested = json!({
        "type": "request",
        "uid":  42,
        "data": { "method": "rpc", "arg": { "name": "getVersion", "args": [] } }
    });
    ws_send(&mut ws, nested).await;
    let resp = ws_recv(&mut ws).await;

    assert_eq!(resp["type"],    "response", "got {resp}");
    assert_eq!(resp["success"], true,        "got {resp}");
    assert_eq!(resp["uid"],     42,          "uid must be echoed; got {resp}");
    assert!(resp["data"].is_string(),        "getVersion must return a string; got {resp}");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_flat_form_dispatches_rpc() {
    let cfg   = super::common::test_config("ws-rr");
    let state = Arc::new(WebState::with_rpc(RpcRegistry::new()));
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws = connect_async(&url).await.expect("ws connect").0;

    let flat = json!({
        "type":   "request",
        "method": "rpc",
        "uid":    42,
        "arg":    { "name": "getVersion", "args": [] }
    });
    ws_send(&mut ws, flat).await;
    let resp = ws_recv(&mut ws).await;

    assert_eq!(resp["type"],    "response", "got {resp}");
    assert_eq!(resp["success"], true,        "got {resp}");
    assert_eq!(resp["uid"],     42,          "got {resp}");
    assert!(resp["data"].is_string(),        "got {resp}");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_extra_field_is_ignored() {
    let cfg   = super::common::test_config("ws-rr");
    let state = Arc::new(WebState::with_rpc(RpcRegistry::new()));
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws = connect_async(&url).await.expect("ws connect").0;

    let with_extra = json!({
        "type":    "request",
        "method":  "rpc",
        "uid":     43,
        "arg":     { "name": "getVersion", "args": [] },
        "extra":   "this field must be ignored"
    });
    ws_send(&mut ws, with_extra).await;
    let resp = ws_recv(&mut ws).await;

    assert_eq!(resp["success"], true, "extra field must not cause failure; got {resp}");
    assert_eq!(resp["uid"],     43,   "got {resp}");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_unknown_rpc_name_returns_error() {
    let cfg   = super::common::test_config("ws-rr");
    let state = Arc::new(WebState::with_rpc(RpcRegistry::new()));
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws = connect_async(&url).await.expect("ws connect").0;

    let unknown = json!({
        "type":   "request",
        "method": "rpc",
        "uid":    10,
        "arg":    { "name": "noSuchHandler", "args": [] }
    });
    ws_send(&mut ws, unknown).await;
    let resp = ws_recv(&mut ws).await;

    assert_eq!(resp["type"],    "response", "got {resp}");
    assert_eq!(resp["success"], false,       "unknown handler must fail; got {resp}");
    assert_eq!(resp["uid"],     10,          "got {resp}");
    let error = resp["error"].as_str().expect("error field must be a string");
    assert!(
        error.contains("unknown rpc handler") && error.contains("noSuchHandler"),
        "error message must name the handler; got {resp}"
    );

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_malformed_frame_no_method_returns_error() {
    let cfg   = super::common::test_config("ws-rr");
    let state = Arc::new(WebState::with_rpc(RpcRegistry::new()));
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws = connect_async(&url).await.expect("ws connect").0;

    // Frame has a uid but no method — structurally malformed.
    let malformed = json!({ "type": "request", "uid": 5 });
    ws_send(&mut ws, malformed).await;
    let resp = ws_recv(&mut ws).await;

    assert_eq!(resp["type"],    "response", "got {resp}");
    assert_eq!(resp["success"], false,       "malformed frame must fail; got {resp}");
    assert_eq!(resp["uid"],     5,           "uid must be echoed; got {resp}");

    // Frame with no uid at all — server must substitute -1.
    let no_uid = json!({ "type": "request" });
    ws_send(&mut ws, no_uid).await;
    let resp2 = ws_recv(&mut ws).await;

    assert_eq!(resp2["success"], false, "got {resp2}");
    assert_eq!(resp2["uid"],     -1,    "missing uid must become -1; got {resp2}");

    shutdown.notify_one();
    handle.stop().await;
}
