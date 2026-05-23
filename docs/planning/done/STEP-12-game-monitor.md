# Step 12 — GameMonitor orchestration: sustainable end-to-end connectivity

**Status:** planned (2026-04-28).

## Goal

The full coordinator that brings up the relay session, owns one
TCP channel and N UDP channels, sends the periodic 1 Hz UDP
heartbeat that keeps Zwift's server-side connection alive, parses
`udpConfigVOD` updates and routes UDP to the appropriate pool by
the watched athlete's `(realm, courseId)` and geographic position,
suspends UDP when the watched athlete is stationary, captures the
stream to a wire-capture file when `--capture` is given, and emits
a structured `tracing` record for every observable channel event.

End state: a `ranchero start` invocation against valid Zwift
credentials runs indefinitely without server-side timeout, every
inbound and outbound packet is observable through the configured
log file (and recorded in the capture file when one is requested),
and `ranchero stop` performs a clean teardown that flushes the
capture writer and shuts down the relay session in order.

## Background — what was missed in earlier framing

When STEP-11.5 was scoped, the user-facing goal was to enable an
end-to-end connectivity proof: start the daemon, capture the live
stream to a file, and confirm that the protocol implementation
works against the real Zwift servers. STEP-11.5 as written
delivered only the mechanism — the writer, the reader, the four
channel taps, and the file format. The plan's "What this
unblocks" section described the deliverable in terms of fixture
generation for STEPS 18 and 19 (formatter parity and compatibility
tests). The connectivity-proof framing was discussed verbally but
did not survive into the written plan.

The work required to actually exercise the capture mechanism
end-to-end was originally deferred to a separate plan document.
That separation was a mistake: the missing pieces — auth bootstrap,
relay-session login wiring, daemon and CLI integration, capture
lifecycle, tracing log emission, and shutdown coordination — are
the foundation on which the rest of the GameMonitor's work
(routing, suspension, watched-athlete switching) builds. They
belong in this plan as the first sub-step. STEP-12 now owns
sustainable end-to-end connectivity from the orchestrator
construction onward.

## Sub-steps and order

| Sub-step | Scope |
|---|---|
| 12.1 | TCP-only foundation. Builds the `RelayRuntime` orchestrator, performs auth and relay-session login, opens a single TCP channel, wires the capture writer and tracing log, and integrates with the daemon and CLI. The TCP-only window is bounded by a server-side timeout (roughly 30 s without a UDP heartbeat); 12.3 closes that gap. |
| 12.3 | UDP channel + 1 Hz heartbeat. Brings up the UDP transport, runs the existing hello-loop / SNTP-style time sync, and adds the heartbeat scheduler that prevents server-side timeout. After 12.3, the connection is sustainable indefinitely. |
| 12.4 | `udpConfigVOD` parsing + `findBestUDPServer`. Builds and updates the per-`(realm, courseId)` pool from inbound TCP messages and selects the right UDP server by the watched athlete's position. Adds per-course UDP reselection. |
| 12.5 | Idle suspension FSM + watched-athlete switching + the `GameEvent` enum that downstream consumers will subscribe to. |

The internal sub-step labelling skips 12.2 to avoid colliding with
the separate plan document `STEP-12.2-follow-command.md`, which is
unrelated to STEP-12's internal structure despite the digit
overlap. The numbering starts at 12.1 and continues 12.3, 12.4,
12.5; this is intentional.

## Scope

In scope:

- The `RelayRuntime` orchestrator: a type that owns the lifetime
  of the relay session, the TCP channel, the UDP channel(s), the
  optional capture writer, and the tracing emission.
- Auth bootstrap: construct `zwift_api::ZwiftAuth` from
  `ResolvedConfig`, perform the OAuth login.
- Relay-session login and refresh supervisor.
- TCP channel establishment and the initial `ClientToServer`
  hello.
- UDP channel establishment using the existing
  `zwift_relay::UdpChannel` (hello-loop and time-sync are
  already implemented in STEP-10).
- 1 Hz UDP heartbeat: a `ClientToServer` carrying the watched
  athlete's `PlayerState`, sent on a fixed cadence so the
  server-side liveness model (spec §7.12) does not time out the
  connection.
- `udpConfigVOD` parsing: each inbound `ServerToClient` is
  inspected for an attached pool update, and a per-`(realm,
  courseId)` pool table is maintained.
- `findBestUDPServer(pool, x, y)`: a port of
  `zwift.mjs:2295-2317` that selects the appropriate UDP server
  by the watched athlete's position, with a `useFirstInBounds`
  short-circuit and a minimum-Euclidean-distance fallback.
- Idle suspension: when the watched athlete shows
  `speed = 0 && cadence = 0 && power = 0` for the configured
  idle window (default approximately 60 s per spec §4.13), the
  UDP channel is shut down. UDP resumes immediately on any
  non-zero motion field.
- Watched-athlete state: an internal `(realm, courseId, x, y)`
  record updated from the inbound `PlayerState` of the watched
  athlete. A change in `(realm, courseId)` triggers a UDP pool
  reselection; a change in `(x, y)` within the same pool may
  trigger a server swap if the new position falls outside the
  current server's bounds.
- `GameEvent` enum emission: a broadcast channel that
  downstream consumers (the web/WS server in STEP 17, the
  per-athlete data model in STEP 14) subscribe to for player
  state, world update, latency, and state-change events.
