//! STEP-12.1 — Integration tests for the relay runtime
//! orchestrator.
//!
//! These tests exercise `RelayRuntime` against locally-defined
//! stub dependency-injection types. They sit in `tests/` rather
//! than alongside the unit tests so that they exercise the
//! crate's public surface only — `#[cfg(test)]` items defined
//! inside `src/daemon/relay.rs` are not accessible here.
//!
//! See `docs/plans/STEP-12-game-monitor.md`, sub-step 12.1.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use ranchero::config::{
    EditingMode, RedactedString, ResolvedConfig, ZwiftEndpoints,
};
use ranchero::daemon::relay::{
    AuthLogin, DefaultUdpTransportFactory, RelayRuntime, SessionLogin,
    SessionSupervisorFactory, SessionSupervisorHandle, TcpTransportFactory,
    UdpTransportFactory,
};

fn make_config(email: &str, password: &str) -> ResolvedConfig {
    ResolvedConfig {
        main_email: None,
        main_password: None,
        monitor_email: Some(email.to_string()),
        monitor_password: Some(RedactedString::new(password.to_string())),
        server_bind: "127.0.0.1".into(),
        server_port: 1080,
        server_https: false,
        log_level: None,
        log_file: PathBuf::from("/tmp/ranchero-it.log"),
        pidfile: PathBuf::from("/tmp/ranchero-it.pid"),
        config_path: None,
        editing_mode: EditingMode::Default,
        // These tests use `start_with_deps` with stubs that never
        // reach the network; the endpoint values are unused but
        // pinned to an unroutable address as a defence in depth.
        zwift_endpoints: ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        relay_enabled: true,
        watched_athlete_id: None,
    }
}

// --- local stub DI types ------------------------------------------

struct StubAuth;

/// Records the email address passed to `login` for assertion in Defect 11 tests.
struct RecordingAuth {
    called_with_email: Arc<StdMutex<Option<String>>>,
}

impl RecordingAuth {
    fn new() -> (Self, Arc<StdMutex<Option<String>>>) {
        let called_with_email = Arc::new(StdMutex::new(None));
        (Self { called_with_email: Arc::clone(&called_with_email) }, called_with_email)
    }
}

impl AuthLogin for RecordingAuth {
    async fn login(&self, email: &str, _password: &str) -> Result<(), zwift_api::Error> {
        *self.called_with_email.lock().unwrap() = Some(email.to_string());
        Ok(())
    }

    async fn athlete_id(&self) -> Result<i64, zwift_api::Error> {
        Ok(12345)
    }
}

/// Returns a fixed athlete ID from `athlete_id()` for Defect 12 tests.
struct KnownIdAuth {
    id: i64,
}

impl AuthLogin for KnownIdAuth {
    async fn login(&self, _email: &str, _password: &str) -> Result<(), zwift_api::Error> {
        Ok(())
    }

    async fn athlete_id(&self) -> Result<i64, zwift_api::Error> {
        Ok(self.id)
    }
}

impl AuthLogin for StubAuth {
    async fn login(
        &self,
        _email: &str,
        _password: &str,
    ) -> Result<(), zwift_api::Error> {
        Ok(())
    }

    async fn athlete_id(&self) -> Result<i64, zwift_api::Error> {
        Ok(12345)
    }
}

struct StubSession {
    session: StdMutex<Option<zwift_relay::RelaySession>>,
}

impl StubSession {
    fn new(session: zwift_relay::RelaySession) -> Self {
        Self {
            session: StdMutex::new(Some(session)),
        }
    }
}

impl SessionLogin for StubSession {
    fn login(
        &self,
    ) -> impl std::future::Future<
        Output = Result<zwift_relay::RelaySession, zwift_relay::SessionError>,
    > + Send {
        let result = self
            .session
            .lock()
            .unwrap()
            .take()
            .expect("StubSession::login called more than once");
        async move { Ok(result) }
    }
}

/// A no-op TCP transport that lets the channel come up without
/// going through the kernel. `write_all` records bytes for
/// inspection but otherwise does nothing; `read_chunk` blocks
/// until the test drops the runtime.
struct NoopTcpTransport;

impl zwift_relay::TcpTransport for NoopTcpTransport {
    async fn write_all(&self, _bytes: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
        std::future::pending::<()>().await;
        unreachable!()
    }
}

struct StubTcpFactory {
    transport: StdMutex<Option<NoopTcpTransport>>,
}

impl StubTcpFactory {
    fn new() -> Self {
        Self {
            transport: StdMutex::new(Some(NoopTcpTransport)),
        }
    }
}

impl TcpTransportFactory for StubTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        let transport = self.transport.lock().unwrap().take();
        async move {
            transport.ok_or_else(|| std::io::Error::other("StubTcpFactory: no transport"))
        }
    }
}

fn fixture_session() -> zwift_relay::RelaySession {
    zwift_relay::RelaySession {
        aes_key: [0u8; 16],
        relay_id: 42,
        tcp_servers: vec![zwift_relay::TcpServer { ip: "127.0.0.1".into() }],
        expires_at: tokio::time::Instant::now() + std::time::Duration::from_secs(3600),
        server_time_ms: Some(0),
    }
}

// --- tests --------------------------------------------------------

