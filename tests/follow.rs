//! `ranchero follow` command output tests.
//!
//! These tests exercise the public `print_follow_to` function
//! that the CLI dispatcher delegates to.
//!
//! Behaviour under test:
//! - Prints a file-header line ("Format version: N") and one summary
//!   line per frame record.
//! - When a Manifest record precedes a frame, decrypts the frame using
//!   the manifest's AES key and seqno counters, then proto-decodes and
//!   prints the result.
//! - Exits cleanly when the idle timeout elapses without a new record.
//! - Returns an error for malformed capture files.

use std::path::Path;
use std::time::{Duration, Instant};

use prost::Message as _;
use tempfile::NamedTempFile;

use ranchero::cli::print_follow_to;

use zwift_relay::{
    ChannelType, DeviceType, Header, HeaderFlags, RelayIv, encrypt, frame_tcp, tcp_plaintext,
};
use zwift_relay::capture::{
    CaptureRecord, CaptureWriter, ContentType, Direction, SessionManifest, TransportKind,
};

// --- helpers ------------------------------------------------------

/// Open a capture writer at a fresh temp file, push the supplied
/// records, close the writer, and return the file handle.
async fn write_capture(records: Vec<CaptureRecord>) -> NamedTempFile {
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("open writer");
    for record in records {
        writer.record(record);
    }
    writer.flush_and_close().await.expect("close writer");
    path
}

fn record_with(
    direction: Direction,
    transport: TransportKind,
    payload: Vec<u8>,
) -> CaptureRecord {
    CaptureRecord {
        ts_unix_ns: 1_700_000_000_000_000_000,
        direction,
        transport,
        hello: false,
        content_type: ContentType::Unspecified,
        payload,
    }
}

/// Run `print_follow_to` on a separate blocking thread because
/// the function is synchronous and may sleep (the production
/// implementation polls the file with `std::thread::sleep`); a
/// `#[tokio::test]` runtime would block its worker if we called
/// it directly.
async fn run_follow(
    path: &Path,
    idle_timeout_secs: Option<u64>,
) -> (Result<(), Box<dyn std::error::Error + Send + Sync>>, Vec<u8>) {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut out: Vec<u8> = Vec::new();
        let result = print_follow_to(&mut out, &path, idle_timeout_secs)
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                Box::<dyn std::error::Error + Send + Sync>::from(e.to_string())
            });
        (result, out)
    })
    .await
    .expect("spawn_blocking")
}

// --- tests --------------------------------------------------------

#[tokio::test]
async fn follow_prints_format_version_header() {
    let path = write_capture(Vec::new()).await;
    let (result, out) = run_follow(path.path(), Some(1)).await;
    result.expect("follow must return Ok on idle timeout");
    let text = String::from_utf8(out).expect("utf-8 output");
    assert!(
        text.contains("Format version: 3"),
        "follow must print the format-version header before iterating records; got:\n{text}",
    );
}

#[tokio::test]
async fn follow_prints_one_summary_line_per_frame_record() {
    let records = vec![
        record_with(Direction::Inbound, TransportKind::Udp, vec![1, 2, 3]),
        record_with(Direction::Inbound, TransportKind::Tcp, vec![4, 5, 6, 7]),
        record_with(Direction::Outbound, TransportKind::Udp, vec![8, 9]),
    ];
    let path = write_capture(records).await;
    let (result, out) = run_follow(path.path(), Some(1)).await;
    result.expect("follow must return Ok on idle timeout");

    let text = String::from_utf8(out).expect("utf-8 output");
    let summary_lines: Vec<&str> = text.lines().filter(|l| l.contains("ts=")).collect();
    assert_eq!(
        summary_lines.len(),
        3,
        "follow must print one summary line per record; got:\n{text}",
    );

    assert!(
        text.contains("in  UDP") || text.contains("in UDP"),
        "first record must show inbound UDP direction; got:\n{text}",
    );
    assert!(
        text.contains("in  TCP") || text.contains("in TCP"),
        "second record must show inbound TCP direction; got:\n{text}",
    );
    assert!(
        text.contains("out UDP"),
        "third record must show outbound UDP direction; got:\n{text}",
    );
}

#[tokio::test]
async fn follow_returns_within_idle_timeout_when_no_records_arrive() {
    let path = write_capture(Vec::new()).await;
    let timeout = Duration::from_millis(800);

    let start = Instant::now();
    let (result, _out) = run_follow(path.path(), Some(1)).await;
    let elapsed = start.elapsed();

    result.expect("follow must return Ok on idle timeout");
    assert!(
        elapsed >= timeout,
        "follow must run for at least the configured idle window; elapsed = {elapsed:?}",
    );
    assert!(
        elapsed < Duration::from_millis(2_500),
        "follow must not block past the idle window by a wide margin; elapsed = {elapsed:?}",
    );
}

#[tokio::test]
async fn follow_returns_error_for_bad_magic() {
    let path = NamedTempFile::new().expect("tempfile");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(path.path()).expect("create");
        file.write_all(b"NOTACAPS").expect("write magic");
        file.write_all(&[0x01, 0x00]).expect("write version");
        file.flush().expect("flush");
    }

    let (result, _out) = run_follow(path.path(), Some(1)).await;
    assert!(
        result.is_err(),
        "follow must return an error for a malformed capture file",
    );
}

// --- STEP-12.30 Phase 1a: follow must decrypt encrypted frames --------

