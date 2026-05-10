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
    AuthLogin, DefaultUdpTransportFactory, GameEvent, RelayRuntime, SessionLogin,
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
/// resolves. Uses `udp_config_vod_1` — the only format that
/// `extract_udp_pools` processes after §k2; flat `udp_config` is
/// intentionally ignored by the production daemon.
fn default_udp_config_push() -> Vec<u8> {
    let stc = zwift_proto::ServerToClient {
        udp_config_vod_1: Some(zwift_proto::UdpConfigVod {
            relay_addresses_vod: vec![zwift_proto::RelayAddressesVod {
                lb_realm: Some(0),
                lb_course: Some(0),
                relay_addresses: vec![zwift_proto::RelayAddress {
                    lb_realm: Some(0),
                    lb_course: Some(0),
                    ip: Some("127.0.0.1".to_string()),
                    port: Some(3024),
                    ..Default::default()
                }],
                rav_f4: None,
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

/// Stub auth representing a watched athlete who is online (Zwift
/// returned a `PlayerState`) but who is not currently in any world
/// (`state.world = None`). STEP-12.16 §F6 Phase 1a fixture: the
/// daemon must accept this state and start in a suspended posture
/// rather than aborting startup with `WatchedAthleteNotInGame`.
struct WatchedAthleteOfflineAuth;

impl AuthLogin for WatchedAthleteOfflineAuth {
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
            world: None,
            ..Default::default()
        }))
    }
}

/// Stub auth representing a watched athlete whose
/// `/relay/worlds/1/players/{id}` endpoint returns 404 (mapped to
/// `Ok(None)` by `ZwiftAuth::get_player_state`). STEP-12.16 §F6
/// Phase 1a fixture for sauce4zwift's null-state branch
/// (`zwift.mjs:613-622`, `:1706-1716`).
struct WatchedAthleteNoStateAuth;

impl AuthLogin for WatchedAthleteNoStateAuth {
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
        Ok(None)
    }
}

/// Auth stub that simulates a watched athlete transitioning from offline to
/// in-game.  The first call to `get_player_state` (the startup course-gate
/// check) returns `world: None` so the daemon starts suspended.  Every
/// subsequent call returns `world: Some(7)`, modelling the athlete entering
/// Watopia.  Used by Phase 3a state-refresher resume tests.
struct TransitioningAuth {
    call_count: Arc<StdMutex<usize>>,
}

impl TransitioningAuth {
    fn new() -> Self {
        Self { call_count: Arc::new(StdMutex::new(0)) }
    }
}

impl AuthLogin for TransitioningAuth {
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
        let mut count = self.call_count.lock().unwrap();
        let n = *count;
        *count += 1;
        if n == 0 {
            Ok(Some(zwift_proto::PlayerState { world: None, ..Default::default() }))
        } else {
            Ok(Some(zwift_proto::PlayerState { world: Some(7), ..Default::default() }))
        }
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
        content_type: zwift_relay::capture::ContentType::Unspecified,
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
        HeartbeatScheduler::new(
            sink,
            WorldTimer::new(),
            12345,
            99,
            10,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
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
#[ignore = "slow: waits 1.2 s for the first heartbeat tick to produce a UDP outbound record"]
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
        HeartbeatScheduler::new(
            FailingHeartbeatSink,
            WorldTimer::new(),
            12345,
            99,
            10,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
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
                    ..Default::default()
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

/// 3a.i — UDP target must come from the first `udp_config_vod_1` push on
/// the TCP stream, not from `session.tcp_servers[0]`. Before D3, the
/// daemon connected UDP to whatever `tcp_servers[0]` said, which is
/// why the live trace got `Connection refused` — the UDP server
/// pool is announced separately from the TCP server pool.
#[tokio::test]
async fn udp_target_taken_from_first_udp_config_push_not_tcp_servers() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let mut session = fixture_session();
    session.tcp_servers = vec![zwift_relay::TcpServer { ip: "10.99.99.99".into() }];
    let pushed_udp_ip = "10.55.55.55";

    // §k2: only udp_config_vod_1 is processed; use that format here.
    let tcp_factory = ScriptedTcpFactory::pushing_udp_config_vod(&[(0, pushed_udp_ip)]);
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
             start_all_inner sees the first udp_config_vod_1 push",
        );
    assert_eq!(
        target.ip().to_string(),
        pushed_udp_ip,
        "STEP-12.13 D3: UDP target must come from the udp_config_vod_1 push, \
         not from session.tcp_servers; expected {pushed_udp_ip}, got {target}",
    );
    assert_ne!(
        target.ip().to_string(),
        "10.99.99.99",
        "STEP-12.13 D3: UDP must not silently fall back to tcp_servers[0] \
         when a udp_config_vod_1 push is available on the TCP stream",
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

// ==========================================================================
// STEP-12.14 Phase 8a — WorldAttribute timestamp tracking + TCP hello
// larg_wa_time field.
//
// Tests cover M2 (TCP hello must carry larg_wa_time) and L3 (recv-loop must
// advance last_world_update_ts from inbound WorldAttribute.timestamp entries).
//
// Both tests are red until Phase 8b:
//   1. RuntimeInner gains last_world_update_ts: AtomicI64 (initially 0).
//   2. recv-loop Inbound arm walks stc.updates, advances last_world_update_ts
//      to max(current, wa.timestamp.unwrap_or(0)), and emits a
//      relay.tcp.world_update.tracked trace event carrying the new value.
//   3. TCP hello construction reads last_world_update_ts and sets
//      larg_wa_time: Some(...); relay.tcp.hello.sent trace gains larg_wa_time=.
// ==========================================================================

/// After injecting an inbound ServerToClient that carries WorldAttribute
/// entries with timestamp values, the recv-loop must advance
/// `last_world_update_ts` to the highest timestamp in the batch and emit a
/// `relay.tcp.world_update.tracked` trace event. (STEP-12.14 §L3)
///
/// Red state: the recv-loop's Inbound arm checks `stc.updates.is_empty()`
/// for the message-kind label but does not read `wa.timestamp` or advance
/// any tracked state. No trace event is emitted and the assertion fails.
#[tokio::test]
#[tracing_test::traced_test]
async fn inbound_world_updates_advance_last_world_update_ts() {
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
    .expect("start");

    // Two WorldAttributes in one batch; the one with the higher timestamp
    // (9_000_000) must win.
    let stc = zwift_proto::ServerToClient {
        updates: vec![
            zwift_proto::WorldAttribute {
                timestamp: Some(5_000_000),
                ..Default::default()
            },
            zwift_proto::WorldAttribute {
                timestamp: Some(9_000_000),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.world_update.tracked",
        ),
        "STEP-12.14 §L3: recv-loop must emit relay.tcp.world_update.tracked \
         when processing WorldAttribute entries with timestamps; not found in log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "last_world_update_ts=9000000",
        ),
        "STEP-12.14 §L3: relay.tcp.world_update.tracked must carry \
         last_world_update_ts=9000000 (the max of all timestamps in the batch); \
         not found in log",
    );
}

/// The TCP hello must include `larg_wa_time` in both the encoded
/// ClientToServer and the `relay.tcp.hello.sent` trace event so operators
/// can verify the reconnect-timestamp handshake. (STEP-12.14 §M2)
///
/// Red state: `relay.tcp.hello.sent` is emitted without any fields; the
/// assertion on `larg_wa_time=` fails immediately.
#[tokio::test]
#[tracing_test::traced_test]
async fn tcp_hello_carries_larg_wa_time_field_in_trace() {
    let cfg = make_config("monitor@example.com", "pass");

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.tcp.hello.sent"),
        "relay.tcp.hello.sent must be emitted (prerequisite for the larg_wa_time check)",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "larg_wa_time="),
        "STEP-12.14 §M2: relay.tcp.hello.sent must carry a larg_wa_time= field \
         so operators can verify the reconnect timestamp is correctly populated; \
         not found in log",
    );
}

// ==========================================================================
// STEP-12.14 Batch A §Aa — Live pool routing integration tests.
//
// Tests cover recv_loop storing udp_config_vod pools in inner.pool_router
// and calling recompute_udp_selection after each update.
//
// All four tests are red until Batch Ab:
//   1. recv_loop Inbound arm calls extract_udp_pools(&stc) and stores each
//      pool via inner.pool_router.lock().apply_pool_update(pool), emitting
//      relay.udp.pool_router.updated with lb_realm and lb_course fields.
//   2. recompute_udp_selection moves from #[cfg(test)] to production code
//      and is called from the recv_loop after each pool update; a
//      GameEvent::PoolSwap is broadcast when the selected server changes.
//   3. On a server change, the old UdpChannel is scheduled for a 60-second
//      grace shutdown; relay.udp.channel.grace_shutdown is emitted when the
//      grace task is spawned.
// ==========================================================================

/// Build a `ServerToClient` carrying a single `udp_config_vod_1` pool entry.
/// Used by Batch A integration tests to inject mid-session pool pushes into
/// the recv-loop without going through the startup wait path.
fn build_pool_push_stc(lb_realm: i32, lb_course: i32, ip: &str) -> zwift_proto::ServerToClient {
    zwift_proto::ServerToClient {
        udp_config_vod_1: Some(zwift_proto::UdpConfigVod {
            relay_addresses_vod: vec![zwift_proto::RelayAddressesVod {
                lb_realm: Some(lb_realm),
                lb_course: Some(lb_course),
                relay_addresses: vec![zwift_proto::RelayAddress {
                    ip: Some(ip.to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A mid-session `udp_config_vod_1` push must cause the recv-loop to store
/// the inbound pool in `inner.pool_router` and emit a
/// `relay.udp.pool_router.updated` debug trace event carrying the pool's
/// `lb_realm` and `lb_course` fields. (Batch A §Aa)
///
/// Red state: the recv-loop's Inbound arm ignores `stc.udp_config_vod_1`
/// entirely; no pool is stored and no trace event fires.
#[tokio::test]
#[tracing_test::traced_test]
async fn recv_loop_inbound_updates_pool_router_from_udp_config_push() {
    let cfg = make_config("monitor@example.com", "pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    // Inject a pool for (realm=1, course=5) — distinct from the generic
    // lb_course=0 pool, so any match must come from the Inbound arm.
    let stc = build_pool_push_stc(1, 5, "10.0.0.1");
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.udp.pool_router.updated",
        ),
        "Batch A §Aa: recv_loop must call extract_udp_pools on inbound STC \
         and store each pool via pool_router.apply_pool_update, emitting \
         relay.udp.pool_router.updated; not found in trace log",
    );
}

/// After a mid-session pool push, the recv-loop must call
/// `recompute_udp_selection`. When the computed server differs from the
/// current selection, a `GameEvent::PoolSwap` must be broadcast.
/// (Batch A §Aa)
///
/// The watched athlete starts at `(realm=0, course=0, x=0, y=0)`. Injecting
/// a pool for `(realm=0, course=0)` with one server should trigger a swap
/// from `None` to that server immediately.
///
/// Red state: the recv-loop does not call `recompute_udp_selection` after
/// storing a pool; no `GameEvent::PoolSwap` is emitted.
#[tokio::test]
async fn pool_router_swap_emits_pool_swap_game_event() {
    let cfg = make_config("monitor@example.com", "pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    let mut events_rx = runtime.events();

    // Pool for (realm=0, course=0): matches the watched athlete's initial
    // state, so recompute_udp_selection should pick this server and emit
    // PoolSwap { from: None, to: <addr> }.
    let stc = build_pool_push_stc(0, 0, "10.0.0.2");
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    let mut found_swap = false;
    while let Ok(e) = events_rx.try_recv() {
        if matches!(e, GameEvent::PoolSwap { .. }) {
            found_swap = true;
        }
    }
    assert!(
        found_swap,
        "Batch A §Aa: recv_loop must call recompute_udp_selection after a \
         pool update; when the new server differs from the current selection, \
         GameEvent::PoolSwap must be broadcast; none received",
    );
}

/// When a pool swap changes the active UDP server, the old channel must be
/// scheduled for a 60-second grace shutdown, and the runtime must emit
/// `relay.udp.channel.grace_shutdown` when the grace task is spawned.
/// (Batch A §Aa / Ab §L6)
///
/// Red state: channel swapping is not yet implemented; no grace-shutdown
/// event fires and the assertion fails.
#[tokio::test]
#[tracing_test::traced_test]
async fn udp_channel_swap_runs_grace_shutdown_on_old_channel() {
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
    .expect("start");

    // First push: establishes 10.0.0.1 as the selected UDP server.
    let stc_a = build_pool_push_stc(0, 0, "10.0.0.1");
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc_a)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Second push for the same (realm=0, course=0) with a different IP:
    // triggers a swap and should schedule the old channel for grace shutdown.
    let stc_b = build_pool_push_stc(0, 0, "10.0.0.3");
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc_b)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.udp.channel.grace_shutdown",
        ),
        "Batch A §Aa: when a pool swap changes the active UDP server, the \
         old channel must be scheduled for grace shutdown and \
         relay.udp.channel.grace_shutdown must be emitted at that point; \
         not found in log",
    );
}

/// A `udp_config_vod_1` push may carry pools keyed by non-zero `lb_realm`
/// (portal realm). Each pool must be stored independently in the router
/// under its own `(lb_realm, lb_course)` key, and the trace event must
/// carry the `lb_realm` value so operators can identify portal-realm entries.
/// (Batch A §Aa)
///
/// Red state: the recv-loop ignores pool pushes entirely; no pools are
/// stored and no trace events fire.
#[tokio::test]
#[tracing_test::traced_test]
async fn portal_pool_handled_via_portal_key() {
    let cfg = make_config("monitor@example.com", "pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    // Push an STC with two pools: one generic (lb_realm=0) and one
    // portal-realm (lb_realm=1). Both must be stored independently.
    let stc = zwift_proto::ServerToClient {
        udp_config_vod_1: Some(zwift_proto::UdpConfigVod {
            relay_addresses_vod: vec![
                zwift_proto::RelayAddressesVod {
                    lb_realm: Some(0),
                    lb_course: Some(0),
                    relay_addresses: vec![zwift_proto::RelayAddress {
                        ip: Some("10.0.0.1".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                zwift_proto::RelayAddressesVod {
                    lb_realm: Some(1),
                    lb_course: Some(0),
                    relay_addresses: vec![zwift_proto::RelayAddress {
                        ip: Some("10.0.0.2".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    };
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.udp.pool_router.updated",
        ),
        "Batch A §Aa: recv_loop must store all pools (including portal-realm) \
         from an inbound udp_config_vod_1 push; relay.udp.pool_router.updated \
         not found in trace log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "lb_realm=1"),
        "Batch A §Aa: relay.udp.pool_router.updated must carry lb_realm=1 \
         for the portal-realm pool entry; not found in trace log",
    );
}

// ==========================================================================
// STEP-12.14 Batch B §Ba — Connect retry & supervisor recovery (red state).
//
// Tests cover L5 (connect retry with exponential back-off), L4 (TCP server
// pinning across reconnects), N14 (supervisor re-login causes channel
// recreation), and N9 (logout + leave called on clean shutdown).
//
// All four tests are red until Batch Bb:
//   1. `start_with_all_deps` (or its internal `connect_with_retry` wrapper)
//      retries a failed `start_all_inner` with 1000 ms × 1.2^attempt
//      back-off, emitting `relay.runtime.connect_retry attempt=N backoff_ms=M`
//      before each retry.
//   2. The chosen TCP server's IP is remembered across retry/reconnect calls;
//      when the supervisor returns a session with a shuffled server list, the
//      runtime prefers the remembered IP and emits
//      `relay.runtime.tcp_server_pinned`.
//   3. A `SessionEvent::LoggedIn(new_session)` carrying a changed AES key
//      causes the runtime to tear down and recreate TCP + UDP channels with
//      the new key, emitting `relay.runtime.channels_recreated`.
//   4. `RelayRuntime::shutdown()` calls `auth.logout()` and `auth.leave()`
//      best-effort, emitting `relay.runtime.logout` and `relay.runtime.leave`.
// ==========================================================================

/// TCP factory that fails the first connect with ConnectionRefused,
/// then succeeds. Used to exercise the transient retry path.
struct TransientTcpFactory {
    attempts: Arc<StdMutex<u32>>,
}

impl TransientTcpFactory {
    fn new() -> Self {
        Self { attempts: Arc::new(StdMutex::new(0)) }
    }
}

impl TcpTransportFactory for TransientTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        let mut attempts = self.attempts.lock().unwrap();
        *attempts += 1;
        let attempt = *attempts;
        async move {
            if attempt == 1 {
                Err(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "transient error"))
            } else {
                Ok(NoopTcpTransport::with_pending(Some(default_udp_config_push())))
            }
        }
    }
}

#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn start_with_writer_retries_on_transient_tcp_connect() {
    let cfg = make_config("monitor@example.com", "pass");
    let tcp_factory = TransientTcpFactory::new();

    let runtime = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            tcp_factory,
            NoopUdpFactory,
        ),
    )
    .await
    .expect("test must not hang")
    .expect("runtime must start successfully after retry");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.connect_retry",
        ),
        "STEP-12.15 F2: production path must emit relay.runtime.connect_retry \
         before retrying; not found in log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "attempt=1"),
        "STEP-12.15 F2: production path must emit attempt=1 on first retry",
    );
}

#[tokio::test]
async fn start_with_writer_propagates_permanent_errors_immediately() {
    let mut cfg = make_config("monitor@example.com", "pass");
    // Invalid config to cause a permanent error before TCP connect.
    cfg.monitor_email = None;

    let started = std::time::Instant::now();
    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await;

    let elapsed = started.elapsed();
    assert!(
        result.is_err(),
        "STEP-12.15 F2: permanent errors must propagate; expected Err, got Ok"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "STEP-12.15 F2: permanent errors must propagate immediately without retry delays; \
         elapsed {:?}", elapsed
    );
}
/// exercise the connect-retry path without a real TCP server.
struct FailingTcpFactory;

impl TcpTransportFactory for FailingTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        async { Err(std::io::Error::other("FailingTcpFactory: connect refused")) }
    }
}

/// TCP factory that vends a fresh `NoopTcpTransport` (with the default
/// udp_config push) on every `connect()` call. Unlike `StubTcpFactory`,
/// it never exhausts and can be called multiple times — needed by
/// reconnect tests where `start_all_inner` runs more than once.
struct RepeatableTcpFactory;

impl TcpTransportFactory for RepeatableTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        async { Ok(NoopTcpTransport::with_pending(Some(default_udp_config_push()))) }
    }
}

// --- Batch B tests -------------------------------------------------------

/// After a TCP connect failure the runtime must retry `start_all_inner`
/// with exponential back-off and emit `relay.runtime.connect_retry` carrying
/// `attempt` and `backoff_ms` fields before each retry. (STEP-12.14 §L5)
///
/// Red state: `start_with_all_deps` propagates the `TcpConnect` error
/// immediately without entering any retry loop; the trace event never fires.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn start_failure_triggers_exponential_backoff_retry() {
    let cfg = make_config("monitor@example.com", "pass");

    // FailingTcpFactory always errors; `start_all_inner` never succeeds.
    // With paused time, any `tokio::time::sleep` calls in the retry loop
    // complete in zero real time, so the first retry fires within the
    // 5-second advance below.
    let task = tokio::spawn(async move {
        let _ = RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            FailingTcpFactory,
            NoopUdpFactory,
        )
        .await;
    });

    // Advance the paused clock by 5 s; enough for the first retry's
    // 1200 ms back-off to elapse and fire at least one retry event.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    task.abort();
    let _ = task.await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.connect_retry",
        ),
        "STEP-12.14 §L5: start_with_all_deps must emit relay.runtime.connect_retry \
         before each retry attempt after a TCP connect failure; not found in log",
    );
    for field in ["attempt=1", "backoff_ms="] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", field),
            "STEP-12.14 §L5: relay.runtime.connect_retry must carry field {field:?} \
             — not found in log",
        );
    }
}