#[tokio::test]
async fn runtime_writes_capture_file_for_inbound_packets() {
    // Open a capture writer the test holds an `Arc` clone of, push
    // a synthetic inbound record before bringing up the runtime,
    // start the runtime with the same writer, then shut down. The
    // resulting file must contain exactly one record.
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    writer.record(zwift_relay::capture::CaptureRecord {
        ts_unix_ns: 1_700_000_000_000_000_000,
        direction: zwift_relay::capture::Direction::Inbound,
        transport: zwift_relay::capture::TransportKind::Tcp,
        hello: false,
        payload: vec![1, 2, 3, 4],
    });

    let cfg = make_config("rider@example.com", "secret");
    let runtime = RelayRuntime::start_with_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth,
        StubSession::new(fixture_session()),
        StubTcpFactory::new(),
    )
    .await
    .expect("start_with_deps_and_writer must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    drop(writer);
    let reader =
        zwift_relay::capture::CaptureReader::open(path.path()).expect("reader");
    let count = reader.count();
    assert_eq!(count, 1, "shutdown must drain the accepted record");
}

#[tokio::test]
#[tracing_test::traced_test]
async fn runtime_logs_login_and_established_at_info() {
    let cfg = make_config("rider@example.com", "secret");
    let runtime = RelayRuntime::start_with_deps(
        &cfg,
        None,
        StubAuth,
        StubSession::new(fixture_session()),
        StubTcpFactory::new(),
    )
    .await
    .expect("start_with_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.login.ok"),
        "expected a `relay.login.ok` record at INFO",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.tcp.established"),
        "expected a `relay.tcp.established` record at INFO",
    );
}

// ==========================================================================
// Defect 3–7 infrastructure: additional stub DI types.
// ==========================================================================

// --- SessionSupervisorFactory stubs (Defect 7) ----------------------------

/// A stub [`SessionSupervisorHandle`] that returns a pre-loaded
/// `RelaySession` from `current()` and emits any pre-seeded events
/// from `subscribe_events()`.
struct StubSupervisorHandle {
    session: zwift_relay::RelaySession,
    events_tx: tokio::sync::broadcast::Sender<zwift_relay::SessionEvent>,
}

impl StubSupervisorHandle {
    fn with_events(
        session: zwift_relay::RelaySession,
        events_tx: tokio::sync::broadcast::Sender<zwift_relay::SessionEvent>,
    ) -> Self {
        Self { session, events_tx }
    }
}

impl SessionSupervisorHandle for StubSupervisorHandle {
    fn current(
        &self,
    ) -> impl std::future::Future<Output = zwift_relay::RelaySession> + Send {
        let s = self.session.clone();
        async move { s }
    }

    fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<zwift_relay::SessionEvent> {
        self.events_tx.subscribe()
    }

    fn shutdown(&self) {}
}

struct StubSupervisorFactory {
    session: zwift_relay::RelaySession,
    events_tx: tokio::sync::broadcast::Sender<zwift_relay::SessionEvent>,
}

impl StubSupervisorFactory {
    fn new(session: zwift_relay::RelaySession) -> Self {
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        Self { session, events_tx }
    }

    /// Return a factory whose handle emits a pre-seeded event when
    /// the test triggers the broadcast sender.
    fn with_events_tx(
        session: zwift_relay::RelaySession,
        events_tx: tokio::sync::broadcast::Sender<zwift_relay::SessionEvent>,
    ) -> Self {
        Self { session, events_tx }
    }
}

impl SessionSupervisorFactory for StubSupervisorFactory {
    type Handle = StubSupervisorHandle;

    fn start(
        &self,
    ) -> impl std::future::Future<Output = Result<Self::Handle, ranchero::daemon::relay::RelayRuntimeError>>
           + Send {
        let session = self.session.clone();
        let events_tx = self.events_tx.clone();
        async move { Ok(StubSupervisorHandle::with_events(session, events_tx)) }
    }
}

// --- UDP transport stubs (Defects 4 and 5) --------------------------------

/// A no-op UDP transport. `send` always succeeds silently; `recv`
/// blocks forever.
struct NoopUdpTransport;

impl zwift_relay::UdpTransport for NoopUdpTransport {
    async fn send(&self, _bytes: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    async fn recv(&self) -> std::io::Result<Vec<u8>> {
        std::future::pending::<()>().await;
        unreachable!()
    }
}

struct NoopUdpFactory;

impl UdpTransportFactory for NoopUdpFactory {
    type Transport = NoopUdpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        async { Ok(NoopUdpTransport) }
    }

    fn channel_config(&self) -> zwift_relay::UdpChannelConfig {
        zwift_relay::UdpChannelConfig { max_hellos: 0, ..Default::default() }
    }
}

/// A recording UDP transport. `send` appends datagrams to a shared
/// `written` list for inspection by tests; `recv` blocks forever.
struct RecordingUdpTransport {
    written: Arc<StdMutex<Vec<Vec<u8>>>>,
}

impl zwift_relay::UdpTransport for RecordingUdpTransport {
    async fn send(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.written.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }

    async fn recv(&self) -> std::io::Result<Vec<u8>> {
        std::future::pending::<()>().await;
        unreachable!()
    }
}

/// A recording UDP factory. The first `connect` call records that it
/// was called and vends a `RecordingUdpTransport` backed by a shared
/// write log.
struct RecordingUdpFactory {
    connected: Arc<StdMutex<bool>>,
    written: Arc<StdMutex<Vec<Vec<u8>>>>,
}

