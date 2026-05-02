// SPDX-License-Identifier: AGPL-3.0-only
//
// TCP channel — secure, length-framed AES-128-GCM-4 stream over
// `TcpStream` to the chosen relay server's port 3025. Mirrors
// `class TCPChannel` (`zwift.mjs:1201-1306`).

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use prost::Message as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, Notify, broadcast};
use tokio::task::JoinHandle;

use crate::CodecError;
use crate::capture::{
    CaptureWriter, TransportKind, record_inbound, record_outbound,
};
use crate::consts::CHANNEL_TIMEOUT;
use crate::frame::{frame_tcp, next_tcp_frame, tcp_plaintext};
use crate::header::{Header, HeaderFlags, decode_header};
use crate::iv::RelayIv;
use crate::session::RelaySession;
use crate::{ChannelType, DeviceType, decrypt, encrypt};

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

/// Production [`TcpTransport`]. Wraps a `tokio::net::TcpStream` split
/// into owned read / write halves so concurrent read + write don't
/// contend on a single mutex.
pub struct TokioTcpTransport {
    read_half: Mutex<tokio::net::tcp::OwnedReadHalf>,
    write_half: Mutex<tokio::net::tcp::OwnedWriteHalf>,
}

impl TokioTcpTransport {
    /// Connect to `addr` with `connect_timeout`. The 1 Hz UDP
    /// `ClientToServer` heartbeat (supervisor-driven, STEP 12) is
    /// the application-level liveness signal — we deliberately do
    /// **not** call `set_keepalive(true)`. Tokio defaults keepalive
    /// to off; the silence here is intentional. (Spec §7.12 footgun
    /// is Node-specific but worth honoring on the Rust side too:
    /// the heartbeat is what the server expects.)
    pub async fn connect(
        addr: std::net::SocketAddr,
        connect_timeout: Duration,
    ) -> std::io::Result<Self> {
        let stream = tokio::time::timeout(connect_timeout, tokio::net::TcpStream::connect(addr))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TCP connect timeout"))??;
        let (r, w) = stream.into_split();
        Ok(Self {
            read_half: Mutex::new(r),
            write_half: Mutex::new(w),
        })
    }
}

impl TcpTransport for TokioTcpTransport {
    async fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self.write_half.lock().await;
        w.write_all(bytes).await
    }

    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; 65_536];
        let mut r = self.read_half.lock().await;
        let n = r.read(&mut buf).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "TCP peer closed",
            ));
        }
        buf.truncate(n);
        Ok(buf)
    }
}

// --- channel -------------------------------------------------------

#[derive(Clone)]
pub struct TcpChannelConfig {
    pub athlete_id: i64,
    pub conn_id: u16,
    pub watchdog_timeout: Duration,
    /// Optional wire-capture tap. `None` means no overhead in the
    /// channel hot path. Wired in by STEP 12 supervisor when the
    /// user passes `--capture <path>` on `start`.
    pub capture: Option<std::sync::Arc<crate::capture::CaptureWriter>>,
}

