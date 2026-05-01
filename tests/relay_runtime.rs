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
    AuthLogin, RelayRuntime, SessionLogin, SessionSupervisorFactory, SessionSupervisorHandle,
    TcpTransportFactory, UdpTransportFactory,
};

fn make_config(email: &str, password: &str) -> ResolvedConfig {
    ResolvedConfig {
        main_email: Some(email.to_string()),
        main_password: Some(RedactedString::new(password.to_string())),
        monitor_email: None,
        monitor_password: None,
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
    }
}

// --- local stub DI types ------------------------------------------

struct StubAuth;

impl AuthLogin for StubAuth {
    async fn login(
        &self,
        _email: &str,
        _password: &str,
    ) -> Result<(), zwift_api::Error> {
        Ok(())
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
        tcp_servers: vec![zwift_relay::TcpServer {
            ip: "127.0.0.1".into(),
            port: 3025,
        }],
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
