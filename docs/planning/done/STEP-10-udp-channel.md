# Step 10 — UDP channel + time sync

**Status:** planned (2026-04-27).

## Goal

Establish the secure UDPv4 telemetry channel per spec §4.6 / §4.7:

- Connected UDPv4 socket to the chosen server's `securePort` (3024).
- Hello-loop handshake: up to **25** hello packets with increasing
  delay (10 ms, 20 ms, …, 240 ms). Each carries the full IV (RELAY_ID
  + CONN_ID + SEQNO) and an empty-state `ClientToServer` so the AES /
  IV state machine on both ends synchronizes even under packet loss.
- SNTP-style time sync: for each reply, accumulate a `(latency,
  offset)` sample; once ≥ 5 samples are within stddev of the median
  latency, average their offsets and call
  `WorldTimer::adjust_offset(-mean_offset)`.
- After convergence the channel becomes "active": a background recv-loop
  decrypts inbound `ServerToClient` packets and emits them on a
  broadcast; `send_player_state` is the outbound side.
- Watchdog: 30 s of silence (or arbitrary recv-loop error budget)
  emits a `Timeout` event so the supervisor (STEP 12) can reconnect.

This is the first step that touches a real `tokio::net::UdpSocket`
in the workspace. It owns the IV state mutation rules that STEP 08's
codec deliberately deferred.

## Scope

**In scope**:

- `WorldTimer` — local clock with adjustable offset against the Zwift
  epoch (`1414016074400`, spec §4.3). Lives at the crate root because
  STEP 11's TCP channel will share it.
- `UdpTransport` trait + `TokioUdpTransport` impl: abstracts the
  `tokio::net::UdpSocket::send` / `recv` pair so tests can drive
  byte-level scripts deterministically.
- `time_sync` module: pure-math SNTP filter (sample collection,
  median-by-latency, stddev-based outlier rejection, mean offset).
- `UdpChannel`: the orchestration: hello-loop, recv-loop background
  task, send path, IV state mutation, watchdog, shutdown.
- IV state ownership for the UDP send/recv path (the contract STEP 08
  planned around).

**Out of scope** (deferred to later steps):

| Concern | Where it lives |
|---|---|
| TCP channel | STEP 11 |
| Per-course UDP server pool selection (`UdpConfigVod` driven swaps) | STEP 12 (`GameMonitor`) |
| Reconnect / backoff supervision when `Timeout` fires | STEP 12 |
| `_lastUDPServer` "stick to the same direct server when possible" | STEP 12 |
| Companion-app UDP / tertiary channels | Out of scope for v1 (spec §6) |
| Player-state encoding (`encodePlayerStateFlags1/2`, cadence clamp, and similar) | STEP 13+ (`zwift-stats`); UDP sends whatever `PlayerState` it is given |

## Crate layout

`zwift-relay` already exists with codec + session modules. STEP 10
adds two siblings:

```
crates/zwift-relay/
├── Cargo.toml
├── src/
│   ├── lib.rs           — re-exports (codec + session + udp + world_timer)
│   ├── consts.rs        (existing — extended for UDP_PORT_SECURE, CHANNEL_TIMEOUT, MAX_HELLOS, MIN_SYNC_SAMPLES, ZWIFT_EPOCH_MS)
│   ├── crypto.rs        (existing)
│   ├── frame.rs         (existing)
│   ├── header.rs        (existing)
│   ├── iv.rs            (existing)
│   ├── session.rs       (existing)
│   ├── world_timer.rs   ← NEW
│   └── udp.rs           ← NEW (channel + transport trait + private time_sync helpers)
└── tests/
    ├── world_timer.rs   ← NEW
    ├── time_sync.rs     ← NEW (tests against `udp::sync` made `pub(crate)` for tests, or extracted to its own pub module)
    └── udp.rs           ← NEW (mock-transport-driven channel tests)
```

If `udp.rs` exceeds approximately 500 lines, split into `udp/{mod, channel,
transport, sync}.rs`. The plan starts with the single-file form.

## Dependencies

`crates/zwift-relay/Cargo.toml` gains the `net` feature on tokio.
There are no new direct dependencies:

```toml
[dependencies]
# (existing — codec)
aes      = "..."
bitflags = "..."
ghash    = "..."
subtle   = "..."
thiserror = "..."

# (existing — session)
prost    = "0.13"
rand     = "0.8"
reqwest  = "0.12"
tokio    = { version = "1", features = ["sync", "time", "rt", "macros", "net"] }   # ← `net` is new
tracing  = "0.1"
zwift-api    = { path = "../zwift-api" }
zwift-proto  = { path = "../zwift-proto" }
```

