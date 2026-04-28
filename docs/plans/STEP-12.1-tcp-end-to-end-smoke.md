# Step 12.1 — TCP-only end-to-end smoke

**Status:** planned (2026-04-28).

This is the first sub-step of STEP-12 (`STEP-12-game-monitor.md`).
It delivers the foundation on which the rest of STEP-12 builds:
authenticated login, a relay session, and a single TCP channel
emitting captured bytes to a wire-capture file and tracing records
to the configured log file. It does **not** deliver sustained
operation. Without the 1 Hz UDP heartbeat (added in the rest of
STEP-12), the Zwift server will terminate the TCP connection
within roughly 30 s of client silence per the client-driven
liveness model documented in spec §7.12.

## Goal

A `ranchero start` invocation that authenticates against Zwift,
establishes a relay session, opens a TCP channel to a Zwift relay
server, receives `ServerToClient` messages, and logs each arrival
to the configured log file. When `--capture <path>` is passed,
the same byte stream is also written to a wire-capture file
readable by `ranchero replay`.

The deliverable is a connection that lasts long enough to confirm
that the protocol stack works against the production Zwift
servers (roughly 30 s, until the server times out the connection
for lack of UDP heartbeats). That confirmation is the
prerequisite for the rest of STEP-12, which then sustains the
connection indefinitely.

## Scope

In scope:

- An orchestrator module that wires authentication, the relay
  session, and a single TCP channel into the daemon's run loop.
- CLI integration: `--capture <path>` opens a `CaptureWriter`,
  the writer is passed through to the TCP channel configuration,
  and the writer is closed cleanly on shutdown.
- Logging: each `TcpChannelEvent` reaches the configured log
  file via `tracing`. Inbound `ServerToClient` messages are
  summarised at one record per arrival.
- Daemon shutdown coordination: the existing UDS control
  protocol triggers a graceful teardown of the orchestrator
  before the process exits.
- Removal of the STEP-11.6 Fix-D guard, replaced by positive
  behaviour and an updated test.
- A bounded live validation: confirm the daemon connects, logs
  the inbound stream, and the capture file contains records.
  The validation window ends either at the first server-side
  timeout (expected) or after a manual `ranchero stop`.

Out of scope (delivered by the rest of STEP-12):

- UDP channel establishment and the SNTP-style time sync.
- The 1 Hz UDP heartbeat that keeps the server-side TCP
  connection alive.
- `udpConfigVOD` parsing and `findBestUDPServer` pool routing.
- Idle suspension when the watched athlete is stationary.
- Watched-athlete switching and per-course UDP reselection.
- The `GameEvent` enum and its downstream consumers.
- A parsed view of inbound `ServerToClient` messages beyond the
  summary log line. Decoding to the full per-athlete data model
  is STEP 14.

## Module layout

The orchestrator is owned by the `ranchero` root crate. It is
placed under the daemon module, alongside the existing run loop,
because its lifetime is bound to the daemon's lifetime.

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

The rest of STEP-12 extends `relay.rs` with UDP-channel and
heartbeat ownership; it may also split into multiple files as
the orchestrator grows.

## Public API surface (proposed)

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

The signature is forward-compatible with the rest of STEP-12:
later sub-steps add a UDP channel inside the runtime, attach a
heartbeat scheduler to it, and consume inbound TCP messages to
update the UDP pool — none of which changes the public surface
seen by `daemon::runtime`.

## Logging contract

The orchestrator emits the following `tracing` events. Targets
are prefixed with `ranchero::relay` so that `RUST_LOG`-style
filtering can reach them precisely.

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

`relay.tcp.inbound` is at DEBUG so that a long-running session
does not flood the default log at INFO. The default
backgrounded configuration (per STEP 04) emits INFO on ranchero
crates, which is sufficient to confirm that the daemon connected
and remained connected without recording each frame. `-v` or
`-D` reaches the per-frame records.

## CLI and daemon changes

### `src/cli.rs`

The STEP-11.6 Fix-D guard is removed. The `Command::Start` arm
passes `cli.global.capture` through to `daemon::start`:

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

### `src/daemon/mod.rs`

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

### `src/daemon/runtime.rs`

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

## Tests-first plan

The orchestrator depends on real network endpoints (HTTPS for
auth, HTTPS for relay session, TCP for the channel). The tests
use the same mock infrastructure already in use by the lower
crates: `wiremock` for the two HTTPS endpoints (already used in
`zwift-api/tests/auth.rs` and `zwift-relay/tests/session.rs`),
and the existing mock `TcpTransport` used in
`zwift-relay/tests/tcp.rs`. Dependency injection points are
introduced on `RelayRuntime` so that tests can substitute the
transport without going through the kernel.

