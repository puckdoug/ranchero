// SPDX-License-Identifier: AGPL-3.0-only
//
// TCP channel — secure, length-framed AES-128-GCM-4 stream over
// `TcpStream` to the chosen relay server's port 3025. Mirrors
// `class TCPChannel` (`zwift.mjs:1201-1306`).
//
// Public API documented in `docs/plans/STEP-11-tcp-channel.md`.
// This file currently exposes the surface as stubs so
// `tests/tcp.rs` compiles. Implementation lands in green state.

use std::time::Duration;

use tokio::sync::broadcast;

use crate::CodecError;
use crate::consts::CHANNEL_TIMEOUT;
use crate::session::RelaySession;

// --- TCP transport abstraction -------------------------------------

/// Stream-oriented transport. Implemented by [`TokioTcpTransport`] in
/// production and by tests' mock. `async fn` in trait is stable since
/// Rust 1.75; the channel uses generics, not `dyn`.
pub trait TcpTransport: Send + Sync + 'static {
    fn write_all(
        &self,
        bytes: &[u8],
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send;

    /// Read whatever the OS has available. May return a partial frame,
    /// multiple frames, or anything in between. The recv loop accumulates
    /// across calls and drives `next_tcp_frame` to slice out complete
    /// frames.
    fn read_chunk(&self) -> impl std::future::Future<Output = std::io::Result<Vec<u8>>> + Send;
}

/// Production [`TcpTransport`]. Wraps a `tokio::net::TcpStream`.
pub struct TokioTcpTransport {
    #[allow(dead_code)]
    stream: tokio::net::TcpStream,
}

impl TokioTcpTransport {
    /// Connect to `addr` with `connect_timeout`. **Does not** call
    /// `set_keepalive(true)` — the spec §7.12 footgun comes from a
    /// Node-specific bug; in our case the application-level 1 Hz
    /// `ClientToServer` heartbeat (UDP, supervisor-driven) is the
    /// liveness signal. Tokio defaults keepalive to off; this
    /// non-action is deliberate.
    pub async fn connect(
        _addr: std::net::SocketAddr,
        _connect_timeout: Duration,
    ) -> std::io::Result<Self> {
        unimplemented!("STEP-11: TcpStream::connect wrapped in tokio::time::timeout")
    }
}

impl TcpTransport for TokioTcpTransport {
    async fn write_all(&self, _bytes: &[u8]) -> std::io::Result<()> {
        unimplemented!("STEP-11: AsyncWriteExt::write_all on the stream")
    }

    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
        unimplemented!("STEP-11: read into a 64 KiB buffer + truncate")
    }
}

// --- channel -------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TcpChannelConfig {
    pub athlete_id: i64,
    pub conn_id: u16,
    pub watchdog_timeout: Duration,
}

impl Default for TcpChannelConfig {
    fn default() -> Self {
        Self {
            athlete_id: 0,
            conn_id: 0,
            watchdog_timeout: CHANNEL_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TcpChannelEvent {
    Established,
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

    #[error("inbound relay_id mismatch: expected {expected}, got {got}")]
    BadRelayId { expected: u32, got: u32 },
}

pub struct TcpChannel<T: TcpTransport> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: TcpTransport> TcpChannel<T> {
    /// Spawn the recv loop and return. Does NOT send a hello packet —
    /// the supervisor sends that as the first
    /// `send_packet(.., hello: true)` call so it can carry
    /// supervisor-tracked fields like
    /// `largestWorldAttributeTimestamp`.
    pub async fn establish(
        _transport: T,
        _session: &RelaySession,
        _config: TcpChannelConfig,
    ) -> Result<(Self, broadcast::Receiver<TcpChannelEvent>), Error> {
        unimplemented!("STEP-11: spawn recv loop, emit Established as first event")
    }

    /// Send one `ClientToServer` payload. `hello` controls:
    /// - **header flags**: `RELAY_ID | CONN_ID | SEQNO` vs `SEQNO` only
    /// - **plaintext envelope hello byte**: `[2, 0, …]` vs `[2, 1, …]`
    pub async fn send_packet(
        &self,
        _payload: zwift_proto::ClientToServer,
        _hello: bool,
    ) -> Result<(), Error> {
        unimplemented!("STEP-11: build header + envelope + encrypt + frame_tcp + write_all")
    }

    /// Subscribe an additional event consumer.
    pub fn subscribe(&self) -> broadcast::Receiver<TcpChannelEvent> {
        unimplemented!("STEP-11: events_tx.subscribe()")
    }

    /// Cancel the recv loop / watchdog and emit `Shutdown`.
    pub fn shutdown(&self) {
        unimplemented!("STEP-11: notify_one + drop recv handle")
    }
}
