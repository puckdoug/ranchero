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

/// 1 Hz UDP heartbeat that sends a `ClientToServer` carrying the
/// watched athlete's `PlayerState`. The scheduler owns the seqno
/// and reads `world_time` from the shared `WorldTimer`. Required
/// to keep the server-side TCP connection alive (spec §7.12).
///
/// STEP-12.3 stub: every method panics. See
/// `docs/plans/STEP-12-game-monitor.md` "Sub-step 12.3".
#[derive(Debug)]
#[allow(dead_code)]
pub struct HeartbeatScheduler {
    seqno: u32,
}

impl HeartbeatScheduler {
    pub fn new() -> Self {
        unimplemented!("STEP-12.3: HeartbeatScheduler::new")
    }

    /// Start the 1 Hz scheduler. Returns a handle that can be
    /// dropped to stop the scheduler.
    pub async fn start(&self) {
        unimplemented!("STEP-12.3: HeartbeatScheduler::start")
    }

    /// Current seqno; increments on every send.
    pub fn seqno(&self) -> u32 {
        unimplemented!("STEP-12.3: HeartbeatScheduler::seqno")
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
/// `zwift.mjs:2295-2317`.
///
/// STEP-12.4 stub: panics with `unimplemented!()`.
pub fn find_best_udp_server<'a>(
    _pool: &'a UdpServerPool,
    _x: f64,
    _y: f64,
) -> Option<&'a UdpServerEntry> {
    unimplemented!("STEP-12.4: find_best_udp_server")
}

/// Maintains a per-`(realm, courseId)` table of UDP server pools.
/// Updates arrive as inbound `udpConfigVOD` messages on TCP; the
/// latest update for a given key replaces the previous entry.
///
/// STEP-12.4 stub: every method panics.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct UdpPoolRouter {
    placeholder: (),
}

impl UdpPoolRouter {
    pub fn new() -> Self {
        unimplemented!("STEP-12.4: UdpPoolRouter::new")
    }

    /// Apply an inbound `udpConfigVOD` update. Replaces any
    /// existing pool for the same `(realm, courseId)` key.
    pub fn apply_pool_update(&mut self, _pool: UdpServerPool) {
        unimplemented!("STEP-12.4: UdpPoolRouter::apply_pool_update")
    }

    /// Look up the pool for a given `(realm, courseId)`.
    pub fn pool_for(&self, _realm: i32, _course_id: i32) -> Option<&UdpServerPool> {
        unimplemented!("STEP-12.4: UdpPoolRouter::pool_for")
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

/// Per spec §4.13: when the watched athlete shows zero motion for
/// approximately 60 s, suspend UDP. Resume on any motion.
///
/// STEP-12.5 stub: every method panics.
#[derive(Debug)]
#[allow(dead_code)]
pub struct IdleFSM {
    state: IdleState,
}

impl IdleFSM {
    pub fn new() -> Self {
        unimplemented!("STEP-12.5: IdleFSM::new")
    }

    pub fn current(&self) -> IdleState {
        unimplemented!("STEP-12.5: IdleFSM::current")
    }

    /// Apply a motion observation: `(speed, cadence, power)` from
    /// the watched athlete's `PlayerState`. Drives state
    /// transitions according to the spec.
    pub fn observe_motion(&mut self, _speed: i32, _cadence: i32, _power: i32) {
        unimplemented!("STEP-12.5: IdleFSM::observe_motion")
    }

    /// Apply a tick: advance the internal timer. Returns `true`
    /// if a state transition occurred.
    pub fn tick(&mut self, _elapsed: std::time::Duration) -> bool {
        unimplemented!("STEP-12.5: IdleFSM::tick")
    }
}

/// State for the currently watched athlete. Updated from inbound
/// `PlayerState` messages.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct WatchedAthleteState {
    pub athlete_id: i64,
    pub realm: i32,
    pub course_id: i32,
    pub position: (f64, f64),
}

impl WatchedAthleteState {
    pub fn for_athlete(_athlete_id: i64) -> Self {
        unimplemented!("STEP-12.5: WatchedAthleteState::for_athlete")
    }

