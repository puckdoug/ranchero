// SPDX-License-Identifier: AGPL-3.0-only
//
// `UdpChannel` behavioral tests (STEP 10). All tests use an in-process
// `MockUdpTransport` (defined inline below) driven by `tokio::sync::mpsc`
// channels — no real socket, no network. Tests that need full hello-
// loop convergence spawn `establish()` in a background task and feed
// scripted replies through the mock.
//
// In red state, every test panics at `unimplemented!()` somewhere
// inside `UdpChannel`. The test setup, mock transport, and
// helper functions are real and will drive green-state implementation
// without modification.

use std::sync::Arc;
use std::time::Duration;

use prost::Message;
use tokio::sync::{Mutex, mpsc};
use tokio::time::Instant;
use zwift_proto::{ClientToServer, PlayerState, ServerToClient};
use zwift_relay::udp::{ChannelEvent, UdpChannel, UdpChannelConfig, UdpTransport};
use zwift_relay::{
    ChannelType, DeviceType, Header, HeaderFlags, RelaySession, RelayIv, TcpServer, WorldTimer,
    decode_header, decrypt, encrypt, parse_udp_plaintext,
};

// --- mock transport ------------------------------------------------

struct MockUdpTransport {
    inbox: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    outbox: mpsc::UnboundedSender<Vec<u8>>,
}

impl MockUdpTransport {
    fn new() -> (Self, MockHandle) {
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let transport = Self {
            inbox: Arc::new(Mutex::new(in_rx)),
            outbox: out_tx,
        };
        let handle = MockHandle {
            inbound_sender: in_tx,
            outbound_receiver: out_rx,
        };
        (transport, handle)
    }
}

impl UdpTransport for MockUdpTransport {
    async fn send(&self, bytes: &[u8]) -> std::io::Result<()> {
        let _ = self.outbox.send(bytes.to_vec());
        Ok(())
    }

    async fn recv(&self) -> std::io::Result<Vec<u8>> {
        let mut inbox = self.inbox.lock().await;
        inbox.recv().await.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "mock transport inbound channel closed",
            )
        })
    }
}

struct MockHandle {
    inbound_sender: mpsc::UnboundedSender<Vec<u8>>,
    outbound_receiver: mpsc::UnboundedReceiver<Vec<u8>>,
}

// --- helpers -------------------------------------------------------

const TEST_AES_KEY: [u8; 16] = [
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
];
const TEST_RELAY_ID: u32 = 0xDEAD_BEEF;
const TEST_CONN_ID: u16 = 0x1234;
const TEST_ATHLETE_ID: i64 = 4_242_424;

fn test_session() -> RelaySession {
    RelaySession {
        aes_key: TEST_AES_KEY,
        relay_id: TEST_RELAY_ID,
        tcp_servers: Vec::<TcpServer>::new(),
        expires_at: Instant::now() + Duration::from_secs(600),
        server_time_ms: None,
    }
}

fn test_config() -> UdpChannelConfig {
    UdpChannelConfig {
        course_id: 7,
        athlete_id: TEST_ATHLETE_ID,
        conn_id: TEST_CONN_ID,
        max_hellos: 6,
        min_sync_samples: 5,
        watchdog_timeout: Duration::from_millis(200),
        ..Default::default()
    }
}

/// Decode an outbound UDP packet that the channel sent: parse the
/// header, decrypt with the test session's key, strip the
/// `[u8 version=1]` envelope, decode the inner `ClientToServer`.
fn parse_outbound(bytes: &[u8]) -> (Header, ClientToServer) {
    let parsed = decode_header(bytes).expect("header decodes");
    let aad = &bytes[..parsed.consumed];
    let cipher = &bytes[parsed.consumed..];
    let conn_id = parsed.header.conn_id.unwrap_or(TEST_CONN_ID);
    let iv_seqno = parsed.header.seqno.unwrap_or(0);
    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::UdpClient,
        conn_id,
        seqno: iv_seqno,
    };
    let plaintext = decrypt(&TEST_AES_KEY, &iv.to_bytes(), aad, cipher).expect("decrypt outbound");
    let envelope = parse_udp_plaintext(&plaintext).expect("parse [version|proto] envelope");
    let cts = ClientToServer::decode(envelope.proto_bytes).expect("decode CTS");
    (parsed.header, cts)
}

