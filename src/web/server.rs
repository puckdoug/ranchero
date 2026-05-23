// SPDX-License-Identifier: AGPL-3.0-only

use std::net::SocketAddr;
use std::sync::Arc;

use std::time::Duration;

use actix_web::{web, App, HttpServer};
use tokio::sync::Notify;

use crate::config::ResolvedConfig;
use super::http::{configure_api, configure_static};
use super::state::{WebState, gc_tick_loop};
use super::GC_TICK_INTERVAL_SECS;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum WebError {
    Bind(std::io::Error),
    Tls(String),
}

impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebError::Bind(e) => write!(f, "web server bind error: {e}"),
            WebError::Tls(e)  => write!(f, "web server TLS error: {e}"),
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
    addr:       SocketAddr,
    https_addr: Option<SocketAddr>,
    inner:      actix_web::dev::ServerHandle,
}

impl WebServerHandle {
    /// The HTTP address the server is actually listening on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// The HTTPS address, if TLS was successfully configured and bound.
    pub fn https_addr(&self) -> Option<SocketAddr> {
        self.https_addr
    }

    /// Stop the server gracefully and wait for it to exit.
    pub async fn stop(self) {
        self.inner.stop(true).await;
    }
}

// ---------------------------------------------------------------------------
// TLS helpers
// ---------------------------------------------------------------------------

enum TlsState {
    /// Both cert and key were present and loaded successfully.
    Ready(rustls::ServerConfig),
    /// One or both cert files were absent — fall back to HTTP-only.
    Missing,
}

fn load_tls(cert_dir: &std::path::Path) -> Result<TlsState, WebError> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls_pemfile::{certs, private_key};

    let cert_path = cert_dir.join("cert.pem");
    let key_path  = cert_dir.join("key.pem");

    if !cert_path.exists() || !key_path.exists() {
        return Ok(TlsState::Missing);
    }

    let cert_bytes = std::fs::read(&cert_path)
        .map_err(|e| WebError::Tls(format!("reading {}: {e}", cert_path.display())))?;
    let key_bytes  = std::fs::read(&key_path)
        .map_err(|e| WebError::Tls(format!("reading {}: {e}", key_path.display())))?;

    let cert_chain: Vec<CertificateDer<'static>> = certs(&mut cert_bytes.as_ref())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WebError::Tls(format!("parsing cert.pem: {e}")))?;

    if cert_chain.is_empty() {
        return Err(WebError::Tls("cert.pem contains no certificates".into()));
    }

    let private_key: PrivateKeyDer<'static> = private_key(&mut key_bytes.as_ref())
        .map_err(|e| WebError::Tls(format!("parsing key.pem: {e}")))?
        .ok_or_else(|| WebError::Tls("key.pem contains no private key".into()))?;

    let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(rustls::DEFAULT_VERSIONS)
        .map_err(|e| WebError::Tls(format!("selecting TLS versions: {e}")))?
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(|e| WebError::Tls(format!("building TLS config: {e}")))?;

    Ok(TlsState::Ready(config))
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
///
/// When `cfg.server_https` is true and both `cert.pem` and `key.pem` exist
/// in `cfg.server_https_cert_dir`, a second HTTPS listener is bound on
/// `server_port + 1`. If either file is absent a warning is logged and the
/// server starts HTTP-only. If the files exist but fail to parse, the
/// function returns an error.
pub async fn start(
    cfg:      &ResolvedConfig,
    state:    Arc<WebState>,
    shutdown: Arc<Notify>,
) -> Result<WebServerHandle, WebError> {
    let bind_addr  = format!("{}:{}", cfg.server_bind, cfg.server_port);
    let pages_root = cfg.server_pages_root.clone();

    let gc_state   = Arc::clone(&state);
    let state_data = web::Data::new(state);

    let builder = HttpServer::new(move || {
        App::new()
            .app_data(state_data.clone())
            .configure(configure_static(pages_root.clone()))
            .configure(configure_api)
    })
    .bind(&bind_addr)
    .map_err(WebError::Bind)?;

    // Use the port the OS assigned for HTTP (handles server_port = 0).
    let http_port = builder
        .addrs()
        .into_iter()
        .next()
        .map(|a| a.port())
        .unwrap_or(cfg.server_port);

    // Conditionally bind a TLS listener.  When the HTTP port was OS-assigned
    // (server_port = 0), also request an OS-assigned HTTPS port to avoid the
    // http_port + 1 collision that can occur under parallel test load.
    let (server, bound_https_addr) = if cfg.server_https {
        match load_tls(&cfg.server_https_cert_dir)? {
            TlsState::Ready(tls_config) => {
                let https_port = if cfg.server_port == 0 { 0 } else { http_port + 1 };
                let https_bind = format!("{}:{}", cfg.server_bind, https_port);
                tracing::info!(bind = %https_bind, "web server HTTPS listener starting");
                let s = builder
                    .bind_rustls_0_23(&https_bind, tls_config)
                    .map_err(WebError::Bind)?;
                // The HTTPS address is the bound address whose port differs from http_port.
                let https_addr = s.addrs().into_iter().find(|a| a.port() != http_port);
                (s, https_addr)
            }
            TlsState::Missing => {
                tracing::warn!(
                    cert_dir = %cfg.server_https_cert_dir.display(),
                    "HTTPS requested but cert.pem or key.pem is absent; running HTTP-only"
                );
                (builder, None)
            }
        }
    } else {
        (builder, None)
    };

    let addr = server
        .addrs()
        .into_iter()
        .next()
        .ok_or_else(|| WebError::Bind(
            std::io::Error::other("server reported no bound addresses")
        ))?;

    let srv    = server.run();
    let handle = srv.handle();

    // Watch for the shutdown signal and stop the server.
    let watch = handle.clone();
    tokio::spawn(async move {
        shutdown.notified().await;
        watch.stop(true).await;
    });

    tokio::spawn(srv);
    tokio::spawn(gc_tick_loop(gc_state, Duration::from_secs_f64(GC_TICK_INTERVAL_SECS)));

    tracing::info!(bind = %addr, "web server started");

    Ok(WebServerHandle { addr, https_addr: bound_https_addr, inner: handle })
}
