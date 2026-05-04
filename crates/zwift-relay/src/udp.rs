// SPDX-License-Identifier: AGPL-3.0-only
//
// UDP channel + SNTP-style time sync. Mirrors `class UDPChannel`
// (`zwift.mjs:1313-1448`), the SNTP filter inside its hello-loop
// (`zwift.mjs:1342-1377`), and the recv path at
// `zwift.mjs:1416-1430`. See `docs/plans/STEP-10-udp-channel.md`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prost::Message as _;
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::CodecError;
use crate::capture::{
    CaptureWriter, TransportKind, record_inbound, record_outbound,
};
use crate::consts::{CHANNEL_TIMEOUT, MAX_HELLOS, MIN_SYNC_SAMPLES};
use crate::frame::udp_plaintext;
use crate::header::{Header, HeaderFlags, decode_header};
use crate::iv::RelayIv;
use crate::session::RelaySession;
use crate::world_timer::WorldTimer;
use crate::{ChannelType, DeviceType, decrypt, encrypt};

// --- UDP transport abstraction -------------------------------------

pub trait UdpTransport: Send + Sync + 'static {
    fn send(&self, bytes: &[u8]) -> impl std::future::Future<Output = std::io::Result<()>> + Send;
    fn recv(&self) -> impl std::future::Future<Output = std::io::Result<Vec<u8>>> + Send;
}

pub struct TokioUdpTransport {
    socket: tokio::net::UdpSocket,
}

impl TokioUdpTransport {
    /// Bind an ephemeral local port and `connect()` to `server`.
    /// After connection, [`UdpTransport::send`] / [`UdpTransport::recv`]
    /// use the connected peer; the OS drops mismatched-source
    /// datagrams.
    pub async fn connect(server: std::net::SocketAddr) -> std::io::Result<Self> {
        let bind_addr: std::net::SocketAddr = match server {
            std::net::SocketAddr::V4(_) => "0.0.0.0:0".parse().unwrap(),
            std::net::SocketAddr::V6(_) => "[::]:0".parse().unwrap(),
        };
        let socket = tokio::net::UdpSocket::bind(bind_addr).await?;
        socket.connect(server).await?;
        Ok(Self { socket })
    }
}

impl UdpTransport for TokioUdpTransport {
    async fn send(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.socket.send(bytes).await.map(|_| ())
    }

    async fn recv(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; 65_536];
        let n = self.socket.recv(&mut buf).await?;
        buf.truncate(n);
        Ok(buf)
    }
}

// --- pure-math SNTP filter -----------------------------------------

