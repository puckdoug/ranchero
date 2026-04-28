// SPDX-License-Identifier: AGPL-3.0-only
//
// Wire-capture file format + writer + reader. Plaintext-only
// (post-decrypt for inbound, proto-bytes-only for outbound) so
// captures don't leak the AES session key. See
// `docs/plans/STEP-11.5-wire-capture.md` for the full design.

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncWriteExt as _;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

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

/// Default channel capacity between the channel hot path and the
/// writer task. Sized so a few hundred ms of disk stall doesn't drop
/// records under steady-state UDP load (~1 Hz outbound + a few Hz
/// inbound). Tests can override via [`CaptureWriter::open_with_capacity`].
const DEFAULT_CHANNEL_CAPACITY: usize = 4096;

const FLAG_HELLO: u8 = 0x01;

// --- record + supporting POD types --------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
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
    pub ts_unix_ns: u64,
    pub direction: Direction,
    pub transport: TransportKind,
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
#[derive(Debug)]
pub struct CaptureWriter {
    sender: mpsc::Sender<CaptureRecord>,
    dropped_count: Arc<AtomicU64>,
    writer_task: Option<JoinHandle<std::io::Result<()>>>,
}

impl CaptureWriter {
    pub async fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::open_with_capacity(path, DEFAULT_CHANNEL_CAPACITY).await
    }

    pub async fn open_with_capacity(
        path: impl AsRef<Path>,
        capacity: usize,
    ) -> std::io::Result<Self> {
        let mut file = tokio::fs::File::create(path.as_ref()).await?;
        let mut header = [0u8; FILE_HEADER_LEN];
        header[0..8].copy_from_slice(MAGIC);
        header[8..10].copy_from_slice(&VERSION.to_le_bytes());
        file.write_all(&header).await?;

        let (sender, receiver) = mpsc::channel::<CaptureRecord>(capacity);
        let writer_task = tokio::spawn(writer_task(file, receiver));

        Ok(Self {
            sender,
            dropped_count: Arc::new(AtomicU64::new(0)),
            writer_task: Some(writer_task),
        })
    }

    pub fn record(&self, record: CaptureRecord) {
        if self.sender.try_send(record).is_err() {
            // Either Full (slow disk) or Closed (writer task died).
            // Either way, count it as dropped — capture must never
            // backpressure the channel hot path.
            self.dropped_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    pub async fn flush_and_close(mut self) -> std::io::Result<()> {
        // Drop the sender so the writer task sees `recv() -> None` and
        // exits cleanly.
        let CaptureWriter {
            sender,
            writer_task,
            ..
        } = &mut self;
        drop(std::mem::replace(sender, mpsc::channel(1).0));
        if let Some(handle) = writer_task.take() {
            match handle.await {
                Ok(result) => result,
                Err(join_err) => Err(std::io::Error::other(format!(
                    "capture writer task panicked: {join_err}"
                ))),
            }
        } else {
            Ok(())
        }
    }
}

async fn writer_task(
    mut file: tokio::fs::File,
    mut rx: mpsc::Receiver<CaptureRecord>,
) -> std::io::Result<()> {
    while let Some(record) = rx.recv().await {
        write_record(&mut file, &record).await?;
    }
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

async fn write_record(file: &mut tokio::fs::File, record: &CaptureRecord) -> std::io::Result<()> {
    let mut header = [0u8; RECORD_HEADER_LEN];
    header[0..8].copy_from_slice(&record.ts_unix_ns.to_le_bytes());
    header[8] = record.direction.as_byte();
    header[9] = record.transport.as_byte();
    header[10] = if record.hello { FLAG_HELLO } else { 0 };
    let len = u32::try_from(record.payload.len()).unwrap_or(u32::MAX);
    header[11..15].copy_from_slice(&len.to_le_bytes());
    file.write_all(&header).await?;
    file.write_all(&record.payload).await?;
    Ok(())
}

// --- reader --------------------------------------------------------

/// Sync iterator over the records in a wire capture file. Replay is
/// read-once, sequential; sync API keeps the common case simple.
#[derive(Debug)]
pub struct CaptureReader {
    version: u16,
    reader: std::io::BufReader<std::fs::File>,
}

impl CaptureReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CaptureError> {
        let file = std::fs::File::open(path.as_ref())?;
        let mut reader = std::io::BufReader::new(file);
        let mut header = [0u8; FILE_HEADER_LEN];
        let n = read_partial(&mut reader, &mut header)?;
        if n < FILE_HEADER_LEN {
            // Empty or truncated header: treat as bad magic so the
            // caller doesn't have to special-case empty files.
            return Err(CaptureError::BadMagic);
        }
        if &header[0..8] != MAGIC {
            return Err(CaptureError::BadMagic);
        }
        let version = u16::from_le_bytes([header[8], header[9]]);
        if version != VERSION {
            return Err(CaptureError::UnsupportedVersion(version));
        }
        Ok(Self { version, reader })
    }

    pub fn version(&self) -> u16 {
        self.version
    }
}

impl Iterator for CaptureReader {
    type Item = Result<CaptureRecord, CaptureError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut header = [0u8; RECORD_HEADER_LEN];
        let n = match read_partial(&mut self.reader, &mut header) {
            Ok(n) => n,
            Err(e) => return Some(Err(CaptureError::Io(e))),
        };
        if n == 0 {
            // Clean EOF at a record boundary.
            return None;
        }
        if n < RECORD_HEADER_LEN {
            return Some(Err(CaptureError::Truncated {
                needed: RECORD_HEADER_LEN,
                got: n,
            }));
        }

        let ts_unix_ns = u64::from_le_bytes(header[0..8].try_into().unwrap());
        let direction = match Direction::from_byte(header[8]) {
            Ok(d) => d,
            Err(e) => return Some(Err(e)),
        };
        let transport = match TransportKind::from_byte(header[9]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let flags = header[10];
        let hello = (flags & FLAG_HELLO) != 0;
        let len = u32::from_le_bytes(header[11..15].try_into().unwrap()) as usize;

        let mut payload = vec![0u8; len];
        let got = match read_partial(&mut self.reader, &mut payload) {
            Ok(n) => n,
            Err(e) => return Some(Err(CaptureError::Io(e))),
        };
        if got < len {
            return Some(Err(CaptureError::Truncated {
                needed: len,
                got,
            }));
        }

        Some(Ok(CaptureRecord {
            ts_unix_ns,
            direction,
            transport,
            hello,
            payload,
        }))
    }
}

/// Read up to `buf.len()` bytes from `reader`, returning the actual
/// count. Distinguishes "clean EOF" (returns 0) from "partial read"
/// (returns 0 < n < buf.len()) so the iterator can surface
/// `Truncated` errors precisely.
fn read_partial(
    reader: &mut std::io::BufReader<std::fs::File>,
    buf: &mut [u8],
) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

// --- channel-tap helpers (called from `udp.rs` and `tcp.rs`) ------

/// Unix-epoch nanoseconds, the timestamp source captures use.
pub fn now_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Record an inbound packet's plaintext (after decrypt, before proto
/// decode). No-op when `capture` is `None`.
pub fn record_inbound(
    capture: Option<&Arc<CaptureWriter>>,
    transport: TransportKind,
    plaintext: &[u8],
) {
    if let Some(cap) = capture {
        cap.record(CaptureRecord {
            ts_unix_ns: now_unix_ns(),
            direction: Direction::Inbound,
            transport,
            hello: false,
            payload: plaintext.to_vec(),
        });
    }
}

/// Record an outbound packet's proto bytes (after `encode_to_vec`,
/// before envelope wrap). `hello` is meaningful only for outbound TCP
/// (the channel passes `false` for UDP). No-op when `capture` is
/// `None`.
pub fn record_outbound(
    capture: Option<&Arc<CaptureWriter>>,
    transport: TransportKind,
    hello: bool,
    proto_bytes: &[u8],
) {
    if let Some(cap) = capture {
        cap.record(CaptureRecord {
            ts_unix_ns: now_unix_ns(),
            direction: Direction::Outbound,
            transport,
            hello,
            payload: proto_bytes.to_vec(),
        });
    }
}

// --- follower (STEP-12.2 stub) ------------------------------------

/// Tailing reader over a wire-capture file. Like
/// [`CaptureReader`], but on end-of-file or a truncated-record
/// condition, the iterator sleeps and retries rather than
/// returning `None` or an error. Exits when the writer signals
/// that no further records will arrive (the file is closed and a
/// configured idle timeout elapses) or when the caller drops the
/// iterator.
///
/// STEP-12.2 stub: every method panics with `unimplemented!()`.
/// See `docs/plans/STEP-12.2-follow-command.md` for the design.
#[derive(Debug)]
#[allow(dead_code)]
pub struct CaptureFollower {
    poll_interval: Duration,
    idle_timeout: Option<Duration>,
}

impl CaptureFollower {
    /// Open `path`, validate the file header, return a follower
    /// with default tuning (`poll_interval = 100 ms`,
    /// `idle_timeout = None`).
    pub fn open(_path: impl AsRef<Path>) -> Result<Self, CaptureError> {
        unimplemented!("STEP-12.2: CaptureFollower::open")
    }

    /// Override the polling interval used between end-of-file
    /// retries.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set an idle timeout. When the follower has not observed a
    /// new record for this duration, the iterator returns `None`.
    pub fn with_idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Format version from the file header (currently always 1).
    pub fn version(&self) -> u16 {
        unimplemented!("STEP-12.2: CaptureFollower::version")
    }
}

impl Iterator for CaptureFollower {
    type Item = Result<CaptureRecord, CaptureError>;

    fn next(&mut self) -> Option<Self::Item> {
        unimplemented!("STEP-12.2: CaptureFollower::next")
    }
}
