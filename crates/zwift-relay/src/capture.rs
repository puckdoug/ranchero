// SPDX-License-Identifier: AGPL-3.0-only
//
// Wire-capture file format + writer + reader. Plaintext-only
// (post-decrypt for inbound, proto-bytes-only for outbound) so
// captures don't leak the AES session key. See
// `docs/plans/STEP-11.5-wire-capture.md` for the full design.
//
// This file currently exposes the public surface as stubs so
// `tests/capture.rs` (and the channel-tap tests) compile.
// Behavior lands in green state.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

// --- format constants ---------------------------------------------

/// 8-byte file header magic. ASCII `"RNCWCAP"` + NUL terminator.
pub const MAGIC: &[u8; 8] = b"RNCWCAP\0";

/// Current wire-capture format version.
pub const VERSION: u16 = 1;

/// File header byte length (`MAGIC` + version u16 LE).
pub const FILE_HEADER_LEN: usize = 10;

/// Per-record fixed-overhead byte length:
/// `ts_unix_ns(8) + direction(1) + transport(1) + flags(1) + len(4)`.
pub const RECORD_HEADER_LEN: usize = 15;

/// Hard cap on per-record payload length, matching the `BE u16` TCP
/// frame ceiling. The format's `len` field is u32 to leave room for
/// future protocols, but production payloads never exceed this.
pub const MAX_PAYLOAD_LEN: usize = 65_535;

// --- record + supporting POD types --------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    /// Wire-format byte value (`0` = Inbound, `1` = Outbound).
    pub fn as_byte(self) -> u8 {
        match self {
            Direction::Inbound => 0,
            Direction::Outbound => 1,
        }
    }

    pub fn from_byte(b: u8) -> Result<Self, CaptureError> {
        match b {
            0 => Ok(Direction::Inbound),
            1 => Ok(Direction::Outbound),
            other => Err(CaptureError::BadDirection(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Udp,
    Tcp,
}

impl TransportKind {
    /// Wire-format byte value (`0` = Udp, `1` = Tcp).
    pub fn as_byte(self) -> u8 {
        match self {
            TransportKind::Udp => 0,
            TransportKind::Tcp => 1,
        }
    }

    pub fn from_byte(b: u8) -> Result<Self, CaptureError> {
        match b {
            0 => Ok(TransportKind::Udp),
            1 => Ok(TransportKind::Tcp),
            other => Err(CaptureError::BadTransport(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRecord {
    /// Unix-epoch nanoseconds at the moment of capture.
    pub ts_unix_ns: u64,
    pub direction: Direction,
    pub transport: TransportKind,
    /// Outbound-TCP-only: the original hello byte (`true` =
    /// `[2, 0, …]`, `false` = `[2, 1, …]`). Ignored for other
    /// (direction, transport) combinations on read.
    pub hello: bool,
    pub payload: Vec<u8>,
}

// --- errors --------------------------------------------------------

#[derive(thiserror::Error, Debug)]
pub enum CaptureError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("not a ranchero wire capture file (bad magic)")]
    BadMagic,

    #[error("capture format version {0} not supported by this build")]
    UnsupportedVersion(u16),

    #[error("invalid direction byte: {0}")]
    BadDirection(u8),

    #[error("invalid transport byte: {0}")]
    BadTransport(u8),

    #[error("file truncated mid-record (read {got} of {needed} bytes)")]
    Truncated { needed: usize, got: usize },
}

// --- writer --------------------------------------------------------

/// Append-only writer for wire captures. Owns a background task that
/// drains a bounded channel and writes to disk. The hot-path
/// [`Self::record`] is sync and non-blocking — capture must never
/// affect live network behavior.
///
/// Cheap to clone? Wrap in `Arc` at the channel-config layer so
/// multiple channels (UDP + TCP, plus future per-course UDP pools)
/// share one writer.
#[derive(Debug)]
pub struct CaptureWriter {
    #[allow(dead_code)]
    sender: tokio::sync::mpsc::Sender<CaptureRecord>,
    #[allow(dead_code)]
    dropped_count: Arc<AtomicU64>,
    // The background writer task's JoinHandle lives here so
    // `flush_and_close` can await it.
    #[allow(dead_code)]
    writer_task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl CaptureWriter {
    /// Open `path` for writing, write the file header, spawn the
    /// background writer task. Default channel capacity tuned for
    /// production (see green-state implementation); tests that need
    /// to force the drop-on-saturation path use
    /// [`Self::open_with_capacity`].
    pub async fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let _ = path;
        unimplemented!("STEP-11.5: open file, write header, spawn writer task")
    }

    /// Test-only opener with an explicit channel capacity, used by
    /// drop-on-saturation tests. Production callers use [`Self::open`].
    pub async fn open_with_capacity(
        path: impl AsRef<Path>,
        capacity: usize,
    ) -> std::io::Result<Self> {
        let _ = (path, capacity);
        unimplemented!("STEP-11.5: same as open() but with caller-supplied channel capacity")
    }

    /// Buffer `record` for write. **Non-blocking; never awaits.**
    /// If the internal channel is full (slow disk), drop the record
    /// and bump [`Self::dropped_count`]. Safe to call from any task
    /// on any thread.
    pub fn record(&self, record: CaptureRecord) {
        let _ = record;
        unimplemented!("STEP-11.5: try_send; on full, increment dropped_count")
    }

    /// Cumulative count of records dropped due to channel saturation
    /// since [`Self::open`]. Never reset.
    pub fn dropped_count(&self) -> u64 {
        unimplemented!("STEP-11.5: load dropped_count atomically")
    }

    /// Flush pending records, fsync, close the file. Awaits the
    /// background writer task to drain.
    pub async fn flush_and_close(self) -> std::io::Result<()> {
        unimplemented!("STEP-11.5: drop sender, await writer task, return io::Result")
    }
}

// --- reader --------------------------------------------------------

/// Sync iterator over the records in a wire capture file. Replay is
/// read-once, sequential; sync API keeps the common case simple.
#[derive(Debug)]
pub struct CaptureReader {
    #[allow(dead_code)]
    version: u16,
    // Sync File handle + buffered reader live here in the
    // green-state implementation.
}

impl CaptureReader {
    /// Open `path`, validate the file header, return an iterator over
    /// the records.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CaptureError> {
        let _ = path;
        unimplemented!("STEP-11.5: open + read magic + read version + return iterator")
    }

    /// Format version from the file header (currently always
    /// [`VERSION`]).
    pub fn version(&self) -> u16 {
        self.version
    }
}

impl Iterator for CaptureReader {
    type Item = Result<CaptureRecord, CaptureError>;

    fn next(&mut self) -> Option<Self::Item> {
        unimplemented!("STEP-11.5: read RECORD_HEADER_LEN + payload, return Some(record) or None on EOF")
    }
}