impl RecordingUdpFactory {
    fn new() -> (Self, Arc<StdMutex<bool>>, Arc<StdMutex<Vec<Vec<u8>>>>) {
        let connected = Arc::new(StdMutex::new(false));
        let written = Arc::new(StdMutex::new(Vec::new()));
        (
            Self {
                connected: Arc::clone(&connected),
                written: Arc::clone(&written),
            },
            connected,
            written,
        )
    }
}

impl UdpTransportFactory for RecordingUdpFactory {
    type Transport = RecordingUdpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        *self.connected.lock().unwrap() = true;
        let written = Arc::clone(&self.written);
        async move { Ok(RecordingUdpTransport { written }) }
    }

    fn channel_config(&self) -> zwift_relay::UdpChannelConfig {
        zwift_relay::UdpChannelConfig { max_hellos: 0, ..Default::default() }
    }
}

// --- TCP recording transport (Defects 3 and 6) ----------------------------

/// A recording TCP transport. Every `write_all` call appends the
/// supplied bytes to a shared list so tests can verify outbound
/// writes. `read_chunk` blocks forever.
struct RecordingTcpTransport {
    written: Arc<StdMutex<Vec<Vec<u8>>>>,
}

impl zwift_relay::TcpTransport for RecordingTcpTransport {
    async fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.written.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }

    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
        std::future::pending::<()>().await;
        unreachable!()
    }
}

struct RecordingTcpFactory {
    written: Arc<StdMutex<Vec<Vec<u8>>>>,
}

impl RecordingTcpFactory {
    fn new() -> (Self, Arc<StdMutex<Vec<Vec<u8>>>>) {
        let written = Arc::new(StdMutex::new(Vec::new()));
        (
            Self { written: Arc::clone(&written) },
            written,
        )
    }
}

impl TcpTransportFactory for RecordingTcpFactory {
    type Transport = RecordingTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        let written = Arc::clone(&self.written);
        async move { Ok(RecordingTcpTransport { written }) }
    }
}

// ==========================================================================
// Defect 6 — TcpChannel handle inaccessible after start.
//
// Red state: `RelayRuntime::send_tcp` is a stub that always returns
// `Ok(())` without writing anything through the underlying transport.
// The test fails because `written` remains empty.
// ==========================================================================

#[tokio::test]
async fn relay_runtime_exposes_outbound_tcp_send_path_after_start() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, written) = RecordingTcpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        tcp_factory,
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    let payload = zwift_proto::ClientToServer {
        seqno: Some(1),
        ..Default::default()
    };
    runtime
        .send_tcp(payload, false)
        .await
        .expect("send_tcp must not error");

    runtime.shutdown();
    let _ = runtime.join().await;

    let writes = written.lock().unwrap();
    assert!(
        !writes.is_empty(),
        "Defect 6 red state: send_tcp must forward bytes to the \
         underlying TCP transport; no writes were recorded",
    );
}

// ==========================================================================
// Defect 3 — TCP hello `ClientToServer` never sent.
//
// Red state: after `start_with_all_deps` returns, no hello packet has
// been written to the transport. The test fails because `written` is
// empty.
// ==========================================================================

#[tokio::test]
async fn relay_runtime_sends_tcp_hello_after_established() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, written) = RecordingTcpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        tcp_factory,
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    let writes = written.lock().unwrap();
    assert!(
        !writes.is_empty(),
        "Defect 3 red state: start_with_all_deps must write a TCP hello \
         packet to the transport after the channel is established; \
         no writes were recorded",
    );
}

// ==========================================================================
// Defect 4 — No UDP channel constructed in production.
//
// Red state: `start_with_all_deps` accepts a `UdpTransportFactory` but
// does not yet call `connect()` on it. Both assertions below fail.
// ==========================================================================

#[tokio::test]
async fn relay_runtime_connects_udp_transport_after_tcp_hello() {
    let cfg = make_config("rider@example.com", "secret");
    let (udp_factory, connected, _written) = RecordingUdpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        *connected.lock().unwrap(),
        "Defect 4 red state: start_with_all_deps must call \
         UdpTransportFactory::connect after TCP is established; \
         factory was never called",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn relay_runtime_logs_udp_established_at_info() {
    let cfg = make_config("rider@example.com", "secret");
    let (udp_factory, _connected, _written) = RecordingUdpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.established"),
        "Defect 4 red state: expected a `relay.udp.established` record \
         at INFO after UDP channel comes up",
    );
}

// ==========================================================================
// Defect 5 — 1 Hz HeartbeatScheduler never spawned.
//
// Red state: `start_all_inner` returns without spawning the scheduler,
// so no `relay.heartbeat.started` record is ever emitted.
// ==========================================================================

#[tokio::test]
#[tracing_test::traced_test]
async fn relay_runtime_spawns_heartbeat_after_udp_established() {
    let cfg = make_config("rider@example.com", "secret");

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.heartbeat.started"),
        "Defect 5 red state: expected relay.heartbeat.started after \
         UDP channel comes up; heartbeat scheduler was never spawned",
    );
}

// ==========================================================================
// Defect 7 — RelaySessionSupervisor never started.
//
// Red state: `start_all_inner` calls `sf.start()` to get the initial
// session but does not subscribe to the supervisor's event broadcast.
// Tests that assert tracing records for session events fail because the
// records are never emitted.
// ==========================================================================