`reqwest` is already in for STEP 09; nothing new at the dev-dep
level.

## Public API surface (proposed)

### `WorldTimer` (new, `world_timer.rs`)

```rust
/// Local clock aligned to Zwift's "world time" epoch
/// (`1414016074400` ms ≈ 2014-10-22 UTC, spec §4.3). Offset is
/// adjusted by the UDP channel's SNTP-style sync (and, optionally,
/// by an initial coarse correction at relay-login time per
/// `zwift.mjs:1644-1648`).
///
/// Cloneable handle pattern: the `WorldTimer` struct holds the
/// adjustable state behind an `Arc<Mutex<…>>`; clones share the same
/// underlying state. Inexpensive to pass to multiple channels.
#[derive(Clone)]
pub struct WorldTimer { /* private */ }

impl WorldTimer {
    pub fn new() -> Self;
    /// `Date.now() + offset - epoch` (ms). What the protocol's
    /// `worldTime` fields use.
    pub fn now(&self) -> i64;
    /// `Date.now() + offset` (ms since Unix epoch). Used for log
    /// timestamps that should reflect the corrected wall clock.
    pub fn server_now(&self) -> i64;
    /// Shift the offset by `diff_ms` (positive = clock advances).
    /// Logged at WARN if `|diff_ms| > 5000`.
    pub fn adjust_offset(&self, diff_ms: i64);
    /// Current offset for tests / observability.
    pub fn offset_ms(&self) -> i64;
}
```

### `UdpTransport` trait (new, in `udp`)

```rust
/// Async send/recv pair for a connected UDPv4 socket. Implemented by
/// `TokioUdpTransport` for production and by tests' mock transports.
/// `async fn` in trait is stable since Rust 1.75; channel uses
/// generics, not `dyn`, so no `async-trait` crate is required.
pub trait UdpTransport: Send + Sync + 'static {
    async fn send(&self, bytes: &[u8]) -> std::io::Result<()>;
    async fn recv(&self) -> std::io::Result<Vec<u8>>;
}

pub struct TokioUdpTransport { /* tokio::net::UdpSocket */ }
impl TokioUdpTransport {
    /// Bind ephemeral local port and `connect()` to `(server_ip,
    /// server_port)`. Subsequent `send` / `recv` use the connected
    /// peer; OS drops mismatched-source datagrams.
    pub async fn connect(server: std::net::SocketAddr) -> std::io::Result<Self>;
}
impl UdpTransport for TokioUdpTransport { /* … */ }
```

### `UdpChannel` (new, in `udp`)

```rust
pub struct UdpChannelConfig {
    pub course_id: i32,
    pub athlete_id: i64,
    /// Hard cap on hello attempts before declaring sync failure.
    /// Production default `25` (sauce).
    pub max_hellos: u32,
    /// Minimum sync samples required before convergence is declared.
    /// Production default `5`.
    pub min_sync_samples: usize,
    /// Watchdog: emit `Timeout` after this much silence on the
    /// inbound side. Production default `30s` (spec §7.4
    /// `CHANNEL_TIMEOUT`).
    pub watchdog_timeout: std::time::Duration,
}

#[derive(Debug, Clone)]
pub enum ChannelEvent {
    /// Sync converged; channel is now "active".
    Established { latency_ms: i64 },
    /// One inbound `ServerToClient` decoded successfully.
    Inbound(zwift_proto::ServerToClient),
    /// Watchdog fired (no inbound packets for `watchdog_timeout`).
    Timeout,
    /// Recv-loop hit a fatal error (decrypt fail, malformed proto,
    /// transport closed). Channel is shutting down.
    RecvError(String),
    /// `shutdown()` was called or the recv-loop exited.
    Shutdown,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("codec: {0}")]
    Codec(#[from] crate::CodecError),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("protobuf decode: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("hello-loop timed out after {attempts} attempts without sync")]
    SyncTimeout { attempts: u32 },

    #[error("inbound relay_id mismatch: expected {expected}, got {got}")]
    BadRelayId { expected: u32, got: u32 },
}

pub struct UdpChannel { /* private */ }