/// After successfully connecting to a TCP server, the runtime must remember
/// that server's IP. When the supervisor returns a session with a shuffled
/// server list on a reconnect, the runtime must prefer the remembered IP and
/// emit `relay.runtime.tcp_server_pinned`. (STEP-12.14 §L4)
///
/// Red state: the runtime does not track which TCP server was last used;
/// no pinning logic exists and the trace event never fires.
#[tokio::test]
#[tracing_test::traced_test]
async fn tcp_server_pinned_across_reconnects() {
    let cfg = make_config("monitor@example.com", "pass");

    // Initial session: 10.0.0.1 first; the runtime should pin to that IP.
    let initial_session = zwift_relay::RelaySession {
        aes_key: [0u8; 16],
        relay_id: 42,
        tcp_servers: vec![
            zwift_relay::TcpServer { ip: "10.0.0.1".into() },
            zwift_relay::TcpServer { ip: "10.0.0.2".into() },
        ],
        expires_at: tokio::time::Instant::now() + std::time::Duration::from_secs(3600),
        server_time_ms: Some(0),
    };
    // Shuffled session: 10.0.0.2 now first — Bb must still reconnect to
    // 10.0.0.1 because that was the previously-used server.
    let shuffled_session = zwift_relay::RelaySession {
        aes_key: [1u8; 16],
        relay_id: 43,
        tcp_servers: vec![
            zwift_relay::TcpServer { ip: "10.0.0.2".into() },
            zwift_relay::TcpServer { ip: "10.0.0.1".into() },
        ],
        expires_at: tokio::time::Instant::now() + std::time::Duration::from_secs(3600),
        server_time_ms: Some(0),
    };

    let (events_tx, _) = tokio::sync::broadcast::channel::<zwift_relay::SessionEvent>(16);
    let factory = StubSupervisorFactory::with_events_tx(
        initial_session,
        events_tx.clone(),
    );

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        factory,
        RepeatableTcpFactory,
        NoopUdpFactory,
    )
    .await
    .expect("start_with_all_deps must succeed");

    // Inject a LoggedIn event carrying the shuffled session. Under Bb (N14),
    // the runtime treats this as a reconnect trigger. L4 pinning must prefer
    // 10.0.0.1 even though 10.0.0.2 is now first in the server list.
    let _ = events_tx.send(zwift_relay::SessionEvent::LoggedIn(shuffled_session));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.tcp_server_pinned",
        ),
        "STEP-12.14 §L4: when a reconnect sees a shuffled server list, the \
         runtime must prefer the previously-used TCP server (10.0.0.1) and \
         emit relay.runtime.tcp_server_pinned; not found in log",
    );
}

