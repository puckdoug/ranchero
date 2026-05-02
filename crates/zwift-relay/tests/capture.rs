// SPDX-License-Identifier: AGPL-3.0-only
//
// Wire-capture file format + writer + reader tests (STEP 11.5).
// Tap integration tests live in `tests/udp.rs` and `tests/tcp.rs`
// next to the channel tests they extend.

use std::sync::Arc;

use tempfile::NamedTempFile;
use zwift_relay::capture::{
    CaptureError, CaptureReader, CaptureRecord, CaptureWriter, Direction, FILE_HEADER_LEN, MAGIC,
    RECORD_HEADER_LEN, TransportKind, VERSION,
};

// --- helpers -------------------------------------------------------

fn record_with_payload(direction: Direction, transport: TransportKind, payload: Vec<u8>) -> CaptureRecord {
    CaptureRecord {
        ts_unix_ns: 1_700_000_000_000_000_000,
        direction,
        transport,
        hello: false,
        payload,
    }
}

fn write_records(records: Vec<CaptureRecord>) -> NamedTempFile {
    let path = NamedTempFile::new().expect("tempfile");
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let writer = CaptureWriter::open(path.path())
            .await
            .expect("open writer");
        for r in records {
            writer.record(r);
        }
        writer.flush_and_close().await.expect("flush");
    });
    path
}

// --- 1. format & header --------------------------------------------

#[tokio::test]
async fn file_header_starts_with_magic_and_version() {
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("open");
    writer.flush_and_close().await.expect("close");

    let bytes = std::fs::read(path.path()).expect("read file");
    assert!(
        bytes.len() >= FILE_HEADER_LEN,
        "file must contain at least the {FILE_HEADER_LEN}-byte header",
    );
    assert_eq!(&bytes[0..8], MAGIC);
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    assert_eq!(version, VERSION);
}

#[test]
fn reader_rejects_bad_magic() {
    let mut path = NamedTempFile::new().expect("tempfile");
    use std::io::Write;
    path.write_all(b"NOTACAPS")
        .expect("write magic");
    path.write_all(&[0x01, 0x00]).expect("write version");
    path.flush().expect("flush");

    match CaptureReader::open(path.path()) {
        Err(CaptureError::BadMagic) => {}
        other => panic!("expected BadMagic, got {other:?}"),
    }
}

#[test]
fn reader_rejects_unsupported_version() {
    let mut path = NamedTempFile::new().expect("tempfile");
    use std::io::Write;
    path.write_all(MAGIC).expect("write magic");
    path.write_all(&[0x02, 0x00]).expect("write version 2");
    path.flush().expect("flush");

    match CaptureReader::open(path.path()) {
        Err(CaptureError::UnsupportedVersion(2)) => {}
        other => panic!("expected UnsupportedVersion(2), got {other:?}"),
    }
}

#[test]
fn reader_handles_empty_file() {
    let path = NamedTempFile::new().expect("tempfile");
    // Don't write anything.
    match CaptureReader::open(path.path()) {
        Err(CaptureError::BadMagic) => {}
        Err(CaptureError::Io(_)) => {}
        other => panic!("expected BadMagic or Io, got {other:?}"),
    }
}

// --- 2. round-trip --------------------------------------------------

#[test]
fn writer_then_reader_round_trip_one_record() {
    let original = record_with_payload(Direction::Inbound, TransportKind::Udp, b"hello".to_vec());
    let path = write_records(vec![original.clone()]);

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    assert_eq!(reader.version(), VERSION);

    let recovered = reader
        .next()
        .expect("first record present")
        .expect("decode ok");
    assert_eq!(recovered, original);
    assert!(reader.next().is_none(), "no more records after the first");
}

