// SPDX-License-Identifier: AGPL-3.0-only
//! 17.1-T — `WebServer` binds to a host:port pair and a separate
//! client can open a TCP connection to it.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.1-T.
//!
//! This test is `#[ignore]` because it opens a real loopback socket.
//! Run it with `cargo test -- --ignored` or by name.

use std::net::TcpStream;
use std::sync::Arc;

use ranchero::web::{start, WebState};
use tokio::sync::Notify;

#[tokio::test]
#[ignore = "slow: binds a real socket"]
async fn web_server_binds_and_accepts_tcp_connection() {
    let cfg = super::common::test_config("web-bind");
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