/// Build an inbound packet that the test scripts back through the mock
/// transport. Inbound UDP plaintext is the raw `ServerToClient` proto
/// bytes (no version envelope; sauce `zwift.mjs:1427`).
fn build_inbound(recv_iv_seqno: u32, ack_seqno: u32, world_time_ms: i64) -> Vec<u8> {
    // The client carries `seqno: u32` in `ClientToServer`; the server
    // echoes it as `seqno: i32` in `ServerToClient`. Cast at this
    // boundary; in tests the value fits trivially.
    let stc = ServerToClient {
        seqno: Some(ack_seqno as i32),
        world_time: Some(world_time_ms),
        ..Default::default()
    };
    let proto_bytes = stc.encode_to_vec();

    let header = Header {
        flags: HeaderFlags::SEQNO,
        relay_id: None,
        conn_id: None,
        seqno: Some(recv_iv_seqno),
    };
    let header_bytes = header.encode();

    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::UdpServer,
        conn_id: TEST_CONN_ID,
        seqno: recv_iv_seqno,
    };
    let cipher = encrypt(&TEST_AES_KEY, &iv.to_bytes(), &header_bytes, &proto_bytes);

    let mut wire = Vec::with_capacity(header_bytes.len() + cipher.len());
    wire.extend_from_slice(&header_bytes);
    wire.extend_from_slice(&cipher);
    wire
}

// --- 1-3. hello packet shape ---------------------------------------

#[tokio::test]
async fn establish_first_hello_carries_relay_conn_seqno_flags() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    let bytes = handle
        .outbound_receiver
        .recv()
        .await
        .expect("first hello sent");
    let (header, _cts) = parse_outbound(&bytes);
    assert!(header.flags.contains(HeaderFlags::RELAY_ID));
    assert!(header.flags.contains(HeaderFlags::CONN_ID));
    assert!(header.flags.contains(HeaderFlags::SEQNO));
    assert_eq!(header.relay_id, Some(TEST_RELAY_ID));
    assert_eq!(header.conn_id, Some(TEST_CONN_ID));
    assert_eq!(header.seqno, Some(0));

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn establish_subsequent_hellos_carry_seqno_only() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    let _first = handle.outbound_receiver.recv().await.expect("hello 1");
    let second = handle.outbound_receiver.recv().await.expect("hello 2");
    let (header, _cts) = parse_outbound(&second);
    assert!(!header.flags.contains(HeaderFlags::RELAY_ID));
    assert!(!header.flags.contains(HeaderFlags::CONN_ID));
    assert!(header.flags.contains(HeaderFlags::SEQNO));
    assert_eq!(header.seqno, Some(1));

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn establish_hello_payload_athlete_id_realm_one_world_time_zero() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    let bytes = handle.outbound_receiver.recv().await.expect("first hello");
    let (_header, cts) = parse_outbound(&bytes);
    assert_eq!(cts.player_id, TEST_ATHLETE_ID);
    assert_eq!(cts.server_realm, 1);
    assert_eq!(cts.world_time, Some(0));

    task.abort();
    let _ = task.await;
}

// --- 4. app-seqno increments ---------------------------------------

#[tokio::test]
async fn establish_increments_app_seqno_per_hello() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    let h1 = handle.outbound_receiver.recv().await.expect("h1");
    let h2 = handle.outbound_receiver.recv().await.expect("h2");
    let h3 = handle.outbound_receiver.recv().await.expect("h3");
    let (_, c1) = parse_outbound(&h1);
    let (_, c2) = parse_outbound(&h2);
    let (_, c3) = parse_outbound(&h3);
    assert_eq!(c1.seqno, Some(0));
    assert_eq!(c2.seqno, Some(1));
    assert_eq!(c3.seqno, Some(2));

    task.abort();
    let _ = task.await;
}

// --- 5. timeout when no replies ------------------------------------