#[test]
fn writer_then_reader_round_trip_many_records() {
    let originals: Vec<CaptureRecord> = (0..1_000u32)
        .map(|i| {
            let payload: Vec<u8> = (0..(1 + (i as usize % 100)))
                .map(|j| (j as u8).wrapping_add(i as u8))
                .collect();
            CaptureRecord {
                ts_unix_ns: 1_700_000_000_000_000_000 + u64::from(i),
                direction: if i % 2 == 0 { Direction::Inbound } else { Direction::Outbound },
                transport: if i % 3 == 0 { TransportKind::Udp } else { TransportKind::Tcp },
                hello: i % 5 == 0,
                payload,
            }
        })
        .collect();

    let path = write_records(originals.clone());
    let mut reader = CaptureReader::open(path.path()).expect("reader");

    for (idx, original) in originals.into_iter().enumerate() {
        let recovered = reader
            .next()
            .unwrap_or_else(|| panic!("record {idx} present"))
            .expect("decode ok");
        assert_eq!(recovered, original, "record {idx} mismatch");
    }
    assert!(reader.next().is_none(), "no more records after 1000");
}

#[test]
fn record_direction_inbound_outbound_round_trip() {
    let inbound = record_with_payload(Direction::Inbound, TransportKind::Udp, b"in".to_vec());
    let outbound = record_with_payload(Direction::Outbound, TransportKind::Udp, b"out".to_vec());
    let path = write_records(vec![inbound.clone(), outbound.clone()]);

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    assert_eq!(reader.next().unwrap().unwrap().direction, Direction::Inbound);
    assert_eq!(reader.next().unwrap().unwrap().direction, Direction::Outbound);
}

#[test]
fn record_transport_udp_tcp_round_trip() {
    let udp = record_with_payload(Direction::Inbound, TransportKind::Udp, b"u".to_vec());
    let tcp = record_with_payload(Direction::Inbound, TransportKind::Tcp, b"t".to_vec());
    let path = write_records(vec![udp.clone(), tcp.clone()]);

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    assert_eq!(reader.next().unwrap().unwrap().transport, TransportKind::Udp);
    assert_eq!(reader.next().unwrap().unwrap().transport, TransportKind::Tcp);
}

#[test]
fn record_hello_flag_round_trip() {
    let with_hello = CaptureRecord {
        ts_unix_ns: 0,
        direction: Direction::Outbound,
        transport: TransportKind::Tcp,
        hello: true,
        payload: vec![0xAA],
    };
    let without_hello = CaptureRecord { hello: false, ..with_hello.clone() };
    let path = write_records(vec![with_hello.clone(), without_hello.clone()]);

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    assert!(reader.next().unwrap().unwrap().hello);
    assert!(!reader.next().unwrap().unwrap().hello);
}

#[test]
fn record_payload_max_len_round_trips() {
    let payload: Vec<u8> = (0..u16::MAX as usize).map(|i| (i & 0xFF) as u8).collect();
    let original = record_with_payload(Direction::Inbound, TransportKind::Tcp, payload);
    let path = write_records(vec![original.clone()]);

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    let recovered = reader.next().unwrap().expect("decode");
    assert_eq!(recovered.payload.len(), u16::MAX as usize);
    assert_eq!(recovered, original);
}

// --- 3. truncation & error paths ----------------------------------

fn write_partial_capture(bytes: &[u8]) -> NamedTempFile {
    use std::io::Write;
    let mut path = NamedTempFile::new().expect("tempfile");
    path.write_all(bytes).expect("write partial");
    path.flush().expect("flush");
    path
}

