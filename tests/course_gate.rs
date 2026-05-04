// SPDX-License-Identifier: AGPL-3.0-only
//
// STEP-12.14 Phase 4a — daemon course gate tests (C2 + R1 + C9).
//
// Sauce calls `this.api.getPlayerState(this.selfAthleteId)` BEFORE
// establishing TCP, then gates UDP setup on the watched athlete being in
// a game (`this.courseId != null` from `state.world`, proto tag 35).
//
// This file lives in `tests/` rather than alongside `relay_runtime.rs` so
// that compile failures caused by referencing the planned 4b APIs
// (`AuthLogin::get_player_state`, `RelayRuntimeError::NoWatchedAthlete`)
// do not block the existing relay_runtime test suite.
//
// All four tests are red until Phase 4b:
//   1. `AuthLogin` gains a `get_player_state` method.
//   2. `RelayRuntimeError::NoWatchedAthlete` is added.
//   3. `start_all_inner` inserts the course-gate step.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use ranchero::config::{EditingMode, RedactedString, ResolvedConfig, ZwiftEndpoints};
use ranchero::daemon::relay::{
    AuthLogin, RelayRuntime, RelayRuntimeError, SessionSupervisorFactory,
    SessionSupervisorHandle, TcpTransportFactory, UdpTransportFactory,
};

// --- config helper -------------------------------------------------------

fn make_config(email: &str, password: &str, watched_id: Option<u64>) -> ResolvedConfig {
    ResolvedConfig {
        main_email: None,
        main_password: None,
        monitor_email: Some(email.to_string()),
        monitor_password: Some(RedactedString::new(password.to_string())),
        server_bind: "127.0.0.1".into(),
        server_port: 1080,
        server_https: false,
        log_level: None,
        log_file: PathBuf::from("/tmp/ranchero-course-gate-test.log"),
        pidfile: PathBuf::from("/tmp/ranchero-course-gate-test.pid"),
        config_path: None,
        editing_mode: EditingMode::Default,
        zwift_endpoints: ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        relay_enabled: true,
        watched_athlete_id: watched_id,
    }
}

// --- stubs ---------------------------------------------------------------

/// Auth stub that returns a configurable `PlayerState` from
/// `get_player_state` so the daemon's course-gate logic can be driven
/// without a real HTTP server.
///
/// Fails to compile in red state because `AuthLogin::get_player_state`
/// does not exist on the trait yet (STEP-12.14 §4b adds it).
struct PlayerStateAuth {
    athlete_id: i64,
    player_state: Option<zwift_proto::PlayerState>,
    get_player_state_called_with: Arc<StdMutex<Option<i64>>>,
}

impl PlayerStateAuth {
    fn new(
        athlete_id: i64,
        player_state: Option<zwift_proto::PlayerState>,
    ) -> (Self, Arc<StdMutex<Option<i64>>>) {
        let called_with = Arc::new(StdMutex::new(None));
        (
            Self {
                athlete_id,
                player_state,
                get_player_state_called_with: Arc::clone(&called_with),
            },
            called_with,
        )
    }
}

impl AuthLogin for PlayerStateAuth {
    async fn login(&self, _email: &str, _password: &str) -> Result<(), zwift_api::Error> {
        Ok(())
    }

    async fn athlete_id(&self) -> Result<i64, zwift_api::Error> {
        Ok(self.athlete_id)
    }

    // STEP-12.14 §4b adds this method to `AuthLogin`. The impl below
    // drives the course-gate tests; it compiles once the trait is extended.
    async fn get_player_state(
        &self,
        athlete_id: i64,
    ) -> Result<Option<zwift_proto::PlayerState>, zwift_api::Error> {
        *self.get_player_state_called_with.lock().unwrap() = Some(athlete_id);
        Ok(self.player_state.clone())
    }
}

// --- shared test infrastructure (reuse patterns from relay_runtime.rs) ---

struct StubSupervisor(zwift_relay::RelaySession);
impl SessionSupervisorHandle for StubSupervisor {
    fn current(
        &self,
    ) -> impl std::future::Future<Output = zwift_relay::RelaySession> + Send {
        let s = self.0.clone();
        async move { s }
    }
    fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<zwift_relay::SessionEvent> {
        tokio::sync::broadcast::channel(1).1
    }
    fn shutdown(&self) {}
}