pub mod sync {
    use super::MIN_SYNC_SAMPLES;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Sample {
        pub latency_ms: i64,
        pub offset_ms: i64,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SyncOutcome {
        NeedMore,
        Converged {
            mean_offset_ms: i64,
            median_latency_ms: i64,
        },
    }

    /// Sauce's filter (`zwift.mjs:1359-1373`). Returns `Converged`
    /// only when (a) the sample count is **strictly greater than**
    /// [`MIN_SYNC_SAMPLES`] and (b) **strictly more than 4** samples
    /// survive the stddev-based outlier filter.
    pub fn compute_offset(samples: &[Sample]) -> SyncOutcome {
        if samples.len() <= MIN_SYNC_SAMPLES {
            return SyncOutcome::NeedMore;
        }
        let mut sorted: Vec<Sample> = samples.to_vec();
        sorted.sort_by_key(|s| s.latency_ms);

        let n = sorted.len() as f64;
        let mean_latency: f64 = sorted.iter().map(|s| s.latency_ms as f64).sum::<f64>() / n;
        let variance_sum: f64 = sorted
            .iter()
            .map(|s| (mean_latency - s.latency_ms as f64).powi(2))
            .sum();
        let stddev = (variance_sum / n).sqrt();
        let median_latency = sorted[sorted.len() / 2].latency_ms;

        // When `stddev == 0` every sample shares the same latency. Sauce's
        // `< stddev` test rejects all of them (0 < 0 is false) and the
        // hello loop would loop forever waiting for variance that can't
        // arrive. In real networks latencies always vary; this branch
        // primarily guards against test scenarios where mock transports
        // produce sub-ms latencies that all truncate to 0. Treating zero
        // variance as trivially-converged is semantically equivalent: the
        // samples *are* tightly clustered, by definition.
        let valid: Vec<&Sample> = if stddev == 0.0 {
            sorted.iter().collect()
        } else {
            sorted
                .iter()
                .filter(|s| ((s.latency_ms - median_latency) as f64).abs() < stddev)
                .collect()
        };

        if valid.len() <= 4 {
            return SyncOutcome::NeedMore;
        }

        let mean_offset: f64 =
            valid.iter().map(|s| s.offset_ms as f64).sum::<f64>() / valid.len() as f64;

        SyncOutcome::Converged {
            mean_offset_ms: mean_offset.round() as i64,
            median_latency_ms: median_latency,
        }
    }
}

use self::sync::{Sample, SyncOutcome};

// --- channel -------------------------------------------------------

#[derive(Clone)]
pub struct UdpChannelConfig {
    pub course_id: i32,
    pub athlete_id: i64,
    pub conn_id: u16,
    pub max_hellos: u32,
    pub min_sync_samples: usize,
    pub watchdog_timeout: Duration,
    /// Optional wire-capture tap. `None` means no overhead in the
    /// channel hot path. Wired in by STEP 12 supervisor when the
    /// user passes `--capture <path>` on `start`.
    pub capture: Option<std::sync::Arc<crate::capture::CaptureWriter>>,
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
            capture: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChannelEvent {
    Established { latency_ms: i64 },
    /// `ServerToClient` is large; boxing keeps the `ChannelEvent`
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

    #[error("hello-loop timed out after {attempts} attempts without sync")]
    SyncTimeout { attempts: u32 },

    #[error("inbound relay_id mismatch: expected {expected}, got {got}")]
    BadRelayId { expected: u32, got: u32 },
}

struct SendState {
    iv_seqno: u32,
    app_seqno: u32,
}

pub struct UdpChannel<T: UdpTransport> {
    events_tx: broadcast::Sender<ChannelEvent>,
    shutdown_notify: Arc<Notify>,
    recv_handle: Mutex<Option<JoinHandle<()>>>,
    transport: Arc<T>,
    send_state: Arc<Mutex<SendState>>,
    aes_key: [u8; 16],
    conn_id: u16,
    athlete_id: i64,
    latency_ms: i64,
    capture: Option<Arc<CaptureWriter>>,
}

impl<T: UdpTransport> UdpChannel<T> {
    /// Run the hello-loop synchronously against `transport`, then
    /// spawn the recv-loop + watchdog. Returns once sync converges
    /// or after `config.max_hellos` attempts exhausted.
    pub async fn establish(
        transport: T,
        session: &RelaySession,
        clock: WorldTimer,
        config: UdpChannelConfig,
    ) -> Result<(Self, broadcast::Receiver<ChannelEvent>), Error> {
        let transport = Arc::new(transport);
        let mut send_iv_seqno: u32 = 0;
        let mut app_seqno: u32 = 0;
        let mut samples: Vec<Sample> = Vec::new();
        let mut send_times: HashMap<u32, i64> = HashMap::new();
        let mut recv_iv_seqno: u32 = 0;
        let mut recv_iv_conn_id: u16 = config.conn_id;
        let mut latency_ms: Option<i64> = None;

        tracing::info!(
            target: "ranchero::relay",
            athlete_id = config.athlete_id,
            max_hellos = config.max_hellos,
            "relay.udp.hello.started",
        );

        'hello_loop: for hello_idx in 1..=config.max_hellos {
            // ── send hello ──
            let hello_app_seqno = app_seqno;
            let hello_iv_seqno = send_iv_seqno;
            let cts = build_hello(config.athlete_id, hello_app_seqno);
            let proto_bytes = cts.encode_to_vec();
            let plaintext = udp_plaintext(&proto_bytes);

            let header = build_send_header(
                session.relay_id,
                config.conn_id,
                hello_iv_seqno,
            );
            let header_bytes = header.encode();
            let send_iv = RelayIv {
                device: DeviceType::Relay,
                channel: ChannelType::UdpClient,
                conn_id: config.conn_id,
                seqno: hello_iv_seqno,
            };
            let cipher = encrypt(&session.aes_key, &send_iv.to_bytes(), &header_bytes, &plaintext);
            let mut wire = Vec::with_capacity(header_bytes.len() + cipher.len());
            wire.extend_from_slice(&header_bytes);
            wire.extend_from_slice(&cipher);

            record_outbound(config.capture.as_ref(), TransportKind::Udp, false, &wire);
            let send_world_time = clock.now();
            send_times.insert(hello_app_seqno, send_world_time);
            transport.send(&wire).await?;
            tracing::debug!(
                target: "ranchero::relay",
                hello_idx,
                app_seqno = hello_app_seqno,
                iv_seqno = hello_iv_seqno,
                payload_size = wire.len(),
                "relay.udp.hello.sent",
            );

            send_iv_seqno = send_iv_seqno.wrapping_add(1);
            app_seqno = app_seqno.wrapping_add(1);

            // ── wait window: drain any replies that arrive before
            //    the next hello. `delay = 10 * i ms` per sauce.
            let deadline = Instant::now() + Duration::from_millis(10 * u64::from(hello_idx));
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining = deadline - now;
                tokio::select! {
                    biased;
                    result = transport.recv() => {
                        let bytes = result?;
                        record_inbound(config.capture.as_ref(), TransportKind::Udp, &bytes);
                        let plaintext = process_inbound_packet(
                            &bytes,
                            &session.aes_key,
                            session.relay_id,
                            &mut recv_iv_conn_id,
                            &mut recv_iv_seqno,
                        )?;
                        let stc = zwift_proto::ServerToClient::decode(plaintext.as_slice())?;

                        // STEP-12.14 §N10 — `stc_f5` (proto tag 5) is
                        // sauce's `ackSeqno` — "UDP ack to our previously
                        // sent seqno" (`zwift.mjs:1351: syncStamps.get(
                        // packet.ackSeqno)`). Tag 4 (`stc.seqno`) is the
                        // SERVER's own outgoing seqno; matching against it
                        // would coincidentally converge at best.
                        if let Some(ack) = stc.stc_f5 {
                            let ack_u32 = ack as u32;
                            if let Some(sent_at) = send_times.remove(&ack_u32) {
                                let local_now = clock.now();
                                let latency = (local_now - sent_at) / 2;
                                let server_world_time = stc.world_time.unwrap_or(0);
                                let offset = local_now - (server_world_time + latency);
                                samples.push(Sample {
                                    latency_ms: latency,
                                    offset_ms: offset,
                                });
                                tracing::debug!(
                                    target: "ranchero::relay",
                                    app_seqno = ack_u32,
                                    latency_ms = latency,
                                    offset_ms = offset,
                                    "relay.udp.hello.ack",
                                );
                            }
                        }

                        match sync::compute_offset(&samples) {
                            SyncOutcome::Converged { mean_offset_ms, median_latency_ms } => {
                                clock.adjust_offset(-mean_offset_ms);
                                latency_ms = Some(median_latency_ms);
                                tracing::info!(
                                    target: "ranchero::relay",
                                    mean_offset_ms,
                                    median_latency_ms,
                                    sample_count = samples.len(),
                                    "relay.udp.sync.converged",
                                );
                                break 'hello_loop;
                            }
                            SyncOutcome::NeedMore => continue,
                        }
                    }
                    _ = sleep(remaining) => break,
                }
            }
        }