#[tokio::test]
async fn establish_max_hellos_then_sync_timeout() {
    let (transport, _handle) = MockUdpTransport::new();
    let session = test_session();
    let config = UdpChannelConfig {
        max_hellos: 3, // keep test short — green state pays ~60 ms
        ..test_config()
    };

    let result = UdpChannel::establish(transport, &session, WorldTimer::new(), config).await;
    match result {
        Err(zwift_relay::UdpError::SyncTimeout { attempts }) => {
            assert_eq!(attempts, 3);
        }
        Ok(_) => panic!("expected SyncTimeout, got Ok"),
        Err(other) => panic!("expected SyncTimeout, got {other:?}"),
    }
}

// --- 6. converge with scripted replies -----------------------------

#[tokio::test]
async fn establish_converges_after_six_replies_and_emits_established() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();
    let clock = WorldTimer::new();

    // Spawn the establish task and concurrently script tightly-spaced
    // replies as each hello arrives.
    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    // 6 hellos → 6 replies. Latencies vary slightly so the SNTP filter
    // can converge (all-equal latencies fail sauce's `> 4 valid` check
    // because every sample is exactly the median, so stddev is 0 and
    // none survive `|x - median| < stddev`).
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("hello carries app seqno");
        // World time advances; latencies inferred by the filter come
        // from real-time wall clock, so they'll be ~ms-scale here.
        let world_time = 1_000_000 + i64::from(i) * 100;
        let reply = build_inbound(i, ack, world_time);
        handle.inbound_sender.send(reply).expect("script reply");
    }

    let (_channel, mut events) = task
        .await
        .expect("establish task")
        .expect("sync converges");
    let first = events.recv().await.expect("first event");
    assert!(matches!(first, ChannelEvent::Established { .. }));
}

// --- 7-8. recv loop ------------------------------------------------

#[tokio::test]
async fn recv_loop_emits_inbound_event_per_decoded_packet() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();
    let clock = WorldTimer::new();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    // Drive sync to convergence first.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (_channel, mut events) = task.await.expect("task").expect("converged");
    // Drain Established.
    let _ = events.recv().await.expect("Established");

    // Now feed two more inbound packets and expect Inbound events.
    handle
        .inbound_sender
        .send(build_inbound(6, 100, 2_000_000))
        .expect("inbound 1");
    handle
        .inbound_sender
        .send(build_inbound(7, 101, 2_000_100))
        .expect("inbound 2");

    let ev1 = events.recv().await.expect("ev1");
    let ev2 = events.recv().await.expect("ev2");
    match (ev1, ev2) {
        (ChannelEvent::Inbound(a), ChannelEvent::Inbound(b)) => {
            assert_eq!(a.seqno, Some(100));
            assert_eq!(b.seqno, Some(101));
        }
        (a, b) => panic!("expected two Inbound events, got {a:?}, {b:?}"),
    }
}

#[tokio::test]
async fn recv_loop_decryption_failure_emits_recv_error() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();
    let clock = WorldTimer::new();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    // Establish first.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (_channel, mut events) = task.await.expect("task").expect("converged");
    let _ = events.recv().await.expect("Established");

    // Push a packet whose tag has been flipped — channel must surface
    // it as RecvError, not panic.
    let mut tampered = build_inbound(6, 99, 1_000_000);
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    handle
        .inbound_sender
        .send(tampered)
        .expect("tampered inbound");

    let ev = events.recv().await.expect("recv error event");
    assert!(matches!(ev, ChannelEvent::RecvError(_)), "got {ev:?}");
}

// --- 9. watchdog ---------------------------------------------------

#[tokio::test]
async fn watchdog_fires_after_silence() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = UdpChannelConfig {
        watchdog_timeout: Duration::from_millis(200),
        ..test_config()
    };
    let clock = WorldTimer::new();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (_channel, mut events) = task.await.expect("task").expect("converged");
    let _ = events.recv().await.expect("Established");

    // No more replies; after watchdog_timeout the channel must emit
    // Timeout (not shut down — supervisor decides).
    let next =
        tokio::time::timeout(Duration::from_millis(500), events.recv())
            .await
            .expect("event arrives within budget")
            .expect("event");
    assert!(matches!(next, ChannelEvent::Timeout), "got {next:?}");
}