/// A `SessionEvent::LoggedIn` carrying a new AES key must cause the runtime
/// to tear down the existing TCP and UDP channels and recreate them using
/// the new key, emitting `relay.runtime.channels_recreated`. (STEP-12.14 §N14)
///
/// Red state: the runtime either ignores the `LoggedIn` event or updates the
/// AES key in-place without recreating channels; the trace event never fires.
#[tokio::test]
#[tracing_test::traced_test]
async fn supervisor_relogin_recreates_channels_with_new_key() {
    let cfg = make_config("monitor@example.com", "pass");

    let (events_tx, _) = tokio::sync::broadcast::channel::<zwift_relay::SessionEvent>(16);
    let factory = StubSupervisorFactory::with_events_tx(
        fixture_session(),
        events_tx.clone(),
    );

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        factory,
        RepeatableTcpFactory,
        NoopUdpFactory,
    )
    .await
    .expect("start");

    // Inject a LoggedIn event with a different AES key. Under Bb (N14),
    // the runtime tears down and recreates TCP + UDP channels with the
    // new key, then emits relay.runtime.channels_recreated.
    let new_session = zwift_relay::RelaySession {
        aes_key: [0xABu8; 16],
        relay_id: 99,
        tcp_servers: vec![zwift_relay::TcpServer { ip: "127.0.0.1".into() }],
        expires_at: tokio::time::Instant::now() + std::time::Duration::from_secs(3600),
        server_time_ms: Some(0),
    };
    let _ = events_tx.send(zwift_relay::SessionEvent::LoggedIn(new_session));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.channels_recreated",
        ),
        "STEP-12.14 §N14: a SessionEvent::LoggedIn with a different AES key must \
         cause the runtime to tear down and recreate TCP + UDP channels, emitting \
         relay.runtime.channels_recreated; not found in log",
    );
}

/// On clean shutdown, the runtime must call `auth.logout()` and
/// `auth.leave()` best-effort and emit `relay.runtime.logout` and
/// `relay.runtime.leave`. Failures must not block exit. (STEP-12.14 §N9)
///
/// Red state: `RelayRuntime::shutdown()` tears down channels without
/// calling logout or leave; neither trace event fires.
#[tokio::test]
#[tracing_test::traced_test]
async fn clean_shutdown_sends_logout_and_leave() {
    let cfg = make_config("monitor@example.com", "pass");

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.logout",
        ),
        "STEP-12.14 §N9: shutdown must call auth.logout() (POST /api/users/logout) \
         and emit relay.runtime.logout; not found in log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.leave",
        ),
        "STEP-12.14 §N9: shutdown must call auth.leave() (POST /relay/worlds/1/leave) \
         and emit relay.runtime.leave; not found in log",
    );
}

// --- Batch C tests -------------------------------------------------------
//
// Cover STEP-12.14 §L1 (state-refresh fallback) and §L2 (suspend / resume
// on idle). Sauce's `_refreshStates` polls `getPlayerState` on a 3-30 s
// self-tuning interval; auto-suspends after 15 s of no fresh self-state;
// auto-resumes on incoming live data; and synthesizes "fake server
// packets" from polled state so downstream consumers always see fresh
// data. All four tests are red until Cb implements the refresher and
// suspend / resume hooks.

/// Auth stub that records every `get_player_state(id)` call's
/// (athlete_id, virtual-clock instant) and returns a configurable
/// `PlayerState`. Drives both the course gate (one call at startup)
/// and the state-refresher (recurring calls during the test).
struct PollingCounterAuth {
    polls: Arc<StdMutex<Vec<(i64, tokio::time::Instant)>>>,
    state: zwift_proto::PlayerState,
}

impl PollingCounterAuth {
    fn new(
        state: zwift_proto::PlayerState,
    ) -> (Self, Arc<StdMutex<Vec<(i64, tokio::time::Instant)>>>) {
        let polls = Arc::new(StdMutex::new(Vec::new()));
        (
            Self {
                polls: Arc::clone(&polls),
                state,
            },
            polls,
        )
    }
}

impl AuthLogin for PollingCounterAuth {
    async fn login(&self, _email: &str, _password: &str) -> Result<(), zwift_api::Error> {
        Ok(())
    }
    async fn athlete_id(&self) -> Result<i64, zwift_api::Error> {
        Ok(12345)
    }
    async fn get_player_state(
        &self,
        athlete_id: i64,
    ) -> Result<Option<zwift_proto::PlayerState>, zwift_api::Error> {
        self.polls
            .lock()
            .unwrap()
            .push((athlete_id, tokio::time::Instant::now()));
        Ok(Some(self.state.clone()))
    }
}

/// The state-refresher must poll `get_player_state(watched_id)` on a
/// self-tuning interval — 3 s minimum while the inbound stream is live,
/// expanding toward 30 s after 15 s of no inbound self-state.
/// (STEP-12.14 §L1, §Ca)
///
/// Red state: no state-refresher exists; only the course-gate calls
/// `get_player_state` once at startup, so the in-window count never
/// rises above 1.
#[tokio::test(start_paused = true)]
async fn state_refresh_polls_get_player_state_on_self_tuning_interval() {
    let cfg = make_config("monitor@example.com", "pass");
    let (auth, poll_log) = PollingCounterAuth::new(zwift_proto::PlayerState {
        world: Some(1),
        ..Default::default()
    });
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        auth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    // The course gate accounts for one call before start returns.
    let initial = poll_log.lock().unwrap().len();

    // 14 s of paused virtual time. Auto-advance fires the refresher's
    // pending timer between each iteration, so a 3-s minimum cadence
    // produces polls at t = 3, 6, 9, 12 → 4 additional calls.
    tokio::time::sleep(std::time::Duration::from_secs(14)).await;
    let after_initial = poll_log.lock().unwrap().len();
    let polls_in_first_14s = after_initial - initial;

    // 60 s of additional virtual time after the suspend threshold (15 s)
    // is crossed. With the 3-s cadence held flat we would see ~20 polls;
    // the self-tuning expansion toward 30 s must reduce that count.
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    let after_expansion = poll_log.lock().unwrap().len();
    let polls_in_next_60s = after_expansion - after_initial;

    runtime.shutdown();
    let _ = runtime.join().await;

    // Every refresher poll must target the WATCHED athlete (54321),
    // not the monitor account (12345). Sauce's `_refreshStates` calls
    // `getPlayerState(watchingId)`. (STEP-12.14 §R1 parity)
    let log = poll_log.lock().unwrap();
    for (id, _) in log.iter() {
        assert_eq!(
            *id,
            54321i64,
            "Batch C §Ca: refresher must poll the watched athlete's ID \
             (54321), NOT the monitor's (12345); saw {id}",
        );
    }
    drop(log);

    assert!(
        polls_in_first_14s >= 3,
        "Batch C §Ca: state-refresher must poll get_player_state at the \
         3-s minimum cadence; expected ≥3 refresher polls in 14s, got \
         {polls_in_first_14s} (course gate accounted for {initial} initial poll)",
    );
    assert!(
        polls_in_next_60s < 15,
        "Batch C §Ca: after 15s of no inbound state the refresh delay \
         must expand toward 30s; expected fewer than 15 polls in the \
         next 60s of suspended idle, got {polls_in_next_60s}",
    );
}

/// After 15 s with no inbound self-state, the daemon must auto-suspend
/// and emit `relay.runtime.suspended_idle`. Sauce's `_refreshStates`
/// calls `suspend()` when `age > 15000`. (STEP-12.14 §L2, §Ca)
///
/// Red state: no suspend logic exists; the trace event never fires.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn daemon_suspends_after_15s_of_no_self_state() {
    let cfg = make_config("monitor@example.com", "pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    // The NoopTcp transport delivers the udp_config push frame and then
    // blocks forever; no inbound self-state ever arrives. Advance past
    // the 15-s suspend threshold.
    tokio::time::sleep(std::time::Duration::from_secs(16)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.suspended_idle",
        ),
        "Batch C §Ca: after 15s of no inbound self-state the daemon \
         must auto-suspend and emit relay.runtime.suspended_idle; not \
         found in log",
    );
}