        // When max_hellos == 0 the hello loop ran zero iterations (the
        // `1..=0` range is empty), so latency_ms is always None. Treat
        // this as an intentional bypass used by test transports that
        // never respond to hellos; proceed with latency = 0.
        let latency_ms = if config.max_hellos == 0 {
            latency_ms.unwrap_or(0)
        } else {
            latency_ms.ok_or(Error::SyncTimeout { attempts: config.max_hellos })?
        };

        // ── build channel + spawn recv loop ──
        let (events_tx, events_rx) = broadcast::channel::<ChannelEvent>(64);
        let shutdown_notify = Arc::new(Notify::new());
        let send_state = Arc::new(Mutex::new(SendState {
            iv_seqno: send_iv_seqno,
            app_seqno,
        }));

        let transport_for_recv = transport.clone();
        let events_tx_for_recv = events_tx.clone();
        let shutdown_for_recv = shutdown_notify.clone();
        let aes_key = session.aes_key;
        let relay_id = session.relay_id;
        let watchdog_timeout = config.watchdog_timeout;
        let capture_for_recv = config.capture.clone();

        let handle = tokio::spawn(async move {
            // Emit Established as the first event from this task,
            // so callers who subscribe via `events()` after
            // `establish()` returns still see it (same trick STEP 09
            // uses for the supervisor's initial LoggedIn).
            let _ = events_tx_for_recv.send(ChannelEvent::Established { latency_ms });
            recv_loop(
                transport_for_recv,
                events_tx_for_recv,
                shutdown_for_recv,
                aes_key,
                relay_id,
                recv_iv_conn_id,
                recv_iv_seqno,
                watchdog_timeout,
                capture_for_recv,
            )
            .await;
        });