// --- 10. send player state -----------------------------------------

#[tokio::test]
async fn send_player_state_emits_packet_with_seqno_flag_only() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();
    let clock = WorldTimer::new();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    // Establish.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (channel, _events) = task.await.expect("task").expect("converged");

    // Send a player state and inspect the resulting wire packet.
    let state = PlayerState {
        id: Some(TEST_ATHLETE_ID),
        power: Some(250),
        ..Default::default()
    };
    channel.send_player_state(state).await.expect("send");

    let bytes = handle
        .outbound_receiver
        .recv()
        .await
        .expect("send produced a packet");
    let (header, cts) = parse_outbound(&bytes);
    assert!(!header.flags.contains(HeaderFlags::RELAY_ID));
    assert!(!header.flags.contains(HeaderFlags::CONN_ID));
    assert!(header.flags.contains(HeaderFlags::SEQNO));
    assert_eq!(cts.state.power, Some(250));
}

// --- 11. shutdown --------------------------------------------------

#[tokio::test]
async fn shutdown_stops_recv_loop_and_emits_shutdown_event() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();
    let clock = WorldTimer::new();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (channel, mut events) = task.await.expect("task").expect("converged");
    let _ = events.recv().await.expect("Established");

    channel.shutdown();
    let next = tokio::time::timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("shutdown event within budget")
        .expect("event");
    assert!(matches!(next, ChannelEvent::Shutdown), "got {next:?}");
}

// --- 12. cheap compile-time wiring sanity --------------------------

#[test]
fn channel_event_is_clone_for_broadcast() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<ChannelEvent>();
}

// --- 13-14. capture tap (STEP 11.5) --------------------------------

#[tokio::test]
async fn udp_channel_with_capture_records_inbound_packets() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path())
        .await
        .expect("capture writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let mut config = test_config();
    config.capture = Some(writer.clone());
    let clock = WorldTimer::new();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    // Drive sync to convergence with 6 replies.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (channel, _events) = task.await.expect("task").expect("converged");

    channel.shutdown_and_wait().await;
    drop(channel);

    let writer = std::sync::Arc::try_unwrap(writer)
        .expect("only the test owns the writer once the channel drops it");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let inbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Inbound && r.transport == TransportKind::Udp)
        .collect();
    assert!(
        inbound.len() >= 6,
        "expected at least 6 inbound captures (one per reply), got {}",
        inbound.len(),
    );
}

#[tokio::test]
async fn udp_channel_with_capture_records_outbound_player_state() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path())
        .await
        .expect("capture writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let mut config = test_config();
    config.capture = Some(writer.clone());
    let clock = WorldTimer::new();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, clock, config).await
    });

    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (channel, _events) = task.await.expect("task").expect("converged");

    // Steady-state sends.
    for power in 100u32..=104 {
        let state = PlayerState {
            id: Some(TEST_ATHLETE_ID),
            power: Some(power as i32),
            ..Default::default()
        };
        channel.send_player_state(state).await.expect("send");
        // Drain the outbound mock so the channel doesn't block.
        let _ = handle.outbound_receiver.recv().await;
    }

    channel.shutdown_and_wait().await;
    drop(channel);

    let writer = std::sync::Arc::try_unwrap(writer).expect("only test owner");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let outbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Outbound && r.transport == TransportKind::Udp)
        .collect();
    // 6 hellos + 5 steady-state = 11 outbound records with wire-byte capture.
    assert!(
        outbound.len() >= 5,
        "expected at least 5 outbound captures, got {}",
        outbound.len(),
    );
    // Captured payload is now the encrypted wire bytes (header + ciphertext + tag).
    // Decode the last capture via parse_outbound to verify the original payload.
    let last = outbound.last().expect("at least one");
    let (_header, decoded) = parse_outbound(&last.payload);
    assert_eq!(decoded.player_id, TEST_ATHLETE_ID);
    assert_eq!(decoded.state.power, Some(104));
}