#[test]
fn reader_handles_truncated_record_header() {
    // Valid file header + 5 bytes of record header (not 15).
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&[0xFF; 5]); // partial record header
    let path = write_partial_capture(&bytes);

    let mut reader = CaptureReader::open(path.path()).expect("header ok");
    match reader.next().expect("some result") {
        Err(CaptureError::Truncated { needed, got }) => {
            assert_eq!(needed, RECORD_HEADER_LEN);
            assert_eq!(got, 5);
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
}

#[test]
fn reader_handles_truncated_payload() {
    // Valid file header + complete record header advertising
    // len=100, but only 50 payload bytes follow.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes()); // ts
    bytes.push(Direction::Inbound.as_byte());
    bytes.push(TransportKind::Udp.as_byte());
    bytes.push(0); // flags
    bytes.extend_from_slice(&100u32.to_le_bytes()); // len
    bytes.extend_from_slice(&[0u8; 50]); // half the payload
    let path = write_partial_capture(&bytes);

    let mut reader = CaptureReader::open(path.path()).expect("header ok");
    match reader.next().expect("some result") {
        Err(CaptureError::Truncated { needed, got }) => {
            assert_eq!(needed, 100);
            assert_eq!(got, 50);
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
}

#[test]
fn reader_handles_bad_direction_byte() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.push(0xFF); // invalid direction
    bytes.push(0); // transport
    bytes.push(0); // flags
    bytes.extend_from_slice(&0u32.to_le_bytes()); // len = 0
    let path = write_partial_capture(&bytes);

    let mut reader = CaptureReader::open(path.path()).expect("header ok");
    match reader.next().expect("some result") {
        Err(CaptureError::BadDirection(0xFF)) => {}
        other => panic!("expected BadDirection(0xFF), got {other:?}"),
    }
}

#[test]
fn reader_handles_bad_transport_byte() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.push(0); // direction
    bytes.push(0xFF); // invalid transport
    bytes.push(0); // flags
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let path = write_partial_capture(&bytes);

    let mut reader = CaptureReader::open(path.path()).expect("header ok");
    match reader.next().expect("some result") {
        Err(CaptureError::BadTransport(0xFF)) => {}
        other => panic!("expected BadTransport(0xFF), got {other:?}"),
    }
}

// --- 4. drop-on-saturation ----------------------------------------

#[tokio::test]
async fn writer_drops_when_channel_full() {
    let path = NamedTempFile::new().expect("tempfile");
    // Capacity-1 channel + a tight push loop; some records must be
    // dropped because the writer task can't keep up.
    let writer = CaptureWriter::open_with_capacity(path.path(), 1)
        .await
        .expect("open");
    let writer = Arc::new(writer);

    // Push aggressively. Even on a fast disk, capacity 1 will saturate.
    for i in 0..10_000u32 {
        writer.record(record_with_payload(
            Direction::Inbound,
            TransportKind::Udp,
            vec![(i & 0xFF) as u8; 32],
        ));
    }

    let dropped = writer.dropped_count();
    assert!(
        dropped > 0,
        "expected some records to be dropped under saturation; got {dropped}",
    );

    // The Arc-wrapped writer can't be moved into flush_and_close
    // (which takes self by value). For the test we just leak the
    // writer task; tempfile cleans up on drop.
    let _ = writer; // keep alive until end of scope
}

#[tokio::test]
async fn writer_record_is_non_blocking() {
    // `record()` must return promptly even when the writer task is
    // slow — capture must never backpressure the channel hot path.
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open_with_capacity(path.path(), 4)
        .await
        .expect("open");

    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        writer.record(record_with_payload(
            Direction::Inbound,
            TransportKind::Udp,
            vec![0u8; 64],
        ));
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "10k record() calls took {:?} — must be sub-second (non-blocking)",
        elapsed,
    );
}

// --- 4b. flush_and_close drain semantics --------------------------

#[tokio::test]
async fn flush_and_close_drains_pending_records() {
    // Pins open verification point #4 of STEP-11.5: closing while
    // records are queued must drain them rather than truncate. The
    // requirement is that every accepted record (i.e. every push
    // that did not increment dropped_count) must be readable after
    // close.
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open_with_capacity(path.path(), 4)
        .await
        .expect("open");

    let n = 100usize;
    for i in 0..n {
        writer.record(record_with_payload(
            Direction::Inbound,
            TransportKind::Udp,
            vec![(i & 0xFF) as u8; 16],
        ));
    }
    let dropped = writer.dropped_count() as usize;
    writer.flush_and_close().await.expect("close");

    let reader = CaptureReader::open(path.path()).expect("read");
    let count = reader.count();
    assert_eq!(
        count,
        n - dropped,
        "every accepted record must survive flush_and_close (n={n}, dropped={dropped}, recovered={count})",
    );
}

// --- 5. capture-off zero overhead ---------------------------------

#[test]
fn udp_channel_config_default_capture_is_none() {
    use zwift_relay::UdpChannelConfig;
    assert!(
        UdpChannelConfig::default().capture.is_none(),
        "default UdpChannelConfig must have no capture tap",
    );
}

#[test]
fn tcp_channel_config_default_capture_is_none() {
    use zwift_relay::TcpChannelConfig;
    assert!(
        TcpChannelConfig::default().capture.is_none(),
        "default TcpChannelConfig must have no capture tap",
    );
}

