// SPDX-License-Identifier: AGPL-3.0-only
//! 17.26-T — HTTPS is conditional on cert files being present.
//!
//! When `{cert_dir}/key.pem` and `{cert_dir}/cert.pem` both exist and are valid,
//! the server binds both an HTTP listener on `server_port` and an HTTPS listener
//! on `server_port + 1`.
//!
//! When either file is absent, only the HTTP listener exists, and a warning
//! appears in the tracing log.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.26-T.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::generate_simple_self_signed;
use tempfile::TempDir;
use tokio::sync::Notify;

use ranchero::config::{EditingMode, ResolvedConfig, ZwiftEndpoints};
use ranchero::web::{start, WebState};

fn test_config(cert_dir: &Path, port: u16) -> ResolvedConfig {
    ResolvedConfig {
        main_email:            None,
        main_password:         None,
        monitor_email:         None,
        monitor_password:      None,
        server_bind:           "127.0.0.1".into(),
        server_port:           port,
        server_https:          true,
        log_level:             None,
        log_file:              PathBuf::from("/tmp/ranchero-https-test.log"),
        pidfile:               PathBuf::from("/tmp/ranchero-https-test.pid"),
        config_path:           None,
        editing_mode:          EditingMode::Default,
        zwift_endpoints:       ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        server_pages_root:     PathBuf::from("pages"),
        server_https_cert_dir: cert_dir.to_path_buf(),
        relay_enabled:         false,
        watched_athlete_id:    None,
    }
}

fn write_self_signed_cert(cert_dir: &Path) {
    let cert = generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("rcgen must generate a self-signed cert");
    std::fs::write(cert_dir.join("cert.pem"), cert.cert.pem()).unwrap();
    std::fs::write(cert_dir.join("key.pem"),  cert.key_pair.serialize_pem()).unwrap();
}

fn port_is_listening(port: u16) -> bool {
    TcpStream::connect(format!("127.0.0.1:{port}")).is_ok()
}

#[tokio::test]
#[ignore = "slow: generates a self-signed cert and binds two sockets"]
async fn https_bound_when_certs_present() {
    let dir = TempDir::new().unwrap();
    write_self_signed_cert(dir.path());

    // Use port 0 so the OS assigns a free port; HTTPS port is http_port + 1.
    let cfg      = test_config(dir.path(), 0);
    let state    = Arc::new(WebState::new());
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone())
        .await
        .expect("server must start with valid certs");

    let http_port  = handle.local_addr().port();
    let https_port = http_port + 1;

    assert!(port_is_listening(http_port),
        "HTTP port {http_port} must be listening");
    assert!(port_is_listening(https_port),
        "HTTPS port {https_port} must be listening when certs are present");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: binds a socket"]
async fn https_not_bound_when_certs_absent() {
    let dir = TempDir::new().unwrap(); // empty — no cert files

    // Use port 0; server must still start (HTTP-only).
    let cfg      = test_config(dir.path(), 0);
    let state    = Arc::new(WebState::new());
    let shutdown = Arc::new(Notify::new());

    let handle = start(&cfg, state, shutdown.clone())
        .await
        .expect("server must start without certs (HTTP-only mode)");

    let http_port  = handle.local_addr().port();
    let https_port = http_port + 1;

    assert!(port_is_listening(http_port),
        "HTTP port {http_port} must be listening");
    assert!(!port_is_listening(https_port),
        "HTTPS port {https_port} must NOT be listening when certs are absent");

    shutdown.notify_one();
    handle.stop().await;
}