#[tokio::test]
#[tracing_test::traced_test]
async fn relay_runtime_logs_session_logged_in_at_info() {
    let cfg = make_config("rider@example.com", "secret");
    let (events_tx, _events_rx) = tokio::sync::broadcast::channel::<zwift_relay::SessionEvent>(16);
    let factory = StubSupervisorFactory::with_events_tx(fixture_session(), events_tx.clone());

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        factory,
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    // The stub supervisor emits `LoggedIn` on the shared channel.
    // The runtime should subscribe and emit a tracing record.
    let _ = events_tx.send(zwift_relay::SessionEvent::LoggedIn(fixture_session()));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.session.logged_in"),
        "Defect 7 red state: expected a `relay.session.logged_in` record \
         after a LoggedIn event; the runtime must subscribe to the \
         supervisor's event broadcast",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn relay_runtime_logs_session_refreshed_at_info() {
    let cfg = make_config("rider@example.com", "secret");
    let (events_tx, _events_rx) = tokio::sync::broadcast::channel::<zwift_relay::SessionEvent>(16);
    let factory = StubSupervisorFactory::with_events_tx(fixture_session(), events_tx.clone());

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        factory,
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    let new_expires_at =
        tokio::time::Instant::now() + std::time::Duration::from_secs(3600);
    let _ = events_tx.send(zwift_relay::SessionEvent::Refreshed {
        relay_id: 42,
        new_expires_at,
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.session.refreshed"),
        "Defect 7 red state: expected a `relay.session.refreshed` record \
         after a Refreshed event; the runtime must subscribe to the \
         supervisor's event broadcast",
    );
}

// ==========================================================================
// Defect 11 — Relay authenticates as the wrong account.
//
// Red state: both start_inner sites pass cfg.main_email / cfg.main_password
// to AuthLogin and SessionLogin. Monitor credentials are resolved and then
// silently discarded, so every live invocation impersonates the rider's
// own game session.
// ==========================================================================

#[tokio::test]
async fn relay_runtime_authenticates_as_monitor_account() {
    // Both main and monitor credentials are present. The relay must use the
    // monitor account for the AuthLogin call, not the main account.
    let mut cfg = make_config("main@example.com", "main-pass");
    cfg.monitor_email    = Some("monitor@example.com".to_string());
    cfg.monitor_password = Some(RedactedString::new("monitor-pass".to_string()));

    let (auth, called_with_email) = RecordingAuth::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        auth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    let email = called_with_email
        .lock()
        .unwrap()
        .take()
        .expect("AuthLogin::login was never called");
    assert_eq!(
        email, "monitor@example.com",
        "Defect 11 red state: relay must authenticate as the monitor account; \
         was called with {email:?} instead",
    );
}

#[tokio::test]
async fn relay_runtime_start_fails_when_monitor_credentials_absent() {
    // Main credentials are set; monitor credentials are absent.
    // After the fix, the runtime must reject this configuration rather than
    // proceeding with the main account.
    let mut cfg = make_config("main@example.com", "main-pass");
    cfg.monitor_email    = None;
    cfg.monitor_password = None;
    cfg.main_email       = Some("main@example.com".to_string());
    cfg.main_password    = Some(RedactedString::new("main-pass".to_string()));

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await;

    assert!(
        result.is_err(),
        "Defect 11 red state: relay must fail to start when monitor credentials \
         are absent; currently succeeds by falling back to the main account",
    );
}

// ==========================================================================
// Item 1 (STEP-12.10) — TCP relay port must be 3025 regardless of what the
// LoginResponse proto field carries.
//
// Red state: relay.rs reads `server.port` from the `TcpServer` struct, so the
// connect address inherits whatever value the session decoder placed there.
// The proto value today is 3023; sauce hard-codes 3025.  The connect must use
// the constant.
// ==========================================================================

/// A [`TcpTransportFactory`] that records the [`SocketAddr`] passed to the
/// first `connect()` call, then hands back a [`NoopTcpTransport`].
struct AddrCapturingTcpFactory {
    captured: Arc<StdMutex<Option<std::net::SocketAddr>>>,
}

impl AddrCapturingTcpFactory {
    fn new() -> (Self, Arc<StdMutex<Option<std::net::SocketAddr>>>) {
        let slot = Arc::new(StdMutex::new(None));
        (Self { captured: Arc::clone(&slot) }, slot)
    }
}

impl TcpTransportFactory for AddrCapturingTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        *self.captured.lock().unwrap() = Some(addr);
        async { Ok(NoopTcpTransport) }
    }
}

#[tokio::test]
async fn tcp_connect_uses_constant_port_not_proto_field() {
    // The proto `TcpAddress.port` field is not the listener port — sauce
    // hard-codes 3025 (`zwift.mjs:1212`). Verify that the connect address
    // always uses `TCP_PORT_SECURE` regardless of what the session decoder
    // found in the proto response.
    let session = zwift_relay::RelaySession {
        aes_key: [0u8; 16],
        relay_id: 42,
        tcp_servers: vec![zwift_relay::TcpServer { ip: "127.0.0.1".into() }],
        expires_at: tokio::time::Instant::now() + std::time::Duration::from_secs(3600),
        server_time_ms: Some(0),
    };

    let cfg = make_config("monitor@example.com", "monitor-pass");
    let (factory, captured) = AddrCapturingTcpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(session),
        factory,
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    let addr = captured
        .lock()
        .unwrap()
        .expect("TcpTransportFactory::connect was never called");

    assert_eq!(
        addr.port(),
        zwift_relay::TCP_PORT_SECURE,
        "TCP connect must use TCP_PORT_SECURE ({}), got port {}",
        zwift_relay::TCP_PORT_SECURE,
        addr.port(),
    );
}