// ==========================================================================
// Phase 2a (STEP-12.12) — UDP capture correctness and tracing coverage.
//
// Red state: record_outbound / record_inbound calls in udp.rs record
// proto_bytes (pre-encryption for outbound) and plaintext (post-decryption
// for inbound) rather than the wire bytes that crossed the socket. The
// tracing events relay.udp.hello.started / hello.sent / hello.ack /
// sync.converged / playerstate.sent / message.recv are not yet emitted.
// ==========================================================================

// --- 15. hello outbound capture correctness ----------------------------

#[tokio::test]
async fn udp_hello_send_records_encrypted_datagram() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let mut config = test_config();
    config.capture = Some(writer.clone());

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    // Receive the first hello wire bytes from the mock transport.
    let first_hello_wire = handle.outbound_receiver.recv().await.expect("first hello");

    // Abort before more sends occur so the capture holds exactly one outbound record.
    task.abort();
    let _ = task.await;

    let writer = std::sync::Arc::try_unwrap(writer).expect("sole owner after task abort");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let outbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Outbound && r.transport == TransportKind::Udp)
        .collect();

    assert_eq!(outbound.len(), 1, "expected exactly one outbound capture from one hello");
    assert_eq!(
        outbound[0].payload, first_hello_wire,
        "Phase 2a red state: hello outbound capture must hold the encrypted wire bytes \
         sent to transport.send() (header + ciphertext + tag), not proto_bytes; \
         expected {} bytes, captured {} bytes",
        first_hello_wire.len(),
        outbound[0].payload.len(),
    );
}

// --- 16. hello inbound capture correctness -----------------------------

#[tokio::test]
async fn udp_hello_recv_records_raw_datagram_pre_decrypt() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let mut config = test_config();
    config.capture = Some(writer.clone());

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    // Send the first hello and script exactly one reply; save the raw wire bytes.
    let first_hello = handle.outbound_receiver.recv().await.expect("hello 1");
    let (_h, cts) = parse_outbound(&first_hello);
    let ack = cts.seqno.expect("seqno");
    let raw_reply = build_inbound(0, ack, 1_000_000);
    handle.inbound_sender.send(raw_reply.clone()).expect("script reply");

    // Give the channel time to process the reply, then abort.
    tokio::time::sleep(Duration::from_millis(50)).await;
    task.abort();
    let _ = task.await;

    let writer = std::sync::Arc::try_unwrap(writer).expect("sole owner after task abort");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let inbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Inbound && r.transport == TransportKind::Udp)
        .collect();

    assert_eq!(inbound.len(), 1, "expected exactly one inbound capture");
    assert_eq!(
        inbound[0].payload, raw_reply,
        "Phase 2a red state: hello inbound capture must hold the raw datagram bytes \
         as returned from transport.recv() (header + ciphertext + tag), not \
         post-decryption plaintext; expected {} bytes, captured {} bytes",
        raw_reply.len(),
        inbound[0].payload.len(),
    );
}

// --- 17. steady-state outbound capture correctness ---------------------

#[tokio::test]
async fn udp_steady_state_send_records_encrypted_datagram() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let mut config = test_config();
    config.capture = Some(writer.clone());

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    // Establish with 6 replies; drain each hello from the outbound channel.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        handle
            .inbound_sender
            .send(build_inbound(i, ack, 1_000_000 + i64::from(i) * 100))
            .expect("reply");
    }
    let (channel, _events) = task.await.expect("task").expect("converged");

    // Send one player state and capture the wire bytes from the mock transport.
    let state = PlayerState {
        id: Some(TEST_ATHLETE_ID),
        power: Some(999),
        ..Default::default()
    };
    channel.send_player_state(state).await.expect("send");
    let wire = handle.outbound_receiver.recv().await.expect("steady-state wire");

    channel.shutdown_and_wait().await;
    drop(channel);

    let writer = std::sync::Arc::try_unwrap(writer).expect("sole owner after channel drop");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let outbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Outbound && r.transport == TransportKind::Udp)
        .collect();

    // 6 hellos + 1 steady-state = at least 7 outbound records.
    assert!(
        outbound.len() >= 7,
        "expected at least 7 outbound captures (6 hellos + 1 steady-state), got {}",
        outbound.len(),
    );
    let steady = outbound.last().expect("at least one");
    assert_eq!(
        steady.payload, wire,
        "Phase 2a red state: steady-state outbound capture must hold the encrypted \
         wire bytes sent to transport.send() (header + ciphertext + tag), not \
         proto_bytes; expected {} bytes, captured {} bytes",
        wire.len(),
        steady.payload.len(),
    );
}

