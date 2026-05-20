// SPDX-License-Identifier: AGPL-3.0-only
//! 17.15-T — a WebSocket client completes the upgrade on /api/ws/events.
//!
//! Fails at runtime (not compile time) until `configure_api` registers
//! the /api/ws/events route.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.15-T.

use std::path::PathBuf;
use std::sync::Arc;

use ranchero::config::{EditingMode, ResolvedConfig, ZwiftEndpoints};
use ranchero::web::{start, RpcRegistry, WebState};
use tokio::sync::Notify;
use tokio_tungstenite::connect_async;

fn test_config() -> ResolvedConfig {
    ResolvedConfig {
        main_email:            None,
        main_password:         None,
        monitor_email:         None,
        monitor_password:      None,
        server_bind:           "127.0.0.1".into(),
        server_port:           0,
        server_https:          false,
        log_level:             None,
        log_file:              PathBuf::from("/tmp/ranchero-ws-handshake-test.log"),
        pidfile:               PathBuf::from("/tmp/ranchero-ws-handshake-test.pid"),
        config_path:           None,
        editing_mode:          EditingMode::Default,
        zwift_endpoints:       ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        relay_enabled:         false,
        watched_athlete_id:    None,
        server_pages_root:     PathBuf::from("pages"),
        server_https_cert_dir: PathBuf::from("https"),
        event_behavior:        Default::default(),
    }
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_handshake_completes_on_api_ws_events() {
    let cfg   = test_config();
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