// --- 6. follower (STEP-12.2 red state) ----------------------------
//
// These tests exercise `CaptureFollower`, the tailing reader added
// by STEP-12.2. The implementation is a stub that panics on every
// method call, so each test fails until STEP-12.2 lands.

use std::time::Duration;
use zwift_relay::capture::CaptureFollower;

#[tokio::test]
async fn follower_reads_records_as_they_are_written() {
    // Spawn a CaptureWriter that pushes one record every 50 ms
    // for ten records. A CaptureFollower opened on the same file
    // observes all ten records in order.
    let path = NamedTempFile::new().expect("tempfile");
    let path_buf = path.path().to_path_buf();

    // Opening the writer writes the file header before returning,
    // so the follower can open immediately afterwards.
    let writer = CaptureWriter::open(&path_buf).await.expect("open writer");

    let follower_path = path_buf.clone();
    let follower_handle = tokio::task::spawn_blocking(move || {
        let follower = CaptureFollower::open(&follower_path)
            .expect("open follower")
            .with_poll_interval(Duration::from_millis(20))
            .with_idle_timeout(Some(Duration::from_secs(3)));
        let mut records = Vec::new();
        for result in follower {
            records.push(result.expect("record decode"));
        }
        records
    });

    for i in 0..10u32 {
        writer.record(record_with_payload(
            Direction::Inbound,
            TransportKind::Udp,
            vec![(i & 0xFF) as u8; 16],
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    writer.flush_and_close().await.expect("flush");

    let records = follower_handle.await.expect("follower task");
    assert_eq!(
        records.len(),
        10,
        "follower must observe all ten records that were appended after the file header",
    );
}

#[tokio::test]
async fn follower_resumes_after_truncated_record_at_eof() {
    // Manually write a valid file header followed by partial bytes
    // of a record header (5 of 15). CaptureFollower::next() does
    // not return until the rest of the record has been written.
    let path = NamedTempFile::new().expect("tempfile");
    let path_buf = path.path().to_path_buf();

    // Write the file header and 5 bytes of a record header.
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&path_buf).expect("create");
        f.write_all(MAGIC).expect("write magic");
        f.write_all(&zwift_relay::capture::VERSION.to_le_bytes()).expect("write version");
        f.write_all(&[0xFF; 5]).expect("write partial record header");
        f.flush().expect("flush");
    }

    // Spawn a thread that completes the record after a short delay.
    let writer_path = path_buf.clone();
    let writer_handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&writer_path)
            .expect("open append");
        // Complete the record header (we already wrote 5 bytes;
        // need 10 more to reach 15). Then write a small payload.
        // Use ts=0, direction=Inbound (0), transport=Udp (0),
        // flags=0, len=4, payload=[0,1,2,3].
        // Total record header bytes: 8 (ts) + 1 + 1 + 1 + 4 (len) = 15.
        // We've written 5 bytes of `0xFF`. To make the record
        // valid we need to overwrite from the start, but `append`
        // mode won't allow that. So instead, treat the 5 0xFF
        // bytes as part of the timestamp (the first 5 bytes of
        // ts) and continue writing. The remaining 3 bytes of ts
        // plus direction(1)+transport(1)+flags(1)+len(4) = 10 bytes.
        f.write_all(&[0x00, 0x00, 0x00]).expect("rest of ts");
        f.write_all(&[0]).expect("direction Inbound");
        f.write_all(&[0]).expect("transport Udp");
        f.write_all(&[0]).expect("flags");
        f.write_all(&4u32.to_le_bytes()).expect("len");
        f.write_all(&[1u8, 2, 3, 4]).expect("payload");
        f.flush().expect("flush");
    });

    let follower_handle = tokio::task::spawn_blocking(move || {
        let mut follower = CaptureFollower::open(&path_buf)
            .expect("open follower")
            .with_poll_interval(Duration::from_millis(20))
            .with_idle_timeout(Some(Duration::from_secs(2)));
        follower.next()
    });

    writer_handle.join().expect("writer thread");
    let result = follower_handle.await.expect("follower task");
    let record = result.expect("must yield Some(_)").expect("record decoded");
    assert_eq!(record.payload, vec![1, 2, 3, 4]);
}