// --- 18. steady-state inbound capture correctness ----------------------

#[tokio::test]
async fn udp_steady_state_recv_records_raw_datagram_pre_decrypt() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let mut config = test_config();
    config.capture = Some(writer.clone());

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    // Establish.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        handle
            .inbound_sender
            .send(build_inbound(i, ack, 1_000_000 + i64::from(i) * 100))
            .expect("reply");
    }
    let (channel, mut events) = task.await.expect("task").expect("converged");
    let _ = events.recv().await.expect("Established");

    // Script one steady-state inbound packet (recv_iv_seqno = 6 after 6 hellos).
    let raw_packet = build_inbound(6, 100, 2_000_000);
    handle.inbound_sender.send(raw_packet.clone()).expect("steady-state inbound");

    let ev = tokio::time::timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("event within budget")
        .expect("event");
    assert!(matches!(ev, ChannelEvent::Inbound(_)), "got {ev:?}");

    channel.shutdown_and_wait().await;
    drop(channel);

    let writer = std::sync::Arc::try_unwrap(writer).expect("sole owner after channel drop");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let inbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Inbound && r.transport == TransportKind::Udp)
        .collect();

    // 6 hello-phase inbounds + 1 steady-state = at least 7.
    assert!(
        inbound.len() >= 7,
        "expected at least 7 inbound captures (6 hello replies + 1 steady-state), got {}",
        inbound.len(),
    );
    let steady = inbound.last().expect("at least one");
    assert_eq!(
        steady.payload, raw_packet,
        "Phase 2a red state: steady-state inbound capture must hold the raw datagram \
         bytes from transport.recv() (header + ciphertext + tag), not post-decryption \
         plaintext; expected {} bytes, captured {} bytes",
        raw_packet.len(),
        steady.payload.len(),
    );
}

// --- 19. hello tracing — started + per-attempt sent -------------------

#[tokio::test]
#[tracing_test::traced_test]
async fn udp_hello_emits_started_and_per_attempt_sent_events() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = UdpChannelConfig {
        max_hellos: 3,
        ..test_config()
    };

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    // Drain 3 hellos so the loop runs to max_hellos.
    for _ in 0..3 {
        let _ = handle.outbound_receiver.recv().await.expect("hello");
    }
    let _ = task.await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.hello.started"),
        "Phase 2a red state: relay.udp.hello.started must be emitted once at info \
         before the first hello is sent; not found in tracing log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.hello.sent"),
        "Phase 2a red state: relay.udp.hello.sent must be emitted at debug for each \
         hello attempt; not found in tracing log",
    );
}

// --- 20. hello tracing — ack per response + converged ------------------

#[tokio::test]
#[tracing_test::traced_test]
async fn udp_hello_recv_emits_ack_per_response_and_one_converged_event() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        let reply = build_inbound(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }
    let (channel, _events) = task.await.expect("task").expect("converged");
    channel.shutdown_and_wait().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.hello.ack"),
        "Phase 2a red state: relay.udp.hello.ack must be emitted at debug for each \
         hello ack received; not found in tracing log",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.sync.converged"),
        "Phase 2a red state: relay.udp.sync.converged must be emitted at info once \
         when the SNTP filter converges; not found in tracing log",
    );
}

// --- 21. steady-state outbound tracing ---------------------------------

#[tokio::test]
#[tracing_test::traced_test]
async fn udp_steady_state_send_emits_relay_udp_playerstate_sent() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        handle
            .inbound_sender
            .send(build_inbound(i, ack, 1_000_000 + i64::from(i) * 100))
            .expect("reply");
    }
    let (channel, _events) = task.await.expect("task").expect("converged");

    let state = PlayerState {
        id: Some(TEST_ATHLETE_ID),
        power: Some(123),
        ..Default::default()
    };
    channel.send_player_state(state).await.expect("send");
    let _ = handle.outbound_receiver.recv().await;

    channel.shutdown_and_wait().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.playerstate.sent"),
        "Phase 2a red state: relay.udp.playerstate.sent must be emitted at debug for \
         each send_player_state call; not found in tracing log",
    );
}

