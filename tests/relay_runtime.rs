//! STEP-12.1 — Integration tests for the relay runtime
//! orchestrator.
//!
//! These tests exercise `RelayRuntime` against a `wiremock`-backed
//! HTTPS surface for auth and relay-session login, plus a fake TCP
//! server on a localhost ephemeral port for the channel. The tests
//! are present during the red phase as skeletons; each test panics
//! because `RelayRuntime::start` is not yet implemented. The
//! comments inside each test describe the assertions the
//! implementation must produce.
//!
//! See `docs/plans/STEP-12.1-tcp-end-to-end-smoke.md` for the
//! detailed contract.

use std::path::PathBuf;

use ranchero::config::{EditingMode, LogLevel, RedactedString, ResolvedConfig};
use ranchero::daemon::relay::RelayRuntime;

fn make_config(email: &str, password: &str) -> ResolvedConfig {
    ResolvedConfig {
        main_email: Some(email.to_string()),
        main_password: Some(RedactedString::new(password.to_string())),
        monitor_email: None,
        monitor_password: None,
        server_bind: "127.0.0.1".into(),
        server_port: 1080,
        server_https: false,
        log_level: LogLevel::Info,
        log_file: PathBuf::from("/tmp/ranchero-it.log"),
        pidfile: PathBuf::from("/tmp/ranchero-it.pid"),
        config_path: None,
        editing_mode: EditingMode::Default,
    }
}

#[tokio::test]
async fn runtime_writes_capture_file_for_inbound_packets() {
    // STEP-12.1 fully-wired contract:
    //
    // 1. Stand up `wiremock` for
    //    `/auth/realms/zwift/protocol/openid-connect/token` and for
    //    `/api/users/login`.
    // 2. Open a localhost TCP listener on an ephemeral port and
    //    spawn a handler that, after accepting the client, emits a
    //    single encrypted `ServerToClient` frame.
    // 3. Configure `RelayRuntime::start` to point at the wiremock
    //    base URLs and the ephemeral TCP port; pass a capture path.
    // 4. Wait for one `Inbound` event to fire, then call
    //    `runtime.shutdown()` and `runtime.join().await`.
    // 5. Open the capture file with `CaptureReader` and assert
    //    exactly one inbound TCP record.
    //
    // The assertions above require the `RelayRuntime` API to expose
    // an injection point for the auth and session base URLs and for
    // the TCP transport factory. Until the implementation lands,
    // this skeleton calls `RelayRuntime::start` and panics on
    // `unimplemented!()`.
    let cfg = make_config("rider@example.com", "secret");
    let _ = RelayRuntime::start(
        &cfg,
        Some(PathBuf::from("/tmp/ranchero-it-capture.cap")),
    )
    .await;
    panic!(
        "STEP-12.1 red state: integration test cannot be exercised \
         until RelayRuntime exposes auth/session/TCP injection points",
    );
}

#[tokio::test]
async fn runtime_logs_login_and_established_at_info() {
    // STEP-12.1 fully-wired contract: with the same setup as
    // `runtime_writes_capture_file_for_inbound_packets`, install a
    // tracing subscriber that records emitted events. After
    // `RelayRuntime::start` returns, assert that one
    // `relay.login.ok` record at INFO and one
    // `relay.tcp.established` record at INFO have been emitted.
    let cfg = make_config("rider@example.com", "secret");
    let _ = RelayRuntime::start(&cfg, None).await;
    panic!(
        "STEP-12.1 red state: integration test cannot be exercised \
         until RelayRuntime emits the documented tracing records",
    );
}