/// While suspended, an inbound self-state for the watched athlete must
/// resume the daemon and emit `relay.runtime.resumed`. Sauce's
/// `_updateSelfState` calls `resume()` on incoming live data.
/// (STEP-12.14 §L2)
///
/// Red state: suspend / resume are not implemented; no trace event fires.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn daemon_resumes_on_incoming_self_state_when_suspended() {
    let cfg = make_config("monitor@example.com", "pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    // Cross the 15-s suspend threshold first.
    tokio::time::sleep(std::time::Duration::from_secs(16)).await;

    // Inject a fresh inbound state for the watched athlete (id = 54321).
    // The recv-loop's Inbound arm must update the self-state timestamp
    // and call resume() when the daemon is currently suspended.
    let stc = zwift_proto::ServerToClient {
        states: vec![zwift_proto::PlayerState {
            id: Some(54321),
            world: Some(1),
            ..Default::default()
        }],
        ..Default::default()
    };
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));

    // Give the recv-loop a moment to process the injected event.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.resumed",
        ),
        "Batch C §Ca: an inbound self-state for the watched athlete must \
         resume a suspended daemon and emit relay.runtime.resumed; not \
         found in log",
    );
}

/// Each polled `PlayerState` from the state-refresher must be broadcast
/// as a `GameEvent::PlayerState` so downstream consumers see it the same
/// as if it had arrived on the wire. Sauce's `_refreshStates` synthesizes
/// "fake server packets" for exactly this reason. (STEP-12.14 §Ca)
///
/// Red state: no state-refresher exists, so polled states are never
/// broadcast and no `GameEvent::PlayerState` arrives.
#[tokio::test(start_paused = true)]
async fn state_refresh_synthesizes_fake_server_packet_from_polled_state() {
    let cfg = make_config("monitor@example.com", "pass");
    // Polled state carries concrete athlete_id + power + cadence + speed
    // so the synthesized GameEvent can be matched precisely.
    let (auth, _polls) = PollingCounterAuth::new(zwift_proto::PlayerState {
        id: Some(54321),
        world: Some(1),
        power: Some(213),
        cadence_u_hz: Some(1_500_000),
        speed: Some(40_000),
        ..Default::default()
    });
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        auth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    let mut events_rx = runtime.events();

    // Sleep past the first refresh tick (3 s) so at least one synthesized
    // packet has been broadcast.
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    let mut found_synthesized = false;
    while let Ok(event) = events_rx.try_recv() {
        if let GameEvent::PlayerState {
            athlete_id: 54321,
            power_w: 213,
            cadence_u_hz: 1_500_000,
            speed_mm_h: 40_000,
            ..
        } = event
        {
            found_synthesized = true;
            break;
        }
    }
    assert!(
        found_synthesized,
        "Batch C §Ca: each polled PlayerState must be broadcast as a \
         GameEvent::PlayerState (the \"fake server packet\" synthesis) \
         carrying the polled athlete_id / power / cadence / speed; \
         no matching event observed",
    );
}

// --- Batch D tests -------------------------------------------------------
//
// Cover STEP-12.14 §N8 (expungeReason logging), §M3 (TCP non-hello sends
// emit flags=0x00), §k1 (TCP hello at iv_seqno=0 omits the SEQNO flag),
// and §k2 (udp_config_vod_2 and flat udp_config are inert in pool
// extraction). All four tests are red until Db.

/// When an inbound STC carries `expunge_reason`, the recv-loop must
/// emit `relay.tcp.expunge_reason` so operators can see why the server
/// cut the session. (STEP-12.14 §N8)
///
/// Red state: the recv-loop ignores `stc.expunge_reason` entirely;
/// no trace event fires.
#[tokio::test]
#[tracing_test::traced_test]
async fn expunge_reason_is_logged_when_present() {
    let cfg = make_config("monitor@example.com", "pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    let stc = zwift_proto::ServerToClient {
        expunge_reason: Some(zwift_proto::ExpungeReason::WorldFull as i32),
        ..Default::default()
    };
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.expunge_reason",
        ),
        "Batch D §Da §N8: when an inbound STC carries `expunge_reason`, \
         the recv-loop must emit relay.tcp.expunge_reason at info so \
         operators can see why the server cut the session; not found in log",
    );
}

/// TCP non-hello sends must emit a header with `flags = 0x00` — no
/// `RELAY_ID`, `CONN_ID`, or `SEQNO` bits set (sauce parity, M3).
///
/// Red state: `tcp.rs::send_packet` non-hello branch sets
/// `flags: HeaderFlags::SEQNO`, so the wire's flags byte is `0x01`.
#[tokio::test]
async fn tcp_non_hello_send_emits_no_seqno_flag_in_header() {
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
    .expect("start");

    // The first write was the TCP hello sent at startup. Drive a
    // non-hello send so we have a second frame to inspect.
    let payload = zwift_proto::ClientToServer {
        seqno: Some(7),
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
        writes.len() >= 2,
        "Batch D §Da §M3: expected hello + non-hello writes; got {}",
        writes.len(),
    );
    // TCP wire layout: [length_hi, length_lo, flags_byte, ...].
    let flags_byte = writes[1][2];
    assert_eq!(
        flags_byte,
        0x00,
        "Batch D §Da §M3: TCP non-hello sends must emit a header with \
         flags=0x00 (no RELAY_ID, CONN_ID, or SEQNO bits set); got 0x{flags_byte:02x}",
    );
}

/// The first TCP hello is sent at `iv_seqno = 0`. Sauce omits the
/// `SEQNO` flag in that case (`(options.hello && iv.seqno) ||
/// options.forceSeq`), so the encoded header carries only
/// `RELAY_ID | CONN_ID = 0x06`. (STEP-12.14 §k1)
///
/// Red state: `tcp.rs::send_packet` hello branch unconditionally sets
/// `RELAY_ID | CONN_ID | SEQNO = 0x07` regardless of `iv_seqno`.
#[tokio::test]
async fn tcp_hello_omits_seqno_flag_when_iv_seqno_is_zero() {
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
    .expect("start");

    runtime.shutdown();
    let _ = runtime.join().await;

    let writes = written.lock().unwrap();
    assert!(
        !writes.is_empty(),
        "Batch D §Da §k1: expected at least one write (the TCP hello); none recorded",
    );
    // TCP wire layout: [length_hi, length_lo, flags_byte, ...]. The
    // first hello is sent at iv_seqno = 0, so the SEQNO flag (0x1)
    // must be omitted; encoded flags = RELAY_ID|CONN_ID = 0x06.
    let flags_byte = writes[0][2];
    assert_eq!(
        flags_byte,
        0x06,
        "Batch D §Da §k1: TCP hello at iv_seqno=0 must encode flags = \
         RELAY_ID|CONN_ID = 0x06 (no SEQNO bit); got 0x{flags_byte:02x}",
    );
}