- Capture wiring on the UDP channel as well as the TCP
  channel; both feed the same `Arc<CaptureWriter>` so that the
  capture file records the complete bidirectional stream.
- Tracing log records on the `ranchero::relay` target prefix
  for every observable channel event.
- A graceful shutdown sequence that cancels outbound
  heartbeats first, then the UDP channel, then the TCP
  channel, then `flush_and_close()` on the capture writer,
  then the relay session supervisor.
- CLI and daemon integration: the existing daemon's
  placeholder `run_daemon` becomes the host for the
  orchestrator; `--capture <path>` opens a `CaptureWriter` and
  passes it through `daemon::start`; the STEP-11.6 Fix-D guard
  is removed.
- Live validation against production Zwift, both as a bounded
  TCP-only smoke at the end of sub-step 12.1 and as a
  sustained multi-minute run at the end of STEP-12.

Out of scope (deferred to later steps):

- Decoding `ServerToClient` into the per-athlete data model.
  STEP 14 owns this.
- Rolling-window statistics (NP, TSS, peak power). STEP 13.
- W-prime balance, segment matching, group detection.
  STEP 15.
- SQLite persistence of athlete history. STEP 16.
- HTTP and WebSocket server compatible with `webserver.mjs`.
  STEP 17.
- v1 / v2 payload formatters for the web surface. STEP 18.
- The full compatibility test battery against captured
  fixtures. STEP 19.

## Architecture overview

The orchestrator lives in `src/daemon/relay.rs`, alongside the
existing daemon run loop. The component map at the end of STEP-12
is:

```
                    ┌────────────────────────────────────────┐
                    │              RelayRuntime              │
                    │  (owns lifecycle, exposes shutdown)    │
                    └──┬─────────────────────────────────────┘
                       │
        ┌──────────────┼──────────────────┬──────────────────┐
        │              │                  │                  │
        ▼              ▼                  ▼                  ▼
   ┌─────────┐   ┌────────────┐    ┌──────────────┐   ┌──────────────┐
   │ Session │   │ TcpChannel │    │  UdpChannel  │   │ CaptureWriter│
   │  (STEP  │   │  (STEP 11) │    │   (STEP 10)  │   │  (STEP 11.5) │
   │   09)   │   └─────┬──────┘    └───┬──────────┘   └──────────────┘
   └─────────┘         │               │
                       │               │
                       ▼               ▼
                  ┌────────────────────────────────┐
                  │  Inbound message dispatcher    │
                  │  - Decodes ServerToClient      │
                  │  - Updates WatchedAthleteState │
                  │  - Updates UdpPoolRouter       │
                  │  - Drives IdleFSM              │
                  │  - Emits GameEvent             │
                  └─────┬────────────────┬─────────┘
                        │                │
                        ▼                ▼
                  ┌──────────────┐  ┌──────────────────────┐
                  │  IdleFSM     │  │  HeartbeatScheduler  │
                  │  Active /    │  │  1 Hz CtS on UDP     │
                  │  Idle /      │  │  Suspends on idle    │
                  │  Suspended   │  │                      │
                  └──────────────┘  └──────────────────────┘
                        │
                        ▼
                  ┌──────────────────────┐
                  │   GameEvent          │
                  │   broadcast::Sender  │
                  └──────────────────────┘
```

Boxes labelled with a step number already exist; the others are
introduced by STEP-12. The dispatcher, the idle FSM, the
heartbeat scheduler, the pool router, and the watched-athlete
state are all owned by `RelayRuntime` and are pure-state /
pure-logic where possible so that they can be unit-tested without
the network.

## Sub-step 12.1 — TCP-only foundation

### What it adds

A `ranchero start` invocation that authenticates against Zwift,
establishes a relay session, opens a TCP channel to a Zwift relay
server, receives `ServerToClient` messages, and logs each arrival
to the configured log file. When `--capture <path>` is passed,
the same byte stream is also written to a wire-capture file
readable by `ranchero replay`.

This sub-step delivers the orchestrator construction and the
TCP-only path. It does not deliver sustained operation: without
the 1 Hz UDP heartbeat (added in 12.3), the Zwift server will
terminate the TCP connection within roughly 30 s of client
silence per the client-driven liveness model documented in spec
§7.12. That bounded window is sufficient to prove that the
protocol stack works against the production Zwift servers.

### Module layout

The orchestrator is owned by the `ranchero` root crate and lives
under the daemon module, alongside the existing run loop, because
its lifetime is bound to the daemon's lifetime.

```
src/daemon/
├── mod.rs              (existing, exports unchanged)
├── control.rs          (existing)
├── pidfile.rs          (existing)
├── probe.rs            (existing)
├── runtime.rs          (existing; modified — see below)
└── relay.rs            (NEW — the orchestrator)
```

`relay.rs` holds the orchestrator type, its construction logic,
and its shutdown handle. `runtime.rs` is modified to instantiate
the orchestrator, run it alongside the UDS control loop, and
coordinate shutdown.

Sub-steps 12.3, 12.4, and 12.5 extend `relay.rs`; the file may be
split into multiple files as the orchestrator grows.

### Public API surface