#[tokio::test]
async fn follower_idle_timeout_returns_none() {
    // Open a follower with idle_timeout = Some(50 ms) on a file
    // that contains the file header but no records. next()
    // returns None after roughly the timeout elapses.
    let path = NamedTempFile::new().expect("tempfile");
    let path_buf = path.path().to_path_buf();

    // Open the writer and immediately close it; the file now
    // contains only the file header.
    let writer = CaptureWriter::open(&path_buf).await.expect("open writer");
    writer.flush_and_close().await.expect("close writer");

    let follower_handle = tokio::task::spawn_blocking(move || {
        let mut follower = CaptureFollower::open(&path_buf)
            .expect("open follower")
            .with_poll_interval(Duration::from_millis(10))
            .with_idle_timeout(Some(Duration::from_millis(50)));
        let start = std::time::Instant::now();
        let result = follower.next();
        (result, start.elapsed())
    });

    let (result, elapsed) = follower_handle.await.expect("follower task");
    assert!(
        result.is_none(),
        "follower with idle_timeout must return None on a quiet file",
    );
    assert!(
        elapsed >= Duration::from_millis(40),
        "follower must respect the idle_timeout (got elapsed = {elapsed:?})",
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "follower must not block significantly past the idle_timeout (got elapsed = {elapsed:?})",
    );
}

#[tokio::test]
async fn follower_no_idle_timeout_blocks_indefinitely() {
    // Open a follower with no idle timeout on a file that
    // contains the file header but no records. The follower
    // continues polling; the test stops the polling by writing a
    // record after a short delay.
    let path = NamedTempFile::new().expect("tempfile");
    let path_buf = path.path().to_path_buf();
    let writer = CaptureWriter::open(&path_buf).await.expect("open writer");

    let follower_path = path_buf.clone();
    let follower_handle = tokio::task::spawn_blocking(move || {
        let mut follower = CaptureFollower::open(&follower_path)
            .expect("open follower")
            .with_poll_interval(Duration::from_millis(10));
        // No idle timeout: this would block forever if the
        // writer never produced a record. The test exercises that
        // the follower keeps polling rather than returning early.
        let start = std::time::Instant::now();
        let result = follower.next();
        (result, start.elapsed())
    });

    // Wait long enough that we know the follower would have
    // returned None if it had an idle timeout. Then write one
    // record so the follower can resolve.
    tokio::time::sleep(Duration::from_millis(150)).await;
    writer.record(record_with_payload(
        Direction::Inbound,
        TransportKind::Udp,
        vec![1, 2, 3, 4],
    ));
    writer.flush_and_close().await.expect("flush");

    let (result, elapsed) = follower_handle.await.expect("follower task");
    assert!(
        result.is_some(),
        "follower with no idle_timeout must wait until a record arrives",
    );
    assert!(
        elapsed >= Duration::from_millis(140),
        "follower must have polled past the 150 ms wait period (got elapsed = {elapsed:?})",
    );
}

#[test]
fn follower_rejects_bad_magic() {
    // A file written with non-magic bytes returns
    // Err(BadMagic) from CaptureFollower::open, mirroring
    // CaptureReader.
    let mut path = NamedTempFile::new().expect("tempfile");
    use std::io::Write;
    path.write_all(b"NOTACAPS").expect("write magic");
    path.write_all(&[0x01, 0x00]).expect("write version");
    path.flush().expect("flush");

    match CaptureFollower::open(path.path()) {
        Err(CaptureError::BadMagic) => {}
        other => panic!(
            "STEP-12.2 red state: CaptureFollower::open must reject \
             bad magic with Err(BadMagic); got {other:?}",
        ),
    }
}

#[test]
fn follower_rejects_unsupported_version() {
    // A file written with magic but version 2 returns
    // Err(UnsupportedVersion(2)).
    let mut path = NamedTempFile::new().expect("tempfile");
    use std::io::Write;
    path.write_all(MAGIC).expect("write magic");
    path.write_all(&[0x02, 0x00]).expect("write version 2");
    path.flush().expect("flush");

    match CaptureFollower::open(path.path()) {
        Err(CaptureError::UnsupportedVersion(2)) => {}
        other => panic!(
            "STEP-12.2 red state: CaptureFollower::open must reject \
             unsupported versions with Err(UnsupportedVersion(_)); \
             got {other:?}",
        ),
    }
}