/// `extract_udp_pools` must return `None` when only `udp_config_vod_2`
/// or only the flat `udp_config` are populated. Sauce's
/// `_udpServerPools` is updated only by `udp_config_vod_1`; the v2
/// and flat fallbacks are dead code in the production daemon.
/// (STEP-12.14 §k2)
///
/// Red state: `extract_udp_pools` walks v2 then flat, so injects of
/// either kind currently flow through to `pool_router.apply_pool_update`
/// and emit additional `relay.udp.pool_router.updated` events.
#[tokio::test]
#[tracing_test::traced_test]
async fn udp_config_v2_and_flat_fallback_paths_are_inert() {
    let cfg = make_config("monitor@example.com", "pass");
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("start");

    // Inject a vod_2-only push; with k2 parity this must NOT update
    // the pool router.
    let stc_vod2 = zwift_proto::ServerToClient {
        udp_config_vod_2: Some(zwift_proto::UdpConfigVod {
            relay_addresses_vod: vec![zwift_proto::RelayAddressesVod {
                lb_realm: Some(99),
                lb_course: Some(99),
                relay_addresses: vec![zwift_proto::RelayAddress {
                    ip: Some("10.99.99.99".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc_vod2)));

    // Inject a flat-`udp_config`-only push; same expectation.
    let stc_flat = zwift_proto::ServerToClient {
        udp_config: Some(zwift_proto::UdpConfig {
            relay_addresses: vec![zwift_proto::RelayAddress {
                ip: Some("10.88.88.88".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc_flat)));

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    // The initial vod_1 push from `default_udp_config_push()` is
    // consumed by `start_all_inner`'s wait-for-udp_config step, not
    // by `recv_loop`, so it does NOT emit `pool_router.updated`.
    // Under k2 parity, the two injects above must also produce zero
    // emissions because `extract_udp_pools` returns `None` for both.
    logs_assert(|lines| {
        let count = lines
            .iter()
            .filter(|l| l.contains("pool_router.updated"))
            .count();
        if count == 0 {
            Ok(())
        } else {
            Err(format!(
                "Batch D §Da §k2: udp_config_vod_2 and flat udp_config \
                 must be inert (extract_udp_pools returns None); expected \
                 zero relay.udp.pool_router.updated emissions, got {count}"
            ))
        }
    });
}

// ---------------------------------------------------------------------------
// Batch E — Tests (Ea, red state)
// STEP-12.14 covers N1, N12, C11.  The four tests below assert the wire
// format produced by the TCP and UDP paths once the proto fork (Eb) removes
// `required` markers from `ClientToServer` fields that sauce never encodes.
// ---------------------------------------------------------------------------

/// Walk raw protobuf bytes and return every top-level field tag number in
/// encounter order.  Wire types 0/1/2/5 are handled; any other wire type
/// stops the scan early.
fn proto_field_tags(mut bytes: &[u8]) -> Vec<u32> {
    fn read_varint(b: &[u8]) -> (u64, usize) {
        let mut v = 0u64;
        let mut shift = 0u32;
        for (i, &byte) in b.iter().enumerate() {
            v |= ((byte & 0x7F) as u64) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return (v, i + 1);
            }
        }
        (v, b.len())
    }
    let mut tags = Vec::new();
    while !bytes.is_empty() {
        let (key, n) = read_varint(bytes);
        if n == 0 {
            break;
        }
        bytes = &bytes[n..];
        tags.push((key >> 3) as u32);
        match key & 0x7 {
            0 => {
                let (_, n) = read_varint(bytes);
                if n == 0 { break; }
                bytes = &bytes[n..];
            }
            1 => {
                if bytes.len() < 8 { break; }
                bytes = &bytes[8..];
            }
            2 => {
                let (len, n) = read_varint(bytes);
                if n == 0 { break; }
                let skip = n + len as usize;
                if bytes.len() < skip { break; }
                bytes = &bytes[skip..];
            }
            5 => {
                if bytes.len() < 4 { break; }
                bytes = &bytes[4..];
            }
            _ => break,
        }
    }
    tags
}

/// Decrypt the first captured TCP write and return the raw `ClientToServer`
/// proto bytes (before prost decode).  The AES key used by all stub
/// transports is `[0u8; 16]`.
fn tcp_hello_proto_bytes(wire: &[u8]) -> Vec<u8> {
    let frame = &wire[2..]; // strip 2-byte big-endian length prefix
    let parsed = zwift_relay::decode_header(frame).expect("decode header");
    let aad = &frame[..parsed.consumed];
    let cipher = &frame[parsed.consumed..];
    let conn_id = parsed.header.conn_id.expect("TCP hello must carry conn_id");
    let seqno = parsed.header.seqno.unwrap_or(0);
    let iv = zwift_relay::RelayIv {
        device: zwift_relay::DeviceType::Relay,
        channel: zwift_relay::ChannelType::TcpClient,
        conn_id,
        seqno,
    };
    let plaintext = zwift_relay::decrypt(&[0u8; 16], &iv.to_bytes(), aad, cipher)
        .expect("decrypt TCP hello");
    let tcp = zwift_relay::parse_tcp_plaintext(&plaintext).expect("parse TCP plaintext");
    tcp.proto_bytes.to_vec()
}

/// TCP hello wire bytes must NOT encode `ClientToServer` tag 7 (`state`),
/// tag 10 (`last_update`), or tag 12 (`last_player_update`).
///
/// Sauce's `sayHello` constructs the packet with none of those fields set;
/// our proto marks them `required`, so prost unconditionally emits them even
/// at zero / default values.  After Eb changes them to `optional` and the
/// hello builder omits them, this test turns green.  (STEP-12.14 §N1/§N12)
///
/// Red state: all three tags appear in the wire bytes.
#[tokio::test]
async fn tcp_hello_wire_bytes_omit_state_last_update_last_player_update() {
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

    let writes = written.lock().unwrap().clone();
    assert!(!writes.is_empty(), "no TCP writes recorded");
    let tags = proto_field_tags(&tcp_hello_proto_bytes(&writes[0]));

    for forbidden in [7u32, 10, 12] {
        assert!(
            !tags.contains(&forbidden),
            "Batch E §N1/§N12: TCP hello proto must NOT encode tag {forbidden} \
             (7=state, 10=last_update, 12=last_player_update); \
             sauce's sayHello never includes these fields. \
             Tags present: {tags:?}",
        );
    }
}

/// TCP hello wire bytes must NOT encode tag 1 (`server_realm`).
///
/// Sauce's `TcpClient::sayHello` does not include `server_realm` in the
/// TCP hello packet; our implementation currently sets it to `1` because
/// the proto marks it `required`.  After Eb, the field is optional and the
/// hello builder omits it.  (STEP-12.14 §N1)
///
/// Red state: tag 1 appears in the wire bytes (value 1).
#[tokio::test]
async fn tcp_hello_wire_bytes_omit_realm() {
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

    let writes = written.lock().unwrap().clone();
    assert!(!writes.is_empty(), "no TCP writes recorded");
    let tags = proto_field_tags(&tcp_hello_proto_bytes(&writes[0]));

    assert!(
        !tags.contains(&1u32),
        "Batch E §N1: TCP hello proto must NOT encode tag 1 (server_realm); \
         sauce's TcpClient::sayHello carries no realm. Tags present: {tags:?}",
    );
}

// ==========================================================================
// Phase 3a — Tests for F3
// ==========================================================================

static MOCK_SERVERS_SPAWNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

fn ensure_mock_tcp_and_udp() {
    MOCK_SERVERS_SPAWNED.get_or_init(|| {
        tokio::spawn(async move {
            let _udp = tokio::net::UdpSocket::bind("127.0.0.1:3024").await.ok();
            if let Ok(tcp) = tokio::net::TcpListener::bind("127.0.0.1:3025").await {
                loop {
                    if let Ok((mut stream, _)) = tcp.accept().await {
                        use tokio::io::AsyncWriteExt;
                        let _ = stream.write_all(&default_udp_config_push()).await;
                    }
                }
            }
        });
    });
}

fn mock_login_response(relay_id: u32) -> Vec<u8> {
    use prost::Message;
    let resp = zwift_proto::LoginResponse {
        session_state: "ok".to_string(),
        info: zwift_proto::PerSessionInfo {
            relay_url: "https://us-or-rly101.zwift.com".to_string(),
            apis: None,
            time: Some(1_700_000_000_000),
            nodes: Some(zwift_proto::TcpConfig {
                nodes: vec![zwift_proto::TcpAddress {
                    ip: Some("127.0.0.1".to_string()),
                    port: Some(3025),
                    lb_realm: Some(0),
                    lb_course: Some(0),
                }],
            }),
            ..Default::default()
        },
        relay_session_id: Some(relay_id),
        expiration: Some(0), // forces 3-second refresh
        ..Default::default()
    };
    resp.encode_to_vec()
}

#[ignore = "slow: hard-coded 4.5 s sleep waiting for supervisor refresh / capture flush"]
#[tokio::test]
#[tracing_test::traced_test]
async fn start_with_writer_subscribes_to_real_supervisor_events() {
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    let server = MockServer::start().await;

    // Auth token
    Mock::given(method("POST"))
        .and(path(zwift_api::TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ATOK",
            "refresh_token": "RTOK",
            "expires_in": 600,
            "refresh_expires_in": 2400,
            "token_type": "Bearer",
        })))
        .mount(&server)
        .await;

    // Profile
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 12345})))
        .mount(&server)
        .await;

    // Course gate check
    use prost::Message;
    Mock::given(method("GET"))
        .and(path("/relay/worlds/1/players/54321"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(
            zwift_proto::PlayerState {
                world: Some(1),
                ..Default::default()
            }.encode_to_vec()
        ))
        .mount(&server)
        .await;

    // Session Login
    let login_resp = mock_login_response(42);
    Mock::given(method("POST"))
        .and(path(zwift_relay::LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(login_resp))
        .mount(&server)
        .await;

    // Refresh
    let refresh_resp = zwift_proto::RelaySessionRefreshResponse {
        relay_session_id: 42,
        expiration: 0,
    }.encode_to_vec();

    Mock::given(method("POST"))
        .and(path(zwift_relay::SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(refresh_resp))
        .mount(&server)
        .await;

    ensure_mock_tcp_and_udp();
    let mut cfg = make_config("monitor@example.com", "pass");
    cfg.zwift_endpoints.auth_base = server.uri();
    cfg.zwift_endpoints.api_base = server.uri();

    let task = tokio::spawn(async move {
        // TCP will fail because nothing is listening on 127.0.0.1:3025.
        // It enters the L5 retry loop, allowing the supervisor to run in the background.
        let _ = RelayRuntime::start_with_writer(&cfg, None).await;
    });

    // Wait 4.5 seconds for the supervisor to settle and refresh.
    tokio::time::sleep(std::time::Duration::from_millis(4500)).await;
    task.abort();
    let _ = task.await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.session.refreshed",
        ),
        "STEP-12.15 F3: start_with_writer must subscribe to real supervisor events; \
         expected relay.session.refreshed in trace after synthetic refresh"
    );
}

#[ignore = "slow: hard-coded 4.5 s sleep waiting for supervisor refresh / capture flush"]
#[tokio::test]
#[tracing_test::traced_test]
async fn start_with_writer_records_fresh_manifest_on_supervisor_relogin() {
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    let server = MockServer::start().await;

    // Auth token
    Mock::given(method("POST"))
        .and(path(zwift_api::TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ATOK",
            "refresh_token": "RTOK",
            "expires_in": 600,
            "refresh_expires_in": 2400,
            "token_type": "Bearer",
        })))
        .mount(&server)
        .await;

    // Profile
    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 12345})))
        .mount(&server)
        .await;

    // Course gate check
    use prost::Message;
    Mock::given(method("GET"))
        .and(path("/relay/worlds/1/players/54321"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(
            zwift_proto::PlayerState {
                world: Some(1),
                ..Default::default()
            }.encode_to_vec()
        ))
        .mount(&server)
        .await;

    // Initial Login
    let login_resp_1 = mock_login_response(42);

    // Fallback Login
    let login_resp_2 = mock_login_response(99);

    // Mock mapping for Login
    Mock::given(method("POST"))
        .and(path(zwift_relay::LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(login_resp_1))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(zwift_relay::LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(login_resp_2))
        .mount(&server)
        .await;

    // Refresh fails
    Mock::given(method("POST"))
        .and(path(zwift_relay::SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .mount(&server)
        .await;

    ensure_mock_tcp_and_udp();
    let mut cfg = make_config("monitor@example.com", "pass");
    cfg.zwift_endpoints.auth_base = server.uri();
    cfg.zwift_endpoints.api_base = server.uri();

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = std::sync::Arc::new(writer);

    let task = tokio::spawn(async move {
        let _ = RelayRuntime::start_with_writer(&cfg, Some(writer)).await;
    });

    // Wait 4.5 seconds for refresh to fail and trigger re-login.
    tokio::time::sleep(std::time::Duration::from_millis(4500)).await;
    task.abort();
    let _ = task.await;

    let mut reader = zwift_relay::capture::CaptureReader::open(path.path()).expect("reader");
    let mut manifest_count = 0;
    while let Some(item) = reader.next_item() {
        if matches!(item.expect("decode"), zwift_relay::capture::CaptureItem::Manifest(_)) {
            manifest_count += 1;
        }
    }

    assert!(
        manifest_count >= 2,
        "STEP-12.15 F3: a SessionEvent::LoggedIn with a fresh AES key must write \
         a new SessionManifest to the capture file. Expected >= 2 manifests, got {manifest_count}"
    );
}

// ==========================================================================
// STEP-12.16 §F6 Phase 1a — Course gate must suspend, not abort.
//
// Red state: `start_all_inner` returns `Err(WatchedAthleteNotInGame)` when
// the watched athlete has no course; the runtime never reaches `Ok(_)` and
// no `relay.runtime.suspended_no_course` trace fires. Each test below
// will fail until Phase 1b replaces the fatal branch with a suspended
// start (and Phase 2b defers UDP/heartbeat startup so the runtime can
// actually return).
// ==========================================================================

#[tokio::test]
#[tracing_test::traced_test]
async fn start_with_watched_athlete_not_in_game_starts_suspended() {
    let cfg = make_config("rider@example.com", "secret");

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        WatchedAthleteOfflineAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await;

    assert!(
        result.is_ok(),
        "STEP-12.16 §F6 Phase 1a: the runtime must start when the watched \
         athlete is online but not in a world (state.world = None); sauce4zwift \
         (zwift.mjs:1917-1922) suspends and waits in this state. Got: {:?}",
        result.as_ref().err(),
    );
    let runtime = result.unwrap();
    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.suspended_no_course",
        ),
        "STEP-12.16 §F6 Phase 1a: a `relay.runtime.suspended_no_course` \
         lifecycle event must fire when the daemon starts in the suspended \
         state because no course is yet known",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn start_with_watched_athlete_not_logged_in_starts_suspended() {
    let cfg = make_config("rider@example.com", "secret");

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        WatchedAthleteNoStateAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await;

    assert!(
        result.is_ok(),
        "STEP-12.16 §F6 Phase 1a: the runtime must start when \
         `get_player_state` returns `Ok(None)` (sauce4zwift's 404 branch at \
         zwift.mjs:613-622). Got: {:?}",
        result.as_ref().err(),
    );
    let runtime = result.unwrap();
    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.suspended_no_course",
        ),
        "STEP-12.16 §F6 Phase 1a: a `relay.runtime.suspended_no_course` \
         lifecycle event must fire when the player-state endpoint reports \
         the athlete is not logged in",
    );
}

/// The in-game path (state.world = Some(1)) must start without suspending.
/// StubAuth returns world = Some(1), so the course gate passes immediately
/// and the runtime proceeds to UDP and heartbeat setup as before Phase 1.
/// The absence of relay.runtime.suspended_no_course is not asserted here —
/// negative trace assertions are unreliable in parallel because other tests
/// that DO start suspended emit the same event and contaminate the global
/// log buffer.  The positive contract (startup succeeds + TCP + UDP) is
/// verified by the existing happy-path tests above.
#[tokio::test]
async fn start_with_watched_athlete_in_game_proceeds_normally() {
    let cfg = make_config("rider@example.com", "secret");

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
        result.is_ok(),
        "STEP-12.16 §F6 Phase 1a: the in-game start path must continue to \
         succeed unchanged; got {:?}",
        result.err(),
    );

    let runtime = result.unwrap();
    runtime.shutdown();
    let _ = runtime.join().await;
}