    /// Switch the watched athlete. Clears the cached
    /// `(realm, courseId, x, y)` so that the next observed
    /// `PlayerState` for the new athlete repopulates it.
    pub fn switch_to(&mut self, _new_athlete_id: i64) {
        unimplemented!("STEP-12.5: WatchedAthleteState::switch_to")
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
    pub async fn start(
        _cfg: &ResolvedConfig,
        _capture_path: Option<PathBuf>,
    ) -> Result<Self, RelayRuntimeError> {
        unimplemented!("STEP-12.1: RelayRuntime::start")
    }

    /// Request a graceful shutdown. Idempotent.
    pub fn shutdown(&self) {
        unimplemented!("STEP-12.1: RelayRuntime::shutdown")
    }

    /// Await orchestrator completion.
    pub async fn join(self) -> Result<(), RelayRuntimeError> {
        unimplemented!("STEP-12.1: RelayRuntime::join")
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
    use std::sync::atomic::AtomicUsize;

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
    /// observe the order in which the orchestrator invokes them.
    /// The orchestrator must be wired in 12.1 to drive these
    /// dependencies; until then, the tests fail because
    /// `RelayRuntime::start` panics with `unimplemented!()`.
    #[derive(Default)]
    #[allow(dead_code)]
    struct CallCounter {
        auth: AtomicUsize,
        session: AtomicUsize,
        tcp: AtomicUsize,
    }

    #[tokio::test]
    async fn start_calls_auth_login_then_session_login_then_tcp_connect() {
        // The fully-wired version of this test substitutes stub
        // implementations of `AuthLogin`, `SessionLogin`, and
        // `TcpTransportFactory` that increment the matching counter
        // and read the call ordering off the counters. The DI
        // surface and the wiring land with the implementation;
        // until then, this test panics on `unimplemented!()` and
        // is therefore red. The assertion below documents the
        // intended check.
        let _counter = Arc::new(CallCounter::default());
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        // After implementation:
        // assert_eq!(counter.auth.load(Ordering::SeqCst), 1);
        // assert_eq!(counter.session.load(Ordering::SeqCst), 1);
        // assert_eq!(counter.tcp.load(Ordering::SeqCst), 1);
        panic!(
            "STEP-12.1 red state: this test must observe \
             auth → session → tcp ordering once `RelayRuntime::start` \
             is implemented",
        );
    }

    #[tokio::test]
    async fn start_propagates_auth_error() {
        // The fully-wired version uses a stub `AuthLogin` that
        // returns `Err(zwift_api::Error::...)` and asserts the
        // returned variant is `RelayRuntimeError::Auth(_)` without
        // any session-login or TCP-connect attempt.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.1 red state: must propagate auth errors as \
             `RelayRuntimeError::Auth` without further calls",
        );
    }

    #[tokio::test]
    async fn start_propagates_session_error() {
        // Stub auth succeeds; stub session login returns
        // `Err(zwift_relay::SessionError::...)`; result is
        // `Err(RelayRuntimeError::Session(_))` without a TCP attempt.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.1 red state: must propagate session errors as \
             `RelayRuntimeError::Session` without a TCP attempt",
        );
    }

    #[tokio::test]
    async fn start_returns_no_tcp_servers_error_when_session_returns_empty_pool() {
        // Stub auth succeeds; stub session returns a `RelaySession`
        // whose `tcp_servers` list is empty; result is
        // `Err(RelayRuntimeError::NoTcpServers)` without a TCP attempt.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.1 red state: must return NoTcpServers when the \
             session reports an empty `tcp_servers` list",
        );
    }

    // --- 3. lifecycle: established, inbound, recv error ----------

    #[tokio::test]
    async fn start_returns_after_first_established_event() {
        // Stub TCP transport emits `Established` immediately.
        // `start` must return as soon as the channel is up; the
        // recv-loop task continues to process subsequent events.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let _ = RelayRuntime::start(&cfg, None).await;
        panic!(
            "STEP-12.1 red state: must return Ok(_) after first \
             `Established` event from the TCP channel",
        );
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
        // Two consecutive shutdown() calls do not panic; join()
        // resolves cleanly.
        let cfg = make_config(Some("rider@example.com"), Some("secret"));
        let runtime = match RelayRuntime::start(&cfg, None).await {
            Ok(r) => r,
            Err(_) => panic!(
                "STEP-12.1 red state: start() returned an error, so \
                 shutdown idempotency cannot be exercised yet",
            ),
        };
        runtime.shutdown();
        runtime.shutdown();
        let _ = runtime.join().await;
    }

    // -----------------------------------------------------------------
    // STEP-12.3 — Heartbeat scheduler tests (red state).
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn heartbeat_emits_at_one_hz() {
        // The fully-wired version pauses tokio time, drives the
        // scheduler for N seconds, and asserts that exactly N
        // outbound `ClientToServer` messages were dispatched
        // through a mock UDP transport. Until 12.3 lands,
        // `HeartbeatScheduler::new` panics.
        let _scheduler = HeartbeatScheduler::new();
        panic!(
            "STEP-12.3 red state: HeartbeatScheduler must emit one \
             ClientToServer per second when started",
        );
    }

    #[tokio::test]
    async fn heartbeat_increments_seqno_per_send() {
        // Successive heartbeats carry strictly increasing seqno
        // values starting from 0 (or 1, per spec).
        let _scheduler = HeartbeatScheduler::new();
        panic!(
            "STEP-12.3 red state: each heartbeat send must increment \
             the seqno",
        );
    }

    #[tokio::test]
    async fn heartbeat_world_time_tracks_world_timer() {
        // The heartbeat's `world_time` field reflects the shared
        // `WorldTimer`. When the timer advances by N ms between
        // heartbeats, the next heartbeat's world_time is N ms
        // greater than the previous.
        let _scheduler = HeartbeatScheduler::new();
        panic!(
            "STEP-12.3 red state: heartbeat world_time must track \
             the shared WorldTimer",
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