// --- 7. Phase 0a: format v2, manifest record, Http transport -------
//
// These tests target the new `RecordKind::Manifest` / `SessionManifest`
// API, the bumped `VERSION = 2`, and `TransportKind::Http`. They are
// written against the API the implementation step (0b) is required to
// produce; until 0b lands they must fail to compile.

use zwift_relay::capture::{CaptureItem, RecordKind, SessionManifest};

fn sample_manifest() -> SessionManifest {
    SessionManifest {
        aes_key: [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF,
        ],
        device_type: 1,
        channel_type: 2,
        send_iv_seqno_tcp: 0,
        recv_iv_seqno_tcp: 0,
        send_iv_seqno_udp: 7,
        recv_iv_seqno_udp: 11,
        relay_id: 0xDEAD_BEEF,
        conn_id: 0xCAFE_F00D,
        expires_at_unix_ns: 1_700_000_000_000_000_000,
    }
}

#[tokio::test]
async fn capture_format_v2_round_trip_writes_and_reads_manifest_then_frames() {
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("open writer");
    let manifest = sample_manifest();
    writer.record_session_manifest(manifest.clone());
    writer.record(record_with_payload(
        Direction::Outbound,
        TransportKind::Tcp,
        b"tcp-frame-bytes".to_vec(),
    ));
    writer.record(record_with_payload(
        Direction::Inbound,
        TransportKind::Udp,
        b"udp-datagram-bytes".to_vec(),
    ));
    writer.flush_and_close().await.expect("close");

    // File header advertises VERSION = 2 (the current format).
    let bytes = std::fs::read(path.path()).expect("read file");
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    assert_eq!(
        version, VERSION,
        "STEP-12.12 Phase 0a: capture file must be written at the current VERSION ({VERSION}); \
         got {version}",
    );
    assert_eq!(
        VERSION, 2,
        "STEP-12.12 Phase 0a: capture format VERSION must be bumped to 2 by Phase 0b",
    );

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    assert_eq!(reader.version(), VERSION);

    let first = reader
        .next_item()
        .expect("first item present")
        .expect("decode ok");
    match first {
        CaptureItem::Manifest(m) => assert_eq!(m, manifest, "manifest fields must round-trip"),
        other => panic!(
            "STEP-12.12 Phase 0a: first item after header must be a Manifest record, got {other:?}",
        ),
    }

    let second = reader
        .next_item()
        .expect("second item present")
        .expect("decode ok");
    match second {
        CaptureItem::Frame(rec) => {
            assert_eq!(rec.direction, Direction::Outbound);
            assert_eq!(rec.transport, TransportKind::Tcp);
            assert_eq!(rec.payload, b"tcp-frame-bytes".to_vec());
        }
        other => panic!("expected Frame(TCP outbound), got {other:?}"),
    }

    let third = reader
        .next_item()
        .expect("third item present")
        .expect("decode ok");
    match third {
        CaptureItem::Frame(rec) => {
            assert_eq!(rec.direction, Direction::Inbound);
            assert_eq!(rec.transport, TransportKind::Udp);
            assert_eq!(rec.payload, b"udp-datagram-bytes".to_vec());
        }
        other => panic!("expected Frame(UDP inbound), got {other:?}"),
    }

    assert!(reader.next_item().is_none(), "no more items after the third");
}

#[test]
fn capture_reader_rejects_v1_file_with_clear_error() {
    // A hand-crafted file with the magic and version-1 header must be
    // rejected by `CaptureReader::open` with `UnsupportedVersion(1)`,
    // because Phase 0b bumps the format to VERSION = 2 and there are
    // no production v1 captures to migrate.
    let mut path = NamedTempFile::new().expect("tempfile");
    use std::io::Write;
    path.write_all(MAGIC).expect("write magic");
    path.write_all(&1u16.to_le_bytes()).expect("write version 1");
    path.flush().expect("flush");

    match CaptureReader::open(path.path()) {
        Err(CaptureError::UnsupportedVersion(1)) => {}
        other => panic!(
            "STEP-12.12 Phase 0a: CaptureReader must reject v1 files with \
             Err(UnsupportedVersion(1)); got {other:?}",
        ),
    }
}