// ==========================================================================
// STEP-12.16 §F6 Phase 2a — UDP and heartbeat deferred on suspended start
//
// These tests verify that a suspended start (watched athlete not in game)
// brings TCP up normally but does NOT call udp_factory.connect() and does
// NOT emit relay.heartbeat.started.  Complements the Phase 1a lifecycle
// trace tests above and the course_gate.rs assertion that `connected`
// remains false.
// ==========================================================================

/// UDP factory connect must NOT be called when the daemon starts suspended.
/// Phase 2b wraps the UDP-connect block in `if let Some(course_id_val) =
/// course_id`, so `RecordingUdpFactory::connected` stays false.
#[tokio::test]
async fn suspended_start_does_not_create_udp_channel() {
    let cfg = make_config("rider@example.com", "secret");

    let (udp_factory, connected, _written) = RecordingUdpFactory::new();

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        WatchedAthleteOfflineAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await;

    assert!(
        result.is_ok(),
        "STEP-12.16 §F6 Phase 2a: suspended start must succeed; got {:?}",
        result.err(),
    );
    let runtime = result.unwrap();
    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        !*connected.lock().unwrap(),
        "STEP-12.16 §F6 Phase 2a: udp_factory.connect() must NOT be called \
         when the daemon starts suspended (watched athlete not in a game)",
    );
}

/// The heartbeat task must NOT be spawned on a suspended start.  The
/// heartbeat's only output is UDP packets; if no UDP packets are written
/// to a RecordingUdpFactory transport, the task was never started.
/// Using RecordingUdpFactory avoids the fragile negative tracing assertion
/// (relay.heartbeat.started fires in many other tests and pollutes the
/// global log buffer when tests run in parallel).
#[tokio::test]
async fn suspended_start_does_not_spawn_heartbeat() {
    let cfg = make_config("rider@example.com", "secret");

    let (udp_factory, _connected, written) = RecordingUdpFactory::new();

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        WatchedAthleteOfflineAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await;

    assert!(
        result.is_ok(),
        "STEP-12.16 §F6 Phase 2a: suspended start must succeed; got {:?}",
        result.err(),
    );
    let runtime = result.unwrap();
    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        written.lock().unwrap().is_empty(),
        "STEP-12.16 §F6 Phase 2a: no UDP packets must be written on a \
         suspended start — the heartbeat task is gated on the watched \
         athlete being in a known world",
    );
}

/// TCP must connect and establish normally on a suspended start.
/// Phase 2b defers only the UDP-side steps; the TCP channel runs regardless
/// of whether a course is known.  The relay.tcp.established positive
/// assertion is safe in parallel — it is emitted by this test's own
/// runtime.  The absence of relay.udp.established is already verified
/// by suspended_start_does_not_create_udp_channel (RecordingUdpFactory
/// shows connected = false); asserting it via the global log buffer
/// would be unreliable when other tests emit the same event concurrently.
#[tokio::test]
#[tracing_test::traced_test]
async fn suspended_start_still_establishes_tcp() {
    let cfg = make_config("rider@example.com", "secret");

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        WatchedAthleteOfflineAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await;

    assert!(
        result.is_ok(),
        "STEP-12.16 §F6 Phase 2a: suspended start must succeed; got {:?}",
        result.err(),
    );
    let runtime = result.unwrap();
    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.established",
        ),
        "STEP-12.16 §F6 Phase 2a: relay.tcp.established must fire on a \
         suspended start — TCP comes up regardless of whether a course is known",
    );
}

// ==========================================================================
// STEP-12.16 §F6 Phase 3a — resume UDP when the watched athlete enters a game
//
// Two paths can observe the athlete entering a game:
//   A. The state-refresher poll detects world changing from None to Some.
//   B. The recv-loop receives an inbound self-state with a world field.
//
// Both paths must call the (not-yet-implemented) resume_udp helper which
// connects UDP, spawns the heartbeat, and emits relay.runtime.resumed with
// a course_id field.  These tests are RED until Phase 3b implements that
// helper and wires it into both call sites.
// ==========================================================================

/// After the state refresher detects the watched athlete entering a game,
/// UDP must be connected and relay.runtime.resumed (with course_id) must fire.
///
/// Uses TransitioningAuth: call 1 (startup course gate) → world None (suspended
/// start); call 2 (first state-refresher poll) → world Some(7) (resume).
///
/// RED until Phase 3b implements resume_udp and wires it into run_state_refresher.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn state_refresher_resumes_when_watched_athlete_enters_game() {
    let cfg = make_config("rider@example.com", "secret");
    let (udp_factory, connected, _written) = RecordingUdpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        TransitioningAuth::new(),
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await
    .expect("suspended start must succeed (TransitioningAuth call 1 → world None)");

    // Advance past MIN_DELAY (3 s) so the state refresher fires its first
    // poll.  TransitioningAuth call 2 returns world = Some(7) which should
    // trigger resume_udp.
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        *connected.lock().unwrap(),
        "STEP-12.16 §F6 Phase 3a: UDP must be connected after the state \
         refresher observes the watched athlete entering a game \
         (TransitioningAuth second poll returns world = Some(7))",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.runtime.resumed"),
        "STEP-12.16 §F6 Phase 3a: relay.runtime.resumed must be emitted \
         when the state refresher detects the athlete entering a game",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "course_id"),
        "STEP-12.16 §F6 Phase 3a: relay.runtime.resumed must carry a \
         course_id field so operators can confirm which world was entered",
    );
}

/// After the recv-loop receives an inbound self-state for the watched athlete
/// carrying a world field, UDP must be connected.
///
/// Starts suspended (WatchedAthleteOfflineAuth → world None).  Injects an
/// inbound ServerToClient whose states list contains the watched athlete's
/// PlayerState with world = Some(7).
///
/// RED until Phase 3b wires resume_udp into the recv-loop's inbound-state
/// branch.
#[tokio::test(start_paused = true)]
async fn recv_loop_self_state_with_world_transitions_out_of_suspended() {
    let cfg = make_config("rider@example.com", "secret");
    let (udp_factory, connected, _written) = RecordingUdpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        WatchedAthleteOfflineAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await
    .expect("suspended start must succeed");

    // Inject inbound state for the watched athlete (id 54321 per make_config)
    // showing they have entered Watopia (world 7).
    let stc = zwift_proto::ServerToClient {
        states: vec![zwift_proto::PlayerState {
            id: Some(54321),
            world: Some(7),
            ..Default::default()
        }],
        ..Default::default()
    };
    // Yield before injecting so the recv-loop has a chance to process the
    // pending udp_config push from StubTcpFactory, which sets
    // inner.initial_udp_addr.  Without this yield the injected state
    // arrives before the udp_config event and the resume task cannot
    // pick an address.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));

    // Give the recv-loop and resume task time to connect UDP.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        *connected.lock().unwrap(),
        "STEP-12.16 §F6 Phase 3a: UDP must be connected after the recv-loop \
         receives an inbound state for the watched athlete with world = Some(7); \
         resume_udp must be called from the inbound-state branch",
    );
}

// ==========================================================================
// STEP-12.16 §F7 Phase 4a — TCP auto-reconnect on mid-session shutdown
//
// sauce4zwift (zwift.mjs:1869-1883) calls _schedConnectRetry() whenever
// the TCP channel emits a shutdown event while the daemon is running.
// Ranchero currently returns Ok(()) from recv_loop on
// TcpChannelEvent::Shutdown, which exits the orchestrator task and
// terminates the daemon.
//
// Tests 1 and 2 are RED until Phase 4b implements the reconnect loop.
// Test 3 documents the existing clean-shutdown behaviour; it is GREEN
// from the start and serves as a regression guard once Phase 4b lands.
// ==========================================================================

/// TCP factory that counts every call to connect() and always vends a fresh
/// transport with the default udp_config push.  The Arc<AtomicU32> counter
/// stays accessible after the factory is moved into start_with_all_deps,
/// so reconnect tests can read it back.
struct CountingRepeatableTcpFactory {
    connect_count: Arc<std::sync::atomic::AtomicU32>,
}

impl CountingRepeatableTcpFactory {
    fn new() -> (Self, Arc<std::sync::atomic::AtomicU32>) {
        let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        (Self { connect_count: Arc::clone(&count) }, count)
    }
}

impl TcpTransportFactory for CountingRepeatableTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        self.connect_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async { Ok(NoopTcpTransport::with_pending(Some(default_udp_config_push()))) }
    }
}