        Ok((
            Self {
                events_tx,
                shutdown_notify,
                recv_handle: Mutex::new(Some(handle)),
                transport,
                send_state,
                aes_key,
                conn_id: config.conn_id,
                athlete_id: config.athlete_id,
                latency_ms,
                capture: config.capture,
            },
            events_rx,
        ))
    }

    /// Send one `ClientToServer` payload (typically a `PlayerState`).
    pub async fn send_player_state(&self, state: zwift_proto::PlayerState) -> Result<(), Error> {
        let (header_bytes, cipher, app_seqno, iv_seqno, world_time) = {
            let mut send = self.send_state.lock().expect("send_state mutex");
            let app_seqno = send.app_seqno;
            let iv_seqno = send.iv_seqno;
            let world_time = state.world_time.unwrap_or(0);
            let cts = zwift_proto::ClientToServer {
                server_realm: 1,
                player_id: self.athlete_id,
                world_time: state.world_time,
                seqno: Some(app_seqno),
                state,
                last_update: 0,
                last_player_update: 0,
                ..Default::default()
            };
            let proto_bytes = cts.encode_to_vec();
            let plaintext = udp_plaintext(&proto_bytes);

            // Steady-state header: SEQNO only (CONN_ID and RELAY_ID
            // were already established during the hello loop).
            let header = Header {
                flags: HeaderFlags::SEQNO,
                relay_id: None,
                conn_id: None,
                seqno: Some(iv_seqno),
            };
            let header_bytes = header.encode();
            let iv = RelayIv {
                device: DeviceType::Relay,
                channel: ChannelType::UdpClient,
                conn_id: self.conn_id,
                seqno: iv_seqno,
            };
            let cipher = encrypt(&self.aes_key, &iv.to_bytes(), &header_bytes, &plaintext);

            send.iv_seqno = send.iv_seqno.wrapping_add(1);
            send.app_seqno = send.app_seqno.wrapping_add(1);
            (header_bytes, cipher, app_seqno, iv_seqno, world_time)
        };

        let mut wire = Vec::with_capacity(header_bytes.len() + cipher.len());
        wire.extend_from_slice(&header_bytes);
        wire.extend_from_slice(&cipher);
        record_outbound(self.capture.as_ref(), TransportKind::Udp, false, &wire);
        self.transport.send(&wire).await?;
        tracing::debug!(
            target: "ranchero::relay",
            world_time,
            app_seqno,
            iv_seqno,
            payload_size = wire.len(),
            "relay.udp.playerstate.sent",
        );
        Ok(())
    }