// --- 22. steady-state inbound tracing ----------------------------------

#[tokio::test]
#[tracing_test::traced_test]
async fn udp_steady_state_recv_emits_relay_udp_message_recv_with_fields() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        handle
            .inbound_sender
            .send(build_inbound(i, ack, 1_000_000 + i64::from(i) * 100))
            .expect("reply");
    }
    let (_channel, mut events) = task.await.expect("task").expect("converged");
    let _ = events.recv().await.expect("Established");

    // Push one steady-state inbound packet.
    handle
        .inbound_sender
        .send(build_inbound(6, 100, 2_000_000))
        .expect("inbound");
    let ev = tokio::time::timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("event within budget")
        .expect("event");
    assert!(matches!(ev, ChannelEvent::Inbound(_)), "got {ev:?}");

    _channel.shutdown_and_wait().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.message.recv"),
        "Phase 2a red state: relay.udp.message.recv must be emitted at debug for each \
         decoded inbound ServerToClient; not found in tracing log",
    );
    assert!(
        !tracing_test::internal::logs_with_scope_contain("ranchero", "relay.udp.inbound"),
        "Phase 2a red state: bare relay.udp.inbound must be replaced by \
         relay.udp.message.recv; found the old event name in tracing log",
    );
}

// --- STEP-12.14 §N10 / §N11 — Phase 1a tests ---------------------------
//
// N10: the hello-ack matcher must read the ack-seqno from
// `ServerToClient.stc_f5` (proto tag 5 = sauce's `ackSeqno` —
// "UDP ack to our previously sent seqno"). We currently read
// `stc.seqno` (tag 4), which is the SERVER's own outgoing seqno
// and has no relationship to our outgoing hello seqnos. Sync
// "converges" today only by coincidence (when the server's seqno
// happens to match a value we sent); against live Zwift, sync
// would never converge.
//
// N11: the `relay.udp.message.recv` debug event currently reports
// `player_count = stc.player_states.len()` — but `stc.player_states`
// is zoffline's name for tag 28 (= sauce's `blockPlayerStates`,
// the BLOCKED list). The actual player states are at tag 8
// (`stc.states`), which the daemon correctly uses elsewhere.

/// Build an inbound packet that carries the ack-seqno in `stc_f5`
/// (proto tag 5 = sauce's `ackSeqno`), leaving `stc.seqno` (tag 4
/// = the server's own outgoing seqno) at `None`. This mirrors what
/// Zwift's real UDP server does — sauce's hello-ack matcher
/// (`zwift.mjs:1351`) reads `packet.ackSeqno`, not `packet.seqno`.
fn build_inbound_ack_at_tag_5(
    recv_iv_seqno: u32,
    ack_seqno: u32,
    world_time_ms: i64,
) -> Vec<u8> {
    let stc = ServerToClient {
        seqno: None,                      // tag 4 — server's own seqno (unused)
        stc_f5: Some(ack_seqno as i32),   // tag 5 — sauce's `ackSeqno`
        world_time: Some(world_time_ms),
        ..Default::default()
    };
    let proto_bytes = stc.encode_to_vec();
    let header = Header {
        flags: HeaderFlags::SEQNO,
        relay_id: None,
        conn_id: None,
        seqno: Some(recv_iv_seqno),
    };
    let header_bytes = header.encode();
    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::UdpServer,
        conn_id: TEST_CONN_ID,
        seqno: recv_iv_seqno,
    };
    let cipher = encrypt(&TEST_AES_KEY, &iv.to_bytes(), &header_bytes, &proto_bytes);
    let mut wire = Vec::with_capacity(header_bytes.len() + cipher.len());
    wire.extend_from_slice(&header_bytes);
    wire.extend_from_slice(&cipher);
    wire
}

