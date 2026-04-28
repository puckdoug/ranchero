# Step 11.5 — Wire capture & replay

**Status:** planned (2026-04-27).

## Goal

Make ranchero self-sufficient for producing the captured
`ServerToClient` / `ClientToServer` fixtures that STEPS 08, 18, and
19 depend on, instead of inheriting them from a JS reference
implementation. Useful for debugging and for exploring future
protocol changes.

- `ranchero start --capture <path>` opens a writer; capture is off
  when the flag is absent (zero overhead in that path: an
  `Option::None` branch in the channel hot path).
- A single `Capture` tap covers both UDP (STEP 10) and TCP
  (STEP 11) channels. The tap sits on the recv path **after**
  AES-GCM tag verification and **before** prost decode, and on the
  send path **after** prost encode and **after** envelope stripping
  (so what is stored is exactly the bytes that go into and come
  out of `prost::decode`).
- `ranchero replay <path>` reads a capture and prints a summary.
  The programmatic `CaptureReader` (an `Iterator`) is the surface
  STEPS 18 and 19 consume for parity tests.

## Scope

**In scope**:

- Append-only file format: 10-byte header (magic and version),
  variable-length records (timestamp, direction, transport, flags,
  length, and payload). Concrete byte specification below.
- `CaptureWriter`: opens the file, owns a background writer task,
  exposes a non-blocking synchronous `record(&self, …)` for the
  channel hot path, exposes `dropped_count()`, and provides a
  `flush_and_close()` for graceful shutdown.
- `CaptureReader`: `Iterator<Item = Result<CaptureRecord,
  CaptureError>>` over a capture file.
- `CaptureRecord` POD type both writer and reader use.
- `UdpChannelConfig::capture` and `TcpChannelConfig::capture`:
  optional `Arc<CaptureWriter>` injected at construction. `None`
  means no tap (zero hot-path overhead beyond an `Option::is_some`
  check that LLVM should fold into an inert branch when the field
  is statically known to be `None`).
- CLI: `--capture <path>` flag on `start`; new top-level
  `replay <path>` subcommand printing a summary.

**Out of scope** (defer):

| Concern | Why deferred |
|---|---|
| `--capture-raw` (ciphertext, IV, AAD, AES key) | Stub already deferred. Useful only for STEP 08-style codec replay; the project already has custom test vectors there. Not blocking STEP 18/19. |
| Capture rotation and size limits | A single file per `start` invocation is sufficient for the fixture use case. |
| `CaptureReader` doing full pipeline replay (decode → stats → WS broadcast) | STEP 18 and later own the pipeline; this step provides the bytes. |
| Multi-channel disambiguation (per-UDP-pool channel id) | Out for v1. A single UDP channel is the steady-state today. If a multi-UDP capture is ever needed, the `flags` byte has 7 reserved bits and the format version can be incremented. |
| `fsync` per record | Per-close `fsync` (via `flush_and_close`) is sufficient for fixture use; per-record would devastate throughput. |
| Compression | Plaintext proto bytes compress well, but compression adds a dependency and complexity. Add only if file sizes become a problem in practice. |

## Crate layout

`zwift-relay` already owns the channels that need the tap. Adding
the capture there keeps coupling minimal: no cross-crate
dependency edge.

```
crates/zwift-relay/
├── Cargo.toml          (no edits expected — uses tokio fs which is in `net` or `fs` feature)
├── src/
│   ├── …               (existing: consts, codec, session, world_timer, udp, tcp)
│   └── capture.rs      ← NEW (writer + reader + format constants)
└── tests/
    └── capture.rs      ← NEW (round-trip + format + drop-on-saturation + channel-tap tests)
```

If `capture.rs` grows past approximately 500 lines or develops
independent sub-concerns, fold into
`capture/{mod, format, writer, reader}.rs`. The plan starts flat.

The CLI surface is in the root `ranchero` crate
(`src/cli.rs`: extend the existing `Command` enum and the
`dispatch()` arm).

## Dependencies

`crates/zwift-relay/Cargo.toml` may need the tokio `fs` feature
added (the `net` feature is already enabled for STEP 10). Verify
against what `tokio::fs::File` requires at implementation time. No
new direct dependencies expected.

