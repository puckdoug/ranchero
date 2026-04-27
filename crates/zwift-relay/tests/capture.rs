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
