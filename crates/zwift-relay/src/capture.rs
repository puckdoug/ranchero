// SPDX-License-Identifier: AGPL-3.0-only
//
// Wire-capture file format + writer + reader. Stores the encrypted
// bytes that crossed the socket along with a per-session manifest
// containing the AES key and IV state needed for offline replay; see
// `docs/plans/STEP-12.12-log-shit-properly.md` for the full design.
// The capture file inherits the trust boundary of the daemon process —
// AES keys and credentials are written verbatim.

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

/// Current wire-capture format version. v2 adds the per-record kind
/// byte and the `Manifest` record kind; v1 captures are not supported.
pub const VERSION: u16 = 2;

/// File header byte length (`MAGIC` + version u16 LE).
pub const FILE_HEADER_LEN: usize = 10;

/// Per-record fixed-overhead byte length (v2 layout):
/// `ts_unix_ns(8) + kind(1) + direction(1) + transport(1) + flags(1) + len(4)`.
pub const RECORD_HEADER_LEN: usize = 16;

/// Hard cap on per-record payload length, matching the `BE u16` TCP
/// frame ceiling. The format's `len` field is u32 to leave room for
/// future protocols, but production payloads never exceed this.
pub const MAX_PAYLOAD_LEN: usize = 65_535;

/// Serialised size of [`SessionManifest`] when written as the payload
/// of a `RecordKind::Manifest` record.
pub const MANIFEST_PAYLOAD_LEN: usize = 50;

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
    Http,
}

impl TransportKind {
    pub fn as_byte(self) -> u8 {
        match self {
            TransportKind::Udp => 0,
            TransportKind::Tcp => 1,
            TransportKind::Http => 2,
        }
    }

    pub fn from_byte(b: u8) -> Result<Self, CaptureError> {
        match b {
            0 => Ok(TransportKind::Udp),
            1 => Ok(TransportKind::Tcp),
            2 => Ok(TransportKind::Http),
            other => Err(CaptureError::BadTransport(other)),
        }
    }
}

/// Discriminant byte that appears at offset 8 of every v2 record
/// header. `Frame` records carry on-the-wire bytes; `Manifest` records
/// carry the [`SessionManifest`] needed to decrypt the frames that
/// follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Frame,
    Manifest,
}

impl RecordKind {
    pub fn as_byte(self) -> u8 {
        match self {
            RecordKind::Frame => 0,
            RecordKind::Manifest => 1,
        }
    }