// ==========================================================================
// Defect 12 — athlete_id hardcoded to 0 in TcpChannelConfig, UdpChannelConfig,
// and HeartbeatScheduler.
//
// Red state: start_all_inner does not call auth.athlete_id(); the monitor
// account's profile ID is never retrieved and therefore never appears in
// log records or outbound packets.
// ==========================================================================

#[tokio::test]
#[tracing_test::traced_test]
async fn relay_runtime_logs_monitor_athlete_id_after_login() {
    // KnownIdAuth returns 99_999 from athlete_id(). After the fix, the runtime
    // must call athlete_id(), log the value, and forward it to the channel
    // configs and heartbeat scheduler.
    let mut cfg = make_config("main@example.com", "main-pass");
    cfg.monitor_email    = Some("monitor@example.com".to_string());
    cfg.monitor_password = Some(RedactedString::new("monitor-pass".to_string()));

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        KnownIdAuth { id: 99_999 },
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "99999"),
        "Defect 12 red state: relay must retrieve and log the monitor account's \
         athlete ID after login; athlete_id 99999 was not found in any log record",
    );
}

// ==========================================================================
// STEP-12.11 Item 1 — DefaultUdpTransportFactory connects to a real UDP socket.
//
// Red state: DefaultUdpTransportFactory::connect returns the stub error
// "Defect 4: UDP connection not yet implemented".
// ==========================================================================

#[tokio::test]
async fn default_udp_transport_factory_connects_to_bound_socket() {
    // Bind a local UDP socket to 127.0.0.1:0 to get an OS-assigned port.
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind UDP socket");
    let addr = socket.local_addr()
        .expect("get local address");

    // Call the production factory's connect method.
    let factory = DefaultUdpTransportFactory;
    let result = factory.connect(addr).await;

    if let Err(e) = &result {
        panic!(
            "STEP-12.11 Item 1 red state: DefaultUdpTransportFactory::connect must \
             connect to a real UDP socket; currently fails with: {}",
            e,
        );
    }
    assert!(result.is_ok());
}

// ==========================================================================
// STEP-12.11 Item 2 — The full relay pipeline emits all lifecycle events.
//
// Red state: start_all_inner (called by start_with_all_deps_and_writer)
// is incomplete; it does not emit relay.tcp.hello.sent, relay.udp.established,
// or relay.heartbeat.started.
//
// This test uses the full DI pipeline (start_with_all_deps_and_writer) to
// verify that when all components are wired correctly, the complete event
// sequence is emitted. The production daemon entry point (start_with_writer)
// must eventually route through this same pipeline.
// ==========================================================================

#[tokio::test]
#[tracing_test::traced_test]
async fn start_with_all_deps_and_writer_emits_full_lifecycle_event_sequence() {
    // This test verifies that when RelayRuntime is started with the full
    // dependency stack and a capture writer, it emits the complete event
    // sequence. When Item 2 is implemented, RelayRuntime::start_with_writer
    // (the production entry point) must route through the same pipeline.
    let cfg = make_config("monitor@example.com", "monitor-pass");

    // Create a capture writer to pass along.
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    let runtime = RelayRuntime::start_with_all_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps_and_writer must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    // Verify the full event sequence is emitted.
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.tcp.hello.sent"),
        "STEP-12.11 Item 2 red state: full pipeline must emit \
         relay.tcp.hello.sent after TCP is established; not found in tracing log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.established"),
        "STEP-12.11 Item 2 red state: full pipeline must emit \
         relay.udp.established after UDP connect; not found in tracing log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.heartbeat.started"),
        "STEP-12.11 Item 2 red state: full pipeline must emit \
         relay.heartbeat.started after UDP is ready; not found in tracing log",
    );
}

// ==========================================================================
// STEP-12.12 Phase 6a — daemon-level wiring of capture, recv_loop, state,
// and heartbeat tracing. Each test pins one strand of behaviour the
// daemon must own (since none of the per-crate phases own it).
// ==========================================================================

use ranchero::daemon::relay::{HeartbeatScheduler, HeartbeatSink};
use zwift_relay::WorldTimer;

#[tokio::test]
async fn start_all_inner_writes_session_manifest_after_session_login() {
    // Drive the full DI pipeline with a capture writer attached. The
    // first non-header item in the resulting file must be a Manifest
    // record carrying the AES key and relay_id from the fixture
    // session, proving start_all_inner calls record_session_manifest
    // immediately after login.
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    let session = fixture_session();
    let expected_aes_key = session.aes_key;
    let expected_relay_id = session.relay_id;

    let runtime = RelayRuntime::start_with_all_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth,
        StubSupervisorFactory::new(session),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps_and_writer must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;
    drop(writer);

    let mut reader = zwift_relay::capture::CaptureReader::open(path.path())
        .expect("open reader");
    let first_item = reader
        .next_item()
        .expect("at least one item")
        .expect("decode ok");
    match first_item {
        zwift_relay::capture::CaptureItem::Manifest(m) => {
            assert_eq!(
                m.aes_key, expected_aes_key,
                "STEP-12.12 Phase 6a: manifest must carry the live session AES key",
            );
            assert_eq!(
                m.relay_id, expected_relay_id,
                "STEP-12.12 Phase 6a: manifest must carry the live session relay_id",
            );
        }
        other => panic!(
            "STEP-12.12 Phase 6a: first capture item must be a Manifest record \
             (start_all_inner must call record_session_manifest after login); \
             got {other:?}",
        ),
    }
}

