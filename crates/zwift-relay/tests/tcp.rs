// SPDX-License-Identifier: AGPL-3.0-only
//
// `TcpChannel` behavioral tests (STEP 11). Same shape as STEP 10's
// UDP tests but with stream-oriented framing: tests script raw bytes
// (possibly partial frames) into a `MockTcpTransport`, and the
// channel's recv loop must accumulate across reads + drive
// `next_tcp_frame` to slice out complete frames.

use std::sync::Arc;
use std::time::Duration;

use prost::Message;
use tokio::sync::{Mutex, mpsc};
use tokio::time::Instant;
use zwift_proto::{ClientToServer, PlayerState, ServerToClient};
use zwift_relay::{
    ChannelType, DeviceType, Header, HeaderFlags, RelaySession, RelayIv, TcpChannel,
    TcpChannelConfig, TcpChannelEvent, TcpServer, TcpTransport, decode_header, decrypt, encrypt,
    frame_tcp, next_tcp_frame, parse_tcp_plaintext,
};

// --- mock transport ------------------------------------------------

struct MockTcpTransport {
    inbox: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    outbox: mpsc::UnboundedSender<Vec<u8>>,
}

impl MockTcpTransport {
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

impl TcpTransport for MockTcpTransport {
    async fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
        let _ = self.outbox.send(bytes.to_vec());
        Ok(())
    }

    async fn read_chunk(&self) -> std::io::Result<Vec<u8>> {
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
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
];
const TEST_RELAY_ID: u32 = 0xCAFE_F00D;
const TEST_CONN_ID: u16 = 0x5678;
const TEST_ATHLETE_ID: i64 = 8_675_309;

fn test_session() -> RelaySession {
    RelaySession {
        aes_key: TEST_AES_KEY,
        relay_id: TEST_RELAY_ID,
        tcp_servers: Vec::<TcpServer>::new(),
        expires_at: Instant::now() + Duration::from_secs(600),
        server_time_ms: None,
    }
}

fn test_config() -> TcpChannelConfig {
    TcpChannelConfig {
        athlete_id: TEST_ATHLETE_ID,
        conn_id: TEST_CONN_ID,
        watchdog_timeout: Duration::from_millis(200),
        ..Default::default()
    }
}

fn test_payload(seqno: u32) -> ClientToServer {
    ClientToServer {
        server_realm: 1,
        player_id: TEST_ATHLETE_ID,
        world_time: Some(0),
        seqno: Some(seqno),
        state: PlayerState::default(),
        last_update: 0,
        last_player_update: 0,
        ..Default::default()
    }
}

/// Decode an outbound TCP frame: strip the BE u16 size prefix,
/// decode the header, decrypt with the test session's key + the
/// `TcpClient` IV direction, strip the `[2, hello?, …]` envelope,
/// decode the inner `ClientToServer`. Returns the parsed pieces.
fn parse_outbound_tcp(bytes: &[u8]) -> (Header, /*hello*/ bool, ClientToServer) {
    let (frame_payload, _consumed) = next_tcp_frame(bytes)
        .expect("frame parses")
        .expect("complete frame");
    let parsed = decode_header(frame_payload).expect("header decodes");
    let aad = &frame_payload[..parsed.consumed];
    let cipher = &frame_payload[parsed.consumed..];
    let conn_id = parsed.header.conn_id.unwrap_or(TEST_CONN_ID);
    let iv_seqno = parsed.header.seqno.unwrap_or(0);
    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::TcpClient,
        conn_id,
        seqno: iv_seqno,
    };
    let plaintext = decrypt(&TEST_AES_KEY, &iv.to_bytes(), aad, cipher).expect("decrypt outbound");
    let envelope = parse_tcp_plaintext(&plaintext).expect("[2, hello?, proto] envelope");
    let cts = ClientToServer::decode(envelope.proto_bytes).expect("decode CTS");
    (parsed.header, envelope.hello, cts)
}

/// Build an inbound TCP frame: encode the `ServerToClient` proto
/// directly (no envelope — sauce `zwift.mjs:1285-1286`), encrypt
/// with the `TcpServer` IV direction, prepend header + BE u16 size.
fn build_inbound_tcp(recv_iv_seqno: u32, stc: &ServerToClient) -> Vec<u8> {
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
        channel: ChannelType::TcpServer,
        conn_id: TEST_CONN_ID,
        seqno: recv_iv_seqno,
    };
    let cipher = encrypt(&TEST_AES_KEY, &iv.to_bytes(), &header_bytes, &proto_bytes);
    frame_tcp(&header_bytes, &cipher)
}