The root `ranchero` crate already depends on `zwift-relay` (added
in STEP 09's wave); CLI dispatch can call `CaptureReader` directly.

## File format (concrete byte spec)

All multi-byte integers are **little-endian**. Total fixed overhead
per record: 15 bytes.

### File header (10 bytes, written once at file open)

```
offset 0..8  : magic bytes b"RNCWCAP\0" (ASCII, NUL terminator included)
offset 8..10 : version u16 LE
```

Version `1` is the format described below. Reading a file with a
higher version returns `CaptureError::UnsupportedVersion(v)`.

### Per-record (variable length, repeated until EOF)

```
offset  0..8  : ts_unix_ns       u64 LE  — Unix-epoch nanoseconds at the moment of capture
offset  8     : direction        u8      — 0 = Inbound (server→client), 1 = Outbound (client→server)
offset  9     : transport        u8      — 0 = Udp, 1 = Tcp
offset 10     : flags            u8      — bit 0: hello (only meaningful when direction=Outbound and transport=Tcp); bits 1..7 reserved (must be zero on write, ignored on read)
offset 11..15 : len              u32 LE  — payload byte count, capped at 65 535 (`u16::MAX`, the TCP frame ceiling)
offset 15..   : payload          [u8; len] — the bytes that go into and come out of prost::decode
```

### "Plaintext" semantics

- **Inbound** (server→client, both UDP and TCP): the payload is the
  raw decrypted bytes, directly `ServerToClient::decode`-able.
  Sauce's `zwift.mjs:1285-1286` and `:1427` both decode without
  envelope stripping; this implementation matches.
- **Outbound** (client→server): the payload is the **proto bytes
  only**, with the `[1]` (UDP) or `[2, hello?]` (TCP) envelope
  **already stripped**. The `flags` byte's bit 0 records the
  outbound TCP hello state so a replayer can reconstruct the
  envelope without inspecting bytes.

This makes both directions trivially decode-symmetric (always pass
`payload` to `decode_to_vec`-style methods) at the cost of one
small strip on the outbound tap.

### Why little-endian, not big-endian

Wire formats use BE. Captures are local-machine artifacts written
and read on the same architecture (almost always x86-64 or arm64,
both LE). LE is one byteswap cheaper per integer on the writer
hot path. If a capture ever needs to move between LE and BE hosts,
the format version can be incremented.

## Public API surface (proposed)

### Records

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction { Inbound, Outbound }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind { Udp, Tcp }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRecord {
    pub ts_unix_ns: u64,
    pub direction:  Direction,
    pub transport:  TransportKind,
    /// Outbound-TCP-only: the original hello byte (`true` =
    /// `[2, 0, …]`, `false` = `[2, 1, …]`). Ignored for other
    /// (direction, transport) combinations.
    pub hello:      bool,
    pub payload:    Vec<u8>,
}
```

### Writer

```rust
pub struct CaptureWriter { /* private */ }

impl CaptureWriter {
    /// Open `path` for writing, write the file header, spawn the
    /// background writer task. Returns the handle (cheap to share
    /// via `Arc`) that channels feed records into.
    pub async fn open(path: impl AsRef<Path>) -> std::io::Result<Self>;

    /// Buffer `record` for write. **Non-blocking; never awaits.**
    /// If the internal channel is full (slow disk), drop the record
    /// and bump `dropped_count`. Capture must never backpressure
    /// the channel hot path. Safe to call from any task on any
    /// thread; takes `&self`.
    pub fn record(&self, record: CaptureRecord);

    /// How many records have been dropped due to channel
    /// saturation since open. Cumulative; never reset.
    pub fn dropped_count(&self) -> u64;

    /// Flush pending records, fsync, close the file. Awaits the
    /// background writer task to drain. Call once at supervisor
    /// shutdown.
    pub async fn flush_and_close(self) -> std::io::Result<()>;
}
```

The `record()` API is *deliberately synchronous*. Channels call it
from inside their hot loops, where `await` would either complicate
control flow (an additional `select!` arm) or risk introducing
ordering questions. `try_send` on an `mpsc::Sender` is the
appropriate primitive.

### Reader

```rust
pub struct CaptureReader { /* private */ }

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

impl CaptureReader {
    /// Open `path`, validate the file header, return an iterator
    /// over the records.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CaptureError>;

    /// Format version from the file header (currently always `1`).
    pub fn version(&self) -> u16;
}

impl Iterator for CaptureReader {
    type Item = Result<CaptureRecord, CaptureError>;
    fn next(&mut self) -> Option<Self::Item> { /* … */ }
}
```

The reader is **synchronous**, not async. Replay use cases iterate
the file at their own pace; async machinery would only complicate
the common case (read once, iterate, decode). If batch-replay
later requires concurrent file reads and decode, wrap the iterator
in a `spawn_blocking`.

## Tap integration

### `UdpChannelConfig` / `TcpChannelConfig` extended

```rust
pub struct UdpChannelConfig {
    // (existing fields — STEP 10)
    pub capture: Option<Arc<CaptureWriter>>,
}

pub struct TcpChannelConfig {
    // (existing fields — STEP 11)
    pub capture: Option<Arc<CaptureWriter>>,
}
```

`Default` implementations set `capture: None`. Production callers
(STEP 12 supervisor) wire it in when the user passed
`--capture <path>`; tests construct with `None` and are not
affected.

### Where the tap calls live in `udp.rs`

- **Inbound** (recv loop): immediately after `process_inbound_packet`
  returns `Ok(stc)`, before sending the `Inbound` event.
  Capture is on the **plaintext bytes**, not the parsed `stc`, so
  the tap fires inside `process_inbound_packet` after
  `decrypt(...)` succeeds and before `ServerToClient::decode(...)`.
  Refactor: move the proto decode out of `process_inbound_packet`
  and into the recv loop, so the function returns `Vec<u8>` and the
  loop both decodes and (optionally) records.

- **Outbound** (`send_player_state`): immediately after
  `proto_bytes = cts.encode_to_vec()`, before `udp_plaintext`
  wraps it.

### Where the tap calls live in `tcp.rs`

- **Inbound**: same shape as UDP: `process_inbound` returns the
  decrypted `Vec<u8>`; the loop decodes and records.
- **Outbound** (`send_packet`): immediately after
  `proto_bytes = payload.encode_to_vec()`, before `tcp_plaintext`
  wraps it. `flags` bit 0 = `hello`.

### Tap call shape (both channels)

```rust
if let Some(cap) = &self.config.capture {
    cap.record(CaptureRecord {
        ts_unix_ns: capture::now_unix_ns(),
        direction:  Direction::Outbound,
        transport:  TransportKind::Udp,    // or Tcp
        hello:      false,                 // or `hello` for outbound TCP
        payload:    proto_bytes.clone(),
    });
}
```

The clone is acceptable: capture is opt-in, and the cost is one
copy of bytes already on the heap. If profile data later shows
this matters, swap `Vec<u8>` for `bytes::Bytes` end-to-end and
share via inexpensive clone.

## CLI surface

### `start --capture <path>`

Add an optional flag to the existing `start` subcommand:

```rust
#[derive(Args)]
pub struct StartArgs {
    // (existing flags …)
    /// Write a wire-capture file alongside the live session.
    #[arg(long, value_name = "PATH")]
    pub capture: Option<PathBuf>,
}
```

`dispatch()` for `start` opens the writer (if `Some`), wires the
`Arc<CaptureWriter>` into both channel configs via STEP 12's
supervisor, and registers a graceful-shutdown hook to call
`flush_and_close()` on exit.

### `replay <path>`

New top-level subcommand:

```rust
#[derive(Subcommand)]
pub enum Command {
    // (existing variants …)
    /// Print a summary of a wire capture file.
    Replay {
        path: PathBuf,
        /// Print one line per record instead of a summary.
        #[arg(long)]
        verbose: bool,
    },
}
```

`dispatch()` for `Replay`:

- `summary` mode (default): record count by `(direction,
  transport)`, total bytes, time range, dropped-count if recorded
  in a future format version (today: not stored).
- `verbose` mode: one line per record:
  `<ts> <direction> <transport> <hello> <len> bytes`.

User-visible (not xtask) because the stub asked for the option to
explore captures interactively.

## Tests-first plan

All tests in `crates/zwift-relay/tests/capture.rs`. Channel-tap
tests reside in the existing `tests/udp.rs` and `tests/tcp.rs`
(extend rather than duplicate the mock-transport setup).

### Format & header

| Test | Asserts |
|---|---|
| `file_header_starts_with_magic_and_version` | After `CaptureWriter::open` + `flush_and_close`, the first 8 bytes of the file are `b"RNCWCAP\0"` and bytes 8..10 are `[0x01, 0x00]` (version 1, LE). |
| `reader_rejects_bad_magic` | `tempfile` written with a non-magic header → `CaptureReader::open` returns `Err(BadMagic)`. |
| `reader_rejects_unsupported_version` | File with magic but version 2 → `Err(UnsupportedVersion(2))`. |
| `reader_handles_empty_file` | Zero-byte file → `Err(BadMagic)` (header missing, not silent). |

### Round-trip

| Test | Asserts |
|---|---|
| `writer_then_reader_round_trip_one_record` | Write a single record, close, read; record is byte-equal. |
| `writer_then_reader_round_trip_many_records` | Write 1 000 random records, close, read; all 1 000 byte-equal in order. |
| `record_direction_inbound_outbound_round_trip` | Both `Inbound` and `Outbound` survive the round-trip. |
| `record_transport_udp_tcp_round_trip` | Both `Udp` and `Tcp` survive. |
| `record_hello_flag_round_trip` | `hello=true` and `hello=false` survive. |
| `record_payload_max_len_round_trips` | `len = u16::MAX = 65 535` payload survives. |

### Truncation & error paths

| Test | Asserts |
|---|---|
| `reader_handles_truncated_record_header` | File ending after 5 of the 15 record-header bytes → `Err(Truncated { needed: 15, got: 5 })`. |
| `reader_handles_truncated_payload` | File with a record header advertising `len=100` but only 50 payload bytes → `Err(Truncated { needed: 100, got: 50 })`. |
| `reader_handles_bad_direction_byte` | `direction = 0xFF` → `Err(BadDirection(0xFF))`. |
| `reader_handles_bad_transport_byte` | `transport = 0xFF` → `Err(BadTransport(0xFF))`. |

### Drop-on-saturation

| Test | Asserts |
|---|---|
| `writer_drops_when_channel_full` | Open a writer with a deliberately small channel (for example, capacity 1, exposed via a test-only constructor `CaptureWriter::open_with_capacity`); push 1 000 records as quickly as possible; assert `dropped_count() > 0` and the writer task did not panic. |
| `writer_record_is_non_blocking` | Spawn a slow writer (block file writes via a stub or injected `tokio::time::sleep`); assert `record()` returns within 1 ms even when many records are pending. |

### Capture-off zero overhead

| Test | Asserts |
|---|---|
| `udp_channel_config_default_capture_is_none` | `UdpChannelConfig::default().capture.is_none()` (the same for TCP). Compile-time invariant; the runtime hot path is `if let Some(cap) = &self.config.capture { … }`, which optimizes to a single null check. |

(A microbench asserting equivalent throughput is mentioned in the
stub but skipped here as gold-plating; the compile-time invariant
is the contract that matters. Add a benchmark later if profiling
shows the null check itself is a problem.)

### Channel tap (in `tests/udp.rs` / `tests/tcp.rs`)

| Test | Asserts |
|---|---|
| `udp_channel_with_capture_records_inbound_packets` | Set up a UdpChannel with `capture: Some(writer)`; exercise convergence and push 3 inbound replies; close the writer; reader sees 3 inbound records (plus possibly hello-loop replies; assert at least 3). |
| `udp_channel_with_capture_records_outbound_player_state` | After convergence, call `send_player_state` 5 times; reader sees outbound records with the proto-only payload (no `[1]` envelope byte). |
| `tcp_channel_with_capture_records_inbound_packets` | Similar for the TCP recv side. |
| `tcp_channel_with_capture_records_outbound_packets_with_hello_flag` | `send_packet(payload, hello=true)` → captured record has `hello=true` and payload is proto-only (no `[2, 0]` envelope). Same for `hello=false`. |

### CLI parser tests (in `tests/cli_args.rs`)

| Test | Asserts |
|---|---|
| `start_with_capture_flag_captures_path` | `parse(["ranchero", "start", "--capture", "/tmp/x.cap"])` → `cli.global.capture == Some("/tmp/x.cap")`. |
| `parses_replay_subcommand` | `parse(["ranchero", "replay", "/tmp/x.cap"])` → `cli.command == Replay { path, verbose: false }`. |
| `parses_replay_with_verbose` | `--verbose` flag captured. |
| `dispatch_replay_stub` | Stub run() output contains `"replay"`. |

## Open verification points

1. **Timestamp source.** The plan uses Unix-epoch nanoseconds
   (absolute, reproducible across captures, easy to compare).
   Alternatives considered:
   - `WorldTimer::now()` (Zwift epoch ms): protocol-aligned but only
     exists if a `WorldTimer` is in scope; capture must work
     without one. Rejected.
   - Monotonic since capture-start: ordering-correct, smaller, but
     cannot be aligned across captures. Rejected.

2. **Channel-id disambiguation in `flags`.** Bit 0 of `flags` is
   the hello flag. Bits 1–7 are reserved. If a future
   multi-UDP-channel daemon needs to disambiguate per-channel
   captures, two paths exist:
   - Use bits 1–4 as a 4-bit channel id (maximum 16 channels per
     transport per file).
   - Increment the format version and add a `channel_id: u8` field
     to the record header.

   No decision needed today; v1 single-UDP-channel is the steady
   state.

3. **Drop policy: oldest versus newest on channel-full.** Plan:
   drop the newest (the record being added). Rationale: this
   prevents the writer from having to coordinate with
   already-queued records and preserves ordering for whatever does
   get written. Sauce-style "drop oldest" would let the most
   recent state survive at the cost of gaps in the timeline that
   confuse parity tests. Newest-drop with a counter is explicit
   about what was lost.

4. **`flush_and_close` ordering at shutdown.** Plan: the
   supervisor (STEP 12) calls it after channels have shut down.
   Open question for implementation: does the supervisor wait for
   in-flight record sends from the channel hot path to complete
   before closing the writer? Tests should pin this: closing while
   a record is mid-flight should not lose it (or should clearly
   drop and count it).

5. **Per-record `fsync`.** None today (closed in `flush_and_close`
   only). If a real-world session crashes mid-stream, the most
   recent few records are lost. Acceptable for the fixture use
   case. Add a `--capture-sync` flag if a use case appears.

6. **Bytes versus Vec<u8> in `CaptureRecord::payload`.** The plan
   uses `Vec<u8>`. The channel hot path already has the bytes on
   the heap (from `proto_bytes = cts.encode_to_vec()`), so cloning
   into the record is one heap copy. If profiling shows this is a
   hotspot, switch to `bytes::Bytes` end-to-end.

## Design decisions worth pre-committing

- **Plaintext capture only.** `--capture-raw` is deferred. This
  reduces attack surface (no AES key persistence) and matches the
  use case STEPS 18 and 19 require.
- **Single file, append-only, no rotation.** One `start`
  invocation = one file.
- **`record()` is synchronous and non-blocking.** Hot-path safety
  is the reason this entire design exists.
- **Drop on saturation, count metric, never backpressure the
  channel.** The plan sketch's strongest invariant. Tests pin it.
- **Synchronous `CaptureReader` Iterator.** Replay is read-once,
  iterate; async would complicate the common case for no benefit.
- **Capture is opt-in; default `None`.** Tests for capture-off
  channels remain unchanged from STEPs 10 and 11.
- **Refactor `process_inbound{,_packet}` to return `Vec<u8>`
  plaintext rather than the parsed `ServerToClient`.** The recv
  loop calls prost decode itself; the tap fires on the plaintext.
  This makes the tap insertion clean and (incidentally) is the
  right move for the STEP 20 §20.2 "shared inbound-decode helper"
  parking-lot item, should that be taken on later.

## What this unblocks / updates downstream

- **STEP 08.** No change needed today: STEP 08's known-answer
  vectors are generated by a Node script (`gen_vectors.mjs`) and
  serve their purpose. If `--capture-raw` is ever implemented,
  STEP 08 could regenerate vectors from real Zwift traffic for the
  `aes_gcm4_decrypt_*_known_vector` tests, but the current Node
  oracle is sufficient.
- **STEP 18 (formatter parity).** Replace "feed a recorded JS
  trace" with "open `tests/fixtures/<name>.cap` via
  `CaptureReader`, iterate, decode, feed the formatter."
- **STEP 19 (compatibility tests).** The same: `compat/fixtures/*.cap`
  are ranchero-generated, with no JS dependency. Update the
  as-built document to reflect the new fixture-generation recipe
  (for example, "run `ranchero start --capture
  compat/fixtures/<name>.cap` for approximately 30 s during a live
  ride").

## Wiring into the workspace

- `crates/zwift-relay/`:
  - `src/capture.rs` (new, ~250 lines).
  - `src/udp.rs` and `src/tcp.rs` add `Option<Arc<CaptureWriter>>`
    to their config structs and call `cap.record(...)` at the four
    tap points.
  - `tests/capture.rs` (new). Channel-tap tests extend the existing
    `tests/udp.rs` / `tests/tcp.rs`.
  - `Cargo.toml`: verify tokio `fs` feature is enabled (add if not).

- `ranchero` (root crate):
  - `src/cli.rs`: `--capture` flag on `start`; new `Replay`
    variant; dispatch arms.
  - `src/daemon/start.rs` (or wherever the `start` flow resides):
    open `CaptureWriter` if path is set, pass into the supervisor.
  - `tests/cli_args.rs`: parser + dispatch-stub tests for the new
    flag + subcommand.

- License header `// SPDX-License-Identifier: AGPL-3.0-only` at
  the top of every new `.rs` file.