```rust
// src/daemon/relay.rs

pub struct RelayRuntime {
    join_handle: tokio::task::JoinHandle<Result<(), RelayRuntimeError>>,
    shutdown:    Arc<tokio::sync::Notify>,
}

#[derive(thiserror::Error, Debug)]
pub enum RelayRuntimeError {
    #[error("missing main account email; configure via `ranchero configure`")]
    MissingEmail,

    #[error("missing main account password; store one via `ranchero configure`")]
    MissingPassword,

    #[error("auth: {0}")]
    Auth(#[from] zwift_api::Error),

    #[error("relay session: {0}")]
    Session(#[from] zwift_relay::SessionError),

    #[error("TCP channel: {0}")]
    TcpChannel(#[from] zwift_relay::TcpError),

    #[error("relay session reported no TCP servers")]
    NoTcpServers,

    #[error("capture writer I/O: {0}")]
    CaptureIo(std::io::Error),
}

impl RelayRuntime {
    /// Build the runtime, perform the auth and relay-session
    /// login synchronously, open the capture writer if a path is
    /// given, then spawn the recv-loop task. Returns once the
    /// TCP channel has emitted `Established` (or once login
    /// fails, whichever is first).
    pub async fn start(
        cfg: &ResolvedConfig,
        capture_path: Option<PathBuf>,
    ) -> Result<Self, RelayRuntimeError>;

    /// Request a graceful shutdown. The recv loop drains, the
    /// channel is closed, and the capture writer is flushed and
    /// closed. Idempotent.
    pub fn shutdown(&self);

    /// Await orchestrator completion. Resolves either when
    /// `shutdown` is called or when the orchestrator exits on
    /// its own (for example, on a fatal recv error or a
    /// server-side TCP timeout).
    pub async fn join(self) -> Result<(), RelayRuntimeError>;
}
```

The signature is forward-compatible with sub-steps 12.3 to 12.5:
those sub-steps add a UDP channel, attach a heartbeat scheduler,
and consume inbound TCP messages to update the UDP pool — none of
which changes the public surface seen by `daemon::runtime`.

### CLI and daemon changes

The STEP-11.6 Fix-D guard is removed. The `Command::Start` arm
of `cli::dispatch` passes `cli.global.capture` through to
`daemon::start`:

```rust
Command::Start => {
    let log_opts = crate::logging::LogOpts {
        verbose: cli.global.verbose,
        debug: cli.global.debug,
    };
    Ok(daemon::start(
        &resolved,
        cli.global.foreground,
        log_opts,
        cli.global.capture.clone(),
    )?)
}
```

`daemon::start`'s signature is extended to accept the capture
path:

```rust
pub fn start(
    cfg: &ResolvedConfig,
    foreground: bool,
    log_opts: LogOpts,
    capture_path: Option<PathBuf>,
) -> Result<ExitCode, DaemonError> {
    runtime::start(cfg, foreground, log_opts, capture_path)
}
```

