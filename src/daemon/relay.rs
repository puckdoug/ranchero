//! STEP-12.1 — Relay runtime orchestrator.
//!
//! This module is in the red-state phase: the public surface and
//! the test module are present, but the internal logic is not yet
//! implemented. Every method on `RelayRuntime` panics with
//! `unimplemented!()`, so every test in the inline test module
//! fails until STEP-12.1's implementation lands.
//!
//! See `docs/plans/STEP-12.1-tcp-end-to-end-smoke.md` for the
//! design and `docs/plans/STEP-12-game-monitor.md` for the parent
//! plan.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// Build a [`zwift_relay::capture::SessionManifest`] from a live
/// [`zwift_relay::RelaySession`] plus the channel `conn_id`. Called
/// from `start_all_inner` and from the supervisor-event handler so a
/// `--capture` reader can recover the AES key + IV state needed to
/// decrypt the frames that follow.
fn manifest_from_session(
    session: &zwift_relay::RelaySession,
    conn_id: u32,
) -> zwift_relay::capture::SessionManifest {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let remaining = session
        .expires_at
        .saturating_duration_since(tokio::time::Instant::now());
    let expires_at_unix_ns = (now_unix + remaining).as_nanos() as u64;
    zwift_relay::capture::SessionManifest {
        aes_key: session.aes_key,
        // DeviceType::Relay is 1; the IV's per-direction channel byte
        // varies by transport so we leave channel_type as a sentinel
        // 0 ("unspecified") and let the replay tool derive the right
        // value from direction + transport on each frame.
        device_type: 1,
        channel_type: 0,
        send_iv_seqno_tcp: 0,
        recv_iv_seqno_tcp: 0,
        send_iv_seqno_udp: 0,
        recv_iv_seqno_udp: 0,
        relay_id: session.relay_id,
        conn_id,
        expires_at_unix_ns,
    }
}

/// Pick the initial UDP target from a `udp_config` push. Returns the
/// first entry whose `ip` is set and whose `(ip, port)` parses as a
/// valid `SocketAddr`. Falls back to [`zwift_relay::UDP_PORT_SECURE`]
/// when the entry has no port.
///
/// STEP-12.13 §D3: this is intentionally a "first valid" pick rather
/// than a pool-router lookup. The watched-athlete state is empty at
/// initial-connect time (no `ServerToClient.states` have arrived
/// yet), so per-realm/per-course routing has nothing to discriminate
/// on. Mid-session pool updates and routing live in a follow-up
/// step.
fn pick_initial_udp_target(
    addrs: &[zwift_proto::RelayAddress],
) -> Option<std::net::SocketAddr> {
    for a in addrs {
        let ip = match a.ip.as_deref() {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        // STEP-12.14 §C5 — sauce's UDPChannel.establish line 1338 hardcodes
        // port 3024 (the encrypted/secure port) regardless of what the proto
        // says. `RelayAddress.port` (tag 4) carries the PLAINTEXT port (default
        // 3022); connecting there with AES-128-GCM-4 hellos produces
        // `os error 61: Connection refused`. Always use the secure port.
        if let Ok(addr) = format!("{ip}:{}", zwift_relay::UDP_PORT_SECURE).parse() {
            return Some(addr);
        }
    }
    None
}

/// Emit a `relay.state.change` info event and broadcast the matching
/// `GameEvent::StateChange`. Tracks `prev_state` so the event carries
/// both the previous and new discriminant names.
fn emit_state_change(
    game_events_tx: &tokio::sync::broadcast::Sender<GameEvent>,
    prev_state: &mut Option<RuntimeState>,
    new_state: RuntimeState,
) {
    let from = match prev_state.as_ref() {
        Some(s) => format!("{s:?}"),
        None => "None".to_string(),
    };
    let to = format!("{new_state:?}");
    tracing::info!(
        target: "ranchero::relay",
        from = %from,
        to = %to,
        "relay.state.change",
    );
    let _ = game_events_tx.send(GameEvent::StateChange(new_state.clone()));
    *prev_state = Some(new_state);
}

use crate::config::ResolvedConfig;

// STEP-12.14 §N2 — sauce's NetChannel subclasses (TCPChannel, UDPChannel)
// each have their own `static _connInc = 0` counter so TCP and UDP start
// at connId=0 independently. We previously shared one counter; split into two.
static TCP_CONN_ID_COUNTER: AtomicU32 = AtomicU32::new(0);
static UDP_CONN_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Returns the next TCP channel connection ID, wrapping at 0xffff.
pub fn next_tcp_conn_id() -> u16 {
    (TCP_CONN_ID_COUNTER.fetch_add(1, Ordering::Relaxed) % 0xffff) as u16
}

/// Returns the next UDP channel connection ID, wrapping at 0xffff.
pub fn next_udp_conn_id() -> u16 {
    (UDP_CONN_ID_COUNTER.fetch_add(1, Ordering::Relaxed) % 0xffff) as u16
}

#[derive(Error, Debug)]
pub enum RelayRuntimeError {
    #[error("missing monitor account email; configure via `ranchero configure`")]
    MissingEmail,

    #[error("missing monitor account password; store one via `ranchero configure`")]
    MissingPassword,

    #[error("auth: {0}")]
    Auth(zwift_api::Error),

    #[error("relay session: {0}")]
    Session(zwift_relay::SessionError),

    #[error("TCP channel: {0}")]
    TcpChannel(zwift_relay::TcpError),

    #[error("relay session reported no TCP servers")]
    NoTcpServers,

    #[error("capture writer I/O: {0}")]
    CaptureIo(std::io::Error),

    #[error("invalid TCP server address `{0}`")]
    BadTcpAddress(String),

    #[error("TCP connect: {0}")]
    TcpConnect(std::io::Error),

    #[error("UDP connect: {0}")]
    UdpConnect(std::io::Error),

    #[error("UDP channel: {0}")]
    UdpChannel(zwift_relay::UdpError),

    #[error("TCP channel did not emit Established within {0:?}")]
    EstablishedTimeout(std::time::Duration),

    #[error("no udp_config received from TCP stream within {0:?}")]
    NoUdpConfig(std::time::Duration),

    /// A `udp_config_vod*` push arrived but contained no generic
    /// load-balancer pool (`lb_course=0`). Sauce always uses
    /// `_udpServerPools.get(0)` for the initial UDP connect; without
    /// it we would silently pick a per-course pool that may reject
    /// athletes who are not on that course. (STEP-12.14 §C1)
    #[error("udp_config push contains no lb_course=0 generic pool")]
    NoGenericPool,

    /// `cfg.watched_athlete_id` was not set when the daemon tried to
    /// start. Sauce gates UDP setup on `getPlayerState(selfAthleteId)`
    /// and only proceeds when an athlete to observe is configured.
    /// (STEP-12.14 §C2 / R1)
    #[error("no watched athlete configured; set one via `ranchero configure`")]
    NoWatchedAthlete,

    /// `getPlayerState` for the watched athlete returned no
    /// `state.world` (proto tag 35), which means the athlete is not
    /// currently in a game. The daemon refuses to bring UDP up in
    /// this state because the relay would have no course context to
    /// route on. (STEP-12.14 §C2)
    #[error("watched athlete is not in a game (no course); waiting to resume")]
    WatchedAthleteNotInGame,
}

impl From<zwift_api::Error> for RelayRuntimeError {
    fn from(e: zwift_api::Error) -> Self { RelayRuntimeError::Auth(e) }
}

impl From<zwift_relay::SessionError> for RelayRuntimeError {
    fn from(e: zwift_relay::SessionError) -> Self { RelayRuntimeError::Session(e) }
}

impl From<zwift_relay::TcpError> for RelayRuntimeError {
    fn from(e: zwift_relay::TcpError) -> Self { RelayRuntimeError::TcpChannel(e) }
}

/// Dependency-injection trait for the auth-login step. The default
/// implementation in `RelayRuntime::start` delegates to
/// `zwift_api::ZwiftAuth`. Tests substitute a stub.
pub trait AuthLogin: Send + Sync + 'static {
    fn login(
        &self,
        email: &str,
        password: &str,
    ) -> impl std::future::Future<Output = Result<(), zwift_api::Error>> + Send;

    /// Returns the authenticated account's Zwift athlete ID (profile ID).
    /// Populated by `login` via `GET /api/profiles/me`. Every implementor
    /// must provide an explicit override — there is no default so that a
    /// missing implementation fails at compile time rather than silently
    /// sending `player_id = 0` in relay packets.
    fn athlete_id(
        &self,
    ) -> impl std::future::Future<Output = Result<i64, zwift_api::Error>> + Send;

    /// Fetch the current `PlayerState` for `athlete_id` from the
    /// `/relay/worlds/1/players/{id}` endpoint. Returns `Ok(None)`
    /// when the athlete is not currently in a game (HTTP 404). Used
    /// by the `start_all_inner` course-gate (STEP-12.14 §C2 / R1) to
    /// learn the watched athlete's `state.world` (proto tag 35) before
    /// the daemon brings UDP up.
    fn get_player_state(
        &self,
        athlete_id: i64,
    ) -> impl std::future::Future<
        Output = Result<Option<zwift_proto::PlayerState>, zwift_api::Error>,
    > + Send;
}

/// Dependency-injection trait for the relay-session login step.
/// Returns the session handle on success.
pub trait SessionLogin: Send + Sync + 'static {
    fn login(
        &self,
    ) -> impl std::future::Future<
        Output = Result<zwift_relay::RelaySession, zwift_relay::SessionError>,
    > + Send;
}

/// Dependency-injection trait for the TCP transport factory. The
/// default implementation delegates to `TokioTcpTransport::connect`.
pub trait TcpTransportFactory: Send + Sync + 'static {
    type Transport: zwift_relay::TcpTransport;
    fn connect(
        &self,
        addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send;
}

/// Dependency-injection trait for the UDP transport factory.
/// The default production implementation constructs a
/// `zwift_relay::TokioUdpTransport`. Tests substitute a recording
/// stub that tracks which address was passed to `connect` and
/// captures every datagram passed to `send`.
///
/// Defect 4 (red state): this trait exists but `start_with_all_deps`
/// does not yet call `connect`. The trait is present so that the
/// integration tests can compile and fail at the assertion level.
pub trait UdpTransportFactory: Send + Sync + 'static {
    type Transport: zwift_relay::UdpTransport;
    fn connect(
        &self,
        addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send;

    /// Returns the `UdpChannelConfig` to use when establishing the
    /// channel. The default matches the production constants. Test
    /// factories override this to set `max_hellos: 0` so the SNTP
    /// hello loop is bypassed when the transport never responds.
    fn channel_config(&self) -> zwift_relay::UdpChannelConfig {
        zwift_relay::UdpChannelConfig::default()
    }
}

/// Object-safe wrapper for `TcpChannel::send_packet`. Lets
/// `RelayRuntime` hold a type-erased `Arc<dyn TcpSend>` so the channel
/// can be shared between `recv_loop` and `send_tcp` without making
/// `RelayRuntime` generic.
trait TcpSend: Send + Sync + 'static {
    fn send_packet<'a>(
        &'a self,
        payload: zwift_proto::ClientToServer,
        hello: bool,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), RelayRuntimeError>> + Send + 'a>,
    >;
}

impl<T: zwift_relay::TcpTransport> TcpSend for zwift_relay::TcpChannel<T> {
    fn send_packet<'a>(
        &'a self,
        payload: zwift_proto::ClientToServer,
        hello: bool,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), RelayRuntimeError>> + Send + 'a>,
    > {
        Box::pin(async move {
            zwift_relay::TcpChannel::send_packet(self, payload, hello)
                .await
                .map_err(RelayRuntimeError::TcpChannel)
        })
    }
}

/// `HeartbeatSink` implementation backed by a `UdpChannel`. Extracts
/// the `PlayerState` from the scheduler-built `ClientToServer` and
/// forwards it to `UdpChannel::send_player_state`, which re-wraps it
/// with the channel's own seqno and IV state.
struct UdpHeartbeatSink<T: zwift_relay::UdpTransport>(Arc<zwift_relay::UdpChannel<T>>);

impl<T: zwift_relay::UdpTransport> HeartbeatSink for UdpHeartbeatSink<T> {
    fn send(
        &self,
        payload: zwift_proto::ClientToServer,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send {
        let ch = Arc::clone(&self.0);
        async move {
            ch.send_player_state(payload.state)
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))
        }
    }
}

/// Read-only view over a supervisor-managed relay session. The
/// production implementation delegates to
/// `zwift_relay::RelaySessionSupervisor`. Tests substitute a stub
/// that returns a pre-configured session and emits pre-loaded events.
pub trait SessionSupervisorHandle: Send + Sync + 'static {
    fn current(
        &self,
    ) -> impl std::future::Future<Output = zwift_relay::RelaySession> + Send;

    fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<zwift_relay::SessionEvent>;

    fn shutdown(&self);
}

/// Dependency-injection factory for the relay-session supervisor.
/// The default production implementation calls
/// `RelaySessionSupervisor::start`. Tests substitute a stub
/// factory whose `start` returns a handle with pre-loaded events.
///
/// Defect 7 (red state): this trait exists and `start_with_all_deps`
/// calls `start()` to obtain the initial session, but does not yet
/// subscribe to the event broadcast. Tests that assert log records for
/// `relay.session.refreshed` therefore fail.
pub trait SessionSupervisorFactory: Send + Sync + 'static {
    type Handle: SessionSupervisorHandle;
    fn start(
        &self,
    ) -> impl std::future::Future<Output = Result<Self::Handle, RelayRuntimeError>> + Send;
}

/// Internal state owned by the runtime, shared between the
/// recv-loop task and any test-only injection points.
///
/// In production builds the orchestrator does not yet auto-update
/// these fields from inbound TCP messages (the recv-loop only
/// emits `GameEvent::PlayerState` for every observed state today).
/// Until that auto-extraction lands, the fields are written via
/// the `#[cfg(test)]` injection methods only, so production builds
/// never read them. The `dead_code` allowance documents that
/// asymmetry rather than silently masking a real defect.
#[derive(Debug)]
#[allow(dead_code)]
struct RuntimeInner {
    pool_router: std::sync::Mutex<UdpPoolRouter>,
    watched_state: std::sync::Mutex<WatchedAthleteState>,
    current_udp_server: std::sync::Mutex<Option<std::net::SocketAddr>>,
}

