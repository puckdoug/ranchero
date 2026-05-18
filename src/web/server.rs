// SPDX-License-Identifier: AGPL-3.0-only

use std::net::SocketAddr;
use std::sync::Arc;

use actix_web::{web, App, HttpServer};
use tokio::sync::Notify;

use crate::config::ResolvedConfig;
use super::state::WebState;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum WebError {
    Bind(std::io::Error),
}

impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebError::Bind(e) => write!(f, "web server bind error: {e}"),
        }
    }
}

impl std::error::Error for WebError {}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Returned by [`start`]. Gives the caller the bound address and a way
/// to stop the server.
pub struct WebServerHandle {
    addr:  SocketAddr,
    inner: actix_web::dev::ServerHandle,
}

impl WebServerHandle {
    /// The address the server is actually listening on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop the server gracefully and wait for it to exit.
    pub async fn stop(self) {
        self.inner.stop(true).await;
    }
}

// ---------------------------------------------------------------------------
// start
// ---------------------------------------------------------------------------

/// Bind the HTTP server and spawn it on the current tokio runtime.
///
/// Binding is synchronous; the function returns once the port is open.
/// The spawned server task runs until the `shutdown` Notify fires or
/// `WebServerHandle::stop` is called directly — whichever comes first.
///
/// Using port `0` in `cfg.server_port` lets the OS assign a free port;
/// `WebServerHandle::local_addr()` reports the actual address.
pub async fn start(
    cfg:      &ResolvedConfig,
    state:    Arc<WebState>,
    shutdown: Arc<Notify>,
) -> Result<WebServerHandle, WebError> {
    let bind_addr = format!("{}:{}", cfg.server_bind, cfg.server_port);

    let state_data = web::Data::new(state);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(state_data.clone())
    })
    .bind(&bind_addr)
    .map_err(WebError::Bind)?;

    let addr = server
        .addrs()
        .into_iter()
        .next()
        .ok_or_else(|| WebError::Bind(
            std::io::Error::other("server reported no bound addresses")
        ))?;

    let srv     = server.run();
    let handle  = srv.handle();

    // Watch for the shutdown signal and stop the server.
    let watch = handle.clone();
    tokio::spawn(async move {
        shutdown.notified().await;
        watch.stop(true).await;
    });

    tokio::spawn(srv);

    tracing::info!(bind = %addr, "web server started");

    Ok(WebServerHandle { addr, inner: handle })
}
