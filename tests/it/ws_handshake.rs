// SPDX-License-Identifier: AGPL-3.0-only
//! 17.15-T — a WebSocket client completes the upgrade on /api/ws/events.
//!
//! Fails at runtime (not compile time) until `configure_api` registers
//! the /api/ws/events route.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.15-T.

use std::sync::Arc;

use ranchero::web::{start, RpcRegistry, WebState};
use tokio::sync::Notify;
use tokio_tungstenite::connect_async;

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_handshake_completes_on_api_ws_events() {
    let cfg   = super::common::test_config("ws-handshake");
    let state = Arc::new(WebState::with_rpc(RpcRegistry::new()));
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone())
        .await
        .expect("web server must start");
    let addr = handle.local_addr();

    let url = format!("ws://{addr}/api/ws/events");
    let (ws, _response) = connect_async(&url)
        .await
        .expect("WebSocket upgrade on /api/ws/events must succeed");

    drop(ws);

    shutdown.notify_one();
    handle.stop().await;
}