struct StubSupervisorFactoryLocal(zwift_relay::RelaySession);
impl SessionSupervisorFactory for StubSupervisorFactoryLocal {
    type Handle = StubSupervisor;
    fn start(
        &self,
    ) -> impl std::future::Future<
        Output = Result<Self::Handle, RelayRuntimeError>,
    > + Send {
        let s = self.0.clone();
        async move { Ok(StubSupervisor(s)) }
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

// Noop TCP transport that delivers a default udp_config push.
struct NormalTcpTransport {
    pending: StdMutex<Option<Vec<u8>>>,
}
impl zwift_relay::TcpTransport for NormalTcpTransport {
    async fn write_all(&self, _b: &[u8]) -> std::io::Result<()> { Ok(()) }
    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
        if let Some(f) = self.pending.lock().unwrap().take() { return Ok(f); }
        std::future::pending::<()>().await;
        unreachable!()
    }
}
struct NormalTcpFactory;
impl TcpTransportFactory for NormalTcpFactory {
    type Transport = NormalTcpTransport;
    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        async { Ok(NormalTcpTransport { pending: StdMutex::new(Some(default_udp_push())) }) }
    }
}

fn default_udp_push() -> Vec<u8> {
    use prost::Message as _;
    let stc = zwift_proto::ServerToClient {
        udp_config_vod_1: Some(zwift_proto::UdpConfigVod {
            relay_addresses_vod: vec![zwift_proto::RelayAddressesVod {
                lb_realm: Some(0),
                lb_course: Some(0),
                relay_addresses: vec![zwift_proto::RelayAddress {
                    lb_realm: Some(0),
                    lb_course: Some(0),
                    ip: Some("127.0.0.1".to_string()),
                    port: None,
                    ra_f5: None,
                    ra_f6: None,
                }],
                rav_f4: None,
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let proto_bytes = stc.encode_to_vec();
    let header = zwift_relay::Header {
        flags: zwift_relay::HeaderFlags::CONN_ID | zwift_relay::HeaderFlags::SEQNO,
        relay_id: None,
        conn_id: Some(0),
        seqno: Some(0),
    };
    let header_bytes = header.encode();
    let iv = zwift_relay::RelayIv {
        device: zwift_relay::DeviceType::Relay,
        channel: zwift_relay::ChannelType::TcpServer,
        conn_id: 0,
        seqno: 0,
    };
    let cipher = zwift_relay::encrypt(&[0u8; 16], &iv.to_bytes(), &header_bytes, &proto_bytes);
    // TCP frames carry a 2-byte big-endian length prefix; without it
    // the daemon's `recv_loop` cannot demux the chunk and never emits
    // the `Inbound` event the wait-for-udp_config step needs.
    zwift_relay::frame_tcp(&header_bytes, &cipher)
}

struct ConnectCapturingUdp {
    connected: Arc<StdMutex<bool>>,
}
impl UdpTransportFactory for ConnectCapturingUdp {
    type Transport = zwift_relay::TokioUdpTransport;
    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        *self.connected.lock().unwrap() = true;
        async { Err(std::io::Error::other("stub — connect intentionally fails after recording")) }
    }
    fn channel_config(&self) -> zwift_relay::UdpChannelConfig {
        zwift_relay::UdpChannelConfig { max_hellos: 0, ..Default::default() }
    }
}

// --- tests ---------------------------------------------------------------

/// The daemon must call `auth.get_player_state(cfg.watched_athlete_id)` —
/// specifically with the WATCHED athlete's ID, not the monitor account's
/// `auth.athlete_id()`. (STEP-12.14 §C2 / R1)
#[tokio::test]
async fn start_all_inner_calls_get_player_state_with_watched_athlete_id() {
    let cfg = make_config("monitor@example.com", "pass", Some(99_999));
    let (auth, called_with) = PlayerStateAuth::new(
        12345, // monitor's athlete_id
        Some(zwift_proto::PlayerState {
            world: Some(7), // watched athlete IS in a game
            ..Default::default()
        }),
    );
    let connected = Arc::new(StdMutex::new(false));
    let udp = ConnectCapturingUdp { connected: Arc::clone(&connected) };

    let _ = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        auth,
        StubSupervisorFactoryLocal(fixture_session()),
        NormalTcpFactory,
        udp,
    )
    .await;

    let id = *called_with.lock().unwrap();
    assert_eq!(
        id,
        Some(99_999i64),
        "STEP-12.14 §C2 / R1: get_player_state must be called with \
         cfg.watched_athlete_id (99_999), NOT the monitor's athlete_id \
         (12345). Got {id:?}",
    );
}

/// When the watched athlete has no course (`state.world = None`), the
/// daemon must NOT connect UDP — it should suspend and surface a typed
/// error rather than attempting a connection that the server would ignore.
#[tokio::test]
async fn start_all_inner_suspends_when_watched_athlete_has_no_course() {
    let cfg = make_config("monitor@example.com", "pass", Some(42));
    let (auth, _) = PlayerStateAuth::new(
        12345,
        Some(zwift_proto::PlayerState {
            world: None, // athlete not in a game — no course
            ..Default::default()
        }),
    );
    let connected = Arc::new(StdMutex::new(false));
    let udp = ConnectCapturingUdp { connected: Arc::clone(&connected) };

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        auth,
        StubSupervisorFactoryLocal(fixture_session()),
        NormalTcpFactory,
        udp,
    )
    .await;

    assert!(
        !*connected.lock().unwrap(),
        "STEP-12.14 §C2: udp_factory.connect() must NOT be called when \
         the watched athlete has no course (state.world = None)",
    );
    let err_msg = match result {
        Ok(_) => panic!(
            "STEP-12.14 §C2: start must return an error when watched athlete \
             is not in a game; got Ok",
        ),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.to_lowercase().contains("game")
            || err_msg.to_lowercase().contains("course")
            || err_msg.to_lowercase().contains("suspend"),
        "STEP-12.14 §C2: error must mention game/course/suspend; got {err_msg:?}",
    );
}

/// When the watched athlete IS in a game (state.world = Some(7)), the
/// daemon must proceed past the course gate and attempt UDP setup.
#[tokio::test]
async fn start_all_inner_proceeds_when_watched_athlete_has_course() {
    let cfg = make_config("monitor@example.com", "pass", Some(42));
    let (auth, _) = PlayerStateAuth::new(
        12345,
        Some(zwift_proto::PlayerState {
            world: Some(7), // course 7 = Watopia
            ..Default::default()
        }),
    );
    let connected = Arc::new(StdMutex::new(false));
    let udp = ConnectCapturingUdp { connected: Arc::clone(&connected) };

    let _ = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        auth,
        StubSupervisorFactoryLocal(fixture_session()),
        NormalTcpFactory,
        udp,
    )
    .await;

    assert!(
        *connected.lock().unwrap(),
        "STEP-12.14 §C2: udp_factory.connect() must be called when the \
         watched athlete is in a game (state.world = Some(7)); course gate \
         must not block the UDP connect",
    );
}

/// When `cfg.watched_athlete_id` is None the daemon must refuse to start
/// with a clear `RelayRuntimeError::NoWatchedAthlete` error. Trying to
/// call `getPlayerState(None)` would be a no-op in sauce because
/// `initPlayerState` only runs when `selfAthleteId != null`.
///
/// Fails to compile in red state because `RelayRuntimeError::NoWatchedAthlete`
/// does not exist yet.
#[tokio::test]
async fn start_all_inner_errors_when_watched_athlete_id_not_configured() {
    let cfg = make_config("monitor@example.com", "pass", None); // no watched_athlete_id
    let (auth, _) = PlayerStateAuth::new(12345, None);

    let result = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        auth,
        StubSupervisorFactoryLocal(fixture_session()),
        NormalTcpFactory,
        ConnectCapturingUdp { connected: Arc::new(StdMutex::new(false)) },
    )
    .await;

    match result {
        Err(RelayRuntimeError::NoWatchedAthlete) => {}
        Err(e) => panic!(
            "STEP-12.14 §C2: start must return Err(NoWatchedAthlete) when \
             cfg.watched_athlete_id is None; got Err({e})",
        ),
        Ok(_) => panic!(
            "STEP-12.14 §C2: start must return Err(NoWatchedAthlete) when \
             cfg.watched_athlete_id is None; got Ok",
        ),
    }
}
