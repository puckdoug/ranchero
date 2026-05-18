// SPDX-License-Identifier: AGPL-3.0-only
//! 17.1-T — `WebServer` binds to a host:port pair and a separate
//! client can open a TCP connection to it.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.1-T.
//!
//! This test is `#[ignore]` because it opens a real loopback socket.
//! Run it with `cargo test -- --ignored` or by name.

use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;

use ranchero::config::{EditingMode, ResolvedConfig, ZwiftEndpoints};
use ranchero::web::{start, WebState};
use tokio::sync::Notify;

fn test_config() -> ResolvedConfig {
    ResolvedConfig {
        main_email:         None,
        main_password:      None,
        monitor_email:      None,
        monitor_password:   None,
        server_bind:        "127.0.0.1".into(),
        server_port:        0, // OS assigns a free port at bind
        server_https:       false,
        log_level:          None,
        log_file:           PathBuf::from("/tmp/ranchero-web-bind-test.log"),
        pidfile:            PathBuf::from("/tmp/ranchero-web-bind-test.pid"),
        config_path:        None,
        editing_mode:       EditingMode::Default,
        zwift_endpoints:    ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        relay_enabled:      false,
        watched_athlete_id: None,
    }
}

#[tokio::test]
#[ignore = "slow: binds a real socket"]
async fn web_server_binds_and_accepts_tcp_connection() {
    let cfg = test_config();
    let state = Arc::new(WebState::new());
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone())
        .await
        .expect("web::start must succeed on 127.0.0.1:0");

    let addr = handle.local_addr();

    // A plain TCP connect must succeed.  Routing is not exercised here.
    TcpStream::connect(addr)
        .expect("TCP connection to the bound address must succeed");

    // Ask the server to stop and wait for it to exit.
    shutdown.notify_one();
    handle.stop().await;
}