/// TCP factory whose connect sequence is: succeed (initial) → fail
/// ConnectionRefused (first reconnect attempt) → succeed (second reconnect).
/// The connect count is shared via Arc so assertions can read it back.
struct ReconnectSequenceTcpFactory {
    connect_count: Arc<std::sync::atomic::AtomicU32>,
}

impl ReconnectSequenceTcpFactory {
    fn new() -> (Self, Arc<std::sync::atomic::AtomicU32>) {
        let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        (Self { connect_count: Arc::clone(&count) }, count)
    }
}

impl TcpTransportFactory for ReconnectSequenceTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        let n = self
            .connect_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1; // 1-based
        async move {
            match n {
                1 => Ok(NoopTcpTransport::with_pending(Some(default_udp_config_push()))),
                2 => Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "reconnect refused",
                )),
                _ => Ok(NoopTcpTransport::with_pending(Some(default_udp_config_push()))),
            }
        }
    }
}

/// After a mid-session TcpChannelEvent::Shutdown the daemon must schedule a
/// reconnect and call tcp_factory.connect() a second time within the minimum
/// backoff window (1 s per the plan).  relay.tcp.reconnect.scheduled and a
/// second relay.tcp.established must both appear in the trace.
///
/// RED until Phase 4b implements the reconnect loop in recv_loop.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn tcp_channel_shutdown_mid_session_triggers_reconnect() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, tcp_connect_count) = CountingRepeatableTcpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        tcp_factory,
        NoopUdpFactory,
    )
    .await
    .expect("initial start must succeed");

    // Yield to let the runtime reach established state.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Simulate a mid-session TCP channel shutdown (e.g. server closed
    // the connection).
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Shutdown);

    // Advance past the minimum reconnect backoff (1 s) so the reconnect
    // attempt fires.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let count = tcp_connect_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        count,
        2,
        "STEP-12.16 §F7 Phase 4a: TCP must reconnect after a mid-session \
         Shutdown; tcp_factory.connect() must be called twice (initial + \
         reconnect). Got {count}",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.reconnect.scheduled",
        ),
        "STEP-12.16 §F7 Phase 4a: relay.tcp.reconnect.scheduled must be \
         emitted when a mid-session TCP shutdown triggers a reconnect",
    );

    runtime.shutdown();
    let _ = runtime.join().await;
}

/// When the first reconnect attempt fails the daemon must retry, incrementing
/// the attempt counter.  The trace must contain
/// relay.tcp.reconnect.attempt attempt=1 error=… and attempt=2.
///
/// RED until Phase 4b implements the reconnect loop.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn tcp_reconnect_increments_attempt_counter_on_repeated_failures() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, tcp_connect_count) = ReconnectSequenceTcpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        tcp_factory,
        NoopUdpFactory,
    )
    .await
    .expect("initial start must succeed");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Trigger reconnect.
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Shutdown);

    // Allow time for two reconnect attempts (1st fails, 2nd succeeds; each
    // with ~1 s backoff).
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let count = tcp_connect_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        count,
        3, // initial + 2 reconnect attempts
        "STEP-12.16 §F7 Phase 4a: expected 3 connect() calls (initial, \
         refused reconnect, successful reconnect); got {count}",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.reconnect.attempt",
        ),
        "STEP-12.16 §F7 Phase 4a: relay.tcp.reconnect.attempt must be \
         emitted on each reconnect attempt",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "attempt=1"),
        "STEP-12.16 §F7 Phase 4a: relay.tcp.reconnect.attempt must carry \
         attempt=1 on the first reconnect attempt",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "attempt=2"),
        "STEP-12.16 §F7 Phase 4a: relay.tcp.reconnect.attempt must carry \
         attempt=2 on the second reconnect attempt",
    );

    runtime.shutdown();
    let _ = runtime.join().await;
}

/// After runtime.shutdown() the daemon must not schedule any further TCP
/// reconnect attempts.  This test is GREEN from the start (existing behaviour)
/// and serves as a regression guard once Phase 4b lands.
#[tokio::test(start_paused = true)]
async fn tcp_reconnect_stops_on_explicit_shutdown() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, tcp_connect_count) = CountingRepeatableTcpFactory::new();

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        tcp_factory,
        NoopUdpFactory,
    )
    .await
    .expect("initial start must succeed");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Inject a Shutdown event, then immediately call shutdown() to prevent any
    // reconnect that Phase 4b would otherwise schedule.
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Shutdown);
    runtime.shutdown();
    let _ = runtime.join().await;

    // Allow any hypothetical reconnect window to pass.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let count = tcp_connect_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        count,
        1,
        "STEP-12.16 §F7 Phase 4a: explicit shutdown must prevent reconnect; \
         tcp_factory.connect() must be called exactly once (initial only). \
         Got {count}",
    );
}

// ==========================================================================
// STEP-12.16 §F8 Phase 5a — startup handshake timeout extension to 30 s
//
// sauce4zwift (zwift.mjs:1885-1923) wraps the entire session-activation
// sequence in a single 30 s race.  Ranchero currently has two separate 5 s
// deadlines: one for TcpChannelEvent::Established (which TcpChannel emits
// unconditionally, so the deadline never fires in practice) and one for the
// udp_config push over the TCP stream.
//
// Phase 5b replaces both with a single HANDSHAKE_BUDGET constant (30 s).
// On expiry the daemon must enter the F7 reconnect loop (not return a fatal
// error), emitting relay.tcp.reconnect.scheduled with reason="handshake_timeout".
//
// Tests 1-3 are RED because the current 5 s udp_config deadline fires before
// the push arrives.  Tests 4-5 are RED because the timeout currently returns
// a fatal error rather than routing to the reconnect path.
// ==========================================================================

/// TCP transport that delivers the udp_config push after a configurable delay.
/// Used to exercise the handshake budget: with `delay > current_deadline` the
/// startup times out; with `delay < new_budget` it succeeds.
struct DelayedUdpConfigTransport {
    delay: std::time::Duration,
    delivered: StdMutex<bool>,
}

impl zwift_relay::TcpTransport for DelayedUdpConfigTransport {
    async fn write_all(&self, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
        let already = {
            let mut lock = self.delivered.lock().unwrap();
            let was = *lock;
            if !was { *lock = true; }
            was
        };
        if !already {
            tokio::time::sleep(self.delay).await;
            return Ok(default_udp_config_push());
        }
        std::future::pending::<()>().await;
        unreachable!()
    }
}

struct DelayedUdpConfigTcpFactory {
    delay: std::time::Duration,
}

impl TcpTransportFactory for DelayedUdpConfigTcpFactory {
    type Transport = DelayedUdpConfigTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        let delay = self.delay;
        async move {
            Ok(DelayedUdpConfigTransport {
                delay,
                delivered: StdMutex::new(false),
            })
        }
    }
}

/// TCP factory whose first connect is silent (never delivers any frames,
/// causing a handshake timeout) and whose subsequent connects deliver a
/// normal udp_config push immediately.  Used by tests 4-5 to verify the
/// reconnect path after a handshake timeout.
struct SilentThenNormalTcpFactory {
    connect_count: Arc<std::sync::atomic::AtomicU32>,
}

impl SilentThenNormalTcpFactory {
    fn new() -> (Self, Arc<std::sync::atomic::AtomicU32>) {
        let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        (Self { connect_count: Arc::clone(&count) }, count)
    }
}

impl TcpTransportFactory for SilentThenNormalTcpFactory {
    type Transport = NoopTcpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        let n = self
            .connect_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        async move {
            if n == 1 {
                Ok(NoopTcpTransport::with_pending(None)) // silent — no udp_config push
            } else {
                Ok(NoopTcpTransport::with_pending(Some(default_udp_config_push())))
            }
        }
    }
}

/// When the udp_config push arrives 6 s after TCP connect, startup must
/// succeed with the new 30 s handshake budget.
///
/// RED until Phase 5b: current udp_config_deadline is 5 s, so this
/// fails with Err(NoUdpConfig(5s)).
#[tokio::test(start_paused = true)]
async fn udp_config_within_30s_budget_succeeds() {
    let cfg = make_config("rider@example.com", "secret");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(35),
        RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            DelayedUdpConfigTcpFactory {
                delay: std::time::Duration::from_secs(6),
            },
            NoopUdpFactory,
        ),
    )
    .await
    .expect("test must complete within 35 s");

    assert!(
        result.is_ok(),
        "STEP-12.16 §F8 Phase 5a: startup must succeed when the udp_config \
         push arrives 6 s after TCP connect — the 30 s handshake budget must \
         cover this.  Got: {:?}",
        result.err(),
    );

    if let Ok(runtime) = result {
        runtime.shutdown();
        let _ = runtime.join().await;
    }
}

/// With the combined 30 s budget, a udp_config push arriving at 25 s must
/// allow startup to complete successfully.
///
/// RED until Phase 5b: current 5 s deadline fires long before 25 s.
#[tokio::test(start_paused = true)]
async fn combined_handshake_timeout_is_30_seconds() {
    let cfg = make_config("rider@example.com", "secret");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(35),
        RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            DelayedUdpConfigTcpFactory {
                delay: std::time::Duration::from_secs(25),
            },
            NoopUdpFactory,
        ),
    )
    .await
    .expect("test must complete within 35 s");

    assert!(
        result.is_ok(),
        "STEP-12.16 §F8 Phase 5a: startup must succeed when the udp_config \
         push arrives 25 s after TCP connect (within the 30 s combined budget). \
         Got: {:?}",
        result.err(),
    );

    if let Ok(runtime) = result {
        runtime.shutdown();
        let _ = runtime.join().await;
    }
}

/// When the handshake budget expires without a udp_config push, the daemon
/// must route to the F7 reconnect loop rather than exiting with a fatal
/// error, so the overall start_with_all_deps call succeeds once the
/// reconnect delivers a valid session.
///
/// RED until Phase 5b: current code returns Err(NoUdpConfig) on timeout
/// and start_with_all_deps propagates this as a fatal error.
#[tokio::test(start_paused = true)]
async fn handshake_timeout_triggers_reconnect_not_exit() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, tcp_connect_count) = SilentThenNormalTcpFactory::new();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(65),
        RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            tcp_factory,
            NoopUdpFactory,
        ),
    )
    .await
    .expect("test must complete within 65 s (30 s budget + reconnect)");

    assert!(
        result.is_ok(),
        "STEP-12.16 §F8 Phase 5a: start_with_all_deps must succeed after a \
         handshake timeout triggers a reconnect; the second connect delivers \
         a valid session.  Got: {:?}",
        result.err(),
    );

    let count = tcp_connect_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        count >= 2,
        "STEP-12.16 §F8 Phase 5a: tcp_factory.connect() must be called at \
         least twice (silent first attempt + successful reconnect). Got {count}",
    );

    if let Ok(runtime) = result {
        runtime.shutdown();
        let _ = runtime.join().await;
    }
}