fn test_stc(seqno: i32) -> ServerToClient {
    ServerToClient {
        seqno: Some(seqno),
        world_time: Some(1_000_000),
        ..Default::default()
    }
}

// --- 1-4. send packet shape ----------------------------------------

#[tokio::test]
async fn send_packet_hello_carries_full_iv_flags_and_hello_byte_zero() {
    let (transport, mut handle) = MockTcpTransport::new();
    let (channel, _events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");

    channel
        .send_packet(test_payload(0), /* hello */ true)
        .await
        .expect("send");

    let bytes = handle
        .outbound_receiver
        .recv()
        .await
        .expect("hello packet sent");
    let (header, hello, _cts) = parse_outbound_tcp(&bytes);
    assert!(header.flags.contains(HeaderFlags::RELAY_ID));
    assert!(header.flags.contains(HeaderFlags::CONN_ID));
    assert!(header.flags.contains(HeaderFlags::SEQNO));
    assert_eq!(header.relay_id, Some(TEST_RELAY_ID));
    assert_eq!(header.conn_id, Some(TEST_CONN_ID));
    assert!(hello, "hello=true → plaintext envelope hello byte must be 0");

    channel.shutdown();
}

#[tokio::test]
async fn send_packet_steady_carries_seqno_only_and_hello_byte_one() {
    let (transport, mut handle) = MockTcpTransport::new();
    let (channel, _events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");

    channel
        .send_packet(test_payload(0), /* hello */ false)
        .await
        .expect("send");

    let bytes = handle.outbound_receiver.recv().await.expect("steady packet");
    let (header, hello, _cts) = parse_outbound_tcp(&bytes);
    assert!(!header.flags.contains(HeaderFlags::RELAY_ID));
    assert!(!header.flags.contains(HeaderFlags::CONN_ID));
    assert!(header.flags.contains(HeaderFlags::SEQNO));
    assert!(!hello, "hello=false → plaintext envelope hello byte must be 1");

    channel.shutdown();
}

#[tokio::test]
async fn send_packet_increments_iv_seqno() {
    let (transport, mut handle) = MockTcpTransport::new();
    let (channel, _events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");

    channel
        .send_packet(test_payload(0), true)
        .await
        .expect("send 1");
    channel
        .send_packet(test_payload(1), false)
        .await
        .expect("send 2");

    let p1 = handle.outbound_receiver.recv().await.expect("p1");
    let p2 = handle.outbound_receiver.recv().await.expect("p2");
    let (h1, _, _) = parse_outbound_tcp(&p1);
    let (h2, _, _) = parse_outbound_tcp(&p2);
    assert_eq!(h1.seqno, Some(0));
    assert_eq!(h2.seqno, Some(1));

    channel.shutdown();
}

#[tokio::test]
async fn send_packet_prepends_be_u16_size_prefix() {
    let (transport, mut handle) = MockTcpTransport::new();
    let (channel, _events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");

    channel
        .send_packet(test_payload(0), false)
        .await
        .expect("send");

    let bytes = handle.outbound_receiver.recv().await.expect("packet");
    assert!(
        bytes.len() >= 2,
        "wire packet must include 2-byte size prefix",
    );
    let advertised = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    assert_eq!(
        advertised,
        bytes.len() - 2,
        "BE u16 size must equal header.len() + ciphertext.len()",
    );

    channel.shutdown();
}

// --- 5-10. recv-side framing ---------------------------------------

#[tokio::test]
async fn recv_decodes_complete_frame() {
    let (transport, handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established"); // drain

    let frame = build_inbound_tcp(0, &test_stc(42));
    handle.inbound_sender.send(frame).expect("inject frame");

    let ev = events.recv().await.expect("event");
    match ev {
        TcpChannelEvent::Inbound(stc) => assert_eq!(stc.seqno, Some(42)),
        other => panic!("expected Inbound, got {other:?}"),
    }

    channel.shutdown();
}

#[tokio::test]
async fn recv_handles_two_frames_in_one_chunk() {
    let (transport, handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    let mut concat = build_inbound_tcp(0, &test_stc(100));
    concat.extend_from_slice(&build_inbound_tcp(1, &test_stc(101)));
    handle
        .inbound_sender
        .send(concat)
        .expect("inject concatenated frames");

    let ev1 = events.recv().await.expect("ev1");
    let ev2 = events.recv().await.expect("ev2");
    match (ev1, ev2) {
        (TcpChannelEvent::Inbound(a), TcpChannelEvent::Inbound(b)) => {
            assert_eq!(a.seqno, Some(100));
            assert_eq!(b.seqno, Some(101));
        }
        (a, b) => panic!("expected two Inbound events, got {a:?}, {b:?}"),
    }

    channel.shutdown();
}

#[tokio::test]
async fn recv_handles_frame_split_across_two_chunks() {
    let (transport, handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    let frame = build_inbound_tcp(0, &test_stc(7));
    let mid = frame.len() / 2;
    handle
        .inbound_sender
        .send(frame[..mid].to_vec())
        .expect("first half");
    handle
        .inbound_sender
        .send(frame[mid..].to_vec())
        .expect("second half");

    let ev = events.recv().await.expect("event after both halves");
    match ev {
        TcpChannelEvent::Inbound(stc) => assert_eq!(stc.seqno, Some(7)),
        other => panic!("expected Inbound, got {other:?}"),
    }

    channel.shutdown();
}

#[tokio::test]
async fn recv_handles_size_prefix_split_between_chunks() {
    // Edge case: 1 byte of the BE u16 size in chunk #1, the other
    // byte + entire payload in chunk #2. `next_tcp_frame` returns
    // `Ok(None)` on a 1-byte input — the channel must treat that as
    // "need more bytes" and not panic or surface a recv error.
    let (transport, handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    let frame = build_inbound_tcp(0, &test_stc(99));
    handle
        .inbound_sender
        .send(frame[..1].to_vec())
        .expect("size byte 0");
    handle
        .inbound_sender
        .send(frame[1..].to_vec())
        .expect("rest of frame");

    let ev = events.recv().await.expect("event");
    match ev {
        TcpChannelEvent::Inbound(stc) => assert_eq!(stc.seqno, Some(99)),
        other => panic!("expected Inbound, got {other:?}"),
    }

    channel.shutdown();
}

#[tokio::test]
async fn recv_emits_recv_error_on_decryption_failure() {
    let (transport, handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    let mut tampered = build_inbound_tcp(0, &test_stc(1));
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    handle
        .inbound_sender
        .send(tampered)
        .expect("inject tampered");

    let ev = events.recv().await.expect("recv error event");
    assert!(
        matches!(ev, TcpChannelEvent::RecvError(_)),
        "expected RecvError, got {ev:?}",
    );

    channel.shutdown();
}

#[tokio::test]
async fn recv_emits_recv_error_on_bad_relay_id() {
    let (transport, handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    // Build a frame whose header carries a different relay_id. The
    // recv loop must reject it as RecvError, NOT shut down.
    let stc = test_stc(1);
    let proto_bytes = stc.encode_to_vec();
    let header = Header {
        flags: HeaderFlags::RELAY_ID | HeaderFlags::SEQNO,
        relay_id: Some(0x1111_2222),
        conn_id: None,
        seqno: Some(0),
    };
    let header_bytes = header.encode();
    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::TcpServer,
        conn_id: TEST_CONN_ID,
        seqno: 0,
    };
    let cipher = encrypt(&TEST_AES_KEY, &iv.to_bytes(), &header_bytes, &proto_bytes);
    let wire = frame_tcp(&header_bytes, &cipher);
    handle.inbound_sender.send(wire).expect("inject bad-relay");

    let ev = events.recv().await.expect("recv error event");
    assert!(
        matches!(ev, TcpChannelEvent::RecvError(_)),
        "expected RecvError, got {ev:?}",
    );

    channel.shutdown();
}

// --- 11-13. lifecycle ----------------------------------------------

#[tokio::test]
async fn establish_emits_established_event() {
    let (transport, _handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");

    let first = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("event arrives within budget")
        .expect("event");
    assert!(matches!(first, TcpChannelEvent::Established), "got {first:?}");

    channel.shutdown();
}

#[tokio::test]
async fn watchdog_fires_after_silence() {
    let (transport, _handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    // No inbound traffic. After watchdog_timeout (200 ms in test
    // config), channel must emit Timeout (not shut down — supervisor
    // decides).
    let next = tokio::time::timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("event within budget")
        .expect("event");
    assert!(matches!(next, TcpChannelEvent::Timeout), "got {next:?}");

    channel.shutdown();
}

#[tokio::test]
async fn recv_loop_io_error_emits_recv_error_then_shutdown() {
    let (transport, handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    // Drop the inbound sender → mpsc closed → mock `read_chunk`
    // returns Err. Channel emits RecvError then exits with Shutdown.
    drop(handle);

    let mut saw_recv_error = false;
    let mut saw_shutdown = false;
    for _ in 0..4 {
        match tokio::time::timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Ok(TcpChannelEvent::RecvError(_))) => saw_recv_error = true,
            Ok(Ok(TcpChannelEvent::Shutdown)) => {
                saw_shutdown = true;
                break;
            }
            Ok(Ok(TcpChannelEvent::Timeout)) => continue,
            Ok(Ok(other)) => panic!("unexpected event {other:?}"),
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }
    assert!(saw_recv_error, "expected a RecvError event");
    assert!(saw_shutdown, "expected a Shutdown event after RecvError");

    channel.shutdown();
}

#[tokio::test]
async fn shutdown_stops_recv_loop_and_emits_shutdown_event() {
    let (transport, _handle) = MockTcpTransport::new();
    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), test_config())
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    channel.shutdown();
    let next = tokio::time::timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("shutdown event within budget")
        .expect("event");
    assert!(matches!(next, TcpChannelEvent::Shutdown), "got {next:?}");
}

// --- compile-time wiring sanity ------------------------------------

#[test]
fn tcp_channel_event_is_clone_for_broadcast() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<TcpChannelEvent>();
}

// --- capture tap (STEP 11.5) ---------------------------------------

#[tokio::test]
async fn tcp_channel_with_capture_records_inbound_packets() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path())
        .await
        .expect("capture writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, handle) = MockTcpTransport::new();
    let mut config = test_config();
    config.capture = Some(writer.clone());

    let (channel, mut events) = TcpChannel::establish(transport, &test_session(), config)
        .await
        .expect("establish");
    let _ = events.recv().await.expect("Established");

    // Push three frames.
    for ack in 1..=3u32 {
        let frame = build_inbound_tcp(ack - 1, &test_stc(ack as i32));
        handle.inbound_sender.send(frame).expect("inject");
    }

    // Drain the three Inbound events.
    for _ in 0..3 {
        let _ = tokio::time::timeout(Duration::from_millis(500), events.recv())
            .await
            .expect("event")
            .expect("event");
    }

    channel.shutdown();
    drop(channel);

    let writer = std::sync::Arc::try_unwrap(writer)
        .expect("only the test owns the writer once the channel drops it");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let inbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Inbound && r.transport == TransportKind::Tcp)
        .collect();
    assert!(
        inbound.len() >= 3,
        "expected at least 3 inbound TCP captures, got {}",
        inbound.len(),
    );
}

#[tokio::test]
async fn tcp_channel_with_capture_records_outbound_packets_with_hello_flag() {
    use zwift_relay::capture::{CaptureReader, CaptureWriter, Direction, TransportKind};

    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path())
        .await
        .expect("capture writer");
    let writer = std::sync::Arc::new(writer);

    let (transport, mut handle) = MockTcpTransport::new();
    let mut config = test_config();
    config.capture = Some(writer.clone());

    let (channel, _events) = TcpChannel::establish(transport, &test_session(), config)
        .await
        .expect("establish");

    // Send one hello + one steady packet.
    channel
        .send_packet(test_payload(0), /* hello */ true)
        .await
        .expect("send hello");
    channel
        .send_packet(test_payload(1), /* hello */ false)
        .await
        .expect("send steady");

    // Drain outbound from mock so the channel doesn't block (it
    // doesn't, but for cleanliness).
    let _ = handle.outbound_receiver.recv().await;
    let _ = handle.outbound_receiver.recv().await;

    channel.shutdown();
    drop(channel);

    let writer = std::sync::Arc::try_unwrap(writer).expect("only test owner");
    writer.flush_and_close().await.expect("flush");

    let reader = CaptureReader::open(path.path()).expect("reader");
    let outbound: Vec<_> = reader
        .filter_map(|r| r.ok())
        .filter(|r| r.direction == Direction::Outbound && r.transport == TransportKind::Tcp)
        .collect();
    assert_eq!(outbound.len(), 2, "expected exactly 2 outbound TCP captures");

    // First was hello=true; the captured payload is proto-only (no
    // `[2, 0]` envelope) and the `hello` flag round-trips.
    assert!(outbound[0].hello, "first capture is the hello packet");
    assert!(!outbound[1].hello, "second capture is steady-state");

    // Both payloads decode as ClientToServer (proves no envelope
    // bytes were captured).
    let cts0 = ClientToServer::decode(outbound[0].payload.as_slice()).expect("CTS 0");
    let cts1 = ClientToServer::decode(outbound[1].payload.as_slice()).expect("CTS 1");
    assert_eq!(cts0.player_id, TEST_ATHLETE_ID);
    assert_eq!(cts1.player_id, TEST_ATHLETE_ID);
}