/// The orchestrator owned by the daemon. `start` performs the auth
/// and relay-session login synchronously, opens the capture writer
/// if a path is given, then spawns the recv-loop task.
pub struct RelayRuntime {
    #[allow(dead_code)]
    join_handle: JoinHandle<Result<(), RelayRuntimeError>>,
    #[allow(dead_code)]
    shutdown:    Arc<Notify>,
    /// Forwarded broadcast of every `TcpChannelEvent` observed by
    /// the runtime. The runtime forwards the channel's own events
    /// here on a dedicated task; tests use [`Self::inject_event`]
    /// to publish synthetic events for assertion purposes.
    #[allow(dead_code)]
    events_tx: tokio::sync::broadcast::Sender<zwift_relay::TcpChannelEvent>,
    /// Broadcast surface for downstream consumers (web/WS server,
    /// per-athlete data model). Lifecycle `GameEvent::StateChange`
    /// records are emitted before `start_with_deps` returns.
    #[allow(dead_code)]
    game_events_tx: tokio::sync::broadcast::Sender<GameEvent>,
    /// Broadcast surface for synthetic UDP events. The recv-loop
    /// subscribes to this and emits `relay.udp.*` tracing records.
    /// Tests use [`Self::inject_udp_event`].
    #[allow(dead_code)]
    udp_events_tx: tokio::sync::broadcast::Sender<zwift_relay::ChannelEvent>,
    /// Routing state: pool table, watched-athlete state, and the
    /// currently-selected UDP server. Shared with the recv-loop
    /// task and exposed to tests via the injection methods below.
    #[allow(dead_code)]
    inner: Arc<RuntimeInner>,
    /// Type-erased handle to the live TCP channel. `None` when the
    /// runtime was started via the older `start_with_deps` path that
    /// does not yet use `SessionSupervisorFactory`.
    tcp_sender: Option<Arc<dyn TcpSend>>,
    /// Abort handle for the UDP heartbeat task spawned by
    /// `start_all_inner`. `None` on the older start paths.
    heartbeat_abort: Option<tokio::task::AbortHandle>,
    /// Abort handle for the session-event subscriber task spawned by
    /// `start_all_inner`. `None` on the older start paths.
    supervisor_event_abort: Option<tokio::task::AbortHandle>,
}

// ---------------------------------------------------------------------------
// STEP-12.3 — Heartbeat scheduler (stub).
// ---------------------------------------------------------------------------

/// Sink for outbound `ClientToServer` heartbeats. The
/// production implementation wraps a `UdpChannel`; tests use a
/// recording stub.
pub trait HeartbeatSink: Send + Sync + 'static {
    fn send(
        &self,
        payload: zwift_proto::ClientToServer,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send;
}

/// 1 Hz UDP heartbeat that sends a `ClientToServer` carrying the
/// watched athlete's `PlayerState`. The scheduler owns the seqno
/// and reads `world_time` from the shared `WorldTimer`. Required
/// to keep the server-side TCP connection alive (spec §7.12).
pub struct HeartbeatScheduler<T: HeartbeatSink> {
    sink: T,
    world_timer: zwift_relay::WorldTimer,
    interval: std::time::Duration,
    seqno: std::sync::atomic::AtomicU32,
    athlete_id: i64,
}

impl<T: HeartbeatSink> HeartbeatScheduler<T> {
    /// Build a scheduler. The default interval is 1 Hz; tests
    /// may override with `with_interval`.
    pub fn new(sink: T, world_timer: zwift_relay::WorldTimer, athlete_id: i64) -> Self {
        Self {
            sink,
            world_timer,
            interval: std::time::Duration::from_secs(1),
            seqno: std::sync::atomic::AtomicU32::new(0),
            athlete_id,
        }
    }

