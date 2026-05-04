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
        // STEP-12.14 §C2 — `start_all_inner`'s course gate refuses
        // to come up without a watched athlete. These tests exercise
        // wiring downstream of the gate, so the helper sets a default
        // ID; tests that exercise the gate itself live in
        // `tests/course_gate.rs` and build their own `ResolvedConfig`.
        watched_athlete_id: Some(54321),
    }
}

// --- helpers for synthesizing inbound TCP frames -----------------
//
// The two helpers below are used by both the stub TCP factories
// (`StubTcpFactory`, `RecordingTcpFactory`) and by the per-test
// `ScriptedTcpFactory` further down. Lifted up here so the factory
// definitions can call them without forward-reference juggling.

use prost::Message as _;

/// Build the framed wire bytes of a `ServerToClient` inbound packet
/// suitable for injection through a stub-transport `read_chunk`
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

/// Default `ServerToClient` udp_config push delivered by the stub
/// TCP transports (`NoopTcpTransport`, `RecordingTcpTransport`) so
/// STEP-12.13 §D3's wait-for-udp_config step in `start_all_inner`
/// resolves. Points the daemon at `127.0.0.1:3024`, which matches
/// what the previous `tcp_servers[0]`-based code would have used.
fn default_udp_config_push() -> Vec<u8> {
    let stc = zwift_proto::ServerToClient {
        udp_config: Some(zwift_proto::UdpConfig {
            relay_addresses: vec![zwift_proto::RelayAddress {
                lb_realm: Some(0),
                lb_course: Some(0),
                ip: Some("127.0.0.1".to_string()),
                port: Some(3024),
                ra_f5: None,
                ra_f6: None,
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    build_inbound_servertoclient_frame(&stc, 0, 0)
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

    async fn get_player_state(
        &self,
        _athlete_id: i64,
    ) -> Result<Option<zwift_proto::PlayerState>, zwift_api::Error> {
        // STEP-12.14 §C2: keep the course-gate happy by claiming the
        // watched athlete is in a game. Tests using this stub exercise
        // the auth-login wiring rather than the course gate.
        Ok(Some(zwift_proto::PlayerState {
            world: Some(1),
            ..Default::default()
        }))
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

    async fn get_player_state(
        &self,
        _athlete_id: i64,
    ) -> Result<Option<zwift_proto::PlayerState>, zwift_api::Error> {
        Ok(Some(zwift_proto::PlayerState {
            world: Some(1),
            ..Default::default()
        }))
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

    async fn get_player_state(
        &self,
        _athlete_id: i64,
    ) -> Result<Option<zwift_proto::PlayerState>, zwift_api::Error> {
        Ok(Some(zwift_proto::PlayerState {
            world: Some(1),
            ..Default::default()
        }))
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
/// going through the kernel. `write_all` is a no-op; `read_chunk`
/// drains an optional pre-baked frame then blocks forever.
///
/// STEP-12.13 §D3: `start_all_inner` now waits for a `udp_config`
/// push from the TCP `ServerToClient` stream before bringing UDP
/// up. The default `StubTcpFactory::new()` factory primes the
/// transport with a synthetic push pointing UDP at `127.0.0.1:3024`
/// so existing tests continue to reach UDP-established without
/// modification. Tests that need no-push semantics (e.g. the D3
/// "wait for udp_config" test) use [`StubTcpFactory::silent`].
struct NoopTcpTransport {
    pending: StdMutex<Option<Vec<u8>>>,
}

impl NoopTcpTransport {
    fn with_pending(frame: Option<Vec<u8>>) -> Self {
        Self { pending: StdMutex::new(frame) }
    }
}

impl zwift_relay::TcpTransport for NoopTcpTransport {
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

struct StubTcpFactory {
    transport: StdMutex<Option<NoopTcpTransport>>,
}

impl StubTcpFactory {
    fn new() -> Self {
        Self {
            transport: StdMutex::new(Some(NoopTcpTransport::with_pending(Some(
                default_udp_config_push(),
            )))),
        }
    }

    /// Variant whose transport never pushes anything — `read_chunk`
    /// blocks forever from the first call. Used by D3's
    /// `start_all_inner_waits_for_udp_config_before_udp_connect`
    /// to verify the daemon doesn't silently fall back to
    /// `tcp_servers[0]` when no udp_config arrives.
    fn silent() -> Self {
        Self {
            transport: StdMutex::new(Some(NoopTcpTransport::with_pending(None))),
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
        // Silent variant — this test uses the older `start_with_deps`
        // path (which does NOT go through `start_all_inner`'s STEP-12.13
        // wait-for-udp_config step), so the default udp_config push
        // would just be extra bytes in the capture file that bias the
        // record count assertion below.
        StubTcpFactory::silent(),
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
/// writes. `read_chunk` drains an optional pre-baked frame
/// (defaults to a synthetic `udp_config` push so STEP-12.13 §D3's
/// wait-for-udp_config step in `start_all_inner` resolves), then
/// blocks forever.
struct RecordingTcpTransport {
    written: Arc<StdMutex<Vec<Vec<u8>>>>,
    pending: StdMutex<Option<Vec<u8>>>,
}

impl zwift_relay::TcpTransport for RecordingTcpTransport {
    async fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.written.lock().unwrap().push(bytes.to_vec());
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
        async move {
            Ok(RecordingTcpTransport {
                written,
                pending: StdMutex::new(Some(default_udp_config_push())),
            })
        }
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
        async {
            Ok(NoopTcpTransport::with_pending(Some(default_udp_config_push())))
        }
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
    async fn send(&self, _state: zwift_proto::PlayerState) -> std::io::Result<()> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

struct FailingHeartbeatSink;

impl HeartbeatSink for FailingHeartbeatSink {
    async fn send(&self, _state: zwift_proto::PlayerState) -> std::io::Result<()> {
        Err(std::io::Error::other("simulated heartbeat failure"))
    }
}

#[tokio::test]
#[tracing_test::traced_test]
async fn heartbeat_tick_emits_debug_event_per_interval() {
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sink = CountingHeartbeatSink { count: Arc::clone(&count) };
    let scheduler = Arc::new(
        HeartbeatScheduler::new(sink, WorldTimer::new(), 12345, 99, 10)
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
        HeartbeatScheduler::new(FailingHeartbeatSink, WorldTimer::new(), 12345, 99, 10)
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
//
// `build_inbound_servertoclient_frame` is defined near the top of this
// file (used by both the default stub transports and the scripted
// factory below).
// ==========================================================================

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
    /// Build a factory that delivers one `ServerToClient` containing a
    /// `udp_config_vod_1` with the given pools. Each pool entry is
    /// `(lb_course, ip)` — `lb_realm` defaults to 0 and the port is
    /// always omitted so the daemon must fall back to `UDP_PORT_SECURE`.
    fn pushing_udp_config_vod(pools: &[(i32, &str)]) -> Self {
        let relay_addresses_vod = pools
            .iter()
            .map(|(lb_course, ip)| zwift_proto::RelayAddressesVod {
                lb_realm: Some(0),
                lb_course: Some(*lb_course),
                relay_addresses: vec![zwift_proto::RelayAddress {
                    lb_realm: Some(0),
                    lb_course: Some(*lb_course),
                    ip: Some(ip.to_string()),
                    port: None, // daemon must hardcode 3024 (§C5)
                    ra_f5: None,
                    ra_f6: None,
                }],
                rav_f4: None,
            })
            .collect();
        let stc = zwift_proto::ServerToClient {
            udp_config_vod_1: Some(zwift_proto::UdpConfigVod {
                relay_addresses_vod,
                port: None,
                ucv_f3: None,
                ucv_f4: None,
                ucv_f5: None,
                ucv_f6: None,
            }),
            ..Default::default()
        };
        let frame = build_inbound_servertoclient_frame(&stc, 0, 0);
        Self {
            transport: StdMutex::new(Some(ScriptedTcpTransport {
                pending: StdMutex::new(Some(frame)),
            })),
        }
    }

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

    // Silent variant — the NoopTcpTransport never delivers any
    // ServerToClient, so the daemon's wait-for-udp_config step
    // never resolves.
    let task = tokio::spawn(async move {
        let _ = RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            StubTcpFactory::silent(),
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

// ==========================================================================
// STEP-12.14 §N2 / §1a — TCP and UDP `connId` counters must be independent.
// Sauce's NetChannel subclasses (`TCPChannel`, `UDPChannel`) each have their
// own `static _connInc = 0` so a fresh process gets TCP `connId=0` AND UDP
// `connId=0` (same value, different counters). We currently share a single
// counter, so TCP and UDP get different values. This test fails to compile
// in red state because `next_tcp_conn_id` and `next_udp_conn_id` don't
// exist yet — the fix is to split `next_conn_id` into the two functions.
// ==========================================================================

#[test]
fn tcp_and_udp_conn_id_counters_are_independent() {
    use ranchero::daemon::relay::{next_tcp_conn_id, next_udp_conn_id};

    let tcp_first = next_tcp_conn_id();
    let udp_first = next_udp_conn_id();
    let tcp_second = next_tcp_conn_id();
    let udp_second = next_udp_conn_id();

    assert_eq!(
        tcp_second.wrapping_sub(tcp_first),
        1,
        "TCP counter must increment monotonically",
    );
    assert_eq!(
        udp_second.wrapping_sub(udp_first),
        1,
        "UDP counter must increment monotonically",
    );

    // The crucial assertion: a UDP allocation must NOT advance the TCP
    // counter and vice-versa. After two intervening UDP allocations,
    // the next TCP allocation must still be exactly one step past the
    // previous TCP allocation.
    let tcp_third = next_tcp_conn_id();
    let _udp_third = next_udp_conn_id();
    let _udp_fourth = next_udp_conn_id();
    let tcp_fourth = next_tcp_conn_id();
    assert_eq!(
        tcp_fourth.wrapping_sub(tcp_third),
        1,
        "STEP-12.14 §N2: TCP counter must NOT advance from intervening \
         UDP allocations; sauce uses two separate static counters per \
         NetChannel subclass.",
    );
}

// ==========================================================================
// STEP-12.14 Phase 3a — UDP pool selection (C1)
//
// Sauce keeps `_udpServerPools` as a `Map<courseId, pool>` and always
// uses `_udpServerPools.get(0).servers[0].ip` for the initial UDP target
// (the generic load-balancer pool at lb_course=0). Our current code calls
// `extract_udp_servers` which flattens ALL pools into one list and picks
// the first arbitrary entry — so if lb_course=42 appears first in the
// `udp_config_vod_1` list, we'd connect to a per-course server that
// rejects athletes who aren't on that course. Both tests are red until
// Phase 3b refactors `extract_udp_servers` → `extract_udp_pools` and
// uses the lb_course=0 pool for the initial connect.
// ==========================================================================

#[tokio::test]
async fn udp_target_picked_from_lb_course_zero_pool_not_per_course_pool() {
    let cfg = make_config("monitor@example.com", "monitor-pass");

    // Push a udp_config_vod_1 with TWO pools in this order:
    //   lb_course=42, ip="10.0.0.42"  ← per-course pool, listed FIRST
    //   lb_course=0,  ip="10.0.0.1"   ← generic load-balancer pool
    //
    // The daemon must pick 10.0.0.1 (lb_course=0), not 10.0.0.42
    // (lb_course=42, which is first in the flat list).
    let tcp_factory = ScriptedTcpFactory::pushing_udp_config_vod(&[
        (42, "10.0.0.42"),
        (0,  "10.0.0.1"),
    ]);
    let (udp_factory, captured) = AddrCapturingUdpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
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
        .expect("udp_factory.connect() must be called");
    assert_eq!(
        target.ip().to_string(),
        "10.0.0.1",
        "STEP-12.14 §C1: UDP target must come from the lb_course=0 \
         (generic load-balancer) pool (`_udpServerPools.get(0).servers[0]`). \
         The per-course pool at lb_course=42 appeared first in the list but \
         must not be picked. Got {target}",
    );
    assert_ne!(
        target.ip().to_string(),
        "10.0.0.42",
        "STEP-12.14 §C1: daemon must not silently pick the per-course pool \
         (lb_course=42) when a generic pool (lb_course=0) is also present",
    );
}

#[tokio::test]
async fn udp_setup_errors_when_no_lb_course_zero_pool_present() {
    let cfg = make_config("monitor@example.com", "monitor-pass");

    // Push a udp_config_vod_1 with ONLY a per-course pool. Without a
    // lb_course=0 generic pool, the daemon must surface a typed error
    // rather than silently picking the per-course server.
    let tcp_factory = ScriptedTcpFactory::pushing_udp_config_vod(&[
        (42, "10.0.0.42"),
    ]);
    let (udp_factory, connected_flag) = AddrCapturingUdpFactory::new();

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        tcp_factory,
        udp_factory,
    )
    .await;

    let err = match result {
        Ok(_) => panic!(
            "STEP-12.14 §C1: when no lb_course=0 pool is present the daemon \
             must return a typed error rather than picking an arbitrary \
             per-course server; got Ok",
        ),
        Err(e) => e,
    };
    let err_msg = err.to_string();
    assert!(
        err_msg.to_lowercase().contains("udp") || err_msg.to_lowercase().contains("pool"),
        "STEP-12.14 §C1: error when no generic pool present must mention \
         UDP or pool; got {err_msg:?}",
    );
    assert!(
        connected_flag.lock().unwrap().is_none(),
        "STEP-12.14 §C1: udp_factory.connect() must NOT be called when \
         only per-course pools are present — daemon should error first",
    );
}

// ==========================================================================
// STEP-12.14 Phase 5a — Post-establish UDP send + TCP hello seqno.
//
// Tests cover C3 (post-establish `send_player_state`) and N5 (TCP hello
// seqno = 0, not 1).
//
// All three tests are red until Phase 5b:
//   1. `start_all_inner` calls `udp_channel.send_player_state(initial_state)`
//      between UDP-establish and the heartbeat spawn.
//   2. The call site logs `relay.udp.post_establish.sent` carrying
//      `watching_rider_id`, `just_watching`, and `world` fields.
//   3. The TCP hello literal changes `seqno: Some(1)` to `seqno: Some(0)`.
// ==========================================================================

/// Decode a framed TCP wire packet (as captured by `RecordingTcpTransport`)
/// into its `ClientToServer` payload. Strips the 2-byte big-endian
/// length prefix added by `frame_tcp`, parses the header, decrypts using
/// the fixture AES key `[0u8; 16]`, and decodes the inner proto.
fn decode_tcp_hello_cts(wire: &[u8]) -> zwift_proto::ClientToServer {
    // TCP frames carry a 2-byte big-endian length prefix; skip it.
    let frame = &wire[2..];
    let parsed = zwift_relay::decode_header(frame).expect("decode header");
    let aad = &frame[..parsed.consumed];
    let cipher = &frame[parsed.consumed..];
    let conn_id = parsed.header.conn_id.expect("TCP hello must carry conn_id in header");
    let seqno = parsed.header.seqno.unwrap_or(0);
    let iv = zwift_relay::RelayIv {
        device: zwift_relay::DeviceType::Relay,
        channel: zwift_relay::ChannelType::TcpClient,
        conn_id,
        seqno,
    };
    let plaintext = zwift_relay::decrypt(
        &[0u8; 16],
        &iv.to_bytes(),
        aad,
        cipher,
    ).expect("decrypt TCP hello");
    let tcp = zwift_relay::parse_tcp_plaintext(&plaintext).expect("parse TCP plaintext");
    zwift_proto::ClientToServer::decode(tcp.proto_bytes).expect("decode CTS from TCP hello")
}

/// After `UdpChannel::establish` returns, `start_all_inner` must call
/// `send_player_state` exactly once — before the 1 Hz heartbeat fires —
/// to register the relay session with the server. (STEP-12.14 §C3)
///
/// Uses `RecordingUdpFactory` (max_hellos = 0 → no hello packets, instant
/// convergence). Checks written-packet count immediately after
/// `start_with_all_deps` returns, so the 1-second heartbeat delay hasn't
/// elapsed and all recorded sends are the post-establish registration.
#[tokio::test]
async fn post_establish_sends_exactly_one_udp_packet_before_first_heartbeat() {
    let cfg = make_config("monitor@example.com", "pass");
    let (udp_factory, _connected, written) = RecordingUdpFactory::new();

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

    // Read the count before awaiting anything. The post-establish send is
    // synchronous inside start_all_inner; the 1 Hz heartbeat timer hasn't
    // had time to fire.
    let count = written.lock().unwrap().len();

    runtime.shutdown();
    let _ = runtime.join().await;

    assert_eq!(
        count,
        1,
        "STEP-12.14 §C3: exactly one UDP send (the post-establish \
         watching-registration packet) must fire between UDP convergence \
         and the first heartbeat. Got {count} packets immediately after start.",
    );
}

/// The post-establish `send_player_state` must emit a `relay.udp.post_establish.sent`
/// trace event carrying `watching_rider_id`, `just_watching`, and `world`
/// fields so operators can verify the session registration without decrypting
/// wire bytes. (STEP-12.14 §C3)
#[tokio::test]
#[tracing_test::traced_test]
async fn post_establish_player_state_emits_trace_with_required_fields() {
    let cfg = make_config("monitor@example.com", "pass");
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
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.udp.post_establish.sent",
        ),
        "STEP-12.14 §C3: relay.udp.post_establish.sent must fire at info \
         synchronously after UdpChannel::establish; not found in log",
    );
    for field in ["watching_rider_id=", "just_watching=", "world="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.14 §C3: relay.udp.post_establish.sent must carry \
             field {field:?} — not found in any captured log line",
        );
    }
}

/// The TCP hello must carry `seqno = Some(0)`, matching sauce4zwift which
/// starts the sequence at 0 (`zwift.mjs:1821`: `seqno: 0`). The daemon
/// currently sends `seqno: Some(1)`, which is an off-by-one. (STEP-12.14 §N5)
///
/// Decrypts the first TCP write recorded by `RecordingTcpFactory` to read
/// the hello's `ClientToServer.seqno` field directly.
#[tokio::test]
async fn tcp_hello_seqno_is_zero_not_one() {
    let cfg = make_config("monitor@example.com", "pass");
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
        "STEP-12.14 §N5: TCP hello must have been written; \
         RecordingTcpTransport recorded no writes",
    );
    let cts = decode_tcp_hello_cts(&writes[0]);
    assert_eq!(
        cts.seqno,
        Some(0),
        "STEP-12.14 §N5: TCP hello seqno must be 0 (sauce starts at 0, \
         not 1). Got {:?}",
        cts.seqno,
    );
}

// ==========================================================================
// STEP-12.14 Phase 6a — Heartbeat content + shared WorldTimer.
//
// Tests cover C4 (heartbeat PlayerState must carry watching-identity fields)
// and N13 (world_time must live in the PlayerState, not only in the CTS
// wrapper; the WorldTimer must be the clone shared with UdpChannel::establish
// so any SNTP offset from the hello exchange is reflected in heartbeat ticks).
//
// Both tests are red until Phase 6b:
//   1. HeartbeatScheduler gains `watching_rider_id` and `course_id` fields.
//   2. `next_payload` populates `state.just_watching`, `state.watching_rider_id`,
//      `state.world`, and `state.world_time` from the shared WorldTimer.
//   3. The per-tick loop emits a `relay.heartbeat.state` trace event carrying
//      those fields so operators can verify session registration without
//      decrypting wire bytes.
// ==========================================================================

/// After each heartbeat tick the scheduler must emit a `relay.heartbeat.state`
/// trace event carrying the watching-identity fields — `just_watching`,
/// `watching_rider_id`, and `world` (course ID) — so operators can observe
/// session registration without decrypting UDP traffic. (STEP-12.14 §C4)
///
/// Red state: the scheduler builds `state: PlayerState::default()` and emits
/// no content-field trace. After 6b, the scheduler receives `watching_rider_id`
/// and `course_id` from `start_all_inner` and emits the dedicated state event.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn heartbeat_player_state_emits_trace_with_watching_identity_fields() {
    let cfg = make_config("monitor@example.com", "pass");
    // make_config sets watched_athlete_id = Some(54321).
    // StubAuth::get_player_state returns world = Some(1) → course_id = 1.
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

    // Sleep in paused-time mode: tokio advances the clock through all
    // intermediate timer deadlines, giving the spawned heartbeat task a
    // chance to initialize its interval and fire its first tick at 1000 ms.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.heartbeat.state",
        ),
        "STEP-12.14 §C4: heartbeat must emit a relay.heartbeat.state trace \
         event after each tick; not found in captured log",
    );
    for field in ["watching_rider_id=", "just_watching=", "world="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.14 §C4: relay.heartbeat.state must carry field {field:?} \
             so operators can verify session registration; not found in log",
        );
    }
}

/// The heartbeat's `PlayerState.world_time` must be populated and emitted
/// in the `relay.heartbeat.state` event. In the current code `world_time`
/// lives only in the CTS wrapper's top-level field, not inside `state`; the
/// scheduler also receives a fresh independent timer rather than the clone
/// shared with `UdpChannel::establish`, so any SNTP offset from the hello
/// exchange is invisible to subsequent heartbeats. (STEP-12.14 §N13)
///
/// Red state: `relay.heartbeat.state` is not emitted at all, so its
/// `world_time=` field cannot appear in the log either. After 6b, the
/// WorldTimer clone is passed to the scheduler and the event carries
/// `world_time=<non_zero_value>`. The lower-level `relay.udp.playerstate.sent`
/// line from `zwift_relay::udp` also carries `world_time=`, so the test gates
/// on `relay.heartbeat.state` being present first to confirm the assertion
/// refers to the heartbeat-level field, not the lower-level UDP trace.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn heartbeat_player_state_world_time_in_state_not_only_cts() {
    let cfg = make_config("monitor@example.com", "pass");
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

    // Same paused-time sleep as the companion test.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    // Gate on the heartbeat-state event existing; without it, world_time=
    // might only appear in the lower-level relay.udp.playerstate.sent line.
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.heartbeat.state",
        ),
        "STEP-12.14 §N13: relay.heartbeat.state must be emitted before \
         world_time= can be verified at heartbeat level; not found in log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "world_time="),
        "STEP-12.14 §N13: relay.heartbeat.state must carry world_time= \
         reflecting the WorldTimer clone shared with UdpChannel::establish; \
         not found in captured log",
    );
}