impl UdpChannel {
    /// Run the hello-loop synchronously (against the supplied
    /// transport), then spawn the background recv-loop and watchdog.
    /// Returns `(channel, events)` once sync converges.
    pub async fn establish<T: UdpTransport>(
        transport: T,
        session: &crate::RelaySession,
        clock: WorldTimer,
        config: UdpChannelConfig,
    ) -> Result<(Self, tokio::sync::broadcast::Receiver<ChannelEvent>), Error>;

    /// Send one `ClientToServer` payload (typically a `PlayerState`).
    /// Outbound IV / seqno state is updated atomically.
    pub async fn send_player_state(
        &self,
        state: zwift_proto::PlayerState,
    ) -> Result<(), Error>;

    /// Median latency from the last successful sync, if any.
    pub fn latency_ms(&self) -> Option<i64>;

    /// Subscribe to additional event receivers (e.g. for the
    /// supervisor + a stats consumer).
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ChannelEvent>;

    /// Cancel the recv-loop / watchdog and close the transport.
    pub fn shutdown(&self);
}
```

### Constants extended in `consts.rs`

```rust
pub const ZWIFT_EPOCH_MS: i64 = 1_414_016_074_400;
pub const UDP_PORT_SECURE: u16 = 3024;
pub const UDP_PORT_PLAIN:  u16 = 3022;   // not used by this client
pub const CHANNEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
pub const MAX_HELLOS: u32 = 25;
pub const MIN_SYNC_SAMPLES: usize = 5;
```

## Two seqno spaces (worth pre-committing)

Sauce maintains two independent `seqno` counters per channel. The Rust
port must do the same:

| Counter | Where it lives | What it counts |
|---|---|---|
| `iv.seqno` (u32) | `RelayIv`, channel's send/recv state | Bytes-on-the-wire packet counter, embedded in the AES-GCM IV; goes into `Header::seqno` when the SEQNO flag is set |
| `app_seqno` (u32) | `ClientToServer.seqno` proto field | Application-level packet counter, scoped to the channel; the server echoes it back as `ServerToClient.ack_seqno` so the client can pair hellos to their replies for latency measurement |

`UdpChannel` owns both. The hello-loop's `(seqno → send_time)` map
keys on `app_seqno`, **not** `iv.seqno`. Both increment by 1 per
outbound packet.

## SNTP-style time sync (the math)

Translated from `zwift.mjs:1342-1377`:

For each inbound reply:

```
local_world_time = clock.now()
sent_at          = sync_stamps[reply.ack_seqno]    // discarded if absent
latency_ms       = (local_world_time - sent_at) / 2
offset_ms        = local_world_time - (reply.world_time + latency_ms)
samples.push((latency_ms, offset_ms))
```

Once `samples.len() > min_sync_samples` (sauce uses `> 5`, i.e. ≥ 6):

```
sort by latency
mean_latency  = sum(latency) / n
variance_each = (mean_latency - latency)²
stddev        = sqrt(sum(variance_each) / n)
median_latency = samples[n / 2].latency        // floor-indexed median
valid          = samples.filter(|s| |s.latency - median_latency| < stddev)
if valid.len() > 4:
  mean_offset = sum(valid.offset) / valid.len()
  clock.adjust_offset(-mean_offset)
  emit Established { latency: median_latency }
  return Done