    pub fn with_interval(mut self, interval: std::time::Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Current seqno; reflects the number of heartbeats already
    /// sent.
    pub fn seqno(&self) -> u32 {
        self.seqno.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Build the heartbeat payload for the next send. Increments
    /// the seqno and reads the current `world_time`.
    fn next_payload(&self) -> zwift_proto::ClientToServer {
        let next_seqno = self
            .seqno
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        zwift_proto::ClientToServer {
            server_realm: 1,
            player_id: self.athlete_id,
            world_time: Some(self.world_timer.now()),
            seqno: Some(next_seqno),
            state: zwift_proto::PlayerState::default(),
            last_update: 0,
            last_player_update: 0,
            ..Default::default()
        }
    }

    /// Send a single heartbeat. Used by both the scheduler's
    /// internal loop and by tests that want to exercise the
    /// payload-construction logic without a tokio interval.
    pub async fn send_one(&self) -> std::io::Result<()> {
        let payload = self.next_payload();
        self.sink.send(payload).await
    }

    /// Run the 1 Hz scheduler until cancelled. The loop never
    /// terminates on its own; the caller must abort the spawned
    /// task to stop it.
    pub async fn run(&self) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let interval_ms = self.interval.as_millis() as u64;
        // The first tick fires immediately; advance past it so the
        // first heartbeat lands one interval after start, matching
        // the "1 Hz" expectation more naturally.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match self.send_one().await {
                Ok(()) => {
                    tracing::debug!(
                        target: "ranchero::relay",
                        interval_ms,
                        send_ok = true,
                        "relay.heartbeat.tick",
                    );
                }
                Err(e) => {
                    tracing::debug!(
                        target: "ranchero::relay",
                        interval_ms,
                        send_ok = false,
                        "relay.heartbeat.tick",
                    );
                    tracing::warn!(
                        target: "ranchero::relay",
                        error = %e,
                        "relay.heartbeat.send_failed",
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// STEP-12.4 — `udpConfigVOD` parsing and pool routing (stubs).
// ---------------------------------------------------------------------------

/// One pool of UDP servers, scoped to a `(realm, courseId)` pair.
/// Mirrors `UDPServerVODPool` from the proto definitions.
///
/// STEP-12.4 stub: full field set is fleshed out with the
/// implementation. The minimum present here lets the tests
/// reference the type.
#[derive(Debug, Clone)]
pub struct UdpServerPool {
    pub realm: i32,
    pub course_id: i32,
    pub use_first_in_bounds: bool,
    pub servers: Vec<UdpServerEntry>,
}

#[derive(Debug, Clone)]
pub struct UdpServerEntry {
    pub addr: std::net::SocketAddr,
    pub x_bound_min: f64,
    pub x_bound: f64,
    pub y_bound_min: f64,
    pub y_bound: f64,
}

/// Selects the appropriate UDP server from a pool given the
/// watched athlete's `(x, y)` position. Ports
/// `zwift.mjs:2295-2317`. With `use_first_in_bounds` set, the
/// first server whose bounding box contains the position is
/// returned. Otherwise, or if no server is in bounds, the result
/// is the server whose bound centre minimises the Euclidean
/// distance to the position.
pub fn find_best_udp_server(
    pool: &UdpServerPool,
    x: f64,
    y: f64,
) -> Option<&UdpServerEntry> {
    if pool.servers.is_empty() {
        return None;
    }
    if pool.use_first_in_bounds {
        for server in &pool.servers {
            if x >= server.x_bound_min
                && x <= server.x_bound
                && y >= server.y_bound_min
                && y <= server.y_bound
            {
                return Some(server);
            }
        }
    }
    pool.servers.iter().min_by(|a, b| {
        let da = euclidean_distance_to_centre(a, x, y);
        let db = euclidean_distance_to_centre(b, x, y);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn euclidean_distance_to_centre(server: &UdpServerEntry, x: f64, y: f64) -> f64 {
    let cx = (server.x_bound_min + server.x_bound) / 2.0;
    let cy = (server.y_bound_min + server.y_bound) / 2.0;
    let dx = cx - x;
    let dy = cy - y;
    (dx * dx + dy * dy).sqrt()
}

/// Maintains a per-`(realm, courseId)` table of UDP server pools.
/// Updates arrive as inbound `udpConfigVOD` messages on TCP; the
/// latest update for a given key replaces the previous entry.
#[derive(Debug, Default)]
pub struct UdpPoolRouter {
    pools: std::collections::HashMap<(i32, i32), UdpServerPool>,
}

impl UdpPoolRouter {
    pub fn new() -> Self {
        Self { pools: std::collections::HashMap::new() }
    }

    /// Apply an inbound `udpConfigVOD` update. Replaces any
    /// existing pool for the same `(realm, courseId)` key.
    pub fn apply_pool_update(&mut self, pool: UdpServerPool) {
        let key = (pool.realm, pool.course_id);
        self.pools.insert(key, pool);
    }

    /// Look up the pool for a given `(realm, courseId)`.
    pub fn pool_for(&self, realm: i32, course_id: i32) -> Option<&UdpServerPool> {
        self.pools.get(&(realm, course_id))
    }
}

// ---------------------------------------------------------------------------
// STEP-12.5 — Idle suspension FSM, watched-athlete state, GameEvent.
// ---------------------------------------------------------------------------

/// States in the idle-suspension FSM. Per spec §4.13.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleState {
    /// The watched athlete is in motion or has been recently;
    /// UDP is active.
    Active,
    /// The watched athlete has been stationary; the suspension
    /// timer is running.
    Idle,
    /// The suspension timer has fired and UDP is shut down.
    Suspended,
}

/// Default idle window per spec §4.13. The implementation matches
/// sauce4zwift's constant; if a future audit of the upstream
/// source reveals a different value, this constant is the only
/// place that needs to be updated.
const IDLE_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

/// Per spec §4.13: when the watched athlete shows zero motion for
/// approximately 60 s, suspend UDP. Resume on any motion.
#[derive(Debug)]
pub struct IdleFSM {
    state: IdleState,
    idle_elapsed: std::time::Duration,
    idle_window: std::time::Duration,
}

impl Default for IdleFSM {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleFSM {
    pub fn new() -> Self {
        Self {
            state: IdleState::Active,
            idle_elapsed: std::time::Duration::ZERO,
            idle_window: IDLE_WINDOW,
        }
    }

    /// For tests that need a shorter idle window than 60 s.
    #[cfg(test)]
    pub fn with_idle_window(window: std::time::Duration) -> Self {
        Self {
            state: IdleState::Active,
            idle_elapsed: std::time::Duration::ZERO,
            idle_window: window,
        }
    }

    pub fn current(&self) -> IdleState {
        self.state
    }

    /// Apply a motion observation: `(speed, cadence, power)` from
    /// the watched athlete's `PlayerState`. Any non-zero field
    /// returns the FSM to `Active`; all-zero motion when in
    /// `Active` transitions to `Idle` and starts the suspension
    /// timer.
    pub fn observe_motion(&mut self, speed: i32, cadence: i32, power: i32) {
        let in_motion = speed != 0 || cadence != 0 || power != 0;
        if in_motion {
            self.state = IdleState::Active;
            self.idle_elapsed = std::time::Duration::ZERO;
            return;
        }
        // Zero motion observed.
        if self.state == IdleState::Active {
            self.state = IdleState::Idle;
            self.idle_elapsed = std::time::Duration::ZERO;
        }
        // In `Idle` or `Suspended` we remain in the current state
        // until a timer tick or a fresh motion observation drives
        // the next transition.
    }

    /// Apply a tick: advance the suspension timer when in `Idle`.
    /// Returns `true` if a state transition occurred (specifically,
    /// the `Idle → Suspended` transition).
    pub fn tick(&mut self, elapsed: std::time::Duration) -> bool {
        if self.state == IdleState::Idle {
            self.idle_elapsed += elapsed;
            if self.idle_elapsed >= self.idle_window {
                self.state = IdleState::Suspended;
                return true;
            }
        }
        false
    }
}

/// State for the currently watched athlete. Updated from inbound
/// `PlayerState` messages.
#[derive(Debug, Clone, Default)]
pub struct WatchedAthleteState {
    pub athlete_id: i64,
    pub realm: i32,
    pub course_id: i32,
    pub position: (f64, f64),
}

impl WatchedAthleteState {
    pub fn for_athlete(athlete_id: i64) -> Self {
        Self {
            athlete_id,
            realm: 0,
            course_id: 0,
            position: (0.0, 0.0),
        }
    }

    /// Switch the watched athlete. Clears the cached
    /// `(realm, courseId, x, y)` so that the next observed
    /// `PlayerState` for the new athlete repopulates it.
    pub fn switch_to(&mut self, new_athlete_id: i64) {
        self.athlete_id = new_athlete_id;
        self.realm = 0;
        self.course_id = 0;
        self.position = (0.0, 0.0);
    }
}

/// High-level events emitted by the orchestrator for downstream
/// consumers (web/WS server, the per-athlete data model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    PlayerState {
        athlete_id: i64,
        power_w: i32,
        cadence_u_hz: i32,
        speed_mm_h: u32,
        world_time_ms: i64,
    },
    Latency {
        latency_ms: i64,
        server_addr: std::net::SocketAddr,
    },
    StateChange(RuntimeState),
    /// Emitted when the orchestrator selects a different UDP
    /// server in response to a pool update, a watched-athlete
    /// position change, a course change, or a watched-athlete
    /// switch.
    PoolSwap {
        from: Option<std::net::SocketAddr>,
        to: std::net::SocketAddr,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeState {
    Authenticating,
    SessionLoggedIn,
    TcpEstablished,
    UdpEstablished,
    UdpSuspended,
    ShuttingDown,
}

impl RelayRuntime {
    /// Build the runtime. Returns once the TCP channel has emitted
    /// its first `Established` event, or once login fails.
    ///
    /// This is the production entry point. It constructs the default
    /// dependency-injection types from `zwift_api` and `zwift_relay`
    /// and delegates to [`Self::start_with_deps`]. The auth handle is
    /// shared between the auth-login and session-login DI types via
    /// `Arc` so that the bearer token deposited by the OAuth login
    /// is visible to the relay-session login.
    ///
    /// The HTTPS endpoints (`auth_base`, `api_base`) are read from
    /// `cfg.zwift_endpoints` rather than `zwift_api::Config::default()`,
    /// so an operator (or a test) can redirect the daemon to a
    /// staging environment, a self-hosted relay, or a localhost
    /// mock by setting the `[zwift]` section in the config file or
    /// the `RANCHERO_ZWIFT_AUTH_BASE` / `RANCHERO_ZWIFT_API_BASE`
    /// environment variables. See
    /// `docs/plans/STEP-12.5-still-not-doing-the-job-as-specified.md`
    /// §F. Tests that want to substitute the entire DI chain use
    /// `start_with_deps` directly with stub types.
    pub async fn start(
        cfg: &ResolvedConfig,
        capture_path: Option<PathBuf>,
    ) -> Result<Self, RelayRuntimeError> {
        let auth_config = zwift_api::Config {
            auth_base:  cfg.zwift_endpoints.auth_base.clone(),
            api_base:   cfg.zwift_endpoints.api_base.clone(),
            source:     zwift_api::DEFAULT_SOURCE.to_string(),
            user_agent: zwift_api::DEFAULT_USER_AGENT.to_string(),
            platform:   "OSX".to_string(),
        };
        let auth = Arc::new(zwift_api::ZwiftAuth::new(auth_config));
        let session_config = zwift_relay::RelaySessionConfig::default();
        let (game_events_tx, _) = tokio::sync::broadcast::channel::<GameEvent>(64);

        // Pre-open the capture file before auth so the file is created
        // regardless of whether the session succeeds (STEP-12.5 §B).
        let preopen_writer: Option<Arc<zwift_relay::capture::CaptureWriter>> =
            match capture_path {
                Some(ref path) => {
                    let writer = zwift_relay::capture::CaptureWriter::open(path)
                        .await
                        .map_err(RelayRuntimeError::CaptureIo)?;
                    tracing::info!(target: "ranchero::relay", ?path, "relay.capture.opened");
                    Some(Arc::new(writer))
                }
                None => None,
            };

        match Self::start_all_inner(
            cfg,
            None,
            preopen_writer.clone(),
            DefaultAuthLogin::new(auth.clone()),
            DefaultSessionSupervisorFactory::new(auth, session_config),
            DefaultTcpTransportFactory,
            DefaultUdpTransportFactory,
            game_events_tx,
        )
        .await
        {
            Ok(this) => Ok(this),
            Err(e) => {
                if let Some(writer) = preopen_writer {
                    // Drain the writer so the file is left readable.
                    // The writer task emits `relay.capture.writer.closed`
                    // (STEP-12.12 §3b) on its own as it shuts down — no
                    // need for a duplicate daemon-side log line.
                    let _ = writer.flush_and_close().await;
                }
                Err(e)
            }
        }
    }

    /// Production entry point used by the daemon when a capture file was
    /// pre-opened by `validate_startup`. Routes through `start_all_inner`
    /// with all production factories, emitting the full lifecycle event
    /// sequence. Emits `relay.capture.opened` on success and, on the error
    /// path, flushes the writer (which causes the writer task itself to
    /// emit `relay.capture.writer.closed`) before propagating.
    pub async fn start_with_writer(
        cfg: &ResolvedConfig,
        capture_writer: Option<Arc<zwift_relay::capture::CaptureWriter>>,
    ) -> Result<Self, RelayRuntimeError> {
        let auth_config = zwift_api::Config {
            auth_base:  cfg.zwift_endpoints.auth_base.clone(),
            api_base:   cfg.zwift_endpoints.api_base.clone(),
            source:     zwift_api::DEFAULT_SOURCE.to_string(),
            user_agent: zwift_api::DEFAULT_USER_AGENT.to_string(),
            platform:   "OSX".to_string(),
        };
        let auth = Arc::new(zwift_api::ZwiftAuth::new(auth_config));
        let session_config = zwift_relay::RelaySessionConfig::default();
        let (game_events_tx, _) = tokio::sync::broadcast::channel::<GameEvent>(64);

        if capture_writer.is_some() {
            tracing::info!(target: "ranchero::relay", "relay.capture.opened");
        }

        match Self::start_all_inner(
            cfg,
            None,
            capture_writer.clone(),
            DefaultAuthLogin::new(auth.clone()),
            DefaultSessionSupervisorFactory::new(auth, session_config),
            DefaultTcpTransportFactory,
            DefaultUdpTransportFactory,
            game_events_tx,
        )
        .await
        {
            Ok(this) => Ok(this),
            Err(e) => {
                if let Some(writer) = capture_writer {
                    // The writer task emits `relay.capture.writer.closed`
                    // on its own when `flush_and_close` drains the queue
                    // (STEP-12.12 §3b); no duplicate daemon-side line.
                    let _ = writer.flush_and_close().await;
                }
                Err(e)
            }
        }
    }

    /// The dependency-injected entry point used by tests. Performs
    /// credential validation, then drives the auth → session → TCP
    /// connect → channel-establish sequence using the supplied
    /// dependencies. Returns once the channel emits `Established`,
    /// the channel emits a different event first (treated as a
    /// failure), or any earlier step returns an error.
    pub async fn start_with_deps<A, S, F>(
        cfg: &ResolvedConfig,
        capture_path: Option<PathBuf>,
        auth: A,
        session_factory: S,
        tcp_factory: F,
    ) -> Result<Self, RelayRuntimeError>
    where
        A: AuthLogin,
        S: SessionLogin,
        F: TcpTransportFactory,
    {
        let (game_events_tx, _) = tokio::sync::broadcast::channel::<GameEvent>(64);
        Self::start_with_deps_and_events_tx(
            cfg,
            capture_path,
            auth,
            session_factory,
            tcp_factory,
            game_events_tx,
        )
        .await
    }

    /// As [`Self::start_with_deps`], but accepts an externally
    /// constructed `GameEvent` sender. Tests subscribe a receiver
    /// from this sender before calling, so that lifecycle
    /// `StateChange` events emitted during `start_with_deps` can
    /// be observed.
    pub async fn start_with_deps_and_events_tx<A, S, F>(
        cfg: &ResolvedConfig,
        capture_path: Option<PathBuf>,
        auth: A,
        session_factory: S,
        tcp_factory: F,
        game_events_tx: tokio::sync::broadcast::Sender<GameEvent>,
    ) -> Result<Self, RelayRuntimeError>
    where
        A: AuthLogin,
        S: SessionLogin,
        F: TcpTransportFactory,
    {
        // Open a fresh capture writer from the path if one was
        // given; the pre-opened-writer entry point sits beside
        // this one for tests that need to share an `Arc` with the
        // caller.
        let capture_writer: Option<Arc<zwift_relay::capture::CaptureWriter>> =
            match capture_path {
                Some(path) => {
                    let writer = zwift_relay::capture::CaptureWriter::open(&path)
                        .await
                        .map_err(RelayRuntimeError::CaptureIo)?;
                    tracing::info!(target: "ranchero::relay", ?path, "relay.capture.opened");
                    Some(Arc::new(writer))
                }
                None => None,
            };
        // If `start_inner` returns an error after the capture writer
        // was opened, flush and close it before propagating the
        // error so the file is left in a readable state. The writer
        // task emits `relay.capture.writer.closed` on its own as
        // part of its shutdown rollup (STEP-12.12 §3b).
        match Self::start_inner(
            cfg,
            capture_writer.clone(),
            auth,
            session_factory,
            tcp_factory,
            game_events_tx,
        )
        .await
        {
            Ok(this) => Ok(this),
            Err(e) => {
                if let Some(writer) = capture_writer {
                    // The writer task emits `relay.capture.writer.closed`
                    // on its own when `flush_and_close` drains the queue
                    // (STEP-12.12 §3b); no duplicate daemon-side line.
                    let _ = writer.flush_and_close().await;
                }
                Err(e)
            }
        }
    }

    /// Variant for tests that want to pre-open the capture writer
    /// (so that the test can hold its own `Arc` clone, push
    /// records, and verify the file content after shutdown). All
    /// other behaviour matches [`Self::start_with_deps`].
    pub async fn start_with_deps_and_writer<A, S, F>(
        cfg: &ResolvedConfig,
        capture_writer: Arc<zwift_relay::capture::CaptureWriter>,
        auth: A,
        session_factory: S,
        tcp_factory: F,
    ) -> Result<Self, RelayRuntimeError>
    where
        A: AuthLogin,
        S: SessionLogin,
        F: TcpTransportFactory,
    {
        let (game_events_tx, _) = tokio::sync::broadcast::channel::<GameEvent>(64);
        Self::start_inner(
            cfg,
            Some(capture_writer),
            auth,
            session_factory,
            tcp_factory,
            game_events_tx,
        )
        .await
    }

    /// Full dependency-injected entry point used by the Defect 3–7
    /// integration tests. Accepts the complete set of replaceable
    /// dependencies: auth, session supervisor factory, TCP transport
    /// factory, and UDP transport factory.
    ///
    /// Defect 3–7 (red state): the stub implementation below calls
    /// `sf.start()` to obtain the supervisor handle and derives the
    /// initial session from `handle.current()`. It does NOT yet
    /// subscribe to supervisor events (Defect 7), does NOT yet call
    /// `udp_factory.connect()` (Defect 4), and does NOT yet send a TCP
    /// hello packet (Defect 3). Tests that assert those behaviours
    /// fail at the assertion level.
    pub async fn start_with_all_deps<A, SF, F, U>(
        cfg: &ResolvedConfig,
        capture_path: Option<PathBuf>,
        auth: A,
        sf: SF,
        tcp_factory: F,
        udp_factory: U,
    ) -> Result<Self, RelayRuntimeError>
    where
        A: AuthLogin,
        SF: SessionSupervisorFactory,
        F: TcpTransportFactory,
        U: UdpTransportFactory,
    {
        let (game_events_tx, _) = tokio::sync::broadcast::channel::<GameEvent>(64);
        Self::start_all_inner(
            cfg,
            capture_path,
            None,
            auth,
            sf,
            tcp_factory,
            udp_factory,
            game_events_tx,
        )
        .await
    }

    /// Variant of [`Self::start_with_all_deps`] that pre-opens a
    /// capture writer (so the test can hold its own `Arc` clone).
    pub async fn start_with_all_deps_and_writer<A, SF, F, U>(
        cfg: &ResolvedConfig,
        capture_writer: Arc<zwift_relay::capture::CaptureWriter>,
        auth: A,
        sf: SF,
        tcp_factory: F,
        udp_factory: U,
    ) -> Result<Self, RelayRuntimeError>
    where
        A: AuthLogin,
        SF: SessionSupervisorFactory,
        F: TcpTransportFactory,
        U: UdpTransportFactory,
    {
        let (game_events_tx, _) = tokio::sync::broadcast::channel::<GameEvent>(64);
        Self::start_all_inner(
            cfg,
            None,
            Some(capture_writer),
            auth,
            sf,
            tcp_factory,
            udp_factory,
            game_events_tx,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_all_inner<A, SF, F, U>(
        cfg: &ResolvedConfig,
        capture_path: Option<PathBuf>,
        preopen_writer: Option<Arc<zwift_relay::capture::CaptureWriter>>,
        auth: A,
        sf: SF,
        tcp_factory: F,
        udp_factory: U,
        game_events_tx: tokio::sync::broadcast::Sender<GameEvent>,
    ) -> Result<Self, RelayRuntimeError>
    where
        A: AuthLogin,
        SF: SessionSupervisorFactory,
        F: TcpTransportFactory,
        U: UdpTransportFactory,
    {
        // 1. Credential validation.
        let email = cfg
            .monitor_email
            .as_deref()
            .ok_or(RelayRuntimeError::MissingEmail)?;
        let password = cfg
            .monitor_password
            .as_ref()
            .ok_or(RelayRuntimeError::MissingPassword)?;

        // 2. Auth login.
        auth.login(email, password.expose())
            .await
            .map_err(RelayRuntimeError::Auth)?;
        let athlete_id = auth.athlete_id().await.map_err(RelayRuntimeError::Auth)?;
        tracing::info!(target: "ranchero::relay", email, athlete_id, "relay.login.ok");

        // 3. Session supervisor start. Subscribe to events before the
        //    session is fetched so no events are missed.
        let handle = sf.start().await?;
        let supervisor_events = handle.subscribe_events();
        let session = handle.current().await;

        // 4. Reject empty TCP-server pool before any further I/O.
        if session.tcp_servers.is_empty() {
            return Err(RelayRuntimeError::NoTcpServers);
        }

        // 4.5. Course gate (STEP-12.14 §C2 / R1 / C9).
        //
        //     Sauce calls `getPlayerState(selfAthleteId)` before
        //     establishing TCP / UDP and refuses to come up unless
        //     the athlete is in a game (`state.world` populated).
        //     Without this gate, UDP comes up against a server that
        //     has no course context for the athlete and silently
        //     drops every inbound packet.
        //
        //     R1: the call uses `cfg.watched_athlete_id` (the athlete
        //     whose data the daemon is observing), NOT the monitor
        //     account's `auth.athlete_id()`.
        //
        //     C9: the course lives in `state.world` (proto tag 35),
        //     not the `f19` aux-bits field (L7).
        let watched_id = cfg
            .watched_athlete_id
            .ok_or(RelayRuntimeError::NoWatchedAthlete)?;
        let watched_id_i64 = watched_id as i64;
        let watched_state = auth
            .get_player_state(watched_id_i64)
            .await
            .map_err(RelayRuntimeError::Auth)?;
        let course_id = match watched_state.as_ref().and_then(|s| s.world) {
            Some(c) => c,
            None => {
                tracing::info!(
                    target: "ranchero::relay",
                    watched_athlete_id = watched_id,
                    "relay.course_gate.suspended",
                );
                return Err(RelayRuntimeError::WatchedAthleteNotInGame);
            }
        };
        tracing::info!(
            target: "ranchero::relay",
            watched_athlete_id = watched_id,
            course_id,
            "relay.course_gate.in_game",
        );

        // Resolve the capture writer: prefer the preopen, fall back
        // to opening from the supplied path (None → no capture).
        let capture_writer: Option<Arc<zwift_relay::capture::CaptureWriter>> =
            if let Some(writer) = preopen_writer {
                Some(writer)
            } else if let Some(path) = capture_path {
                let writer = zwift_relay::capture::CaptureWriter::open(&path)
                    .await
                    .map_err(RelayRuntimeError::CaptureIo)?;
                tracing::info!(target: "ranchero::relay", ?path, "relay.capture.opened");
                Some(Arc::new(writer))
            } else {
                None
            };

        // 5. Pick the first TCP server and connect.
        let server = &session.tcp_servers[0];
        let addr_str = format!("{}:{}", server.ip, zwift_relay::TCP_PORT_SECURE);
        let addr: std::net::SocketAddr = addr_str
            .parse()
            .map_err(|_| RelayRuntimeError::BadTcpAddress(addr_str.clone()))?;
        tracing::info!(target: "ranchero::relay", addr = %addr, "relay.tcp.connecting");
        let transport = tcp_factory
            .connect(addr)
            .await
            .map_err(RelayRuntimeError::TcpConnect)?;

        // 6. Establish the TCP channel and wait for Established.
        let tcp_config = zwift_relay::TcpChannelConfig {
            athlete_id,
            conn_id: next_tcp_conn_id(),
            watchdog_timeout: zwift_relay::CHANNEL_TIMEOUT,
            capture: capture_writer.clone(),
        };
        // Stash the canonical conn_id + AES key for the per-session
        // manifest; the supervisor-event handler reuses both when a
        // refresh / re-login lands. The TCP `conn_id` is u16 on the
        // wire but the capture-format manifest field is u32, so widen
        // once here and reuse the widened value.
        let session_conn_id: u32 = tcp_config.conn_id.into();
        let session_aes_key = session.aes_key;

        // Write the per-session manifest before any frame records so a
        // `--capture` reader sees `Manifest -> Frame -> Frame -> …`
        // (STEP-12.12 §6b).
        if let Some(writer) = capture_writer.as_ref() {
            writer.record_session_manifest(manifest_from_session(&session, session_conn_id));
        }

        let (channel, mut events_rx) =
            zwift_relay::TcpChannel::establish(transport, &session, tcp_config.clone())
                .await
                .map_err(RelayRuntimeError::TcpChannel)?;

        let mut prev_state: Option<RuntimeState> = None;
        emit_state_change(&game_events_tx, &mut prev_state, RuntimeState::Authenticating);
        emit_state_change(&game_events_tx, &mut prev_state, RuntimeState::SessionLoggedIn);

        let established_deadline = std::time::Duration::from_secs(5);
        match tokio::time::timeout(established_deadline, events_rx.recv()).await {
            Ok(Ok(zwift_relay::TcpChannelEvent::Established)) => {
                tracing::info!(target: "ranchero::relay", addr = %addr, "relay.tcp.established");
                emit_state_change(&game_events_tx, &mut prev_state, RuntimeState::TcpEstablished);
            }
            Ok(Ok(other)) => {
                return Err(RelayRuntimeError::TcpChannel(
                    zwift_relay::TcpError::Io(std::io::Error::other(format!(
                        "expected Established as first event, got {other:?}",
                    ))),
                ));
            }
            Ok(Err(_)) | Err(_) => {
                return Err(RelayRuntimeError::EstablishedTimeout(established_deadline));
            }
        }

        // 7. Arc-wrap the channel so `send_tcp` and `recv_loop` share
        //    ownership. (Defect 6)
        let channel = Arc::new(channel);
        let tcp_sender: Arc<dyn TcpSend> = Arc::clone(&channel) as Arc<dyn TcpSend>;

        // 8. Send the TCP hello packet. (Defect 3)
        tcp_sender
            .send_packet(
                zwift_proto::ClientToServer {
                    server_realm: 1,
                    player_id: athlete_id,
                    world_time: Some(0),
                    seqno: Some(1),
                    state: zwift_proto::PlayerState::default(),
                    ..Default::default()
                },
                true,
            )
            .await?;
        tracing::info!(target: "ranchero::relay", "relay.tcp.hello.sent");

        // 8.5. Wait for the first ServerToClient carrying a udp_config /
        //      udp_config_vod*. Zwift announces UDP servers separately
        //      from TCP servers — `session.tcp_servers` is for TCP only,
        //      and the UDP target arrives over the TCP stream after the
        //      hello. See docs/plans/STEP-12.13-still-screwing-up-after-
        //      all-these-years.md §D3 for the full rationale.
        let udp_config_deadline = std::time::Duration::from_secs(5);
        let udp_addr = {
            let mut picked: Option<std::net::SocketAddr> = None;
            let deadline = tokio::time::Instant::now() + udp_config_deadline;
            while picked.is_none() {
                let remaining = deadline
                    .saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err(RelayRuntimeError::NoUdpConfig(udp_config_deadline));
                }
                match tokio::time::timeout(remaining, events_rx.recv()).await {
                    Ok(Ok(zwift_relay::TcpChannelEvent::Inbound(stc))) => {
                        if let Some(pools) = zwift_relay::extract_udp_pools(&stc) {
                            // STEP-12.14 §C1 — sauce uses
                            // `_udpServerPools.get(0).servers[0].ip`
                            // for the initial connect: the generic
                            // load-balancer pool at lb_course=0. Per-course
                            // pools would reject athletes not on that course.
                            let generic = pools.iter()
                                .find(|p| p.lb_course == 0 && p.lb_realm == 0);
                            match generic {
                                Some(pool) => {
                                    tracing::info!(
                                        target: "ranchero::relay",
                                        pool_count = pools.len(),
                                        server_count = pool.addresses.len(),
                                        "relay.udp.config_received",
                                    );
                                    picked = pick_initial_udp_target(&pool.addresses);
                                    if picked.is_none() {
                                        tracing::warn!(
                                            target: "ranchero::relay",
                                            "relay.udp.config_no_valid_target",
                                        );
                                    }
                                }
                                None => {
                                    // All pools are per-course; error so the
                                    // operator sees a clear message rather
                                    // than a connection refused from a
                                    // per-course server.
                                    tracing::warn!(
                                        target: "ranchero::relay",
                                        pool_count = pools.len(),
                                        "relay.udp.config_no_generic_pool",
                                    );
                                    return Err(RelayRuntimeError::NoGenericPool);
                                }
                            }
                        }
                    }
                    Ok(Ok(_)) => continue,
                    Ok(Err(_)) | Err(_) => {
                        return Err(RelayRuntimeError::NoUdpConfig(udp_config_deadline));
                    }
                }
            }
            picked.expect("loop only exits when picked is Some")
        };

        // 9. Connect and establish the UDP channel.
        tracing::info!(
            target: "ranchero::relay",
            addr = %udp_addr,
            "relay.udp.connecting",
        );
        let udp_transport = udp_factory
            .connect(udp_addr)
            .await
            .map_err(RelayRuntimeError::UdpConnect)?;
        let udp_config = zwift_relay::UdpChannelConfig {
            athlete_id,
            conn_id: next_udp_conn_id(),
            // STEP-12.13 §2b — without this the writer is `None` on
            // the UDP path (the factory's default config has no
            // capture tap) and every UDP send/recv silently bypasses
            // the capture file even when `--capture` is set.
            capture: capture_writer.clone(),
            ..udp_factory.channel_config()
        };
        let world_timer = zwift_relay::WorldTimer::new();
        let (udp_channel, _udp_events_from_channel) =
            zwift_relay::UdpChannel::establish(udp_transport, &session, world_timer, udp_config)
                .await
                .map_err(RelayRuntimeError::UdpChannel)?;

        // Log UDP established synchronously so the record is always
        // present regardless of when shutdown races the event forwarder.
        let udp_latency_ms = udp_channel.latency_ms().unwrap_or(0);
        tracing::info!(
            target: "ranchero::relay",
            latency_ms = udp_latency_ms,
            "relay.udp.established",
        );
        emit_state_change(&game_events_tx, &mut prev_state, RuntimeState::UdpEstablished);

        // 10. Spawn the 1 Hz heartbeat scheduler. (Defect 5)
        let udp_channel = Arc::new(udp_channel);
        let heartbeat_abort = {
            let udp_for_heartbeat = Arc::clone(&udp_channel);
            let heartbeat_world_timer = zwift_relay::WorldTimer::new();
            let handle = tokio::spawn(async move {
                let sink = UdpHeartbeatSink(udp_for_heartbeat);
                let scheduler =
                    HeartbeatScheduler::new(sink, heartbeat_world_timer, athlete_id);
                scheduler.run().await;
            });
            let abort = handle.abort_handle();
            drop(handle);
            abort
        };
        tracing::info!(target: "ranchero::relay", "relay.heartbeat.started");

        // 11. Subscribe to session-supervisor events. (Defect 7)
        let supervisor_event_abort = {
            let mut rx = supervisor_events;
            let writer_for_supervisor = capture_writer.clone();
            let handle = tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(zwift_relay::SessionEvent::LoggedIn(new_session)) => {
                            tracing::info!(
                                target: "ranchero::relay",
                                "relay.session.logged_in",
                            );
                            // Re-login rotates the AES key; persist the
                            // new manifest so the capture stays decryptable.
                            if let Some(writer) = writer_for_supervisor.as_ref() {
                                writer.record_session_manifest(
                                    manifest_from_session(&new_session, session_conn_id),
                                );
                            }
                        }
                        Ok(zwift_relay::SessionEvent::Refreshed { relay_id, new_expires_at }) => {
                            tracing::info!(
                                target: "ranchero::relay",
                                relay_id,
                                "relay.session.refreshed",
                            );
                            // Refresh keeps the AES key but extends the
                            // expiration; persist a fresh manifest so the
                            // reader sees the current relay_id and ttl.
                            if let Some(writer) = writer_for_supervisor.as_ref() {
                                let now_unix = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default();
                                let remaining = new_expires_at
                                    .saturating_duration_since(tokio::time::Instant::now());
                                writer.record_session_manifest(
                                    zwift_relay::capture::SessionManifest {
                                        aes_key: session_aes_key,
                                        device_type: 1,
                                        channel_type: 0,
                                        send_iv_seqno_tcp: 0,
                                        recv_iv_seqno_tcp: 0,
                                        send_iv_seqno_udp: 0,
                                        recv_iv_seqno_udp: 0,
                                        relay_id,
                                        conn_id: session_conn_id,
                                        expires_at_unix_ns: (now_unix + remaining)
                                            .as_nanos() as u64,
                                    },
                                );
                            }
                        }
                        Ok(zwift_relay::SessionEvent::RefreshFailed(error)) => {
                            tracing::warn!(
                                target: "ranchero::relay",
                                %error,
                                "relay.session.refresh_failed",
                            );
                        }
                        Ok(zwift_relay::SessionEvent::LoginFailed { attempt, error }) => {
                            tracing::warn!(
                                target: "ranchero::relay",
                                attempt,
                                %error,
                                "relay.session.login_failed",
                            );
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            let abort = handle.abort_handle();
            drop(handle);
            abort
        };

        // 12. Set up the event broadcast and spawn the recv-loop.
        let (events_tx, recv_rx) =
            tokio::sync::broadcast::channel::<zwift_relay::TcpChannelEvent>(64);
        let forwarder_tx = events_tx.clone();
        tokio::spawn(async move {
            let mut rx = events_rx;
            while let Ok(event) = rx.recv().await {
                if forwarder_tx.send(event).is_err() {
                    break;
                }
            }
        });

        let shutdown = Arc::new(Notify::new());
        let (udp_events_tx, udp_events_rx) =
            tokio::sync::broadcast::channel::<zwift_relay::ChannelEvent>(64);
        let initial_watched = match cfg.watched_athlete_id {
            Some(id) => WatchedAthleteState::for_athlete(id as i64),
            None => WatchedAthleteState::default(),
        };
        let inner = Arc::new(RuntimeInner {
            pool_router: std::sync::Mutex::new(UdpPoolRouter::new()),
            watched_state: std::sync::Mutex::new(initial_watched),
            current_udp_server: std::sync::Mutex::new(None),
        });

        let recv_shutdown = shutdown.clone();
        let recv_writer = capture_writer.clone();
        let recv_game_events = game_events_tx.clone();
        let join_handle = tokio::spawn(async move {
            recv_loop(
                channel,
                recv_rx,
                udp_events_rx,
                recv_shutdown,
                recv_writer,
                recv_game_events,
            )
            .await
        });

        Ok(Self {
            join_handle,
            shutdown,
            events_tx,
            game_events_tx,
            udp_events_tx,
            inner,
            tcp_sender: Some(tcp_sender),
            heartbeat_abort: Some(heartbeat_abort),
            supervisor_event_abort: Some(supervisor_event_abort),
        })
    }

    async fn start_inner<A, S, F>(
        cfg: &ResolvedConfig,
        capture_writer: Option<Arc<zwift_relay::capture::CaptureWriter>>,
        auth: A,
        session_factory: S,
        tcp_factory: F,
        game_events_tx: tokio::sync::broadcast::Sender<GameEvent>,
    ) -> Result<Self, RelayRuntimeError>
    where
        A: AuthLogin,
        S: SessionLogin,
        F: TcpTransportFactory,
    {
        // 1. Credential validation.
        let email = cfg
            .monitor_email
            .as_deref()
            .ok_or(RelayRuntimeError::MissingEmail)?;
        let password = cfg
            .monitor_password
            .as_ref()
            .ok_or(RelayRuntimeError::MissingPassword)?;

        // 2. Auth login.
        auth.login(email, password.expose())
            .await
            .map_err(RelayRuntimeError::Auth)?;
        let athlete_id = auth.athlete_id().await.map_err(RelayRuntimeError::Auth)?;
        tracing::info!(target: "ranchero::relay", email, athlete_id, "relay.login.ok");

        // 3. Relay-session login.
        let session = session_factory
            .login()
            .await
            .map_err(RelayRuntimeError::Session)?;

        // 4. Reject empty TCP-server pool before any further I/O.
        if session.tcp_servers.is_empty() {
            return Err(RelayRuntimeError::NoTcpServers);
        }

        // 5. Pick the first TCP server and connect.
        let server = &session.tcp_servers[0];
        let addr_str = format!("{}:{}", server.ip, zwift_relay::TCP_PORT_SECURE);
        let addr: std::net::SocketAddr = addr_str
            .parse()
            .map_err(|_| RelayRuntimeError::BadTcpAddress(addr_str.clone()))?;
        tracing::info!(target: "ranchero::relay", addr = %addr, "relay.tcp.connecting");
        let transport = tcp_factory
            .connect(addr)
            .await
            .map_err(RelayRuntimeError::TcpConnect)?;

        // 6. Establish the TCP channel and wait for Established.
        let tcp_config = zwift_relay::TcpChannelConfig {
            athlete_id,
            conn_id: next_tcp_conn_id(),
            watchdog_timeout: zwift_relay::CHANNEL_TIMEOUT,
            capture: capture_writer.clone(),
        };
        let (channel, mut events_rx) =
            zwift_relay::TcpChannel::establish(transport, &session, tcp_config)
                .await
                .map_err(RelayRuntimeError::TcpChannel)?;

        // Lifecycle events are emitted on the supplied sender. By
        // the time we reach this point the auth and session login
        // have already succeeded; we replay the sequence as
        // `StateChange` records so that subscribers see the
        // canonical ordering even though the actual transitions
        // happened earlier in this function.
        let _ = game_events_tx.send(GameEvent::StateChange(RuntimeState::Authenticating));
        let _ = game_events_tx.send(GameEvent::StateChange(RuntimeState::SessionLoggedIn));

        let established_deadline = std::time::Duration::from_secs(5);
        match tokio::time::timeout(established_deadline, events_rx.recv()).await {
            Ok(Ok(zwift_relay::TcpChannelEvent::Established)) => {
                tracing::info!(target: "ranchero::relay", addr = %addr, "relay.tcp.established");
                let _ = game_events_tx.send(GameEvent::StateChange(RuntimeState::TcpEstablished));
            }
            Ok(Ok(other)) => {
                return Err(RelayRuntimeError::TcpChannel(
                    zwift_relay::TcpError::Io(std::io::Error::other(format!(
                        "expected Established as first event, got {other:?}",
                    ))),
                ));
            }
            Ok(Err(_)) => {
                return Err(RelayRuntimeError::EstablishedTimeout(established_deadline));
            }
            Err(_) => {
                return Err(RelayRuntimeError::EstablishedTimeout(established_deadline));
            }
        }

        // 7. Set up the forwarded event broadcast and spawn the
        //    recv-loop. The forwarder reads from the channel's
        //    broadcast and republishes on our `events_tx` so that
        //    tests can inject synthetic events on the same surface.
        let (events_tx, recv_rx) = tokio::sync::broadcast::channel::<zwift_relay::TcpChannelEvent>(64);
        let forwarder_tx = events_tx.clone();
        tokio::spawn(async move {
            let mut rx = events_rx;
            while let Ok(event) = rx.recv().await {
                if forwarder_tx.send(event).is_err() {
                    break;
                }
            }
        });

        let shutdown = Arc::new(Notify::new());

        // Synthetic UDP event broadcast. Tests inject events here
        // via `inject_udp_event`; the recv-loop subscribes and
        // emits the matching tracing records.
        let (udp_events_tx, udp_events_rx) =
            tokio::sync::broadcast::channel::<zwift_relay::ChannelEvent>(64);

        // Internal routing state, shared with the recv-loop and
        // with the test-only injection methods.
        let initial_watched = match cfg.watched_athlete_id {
            Some(id) => WatchedAthleteState::for_athlete(id as i64),
            None => WatchedAthleteState::default(),
        };
        let inner = Arc::new(RuntimeInner {
            pool_router: std::sync::Mutex::new(UdpPoolRouter::new()),
            watched_state: std::sync::Mutex::new(initial_watched),
            current_udp_server: std::sync::Mutex::new(None),
        });

        let channel = Arc::new(channel);
        let recv_shutdown = shutdown.clone();
        let recv_writer = capture_writer.clone();
        let recv_game_events = game_events_tx.clone();
        let join_handle = tokio::spawn(async move {
            recv_loop(
                channel,
                recv_rx,
                udp_events_rx,
                recv_shutdown,
                recv_writer,
                recv_game_events,
            )
            .await
        });

        Ok(Self {
            join_handle,
            shutdown,
            events_tx,
            game_events_tx,
            udp_events_tx,
            inner,
            tcp_sender: None,
            heartbeat_abort: None,
            supervisor_event_abort: None,
        })
    }

    /// Inject a synthetic UDP event into the orchestrator's
    /// event stream. Used by tests to drive the UDP recv-loop
    /// without a real UDP transport.
    #[cfg(test)]
    pub fn inject_udp_event(&self, event: zwift_relay::ChannelEvent) {
        let _ = self.udp_events_tx.send(event);
    }

    /// Inject a synthetic TCP event into the recv-loop's broadcast
    /// stream. Used by integration tests to exercise the
    /// `TcpChannelEvent::Inbound` arm without driving a real TCP
    /// channel through the kernel.
    pub fn inject_tcp_event(&self, event: zwift_relay::TcpChannelEvent) {
        let _ = self.events_tx.send(event);
    }

    /// Apply a `udpConfigVOD`-style pool update and recompute the
    /// best UDP server for the currently-watched athlete. If the
    /// computed server differs from the currently-selected one,
    /// emits a `GameEvent::PoolSwap`.
    #[cfg(test)]
    pub fn apply_pool_update(&self, pool: UdpServerPool) {
        self.inner
            .pool_router
            .lock()
            .expect("pool_router mutex")
            .apply_pool_update(pool);
        self.recompute_udp_selection();
    }

    /// Drive a watched-athlete `(realm, courseId, x, y)` update
    /// and recompute the best UDP server.
    #[cfg(test)]
    pub fn observe_watched_player_state(&self, realm: i32, course_id: i32, x: f64, y: f64) {
        {
            let mut watched = self
                .inner
                .watched_state
                .lock()
                .expect("watched_state mutex");
            watched.realm = realm;
            watched.course_id = course_id;
            watched.position = (x, y);
        }
        self.recompute_udp_selection();
    }

    /// Switch the watched athlete by id. Clears the cached
    /// `(realm, courseId, x, y)`; the next
    /// `observe_watched_player_state` call repopulates it.
    #[cfg(test)]
    pub fn switch_watched_athlete(&self, new_athlete_id: i64) {
        let mut watched = self
            .inner
            .watched_state
            .lock()
            .expect("watched_state mutex");
        watched.switch_to(new_athlete_id);
    }

    #[cfg(test)]
    fn recompute_udp_selection(&self) {
        let watched = self
            .inner
            .watched_state
            .lock()
            .expect("watched_state mutex")
            .clone();

        let new_server = {
            let router = self
                .inner
                .pool_router
                .lock()
                .expect("pool_router mutex");
            router
                .pool_for(watched.realm, watched.course_id)
                .and_then(|pool| {
                    find_best_udp_server(pool, watched.position.0, watched.position.1)
                        .map(|entry| entry.addr)
                })
        };

        let Some(addr) = new_server else { return };

        let mut current = self
            .inner
            .current_udp_server
            .lock()
            .expect("current_udp_server mutex");
        if *current == Some(addr) {
            return;
        }
        let from = *current;
        *current = Some(addr);
        let _ = self.game_events_tx.send(GameEvent::PoolSwap { from, to: addr });
    }

    /// Subscribe to high-level `GameEvent`s emitted by the
    /// orchestrator. Only events emitted *after* the subscribe
    /// call are observed; lifecycle events fired during
    /// `start_with_deps` cannot be observed by callers that
    /// subscribe afterwards. For tests that need those
    /// transitions, see `start_with_deps_and_events`.
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<GameEvent> {
        self.game_events_tx.subscribe()
    }

    /// Inject a synthetic `TcpChannelEvent` into the orchestrator's
    /// event stream. Used by tests to drive the recv-loop without a
    /// real TCP transport.
    #[cfg(test)]
    pub fn inject_event(&self, event: zwift_relay::TcpChannelEvent) {
        let _ = self.events_tx.send(event);
    }

    /// Send a `ClientToServer` packet over the live TCP channel.
    /// Returns `Ok(())` silently when no channel is wired (older
    /// `start_with_deps` path).
    pub async fn send_tcp(
        &self,
        payload: zwift_proto::ClientToServer,
        hello: bool,
    ) -> Result<(), RelayRuntimeError> {
        if let Some(sender) = &self.tcp_sender {
            sender.send_packet(payload, hello).await
        } else {
            Ok(())
        }
    }

    /// Request a graceful shutdown. Idempotent. The signal is
    /// stored if no task is yet waiting on it; the recv loop will
    /// observe the signal at its next `notified().await` point.
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
        if let Some(h) = &self.heartbeat_abort {
            h.abort();
        }
        if let Some(h) = &self.supervisor_event_abort {
            h.abort();
        }
    }

    /// Await orchestrator completion.
    pub async fn join(self) -> Result<(), RelayRuntimeError> {
        match self.join_handle.await {
            Ok(result) => result,
            Err(join_err) => Err(RelayRuntimeError::CaptureIo(std::io::Error::other(
                format!("orchestrator task panicked: {join_err}"),
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Default dependency-injection implementations for the production
// daemon. These delegate to the real network types in `zwift_api`
// and `zwift_relay`. Tests use stub implementations of the same
// traits in `tests/relay_runtime.rs`.
// ---------------------------------------------------------------------------

/// Production [`AuthLogin`] that delegates to
/// [`zwift_api::ZwiftAuth`].
pub struct DefaultAuthLogin {
    auth: Arc<zwift_api::ZwiftAuth>,
}

impl DefaultAuthLogin {
    pub fn new(auth: Arc<zwift_api::ZwiftAuth>) -> Self {
        Self { auth }
    }
}

impl AuthLogin for DefaultAuthLogin {
    async fn login(&self, email: &str, password: &str) -> Result<(), zwift_api::Error> {
        self.auth.login(email, password).await
    }

    async fn athlete_id(&self) -> Result<i64, zwift_api::Error> {
        self.auth.athlete_id().await
    }

    async fn get_player_state(
        &self,
        athlete_id: i64,
    ) -> Result<Option<zwift_proto::PlayerState>, zwift_api::Error> {
        self.auth.get_player_state(athlete_id).await
    }
}

/// Production [`SessionLogin`] that delegates to
/// [`zwift_relay::login`]. The shared [`Arc<zwift_api::ZwiftAuth>`]
/// is the same handle the auth-login DI type wrote its bearer
/// token into, so the relay-session login can present that token.
pub struct DefaultSessionLogin {
    auth: Arc<zwift_api::ZwiftAuth>,
    config: zwift_relay::RelaySessionConfig,
}

impl DefaultSessionLogin {
    pub fn new(
        auth: Arc<zwift_api::ZwiftAuth>,
        config: zwift_relay::RelaySessionConfig,
    ) -> Self {
        Self { auth, config }
    }
}

impl SessionLogin for DefaultSessionLogin {
    async fn login(
        &self,
    ) -> Result<zwift_relay::RelaySession, zwift_relay::SessionError> {
        zwift_relay::login(&self.auth, &self.config).await
    }
}

/// Production [`TcpTransportFactory`] that delegates to
/// [`zwift_relay::TokioTcpTransport::connect`] with a 10 s connect
/// timeout.
pub struct DefaultTcpTransportFactory;

impl TcpTransportFactory for DefaultTcpTransportFactory {
    type Transport = zwift_relay::TokioTcpTransport;

    async fn connect(
        &self,
        addr: std::net::SocketAddr,
    ) -> std::io::Result<Self::Transport> {
        zwift_relay::TokioTcpTransport::connect(
            addr,
            std::time::Duration::from_secs(10),
        )
        .await
    }
}

/// Production [`SessionSupervisorHandle`] backed by `DefaultSessionLogin`
/// (single-shot, no supervisor). In the red state this returns a
/// pre-loaded session and a dead event channel; the real supervisor
/// implementation lands with Defect 7 green state.
pub struct DefaultSessionSupervisorHandle {
    session: zwift_relay::RelaySession,
}

impl SessionSupervisorHandle for DefaultSessionSupervisorHandle {
    fn current(
        &self,
    ) -> impl std::future::Future<Output = zwift_relay::RelaySession> + Send {
        let s = self.session.clone();
        async move { s }
    }

    fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<zwift_relay::SessionEvent> {
        let (_, rx) = tokio::sync::broadcast::channel(1);
        rx
    }

    fn shutdown(&self) {}
}

/// Production [`SessionSupervisorFactory`]. In the red state this
/// delegates to `zwift_relay::login` (the same single-shot function
/// `DefaultSessionLogin` used) and wraps the result in
/// `DefaultSessionSupervisorHandle`. The real supervisor call lands
/// with Defect 7 green state.
pub struct DefaultSessionSupervisorFactory {
    auth: Arc<zwift_api::ZwiftAuth>,
    config: zwift_relay::RelaySessionConfig,
}

impl DefaultSessionSupervisorFactory {
    pub fn new(
        auth: Arc<zwift_api::ZwiftAuth>,
        config: zwift_relay::RelaySessionConfig,
    ) -> Self {
        Self { auth, config }
    }
}

impl SessionSupervisorFactory for DefaultSessionSupervisorFactory {
    type Handle = DefaultSessionSupervisorHandle;

    fn start(
        &self,
    ) -> impl std::future::Future<Output = Result<Self::Handle, RelayRuntimeError>> + Send {
        let auth = Arc::clone(&self.auth);
        let config = self.config.clone();
        async move {
            let session = zwift_relay::login(&auth, &config)
                .await
                .map_err(RelayRuntimeError::Session)?;
            Ok(DefaultSessionSupervisorHandle { session })
        }
    }
}

/// Production [`UdpTransportFactory`] used by `start_all_inner`.
pub struct DefaultUdpTransportFactory;

impl UdpTransportFactory for DefaultUdpTransportFactory {
    type Transport = zwift_relay::TokioUdpTransport;

    fn connect(
        &self,
        addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        async move { zwift_relay::TokioUdpTransport::connect(addr).await }
    }
}

async fn recv_loop<T>(
    channel: Arc<zwift_relay::TcpChannel<T>>,
    mut events_rx: tokio::sync::broadcast::Receiver<zwift_relay::TcpChannelEvent>,
    mut udp_events_rx: tokio::sync::broadcast::Receiver<zwift_relay::ChannelEvent>,
    shutdown: Arc<Notify>,
    capture_writer: Option<Arc<zwift_relay::capture::CaptureWriter>>,
    game_events_tx: tokio::sync::broadcast::Sender<GameEvent>,
) -> Result<(), RelayRuntimeError>
where
    T: zwift_relay::TcpTransport,
{
    loop {
        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                tracing::info!(target: "ranchero::relay", "relay.tcp.shutdown");
                channel.shutdown_and_wait().await;
                if let Some(writer) = capture_writer.as_ref()
                    && let Err(e) = writer.flush_and_close().await
                {
                    tracing::warn!(target: "ranchero::relay", error = %e, "capture flush failed");
                    return Err(RelayRuntimeError::CaptureIo(e));
                }
                // The writer task emits `relay.capture.writer.closed`
                // (STEP-12.12 §3b) with its own totals as it drains.
                return Ok(());
            }
            event = events_rx.recv() => {
                match event {
                    Ok(zwift_relay::TcpChannelEvent::Established) => {
                        // Already logged at start.
                    }
                    Ok(zwift_relay::TcpChannelEvent::Inbound(stc)) => {
                        let has_state_change = !stc.states.is_empty();
                        let has_world_info = !stc.updates.is_empty();
                        let message_kind = match (has_state_change, has_world_info) {
                            (true, true) => "PlayerStatesAndUpdates",
                            (true, false) => "PlayerStates",
                            (false, true) => "WorldUpdates",
                            (false, false) => "Empty",
                        };
                        tracing::debug!(
                            target: "ranchero::relay",
                            message_kind,
                            seqno = stc.seqno.unwrap_or(0),
                            has_state_change,
                            has_world_info,
                            "relay.tcp.message.recv",
                        );
                        for state in &stc.states {
                            if let Some(athlete_id) = state.id {
                                let _ = game_events_tx.send(GameEvent::PlayerState {
                                    athlete_id,
                                    power_w: state.power.unwrap_or(0),
                                    cadence_u_hz: state.cadence_u_hz.unwrap_or(0),
                                    speed_mm_h: state.speed.unwrap_or(0),
                                    world_time_ms: state.world_time.unwrap_or(0),
                                });
                            }
                        }
                    }
                    Ok(zwift_relay::TcpChannelEvent::Timeout) => {
                        tracing::info!(target: "ranchero::relay", "relay.tcp.timeout");
                    }
                    Ok(zwift_relay::TcpChannelEvent::RecvError(error)) => {
                        tracing::warn!(target: "ranchero::relay", %error, "relay.tcp.recv_error");
                    }
                    Ok(zwift_relay::TcpChannelEvent::Shutdown) => {
                        // The channel emits Shutdown on its own
                        // shutdown path; treat it as final and exit.
                        return Ok(());
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Skipped events under load; continue.
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Ok(());
                    }
                }
            }
            udp_event = udp_events_rx.recv() => {
                match udp_event {
                    Ok(zwift_relay::ChannelEvent::Established { latency_ms }) => {
                        tracing::info!(target: "ranchero::relay", latency_ms, "relay.udp.established");
                    }
                    Ok(zwift_relay::ChannelEvent::Inbound(_stc)) => {
                        // The per-message UDP recv tracing is owned by
                        // `zwift_relay::udp::recv_loop` (`relay.udp.message.recv`).
                        // The orchestrator-side branch is intentionally silent.
                    }
                    Ok(zwift_relay::ChannelEvent::Timeout) => {
                        tracing::info!(target: "ranchero::relay", "relay.udp.timeout");
                    }
                    Ok(zwift_relay::ChannelEvent::RecvError(error)) => {
                        tracing::warn!(target: "ranchero::relay", %error, "relay.udp.recv_error");
                    }
                    Ok(zwift_relay::ChannelEvent::Shutdown) => {
                        tracing::info!(target: "ranchero::relay", "relay.udp.shutdown");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // No more UDP events incoming; drop this branch
                        // by waiting on a never-resolving future.
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — STEP-12.1 red state. Every test here will fail until the
// implementation lands. The failures are intentional and document the
// behaviour that the implementation must produce.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
    use std::sync::Mutex as StdMutex;
    use std::time::Instant;

    use crate::config::{
        EditingMode, RedactedString, ResolvedConfig, ZwiftEndpoints,
    };

    fn make_config(email: Option<&str>, password: Option<&str>) -> ResolvedConfig {
        ResolvedConfig {
            main_email:    None,
            main_password: None,
            monitor_email:    email.map(str::to_string),
            monitor_password: password.map(|p| RedactedString::new(p.to_string())),
            server_bind: "127.0.0.1".into(),
            server_port: 1080,
            server_https: false,
            log_level: None,
            log_file: PathBuf::from("/tmp/ranchero-test.log"),
            pidfile: PathBuf::from("/tmp/ranchero-test.pid"),
            config_path: None,
            editing_mode: EditingMode::Default,
            // Unit tests use `start_with_deps` with stubs that never
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

    // -----------------------------------------------------------------
    // Test infrastructure: stub dependency-injection types and a
    // mock TCP transport. These let the unit tests drive
    // `RelayRuntime::start_with_deps` without touching the network.
    // -----------------------------------------------------------------

    /// Records the order in which the orchestrator calls each
    /// dependency. Each stub increments the matching counter when
    /// its method fires.
    #[derive(Default)]
    struct CallCounter {
        auth_count: AtomicUsize,
        session_count: AtomicUsize,
        tcp_count: AtomicUsize,
        // The instant at which each call fired, used to verify
        // ordering in `start_calls_auth_login_then_session_login_then_tcp_connect`.
        auth_at: StdMutex<Option<Instant>>,
        session_at: StdMutex<Option<Instant>>,
        tcp_at: StdMutex<Option<Instant>>,
    }

    impl CallCounter {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn record_auth(&self) {
            self.auth_count.fetch_add(1, Ordering::SeqCst);
            *self.auth_at.lock().unwrap() = Some(Instant::now());
        }

        fn record_session(&self) {
            self.session_count.fetch_add(1, Ordering::SeqCst);
            *self.session_at.lock().unwrap() = Some(Instant::now());
        }

        fn record_tcp(&self) {
            self.tcp_count.fetch_add(1, Ordering::SeqCst);
            *self.tcp_at.lock().unwrap() = Some(Instant::now());
        }
    }

    /// A stub `AuthLogin` that records the call and returns a
    /// configured result.
    struct StubAuth {
        counter: Arc<CallCounter>,
        result: StdMutex<Option<Result<(), zwift_api::Error>>>,
    }

    impl StubAuth {
        fn ok(counter: Arc<CallCounter>) -> Self {
            Self {
                counter,
                result: StdMutex::new(Some(Ok(()))),
            }
        }

        fn err(counter: Arc<CallCounter>, error: zwift_api::Error) -> Self {
            Self {
                counter,
                result: StdMutex::new(Some(Err(error))),
            }
        }
    }

    impl AuthLogin for StubAuth {
        fn login(
            &self,
            _email: &str,
            _password: &str,
        ) -> impl std::future::Future<Output = Result<(), zwift_api::Error>> + Send {
            self.counter.record_auth();
            let result = self
                .result
                .lock()
                .unwrap()
                .take()
                .expect("StubAuth::login called more than once");
            async move { result }
        }

        fn athlete_id(
            &self,
        ) -> impl std::future::Future<Output = Result<i64, zwift_api::Error>> + Send {
            async { Ok(12345i64) }
        }

        fn get_player_state(
            &self,
            _athlete_id: i64,
        ) -> impl std::future::Future<
            Output = Result<Option<zwift_proto::PlayerState>, zwift_api::Error>,
        > + Send {
            // The unit-test StubAuth is paired with tests that exercise
            // the supervisor / TCP / UDP wiring rather than the course
            // gate, so a default `Some(state.world = Some(1))` keeps the
            // course-gate happy without a per-test override.
            async {
                Ok(Some(zwift_proto::PlayerState {
                    world: Some(1),
                    ..Default::default()
                }))
            }
        }
    }

    /// A stub `SessionLogin` that records the call and returns a
    /// configured result.
    struct StubSession {
        counter: Arc<CallCounter>,
        result: StdMutex<Option<Result<zwift_relay::RelaySession, zwift_relay::SessionError>>>,
    }

    impl StubSession {
        fn ok(counter: Arc<CallCounter>, session: zwift_relay::RelaySession) -> Self {
            Self {
                counter,
                result: StdMutex::new(Some(Ok(session))),
            }
        }

        fn err(counter: Arc<CallCounter>, error: zwift_relay::SessionError) -> Self {
            Self {
                counter,
                result: StdMutex::new(Some(Err(error))),
            }
        }
    }

    impl SessionLogin for StubSession {
        fn login(
            &self,
        ) -> impl std::future::Future<
            Output = Result<zwift_relay::RelaySession, zwift_relay::SessionError>,
        > + Send {
            self.counter.record_session();
            let result = self
                .result
                .lock()
                .unwrap()
                .take()
                .expect("StubSession::login called more than once");
            async move { result }
        }
    }

    /// A no-op `TcpTransport` used to bring up a real `TcpChannel`
    /// without any network I/O. `read_chunk` blocks until the
    /// transport is told to release; `write_all` is silently
    /// successful and records the bytes for inspection.
    struct MockTcpTransport {
        write_log: StdMutex<Vec<Vec<u8>>>,
        read_release: tokio::sync::Notify,
        read_should_fail: AtomicBool,
    }

    impl MockTcpTransport {
        fn new() -> Self {
            Self {
                write_log: StdMutex::new(Vec::new()),
                read_release: tokio::sync::Notify::new(),
                read_should_fail: AtomicBool::new(false),
            }
        }
    }

    impl zwift_relay::TcpTransport for MockTcpTransport {
        fn write_all(
            &self,
            bytes: &[u8],
        ) -> impl std::future::Future<Output = std::io::Result<()>> + Send {
            self.write_log.lock().unwrap().push(bytes.to_vec());
            async { Ok(()) }
        }

        fn read_chunk(&self) -> impl std::future::Future<Output = std::io::Result<Vec<u8>>> + Send {
            // Wait until the test releases us, then optionally
            // return an error to drive the recv-error path. In
            // either case the call resolves on a `notified()`.
            let notified = self.read_release.notified();
            let should_fail = &self.read_should_fail;
            async move {
                notified.await;
                if should_fail.load(Ordering::SeqCst) {
                    Err(std::io::Error::other("mock recv error"))
                } else {
                    // Block forever after the first release: the
                    // recv loop only cares about reaching the
                    // shutdown branch.
                    std::future::pending::<()>().await;
                    unreachable!()
                }
            }
        }
    }

    struct StubTcpFactory {
        counter: Arc<CallCounter>,
        transport: StdMutex<Option<MockTcpTransport>>,
    }

    impl StubTcpFactory {
        fn ok(counter: Arc<CallCounter>) -> Self {
            Self {
                counter,
                transport: StdMutex::new(Some(MockTcpTransport::new())),
            }
        }
    }

    impl TcpTransportFactory for StubTcpFactory {
        type Transport = MockTcpTransport;
        fn connect(
            &self,
            _addr: std::net::SocketAddr,
        ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
            self.counter.record_tcp();
            let transport = self.transport.lock().unwrap().take();
            async move {
                transport.ok_or_else(|| std::io::Error::other("StubTcpFactory: no transport configured"))
            }
        }
    }

    /// Build a `RelaySession` suitable for stub-driven tests.
    fn fixture_session(tcp_servers: Vec<zwift_relay::TcpServer>) -> zwift_relay::RelaySession {
        zwift_relay::RelaySession {
            aes_key: [0u8; 16],
            relay_id: 42,
            tcp_servers,
            expires_at: tokio::time::Instant::now() + std::time::Duration::from_secs(3600),
            server_time_ms: Some(0),
        }
    }

    fn fixture_servers() -> Vec<zwift_relay::TcpServer> {
        vec![zwift_relay::TcpServer { ip: "127.0.0.1".into() }]
    }

    /// A `zwift_api::Error` value usable in error-propagation tests.
    fn auth_error_fixture() -> zwift_api::Error {
        zwift_api::Error::AuthFailedUnauthorized("test fixture".into())
    }

    /// A `zwift_relay::SessionError` value usable in error-propagation
    /// tests.
    fn session_error_fixture() -> zwift_relay::SessionError {
        zwift_relay::SessionError::MissingField("test fixture")
    }

    // --- 1. credential validation ---------------------------------

    #[tokio::test]
    async fn start_fails_when_email_missing() {
        let cfg = make_config(None, Some("secret"));
        let result = RelayRuntime::start(&cfg, None).await;
        assert!(
            matches!(result, Err(RelayRuntimeError::MissingEmail)),
            "expected MissingEmail; got {:?}",
            result.as_ref().err(),
        );
    }

    #[tokio::test]
    async fn start_fails_when_password_missing() {
        let cfg = make_config(Some("rider@example.com"), None);
        let result = RelayRuntime::start(&cfg, None).await;
        assert!(
            matches!(result, Err(RelayRuntimeError::MissingPassword)),
            "expected MissingPassword; got {:?}",
            result.as_ref().err(),
        );
    }

    // --- 2. call sequence with stub dependencies -----------------

    /// A counter shared across stub dependencies so the tests can
    #[tokio::test]
    async fn start_calls_auth_login_then_session_login_then_tcp_connect() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let _ = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp).await;

        assert_eq!(counter.auth_count.load(Ordering::SeqCst), 1, "auth.login must run once");
        assert_eq!(counter.session_count.load(Ordering::SeqCst), 1, "session.login must run once");
        assert_eq!(counter.tcp_count.load(Ordering::SeqCst), 1, "tcp.connect must run once");

        let auth_at = counter.auth_at.lock().unwrap().expect("auth recorded");
        let session_at = counter.session_at.lock().unwrap().expect("session recorded");
        let tcp_at = counter.tcp_at.lock().unwrap().expect("tcp recorded");
        assert!(auth_at <= session_at, "auth must precede session");
        assert!(session_at <= tcp_at, "session must precede tcp");
    }

    #[tokio::test]
    async fn start_propagates_auth_error() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::err(counter.clone(), auth_error_fixture());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let result = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp).await;

        assert!(
            matches!(result, Err(RelayRuntimeError::Auth(_))),
            "expected Auth error; got {:?}",
            result.as_ref().err(),
        );
        assert_eq!(counter.auth_count.load(Ordering::SeqCst), 1, "auth must run once");
        assert_eq!(counter.session_count.load(Ordering::SeqCst), 0, "session must not run");
        assert_eq!(counter.tcp_count.load(Ordering::SeqCst), 0, "tcp must not run");
    }

    #[tokio::test]
    async fn start_propagates_session_error() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::err(counter.clone(), session_error_fixture());
        let tcp = StubTcpFactory::ok(counter.clone());

        let result = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp).await;

        assert!(
            matches!(result, Err(RelayRuntimeError::Session(_))),
            "expected Session error; got {:?}",
            result.as_ref().err(),
        );
        assert_eq!(counter.auth_count.load(Ordering::SeqCst), 1);
        assert_eq!(counter.session_count.load(Ordering::SeqCst), 1);
        assert_eq!(counter.tcp_count.load(Ordering::SeqCst), 0, "tcp must not run");
    }

    #[tokio::test]
    async fn start_returns_no_tcp_servers_error_when_session_returns_empty_pool() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(Vec::new()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let result = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp).await;

        assert!(
            matches!(result, Err(RelayRuntimeError::NoTcpServers)),
            "expected NoTcpServers; got {:?}",
            result.as_ref().err(),
        );
        assert_eq!(counter.tcp_count.load(Ordering::SeqCst), 0, "tcp must not run");
    }

    // --- 3. lifecycle: established, inbound, recv error ----------

    #[tokio::test]
    async fn start_returns_after_first_established_event() {
        // The mock TCP transport's `read_chunk` blocks; the
        // `TcpChannel::establish` spawned task emits `Established`
        // unconditionally as its first event. `start_with_deps`
        // therefore returns `Ok` once the event arrives.
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let result = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp).await;
        let runtime = result.expect("start_with_deps must succeed when TCP comes up");

        runtime.shutdown();
        let _ = runtime.join().await;
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn inbound_events_emit_debug_tracing_records() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start_with_deps must succeed");

        let stc = zwift_proto::ServerToClient {
            seqno: Some(7),
            world_time: Some(123_456),
            ..Default::default()
        };
        runtime.inject_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));

        // Allow the recv-loop task to process the injected event.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        runtime.shutdown();
        let _ = runtime.join().await;

        assert!(
            logs_contain("relay.tcp.message.recv"),
            "expected a `relay.tcp.message.recv` record after an Inbound event \
             (renamed from `relay.tcp.inbound` in STEP-12.12 §6b)",
        );
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn recv_error_emits_warn_tracing_record() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start_with_deps must succeed");

        runtime.inject_event(zwift_relay::TcpChannelEvent::RecvError(
            "synthetic test error".into(),
        ));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        runtime.shutdown();
        let _ = runtime.join().await;

        assert!(
            logs_contain("relay.tcp.recv_error"),
            "expected a `relay.tcp.recv_error` record after a RecvError event",
        );
    }

    // --- 4. shutdown semantics ----------------------------------

    #[tokio::test]
    async fn shutdown_drains_capture_writer_and_calls_flush_and_close() {
        let path = tempfile::NamedTempFile::new().expect("tempfile");
        let writer = zwift_relay::capture::CaptureWriter::open(path.path())
            .await
            .expect("open writer");
        let writer = Arc::new(writer);

        // Push three records via the test's `Arc` clone before
        // starting the runtime. They are queued on the writer's
        // background task.
        for i in 0..3u8 {
            writer.record(zwift_relay::capture::CaptureRecord {
                ts_unix_ns: 1_700_000_000_000_000_000 + i as u64,
                direction: zwift_relay::capture::Direction::Inbound,
                transport: zwift_relay::capture::TransportKind::Tcp,
                hello: false,
                payload: vec![i; 8],
            });
        }

        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps_and_writer(
            &cfg,
            Arc::clone(&writer),
            auth,
            session,
            tcp,
        )
        .await
        .expect("start_with_deps_and_writer must succeed");

        runtime.shutdown();
        let join_result = runtime.join().await;
        assert!(join_result.is_ok(), "join must resolve cleanly: {:?}", join_result.err());

        // Drop the test's clone so any straggler `Arc` references
        // are released; this is harmless if the runtime already
        // closed the writer.
        drop(writer);

        // Read the file back: exactly three records should be
        // readable, demonstrating that `flush_and_close` drained
        // the queue before the file was closed.
        let reader =
            zwift_relay::capture::CaptureReader::open(path.path()).expect("reader");
        let count = reader.count();
        assert_eq!(
            count, 3,
            "shutdown must drain every accepted record; got {count}",
        );
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start_with_deps must succeed");

        runtime.shutdown();
        runtime.shutdown();
        let result = runtime.join().await;
        assert!(result.is_ok(), "join must resolve cleanly; got {:?}", result.err());
    }

    // -----------------------------------------------------------------
    // STEP-12.3 — Heartbeat scheduler tests.
    // -----------------------------------------------------------------

    /// Recording sink: stores every payload it receives. Used by
    /// the heartbeat tests to observe the scheduler's output
    /// without going through a real UDP transport.
    struct StubHeartbeatSink {
        sent: Arc<StdMutex<Vec<zwift_proto::ClientToServer>>>,
    }

    impl StubHeartbeatSink {
        fn new() -> (Self, Arc<StdMutex<Vec<zwift_proto::ClientToServer>>>) {
            let sent = Arc::new(StdMutex::new(Vec::new()));
            (Self { sent: sent.clone() }, sent)
        }
    }

    impl HeartbeatSink for StubHeartbeatSink {
        fn send(
            &self,
            payload: zwift_proto::ClientToServer,
        ) -> impl std::future::Future<Output = std::io::Result<()>> + Send {
            self.sent.lock().unwrap().push(payload);
            async { Ok(()) }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeat_emits_at_one_hz() {
        let (sink, sent) = StubHeartbeatSink::new();
        let scheduler = HeartbeatScheduler::new(
            sink,
            zwift_relay::WorldTimer::new(),
            12345,
        );

        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(5_500),
            scheduler.run(),
        )
        .await;

        let count = sent.lock().unwrap().len();
        assert_eq!(
            count, 5,
            "expected exactly five heartbeats over 5 simulated seconds; got {count}",
        );
    }

    #[tokio::test]
    async fn heartbeat_increments_seqno_per_send() {
        let (sink, sent) = StubHeartbeatSink::new();
        let scheduler = HeartbeatScheduler::new(
            sink,
            zwift_relay::WorldTimer::new(),
            12345,
        );

        for _ in 0..3 {
            scheduler.send_one().await.expect("send_one");
        }

        let recorded = sent.lock().unwrap();
        let seqnos: Vec<u32> = recorded
            .iter()
            .map(|p| p.seqno.expect("seqno present"))
            .collect();
        assert_eq!(seqnos, vec![1, 2, 3], "seqno must increment by one per send");
        assert_eq!(scheduler.seqno(), 3, "scheduler seqno reports total sends");
    }

    #[tokio::test]
    async fn heartbeat_world_time_tracks_world_timer() {
        let (sink, sent) = StubHeartbeatSink::new();
        let world_timer = zwift_relay::WorldTimer::new();
        let scheduler = HeartbeatScheduler::new(sink, world_timer.clone(), 12345);

        scheduler.send_one().await.expect("send_one #1");
        let first_time = sent.lock().unwrap().last().unwrap().world_time;

        // adjust_offset takes a signed delta added to the offset.
        // Increase the offset so that subsequent calls to
        // `WorldTimer::now()` return a larger value, modelling the
        // SNTP-style adjustment the UDP hello-loop applies once it
        // converges on the server clock.
        world_timer.adjust_offset(100_000);

        scheduler.send_one().await.expect("send_one #2");
        let second_time = sent.lock().unwrap().last().unwrap().world_time;

        let delta = second_time.unwrap_or(0) - first_time.unwrap_or(0);
        assert!(
            delta >= 99_000,
            "second heartbeat world_time must reflect the timer advance; \
             first = {first_time:?}, second = {second_time:?}, delta = {delta}",
        );
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn udp_channel_subscriber_does_not_double_log_inbound() {
        // STEP-12.12 §6b: the bare daemon-side `relay.udp.inbound` log
        // line was removed because per-datagram UDP tracing is owned
        // by `zwift_relay::udp::recv_loop` (which emits
        // `relay.udp.message.recv` with decoded fields). When the
        // daemon's broadcast channel forwards a synthetic
        // `ChannelEvent::Inbound`, no orchestrator-side log line
        // should appear — test guards against the duplicate landing
        // back via a future cleanup that re-introduces it.
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start");

        runtime.inject_udp_event(zwift_relay::ChannelEvent::Inbound(Box::default()));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        runtime.shutdown();
        let _ = runtime.join().await;

        assert!(
            !logs_contain("relay.udp.inbound"),
            "the bare daemon-side `relay.udp.inbound` line was deliberately \
             removed in STEP-12.12 §6b; per-datagram UDP tracing now lives in \
             `zwift_relay::udp::recv_loop` as `relay.udp.message.recv`",
        );
    }

    // STEP-12.13 D1 — recv_loop's shutdown branch still emits the
    // legacy `relay.capture.closed dropped_count=…` line alongside the
    // new `relay.capture.writer.closed total_records=… total_bytes=…`
    // rollup that STEP-12.12 §3b added. Both fire on every clean
    // shutdown; the legacy line is now noise. This test fails red
    // until 1b deletes the legacy emission.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn shutdown_emits_writer_closed_exactly_once_and_no_legacy_closed() {
        let path = tempfile::NamedTempFile::new().expect("tempfile");
        let writer = zwift_relay::capture::CaptureWriter::open(path.path())
            .await
            .expect("open writer");
        let writer = Arc::new(writer);

        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps_and_writer(
            &cfg,
            Arc::clone(&writer),
            auth,
            session,
            tcp,
        )
        .await
        .expect("start_with_deps_and_writer must succeed");

        runtime.shutdown();
        let _ = runtime.join().await;
        drop(writer);

        // Both events end with a distinctive field signature, so a
        // substring check is enough to discriminate without parsing.
        assert!(
            logs_contain("relay.capture.writer.closed"),
            "STEP-12.13 D1: relay.capture.writer.closed must still fire \
             on shutdown — it is the canonical rollup event from \
             STEP-12.12 §3b",
        );
        assert!(
            !logs_contain("relay.capture.closed dropped_count="),
            "STEP-12.13 D1: the legacy daemon-side `relay.capture.closed \
             dropped_count=…` log line must be removed from recv_loop's \
             shutdown branch — it duplicates the writer-task rollup. \
             Per-drop visibility now comes from \
             relay.capture.record.dropped warns at drop time.",
        );
    }

    // STEP-12.14 §C5 / §1a — `pick_initial_udp_target` must hardcode
    // the secure UDP port (3024) and ignore whatever the proto says
    // in `RelayAddress.port` (which Zwift sets to 3022, the
    // *plaintext* port). Sauce4zwift's `UDPChannel.establish` line
    // 1338 hardcodes `socket.connect(3024, ip)`. Sending our
    // AES-128-GCM-encrypted hellos to the plaintext port surfaces as
    // `os error 61: Connection refused` (the symptom in the live
    // trace dated 2026-05-03).
    #[test]
    fn pick_initial_udp_target_uses_secure_port_when_proto_says_plaintext() {
        let addrs = vec![zwift_proto::RelayAddress {
            ip: Some("10.0.0.1".to_string()),
            port: Some(3022), // plaintext port — sauce ignores; zoffline-side comment says default 3022
            lb_realm: Some(0),
            lb_course: Some(0),
            ra_f5: None,
            ra_f6: None,
        }];
        let target = pick_initial_udp_target(&addrs).expect("address parses");
        assert_eq!(
            target.port(),
            zwift_relay::UDP_PORT_SECURE,
            "STEP-12.14 §C5: daemon must hardcode UDP port 3024; the \
             proto's `port` field carries the plaintext port (3022) which \
             would refuse our encrypted hellos. Got {target}",
        );
        assert_eq!(target.ip().to_string(), "10.0.0.1");
    }

    #[tokio::test]
    async fn udp_shutdown_drains_capture_writer() {
        let path = tempfile::NamedTempFile::new().expect("tempfile");
        let writer = zwift_relay::capture::CaptureWriter::open(path.path())
            .await
            .expect("open writer");
        let writer = Arc::new(writer);

        for i in 0..3u8 {
            writer.record(zwift_relay::capture::CaptureRecord {
                ts_unix_ns: 1_700_000_000_000_000_000 + i as u64,
                direction: zwift_relay::capture::Direction::Inbound,
                transport: zwift_relay::capture::TransportKind::Udp,
                hello: false,
                payload: vec![i; 8],
            });
        }
        let initial_dropped = writer.dropped_count();

        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps_and_writer(
            &cfg,
            Arc::clone(&writer),
            auth,
            session,
            tcp,
        )
        .await
        .expect("start_with_deps_and_writer must succeed");

        runtime.shutdown();
        let _ = runtime.join().await;

        let final_dropped = writer.dropped_count();
        assert_eq!(
            final_dropped, initial_dropped,
            "graceful shutdown must not increase dropped_count",
        );

        drop(writer);
        let reader =
            zwift_relay::capture::CaptureReader::open(path.path()).expect("reader");
        let count = reader.count();
        assert_eq!(count, 3, "shutdown must drain every accepted UDP record");
    }

    // -----------------------------------------------------------------
    // STEP-12.4 — `udpConfigVOD` parsing and pool routing (red state).
    // -----------------------------------------------------------------

    fn entry(addr: &str, x_min: f64, x_max: f64, y_min: f64, y_max: f64) -> UdpServerEntry {
        UdpServerEntry {
            addr: addr.parse().expect("valid socket addr"),
            x_bound_min: x_min,
            x_bound: x_max,
            y_bound_min: y_min,
            y_bound: y_max,
        }
    }

    fn pool(realm: i32, course_id: i32, use_first_in_bounds: bool, servers: Vec<UdpServerEntry>) -> UdpServerPool {
        UdpServerPool { realm, course_id, use_first_in_bounds, servers }
    }

    #[test]
    fn find_best_first_in_bounds_returns_first_match() {
        // With `use_first_in_bounds = true`, the first server
        // whose bounding box contains `(x, y)` is returned, even
        // when a later server is also in bounds.
        let pool_value = pool(0, 1, true, vec![
            entry("10.0.0.1:3025", 0.0, 100.0, 0.0, 100.0),  // index 0, contains (50, 50)
            entry("10.0.0.2:3025", 25.0, 75.0, 25.0, 75.0),  // index 1, also contains (50, 50)
        ]);
        let best = find_best_udp_server(&pool_value, 50.0, 50.0);
        assert_eq!(
            best.map(|s| s.addr.to_string()),
            Some("10.0.0.1:3025".to_string()),
            "STEP-12.4 red state: useFirstInBounds must return the \
             first matching server",
        );
    }

    #[test]
    fn find_best_first_in_bounds_falls_back_to_distance_when_no_match() {
        // No bounding box contains the query; the result is the
        // server whose bound centre minimises the Euclidean
        // distance.
        let pool_value = pool(0, 1, true, vec![
            entry("10.0.0.1:3025", 0.0, 10.0, 0.0, 10.0),     // centre (5, 5)
            entry("10.0.0.2:3025", 100.0, 110.0, 100.0, 110.0), // centre (105, 105)
        ]);
        let best = find_best_udp_server(&pool_value, 50.0, 50.0);
        assert_eq!(
            best.map(|s| s.addr.to_string()),
            Some("10.0.0.1:3025".to_string()),
            "STEP-12.4 red state: when no bounding box matches, the \
             nearest-centre server must be returned",
        );
    }

    #[test]
    fn find_best_min_euclidean_when_first_in_bounds_disabled() {
        // With `use_first_in_bounds = false`, the result is
        // min-Euclidean regardless of bounds containment.
        let pool_value = pool(0, 1, false, vec![
            entry("10.0.0.1:3025", 0.0, 100.0, 0.0, 100.0),  // centre (50, 50), contains (50, 50)
            entry("10.0.0.2:3025", 49.0, 51.0, 49.0, 51.0),  // centre (50, 50)
        ]);
        let best = find_best_udp_server(&pool_value, 50.0, 50.0);
        assert!(
            best.is_some(),
            "STEP-12.4 red state: min-Euclidean mode must return some \
             server for a non-empty pool",
        );
    }

    #[test]
    fn find_best_returns_none_for_empty_pool() {
        let pool_value = pool(0, 1, true, vec![]);
        let best = find_best_udp_server(&pool_value, 0.0, 0.0);
        assert!(
            best.is_none(),
            "STEP-12.4 red state: empty pool must return None",
        );
    }

    #[test]
    fn pool_router_replaces_pool_on_repeated_udp_config_vod() {
        // Two consecutive updates for the same `(realm, courseId)`;
        // the second wins.
        let mut router = UdpPoolRouter::new();
        router.apply_pool_update(pool(0, 1, true, vec![
            entry("10.0.0.1:3025", 0.0, 10.0, 0.0, 10.0),
        ]));
        router.apply_pool_update(pool(0, 1, true, vec![
            entry("10.0.0.2:3025", 0.0, 10.0, 0.0, 10.0),
        ]));
        let p = router.pool_for(0, 1).expect("pool present");
        assert_eq!(p.servers.len(), 1);
        assert_eq!(p.servers[0].addr.to_string(), "10.0.0.2:3025");
    }

    #[test]
    fn pool_router_keys_per_realm_and_course() {
        // Updates for `(0, 1)` and `(0, 2)` are stored
        // independently.
        let mut router = UdpPoolRouter::new();
        router.apply_pool_update(pool(0, 1, true, vec![
            entry("10.0.0.1:3025", 0.0, 10.0, 0.0, 10.0),
        ]));
        router.apply_pool_update(pool(0, 2, true, vec![
            entry("10.0.0.2:3025", 0.0, 10.0, 0.0, 10.0),
        ]));
        let p1 = router.pool_for(0, 1).expect("pool 1");
        let p2 = router.pool_for(0, 2).expect("pool 2");
        assert_eq!(p1.servers[0].addr.to_string(), "10.0.0.1:3025");
        assert_eq!(p2.servers[0].addr.to_string(), "10.0.0.2:3025");
    }

    /// Helper: drain a `GameEvent` receiver and return every
    /// `PoolSwap` event that has arrived so far.
    fn drain_pool_swaps(
        rx: &mut tokio::sync::broadcast::Receiver<GameEvent>,
    ) -> Vec<(Option<std::net::SocketAddr>, std::net::SocketAddr)> {
        let mut swaps = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let GameEvent::PoolSwap { from, to } = event {
                swaps.push((from, to));
            }
        }
        swaps
    }

    #[tokio::test]
    async fn position_change_within_same_pool_swaps_server_when_bounds_demand() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start");

        let mut events_rx = runtime.events();

        // Two-server pool. Server A covers (0..50, 0..50). Server B
        // covers (50..100, 50..100). A position update inside A's
        // box selects A; a subsequent update inside B's box selects B.
        let pool = UdpServerPool {
            realm: 0,
            course_id: 1,
            use_first_in_bounds: true,
            servers: vec![
                UdpServerEntry {
                    addr: "10.0.0.1:3025".parse().unwrap(),
                    x_bound_min: 0.0,
                    x_bound: 50.0,
                    y_bound_min: 0.0,
                    y_bound: 50.0,
                },
                UdpServerEntry {
                    addr: "10.0.0.2:3025".parse().unwrap(),
                    x_bound_min: 50.0,
                    x_bound: 100.0,
                    y_bound_min: 50.0,
                    y_bound: 100.0,
                },
            ],
        };
        runtime.apply_pool_update(pool);

        runtime.observe_watched_player_state(0, 1, 25.0, 25.0);
        runtime.observe_watched_player_state(0, 1, 75.0, 75.0);

        let swaps = drain_pool_swaps(&mut events_rx);
        assert_eq!(swaps.len(), 2, "expected two PoolSwap events; got {swaps:?}");
        assert_eq!(swaps[0].0, None, "first swap must come from no current server");
        assert_eq!(swaps[0].1.to_string(), "10.0.0.1:3025");
        assert_eq!(swaps[1].0, Some("10.0.0.1:3025".parse().unwrap()));
        assert_eq!(swaps[1].1.to_string(), "10.0.0.2:3025");

        runtime.shutdown();
        let _ = runtime.join().await;
    }

    #[tokio::test]
    async fn course_change_triggers_pool_reselection() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start");

        let mut events_rx = runtime.events();

        let pool_course_1 = UdpServerPool {
            realm: 0,
            course_id: 1,
            use_first_in_bounds: true,
            servers: vec![UdpServerEntry {
                addr: "10.0.0.1:3025".parse().unwrap(),
                x_bound_min: 0.0,
                x_bound: 100.0,
                y_bound_min: 0.0,
                y_bound: 100.0,
            }],
        };
        let pool_course_2 = UdpServerPool {
            realm: 0,
            course_id: 2,
            use_first_in_bounds: true,
            servers: vec![UdpServerEntry {
                addr: "10.0.0.2:3025".parse().unwrap(),
                x_bound_min: 0.0,
                x_bound: 100.0,
                y_bound_min: 0.0,
                y_bound: 100.0,
            }],
        };
        runtime.apply_pool_update(pool_course_1);
        runtime.apply_pool_update(pool_course_2);

        runtime.observe_watched_player_state(0, 1, 50.0, 50.0);
        runtime.observe_watched_player_state(0, 2, 50.0, 50.0);

        let swaps = drain_pool_swaps(&mut events_rx);
        assert_eq!(swaps.len(), 2, "expected two PoolSwap events on course change; got {swaps:?}");
        assert_eq!(swaps[0].1.to_string(), "10.0.0.1:3025");
        assert_eq!(swaps[1].1.to_string(), "10.0.0.2:3025");

        runtime.shutdown();
        let _ = runtime.join().await;
    }

    // -----------------------------------------------------------------
    // STEP-12.5 — Idle FSM, watched-athlete, GameEvent (red state).
    // -----------------------------------------------------------------

    #[test]
    fn idle_fsm_starts_active_and_remains_active_on_motion() {
        // Inbound `PlayerState` with non-zero power keeps the FSM
        // in `Active`.
        let mut fsm = IdleFSM::new();
        assert_eq!(fsm.current(), IdleState::Active);
        fsm.observe_motion(0, 0, 250); // power > 0
        assert_eq!(fsm.current(), IdleState::Active);
    }

    #[test]
    fn idle_fsm_transitions_active_to_idle_on_zero_motion() {
        // A single zero-motion update moves the FSM to `Idle`
        // with a 60 s timer.
        let mut fsm = IdleFSM::new();
        fsm.observe_motion(0, 0, 0);
        assert_eq!(fsm.current(), IdleState::Idle);
    }

    #[test]
    fn idle_fsm_returns_to_active_on_motion_within_window() {
        // Motion before the timer fires returns the FSM to
        // `Active`.
        let mut fsm = IdleFSM::new();
        fsm.observe_motion(0, 0, 0);
        assert_eq!(fsm.current(), IdleState::Idle);
        fsm.observe_motion(100, 80, 200); // motion resumes
        assert_eq!(fsm.current(), IdleState::Active);
    }

    #[test]
    fn idle_fsm_suspends_after_timer_expires() {
        // The FSM enters `Suspended` after the idle window
        // elapses without observed motion.
        let mut fsm = IdleFSM::new();
        fsm.observe_motion(0, 0, 0);
        let _transitioned = fsm.tick(std::time::Duration::from_secs(70));
        assert_eq!(fsm.current(), IdleState::Suspended);
    }

    #[test]
    fn idle_fsm_resumes_on_motion_when_suspended() {
        // Motion in the `Suspended` state re-establishes UDP
        // (FSM returns to `Active`).
        let mut fsm = IdleFSM::new();
        fsm.observe_motion(0, 0, 0);
        let _ = fsm.tick(std::time::Duration::from_secs(70));
        assert_eq!(fsm.current(), IdleState::Suspended);
        fsm.observe_motion(100, 80, 200);
        assert_eq!(fsm.current(), IdleState::Active);
    }

    #[test]
    fn watched_athlete_switch_resets_state() {
        // Changing the watched-athlete id clears the cached
        // `(realm, courseId, x, y)`.
        let mut watched = WatchedAthleteState::for_athlete(1234);
        watched.switch_to(5678);
        assert_eq!(watched.athlete_id, 5678);
        assert_eq!(watched.realm, 0);
        assert_eq!(watched.course_id, 0);
        assert_eq!(watched.position, (0.0, 0.0));
    }

    #[tokio::test]
    async fn watched_athlete_switch_triggers_udp_reselection_on_course_change() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start");

        let mut events_rx = runtime.events();

        runtime.apply_pool_update(UdpServerPool {
            realm: 0,
            course_id: 1,
            use_first_in_bounds: true,
            servers: vec![UdpServerEntry {
                addr: "10.0.0.1:3025".parse().unwrap(),
                x_bound_min: 0.0,
                x_bound: 100.0,
                y_bound_min: 0.0,
                y_bound: 100.0,
            }],
        });
        runtime.apply_pool_update(UdpServerPool {
            realm: 0,
            course_id: 2,
            use_first_in_bounds: true,
            servers: vec![UdpServerEntry {
                addr: "10.0.0.2:3025".parse().unwrap(),
                x_bound_min: 0.0,
                x_bound: 100.0,
                y_bound_min: 0.0,
                y_bound: 100.0,
            }],
        });

        // Athlete A is on course 1.
        runtime.switch_watched_athlete(1111);
        runtime.observe_watched_player_state(0, 1, 50.0, 50.0);

        // Switch to athlete B, who is on course 2. The cached
        // course is cleared by `switch_to`; the next observe call
        // for athlete B repopulates it.
        runtime.switch_watched_athlete(2222);
        runtime.observe_watched_player_state(0, 2, 50.0, 50.0);

        let swaps = drain_pool_swaps(&mut events_rx);
        assert_eq!(
            swaps.len(),
            2,
            "expected one PoolSwap per watched-athlete observation; got {swaps:?}",
        );
        assert_eq!(swaps[0].1.to_string(), "10.0.0.1:3025");
        assert_eq!(swaps[1].1.to_string(), "10.0.0.2:3025");

        runtime.shutdown();
        let _ = runtime.join().await;
    }

    #[tokio::test]
    async fn game_event_player_state_emitted_on_inbound() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp)
            .await
            .expect("start_with_deps must succeed");

        let mut events_rx = runtime.events();

        // Inject an inbound message carrying a single PlayerState
        // for athlete 12345.
        let stc = zwift_proto::ServerToClient {
            seqno: Some(1),
            world_time: Some(100),
            states: vec![zwift_proto::PlayerState {
                id: Some(12345),
                power: Some(250),
                cadence_u_hz: Some(80_000_000),
                speed: Some(35_000_000),
                world_time: Some(200),
                ..Default::default()
            }],
            ..Default::default()
        };
        runtime.inject_event(zwift_relay::TcpChannelEvent::Inbound(Box::new(stc)));

        let event = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            events_rx.recv(),
        )
        .await
        .expect("event must arrive within timeout")
        .expect("broadcast must deliver event");

        match event {
            GameEvent::PlayerState {
                athlete_id,
                power_w,
                cadence_u_hz,
                speed_mm_h,
                world_time_ms,
            } => {
                assert_eq!(athlete_id, 12345);
                assert_eq!(power_w, 250);
                assert_eq!(cadence_u_hz, 80_000_000);
                assert_eq!(speed_mm_h, 35_000_000);
                assert_eq!(world_time_ms, 200);
            }
            other => panic!("expected GameEvent::PlayerState; got {other:?}"),
        }

        runtime.shutdown();
        let _ = runtime.join().await;
    }

    #[tokio::test]
    async fn game_event_state_change_emitted_on_lifecycle_transitions() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());

        let (game_events_tx, mut game_events_rx) =
            tokio::sync::broadcast::channel::<GameEvent>(64);

        let runtime = RelayRuntime::start_with_deps_and_events_tx(
            &cfg,
            None,
            auth,
            session,
            tcp,
            game_events_tx,
        )
        .await
        .expect("start_with_deps_and_events_tx must succeed");

        let mut observed: Vec<RuntimeState> = Vec::new();
        while let Ok(event) = game_events_rx.try_recv() {
            if let GameEvent::StateChange(state) = event {
                observed.push(state);
            }
        }

        assert_eq!(
            observed,
            vec![
                RuntimeState::Authenticating,
                RuntimeState::SessionLoggedIn,
                RuntimeState::TcpEstablished,
            ],
            "lifecycle transitions must be broadcast in order",
        );

        runtime.shutdown();
        let _ = runtime.join().await;
    }

    // -----------------------------------------------------------------
    // STEP-12.9 §Item-2 — WatchedAthleteState initialisation from config.
    //
    // Tests W-1 and W-2 fail to compile until `ResolvedConfig` gains a
    // `watched_athlete_id: Option<u64>` field. They verify that the relay
    // runtime seeds `RuntimeInner::watched_state` from `cfg.watched_athlete_id`
    // rather than always defaulting to `WatchedAthleteState::default()`.
    // -----------------------------------------------------------------

    // W-1
    #[tokio::test]
    async fn relay_runtime_initialises_watched_state_from_config() {
        let counter = CallCounter::new();
        let mut cfg = make_config(Some("rider@example.com"), Some("secret"));
        cfg.watched_athlete_id = Some(99_999u64); // RED: field does not exist yet
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());
        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp).await.unwrap();

        // The runtime must seed watched_state from cfg.watched_athlete_id at startup.
        let watched_id = runtime.inner.watched_state.lock().unwrap().athlete_id;
        assert_eq!(
            watched_id, 99_999i64,
            "W-1: watched_state must be seeded from cfg.watched_athlete_id at startup",
        );

        runtime.shutdown();
        let _ = runtime.join().await;
    }

    // W-2
    #[tokio::test]
    async fn relay_runtime_default_watched_state_when_none() {
        let counter = CallCounter::new();
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        // cfg.watched_athlete_id is implicitly None once the field exists;
        // make_config will initialise it to None by default.
        let auth = StubAuth::ok(counter.clone());
        let session = StubSession::ok(counter.clone(), fixture_session(fixture_servers()));
        let tcp = StubTcpFactory::ok(counter.clone());
        let runtime = RelayRuntime::start_with_deps(&cfg, None, auth, session, tcp).await.unwrap();

        let watched_id = runtime.inner.watched_state.lock().unwrap().athlete_id;
        assert_eq!(
            watched_id,
            WatchedAthleteState::default().athlete_id,
            "W-2: watched_state must be the default when cfg.watched_athlete_id is None",
        );

        runtime.shutdown();
        let _ = runtime.join().await;
    }
}