`run_daemon` is extended to own a `RelayRuntime` alongside the
existing UDS listener. The `select!` loop gains a third
non-shutdown branch (the orchestrator's `join_handle`); on any
of the three shutdown signals (UDS shutdown, `Ctrl-C`, or
`SIGTERM`) the orchestrator's `shutdown()` method is called,
the join handle is awaited with a timeout, and only then does
the function return.

A representative shape:

```rust
let relay = RelayRuntime::start(cfg, capture_path).await?;

loop {
    tokio::select! {
        biased;
        _ = shutdown_rx.recv() => break,
        _ = tokio::signal::ctrl_c() => break,
        _ = sigterm.recv() => break,
        accept = listener.accept() => {
            // existing UDS handling
        }
    }
}

relay.shutdown();
relay.join().await?;
```

### Tests

The orchestrator depends on real network endpoints (HTTPS for
auth, HTTPS for relay session, TCP for the channel). The tests
use the same mock infrastructure already in use by the lower
crates: `wiremock` for the two HTTPS endpoints (already used in
`zwift-api/tests/auth.rs` and `zwift-relay/tests/session.rs`),
and the existing mock `TcpTransport` used in
`zwift-relay/tests/tcp.rs`. Dependency-injection points are
introduced on `RelayRuntime` so that tests can substitute the
transport without going through the kernel.

Unit tests in `src/daemon/relay.rs`:

| Test | Asserts |
|---|---|
| `start_fails_when_email_missing` | A `ResolvedConfig` with `main_email = None` returns `Err(RelayRuntimeError::MissingEmail)`. |
| `start_fails_when_password_missing` | A `ResolvedConfig` with `main_email = Some(...)` and `main_password = None` returns `Err(RelayRuntimeError::MissingPassword)`. |
| `start_calls_auth_login_then_session_login_then_tcp_connect` | Using a stub `AuthLogin`, a stub `SessionLogin`, and a stub `TcpTransportFactory`, the call sequence is observed in order. |
| `start_propagates_auth_error` | A stub auth that returns `Err` propagates the error without attempting the relay-session login. |
| `start_propagates_session_error` | A stub that returns `Err` from session login surfaces it without attempting the TCP connect. |
| `start_returns_after_first_established_event` | A stub TCP transport that emits `Established` immediately allows `start` to return; subsequent `Inbound` events are processed by the spawned task. |
| `inbound_events_emit_debug_tracing_records` | Using `tracing-test` (or an in-memory subscriber), drive a fixture inbound packet and assert the recorded event is at DEBUG with the expected fields. |
| `recv_error_emits_warn_tracing_record` | A stub transport returns a `RecvError`; the orchestrator emits a single WARN event and continues. |
| `shutdown_drains_capture_writer_and_calls_flush_and_close` | A capture writer is opened, three records are pushed via inbound stub events, `shutdown()` is called; the resulting capture file contains exactly three records when read back. |
| `shutdown_is_idempotent` | Two consecutive `shutdown()` calls do not panic and `join()` resolves cleanly. |
| `start_returns_no_tcp_servers_error_when_session_returns_empty_pool` | A stub session whose `tcp_servers` list is empty returns `RelayRuntimeError::NoTcpServers` without attempting a connect. |

CLI test in `tests/cli_args.rs`:

| Test | Asserts |
|---|---|
| `dispatch_start_passes_capture_path_to_daemon` | A stub `daemon::start` recorded by an injection point receives `capture_path = Some("/tmp/x.cap")` when the user invokes `start --capture /tmp/x.cap`. |

The Fix-D test (`dispatch_start_with_capture_errors_until_step12`)
must be deleted as part of this sub-step. Its presence would
block the positive behaviour from being exercised.

Integration tests in `tests/relay_runtime.rs`:

| Test | Asserts |
|---|---|
| `runtime_writes_capture_file_for_inbound_packets` | Stand up `wiremock` for `/auth/realms/zwift/protocol/openid-connect/token` and `/api/users/login`, plus a fake TCP server on a localhost ephemeral port that emits a single encrypted `ServerToClient` frame. Run `RelayRuntime::start` with a capture path, wait for one `Inbound` event to fire, call `shutdown`, then read the capture file with `CaptureReader` and assert one record. |
| `runtime_logs_login_and_established_at_info` | Same setup; assert that `relay.login.ok` and `relay.tcp.established` records are produced. |

The integration tests are feasible because `wiremock` already
covers the HTTPS endpoints and `TokioTcpTransport::connect` will
accept an arbitrary `SocketAddr`. The orchestrator's `start`
accepts a configuration that points at the mocked endpoints.

### Implementation outline

1. Define `RelayRuntime`, `RelayRuntimeError`, the dependency-
   injection traits (`AuthLogin`, `SessionLogin`,
   `TcpTransportFactory`), and their default implementations
   that delegate to `zwift_api::ZwiftAuth`, `zwift_relay::login`,
   and `TokioTcpTransport::connect`.
2. Implement `RelayRuntime::start`. The function reads
   credentials from `ResolvedConfig` (email and the redacted
   password), opens the capture writer if a path is given,
   performs the auth login synchronously, calls
   `RelaySessionSupervisor::start`, picks
   `session.tcp_servers[0]`, constructs the initial
   `ClientToServer` hello (player id from the relay session,
   server realm 1, world-attribute timestamp 0), establishes
   the TCP channel with the capture writer wired into the
   channel configuration, and sends the hello.
3. Spawn the recv-loop task. The task subscribes to
   `TcpChannelEvent`, emits the corresponding tracing events,
   and exits on `Shutdown`.
4. Implement `RelayRuntime::shutdown`: notify the recv loop,
   await the channel via `shutdown_and_wait`, call
   `flush_and_close` on the capture writer, and signal the
   relay session supervisor to stop.
5. Update `daemon::start` and `daemon::runtime::run_daemon` to
   instantiate the runtime and run it alongside the UDS control
   loop.
6. Update `cli.rs`: remove the Fix-D guard; pass the capture
   path to `daemon::start`. Delete the corresponding negative
   test in `tests/cli_args.rs`.
7. Update
   `docs/plans/done/STEP-11.6-capture-consistency-review.md` to
   record that Fix-D has been superseded by sub-step 12.1.

### Live validation at the end of 12.1

Live validation is a manual acceptance step rather than an
automated test. It must be performed before sub-step 12.1 is
considered complete. Because UDP heartbeats are out of scope at
this point, the validation window ends either at the first
server-side timeout or at a manual stop, whichever is first.

1. Configure ranchero with valid Zwift credentials via
   `ranchero configure`. Confirm with `ranchero auth-check`
   that the credential resolution chain reports the expected
   email and a non-zero password length.
2. Start the daemon in the foreground with capture and verbose
   logging:
   ```
   ranchero start --foreground -v --capture /tmp/ranchero-smoke.cap
   ```
3. Confirm the log lines include `relay.login.ok`,
   `relay.tcp.connecting`, and `relay.tcp.established`.
   Subsequent `relay.tcp.inbound` records (at DEBUG, so add
   `-D` if needed) confirm that frames are arriving.
4. Allow the daemon to run until either a server-side timeout
   or a manual stop. Stop it with `ranchero stop` (from a
   second terminal) or `Ctrl-C`. The expected behaviour is
   that the server closes the connection after roughly 30 s of
   silence; this is acceptable at this sub-step and confirms
   the need for the heartbeat work in 12.3.
5. Confirm `relay.tcp.shutdown` and `relay.capture.closed`
   records are present in the log file.
6. Run `ranchero replay /tmp/ranchero-smoke.cap` and confirm a
   non-zero record count. Run with `--verbose` to confirm the
   per-record summary.

If the smoke fails, the most likely failure modes and their
implications are:

- A 401 from
  `/auth/realms/zwift/protocol/openid-connect/token` indicates
  a credentials mismatch or an OAuth body-format defect in
  `zwift-api`. Re-check the form encoding against the current
  Zwift OAuth contract.
- A 4xx from `/api/users/login` indicates a relay-session
  request defect in `zwift-relay::session`. Re-check the
  protobuf encoding and the headers (`Source`, `User-Agent`).
- An immediate TCP disconnect after the hello indicates
  either an incorrect AES key derivation, an incorrect hello
  payload, or a server-side rate limit. Re-check `RelayIv`
  construction and the `ClientToServer` hello field set
  against `docs/ARCHITECTURE-AND-RUST-SPEC.md` §4.4.
- A long silence followed by a `Timeout` event after roughly
  30 s indicates that the server expects a 1 Hz UDP
  heartbeat. This is the expected behaviour for a TCP-only
  run and confirms the need for sub-step 12.3.

## Sub-step 12.3 — UDP channel and 1 Hz heartbeat

### What it adds

- An owned `UdpChannel` inside `RelayRuntime`.
- A `HeartbeatScheduler` that sends a `ClientToServer` with the
  watched athlete's `PlayerState` once per second over the UDP
  channel.
- Tracing events: `relay.udp.connecting`, `relay.udp.established`,
  `relay.udp.timeout`, `relay.udp.recv_error`,
  `relay.udp.shutdown`, `relay.udp.inbound` (DEBUG),
  `relay.heartbeat.sent` (TRACE).

### Initial UDP-server selection at 12.3

Until 12.4 is implemented, the orchestrator does not yet parse
`udpConfigVOD`. The plan for 12.3 is therefore one of (decision
deferred to implementation; see "Open verification points" below):

- **Option A (preferred if a static initial UDP server can be
  identified from the relay-session response or the
  configuration):** use a hard-coded or session-derived initial
  UDP server. This permits 12.3 to deliver sustained connectivity
  without depending on 12.4's pool routing.
- **Option B:** wait for the first inbound `udpConfigVOD` message
  on TCP, parse the minimum field set required to extract a
  server address, and bring up UDP only at that point. This
  couples 12.3 and 12.4 more tightly, but it matches sauce4zwift's
  observed behaviour.

Either way, the existing `UdpChannel::establish` is used
unchanged: it owns the hello loop, the SNTP-style time sync, and
the recv loop, all from STEP-10.

### Heartbeat content

The heartbeat is a `ClientToServer` carrying the watched
athlete's `PlayerState`. For the smoke case where the watched
athlete is the logged-in user and the user is not actively
riding, the `PlayerState` fields default to zero motion. The
server's liveness model only requires that something arrives on
the cadence; the exact content is not the source of liveness.

The heartbeat thread also owns the seqno and `world_time` fields
on the outgoing `ClientToServer`. `world_time` is taken from the
shared `WorldTimer` (initialised by the UDP channel's hello
loop). Seqno increments monotonically per send.

### Tests

| Test | Asserts |
|---|---|
| `heartbeat_emits_at_one_hz` | A test runtime advances tokio time; the scheduler emits exactly N CtS messages over N seconds. |
| `heartbeat_increments_seqno_per_send` | Successive sends carry strictly increasing seqno values. |
| `heartbeat_world_time_tracks_world_timer` | When the `WorldTimer` advances, the next heartbeat's `world_time` reflects the advance. |
| `udp_channel_subscriber_logs_inbound_at_debug` | An inbound StC packet on UDP triggers a `relay.udp.inbound` DEBUG record. |
| `udp_shutdown_drains_capture_writer` | The capture writer's drop count remains zero across a normal UDP shutdown when no records were dropped due to saturation. |

### Live validation at the end of 12.3

The connection is now indefinitely sustainable. The validation
window is bounded by the user, not the server:

```
ranchero start --foreground -v --capture /tmp/sustained.cap
```

Run for at least five minutes. Confirm via the log that no
`relay.tcp.timeout` event fires and that `relay.heartbeat.sent`
records appear at one-second cadence. Stop the daemon manually
and confirm the capture file contains records from both the UDP
and TCP transports (`ranchero replay --verbose /tmp/sustained.cap`
shows non-zero counts for both transports).

## Sub-step 12.4 — `udpConfigVOD` parsing and pool routing

### What it adds

- A `UdpPoolRouter` that consumes inbound `ServerToClient`
  messages on TCP, extracts attached `udpConfigVOD` records, and
  maintains a per-`(realm, courseId)` table of `UdpServerVODPool`
  entries. The latest update for a given key replaces the
  previous entry.
- A `findBestUDPServer(pool, x, y)` function that ports
  `zwift.mjs:2295-2317`:
  - If `pool.use_first_in_bounds`, return the first server whose
    bounding box `(x_bound_min, y_bound_min, x_bound, y_bound)`
    contains `(x, y)`.
  - Otherwise, return the server whose bound centre minimises
    the Euclidean distance to `(x, y)`.
- Per-course UDP reselection: when the watched athlete's
  `(realm, courseId)` changes, or when the watched athlete moves
  to a position outside the current UDP server's bounds, the
  router recomputes the best server. If the new server differs
  from the current one, the orchestrator brings up a new
  `UdpChannel` to the new address, hands the heartbeat scheduler
  to the new channel, and shuts down the old channel.

### Tests

| Test | Asserts |
|---|---|
| `find_best_first_in_bounds_returns_first_match` | Synthetic pool with `use_first_in_bounds = true`; query inside server B's box returns server B even when A is also in-bounds at a later index. |
| `find_best_first_in_bounds_falls_back_to_distance_when_no_match` | No bounding box contains the query; the result is the min-Euclidean server. |
| `find_best_min_euclidean_when_first_in_bounds_disabled` | `use_first_in_bounds = false`; result is min-Euclidean regardless of bounds containment. |
| `find_best_returns_none_for_empty_pool` | An empty pool returns `None`. |
| `pool_router_replaces_pool_on_repeated_udp_config_vod` | Two consecutive `udpConfigVOD` updates for the same `(realm, courseId)`; the second wins. |
| `pool_router_keys_per_realm_and_course` | Updates for `(realm, courseId)` `(0, 1)` and `(0, 2)` are stored independently. |
| `position_change_within_same_pool_swaps_server_when_bounds_demand` | The watched athlete crosses a bound; the orchestrator selects the new server and swaps UDP channels. |
| `course_change_triggers_pool_reselection` | The watched athlete's course changes; the orchestrator selects a server from the new course's pool. |

### Cross-reference

Spec §4.8 (server selection) and `zwift.mjs:2295-2317` are the
authoritative references for the algorithm. The Rust
implementation must match the JavaScript byte-for-byte on every
test vector.

## Sub-step 12.5 — Idle suspension, watched-athlete switching, GameEvent emission

### Idle suspension FSM

Per spec §4.13. States and transitions:

| State | Trigger | Next state |
|---|---|---|
| Active | Inbound `PlayerState` for the watched athlete shows `speed == 0 && cadence == 0 && power == 0` | Idle (timer starts at 60 s) |
| Idle | Any inbound `PlayerState` with non-zero motion | Active |
| Idle | Timer reaches 60 s without observed motion | Suspended (UDP channel shut down) |
| Suspended | Inbound `PlayerState` with non-zero motion | Active (UDP channel re-established) |

The default idle window is approximately 60 s per spec §4.13;
the exact value should match sauce4zwift's constant when the
implementation begins.

For the smoke case where the watched athlete is the logged-in
user and the user is not actively riding, the FSM enters
`Suspended` after one minute and the UDP channel closes. TCP
remains connected and continues to receive `udpConfigVOD`
updates. When the user begins riding, UDP is re-established
automatically.

### Watched-athlete switching

A configuration field (or a runtime control message) selects the
watched athlete by id. The default is the logged-in user. When
the watched athlete changes:

1. The orchestrator clears its watched-athlete state.
2. On the next inbound `PlayerState` for the new watched
   athlete, it captures the new `(realm, courseId, x, y)`.
3. If the new athlete is on a different course, the UDP pool
   router runs and a new UDP channel is brought up.

### `GameEvent` enum

```rust
#[derive(Debug, Clone)]
pub enum GameEvent {
    /// The watched athlete's `PlayerState` was updated.
    PlayerState {
        athlete_id: i64,
        realm: i32,
        course_id: i32,
        position: (f64, f64),
        power_w: i32,
        cadence_rpm: i32,
        speed_mm_s: i32,
        world_time_ms: i64,
    },
    /// A `WorldUpdate` arrived (typically piggy-backed on a TCP
    /// message). The shape mirrors sauce4zwift's downstream
    /// surface; details deferred to STEP 17.
    WorldUpdate(zwift_proto::WorldUpdate),
    /// A latency sample was produced by a UDP hello-loop response.
    Latency { latency_ms: i64, server_addr: SocketAddr },
    /// The orchestrator's high-level state changed.
    StateChange(RuntimeState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeState {
    Authenticating,
    SessionLoggedIn,
    TcpEstablished,
    UdpEstablished,
    UdpSuspended,
    ShuttingDown,
}
```

`GameEvent` is delivered via a `tokio::sync::broadcast` channel
exposed by `RelayRuntime::events()`. STEP 17's web/WS server is
the first downstream consumer; STEP 14's data model is the
second.

### Tests

| Test | Asserts |
|---|---|
| `idle_fsm_starts_active_and_remains_active_on_motion` | Inbound `PlayerState` with non-zero power keeps the FSM in `Active`. |
| `idle_fsm_transitions_active_to_idle_on_zero_motion` | A single zero-motion update moves the FSM to `Idle` with a 60 s timer. |
| `idle_fsm_returns_to_active_on_motion_within_window` | Motion before the timer fires returns the FSM to `Active`. |
| `idle_fsm_suspends_after_timer_expires` | The orchestrator shuts down UDP when the timer fires. |
| `idle_fsm_resumes_on_motion_when_suspended` | Motion in the `Suspended` state re-establishes UDP. |
| `watched_athlete_switch_resets_state` | Changing the watched-athlete id clears the cached `(realm, courseId, x, y)`. |
| `watched_athlete_switch_triggers_udp_reselection_on_course_change` | A new watched athlete on a different course causes the UDP pool router to fire and the UDP channel to swap. |
| `game_event_player_state_emitted_on_inbound` | An inbound `ServerToClient` carrying the watched athlete's `PlayerState` produces a `GameEvent::PlayerState`. |
| `game_event_state_change_emitted_on_lifecycle_transitions` | The `RuntimeState` transitions are broadcast in order. |

## Implementation phases

A recommended order. Each phase ends with a green
`cargo test --workspace` and, where the phase touches the
network surface, a manual smoke against production Zwift.

1. **12.1a** — Define `RelayRuntime`, `RelayRuntimeError`, the
   DI traits, and their default implementations.
2. **12.1b** — Implement `RelayRuntime::start` for the TCP-only
   path and the recv-loop tracing emission.
3. **12.1c** — Update `daemon::start`, `daemon::runtime::run_daemon`,
   and `cli::dispatch`. Delete the Fix-D test and add the
   positive replacement.
4. **12.1d** — Bounded TCP-only live smoke against production
   Zwift. Confirm the lifecycle records and a non-zero
   capture-record count before proceeding.
5. **12.3a** — `HeartbeatScheduler` and `WorldTimer` plumbing,
   tested against a mock UDP transport.
6. **12.3b** — Wire `UdpChannel::establish` into `RelayRuntime`
   using either Option A or Option B for initial UDP-server
   selection. Sustained live validation at the end of this
   phase confirms the connection survives past the TCP-only
   timeout.
7. **12.4a** — `udpConfigVOD` parsing into a `UdpPoolRouter`
   structure, with table-driven tests on synthetic inbound
   messages.
8. **12.4b** — `findBestUDPServer` port from
   `zwift.mjs:2295-2317`, with the table-driven tests listed
   above.
9. **12.4c** — Wire the router and the watched-athlete state
   into `RelayRuntime`, including UDP channel swap on server
   change.
10. **12.5a** — `IdleFSM` standalone, with state-transition
    tests.
11. **12.5b** — Wire the `IdleFSM` into the orchestrator: it
    shuts down UDP on the suspend transition and re-establishes
    on the resume transition.
12. **12.5c** — Watched-athlete switching, including the
    broadcast-channel control message that selects a new
    athlete.
13. **12.5d** — `GameEvent` enum and the `events()` broadcast
    surface. Existing emitters are reorganised to feed the
    enum.

## CLI and daemon integration

Sub-step 12.1 owns the CLI and `daemon::start` changes for the
orchestrator; the details are in the "CLI and daemon changes"
section under sub-step 12.1 above. No additional CLI surface is
added by 12.3, 12.4, or 12.5. A future `--watch <athlete-id>`
flag (and a control-socket message that switches the watched
athlete at runtime) is anticipated but is deferred; by default
the watched athlete is the logged-in user.

## Logging contract

The orchestrator emits the following `tracing` events. Targets
are prefixed with `ranchero::relay` so that `RUST_LOG`-style
filtering can reach them precisely.

Sub-step 12.1 (TCP-only foundation):

| Level | Event                                | Fields |
|-------|--------------------------------------|--------|
| INFO  | `relay.login.ok`                     | `email`, `relay_id`, `tcp_server_count` |
| INFO  | `relay.tcp.connecting`               | `addr`, `port` |
| INFO  | `relay.tcp.established`              | `addr`, `port` |
| INFO  | `relay.tcp.timeout`                  |  |
| WARN  | `relay.tcp.recv_error`               | `error` |
| INFO  | `relay.tcp.shutdown`                 |  |
| DEBUG | `relay.tcp.inbound`                  | `payload_len`, summary fields drawn from the decoded `ServerToClient` |
| INFO  | `relay.capture.opened`               | `path` |
| INFO  | `relay.capture.closed`               | `dropped_count` |

Extensions added by 12.3, 12.4, and 12.5:

| Level | Event                          | Fields |
|-------|--------------------------------|--------|
| INFO  | `relay.udp.connecting`         | `addr`, `port` |
| INFO  | `relay.udp.established`        | `addr`, `port`, `latency_ms` |
| INFO  | `relay.udp.timeout`            |  |
| WARN  | `relay.udp.recv_error`         | `error` |
| INFO  | `relay.udp.shutdown`           | `reason` (`graceful` / `idle_suspend` / `pool_swap`) |
| DEBUG | `relay.udp.inbound`            | `payload_len` |
| TRACE | `relay.heartbeat.sent`         | `seqno`, `world_time_ms` |
| INFO  | `relay.pool.update`            | `realm`, `course_id`, `server_count` |
| INFO  | `relay.pool.swap`              | `from_addr`, `to_addr`, `reason` |
| INFO  | `relay.idle.suspend`           |  |
| INFO  | `relay.idle.resume`            |  |
| INFO  | `relay.watched_athlete.switch` | `from_id`, `to_id` |

`relay.tcp.inbound` and `relay.udp.inbound` are at DEBUG so that
a long-running session does not flood the default log at INFO.
The default backgrounded configuration (per STEP 04) emits INFO
on ranchero crates, which is sufficient to confirm that the
daemon connected and remained connected without recording each
frame. `-v` or `-D` reaches the per-frame records.

## Live validation procedure (sustained smoke)

Performed at the end of STEP-12 against production Zwift. The
goal is to confirm that the connection survives indefinitely and
that all observed traffic reaches the log file and (when
requested) the capture file.

1. Configure ranchero with valid Zwift credentials. Confirm
   with `ranchero auth-check` that credential resolution
   reports the expected email.
2. Start the daemon in the foreground with verbose logging and
   a capture file:
   ```
   ranchero start --foreground -v --capture /tmp/sustained.cap
   ```
3. Confirm in the log file that the lifecycle records appear in
   order: `relay.login.ok`, `relay.tcp.connecting`,
   `relay.tcp.established`, `relay.udp.connecting`,
   `relay.udp.established`. Subsequent `relay.heartbeat.sent`
   records (at TRACE; add `-D` if needed) confirm that the
   scheduler is firing.
4. Allow the daemon to run for at least 30 minutes. Confirm
   that no `relay.tcp.timeout` or `relay.udp.timeout` records
   appear during the window. If the watched athlete is the
   logged-in user and the user is not riding, confirm that a
   `relay.idle.suspend` record appears after approximately one
   minute and that the connection continues to receive TCP
   traffic.
5. If the user begins riding (or another athlete is selected
   as the watched athlete), confirm that
   `relay.idle.resume` and a fresh
   `relay.udp.established` appear and that
   `relay.heartbeat.sent` records resume.
6. Stop the daemon with `ranchero stop`. Confirm the shutdown
   sequence in the log:
   `relay.idle.* (if applicable)` →
   `relay.udp.shutdown` → `relay.tcp.shutdown` →
   `relay.capture.closed`.
7. Run `ranchero replay /tmp/sustained.cap` and confirm
   non-zero record counts for both transports and a positive
   total-bytes figure. Run with `--verbose` to confirm that
   the per-record summary contains both UDP and TCP records,
   and that outbound records (the heartbeats and the initial
   TCP hello) are present.
8. Append the run's wall-clock duration, record count by
   transport and direction, dropped-record count from the
   capture writer, and any error events to this file under a
   "Live validation results" section.

## Acceptance criteria

- All sub-steps' tests pass: the new tests in
  `src/daemon/relay.rs`, `tests/cli_args.rs`, and
  `tests/relay_runtime.rs` (added by 12.1), plus the unit
  tests for `HeartbeatScheduler`, `UdpPoolRouter`,
  `findBestUDPServer`, `IdleFSM`, and the `GameEvent` surface.
- `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` are
  both green.
- The bounded TCP-only live validation (end of 12.1) and the
  sustained live validation (end of STEP-12) have both been
  performed at least once against production Zwift. The
  results are appended to this file under a "Live validation
  results" section, showing no server-side timeout during the
  sustained run, the expected lifecycle records, and a
  non-zero capture-record count for both transports in both
  directions.
- `ranchero stop` performs a clean teardown that flushes the
  capture writer (zero truncation, every accepted record
  readable on replay) and shuts down the relay session.
- The capture file written during a 30-minute run is
  reproducibly readable by `ranchero replay`. The replay
  summary reports inbound and outbound counts for both UDP
  and TCP.
- The STEP-11.6 Fix-D guard is removed from `src/cli.rs` and
  the corresponding negative test is removed from
  `tests/cli_args.rs`.

## Open verification points

These are decisions or facts that depend on production
behaviour and should be settled during implementation rather
than in this plan.

1. **Initial UDP-server selection at 12.3.** Whether the relay
   session login response carries an initial UDP server, or
   whether the orchestrator must wait for the first
   `udpConfigVOD` over TCP before bringing up UDP. The plan
   accommodates either path; the implementation chooses based
   on what production traffic actually contains.
2. **Heartbeat content for an idle observer.** Whether the
   server requires the heartbeat to carry plausibly recent
   `PlayerState` fields, or whether all-zeros suffices for
   liveness. If all-zeros is rejected, the heartbeat scheduler
   must mirror the most recent inbound `PlayerState` for the
   watched athlete.
3. **Idle window constant.** The exact value used by
   sauce4zwift for the idle window (the plan assumes
   approximately 60 s per spec §4.13). The implementation
   must read the constant from sauce's source rather than
   re-deriving it.
4. **Suspended-state TCP behaviour.** Whether TCP must continue
   to receive `udpConfigVOD` updates while UDP is suspended,
   or whether the server stops sending updates when the client
   has not sent a heartbeat in some time. The plan assumes
   TCP continues; the implementation must confirm.
5. **Watched-athlete switch on a non-self athlete.** The
   permissions model for watching another athlete (whether
   the monitor account is required, and how `udpConfigVOD`
   pools differ between accounts). Deferred to a future
   verification.

## Deferred to later steps

| Concern | Where |
|---|---|
| Decoding `ServerToClient` into a per-athlete data model | STEP 14 |
| Rolling-window statistics (NP, TSS, peak power) | STEP 13 |
| W-prime balance, segment matching, group detection | STEP 15 |
| SQLite persistence of athlete history | STEP 16 |
| HTTP and WebSocket server compatible with `webserver.mjs` | STEP 17 |
| v1 / v2 payload formatters | STEP 18 |
| Compatibility test battery against captured fixtures | STEP 19 |

## Related but separate work

`docs/plans/STEP-12.2-follow-command.md` describes the
`ranchero follow <file>` command, a live tailing reader for
wire-capture files. Its content shares the digit prefix with
this plan but is a separate piece of work, not a sub-step of
STEP-12. It depends on the file format from STEP-11.5 and on
capture files produced by sub-step 12.1, but it does not modify
the orchestrator and it can be implemented independently.

## Cross-references

- `docs/plans/STEP-12.2-follow-command.md` — the
  `ranchero follow <file>` command for live capture-file
  tailing. Independent of this plan.
- `docs/plans/done/STEP-09-relay-session.md` — the relay
  session and supervisor used by `RelayRuntime`.
- `docs/plans/done/STEP-10-udp-channel.md` — the UDP channel
  with hello-loop and SNTP-style time sync.
- `docs/plans/done/STEP-11-tcp-channel.md` — the TCP channel.
- `docs/plans/done/STEP-11.5-wire-capture.md` — the capture
  mechanism wired into both channels.
- `docs/plans/done/STEP-11.6-capture-consistency-review.md` —
  the consistency review whose Fix-D guard is removed by
  sub-step 12.1.
- `docs/ARCHITECTURE-AND-RUST-SPEC.md` — §4.4
  (`ClientToServer` hello fields), §4.8 (UDP server
  selection), §4.13 (idle suspension), §7.12 (client-driven
  liveness model).
- sauce4zwift's `zwift.mjs:2295-2317` — the
  `findBestUDPServer` reference implementation.