/// Write a manifest followed by frame records into a capture file.
async fn write_capture_with_manifest(
    manifest: SessionManifest,
    records: Vec<CaptureRecord>,
) -> NamedTempFile {
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("open writer");
    writer.record_session_manifest(manifest);
    for record in records {
        writer.record(record);
    }
    writer.flush_and_close().await.expect("close writer");
    path
}

/// Build an outbound TCP capture payload as written by `tcp.rs`:
/// `[BE u16 size][header_bytes][ciphertext||tag4]`.
fn make_outbound_tcp_frame(
    aes_key: &[u8; 16],
    conn_id: u16,
    seqno: u32,
    hello: bool,
    proto_bytes: &[u8],
) -> Vec<u8> {
    let plaintext = tcp_plaintext(proto_bytes, hello);
    let header = Header { flags: HeaderFlags::empty(), relay_id: None, conn_id: None, seqno: None };
    let header_bytes = header.encode();
    let iv = RelayIv { device: DeviceType::Relay, channel: ChannelType::TcpClient, conn_id, seqno };
    let ciphertext = encrypt(aes_key, &iv.to_bytes(), &header_bytes, &plaintext);
    frame_tcp(&header_bytes, &ciphertext)
}

/// Build an inbound TCP capture payload as written by `tcp.rs`:
/// `[header_bytes][ciphertext||tag4]` — no 2-byte length prefix.
/// The server's plaintext is raw proto bytes; it does not use the
/// `[TCP_VERSION][hello_byte]` envelope that the client adds to its
/// outbound frames.
fn make_inbound_tcp_frame(
    aes_key: &[u8; 16],
    conn_id: u16,
    seqno: u32,
    proto_bytes: &[u8],
) -> Vec<u8> {
    let header = Header { flags: HeaderFlags::empty(), relay_id: None, conn_id: None, seqno: None };
    let header_bytes = header.encode();
    let iv = RelayIv { device: DeviceType::Relay, channel: ChannelType::TcpServer, conn_id, seqno };
    let ciphertext = encrypt(aes_key, &iv.to_bytes(), &header_bytes, proto_bytes);
    let mut frame = header_bytes;
    frame.extend_from_slice(&ciphertext);
    frame
}

/// Build a `SessionManifest` suitable for a single-session test.
fn test_manifest(aes_key: [u8; 16], conn_id: u32) -> SessionManifest {
    SessionManifest {
        aes_key,
        device_type: DeviceType::Relay as u16 as u8,
        channel_type: ChannelType::TcpClient as u16 as u8,
        send_iv_seqno_tcp: 0,
        recv_iv_seqno_tcp: 0,
        send_iv_seqno_udp: 0,
        recv_iv_seqno_udp: 0,
        relay_id: 42,
        conn_id,
        expires_at_unix_ns: 9_999_999_999_999_999_999,
    }
}

#[tokio::test]
async fn follow_decrypts_outbound_tcp_frame() {
    let aes_key = [1u8; 16];
    let conn_id: u16 = 99;

    let cts = zwift_proto::ClientToServer { seqno: Some(7), ..Default::default() };
    let proto_bytes = cts.encode_to_vec();
    let payload = make_outbound_tcp_frame(&aes_key, conn_id, 0, false, &proto_bytes);

    let record = CaptureRecord {
        ts_unix_ns: 1_700_000_000_000_000_000,
        direction: Direction::Outbound,
        transport: TransportKind::Tcp,
        hello: false,
        content_type: ContentType::Unspecified,
        payload,
    };

    let path = write_capture_with_manifest(
        test_manifest(aes_key, conn_id as u32),
        vec![record],
    ).await;

    let (result, out) = run_follow(path.path(), Some(1)).await;
    result.expect("follow must return Ok on idle timeout");
    let text = String::from_utf8(out).expect("utf-8 output");

    assert!(
        text.contains("ClientToServer"),
        "STEP-12.30: follow must decrypt the manifest key and decode outbound TCP frames as ClientToServer; got:\n{text}",
    );
}

#[tokio::test]
async fn follow_decrypts_inbound_tcp_frame() {
    let aes_key = [2u8; 16];
    let conn_id: u16 = 77;

    let stc = zwift_proto::ServerToClient { seqno: Some(42), world_time: Some(1_000_000), ..Default::default() };
    let proto_bytes = stc.encode_to_vec();
    let payload = make_inbound_tcp_frame(&aes_key, conn_id, 0, &proto_bytes);

    let record = CaptureRecord {
        ts_unix_ns: 1_700_000_000_000_000_000,
        direction: Direction::Inbound,
        transport: TransportKind::Tcp,
        hello: false,
        content_type: ContentType::Unspecified,
        payload,
    };

    let path = write_capture_with_manifest(
        test_manifest(aes_key, conn_id as u32),
        vec![record],
    ).await;

    let (result, out) = run_follow(path.path(), Some(1)).await;
    result.expect("follow must return Ok on idle timeout");
    let text = String::from_utf8(out).expect("utf-8 output");

    assert!(
        text.contains("ServerToClient"),
        "STEP-12.30: follow must decrypt the manifest key and decode inbound TCP frames as ServerToClient; got:\n{text}",
    );
}

#[test]
fn follow_subcommand_has_no_decode_flag() {
    use ranchero::cli::parse_from;
    let result = parse_from(["ranchero", "follow", "--decode", "foo.cap"]);
    assert!(
        result.is_err(),
        "STEP-12.30: `--decode` flag must be removed from `ranchero follow`; \
        the parse succeeded but should have failed with an unknown-argument error",
    );
}