```

Sauce's threshold is **strictly greater than 5** before *attempting*
to filter, and **strictly greater than 4** for accepting it. The plan
preserves both bounds as `> 5` and `> 4` literals (or as `>=
min_sync_samples + 1` / `>= 5` if named constants are preferred; see
"Open verification points" §1).

## Tests-first plan

All tests under `crates/zwift-relay/tests/`. None require a real
network socket; the channel tests use a `MockUdpTransport` driven by
a pair of `tokio::sync::mpsc` channels (one for "what the test
wants the transport to receive next" → `transport.recv()`, and one for
"what the channel sent that the test should see" ← `transport.send()`).

### `world_timer.rs`

| Test | Asserts |
|---|---|
| `world_timer_now_subtracts_epoch` | At zero offset, `now()` ≈ `Date.now() - 1414016074400`. |
| `world_timer_adjust_offset_shifts_now_by_diff` | After `adjust_offset(+1000)`, `now()` = previous_now + 1000 (within a millisecond of jitter). |
| `world_timer_clones_share_state` | Adjusting offset on one clone is visible from another. |
| `world_timer_offset_ms_round_trip` | `offset_ms()` returns the cumulative diff over multiple `adjust_offset` calls. |

### `time_sync.rs` (testing the SNTP filter directly)

The filter is exposed as a free function `udp::sync::compute_offset(samples: &[Sample]) -> SyncOutcome` so tests can hit it with hand-built inputs without spinning up a channel.

| Test | Asserts |
|---|---|
| `sync_returns_pending_below_threshold` | 5 samples → `SyncOutcome::NeedMore`. |
| `sync_picks_median_by_latency` | 6 samples with known latencies; computed `median_latency` equals the floor-indexed middle by sorted latency. |
| `sync_filters_outliers_outside_one_stddev` | 5 tight samples (latency 10±1 ms, offset +5) + 1 absurd (latency 500 ms, offset +500). The absurd sample is discarded; mean_offset stays near +5. |
| `sync_returns_pending_when_too_few_valid` | If outlier filtering leaves ≤ 4 valid samples, return `SyncOutcome::NeedMore` (not `Converged`). |
| `sync_known_vector` | A canned `[(latency, offset)]` set produces a specific `mean_offset` and `median_latency`, both hand-computed. |

### `udp.rs` (channel against `MockUdpTransport`)

| Test | Asserts |
|---|---|
| `establish_sends_first_hello_with_relay_conn_seqno_flags` | The first packet on the wire decodes as a header with all 3 flags set, then the UDP envelope: `[u8 version=1][proto bytes]`. Plaintext after decrypt is exactly that shape. |
| `establish_sends_subsequent_hellos_with_seqno_only` | After the first hello, header flags collapse to SEQNO only (since relay/conn do not change). |
| `establish_sends_payload_athlete_id_realm_one_world_time_zero` | The decrypted `ClientToServer` matches `{athleteId, realm: 1, worldTime: 0}`. |
| `establish_max_hellos_then_sync_timeout` | `MockUdpTransport::recv` never returns; `establish()` errors with `SyncTimeout { attempts: 25 }` (or whatever `max_hellos` is). |
| `establish_converges_after_six_replies` | Mock feeds 6 well-formed `ServerToClient` replies with valid `ack_seqno` and tight-but-distinct `world_time`s; `establish()` returns `(channel, events)` and the first event is `Established { latency_ms: ... }` matching the median. |
| `establish_increments_app_seqno_per_hello` | The first hello has `ClientToServer.seqno = 0`, second `= 1`, and so on. |
| `establish_increments_iv_seqno_per_hello` | The first hello header has `IV.seqno = 0`, second `= 1`, and so on (asserted by checking the SEQNO field in successive headers). |
| `recv_loop_emits_inbound_event_per_decoded_packet` | After establish, mock feeds two `ServerToClient` packets; channel emits two `ChannelEvent::Inbound` events with the correct payloads. |
| `recv_rejects_inbound_with_wrong_relay_id` | An inbound packet whose RELAY_ID flag carries a different `relay_id` produces a `RecvError` event (or is silently dropped; see "Open verification points" §3). |
| `recv_loop_decryption_failure_emits_recv_error` | A tampered-tag inbound packet produces `RecvError` containing the underlying `CodecError`. |
| `watchdog_fires_after_silence` | Mock provides initial sync, then stops responding. After `watchdog_timeout` (test uses small value like `200ms`), channel emits `ChannelEvent::Timeout`. |
| `send_player_state_emits_packet_with_seqno_flag_only` | After convergence, `send_player_state(...)` produces a packet whose decrypted plaintext is `[u8 version=1][PlayerState bytes]` and whose header carries SEQNO only. |
| `shutdown_stops_recv_loop_and_emits_shutdown_event` | Calling `shutdown()` causes the recv-loop task to exit and emit `ChannelEvent::Shutdown`. |

### `MockUdpTransport` shape

```rust
pub struct MockUdpTransport {
    inbox:   Mutex<UnboundedReceiver<Vec<u8>>>,  // bytes the test wants channel.recv() to read
    outbox:  UnboundedSender<Vec<u8>>,           // bytes the channel sent
}

impl MockUdpTransport {
    pub fn new() -> (Self, MockHandle);
}