/// When the handshake budget expires, the daemon must emit
/// relay.tcp.reconnect.scheduled with reason="handshake_timeout" before
/// attempting the reconnect.
///
/// RED until Phase 5b: current code returns Err without emitting that trace.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn handshake_timeout_emits_reconnect_scheduled_with_reason() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, _) = SilentThenNormalTcpFactory::new();

    // With paused time the silent first connect causes the 5 s udp_config
    // deadline to fire almost instantly (no real wall time passes).
    // After Phase 5b the 30 s budget fires and the reconnect trace is emitted.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(65),
        RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            tcp_factory,
            NoopUdpFactory,
        ),
    )
    .await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.reconnect.scheduled",
        ),
        "STEP-12.16 §F8 Phase 5a: relay.tcp.reconnect.scheduled must be \
         emitted when the 30 s handshake budget expires",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "handshake_timeout",
        ),
        "STEP-12.16 §F8 Phase 5a: relay.tcp.reconnect.scheduled must carry \
         reason=\"handshake_timeout\" to distinguish a timeout from a \
         mid-session TCP disconnect",
    );
}

// ==========================================================================
// STEP-12.16 Phase 6a — trace-event audit
//
// These tests document the complete trace-event contract for all new
// lifecycle events added in Phases 1-5.  All four are GREEN from the start
// because the implementations landed alongside the feature phases.  They
// serve as regression guards so future changes cannot silently drop a
// lifecycle event.
// ==========================================================================

/// relay.runtime.suspended_no_course must be emitted at INFO when the daemon
/// starts with the watched athlete offline (no world in PlayerState).
/// Verifies the Phase 1 course-gate trace contract.
#[tokio::test]
#[tracing_test::traced_test]
async fn suspended_start_emits_runtime_suspended_no_course() {
    let cfg = make_config("rider@example.com", "secret");

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        WatchedAthleteOfflineAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await;

    assert!(result.is_ok(), "suspended start must succeed; got {:?}", result.err());
    let runtime = result.unwrap();
    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.runtime.suspended_no_course",
        ),
        "STEP-12.16 Phase 6a: relay.runtime.suspended_no_course must be \
         emitted at INFO when the daemon starts suspended (no course known)",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "watched_athlete_id"),
        "STEP-12.16 Phase 6a: relay.runtime.suspended_no_course must carry \
         the watched_athlete_id field so operators can identify the athlete",
    );
}

/// relay.runtime.resumed must be emitted with a course_id field when the
/// state refresher detects the watched athlete entering a game.
/// Verifies the Phase 3 resume trace contract.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn resume_emits_runtime_resumed_with_course_id() {
    let cfg = make_config("rider@example.com", "secret");

    // TransitioningAuth: call 1 (startup) → world None (suspended);
    // call 2 (first state-refresher poll after 3 s) → world Some(7).
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        TransitioningAuth::new(),
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        NoopUdpFactory,
    )
    .await
    .expect("suspended start must succeed");

    // Advance past MIN_DELAY (3 s) so the state refresher fires and
    // resume_udp brings UDP up.
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.runtime.resumed"),
        "STEP-12.16 Phase 6a: relay.runtime.resumed must be emitted when \
         the state refresher detects the watched athlete entering a game",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "course_id"),
        "STEP-12.16 Phase 6a: relay.runtime.resumed must carry a course_id \
         field so operators can confirm which world the athlete entered",
    );
}

/// The full TCP reconnect lifecycle must emit all three traces in order:
/// relay.tcp.reconnect.scheduled → relay.tcp.reconnect.attempt → relay.tcp.reconnect.succeeded.
/// Verifies the Phase 4 reconnect trace contract.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn tcp_reconnect_emits_full_lifecycle_traces() {
    let cfg = make_config("rider@example.com", "secret");

    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        RepeatableTcpFactory,
        NoopUdpFactory,
    )
    .await
    .expect("initial start must succeed");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Trigger a mid-session TCP disconnect.
    runtime.inject_tcp_event(zwift_relay::TcpChannelEvent::Shutdown);

    // Advance past the minimum backoff (1 s) so the reconnect fires.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    runtime.shutdown();
    let _ = runtime.join().await;

    for event in [
        "relay.tcp.reconnect.scheduled",
        "relay.tcp.reconnect.attempt",
        "relay.tcp.reconnect.succeeded",
    ] {
        assert!(
            tracing_test::internal::logs_with_scope_contain("ranchero", event),
            "STEP-12.16 Phase 6a: {event} must be emitted during the \
             reconnect lifecycle",
        );
    }
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "attempts="),
        "STEP-12.16 Phase 6a: relay.tcp.reconnect.succeeded must carry an \
         attempts field",
    );
}

/// relay.tcp.handshake.timeout must be emitted at WARN with phase and
/// elapsed_ms fields before the reconnect is scheduled.
/// Verifies the Phase 5 handshake-timeout trace contract.
#[tokio::test(start_paused = true)]
#[tracing_test::traced_test]
async fn handshake_timeout_emits_warn_event_before_reconnect() {
    let cfg = make_config("rider@example.com", "secret");
    let (tcp_factory, _) = SilentThenNormalTcpFactory::new();

    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(65),
        RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            tcp_factory,
            NoopUdpFactory,
        ),
    )
    .await;

    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.handshake.timeout",
        ),
        "STEP-12.16 Phase 6a: relay.tcp.handshake.timeout must be emitted \
         at WARN when the 30 s handshake budget expires",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "phase="),
        "STEP-12.16 Phase 6a: relay.tcp.handshake.timeout must carry a \
         phase field (\"established\" or \"udp_config\")",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "elapsed_ms="),
        "STEP-12.16 Phase 6a: relay.tcp.handshake.timeout must carry an \
         elapsed_ms field so operators know how long the budget ran",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain(
            "ranchero",
            "relay.tcp.reconnect.scheduled",
        ),
        "STEP-12.16 Phase 6a: relay.tcp.reconnect.scheduled must follow \
         relay.tcp.handshake.timeout — the timeout triggers a reconnect",
    );
}

// ==========================================================================
// STEP-12.30 Phase 2a — HTTP exchanges must appear in the capture file
// ==========================================================================

/// Verify that `start_with_writer` writes HTTP request and response bodies
/// into the capture file for the login sequence.
///
/// Currently RED for two reasons:
///
/// 1. `zwift_relay::capture::ContentType` and `CaptureRecord::content_type` do
///    not exist yet — the test fails to compile until Phase 2b adds them.
///
/// 2. Even after Phase 2b compiles, `start_with_writer` never calls
///    `ZwiftAuth::set_capture_sink` on the auth object it constructs (defect C,
///    `relay.rs:1326`). Phase 2d must add that call; only then will HTTP records
///    appear in the capture file.
#[ignore = "slow: hard-coded 1.5 s sleep waiting for daemon HTTP exchanges before abort"]
#[tokio::test]
async fn login_http_exchange_appears_in_capture() {
    use prost::Message;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path};

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(zwift_api::TOKEN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ATOK",
            "refresh_token": "RTOK",
            "expires_in": 600,
            "refresh_expires_in": 2400,
            "token_type": "Bearer",
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/profiles/me"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 12345})),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/relay/worlds/1/players/54321"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(
            zwift_proto::PlayerState { world: Some(1), ..Default::default() }.encode_to_vec(),
        ))
        .mount(&server)
        .await;

    let login_resp = mock_login_response(42);
    Mock::given(method("POST"))
        .and(path(zwift_relay::LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(login_resp))
        .mount(&server)
        .await;

    let refresh_resp = zwift_proto::RelaySessionRefreshResponse {
        relay_session_id: 42,
        expiration: 0,
    }
    .encode_to_vec();
    Mock::given(method("POST"))
        .and(path(zwift_relay::SESSION_REFRESH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(refresh_resp))
        .mount(&server)
        .await;

    ensure_mock_tcp_and_udp();

    let mut cfg = make_config("monitor@example.com", "pass");
    cfg.zwift_endpoints.auth_base = server.uri();
    cfg.zwift_endpoints.api_base = server.uri();

    let capture_file = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = Arc::new(
        zwift_relay::capture::CaptureWriter::open(capture_file.path())
            .await
            .expect("open capture writer"),
    );

    let task = {
        let writer = Arc::clone(&writer);
        let cfg = cfg.clone();
        tokio::spawn(async move {
            // TCP will fail at 127.0.0.1:3025 and the daemon enters the
            // retry loop, but login and course-gate HTTP exchanges have
            // already completed by then.
            let _ = RelayRuntime::start_with_writer(&cfg, Some(writer)).await;
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    task.abort();
    let _ = task.await;

    writer.flush_and_close().await.expect("flush capture writer");

    let mut reader = zwift_relay::capture::CaptureReader::open(capture_file.path())
        .expect("open reader");
    let mut http_records: Vec<zwift_relay::capture::CaptureRecord> = Vec::new();
    while let Some(item) = reader.next_item() {
        if let zwift_relay::capture::CaptureItem::Frame(record) = item.expect("decode ok") {
            if record.transport == zwift_relay::capture::TransportKind::Http {
                http_records.push(record);
            }
        }
    }

    assert!(
        !http_records.is_empty(),
        "STEP-12.30 Phase 2a: start_with_writer must write at least one HTTP \
         record to the capture file during login; got 0 records. \
         Defect C: start_with_writer (relay.rs:1326) never calls \
         set_capture_sink on the ZwiftAuth it constructs. \
         Phase 2d must add: auth.set_capture_sink(Arc::new(HttpCaptureSink(Arc::clone(&writer))))",
    );

    assert!(
        http_records.iter().any(|r| !r.payload.is_empty()),
        "STEP-12.30 Phase 2a: at least one HTTP record must carry a non-empty payload",
    );

    // This assertion references CaptureRecord::content_type and
    // ContentType::Unspecified, which do not exist until Phase 2b adds
    // the content_type byte to the v3 record header.
    assert!(
        http_records.iter().any(|r| {
            r.content_type != zwift_relay::capture::ContentType::Unspecified
        }),
        "STEP-12.30 Phase 2a: at least one HTTP record must carry a ContentType \
         field other than Unspecified (e.g. UrlEncoded for the token-grant request \
         body, Json for responses)",
    );
}
