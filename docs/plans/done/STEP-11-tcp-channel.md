# Step 11 — TCP channel

**Status:** planned (2026-04-27).

## Goal

Secure TCP/3025 channel per spec §4.7:

- Connect to the chosen relay server's port 3025 with a 31 s
  connect timeout. **Do not** enable `setKeepAlive` (spec §7.12
  footgun: Node-specific bug; Tokio doesn't enable keepalive by
  default, so the corresponding Rust requirement is to simply not
  call `set_keepalive(true)`).
- After connection, the *supervisor* (STEP 12) sends a hello
  `ClientToServer` whose payload it constructs (carries
  `largestWorldAttributeTimestamp` and other supervisor-tracked
  fields). The channel exposes a `send_packet` API that flips the
  hello / steady-state header + envelope shape based on a flag
  the caller passes.
- Stream-based recv loop: drain the TCP socket into an accumulator
  buffer, drive STEP 08's `next_tcp_frame` until the buffer is
  empty (or short), decrypt + decode each complete frame.
- Watchdog: 30 s of inbound silence emits a `Timeout` event so the
  supervisor can decide to reconnect.

This is the last channel-layer step before STEP 12 wires session +
TCP + UDP into a `GameMonitor` supervisor.

## Scope

**In scope** (channel layer only — transport, IV state, framing,
recv loop, watchdog, shutdown):

- `TcpTransport` trait + `TokioTcpTransport` impl (mirrors STEP 10's
  UDP transport pattern).
- `TcpChannel<T>` generic over `T: TcpTransport`.
- `TcpChannel::establish` — opens TCP, spawns recv loop, returns
  once the channel is ready for traffic.
- `TcpChannel::send_packet(payload, hello)` — caller-controlled
  hello vs steady-state. Hello flips the header flags
  (`RELAY_ID|CONN_ID|SEQNO` vs `SEQNO`) and the plaintext envelope
  byte (`[2,0,...]` vs `[2,1,...]`).
- Recv loop with stream accumulator + `next_tcp_frame` demuxer.
- Watchdog around the read side.
- Shutdown via `Notify` (same pattern STEP 10 used).
- IV state mutation per inbound and outbound packet.

