//! STEP-12.2 — `ranchero follow` command output tests (red state).
//!
//! These tests exercise the public `print_follow_to` function
//! that the CLI dispatcher delegates to. The function is
//! currently `unimplemented!()`, so every test here panics at
//! runtime — the red state for STEP-12.2's user-facing
//! "view logged data" capability.
//!
//! The intended behaviour is documented in
//! `docs/plans/STEP-12.2-follow-command.md`:
//!
//! - Default mode prints a header line ("Format version: 1") and
//!   one summary line per record, in the same shape as
//!   `replay --verbose`.
//! - `decode = true` prints, in addition to the summary, a
//!   `Debug` representation of the decoded `ServerToClient`
//!   (inbound) or `ClientToServer` (outbound) message.
//! - `idle_timeout_secs` causes the function to return cleanly
//!   after the configured window without a new record.
//! - Malformed capture files produce an error result.

use std::path::Path;
use std::time::{Duration, Instant};

use prost::Message as _;
use tempfile::NamedTempFile;

use ranchero::cli::print_follow_to;

use zwift_relay::capture::{
    CaptureRecord, CaptureWriter, Direction, TransportKind,
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
    decode: bool,
    idle_timeout_secs: Option<u64>,
) -> (Result<(), Box<dyn std::error::Error + Send + Sync>>, Vec<u8>) {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut out: Vec<u8> = Vec::new();
        let result = print_follow_to(&mut out, &path, decode, idle_timeout_secs)
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
    let (result, out) = run_follow(path.path(), false, Some(1)).await;
    result.expect("follow must return Ok on idle timeout");
    let text = String::from_utf8(out).expect("utf-8 output");
    assert!(
        text.contains("Format version: 2"),
        "follow must print the format-version header before iterating records; got:\n{text}",
    );
}

#[tokio::test]
async fn follow_default_mode_prints_one_summary_line_per_record() {
    let records = vec![
        record_with(Direction::Inbound, TransportKind::Udp, vec![1, 2, 3]),
        record_with(Direction::Inbound, TransportKind::Tcp, vec![4, 5, 6, 7]),
        record_with(Direction::Outbound, TransportKind::Udp, vec![8, 9]),
    ];
    let path = write_capture(records).await;
    let (result, out) = run_follow(path.path(), false, Some(1)).await;
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
async fn follow_decode_mode_renders_servertoclient_for_inbound() {
    let stc = zwift_proto::ServerToClient {
        seqno: Some(42),
        world_time: Some(123_456_789),
        ..Default::default()
    };
    let payload = stc.encode_to_vec();
    let records = vec![record_with(Direction::Inbound, TransportKind::Tcp, payload)];

    let path = write_capture(records).await;
    let (result, out) = run_follow(path.path(), true, Some(1)).await;
    result.expect("follow --decode must return Ok on idle timeout");

    let text = String::from_utf8(out).expect("utf-8 output");
    assert!(
        text.contains("ServerToClient"),
        "decode mode must render the message type for inbound records; got:\n{text}",
    );
    assert!(
        text.contains("seqno") || text.contains("42"),
        "decode mode must render a recognisable decoded field; got:\n{text}",
    );
}

#[tokio::test]
async fn follow_decode_mode_renders_clienttoserver_for_outbound() {
    let cts = zwift_proto::ClientToServer {
        seqno: Some(7),
        ..Default::default()
    };
    let payload = cts.encode_to_vec();
    let records = vec![record_with(Direction::Outbound, TransportKind::Udp, payload)];

    let path = write_capture(records).await;
    let (result, out) = run_follow(path.path(), true, Some(1)).await;
    result.expect("follow --decode must return Ok on idle timeout");

    let text = String::from_utf8(out).expect("utf-8 output");
    assert!(
        text.contains("ClientToServer"),
        "decode mode must render the message type for outbound records; got:\n{text}",
    );
}

#[tokio::test]
async fn follow_returns_within_idle_timeout_when_no_records_arrive() {
    let path = write_capture(Vec::new()).await;
    let timeout = Duration::from_millis(800);

    let start = Instant::now();
    let (result, _out) = run_follow(path.path(), false, Some(1)).await;
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

    let (result, _out) = run_follow(path.path(), false, Some(1)).await;
    assert!(
        result.is_err(),
        "follow must return an error for a malformed capture file",
    );
}