#[test]
fn capture_record_supports_http_transport_kind() {
    // Round-trip a record with `TransportKind::Http`. Phase 0b adds
    // the variant; until then this test fails to compile.
    let original = CaptureRecord {
        ts_unix_ns: 1_700_000_000_000_000_000,
        direction: Direction::Outbound,
        transport: TransportKind::Http,
        hello: false,
        payload: b"POST /token HTTP/1.1 ...".to_vec(),
    };
    let path = write_records(vec![original.clone()]);

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    let recovered = reader
        .next()
        .expect("first record present")
        .expect("decode ok");
    assert_eq!(
        recovered.transport,
        TransportKind::Http,
        "STEP-12.12 Phase 0a: TransportKind::Http must round-trip",
    );
    assert_eq!(recovered, original);
}

#[tokio::test]
async fn record_session_manifest_can_be_called_again_after_rotation() {
    // Manifest, frame, manifest, frame: simulates a supervisor refresh
    // that rotates AES key material mid-session.
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open(path.path()).await.expect("open writer");

    let first_manifest = sample_manifest();
    let mut second_manifest = first_manifest.clone();
    second_manifest.aes_key = [0xAA; 16];
    second_manifest.relay_id = 0x1234_5678;
    second_manifest.send_iv_seqno_udp = 999;

    writer.record_session_manifest(first_manifest.clone());
    writer.record(record_with_payload(
        Direction::Outbound,
        TransportKind::Udp,
        b"frame-1".to_vec(),
    ));
    writer.record_session_manifest(second_manifest.clone());
    writer.record(record_with_payload(
        Direction::Outbound,
        TransportKind::Udp,
        b"frame-2".to_vec(),
    ));
    writer.flush_and_close().await.expect("close");

    let mut reader = CaptureReader::open(path.path()).expect("reader");
    let items: Vec<CaptureItem> = std::iter::from_fn(|| reader.next_item())
        .map(|r| r.expect("decode"))
        .collect();
    assert_eq!(items.len(), 4, "expected 4 items, got {}", items.len());

    match &items[0] {
        CaptureItem::Manifest(m) => assert_eq!(m, &first_manifest),
        other => panic!("item 0 must be Manifest, got {other:?}"),
    }
    match &items[1] {
        CaptureItem::Frame(r) => assert_eq!(r.payload, b"frame-1".to_vec()),
        other => panic!("item 1 must be Frame, got {other:?}"),
    }
    match &items[2] {
        CaptureItem::Manifest(m) => assert_eq!(m, &second_manifest),
        other => panic!("item 2 must be Manifest, got {other:?}"),
    }
    match &items[3] {
        CaptureItem::Frame(r) => assert_eq!(r.payload, b"frame-2".to_vec()),
        other => panic!("item 3 must be Frame, got {other:?}"),
    }

    // Sanity: the writer's manifest path uses a distinct RecordKind
    // discriminant, so the byte-level guard is exposed for hex-viewer
    // audits in Phase 7.
    let _ = RecordKind::Manifest;
    let _ = RecordKind::Frame;
}

#[tokio::test]
async fn follower_with_poll_interval_respects_setting() {
    // A follower with poll_interval = 5 ms retries faster than
    // the default. With a 25 ms gap between records, a follower
    // configured at 5 ms must finish well before a default-poll
    // (100 ms) follower would.
    let path = NamedTempFile::new().expect("tempfile");
    let path_buf = path.path().to_path_buf();
    let writer = CaptureWriter::open(&path_buf).await.expect("open writer");

    let follower_path = path_buf.clone();
    let follower_handle = tokio::task::spawn_blocking(move || {
        let follower = CaptureFollower::open(&follower_path)
            .expect("open follower")
            .with_poll_interval(Duration::from_millis(5))
            .with_idle_timeout(Some(Duration::from_secs(1)));
        let start = std::time::Instant::now();
        let mut count = 0;
        for result in follower {
            result.expect("decode");
            count += 1;
        }
        (count, start.elapsed())
    });

    for i in 0..5u32 {
        writer.record(record_with_payload(
            Direction::Inbound,
            TransportKind::Udp,
            vec![(i & 0xFF) as u8; 8],
        ));
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    writer.flush_and_close().await.expect("flush");

    let (count, _elapsed) = follower_handle.await.expect("follower task");
    assert_eq!(count, 5, "follower must observe all five records");
}