**Out of scope** (lives in STEP 12's `GameMonitor` supervisor):

| Concern | Why elsewhere |
|---|---|
| Reconnect with `1000 * 1.2^n` backoff | Channel-supervisor split: channel just emits `Shutdown`; supervisor decides retry policy (`zwift.mjs:1876-1883`) |
| Preferring the previously-used server IP on reconnect | Server-pool concern (`zwift.mjs:1815-1827`); supervisor maintains `_lastTCPServer`-style state |
| Constructing the hello payload (`largestWorldAttributeTimestamp`, etc.) | Supervisor tracks the high-water mark across reconnects |
| 1 Hz `ClientToServer` heartbeat | (a) supervisor-driven, (b) **UDP only**, not TCP — sauce's `broadcastPlayerState` iterates `this._udpChannels` (`zwift.mjs:1948`); the original STEP 11 stub mis-listed this as TCP work |
| Channel state machine (`Closed → Connecting → Active → Closed`) | Spec §7.7 names this as a shared state machine, but in practice it's a supervisor concern — the channel itself just exists or doesn't, and emits events on transitions |

## Crate layout

`zwift-relay` already has codec + session + UDP. STEP 11 adds one
sibling file:

```
crates/zwift-relay/
├── Cargo.toml          (no edits — tokio `net` already enabled in STEP 10)
├── src/
│   ├── …               (existing: consts, codec, session, world_timer, udp)
│   └── tcp.rs          ← NEW (channel + transport trait, no submodules)
└── tests/
    └── tcp.rs          ← NEW (mock-transport-driven channel tests)
```

If `tcp.rs` and `udp.rs` start growing in parallel and develop
shared private helpers, fold both into `channel/{mod,udp,tcp}.rs`.
Plan starts with the flat layout; the two channels intentionally
share no state at runtime, so a shared module isn't load-bearing.

## Dependencies

No new direct deps. `tokio` already has the `net` feature from
STEP 10. `prost`, `zwift-proto`, and the codec primitives are all
already in.

Dev-deps unchanged.

## Public API surface (proposed)

### `TcpTransport` trait (new, in `tcp`)

Mirrors `UdpTransport`'s shape but for stream-oriented I/O:

```rust
/// Stream-oriented transport. Implemented by `TokioTcpTransport`
/// (production) and tests' mock. `async fn` in trait is stable
/// since Rust 1.75; the channel uses generics, not `dyn`.
pub trait TcpTransport: Send + Sync + 'static {
    fn write_all(&self, bytes: &[u8])
        -> impl std::future::Future<Output = std::io::Result<()>> + Send;

    /// Read whatever the OS has available right now. May return a
    /// partial frame, multiple frames, or anything in between. The
    /// recv loop accumulates and drives `next_tcp_frame` to slice
    /// out complete frames.
    fn read_chunk(&self)
        -> impl std::future::Future<Output = std::io::Result<Vec<u8>>> + Send;
}

pub struct TokioTcpTransport { /* tokio::net::TcpStream */ }
impl TokioTcpTransport {
    /// Connect with `connect_timeout`. Wraps `TcpStream::connect`
    /// in `tokio::time::timeout`. **Does not** call
    /// `set_keepalive(true)` — see "Open verification points" §4.
    pub async fn connect(
        addr: std::net::SocketAddr,
        connect_timeout: std::time::Duration,
    ) -> std::io::Result<Self>;
}
```

A note on `read_chunk`'s shape: it returns `Vec<u8>` rather than
filling a caller-supplied buffer. This makes the mock transport
trivially scriptable (push pre-built `Vec<u8>` chunks of arbitrary
sizes into an mpsc) at the cost of an alloc per read. For STEP 11
the simpler signature wins; if profile data later shows TCP recv as
a hotspot, switch to `read_into(&mut Vec<u8>) -> usize`.

### `TcpChannel` (new, in `tcp`)

```rust
pub struct TcpChannelConfig {
    pub athlete_id: i64,
    pub conn_id: u16,
    pub watchdog_timeout: std::time::Duration,   // default `CHANNEL_TIMEOUT` (30 s)
}

impl Default for TcpChannelConfig { /* … */ }

#[derive(Debug, Clone)]
pub enum TcpChannelEvent {
    /// Recv loop is running and ready to deliver inbound packets.
    /// First event after `establish` returns. Emitted from the
    /// spawned recv task so subscribers attached after `establish`
    /// still see it (same trick STEPs 09 / 10 used).
    Established,
    /// One inbound `ServerToClient` decoded successfully.
    Inbound(zwift_proto::ServerToClient),
    /// Inbound silence for `watchdog_timeout`.
    Timeout,
    /// Recoverable per-packet error (decrypt fail, malformed proto,
    /// `BadRelayId`). Channel keeps running; supervisor decides
    /// whether the error count justifies a reconnect.
    RecvError(String),
    /// Transport closed (peer disconnected, write failed) or
    /// `shutdown()` was called. Recv loop has exited.
    Shutdown,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("codec: {0}")]
    Codec(#[from] CodecError),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("protobuf decode: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("inbound relay_id mismatch: expected {expected}, got {got}")]
    BadRelayId { expected: u32, got: u32 },
}

pub struct TcpChannel<T: TcpTransport> { /* private */ }

impl<T: TcpTransport> TcpChannel<T> {
    /// Spawn the recv loop and return. Does NOT send a hello packet
    /// — the supervisor sends that as the first
    /// `send_packet(.., hello: true)` call so it can carry
    /// supervisor-tracked fields like
    /// `largestWorldAttributeTimestamp`.
    pub async fn establish(
        transport: T,
        session: &crate::RelaySession,
        config: TcpChannelConfig,
    ) -> Result<(Self, tokio::sync::broadcast::Receiver<TcpChannelEvent>), Error>;

    /// Send one `ClientToServer` payload. `hello` controls:
    /// - **header flags**: `RELAY_ID | CONN_ID | SEQNO` vs `SEQNO` only
    /// - **plaintext envelope hello byte**: `[2, 0, …]` vs `[2, 1, …]`
    pub async fn send_packet(
        &self,
        payload: zwift_proto::ClientToServer,
        hello: bool,
    ) -> Result<(), Error>;

    /// Subscribe an additional event consumer.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<TcpChannelEvent>;

    /// Cancel the recv loop / watchdog and emit `Shutdown`.
    pub fn shutdown(&self);
}
```

## Stream framing strategy

The recv loop reuses STEP 08's `next_tcp_frame(&[u8]) ->
Result<Option<(&[u8], usize)>, CodecError>`. The state the channel
adds is just an accumulator buffer:

```rust
let mut buffer: Vec<u8> = Vec::new();
loop {
    tokio::select! {
        biased;
        _ = shutdown.notified() => {
            let _ = events_tx.send(TcpChannelEvent::Shutdown);
            return;
        }
        result = tokio::time::timeout(watchdog, transport.read_chunk()) => {
            match result {
                Ok(Ok(chunk)) => buffer.extend_from_slice(&chunk),
                Ok(Err(io)) => { events_tx.send(RecvError(io)); return; }
                Err(_)      => { events_tx.send(Timeout); continue; }
            }
        }
    }

    // Drain every complete frame currently in `buffer`.
    loop {
        match next_tcp_frame(&buffer) {
            Ok(Some((payload, consumed))) => {
                // Copy the payload before draining: the slice is
                // borrowed from `buffer`, and `drain` invalidates it.
                let payload = payload.to_vec();
                buffer.drain(..consumed);
                match process_inbound_tcp(&payload, &aes_key, expected_relay_id, &mut recv_iv_conn_id, &mut recv_iv_seqno) {
                    Ok(stc) => events_tx.send(Inbound(stc)),
                    Err(e)  => events_tx.send(RecvError(e.to_string())),
                };
            }
            Ok(None) => break,                                 // need more bytes
            Err(e)   => { events_tx.send(RecvError(e.to_string())); break; }
        }
    }
}
```

`process_inbound_tcp` is structurally the same as STEP 10's
`process_inbound_packet`, but the TCP recv plaintext is wrapped in a
`[2, hello?, proto…]` envelope (STEP 08's `parse_tcp_plaintext`),
whereas UDP recv plaintext is just the raw proto bytes. Worth
extracting a shared private helper for the header-decode / IV-update
/ decrypt steps and parametrizing on the envelope handler. Keeps
the IV rules in one place.

## Send path

```rust
pub async fn send_packet(&self, payload: ClientToServer, hello: bool) -> Result<(), Error> {
    let (header_bytes, ciphertext) = {
        let mut send = self.send_state.lock().expect("send_state mutex");

        let proto_bytes = payload.encode_to_vec();
        let plaintext = tcp_plaintext(&proto_bytes, hello);   // [2, hello?0:1, proto…]

        let header = if hello {
            Header {
                flags: HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
                relay_id: Some(self.relay_id),
                conn_id: Some(self.conn_id),
                seqno: Some(send.iv_seqno),
            }
        } else {
            Header {
                flags: HeaderFlags::SEQNO,
                relay_id: None, conn_id: None, seqno: Some(send.iv_seqno),
            }
        };
        let header_bytes = header.encode();
        let iv = RelayIv {
            device: DeviceType::Relay,
            channel: ChannelType::TcpClient,
            conn_id: self.conn_id,
            seqno: send.iv_seqno,
        };
        let ciphertext = encrypt(&self.aes_key, &iv.to_bytes(), &header_bytes, &plaintext);

        send.iv_seqno = send.iv_seqno.wrapping_add(1);
        // app_seqno: see "Open verification points" §2.
        (header_bytes, ciphertext)
    };

    let wire = frame_tcp(&header_bytes, &ciphertext);          // [BE u16 size][header][cipher||tag4]
    self.transport.write_all(&wire).await?;
    Ok(())
}
```

## Tests-first plan

All tests in `crates/zwift-relay/tests/tcp.rs`. Mock transport
defined inline (mpsc-driven, like UDP's). Same `MockHandle` shape.

### Hello / steady-state send shape

| Test | Asserts |
|---|---|
| `send_packet_hello_carries_full_iv_flags_and_hello_byte_zero` | `send_packet(payload, hello=true)` produces a wire packet whose header has `RELAY_ID | CONN_ID | SEQNO` set and whose decrypted plaintext starts with `[2, 0, …]`. |
| `send_packet_steady_carries_seqno_only_and_hello_byte_one` | Same with `hello=false`: header has SEQNO only; plaintext starts with `[2, 1, …]`. |
| `send_packet_increments_iv_seqno` | Two consecutive sends produce headers with seqno `0` then `1`. |
| `send_packet_prepends_be_u16_size_prefix` | Wire bytes start with a big-endian `u16` whose value equals `header.len() + ciphertext.len()`. |

### Recv-side framing

| Test | Asserts |
|---|---|
| `recv_decodes_complete_frame` | Mock pushes one well-formed frame; channel emits `Inbound(stc)`. |
| `recv_handles_two_frames_in_one_chunk` | Mock pushes two concatenated frames in a single `read_chunk` response; channel emits two `Inbound` events. |
| `recv_handles_frame_split_across_two_chunks` | Mock pushes the first half of a frame, then (on next `read_chunk`) the second half. Channel buffers across reads and emits `Inbound` once. |
| `recv_handles_size_prefix_split_between_chunks` | Edge case: 1 byte of the BE u16 size in one chunk, the other byte + payload in the next. (`next_tcp_frame` returns `Ok(None)` on a 1-byte input — channel should not error.) |
| `recv_emits_recv_error_on_decryption_failure` | Push a frame whose tag has been flipped → `RecvError` event; channel keeps running. |
| `recv_emits_recv_error_on_bad_relay_id` | Push a frame whose RELAY_ID flag carries a different `relay_id` → `RecvError`. |

### Lifecycle

| Test | Asserts |
|---|---|
| `establish_emits_established_event` | First event after `establish` is `Established`. |
| `watchdog_fires_after_silence` | After `watchdog_timeout` of no `read_chunk` data, channel emits `Timeout` (does not shut down). |
| `recv_loop_io_error_emits_recv_error_then_shutdown` | Mock closes its inbound channel (simulating peer disconnect); channel emits `RecvError` then `Shutdown` and the recv task exits. |
| `shutdown_stops_recv_loop_and_emits_shutdown_event` | `channel.shutdown()` causes the recv loop to exit and emit `Shutdown`. |

### Compile-time wiring

| Test | Asserts |
|---|---|
| `tcp_channel_event_is_clone_for_broadcast` | `TcpChannelEvent: Clone`. |

## Open verification points

These are claims the implementor should confirm and record in the
as-built doc.

1. **`hello: bool` vs `enum HelloKind`.** The plan uses a bool
   parameter on `send_packet`. Same deliberation as STEP 08
   `tcp_plaintext`'s `hello: bool` (left as bool there). If
   reviewers find the call site confusing (`send_packet(payload,
   true)` is opaque), swap to `enum HelloKind { Hello, Steady }`.
   Easy to flip.

2. **`app_seqno` ownership for TCP.** Sauce's
   `NetChannel.makeDataPBAndBuffer` (`zwift.mjs:1192-1197`)
   auto-increments `_sendSeqno` per outbound packet, shared between
   TCP and UDP. STEP 10 already adopted this for UDP (channel owns
   `app_seqno` and writes it into `payload.seqno`). For TCP we have
   two options:

   - **Match sauce / mirror UDP** — channel owns `app_seqno`,
     overrides `payload.seqno` on every send. Caller passes a
     payload with `seqno: None`.
   - **Caller-owns** — supervisor sets `payload.seqno` directly;
     channel doesn't touch it.

   Recommend matching sauce (channel-owns). Caller passes the
   payload sans seqno; channel slots it in. Document in as-built.

3. **Connect-timeout location.** `TokioTcpTransport::connect` takes
   the timeout parameter. The channel's `establish` does not. If a
   future transport (TLS-wrapped, e.g.) needs a different timeout
   shape, move the timeout into `TcpChannelConfig`. For now,
   transport owns it.

4. **`set_keepalive` footgun.** Spec §7.12: "Do not enable TCP
   keepalive." Tokio's `TcpStream` defaults to off. Verify by
   inspection that `TokioTcpTransport::connect` doesn't enable it.
   Add a one-line code comment at the `connect` site noting the
   deliberate non-action so a future "improvement" PR doesn't add
   it back. The 1 Hz `ClientToServer` heartbeat (UDP only,
   supervisor concern) is the application-level liveness signal.

5. **Frame-size sanity ceiling.** Sauce's recv buffer is 65 536
   bytes (`Buffer.alloc(65536)`). A frame larger than the buffer
   would confuse sauce's demuxer; ours can in theory accept larger
   accumulators (`Vec<u8>` grows). The `BE u16` length prefix caps
   each frame at 65 535 bytes. Consider a sanity max in the
   channel (e.g. reject if accumulator grows past 128 KiB without
   a complete frame) to bound memory if the peer sends garbage.
   Optional; add only if integration testing surfaces an issue.

6. **Recv-loop "stop" semantics on `Err(io)` from `read_chunk`.**
   Plan: stop the recv loop on transport-level errors (peer
   closed, broken pipe, etc.) — emits `RecvError` then exits with
   `Shutdown`. The supervisor (STEP 12) treats this as a signal to
   reconnect. Worth confirming we don't want to retry transient
   errors automatically; in practice tokio surfaces transient
   errors as `Pending`, not `Err`, so any `Err` we see is fatal.

## Design decisions worth pre-committing

- **`TcpChannel` is generic over `T: TcpTransport`.** Same
  rationale as STEP 10's UDP channel (avoid `async-trait`).
- **Single recv-loop background task.** Owns `transport.read_chunk()`
  + the accumulator buffer + the demuxer drain. Send path is a
  separate code path guarded by a `Mutex` on the IV state.
- **Watchdog co-located with recv loop.** Wraps `read_chunk` in
  `tokio::time::timeout`. On timeout, emit `Timeout` and continue
  (no self-shutdown — supervisor decides).
- **`Notify`-based clean shutdown.** Same pattern STEP 10 uses:
  `shutdown()` notifies, recv-loop selects on the notify, emits
  `Shutdown` and exits.
- **No reconnect logic in this step.** Channel exposes
  `Timeout` / `RecvError` / `Shutdown` events. Backoff lives
  in STEP 12.
- **Share inbound-decode logic with UDP** by extracting the
  per-packet work (header decode + IV update + decrypt) into a
  private helper that both channels reach. Keeps the IV / header /
  decrypt rules in one place. The plaintext-envelope step differs
  (UDP recv has no envelope, TCP recv has `[2, hello?, proto…]`)
  and stays in each channel.

## Wiring into the workspace

- `crates/zwift-relay/` only edits are `src/tcp.rs` + `tests/tcp.rs`
  (plus a small refactor pulling the shared inbound helper into a
  private module if we go that route).
- No new direct deps; no `Cargo.toml` change.
- The root `ranchero` crate does not yet depend on TCP-channel
  surface — that comes at STEP 12.
- License header `// SPDX-License-Identifier: AGPL-3.0-only` at the
  top of every new `.rs` file.
