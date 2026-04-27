// SPDX-License-Identifier: AGPL-3.0-only
//
// UDP channel + SNTP-style time sync. Mirrors `class UDPChannel`
// (`zwift.mjs:1313-1448`), the SNTP filter inside its hello-loop
// (`zwift.mjs:1342-1377`), and the recv path at
// `zwift.mjs:1416-1430`.
//
// Public API is documented at `docs/plans/STEP-10-udp-channel.md`.
// This file currently exposes the surface as stubs so
// `tests/{world_timer,time_sync,udp}.rs` compile. Implementation
// lands in green state.

use std::time::Duration;

use tokio::sync::broadcast;

use crate::CodecError;
use crate::consts::{CHANNEL_TIMEOUT, MAX_HELLOS, MIN_SYNC_SAMPLES};
use crate::session::RelaySession;
use crate::world_timer::WorldTimer;

// --- UDP transport abstraction -------------------------------------

/// Async send/recv pair for a connected UDPv4 socket. Implemented by
/// [`TokioUdpTransport`] in production and by tests' mock
/// transports. `async fn` in trait is stable since Rust 1.75; the
/// channel below uses generics, not `dyn`, so no `async-trait` crate
/// is required.
pub trait UdpTransport: Send + Sync + 'static {
    fn send(&self, bytes: &[u8]) -> impl std::future::Future<Output = std::io::Result<()>> + Send;
    fn recv(&self) -> impl std::future::Future<Output = std::io::Result<Vec<u8>>> + Send;
}

/// Production [`UdpTransport`]. Wraps a connected `tokio::net::UdpSocket`.
pub struct TokioUdpTransport {
    #[allow(dead_code)]
    socket: tokio::net::UdpSocket,
}

impl TokioUdpTransport {
    /// Bind an ephemeral local port and `connect()` to `server`.
    /// After connection, [`UdpTransport::send`] / [`UdpTransport::recv`]
    /// use the connected peer; the OS drops mismatched-source
    /// datagrams.
    pub async fn connect(_server: std::net::SocketAddr) -> std::io::Result<Self> {
        unimplemented!("STEP-10: bind + connect a UdpSocket")
    }
}

impl UdpTransport for TokioUdpTransport {
    async fn send(&self, _bytes: &[u8]) -> std::io::Result<()> {
        unimplemented!("STEP-10: socket.send(bytes).await")
    }

    async fn recv(&self) -> std::io::Result<Vec<u8>> {
        unimplemented!("STEP-10: socket.recv into a 64 KiB buffer + truncate")
    }
}

// --- pure-math SNTP filter -----------------------------------------

/// Tests poke this module directly with synthetic samples; production
/// [`UdpChannel`] feeds it from inbound replies during the hello loop.
pub mod sync {
    /// One latency / offset measurement, in milliseconds.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Sample {
        pub latency_ms: i64,
        pub offset_ms: i64,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SyncOutcome {
        /// Not enough samples yet (or too many filtered out as
        /// outliers); keep collecting.
        NeedMore,
        /// Convergence reached. Caller should
        /// `world_timer.adjust_offset(-mean_offset_ms)` and emit
        /// `Established { latency_ms: median_latency_ms }`.
        Converged {
            mean_offset_ms: i64,
            median_latency_ms: i64,
        },
    }

    /// Sauce's filter, `zwift.mjs:1359-1373`. Returns `Converged`
    /// only when (a) the sample count exceeds
    /// [`crate::MIN_SYNC_SAMPLES`] **and** (b) at least 5 samples
    /// survive the stddev-based outlier filter (sauce uses literal
    /// `> 4`).
    pub fn compute_offset(_samples: &[Sample]) -> SyncOutcome {
        unimplemented!("STEP-10: SNTP filter — sort by latency, mean+stddev, median, filter outliers, average offsets")
    }
}

// --- channel -------------------------------------------------------

#[derive(Debug, Clone)]
pub struct UdpChannelConfig {
    pub course_id: i32,
    pub athlete_id: i64,
    /// Per-channel `connId` u16, embedded in IVs and the hello
    /// header's `CONN_ID` field. Production assigns from a per-process
    /// counter (STEP 12); tests can pass any value.
    pub conn_id: u16,
    /// Hard cap on hello attempts before declaring sync failure.
    /// Production default [`MAX_HELLOS`] (25).
    pub max_hellos: u32,
    /// Minimum SNTP-style samples required before convergence.
    /// Production default [`MIN_SYNC_SAMPLES`] (5).
    pub min_sync_samples: usize,
    /// Watchdog: emit `Timeout` after this much inbound silence.
    /// Production default [`CHANNEL_TIMEOUT`] (30 s).
    pub watchdog_timeout: Duration,
}

impl Default for UdpChannelConfig {
    fn default() -> Self {
        Self {
            course_id: 0,
            athlete_id: 0,
            conn_id: 0,
            max_hellos: MAX_HELLOS,
            min_sync_samples: MIN_SYNC_SAMPLES,
            watchdog_timeout: CHANNEL_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChannelEvent {
    Established { latency_ms: i64 },
    Inbound(zwift_proto::ServerToClient),
    Timeout,
    RecvError(String),
    Shutdown,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("codec: {0}")]
    Codec(#[from] CodecError),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("protobuf decode: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("hello-loop timed out after {attempts} attempts without sync")]
    SyncTimeout { attempts: u32 },

    #[error("inbound relay_id mismatch: expected {expected}, got {got}")]
    BadRelayId { expected: u32, got: u32 },
}

pub struct UdpChannel {
    // Held opaquely; tests use the public methods + the broadcast
    // receiver returned from `establish`.
    _private: (),
}

impl UdpChannel {
    /// Run the hello-loop synchronously against `transport`, then
    /// spawn the recv-loop + watchdog. Returns once sync converges
    /// or after `config.max_hellos` attempts exhausted.
    pub async fn establish<T: UdpTransport>(
        _transport: T,
        _session: &RelaySession,
        _clock: WorldTimer,
        _config: UdpChannelConfig,
    ) -> Result<(Self, broadcast::Receiver<ChannelEvent>), Error> {
        unimplemented!("STEP-10: hello-loop + sync filter + spawn recv-loop")
    }

    /// Send one `ClientToServer` payload (typically a `PlayerState`).
    /// Owns IV / seqno mutation under an internal mutex.
    pub async fn send_player_state(
        &self,
        _state: zwift_proto::PlayerState,
    ) -> Result<(), Error> {
        unimplemented!("STEP-10: build CTS, encode, encrypt, send via transport")
    }

    /// Median latency from the last successful sync.
    pub fn latency_ms(&self) -> Option<i64> {
        unimplemented!("STEP-10: return cached latency from inner state")
    }

    /// Subscribe an additional event consumer (e.g. supervisor + a
    /// debug pane).
    pub fn subscribe(&self) -> broadcast::Receiver<ChannelEvent> {
        unimplemented!("STEP-10: events_tx.subscribe()")
    }

    /// Cancel the recv-loop / watchdog and emit `Shutdown`.
    pub fn shutdown(&self) {
        unimplemented!("STEP-10: abort recv-loop task and emit Shutdown")
    }
}