pub struct MockHandle {
    pub script_inbound: UnboundedSender<Vec<u8>>,  // tests push reply bytes here
    pub captured_outbound: UnboundedReceiver<Vec<u8>>,  // tests assert on what was sent
}
```

Lives in a `tests/common/mod.rs` if multiple test files use it;
otherwise inline in `tests/udp.rs`.

## Open verification points

1. **Sauce's `> 5` and `> 4` thresholds vs. named constants.**
   The plan defaults `min_sync_samples = 5` and uses the literal `>
   min_sync_samples` (so 6+ samples trigger the filter). This matches
   sauce. If tests show the filter never converging on 5-tight-sample
   inputs and a more permissive bound is wanted, reconsider; but match
   sauce at this step.

2. **Watchdog on the *send* side?** Sauce's `NetChannel` only
   watchdogs inbound silence (`tickleWatchdog` is called from
   `_onUDPData` only). The plan inherits that. If real-world testing
   shows that outbound failure must also be detected (for example, a stuck
   send queue), add a separate keepalive timer.

3. **Inbound `relay_id` mismatch: drop or fatal?** Sauce throws
   ("Bad Relay ID", `zwift.mjs:1077-1080`), which propagates through
   `incError` and after enough errors triggers reconnect. The plan emits
   it as `RecvError` (recoverable) rather than tearing down the
   channel; the supervisor (STEP 12) decides whether to reconnect.
   Reconsider if legitimate cross-channel mixups are observed.

4. **Initial coarse clock correction at login.** Sauce's
   `GameMonitor.login` (`zwift.mjs:1644-1648`) performs a one-shot
   coarse correction (`adjustOffset(-tDelta)`) if the local clock is
   off by > 60 s, *before* the UDP-driven SNTP sync runs. Where does
   this responsibility belong? Two clean choices:
   - **STEP 09's relay session** populates `RelaySession.server_time_ms`,
     and the supervisor performs the coarse correction before
     `UdpChannel::establish`. (The plan recommends this; STEP 09 already
     routes `server_time_ms` through.)
   - **STEP 10's `UdpChannel::establish`** accepts an
     `Option<server_time_ms>` and performs the coarse step itself.

   Choose during implementation; record in the as-built document.

5. **Hello-loop `Promise.race([sleep(10*i), syncComplete])` vs.
   `tokio::select!`.** Sauce sleeps `10 * i` between hellos but breaks
   early if sync converges. The Rust port uses `tokio::select!` over the
   sleep and a sync-converged notify; functionally equivalent. Verify
   timing is similar (~3 s worst case, much less if sync converges
   on the first 6 packets).

6. **Hello payload field names.** The vendored proto uses snake_case
   for `ClientToServer` fields (`athlete_id`, `world_time`, and similar),
   while sauce uses camelCase. The implementation uses whatever
   prost generates. Confirmed via STEP 06 inspection; no design
   choice here, only a note for spec readers.

## Design decisions worth pre-committing

- **`UdpChannel` is generic over `T: UdpTransport`.** Compile-time
  polymorphism instead of `Box<dyn UdpTransport>`, which avoids the
  `async-trait` crate (now that async fn in traits is stable). Two
  monomorphizations: `UdpChannel<TokioUdpTransport>` for production
  and `UdpChannel<MockUdpTransport>` for tests.
- **Single recv-loop background task.** One spawned task owns
  `transport.recv()` in a loop, decrypt → decode → broadcast. The send
  path is a separate code path, guarded by an internal mutex on the
  IV state. No multi-consumer for the transport.
- **Watchdog co-located with the recv-loop.** The recv-loop wraps its
  `recv` call in `tokio::time::timeout(watchdog_timeout, …)`. On
  timeout, emit `ChannelEvent::Timeout` and loop again (no
  self-shutdown; the supervisor decides). This avoids a second task
  dedicated to the watchdog.
- **`broadcast` channel for events, capacity 64.** Same pattern STEP
  09 used. Multiple consumers (supervisor + future stats processor +
  TUI debug pane) can subscribe without coordinating.
- **`WorldTimer` is `Clone`-as-handle.** Internally
  `Arc<Mutex<State>>`; clones share state. Inexpensive to pass to multiple
  channels and to a stats-processor instance.
- **No reconnect logic in this step.** `UdpChannel` exposes
  `Timeout` / `RecvError` / `Shutdown` events. The supervisor that
  consumes them (STEP 12) is responsible for re-establishing.

## Wiring into the workspace

- `crates/zwift-relay/Cargo.toml` adds the `net` feature to tokio.
- No new direct dependencies; no new crate.
- The root `ranchero` crate does not yet depend on UDP-channel
  surface; that comes at STEP 12 when the daemon orchestrates
  session + channels + stats.
- License header `// SPDX-License-Identifier: AGPL-3.0-only` at the
  top of every new `.rs` file (matches the rest of the workspace).
