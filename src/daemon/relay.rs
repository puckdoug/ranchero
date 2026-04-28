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

use thiserror::Error;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::config::ResolvedConfig;

#[derive(Error, Debug)]
pub enum RelayRuntimeError {
    #[error("missing main account email; configure via `ranchero configure`")]
    MissingEmail,

    #[error("missing main account password; store one via `ranchero configure`")]
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

    #[error("TCP channel did not emit Established within {0:?}")]
    EstablishedTimeout(std::time::Duration),
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

/// The orchestrator owned by the daemon. `start` performs the auth
/// and relay-session login synchronously, opens the capture writer
/// if a path is given, then spawns the recv-loop task.
pub struct RelayRuntime {
    #[allow(dead_code)]
    join_handle: JoinHandle<Result<(), RelayRuntimeError>>,
    #[allow(dead_code)]
    shutdown:    Arc<Notify>,
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
        // The first tick fires immediately; advance past it so the
        // first heartbeat lands one interval after start, matching
        // the "1 Hz" expectation more naturally.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let _ = self.send_one().await;
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
pub fn find_best_udp_server<'a>(
    pool: &'a UdpServerPool,
    x: f64,
    y: f64,
) -> Option<&'a UdpServerEntry> {
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
#[derive(Debug, Clone)]
pub enum GameEvent {
    PlayerState {
        athlete_id: i64,
        realm: i32,
        course_id: i32,
        position: (f64, f64),
        power_w: i32,
        cadence_rpm: i32,
        speed_mm_s: i32,
        world_time_ms: i64,
    },
    Latency {
        latency_ms: i64,
        server_addr: std::net::SocketAddr,
    },
    StateChange(RuntimeState),
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
    /// This is the production entry point. It validates credentials
    /// up front and would, in a fully-wired build, construct the
    /// default dependency-injection types and call `start_with_deps`.
    /// The default-DI wiring is left for the live-validation phase
    /// of sub-step 12.1; until then, the public `start` simply
    /// performs credential validation and panics on the network
    /// path. Tests use `start_with_deps` directly.
    pub async fn start(
        cfg: &ResolvedConfig,
        capture_path: Option<PathBuf>,
    ) -> Result<Self, RelayRuntimeError> {
        let _email = cfg
            .main_email
            .as_deref()
            .ok_or(RelayRuntimeError::MissingEmail)?;
        let _password = cfg
            .main_password
            .as_ref()
            .ok_or(RelayRuntimeError::MissingPassword)?;

        let _ = capture_path;
        unimplemented!(
            "STEP-12.1: default-DI wiring is the responsibility of \
             the live-validation phase; tests use `start_with_deps`",
        )
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
        // 1. Credential validation.
        let email = cfg
            .main_email
            .as_deref()
            .ok_or(RelayRuntimeError::MissingEmail)?;
        let password = cfg
            .main_password
            .as_ref()
            .ok_or(RelayRuntimeError::MissingPassword)?;

        // 2. Auth login.
        auth.login(email, password.expose())
            .await
            .map_err(RelayRuntimeError::Auth)?;
        tracing::info!(target: "ranchero::relay", email, "relay.login.ok");

        // 3. Relay-session login.
        let session = session_factory
            .login()
            .await
            .map_err(RelayRuntimeError::Session)?;

        // 4. Reject empty TCP-server pool before any further I/O.
        if session.tcp_servers.is_empty() {
            return Err(RelayRuntimeError::NoTcpServers);
        }

        // 5. Open the capture writer if a path was provided.
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

        // 6. Pick the first TCP server and connect.
        let server = &session.tcp_servers[0];
        let addr_str = format!("{}:{}", server.ip, server.port);
        let addr: std::net::SocketAddr = addr_str
            .parse()
            .map_err(|_| RelayRuntimeError::BadTcpAddress(addr_str.clone()))?;
        tracing::info!(target: "ranchero::relay", addr = %addr, "relay.tcp.connecting");
        let transport = tcp_factory
            .connect(addr)
            .await
            .map_err(RelayRuntimeError::TcpConnect)?;

        // 7. Establish the TCP channel and wait for Established.
        let tcp_config = zwift_relay::TcpChannelConfig {
            athlete_id: 0,
            conn_id: 0,
            watchdog_timeout: zwift_relay::CHANNEL_TIMEOUT,
            capture: capture_writer.clone(),
        };
        let (channel, mut events_rx) =
            zwift_relay::TcpChannel::establish(transport, &session, tcp_config)
                .await
                .map_err(RelayRuntimeError::TcpChannel)?;

        let established_deadline = std::time::Duration::from_secs(5);
        match tokio::time::timeout(established_deadline, events_rx.recv()).await {
            Ok(Ok(zwift_relay::TcpChannelEvent::Established)) => {
                tracing::info!(target: "ranchero::relay", addr = %addr, "relay.tcp.established");
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

        // 8. Spawn the recv-loop task.
        let shutdown = Arc::new(Notify::new());
        let recv_shutdown = shutdown.clone();
        let recv_writer = capture_writer.clone();
        let join_handle = tokio::spawn(async move {
            recv_loop(channel, events_rx, recv_shutdown, recv_writer).await
        });

        Ok(Self {
            join_handle,
            shutdown,
        })
    }

    /// Request a graceful shutdown. Idempotent. The signal is
    /// stored if no task is yet waiting on it; the recv loop will
    /// observe the signal at its next `notified().await` point.
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
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

async fn recv_loop<T>(
    channel: zwift_relay::TcpChannel<T>,
    mut events_rx: tokio::sync::broadcast::Receiver<zwift_relay::TcpChannelEvent>,
    shutdown: Arc<Notify>,
    capture_writer: Option<Arc<zwift_relay::capture::CaptureWriter>>,
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
                if let Some(writer) = capture_writer {
                    let dropped_count = writer.dropped_count();
                    if let Ok(writer) = Arc::try_unwrap(writer) {
                        if let Err(e) = writer.flush_and_close().await {
                            tracing::warn!(target: "ranchero::relay", error = %e, "capture flush failed");
                            return Err(RelayRuntimeError::CaptureIo(e));
                        }
                    }
                    tracing::info!(target: "ranchero::relay", dropped_count, "relay.capture.closed");
                }
                return Ok(());
            }
            event = events_rx.recv() => {
                match event {
                    Ok(zwift_relay::TcpChannelEvent::Established) => {
                        // Already logged at start.
                    }
                    Ok(zwift_relay::TcpChannelEvent::Inbound(stc)) => {
                        tracing::debug!(
                            target: "ranchero::relay",
                            seqno = ?stc.seqno,
                            world_time = ?stc.world_time,
                            "relay.tcp.inbound",
                        );
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

    use crate::config::{EditingMode, LogLevel, ResolvedConfig, RedactedString};

    fn make_config(email: Option<&str>, password: Option<&str>) -> ResolvedConfig {
        ResolvedConfig {
            main_email:    email.map(str::to_string),
            main_password: password.map(|p| RedactedString::new(p.to_string())),
            monitor_email: None,
            monitor_password: None,
            server_bind: "127.0.0.1".into(),
            server_port: 1080,
            server_https: false,
            log_level: LogLevel::Info,
            log_file: PathBuf::from("/tmp/ranchero-test.log"),
            pidfile: PathBuf::from("/tmp/ranchero-test.pid"),
            config_path: None,
            editing_mode: EditingMode::Default,
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

        fn err_no_transport(counter: Arc<CallCounter>) -> Self {
            Self {
                counter,
                transport: StdMutex::new(None),
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
        vec![zwift_relay::TcpServer {
            ip: "127.0.0.1".into(),
            port: 3025,
        }]
    }

    /// A `zwift_api::Error` value usable in error-propagation tests.
    fn auth_error_fixture() -> zwift_api::Error {
        zwift_api::Error::AuthFailed("test fixture".into())
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
    async fn inbound_events_emit_debug_tracing_records() {
        // Drive a fixture inbound packet through the recv loop; the
        // orchestrator emits a `relay.tcp.inbound` event at DEBUG
        // with `payload_len` and summary fields. The fully-wired
        // version uses `tracing-test` (or an in-memory subscriber)
        // to capture and assert the recorded event.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.1 red state: each inbound packet must produce a \
             `relay.tcp.inbound` DEBUG record with payload_len and \
             summary fields",
        );
    }

    #[tokio::test]
    async fn recv_error_emits_warn_tracing_record() {
        // Stub transport returns a recv error; the orchestrator
        // emits a single WARN `relay.tcp.recv_error` record and
        // continues running.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.1 red state: recv errors must produce a single \
             WARN `relay.tcp.recv_error` record",
        );
    }

    // --- 4. shutdown semantics ----------------------------------

    #[tokio::test]
    async fn shutdown_drains_capture_writer_and_calls_flush_and_close() {
        // Capture writer is opened at start. Three inbound packets
        // are pushed via stub events. `shutdown()` is called. The
        // resulting capture file contains exactly three records on
        // replay.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(
            &cfg,
            Some(PathBuf::from("/tmp/ranchero-test-capture.cap")),
        ).await;
        panic!(
            "STEP-12.1 red state: shutdown must call \
             `flush_and_close` so that every accepted record \
             survives the close",
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
            .map(|p| p.seqno.expect("seqno present") as u32)
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
    async fn udp_channel_subscriber_logs_inbound_at_debug() {
        // An inbound StC packet on UDP triggers a
        // `relay.udp.inbound` DEBUG record with `payload_len`.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.3 red state: each UDP inbound packet must \
             produce a `relay.udp.inbound` DEBUG record",
        );
    }

    #[tokio::test]
    async fn udp_shutdown_drains_capture_writer() {
        // A graceful UDP shutdown does not drop any records that
        // were accepted by the capture writer. The writer's
        // `dropped_count` remains zero across the shutdown when
        // no records were dropped due to channel saturation.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(
            &cfg,
            Some(PathBuf::from("/tmp/ranchero-12-3-capture.cap")),
        ).await;
        panic!(
            "STEP-12.3 red state: UDP shutdown must not lose accepted \
             records from the capture writer",
        );
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

    #[tokio::test]
    async fn position_change_within_same_pool_swaps_server_when_bounds_demand() {
        // The watched athlete crosses a bound; the orchestrator
        // selects the new server and swaps UDP channels.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.4 red state: a position change that crosses a \
             server bound must trigger a UDP channel swap",
        );
    }

    #[tokio::test]
    async fn course_change_triggers_pool_reselection() {
        // The watched athlete's course changes; the orchestrator
        // selects a server from the new course's pool.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.4 red state: a course change must trigger a \
             pool reselection from the new (realm, courseId) pool",
        );
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
        // A new watched athlete on a different course causes the
        // UDP pool router to fire and the UDP channel to swap.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.5 red state: a watched-athlete switch onto a \
             different course must trigger UDP reselection",
        );
    }

    #[tokio::test]
    async fn game_event_player_state_emitted_on_inbound() {
        // An inbound `ServerToClient` carrying the watched
        // athlete's `PlayerState` produces a
        // `GameEvent::PlayerState` on the broadcast channel.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.5 red state: inbound watched-athlete \
             PlayerState must produce a `GameEvent::PlayerState`",
        );
    }

    #[tokio::test]
    async fn game_event_state_change_emitted_on_lifecycle_transitions() {
        // The `RuntimeState` transitions are broadcast in order
        // (Authenticating, SessionLoggedIn, TcpEstablished,
        // UdpEstablished, ...).
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.5 red state: lifecycle transitions must be \
             emitted as `GameEvent::StateChange(_)` records in \
             order",
        );
    }
}