### `src/daemon/relay.rs` unit tests

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

### `tests/cli_args.rs` updates

The STEP-11.6 negative test is removed and replaced by a
positive contract:

| Test | Asserts |
|---|---|
| `dispatch_start_passes_capture_path_to_daemon` | A stub `daemon::start` recorded by an injection point receives `capture_path = Some("/tmp/x.cap")` when the user invokes `start --capture /tmp/x.cap`. |

The Fix-D test (`dispatch_start_with_capture_errors_until_step12`)
must be deleted as part of this step. Its presence would block
the positive behaviour from being exercised.

### Integration test in `tests/relay_runtime.rs` (new)

| Test | Asserts |
|---|---|
| `runtime_writes_capture_file_for_inbound_packets` | Stand up `wiremock` for `/auth/realms/zwift/protocol/openid-connect/token` and `/api/users/login`, plus a fake TCP server on a localhost ephemeral port that emits a single encrypted `ServerToClient` frame. Run `RelayRuntime::start` with a capture path, wait for one `Inbound` event to fire, call `shutdown`, then read the capture file with `CaptureReader` and assert one record. |
| `runtime_logs_login_and_established_at_info` | Same setup; assert that `relay.login.ok` and `relay.tcp.established` records are produced. |

The integration test is feasible because `wiremock` already
covers the HTTPS endpoints and `TokioTcpTransport::connect` will
accept an arbitrary `SocketAddr`. The orchestrator's `start`
accepts a configuration that points at the mocked endpoints.

## Implementation outline

1. Define `RelayRuntime`, `RelayRuntimeError`, the dependency-
   injection traits (`AuthClient`, `SessionClient`,
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
   path to `daemon::start`. Delete or rewrite the corresponding
   test in `tests/cli_args.rs`.
7. Update
   `docs/plans/done/STEP-11.6-capture-consistency-review.md` to
   record that Fix-D has been superseded by STEP-12.1.

## Live validation procedure

Live validation is a manual acceptance step rather than an
automated test. It must be performed before STEP-12.1 is
considered complete. Because UDP heartbeats are out of scope,
the validation window ends either at the first server-side
timeout or at a manual stop, whichever is first.

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
   the need for the heartbeat work in the rest of STEP-12.
5. Confirm `relay.tcp.shutdown` and `relay.capture.closed`
   records are present in the log file.
6. Run `ranchero replay /tmp/ranchero-smoke.cap` and confirm a
   non-zero record count. Run with `--verbose` to confirm the
   per-record summary.
7. Record the wall-clock duration the connection survived, the
   record count, the capture-record count, and any errors
   observed in this file under a new "Live validation results"
   section appended below.

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
  run and confirms the need for the rest of STEP-12.

## Acceptance criteria

- `cargo test --workspace` passes; the new tests under
  `src/daemon/relay.rs` and `tests/relay_runtime.rs` are
  present and green.
- `cargo clippy --workspace --all-targets -- -D warnings`
  reports no warnings.
- `ranchero start --foreground` against valid credentials
  produces `relay.login.ok` and `relay.tcp.established`
  records in the configured log file.
- `ranchero start --foreground --capture /tmp/x.cap` against
  valid credentials produces a non-empty capture file readable
  by `ranchero replay /tmp/x.cap`. The replay summary reports
  a positive inbound TCP record count and a positive
  total-bytes figure.
- `ranchero stop` triggers a clean shutdown: the log file
  contains `relay.tcp.shutdown` and `relay.capture.closed`
  records, and the capture file is closed without truncation
  (the last record's payload is fully present).
- The STEP-11.6 Fix-D guard is removed from `src/cli.rs`. The
  test `dispatch_start_with_capture_errors_until_step12` is
  removed from `tests/cli_args.rs`.
- The live-validation procedure has been performed at least
  once against the production Zwift servers, and the results
  are appended to this file under "Live validation results".

## Cross-references

- `docs/plans/STEP-12-game-monitor.md` — the parent plan,
  which documents the broader gap and the work that completes
  sustainable connectivity.
- `docs/plans/done/STEP-11.5-wire-capture.md` — the capture
  mechanism this sub-step exercises end-to-end.
- `docs/plans/done/STEP-11.6-capture-consistency-review.md` —
  the Fix-D guard that this sub-step removes.
- `docs/ARCHITECTURE-AND-RUST-SPEC.md` — §4.4
  (`ClientToServer` hello fields), §7.12 (client-driven
  liveness model).