#[tokio::test]
async fn udp_hello_ack_matcher_reads_ackseqno_at_proto_tag_5_not_tag_4() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    // Drive 6 hello/ack pairs (MIN_SYNC_SAMPLES=5; need >5 = 6+ to
    // converge). Each ack puts the matched seqno in `stc_f5` (tag 5)
    // and leaves `stc.seqno` (tag 4) None — exactly what sauce's
    // matcher reads.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("client outgoing seqno");
        let reply = build_inbound_ack_at_tag_5(i, ack, 1_000_000 + i64::from(i) * 100);
        handle.inbound_sender.send(reply).expect("reply");
    }

    let result = task.await.expect("task join");
    if let Err(e) = result {
        panic!(
            "STEP-12.14 §N10: UDP hello sync must converge when the server \
             echoes the ack-seqno in `stc.stc_f5` (proto tag 5 = sauce's \
             `ackSeqno`). Today the matcher reads `stc.seqno` (tag 4 = the \
             server's own outgoing seqno) and ignores tag 5, so sync never \
             finds a sample. Got error: {e}",
        );
    }
}

#[tokio::test]
#[tracing_test::traced_test]
async fn udp_recv_trace_player_count_uses_states_tag_8_not_player_states_tag_28() {
    let (transport, mut handle) = MockUdpTransport::new();
    let session = test_session();
    let config = test_config();

    let task = tokio::spawn(async move {
        UdpChannel::establish(transport, &session, WorldTimer::new(), config).await
    });

    // Drive convergence first.
    for i in 0..6u32 {
        let hello = handle.outbound_receiver.recv().await.expect("hello");
        let (_h, cts) = parse_outbound(&hello);
        let ack = cts.seqno.expect("seqno");
        handle
            .inbound_sender
            .send(build_inbound(i, ack, 1_000_000 + i64::from(i) * 100))
            .expect("reply");
    }
    let (channel, mut events) = task.await.expect("task").expect("converged");
    let _ = events.recv().await.expect("Established");

    // Build a steady-state inbound packet with `states` (tag 8 =
    // sauce's `playerStates`) populated to 3 entries and
    // `player_states` (zoffline's misleading name for tag 28 =
    // sauce's `blockPlayerStates`) left empty. The `relay.udp.message.recv`
    // trace must report `player_count=3`, NOT `player_count=0`.
    let stc = ServerToClient {
        states: vec![
            PlayerState { id: Some(101), ..Default::default() },
            PlayerState { id: Some(102), ..Default::default() },
            PlayerState { id: Some(103), ..Default::default() },
        ],
        player_states: vec![], // tag 28, deliberately empty
        seqno: None,
        stc_f5: Some(100),
        world_time: Some(2_000_000),
        ..Default::default()
    };
    let proto_bytes = stc.encode_to_vec();
    let header = Header {
        flags: HeaderFlags::SEQNO,
        relay_id: None,
        conn_id: None,
        seqno: Some(6),
    };
    let header_bytes = header.encode();
    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::UdpServer,
        conn_id: TEST_CONN_ID,
        seqno: 6,
    };
    let cipher = encrypt(&TEST_AES_KEY, &iv.to_bytes(), &header_bytes, &proto_bytes);
    let mut wire = Vec::with_capacity(header_bytes.len() + cipher.len());
    wire.extend_from_slice(&header_bytes);
    wire.extend_from_slice(&cipher);
    handle.inbound_sender.send(wire).expect("inbound");

    let _ = tokio::time::timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("event within budget")
        .expect("event");

    channel.shutdown_and_wait().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "player_count=3"),
        "STEP-12.14 §N11: relay.udp.message.recv must report \
         `player_count = stc.states.len()` (tag 8 = sauce's `playerStates`), \
         not `stc.player_states.len()` (tag 28 = sauce's `blockPlayerStates`). \
         The inbound packet had 3 entries in tag 8 and 0 in tag 28; the trace \
         must show `player_count=3`.",
    );
    assert!(
        !tracing_test::internal::logs_with_scope_contain("ranchero", "player_count=0"),
        "STEP-12.14 §N11: trace currently reads from tag 28 (the blocked \
         list, empty in this test) so reports `player_count=0`. After the \
         fix it must read from tag 8.",
    );
}