#[tokio::test]
async fn supervisor_refresh_writes_fresh_manifest_when_key_rotates() {
    // Drive the runtime with a capture writer and an injectable
    // supervisor event channel. After the initial manifest is written,
    // broadcast a Refreshed event with new key material; the
    // supervisor-event subscriber must call record_session_manifest
    // again, producing a second Manifest item in the file.
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    let (supervisor_events_tx, _) = tokio::sync::broadcast::channel(16);
    let factory = StubSupervisorFactory::with_events_tx(
        fixture_session(),
        supervisor_events_tx.clone(),
    );

    let runtime = RelayRuntime::start_with_all_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth,
        factory,
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps_and_writer must succeed");

    // Kick a supervisor refresh so the daemon emits a fresh manifest.
    let _ = supervisor_events_tx.send(zwift_relay::SessionEvent::Refreshed {
        relay_id: 999,
        new_expires_at: tokio::time::Instant::now()
            + std::time::Duration::from_secs(7200),
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;
    drop(writer);

    let reader = zwift_relay::capture::CaptureReader::open(path.path())
        .expect("open reader");
    let manifest_count = reader
        .filter_map(|_| {
            // Iterator::next yields only Frames; we need next_item.
            None::<()>
        })
        .count();
    let _ = manifest_count;
    let mut reader = zwift_relay::capture::CaptureReader::open(path.path())
        .expect("open reader (2)");
    let mut manifest_count = 0;
    while let Some(item) = reader.next_item() {
        if matches!(item.expect("decode"), zwift_relay::capture::CaptureItem::Manifest(_)) {
            manifest_count += 1;
        }
    }
    assert!(
        manifest_count >= 2,
        "STEP-12.12 Phase 6a: a Refreshed supervisor event must trigger a \
         fresh record_session_manifest call (expected >= 2 manifest records, \
         got {manifest_count})",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn recv_loop_handles_tcp_inbound_and_emits_relay_tcp_message_recv() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    let runtime = RelayRuntime::start_with_all_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    let stc = zwift_proto::ServerToClient {
        seqno: Some(7),
        world_time: Some(1_700_000),
        ..Default::default()
    };
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.message.recv",
        ),
        "STEP-12.12 Phase 6a: recv_loop must emit relay.tcp.message.recv at \
         debug for every Inbound event (replacing the bare relay.tcp.inbound \
         log line); not found in tracing log",
    );
    for field in ["message_kind=", "seqno=", "has_state_change=", "has_world_info="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.12 Phase 6a: relay.tcp.message.recv must carry field \
             {field:?} — not present in any captured log line",
        );
    }
}

#[tokio::test]
#[tracing_test::traced_test]
async fn state_change_emissions_track_runtime_state_transitions() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.state.change"),
        "STEP-12.12 Phase 6a: relay.state.change must fire at info per \
         RuntimeState transition; not found in tracing log",
    );
    for field in ["from=", "to="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.12 Phase 6a: relay.state.change must carry field \
             {field:?} — not present in any captured log line",
        );
    }
}

// --- HeartbeatSink stubs for the per-tick / failure tracing tests ---

struct CountingHeartbeatSink {
    count: Arc<std::sync::atomic::AtomicUsize>,
}