    pub fn from_byte(b: u8) -> Result<Self, CaptureError> {
        match b {
            0 => Ok(RecordKind::Frame),
            1 => Ok(RecordKind::Manifest),
            other => Err(CaptureError::BadRecordKind(other)),
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

/// Per-session decrypt material written once after the file header
/// (and again after every supervisor refresh that rotates the key).
/// Carries the AES-128 session key, the IV-input discriminants, the
/// starting IV sequence numbers per direction per transport, and the
/// `relay_id` / `conn_id` values that subsequent frames reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionManifest {
    pub aes_key: [u8; 16],
    pub device_type: u8,
    pub channel_type: u8,
    pub send_iv_seqno_tcp: u32,
    pub recv_iv_seqno_tcp: u32,
    pub send_iv_seqno_udp: u32,
    pub recv_iv_seqno_udp: u32,
    pub relay_id: u32,
    pub conn_id: u32,
    pub expires_at_unix_ns: u64,
}

impl SessionManifest {
    fn encode(&self) -> [u8; MANIFEST_PAYLOAD_LEN] {
        let mut buf = [0u8; MANIFEST_PAYLOAD_LEN];
        buf[0..16].copy_from_slice(&self.aes_key);
        buf[16] = self.device_type;
        buf[17] = self.channel_type;
        buf[18..22].copy_from_slice(&self.send_iv_seqno_tcp.to_le_bytes());
        buf[22..26].copy_from_slice(&self.recv_iv_seqno_tcp.to_le_bytes());
        buf[26..30].copy_from_slice(&self.send_iv_seqno_udp.to_le_bytes());
        buf[30..34].copy_from_slice(&self.recv_iv_seqno_udp.to_le_bytes());
        buf[34..38].copy_from_slice(&self.relay_id.to_le_bytes());
        buf[38..42].copy_from_slice(&self.conn_id.to_le_bytes());
        buf[42..50].copy_from_slice(&self.expires_at_unix_ns.to_le_bytes());
        buf
    }

    fn decode(bytes: &[u8]) -> Result<Self, CaptureError> {
        if bytes.len() != MANIFEST_PAYLOAD_LEN {
            return Err(CaptureError::Truncated {
                needed: MANIFEST_PAYLOAD_LEN,
                got: bytes.len(),
            });
        }
        let mut aes_key = [0u8; 16];
        aes_key.copy_from_slice(&bytes[0..16]);
        Ok(Self {
            aes_key,
            device_type: bytes[16],
            channel_type: bytes[17],
            send_iv_seqno_tcp: u32::from_le_bytes(bytes[18..22].try_into().unwrap()),
            recv_iv_seqno_tcp: u32::from_le_bytes(bytes[22..26].try_into().unwrap()),
            send_iv_seqno_udp: u32::from_le_bytes(bytes[26..30].try_into().unwrap()),
            recv_iv_seqno_udp: u32::from_le_bytes(bytes[30..34].try_into().unwrap()),
            relay_id: u32::from_le_bytes(bytes[34..38].try_into().unwrap()),
            conn_id: u32::from_le_bytes(bytes[38..42].try_into().unwrap()),
            expires_at_unix_ns: u64::from_le_bytes(bytes[42..50].try_into().unwrap()),
        })
    }
}

/// One record yielded by [`CaptureReader::next_item`]. `Frame` carries
/// on-the-wire bytes; `Manifest` carries decrypt material for the
/// frames that follow until the next manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureItem {
    Frame(CaptureRecord),
    Manifest(SessionManifest),
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

    #[error("invalid record-kind byte: {0}")]
    BadRecordKind(u8),

    #[error("file truncated mid-record (read {got} of {needed} bytes)")]
    Truncated { needed: usize, got: usize },
}

// --- writer --------------------------------------------------------

/// Internal message carried over the writer mpsc. Frame and manifest
/// records share the same channel so ordering is preserved.
#[derive(Debug)]
enum WriterMsg {
    Frame(CaptureRecord),
    Manifest(SessionManifest),
}

/// Append-only writer for wire captures. Owns a background task
/// that drains a bounded channel and writes to disk. The hot-path
/// [`Self::record`] is sync and non-blocking — capture must never
/// affect live network behavior.
///
/// The writer is designed for shared ownership through `Arc`. The
/// internal sender and writer-task handle live behind an interior
/// mutex so that [`Self::flush_and_close`] can be called by any
/// holder of an `Arc<CaptureWriter>`. Calls after the first close
/// are no-ops.
#[derive(Debug)]
pub struct CaptureWriter {
    sender: std::sync::Mutex<Option<mpsc::Sender<WriterMsg>>>,
    dropped_count: AtomicU64,
    writer_task: std::sync::Mutex<Option<JoinHandle<std::io::Result<()>>>>,
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

        let (sender, receiver) = mpsc::channel::<WriterMsg>(capacity);
        let writer_task = tokio::spawn(writer_task(file, receiver));

        Ok(Self {
            sender: std::sync::Mutex::new(Some(sender)),
            dropped_count: AtomicU64::new(0),
            writer_task: std::sync::Mutex::new(Some(writer_task)),
        })
    }

    /// Create the capture file at `path` and write the 10-byte format header
    /// using synchronous standard-library I/O. Returns the open `File`.
    ///
    /// This is the pre-fork half of the two-step capture setup used by the
    /// daemon: `validate_startup` calls this before `daemonize_self` (no
    /// Tokio runtime yet), and `from_file` is called post-fork inside the
    /// runtime to complete the writer.
    pub fn create_header_sync(path: &Path) -> std::io::Result<std::fs::File> {
        use std::io::Write as _;
        let mut file = std::fs::File::create(path)?;
        let mut header = [0u8; FILE_HEADER_LEN];
        header[0..8].copy_from_slice(MAGIC);
        header[8..10].copy_from_slice(&VERSION.to_le_bytes());
        file.write_all(&header)?;
        file.flush()?;
        Ok(file)
    }