impl Default for TcpChannelConfig {
    fn default() -> Self {
        Self {
            athlete_id: 0,
            conn_id: 0,
            watchdog_timeout: CHANNEL_TIMEOUT,
            capture: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TcpChannelEvent {
    Established,
    /// `ServerToClient` is large; boxing keeps the `TcpChannelEvent`
    /// itself small enough not to bloat the broadcast ring buffer
    /// or every stack frame that holds the enum.
    Inbound(Box<zwift_proto::ServerToClient>),
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

struct SendState {
    iv_seqno: u32,
}

pub struct TcpChannel<T: TcpTransport> {
    events_tx: broadcast::Sender<TcpChannelEvent>,
    shutdown_notify: Arc<Notify>,
    recv_handle: StdMutex<Option<JoinHandle<()>>>,
    transport: Arc<T>,
    send_state: Arc<StdMutex<SendState>>,
    aes_key: [u8; 16],
    conn_id: u16,
    relay_id: u32,
    capture: Option<Arc<CaptureWriter>>,
}

impl<T: TcpTransport> TcpChannel<T> {
    /// Spawn the recv loop and return. Does NOT send a hello packet —
    /// the supervisor sends that as the first
    /// `send_packet(.., hello: true)` call so it can carry
    /// supervisor-tracked fields like
    /// `largestWorldAttributeTimestamp`.
    pub async fn establish(
        transport: T,
        session: &RelaySession,
        config: TcpChannelConfig,
    ) -> Result<(Self, broadcast::Receiver<TcpChannelEvent>), Error> {
        let transport = Arc::new(transport);
        let (events_tx, events_rx) = broadcast::channel::<TcpChannelEvent>(64);
        let shutdown_notify = Arc::new(Notify::new());
        let send_state = Arc::new(StdMutex::new(SendState { iv_seqno: 0 }));

        let transport_for_recv = transport.clone();
        let events_tx_for_recv = events_tx.clone();
        let shutdown_for_recv = shutdown_notify.clone();
        let aes_key = session.aes_key;
        let relay_id = session.relay_id;
        let conn_id = config.conn_id;
        let watchdog_timeout = config.watchdog_timeout;
        let capture_for_recv = config.capture.clone();

        let handle = tokio::spawn(async move {
            // Emit Established from the spawned task so subscribers
            // attached after `establish()` returns still see it (same
            // trick STEPs 09 / 10 use).
            let _ = events_tx_for_recv.send(TcpChannelEvent::Established);
            recv_loop(
                transport_for_recv,
                events_tx_for_recv,
                shutdown_for_recv,
                aes_key,
                relay_id,
                conn_id,
                watchdog_timeout,
                capture_for_recv,
            )
            .await;
        });

        Ok((
            Self {
                events_tx,
                shutdown_notify,
                recv_handle: StdMutex::new(Some(handle)),
                transport,
                send_state,
                aes_key,
                conn_id,
                relay_id: session.relay_id,
                capture: config.capture,
            },
            events_rx,
        ))
    }

    /// Send one `ClientToServer` payload. `hello` controls the header
    /// flags (`RELAY_ID|CONN_ID|SEQNO` vs `SEQNO` only) and the
    /// plaintext envelope hello byte (`[2,0,…]` vs `[2,1,…]`).
    pub async fn send_packet(
        &self,
        payload: zwift_proto::ClientToServer,
        hello: bool,
    ) -> Result<(), Error> {
        let app_seqno = payload.seqno.unwrap_or(0);
        let (header_bytes, ciphertext, iv_seqno_used) = {
            let mut send = self.send_state.lock().expect("send_state mutex");

            let proto_bytes = payload.encode_to_vec();
            let plaintext = tcp_plaintext(&proto_bytes, hello);

            let iv_seqno_used = send.iv_seqno;
            let header = if hello {
                Header {
                    flags: HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
                    relay_id: Some(self.relay_id),
                    conn_id: Some(self.conn_id),
                    seqno: Some(iv_seqno_used),
                }
            } else {
                Header {
                    flags: HeaderFlags::SEQNO,
                    relay_id: None,
                    conn_id: None,
                    seqno: Some(iv_seqno_used),
                }
            };
            let header_bytes = header.encode();
            let iv = RelayIv {
                device: DeviceType::Relay,
                channel: ChannelType::TcpClient,
                conn_id: self.conn_id,
                seqno: iv_seqno_used,
            };
            let ciphertext = encrypt(&self.aes_key, &iv.to_bytes(), &header_bytes, &plaintext);

            send.iv_seqno = send.iv_seqno.wrapping_add(1);
            // app_seqno is supervisor-supplied via `payload.seqno` —
            // the channel does not override it. (Open verification
            // point §2 in the plan: sauce auto-increments. Picked
            // caller-owns here for simplicity; revisit if compat
            // testing surfaces an issue.)
            (header_bytes, ciphertext, iv_seqno_used)
        };

        let wire = frame_tcp(&header_bytes, &ciphertext);
        record_outbound(self.capture.as_ref(), TransportKind::Tcp, hello, &wire);
        tracing::debug!(
            target: "ranchero::relay",
            seqno = app_seqno,
            iv_seqno = iv_seqno_used,
            hello,
            wire_size = wire.len(),
            "relay.tcp.frame.sent",
        );
        self.transport.write_all(&wire).await?;
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TcpChannelEvent> {
        self.events_tx.subscribe()
    }

    /// Cancel the recv loop / watchdog. Notifies the recv task,
    /// which emits `TcpChannelEvent::Shutdown` and exits cleanly.
    /// Does **not** await the task's exit; use
    /// [`Self::shutdown_and_wait`] for synchronous teardown.
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_one();
        let _ = self.recv_handle.lock().expect("recv_handle mutex").take();
    }

    /// Like [`Self::shutdown`] but awaits the recv task's exit so
    /// the channel's references (transport, capture writer) are
    /// fully released by the time this returns.
    pub async fn shutdown_and_wait(&self) {
        self.shutdown_notify.notify_one();
        let handle = self.recv_handle.lock().expect("recv_handle mutex").take();
        if let Some(h) = handle {
            let _ = h.await;
        }
    }
}

// --- internals -----------------------------------------------------

/// Decode header → validate relay_id → update recv IV state →
/// decrypt → return the **plaintext bytes** (no proto decode).
/// Caller decodes as `ServerToClient` after passing the plaintext
/// to the capture tap. Inbound TCP plaintext is just the proto
/// bytes (no `[2, hello?, …]` envelope), per `zwift.mjs:1285-1286`.
fn process_inbound(
    bytes: &[u8],
    aes_key: &[u8; 16],
    expected_relay_id: u32,
    recv_iv_conn_id: &mut u16,
    recv_iv_seqno: &mut u32,
) -> Result<Vec<u8>, Error> {
    let parsed = decode_header(bytes)?;
    let aad = &bytes[..parsed.consumed];
    let cipher = &bytes[parsed.consumed..];

    if let Some(rid) = parsed.header.relay_id
        && rid != expected_relay_id
    {
        return Err(Error::BadRelayId {
            expected: expected_relay_id,
            got: rid,
        });
    }
    if let Some(cid) = parsed.header.conn_id {
        *recv_iv_conn_id = cid;
    }
    if let Some(sno) = parsed.header.seqno {
        *recv_iv_seqno = sno;
    }

    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::TcpServer,
        conn_id: *recv_iv_conn_id,
        seqno: *recv_iv_seqno,
    };
    let plaintext = decrypt(aes_key, &iv.to_bytes(), aad, cipher)?;
    *recv_iv_seqno = recv_iv_seqno.wrapping_add(1);
    Ok(plaintext)
}

#[allow(clippy::too_many_arguments)]
async fn recv_loop<T: TcpTransport>(
    transport: Arc<T>,
    events_tx: broadcast::Sender<TcpChannelEvent>,
    shutdown: Arc<Notify>,
    aes_key: [u8; 16],
    relay_id: u32,
    conn_id_init: u16,
    watchdog_timeout: Duration,
    capture: Option<Arc<CaptureWriter>>,
) {
    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
    let mut recv_iv_conn_id: u16 = conn_id_init;
    let mut recv_iv_seqno: u32 = 0;

    'outer: loop {
        // Wait for either bytes from the transport (with a watchdog)
        // or a shutdown notification.
        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                let _ = events_tx.send(TcpChannelEvent::Shutdown);
                return;
            }
            result = tokio::time::timeout(watchdog_timeout, transport.read_chunk()) => {
                match result {
                    Ok(Ok(chunk)) => buffer.extend_from_slice(&chunk),
                    Ok(Err(io_err)) => {
                        let _ = events_tx.send(TcpChannelEvent::RecvError(io_err.to_string()));
                        let _ = events_tx.send(TcpChannelEvent::Shutdown);
                        return;
                    }
                    Err(_elapsed) => {
                        let _ = events_tx.send(TcpChannelEvent::Timeout);
                        continue 'outer;
                    }
                }
            }
        }

        // Drain every complete frame currently in the buffer.
        loop {
            match next_tcp_frame(&buffer) {
                Ok(Some((payload, consumed))) => {
                    // Copy the payload before draining: the slice is
                    // borrowed from `buffer` and `drain` invalidates it.
                    let payload_owned = payload.to_vec();
                    buffer.drain(..consumed);
                    record_inbound(capture.as_ref(), TransportKind::Tcp, &payload_owned);
                    let parsed_seqno = match decode_header(&payload_owned) {
                        Ok(p) => {
                            tracing::debug!(
                                target: "ranchero::relay",
                                size = consumed,
                                seqno = p.header.seqno.unwrap_or(0),
                                relay_id_present = p.header.relay_id.is_some(),
                                conn_id_present = p.header.conn_id.is_some(),
                                "relay.tcp.frame.recv",
                            );
                            p.header.seqno
                        }
                        Err(_) => None,
                    };
                    match process_inbound(
                        &payload_owned,
                        &aes_key,
                        relay_id,
                        &mut recv_iv_conn_id,
                        &mut recv_iv_seqno,
                    ) {
                        Ok(plaintext) => {
                            tracing::trace!(
                                target: "ranchero::relay",
                                seqno = parsed_seqno
                                    .unwrap_or_else(|| recv_iv_seqno.wrapping_sub(1)),
                                relay_id,
                                conn_id = recv_iv_conn_id,
                                "relay.tcp.decrypt.ok",
                            );
                            match zwift_proto::ServerToClient::decode(plaintext.as_slice()) {
                                Ok(stc) => {
                                    let _ = events_tx.send(TcpChannelEvent::Inbound(Box::new(stc)));
                                }
                                Err(e) => {
                                    let _ = events_tx.send(TcpChannelEvent::RecvError(e.to_string()));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = events_tx.send(TcpChannelEvent::RecvError(e.to_string()));
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = events_tx.send(TcpChannelEvent::RecvError(e.to_string()));
                    break;
                }
            }
        }
    }
}