impl HeartbeatSink for CountingHeartbeatSink {
    async fn send(&self, _payload: zwift_proto::ClientToServer) -> std::io::Result<()> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

struct FailingHeartbeatSink;

impl HeartbeatSink for FailingHeartbeatSink {
    async fn send(&self, _payload: zwift_proto::ClientToServer) -> std::io::Result<()> {
        Err(std::io::Error::other("simulated heartbeat failure"))
    }
}

#[tokio::test]
#[tracing_test::traced_test]
async fn heartbeat_tick_emits_debug_event_per_interval() {
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sink = CountingHeartbeatSink { count: Arc::clone(&count) };
    let scheduler = Arc::new(
        HeartbeatScheduler::new(sink, WorldTimer::new(), 12345)
            .with_interval(std::time::Duration::from_millis(30)),
    );
    let s2 = Arc::clone(&scheduler);
    let handle = tokio::spawn(async move {
        s2.run().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    handle.abort();
    let _ = handle.await;

    assert!(
        count.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "test setup must produce at least 2 heartbeats",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.heartbeat.tick"),
        "STEP-12.12 Phase 6a: relay.heartbeat.tick must fire at debug per \
         scheduler tick; not found in tracing log",
    );
    for field in ["interval_ms=", "send_ok="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.12 Phase 6a: relay.heartbeat.tick must carry field \
             {field:?} — not present in any captured log line",
        );
    }
}

// ==========================================================================
// STEP-12.13 D2 — capture writer is silently dropped on the UDP path.
//
// `start_all_inner` plumbs `capture_writer.clone()` into the
// `TcpChannelConfig` literal but the `UdpChannelConfig` literal
// inherits `capture: None` from `udp_factory.channel_config()`. Live
// runs against Zwift produce zero UDP records in `output.cap` even
// though the per-hello tracing fires twenty times. This test fails
// red until 2b adds the missing field to the UdpChannelConfig
// literal.
//
// `RecordingUdpFactory::channel_config()` returns `max_hellos: 0`,
// which makes the UDP hello loop a no-op — no UDP outbound bytes
// flow during establish. The 1 Hz heartbeat scheduler is the only
// UDP-outbound path that fires under this stub setup, so the test
// waits past one heartbeat interval before shutting down.
#[tokio::test]
async fn start_all_inner_writes_udp_outbound_to_capture_file() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    let (udp_factory, _connected, _udp_written) = RecordingUdpFactory::new();
    let runtime = RelayRuntime::start_with_all_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await
    .expect("start");

    // The heartbeat scheduler ticks at 1 Hz, with the first tick
    // landing one interval after start. Wait past that first tick
    // so the heartbeat path has produced at least one UDP send.
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
    runtime.shutdown();
    let _ = runtime.join().await;
    drop(writer);

    let mut reader = zwift_relay::capture::CaptureReader::open(path.path())
        .expect("reader");
    let mut udp_outbound = 0usize;
    while let Some(item) = reader.next_item() {
        if let Ok(zwift_relay::capture::CaptureItem::Frame(rec)) = item
            && rec.direction == zwift_relay::capture::Direction::Outbound
            && rec.transport == zwift_relay::capture::TransportKind::Udp
        {
            udp_outbound += 1;
        }
    }
    assert!(
        udp_outbound >= 1,
        "STEP-12.13 D2: start_all_inner must thread the capture writer \
         into UdpChannelConfig so at least one UDP outbound record \
         (the 1 Hz heartbeat send_player_state call) reaches the file. \
         Got {udp_outbound} UDP outbound records.",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn heartbeat_send_failure_emits_warn() {
    let scheduler = Arc::new(
        HeartbeatScheduler::new(FailingHeartbeatSink, WorldTimer::new(), 12345)
            .with_interval(std::time::Duration::from_millis(30)),
    );
    let s2 = Arc::clone(&scheduler);
    let handle = tokio::spawn(async move {
        s2.run().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    handle.abort();
    let _ = handle.await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.heartbeat.send_failed",
        ),
        "STEP-12.12 Phase 6a: relay.heartbeat.send_failed must fire at warn \
         when the sink returns an error; not found in tracing log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "error="),
        "STEP-12.12 Phase 6a: relay.heartbeat.send_failed must carry \
         the underlying error message in an error= field",
    );
}

// ==========================================================================
// STEP-12.13 D3 — UDP target must come from the first udp_config push on
// the TCP stream, not from `session.tcp_servers[0]`. Two failing tests
// covering "use the push" and "wait for the push (don't fall back)".
// 3a.iii (per-watched-athlete pool selection) is deferred until
// `observe_watched_player_state` has a non-cfg(test) seam.
// ==========================================================================

use prost::Message as _;

/// Build the framed wire bytes of a ServerToClient inbound packet
/// suitable for injection through a `ScriptedTcpTransport.read_chunk`
/// return. The header pins `conn_id` and `seqno` so the channel's
/// recv-side IV state matches the encryption side regardless of the
/// random `next_conn_id()` the daemon picked. AES key matches the
/// fixture session (`[0u8; 16]`).
fn build_inbound_servertoclient_frame(
    stc: &zwift_proto::ServerToClient,
    conn_id: u16,
    iv_seqno: u32,
) -> Vec<u8> {
    let proto_bytes = stc.encode_to_vec();
    let header = zwift_relay::Header {
        flags: zwift_relay::HeaderFlags::CONN_ID | zwift_relay::HeaderFlags::SEQNO,
        relay_id: None,
        conn_id: Some(conn_id),
        seqno: Some(iv_seqno),
    };
    let header_bytes = header.encode();
    let iv = zwift_relay::RelayIv {
        device: zwift_relay::DeviceType::Relay,
        channel: zwift_relay::ChannelType::TcpServer,
        conn_id,
        seqno: iv_seqno,
    };
    let cipher = zwift_relay::encrypt(&[0u8; 16], &iv.to_bytes(), &header_bytes, &proto_bytes);
    zwift_relay::frame_tcp(&header_bytes, &cipher)
}

/// TCP transport whose first `read_chunk` returns a pre-baked frame
/// (typically a `ServerToClient` carrying a `udp_config*`), then
/// blocks forever. `write_all` is a no-op.
struct ScriptedTcpTransport {
    pending: StdMutex<Option<Vec<u8>>>,
}

impl zwift_relay::TcpTransport for ScriptedTcpTransport {
    async fn write_all(&self, _bytes: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
        if let Some(frame) = self.pending.lock().unwrap().take() {
            return Ok(frame);
        }
        std::future::pending::<()>().await;
        unreachable!()
    }
}

struct ScriptedTcpFactory {
    transport: StdMutex<Option<ScriptedTcpTransport>>,
}

impl ScriptedTcpFactory {
    /// Build a factory whose TCP channel will deliver one
    /// `ServerToClient` containing a flat `UdpConfig` with a single
    /// `RelayAddress` pointing at `(ip, port)`.
    fn pushing_udp_config(ip: &str, port: i32) -> Self {
        let stc = zwift_proto::ServerToClient {
            udp_config: Some(zwift_proto::UdpConfig {
                relay_addresses: vec![zwift_proto::RelayAddress {
                    lb_realm: Some(0),
                    lb_course: Some(0),
                    ip: Some(ip.to_string()),
                    port: Some(port),
                    ra_f5: None,
                    ra_f6: None,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        // conn_id=0, seqno=0 line up with the channel's initial
        // recv_iv state on the very first inbound frame.
        let frame = build_inbound_servertoclient_frame(&stc, 0, 0);
        Self {
            transport: StdMutex::new(Some(ScriptedTcpTransport {
                pending: StdMutex::new(Some(frame)),
            })),
        }
    }
}

impl TcpTransportFactory for ScriptedTcpFactory {
    type Transport = ScriptedTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        let transport = self.transport.lock().unwrap().take();
        async move {
            transport.ok_or_else(|| {
                std::io::Error::other("ScriptedTcpFactory::connect called twice")
            })
        }
    }
}

/// UDP factory that records the `SocketAddr` passed to `connect()`
/// and vends a `NoopUdpTransport` (so the channel comes up but never
/// actually sends). Tests read `connected_to` to check what UDP
/// target the daemon picked.
struct AddrCapturingUdpFactory {
    captured: Arc<StdMutex<Option<std::net::SocketAddr>>>,
}

impl AddrCapturingUdpFactory {
    fn new() -> (Self, Arc<StdMutex<Option<std::net::SocketAddr>>>) {
        let captured = Arc::new(StdMutex::new(None));
        (
            Self { captured: Arc::clone(&captured) },
            captured,
        )
    }
}

impl UdpTransportFactory for AddrCapturingUdpFactory {
    type Transport = NoopUdpTransport;

    fn connect(
        &self,
        addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        *self.captured.lock().unwrap() = Some(addr);
        async { Ok(NoopUdpTransport) }
    }

    fn channel_config(&self) -> zwift_relay::UdpChannelConfig {
        zwift_relay::UdpChannelConfig { max_hellos: 0, ..Default::default() }
    }
}

/// 3a.i — UDP target must come from the first `udp_config` push on
/// the TCP stream, not from `session.tcp_servers[0]`. Today the
/// daemon connects UDP to whatever `tcp_servers[0]` says, which is
/// why the live trace got `Connection refused` — the UDP server
/// pool is announced separately from the TCP server pool.
#[tokio::test]
async fn udp_target_taken_from_first_udp_config_push_not_tcp_servers() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let mut session = fixture_session();
    session.tcp_servers = vec![zwift_relay::TcpServer { ip: "10.99.99.99".into() }];
    let pushed_udp_ip = "10.55.55.55";
    let pushed_udp_port: i32 = 3023;

    let tcp_factory = ScriptedTcpFactory::pushing_udp_config(pushed_udp_ip, pushed_udp_port);
    let (udp_factory, captured) = AddrCapturingUdpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(session),
        tcp_factory,
        udp_factory,
    )
    .await
    .expect("start");
    runtime.shutdown();
    let _ = runtime.join().await;

    let target = captured
        .lock()
        .unwrap()
        .expect(
            "STEP-12.13 D3: udp_factory.connect() must be called once \
             start_all_inner sees the first udp_config push",
        );
    assert_eq!(
        target.ip().to_string(),
        pushed_udp_ip,
        "STEP-12.13 D3: UDP target must come from the udp_config push, \
         not from session.tcp_servers; expected {pushed_udp_ip}, got {target}",
    );
    assert_ne!(
        target.ip().to_string(),
        "10.99.99.99",
        "STEP-12.13 D3: UDP must not silently fall back to tcp_servers[0] \
         when a udp_config push is available on the TCP stream",
    );
}

/// 3a.ii — without a `udp_config` push from the TCP stream, the
/// daemon must NOT silently fall back to `tcp_servers[0]`. Today it
/// does (the very bug D3 fixes), so `connect()` is called within
/// milliseconds of TCP-Established. Post-fix: no `connect()` call
/// within the wait window because the daemon is parked waiting for
/// the push.
#[tokio::test]
async fn start_all_inner_waits_for_udp_config_before_udp_connect() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let (udp_factory, captured) = AddrCapturingUdpFactory::new();

    // Use the regular StubTcpFactory — its NoopTcpTransport blocks
    // forever on read_chunk, so no ServerToClient ever arrives.
    let task = tokio::spawn(async move {
        let _ = RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            StubTcpFactory::new(),
            udp_factory,
        )
        .await;
    });

    // Pre-fix the daemon connects UDP almost immediately after the
    // TCP-Established event (within a few ms). 500 ms is well past
    // any reasonable spin-up time, so a None reading here is strong
    // evidence the daemon is correctly parked waiting for the push.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let observed = *captured.lock().unwrap();
    task.abort();
    let _ = task.await;

    assert!(
        observed.is_none(),
        "STEP-12.13 D3: udp_factory.connect() must not be called before \
         the daemon receives a udp_config push from the TCP stream; \
         silently falling back to tcp_servers[0] is the bug being fixed. \
         Observed connect() target: {observed:?}",
    );
}