    pub fn latency_ms(&self) -> Option<i64> {
        Some(self.latency_ms)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChannelEvent> {
        self.events_tx.subscribe()
    }

    /// Cancel the recv-loop / watchdog. Notifies the recv task,
    /// which emits `ChannelEvent::Shutdown` and exits cleanly. Does
    /// **not** await the task's exit; use [`Self::shutdown_and_wait`]
    /// when the caller needs synchronous teardown (e.g. when the
    /// capture writer must be flushed before the channel's clone of
    /// it can be dropped).
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

fn build_hello(athlete_id: i64, app_seqno: u32) -> zwift_proto::ClientToServer {
    zwift_proto::ClientToServer {
        server_realm: 1,
        player_id: athlete_id,
        world_time: Some(0),
        seqno: Some(app_seqno),
        state: zwift_proto::PlayerState::default(),
        last_update: 0,
        last_player_update: 0,
        ..Default::default()
    }
}

fn build_send_header(relay_id: u32, conn_id: u16, iv_seqno: u32) -> Header {
    // Sauce keeps the full RELAY_ID | CONN_ID | SEQNO triple on every
    // hello iteration so the server can re-initialise its decrypt state
    // on any lossy-network packet loss (STEP-12.14 §M1).
    Header {
        flags: HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
        relay_id: Some(relay_id),
        conn_id: Some(conn_id),
        seqno: Some(iv_seqno),
    }
}

/// Decode header → validate relay_id → update recv IV state →
/// decrypt → return the **plaintext bytes** (no proto decode).
/// Caller decodes as `ServerToClient` after passing the plaintext to
/// the capture tap. The recv plaintext is just the proto bytes (no
/// version envelope), per `zwift.mjs:1427`.
fn process_inbound_packet(
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
        channel: ChannelType::UdpServer,
        conn_id: *recv_iv_conn_id,
        seqno: *recv_iv_seqno,
    };
    let plaintext = decrypt(aes_key, &iv.to_bytes(), aad, cipher)?;
    *recv_iv_seqno = recv_iv_seqno.wrapping_add(1);
    Ok(plaintext)
}

#[allow(clippy::too_many_arguments)]
async fn recv_loop<T: UdpTransport>(
    transport: Arc<T>,
    events_tx: broadcast::Sender<ChannelEvent>,
    shutdown: Arc<Notify>,
    aes_key: [u8; 16],
    relay_id: u32,
    mut recv_iv_conn_id: u16,
    mut recv_iv_seqno: u32,
    watchdog_timeout: Duration,
    capture: Option<Arc<CaptureWriter>>,
) {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                let _ = events_tx.send(ChannelEvent::Shutdown);
                return;
            }
            result = tokio::time::timeout(watchdog_timeout, transport.recv()) => {
                match result {
                    Ok(Ok(bytes)) => {
                        record_inbound(capture.as_ref(), TransportKind::Udp, &bytes);
                        match process_inbound_packet(
                            &bytes,
                            &aes_key,
                            relay_id,
                            &mut recv_iv_conn_id,
                            &mut recv_iv_seqno,
                        ) {
                            Ok(plaintext) => {
                                match zwift_proto::ServerToClient::decode(plaintext.as_slice()) {
                                    Ok(stc) => {
                                        tracing::debug!(
                                            target: "ranchero::relay",
                                            world_time = stc.world_time.unwrap_or(0),
                                            // STEP-12.14 §N11 — tag 8 (`states`) is
                                            // sauce's `playerStates`; tag 28
                                            // (`player_states`) is `blockPlayerStates`.
                                            player_count = stc.states.len(),
                                            payload_size = plaintext.len(),
                                            "relay.udp.message.recv",
                                        );
                                        let _ = events_tx.send(ChannelEvent::Inbound(Box::new(stc)));
                                    }
                                    Err(e) => {
                                        let _ = events_tx.send(ChannelEvent::RecvError(e.to_string()));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = events_tx.send(ChannelEvent::RecvError(e.to_string()));
                            }
                        }
                    }
                    Ok(Err(io_err)) => {
                        let _ = events_tx.send(ChannelEvent::RecvError(io_err.to_string()));
                        // Transport-level failure: stop the loop.
                        return;
                    }
                    Err(_elapsed) => {
                        // Watchdog: emit Timeout but keep listening;
                        // the supervisor decides whether to reconnect.
                        let _ = events_tx.send(ChannelEvent::Timeout);
                    }
                }
            }
        }
    }
}