    /// Wrap an already-opened (and header-written) `std::fs::File` in a
    /// `CaptureWriter` by spawning the background writer task. Called
    /// post-fork inside the Tokio runtime to complete the hand-off from
    /// [`Self::create_header_sync`].
    pub async fn from_file(file: std::fs::File) -> std::io::Result<Self> {
        let file = tokio::fs::File::from_std(file);
        let (sender, receiver) = mpsc::channel::<WriterMsg>(DEFAULT_CHANNEL_CAPACITY);
        let task = tokio::spawn(writer_task(file, receiver));
        Ok(Self {
            sender: std::sync::Mutex::new(Some(sender)),
            dropped_count: AtomicU64::new(0),
            writer_task: std::sync::Mutex::new(Some(task)),
        })
    }

    pub fn record(&self, record: CaptureRecord) {
        self.send_msg(WriterMsg::Frame(record));
    }

    /// Append a [`SessionManifest`] to the capture. The manifest is
    /// written as a record with `RecordKind::Manifest` so a future
    /// `ranchero follow` can recover the AES key and IV state needed
    /// to decrypt the frames that follow.
    pub fn record_session_manifest(&self, manifest: SessionManifest) {
        self.send_msg(WriterMsg::Manifest(manifest));
    }

    fn send_msg(&self, msg: WriterMsg) {
        let guard = self.sender.lock().expect("capture sender mutex");
        if let Some(sender) = guard.as_ref() {
            if sender.try_send(msg).is_err() {
                // Either Full (slow disk) or Closed (writer task
                // died). Either way, count it as dropped —
                // capture must never backpressure the hot path.
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            // After flush_and_close: drop silently. Counting these
            // would inflate dropped_count for callers that
            // legitimately stopped using the writer.
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    /// Flush pending records, await the writer task, and close
    /// the file. Idempotent: subsequent calls return `Ok(())`.
    /// Takes `&self` so that `Arc<CaptureWriter>` holders can call
    /// it without consuming the inner value.
    pub async fn flush_and_close(&self) -> std::io::Result<()> {
        // Drop the sender so the writer task sees `recv() -> None`
        // and exits cleanly. Subsequent `record()` calls become
        // no-ops.
        let _ = self.sender.lock().expect("capture sender mutex").take();

        // Take the writer task out (must release the lock before
        // awaiting). If a concurrent `flush_and_close` already
        // took it, this call sees `None` and returns Ok.
        let handle = self
            .writer_task
            .lock()
            .expect("capture writer-task mutex")
            .take();

        match handle {
            Some(h) => match h.await {
                Ok(result) => result,
                Err(join_err) => Err(std::io::Error::other(format!(
                    "capture writer task panicked: {join_err}"
                ))),
            },
            None => Ok(()),
        }
    }
}

async fn writer_task(
    mut file: tokio::fs::File,
    mut rx: mpsc::Receiver<WriterMsg>,
) -> std::io::Result<()> {
    while let Some(msg) = rx.recv().await {
        match msg {
            WriterMsg::Frame(record) => write_frame_record(&mut file, &record).await?,
            WriterMsg::Manifest(manifest) => write_manifest_record(&mut file, &manifest).await?,
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

async fn write_frame_record(
    file: &mut tokio::fs::File,
    record: &CaptureRecord,
) -> std::io::Result<()> {
    let mut header = [0u8; RECORD_HEADER_LEN];
    header[0..8].copy_from_slice(&record.ts_unix_ns.to_le_bytes());
    header[8] = RecordKind::Frame.as_byte();
    header[9] = record.direction.as_byte();
    header[10] = record.transport.as_byte();
    header[11] = if record.hello { FLAG_HELLO } else { 0 };
    let len = u32::try_from(record.payload.len()).unwrap_or(u32::MAX);
    header[12..16].copy_from_slice(&len.to_le_bytes());
    file.write_all(&header).await?;
    file.write_all(&record.payload).await?;
    Ok(())
}

async fn write_manifest_record(
    file: &mut tokio::fs::File,
    manifest: &SessionManifest,
) -> std::io::Result<()> {
    let payload = manifest.encode();
    let mut header = [0u8; RECORD_HEADER_LEN];
    header[0..8].copy_from_slice(&now_unix_ns().to_le_bytes());
    header[8] = RecordKind::Manifest.as_byte();
    // Direction, transport, and flags are unused for manifest records
    // (zeroed); the kind byte is the sole discriminant.
    let len = MANIFEST_PAYLOAD_LEN as u32;
    header[12..16].copy_from_slice(&len.to_le_bytes());
    file.write_all(&header).await?;
    file.write_all(&payload).await?;
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

    /// Read the next item — frame or manifest — from the file. Returns
    /// `None` at clean EOF.
    pub fn next_item(&mut self) -> Option<Result<CaptureItem, CaptureError>> {
        let mut header = [0u8; RECORD_HEADER_LEN];
        let n = match read_partial(&mut self.reader, &mut header) {
            Ok(n) => n,
            Err(e) => return Some(Err(CaptureError::Io(e))),
        };
        if n == 0 {
            return None;
        }
        if n < RECORD_HEADER_LEN {
            return Some(Err(CaptureError::Truncated {
                needed: RECORD_HEADER_LEN,
                got: n,
            }));
        }

        let parsed = match parse_record_header(&header) {
            Ok(p) => p,
            Err(e) => return Some(Err(e)),
        };
        let mut payload = vec![0u8; parsed.payload_len];
        let got = match read_partial(&mut self.reader, &mut payload) {
            Ok(n) => n,
            Err(e) => return Some(Err(CaptureError::Io(e))),
        };
        if got < parsed.payload_len {
            return Some(Err(CaptureError::Truncated {
                needed: parsed.payload_len,
                got,
            }));
        }
        Some(parsed.into_item(payload))
    }
}

impl Iterator for CaptureReader {
    type Item = Result<CaptureRecord, CaptureError>;

    fn next(&mut self) -> Option<Self::Item> {
        // Yield only frame records; manifests are surfaced through
        // [`CaptureReader::next_item`].
        loop {
            match self.next_item()? {
                Ok(CaptureItem::Frame(rec)) => return Some(Ok(rec)),
                Ok(CaptureItem::Manifest(_)) => continue,
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

/// Parsed v2 record header awaiting payload bytes.
struct ParsedHeader {
    ts_unix_ns: u64,
    kind: RecordKind,
    direction: Direction,
    transport: TransportKind,
    hello: bool,
    payload_len: usize,
}

impl ParsedHeader {
    fn into_item(self, payload: Vec<u8>) -> Result<CaptureItem, CaptureError> {
        match self.kind {
            RecordKind::Frame => Ok(CaptureItem::Frame(CaptureRecord {
                ts_unix_ns: self.ts_unix_ns,
                direction: self.direction,
                transport: self.transport,
                hello: self.hello,
                payload,
            })),
            RecordKind::Manifest => Ok(CaptureItem::Manifest(SessionManifest::decode(&payload)?)),
        }
    }
}

fn parse_record_header(header: &[u8; RECORD_HEADER_LEN]) -> Result<ParsedHeader, CaptureError> {
    let ts_unix_ns = u64::from_le_bytes(header[0..8].try_into().unwrap());
    let kind = RecordKind::from_byte(header[8])?;
    let payload_len = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    match kind {
        RecordKind::Frame => {
            let direction = Direction::from_byte(header[9])?;
            let transport = TransportKind::from_byte(header[10])?;
            let hello = (header[11] & FLAG_HELLO) != 0;
            Ok(ParsedHeader {
                ts_unix_ns,
                kind,
                direction,
                transport,
                hello,
                payload_len,
            })
        }
        RecordKind::Manifest => Ok(ParsedHeader {
            ts_unix_ns,
            kind,
            // Manifest records ignore these fields; populate with
            // sensible placeholders so the struct stays POD.
            direction: Direction::Inbound,
            transport: TransportKind::Tcp,
            hello: false,
            payload_len,
        }),
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

// --- follower (STEP-12.2) -----------------------------------------

/// Tailing reader over a wire-capture file. Like
/// [`CaptureReader`], but on end-of-file or a truncated-record
/// condition, the iterator sleeps and retries rather than
/// returning `None` or an error. Exits when the writer signals
/// that no further records will arrive (the file is closed and a
/// configured idle timeout elapses) or when the caller drops the
/// iterator.
///
/// The follower opens the file with `std::fs::File` directly
/// rather than wrapping it in a `BufReader`, so that data
/// appended by another process (or another task on the same
/// process) becomes visible on the next `read` call without
/// waiting for a buffer flush.
#[derive(Debug)]
pub struct CaptureFollower {
    file: std::fs::File,
    version: u16,
    poll_interval: Duration,
    idle_timeout: Option<Duration>,
}

impl CaptureFollower {
    /// Open `path`, validate the file header, return a follower
    /// with default tuning (`poll_interval = 100 ms`,
    /// `idle_timeout = None`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CaptureError> {
        let mut file = std::fs::File::open(path.as_ref())?;
        let mut header = [0u8; FILE_HEADER_LEN];
        let n = read_partial_io(&mut file, &mut header)?;
        if n < FILE_HEADER_LEN {
            return Err(CaptureError::BadMagic);
        }
        if &header[0..8] != MAGIC {
            return Err(CaptureError::BadMagic);
        }
        let version = u16::from_le_bytes([header[8], header[9]]);
        if version != VERSION {
            return Err(CaptureError::UnsupportedVersion(version));
        }
        Ok(Self {
            file,
            version,
            poll_interval: Duration::from_millis(100),
            idle_timeout: None,
        })
    }

    /// Override the polling interval used between end-of-file
    /// retries. Lower values reduce latency at the cost of CPU.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set an idle timeout. When the follower has not observed a
    /// new record for this duration, the iterator returns `None`.
    /// `None` (the default) means "run until interrupted".
    pub fn with_idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Format version from the file header (currently always 1).
    pub fn version(&self) -> u16 {
        self.version
    }
}

impl Iterator for CaptureFollower {
    type Item = Result<CaptureRecord, CaptureError>;

    fn next(&mut self) -> Option<Self::Item> {
        // The follower yields only frame records to keep its API
        // identical to [`CaptureReader`]; manifest records are skipped.
        loop {
            match self.next_item()? {
                Ok(CaptureItem::Frame(rec)) => return Some(Ok(rec)),
                Ok(CaptureItem::Manifest(_)) => continue,
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

impl CaptureFollower {
    /// Read the next item — frame or manifest — from the followed
    /// file, blocking on partial-read / EOF until the writer extends
    /// the file or the configured idle timeout elapses.
    pub fn next_item(&mut self) -> Option<Result<CaptureItem, CaptureError>> {
        use std::io::Seek as _;

        let last_progress = std::time::Instant::now();
        let pos_before_record = match self.file.stream_position() {
            Ok(p) => p,
            Err(e) => return Some(Err(CaptureError::Io(e))),
        };

        loop {
            let mut header = [0u8; RECORD_HEADER_LEN];
            let n = match read_partial_io(&mut self.file, &mut header) {
                Ok(n) => n,
                Err(e) => return Some(Err(CaptureError::Io(e))),
            };

            if n < RECORD_HEADER_LEN {
                if let Err(e) =
                    self.file.seek(std::io::SeekFrom::Start(pos_before_record))
                {
                    return Some(Err(CaptureError::Io(e)));
                }
                if let Some(timeout) = self.idle_timeout
                    && last_progress.elapsed() >= timeout
                {
                    return None;
                }
                std::thread::sleep(self.poll_interval);
                continue;
            }

            let parsed = match parse_record_header(&header) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };

            let mut payload = vec![0u8; parsed.payload_len];
            let got = match read_partial_io(&mut self.file, &mut payload) {
                Ok(n) => n,
                Err(e) => return Some(Err(CaptureError::Io(e))),
            };

            if got < parsed.payload_len {
                if let Err(e) =
                    self.file.seek(std::io::SeekFrom::Start(pos_before_record))
                {
                    return Some(Err(CaptureError::Io(e)));
                }
                if let Some(timeout) = self.idle_timeout
                    && last_progress.elapsed() >= timeout
                {
                    return None;
                }
                std::thread::sleep(self.poll_interval);
                continue;
            }

            return Some(parsed.into_item(payload));
        }
    }
}

/// Generic partial-read helper used by `CaptureFollower`.
/// Distinguishes "clean EOF" (returns 0) from "partial read"
/// (returns 0 < n < buf.len()) so the follower can detect a
/// mid-record truncation precisely.
fn read_partial_io<R: std::io::Read>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
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
