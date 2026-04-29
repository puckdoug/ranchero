# Step 12.6 — Really basic implementation details that were screwed up anyway

**Status:** review (2026-04-29).

## Summary of findings

Operator-path defects (block `start --capture` → `stop` →
`replay` from delivering a populated capture file):

- **Defect 1** — `run_daemon` swallows `RelayRuntime::start`
  errors and runs the UDS loop in degraded mode instead of
  exiting non-zero. (`src/daemon/runtime.rs:243-258`)
- **Defect 2** — `ResolvedConfig::resolve` never consults the
  OS keychain for passwords; only the `--mainpassword` /
  `--monitorpassword` CLI flags are read.
  (`src/config/mod.rs:370-378`)
- **Defect 3** — The TCP hello `ClientToServer` is never sent;
  the Zwift server has no basis to scope inbound traffic to
  the connection. (`src/daemon/relay.rs:716-756`)
- **Defect 4** — No UDP transport or `UdpChannel` is ever
  constructed in production; the live telemetry stream
  (spec §4.6, §4.10) does not run.
  (`src/daemon/relay.rs:664-812`)
- **Defect 5** — The 1 Hz `HeartbeatScheduler` is never spawned
  in production; without it the server-side liveness model
  expires the connection. (`src/daemon/relay.rs:165-239`,
  no production instantiation)
- **Defect 6** — `TcpChannel<T>` is moved into the recv-loop
  spawn, leaving no handle through which any later step
  could send a hello or heartbeat.
  (`src/daemon/relay.rs:792-802`)
- **Defect 7** — `RelaySessionSupervisor` is never started;
  only the single-shot `zwift_relay::login` runs, so the
  session is never refreshed and silently expires.
  (`src/daemon/relay.rs:983-988`)

Configuration / diagnostic defects (do not block the workflow
but produce silently wrong or misleading behaviour):

- **Defect 8** — `ResolvedConfig.log_level` is read from TOML
  and silently ignored by `daemon::start` /
  `logging::install`. (`src/config/mod.rs:419`,
  `src/daemon/runtime.rs:52`)
- **Defect 9** — `print_auth_check` reports
  `Config::default()` URLs instead of `cfg.zwift_endpoints`,
  contradicting what `start` will actually use after
  STEP-12.5 §F. (`src/cli.rs:333, 340-343, 409, 414`)
- **Defect 10** — `src/tui/keyring.rs` is a four-line
  re-export shim, in violation of the no-shim rule.
  (`src/tui/keyring.rs`, consumers at
  `src/tui/driver.rs:12`, `tests/tui.rs:5`)

Observations (operator-visible configuration with no current
consumer; reasonably deferred to later STEPs but worth flagging
in the parent plan or TUI help text):

- **Observation 1** — Monitor-account credential is stored,
  surfaced in `auth-check`, and ignored by the orchestrator.
  (`src/daemon/relay.rs:678-685`)
- **Observation 2** — `server_bind` / `server_port` /
  `server_https` are loaded with full schema and env
  overrides; no consumer exists outside test fixtures.
  (`src/config/mod.rs:319-321`)

Minor cosmetic findings:

- **Minor 1** — `start_inner` numbered comments skip step 5
  (1, 2, 3, 4, 6, 7, 8). (`src/daemon/relay.rs:677-756`)
- **Minor 2** — `tcp_config.athlete_id` and `conn_id` are
  hardcoded to 0; placeholders that work only because nothing
  is sent today. (`src/daemon/relay.rs:717-722`)

Defects 1 and 2 are detailed below under `Defects`. Defects 3
through 10 and the observations / minor findings are detailed
in `Addendum 2026-04-29 — full operator-path walkthrough`
toward the end of this document.

## Background

This document originally recorded two defects discovered
immediately after the STEP-12.5 §F testability work was
reported as complete. Both surfaced in a single live
invocation:

```
target/debug/ranchero start --debug --capture zwift.cap
```

against a fully configured local installation (credentials in the
macOS keychain, valid TOML config, no CLI password override). The
output was:

```
2026-04-29T08:13:20.287811Z ERROR ranchero::relay:
  relay.start.failed error=missing main account password;
  store one via `ranchero configure`
```

Both findings are basic implementation details that should not have
been in their current state when STEP-12 / STEP-12.5 were reported
as complete. STEP-12.5 §F changed the `ResolvedConfig::resolve`
signature for the Zwift endpoint work but did not notice either
gap during review. The numerical prefix `12.6` is a separate-file
label following `STEP-12.5`; it has no relation to any internal
sub-step `12.6` inside `STEP-12-game-monitor.md`.

## Why this happened

The two defects are independent, but both are gaps in the operator
path that the unit and integration test surfaces did not catch
because every test that exercises `RelayRuntime::start` builds a
`ResolvedConfig` directly (with explicit `main_password`) rather
than going through the CLI dispatcher. The dispatcher is the
component that should consult the keychain and abort on
`relay.start.failed`, and the dispatcher is the layer that has
the least direct test coverage today.

Specifically:

- `tests/full_scope.rs` builds `ResolvedConfig` via `lib_config`
  with a hand-supplied password; it never goes through the
  dispatcher's credential resolution path.
- `tests/daemon_lifecycle.rs` spawns the binary with no
  credentials, so the `MissingEmail` short-circuit in
  `start_inner` returns before any keyring or auth call. The
  failure is silent because `run_daemon` swallows it and runs
  the UDS loop in degraded mode (Defect 1, below).

The combination is exactly the operator-path the live invocation
exercises: a daemon binary, with no `--mainpassword` flag, with
keychain entries written by a previous `ranchero configure`. No
test in the workspace exercises that path end-to-end.

## Defects

### 1. `run_daemon` swallows `RelayRuntime::start` errors

**Where it lives.** `src/daemon/runtime.rs:243-258`:

```rust
let runtime = match super::relay::RelayRuntime::start(&cfg, capture_path).await {
    Ok(r) => Some(r),
    Err(e) => {
        tracing::error!(
            target: "ranchero::relay",
            error = %e,
            "relay.start.failed",
        );
        None
    }
};

loop {
    tokio::select! {
        biased;
        _ = shutdown_rx.recv() => break,
        _ = tokio::signal::ctrl_c() => break,
        _ = sigterm.recv() => break,
        accept = listener.accept() => { /* ... */ }
    }
}
```

**What is wrong.** When `RelayRuntime::start` returns an error —
missing credentials, auth failure, network unreachable, or any
other variant of `RelayRuntimeError` — the daemon swallows the
error, logs a single `ERROR` line, and enters the UDS-only event
loop. From the operator's view, the process appears to have
started successfully: the pidfile is written, the control socket
is bound, `ranchero status` reports "running", and the only
signal that anything is wrong is one `ERROR` record in the log
file. The daemon does no orchestration work and never will.

**Why it shipped.** STEP-12.5 §D explicitly chose this behaviour
and documented it as a feature, with the rationale "continue
running the UDS loop in degraded mode so `ranchero stop` still
terminates the process cleanly." That reasoning is incorrect.
The pidfile and socket are cleaned up by `runtime::start` after
`run_daemon` returns, regardless of return value, so abort-on-
error is already well-behaved. The §D design choice traded
correct semantics for a non-existent benefit. STEP-12.5 §D is
hereby retracted; this document supersedes it.

**Required remediation.** Convert `RelayRuntimeError` to
`io::Error` and propagate it. The `?` operator at the
`RelayRuntime::start` call site replaces the `match` block:

```rust
let runtime = super::relay::RelayRuntime::start(&cfg, capture_path)
    .await
    .map_err(|e| io::Error::other(e))?;
```

Accept the error early; the existing pidfile and socket cleanup
in `runtime::start` runs whether `run_daemon` returns `Ok(())`
or `Err(_)`. The process exits with a non-zero status on
`run_daemon` failure (already the case for `io::Error` returns
in this function).

The `relay.start.failed` log record stays — the operator's log
file is the canonical place that records *why* the start
failed. The change is that the process also exits.

The orchestrator's resilience strategy for transient network
errors (auto-retry with backoff, suspended state with periodic
re-attempt, and so on) is a separate question for a later step.
The current behaviour for *any* `RelayRuntime::start` error is
abort. A retry policy can refine that later without changing
this contract.

### 2. The dispatcher's password resolution path bypasses the keychain

**Where it lives.** `src/config/mod.rs:370-378`:

```rust
let main_password = cli.mainpassword.clone()
    .map(RedactedString::new);

// (no keyring lookup)

let monitor_password = cli.monitorpassword.clone()
    .map(RedactedString::new);
```

**What is wrong.** `ResolvedConfig::resolve` reads `main_password`
and `monitor_password` from CLI flags only. The keychain is not
consulted at all. Files do not carry passwords by design (the
existing schema validates this), so the only way a password
reaches `RelayRuntime::start` is `--mainpassword <PASSWORD>` on
the command line — which is visible in `ps` and a documented
operator hazard.

The keychain integration exists. `src/credentials/mod.rs` defines
`OsKeyringStore::get(role)` and is fully tested. The
`print_auth_check` path at `src/cli.rs:373` already consults it
correctly:

```rust
None => match keyring.get(role) {
    Ok(Some(entry)) => (Some(entry.password), "OS keyring"),
    /* ... */
}
```

The `Start | Stop | Status` arm of `dispatch` at
`src/cli.rs:195-220` constructs no keyring and calls
`ResolvedConfig::resolve` with only `cli`, `OsEnv`, and the
file. The daemon's start path therefore never reaches the
keychain at all, which is what produces the symptom: no
keychain access prompt on macOS, no entry retrieved, and the
"missing main account password" error from the credential check
inside `RelayRuntime::start_inner`.

**Why it shipped.** Latent gap from before STEP-12.5. The
`print_auth_check` path was wired correctly when added, but
`Start | Stop | Status` was never updated to follow the same
pattern. The defect was invisible to the test suite because
every test that exercises `RelayRuntime::start` builds a
`ResolvedConfig` with an explicit password rather than
travelling through the dispatcher. STEP-12.5 §F changed the
`ResolvedConfig::resolve` signature for the Zwift endpoint work
but the password-source survey that should have caught the
keychain gap did not happen.

**Required remediation.** Thread a `&dyn KeyringStore` parameter
through `ResolvedConfig::resolve` and consult it as a fallback
when the CLI flag is absent. The new signature:

```rust
pub fn resolve(
    cli:     &GlobalOpts,
    env:     &dyn Env,
    keyring: &dyn crate::credentials::KeyringStore,
    file:    Option<ConfigFile>,
) -> Result<Self, ConfigError>
```

In the body, replace the password lookups with a CLI →
keychain precedence:

```rust
let main_password = match cli.mainpassword.clone() {
    Some(p) => Some(RedactedString::new(p)),
    None => match keyring.get("main") {
        Ok(Some(entry)) => Some(RedactedString::new(entry.password)),
        Ok(None) => None,
        Err(e) => {
            // Surface the keyring error rather than silently
            // returning None: the operator should see why a
            // configured credential is unreachable.
            return Err(ConfigError::KeyringError(e.to_string()));
        }
    },
};
```

Equivalent block for `monitor_password` against
`keyring.get("monitor")`. Add a `ConfigError::KeyringError`
variant.

The dispatch sites in `src/cli.rs` for `Start | Stop | Status`,
`AuthCheck`, and any future caller pass `&OsKeyringStore::new()`.

In-tree tests pass `&InMemoryKeyringStore::default()` (the
existing no-op stub) when keychain involvement is irrelevant,
or a populated `InMemoryKeyringStore` when the test exercises
the fallback.

The keyring is consulted only if the CLI flag is absent, which
keeps the operator's per-invocation override behaviour
unchanged: `--mainpassword X` still takes precedence over the
keychain. Files never carry passwords; that boundary is
unchanged.

## Acceptance criteria

The following observations must hold against a fully configured
ranchero installation (keychain populated, no `--mainpassword`
flag, valid TOML config):

1. `ranchero start --debug --capture <file>`:
   - Triggers a macOS keychain access prompt the first time it
     is run after a fresh login, or accesses the keychain
     transparently if previously authorised.
   - On successful keychain retrieval, proceeds to the
     orchestrator's auth → session → TCP path.
   - On keychain access failure (denied, no entry), emits a
     `relay.start.failed` record with the keychain error
     surfaced and exits with non-zero status.
   - On any other `RelayRuntime::start` failure, emits
     `relay.start.failed` and exits with non-zero status. The
     pidfile and control socket are removed before exit.

2. The existing acceptance criteria in STEP-12.5 §5 continue to
   hold once these defects are fixed. The `ranchero start
   --capture <file> ; sleep 10 ; ranchero follow <file>`
   workflow used for live verification continues to behave as
   verified on 2026-04-29.

## Test surface

Red-state tests are required for both defects. Each test must
fail before the corresponding remediation is applied and pass
after.

### Defect 1

- `start_aborts_with_nonzero_exit_when_relay_runtime_start_fails`
  — subprocess test. Spawn the binary with no credentials in
  config, no `--mainpassword`, and an unroutable
  `RANCHERO_ZWIFT_AUTH_BASE`. Assert the process exits with
  non-zero status within a short window (current behaviour:
  daemon stays alive in degraded mode, test would time out).

- `start_removes_pidfile_and_socket_on_relay_start_failure` —
  same subprocess setup. After the process exits, assert that
  neither the pidfile nor the control socket remains on disk.

### Defect 2

- `resolve_consults_keyring_for_missing_main_password` — inline
  config test using a populated `InMemoryKeyringStore`. Assert
  `r.main_password.unwrap().expose()` matches the keyring
  value.

- `resolve_cli_password_takes_precedence_over_keyring` — same
  setup with both CLI and keyring populated. Assert the CLI
  value wins.

- `resolve_returns_keyring_error_on_backend_failure` — inline
  test using a stub keyring that always returns
  `KeyringError::Backend`. Assert `resolve` returns the new
  `ConfigError::KeyringError` variant rather than silently
  treating the credential as absent.

- `resolve_no_keyring_entry_leaves_password_unset` — populated
  keyring with no entry for the role; assert `main_password`
  remains `None` and the daemon's downstream credential check
  surfaces `MissingPassword`.

- `start_consults_os_keyring_when_no_cli_password` — subprocess
  test that sets `RANCHERO_ZWIFT_AUTH_BASE=http://127.0.0.1:1`
  and writes a known credential into a test-only keyring
  service name. The test cannot interact with the macOS UI
  prompt, so it must use a service name that has been
  authorised in advance, or it must be marked `#[ignore]` and
  documented as a manual verification step. The latter is
  cheaper for now.

## Cross-references

- `docs/plans/STEP-12.5-still-not-doing-the-job-as-specified.md`
  — STEP-12.5 §D's "degraded mode on start failure" choice is
  retracted by Defect 1 above.
- `docs/plans/STEP-12-game-monitor.md` — the parent plan. The
  acceptance criteria for `ranchero start` should be updated
  to reflect "abort on `RelayRuntime::start` failure" as the
  contract.
- `docs/plans/STEP-20-additional-considerations.md` — entry 20.3
  notes that `source` and `user_agent` are not yet operator-
  configurable; this document does not change that.
- `src/credentials/mod.rs` — the `OsKeyringStore` /
  `InMemoryKeyringStore` / `KeyringStore` trait used by the
  remediation in Defect 2.
- `src/cli.rs:373` — the existing keyring fallback in
  `print_auth_check`, used as the reference pattern for the
  new fallback in `resolve`.

## Honest framing

These two defects are basic implementation details. Defect 1 was
a wrong design choice I made and shipped, documented (under
STEP-12.5 §D) as if it were correct. Defect 2 was a gap that
predated STEP-12.5 and that I did not catch when reviewing the
credential path during the §F work, despite changing the
`resolve` signature for unrelated reasons. Both should have been
caught earlier. Reporting them as fixed under the parent steps
without first exercising the operator path against a fully
configured installation was the underlying error.

---

## Addendum 2026-04-29 — full operator-path walkthrough

After Defects 1 and 2 above were filed, you asked for a complete
walk through every piece of the command-line and startup path,
checking that the work expected by the parent steps was actually
done. This section records that walkthrough. It surfaced eight
further defects (Defects 3 through 10) plus two additional
observations that fall short of being defects in scope for
STEP-12 but are worth recording.

The walkthrough was performed against the following user-facing
goal, taken verbatim from your request:

> at this point the application was expected to be able to fully
> start, connect, and log packet capture for viewing (and view
> it!).

That goal is the contract STEP-12 was supposed to deliver, per
`docs/plans/done/STEP-12-game-monitor.md` lines 16-22:

> a `ranchero start` invocation against valid Zwift credentials
> runs indefinitely without server-side timeout, every inbound
> and outbound packet is observable through the configured log
> file (and recorded in the capture file when one is requested),
> and `ranchero stop` performs a clean teardown that flushes the
> capture writer and shuts down the relay session in order.

The defects below explain why no `ranchero start` invocation can
satisfy that contract today, even after Defects 1 and 2 are
fixed. The capture file written under `--capture <path>` will
contain only the eight-byte `RNCWCAP\0` magic plus the two-byte
format version, with zero records, because the orchestrator
never reaches a state where the Zwift server would send any
inbound traffic and never sends any outbound traffic of its
own.

### The bigger picture

`RelayRuntime::start_inner`
(`src/daemon/relay.rs:664-812`) performs the following sequence
on a successful path:

1. Validate credentials.
2. Call `auth.login(email, password)`.
3. Call `session_factory.login()` (single-shot relay-session
   login).
4. Reject empty TCP-server pool.
5. Connect a TCP socket to `session.tcp_servers[0]`.
6. Call `TcpChannel::establish` (which spawns the channel's
   recv-loop task; that task emits `Established` from inside
   the spawn).
7. Wait up to five seconds for the `Established` event.
8. Spawn a forwarder task that republishes the channel's
   broadcast onto an internal sender, and spawn the
   orchestrator's own `recv_loop` that reads from the forwarded
   broadcast and emits `GameEvent::PlayerState` for every
   inbound `PlayerState`.

Then `start_inner` returns `Ok(self)`. That is the entire
production-side orchestration. After `start_inner` returns:

- No `ClientToServer` packet has been written to the TCP
  channel — not even a hello packet. The Zwift server has an
  open TCP connection from a peer that has not identified
  itself.
- No UDP transport has been bound. No UDP channel exists.
- No 1 Hz heartbeat task is running.
- No relay-session refresh supervisor is running.
- The TCP channel value has been moved into the orchestrator's
  recv-loop spawn (`src/daemon/relay.rs:792-802`); no other
  task has a handle on it. There is no surface through which a
  later step could call `channel.send_packet`.

The Zwift server's behaviour against that profile (silent TCP
peer, no hello, no heartbeat) is to stay quiet until its own
liveness model expires, which the orchestrator sees as
`TcpChannelEvent::Timeout` records on `relay.tcp.timeout` at
INFO. No `relay.tcp.inbound` record will ever land. The
capture file will never grow past the file header.

The four sub-step labels in the parent plan
(`docs/plans/done/STEP-12-game-monitor.md` lines 49-55) describe
what was expected:

| Sub-step | Was supposed to deliver |
|---|---|
| 12.1 | TCP-only foundation, including the initial `ClientToServer` hello. |
| 12.3 | UDP channel + 1 Hz heartbeat. |
| 12.4 | `udpConfigVOD` parsing + `findBestUDPServer`, with per-course UDP reselection. |
| 12.5 | Idle suspension FSM + watched-athlete switching + `GameEvent` enum. |

Of those four sub-steps, only the `GameEvent` enum from 12.5 and
the in-tree pure-function ports of `findBestUDPServer` /
`UdpPoolRouter` / `IdleFSM` / `HeartbeatScheduler` /
`WatchedAthleteState` from 12.3, 12.4, 12.5 are present. None of
them is wired into the production code path. Each has unit-test
coverage that exercises it through `#[cfg(test)]` injection
methods or direct construction; production never instantiates
any of them. The sub-step plans were marked complete on the
strength of those unit tests alone.

### Defect 3. The TCP hello packet is never sent

**Where it lives.** `src/daemon/relay.rs:716-756`,
`crates/zwift-relay/src/tcp.rs:163-220`.

**What is wrong.** The TCP channel's `establish` documentation
(`crates/zwift-relay/src/tcp.rs:164-168`) says:

> Spawn the recv loop and return. Does NOT send a hello packet —
> the supervisor sends that as the first
> `send_packet(.., hello: true)` call so it can carry
> supervisor-tracked fields like
> `largestWorldAttributeTimestamp`.

The orchestrator is the supervisor referenced there. The
orchestrator does not call `channel.send_packet(_, true)`
anywhere — a search across `src/daemon/` for `send_packet` and
`hello` returns zero matches in the production paths. The
hello packet is the contract by which the client identifies
itself to the relay server (relay_id + conn_id + seqno header,
plaintext envelope `[2, 0, …]`); without it the server has no
basis to scope inbound `ServerToClient` traffic to this
connection.

**Why it shipped.** STEP-12.1 was reported complete on the
strength of inline tests that drive
`TcpChannel::establish` directly with a mock transport. The
mock returns immediately for `read_chunk` and never reaches
the inbound-frame path, so the absence of the outbound hello
goes unobserved. The integration test
`tests/full_scope.rs::workflow_start_capture_follow_reads_header`
asserts that `follow` can read the capture header — which
needs only the file header bytes — so it passes against an
empty capture too.

**Required remediation.** After waiting for `Established`,
build a `ClientToServer` carrying the supervisor-tracked
fields (or zero defaults until the rest of STEP-12 lands)
and call `channel.send_packet(payload, true)`. The send path
already records the hello bytes through the capture tap
(`crates/zwift-relay/src/tcp.rs:233-234`).

This requires Defect 6 below to be addressed as well: the
channel value is currently moved into the orchestrator's
recv-loop spawn at `src/daemon/relay.rs:792-802`, leaving no
handle through which to send.

### Defect 4. No UDP channel is established in production

**Where it lives.** `src/daemon/relay.rs:664-812` (no UDP
construction); `crates/zwift-relay/src/udp.rs` (full
implementation, unused).

**What is wrong.** Architecture spec §4.6 and §7.7 mandate a
UDP channel that performs the SNTP-style time sync hello-loop
and then carries the live `PlayerState` stream. The `zwift-relay`
crate exposes `TokioUdpTransport`, `UdpChannel`,
`UdpChannelConfig`, and `ChannelEvent`
(`crates/zwift-relay/src/udp.rs:36-67`, `:142-182`); a search
across `src/` for `UdpChannel`, `TokioUdpTransport`, or
`UdpSocket` returns one comment match
(`src/daemon/relay.rs:152`) and one test-only match
(`src/daemon/relay.rs:1748`). The production path never
instantiates any UDP type.

The orchestrator does pre-allocate a UDP event broadcast
channel (`udp_events_tx` /  `udp_events_rx`,
`src/daemon/relay.rs:778-779`) and the recv-loop subscribes to
it, but the only producer in the codebase is the test-only
`inject_udp_event` method
(`src/daemon/relay.rs:818-820`). In production, that broadcast
channel never receives anything.

The architecture spec is explicit that the live telemetry
stream (`playerStates[]`) arrives over UDP, not TCP
(spec §4.10, §7.7). Without UDP, the operator cannot capture
the stream they came to capture — even the rare TCP
`ServerToClient` records that may arrive once Defect 3 is
fixed are not the live data path.

**Why it shipped.** STEP-12.3 was reported complete on the
strength of unit tests for the `HeartbeatScheduler` and the
test-only `udp_channel_subscriber_logs_inbound_at_debug`
test, which uses `inject_udp_event`. No test exercises the
production wire-up of `TokioUdpTransport` →
`UdpChannel::establish` → orchestrator subscription.

**Required remediation.** After the TCP hello (Defect 3) the
orchestrator should bind a `TokioUdpTransport` to the
chosen UDP server, run `UdpChannel::establish`, subscribe its
`ChannelEvent` broadcast into the orchestrator's
`udp_events_tx`, and apply the resulting time offset to a
shared `WorldTimer`. UDP server selection must consume the
inbound `udpConfigVOD` updates (Defect 5).

### Defect 5. The 1 Hz heartbeat is never spawned in production

**Where it lives.** `src/daemon/relay.rs:165-239` (scheduler
implementation); `src/daemon/relay.rs:664-812` (no
production instantiation).

**What is wrong.** Spec §4.9 and §7.12 require a 1 Hz outbound
`ClientToServer` carrying the watched athlete's `PlayerState`;
without it the Zwift server's liveness model expires the
connection. `HeartbeatScheduler::run`
(`src/daemon/relay.rs:227-238`) exists with full unit-test
coverage (`heartbeat_emits_at_one_hz`,
`heartbeat_increments_seqno_per_send`,
`heartbeat_world_time_tracks_world_timer`,
`src/daemon/relay.rs:1675-1744`), but a search across `src/`
for `HeartbeatScheduler` outside the test module returns no
matches. The production orchestrator never constructs one.

**Why it shipped.** Same pattern as Defect 4: STEP-12.3
acceptance was based on the scheduler's unit tests, not on
end-to-end verification that production spawns the
scheduler.

**Required remediation.** After UDP comes up (Defect 4),
construct a `HeartbeatScheduler` with a `UdpChannel`-backed
`HeartbeatSink`, the shared `WorldTimer`, and the watched
athlete's id, and spawn its `run` future on a tokio task.
Cancel the task as the first step of graceful shutdown
(parent plan acceptance,
`docs/plans/done/STEP-12-game-monitor.md` lines 108-111).

### Defect 6. The TCP channel handle is unreachable after `start`

**Where it lives.** `src/daemon/relay.rs:792-802`.

**What is wrong.** `start_inner` moves the `TcpChannel<T>`
value into the orchestrator's recv-loop spawn:

```rust
let join_handle = tokio::spawn(async move {
    recv_loop(
        channel,
        ...
    ).await
});
```

The spawned `recv_loop` (`src/daemon/relay.rs:1011-1108`) only
reads from event broadcasts; it never calls `send_packet` on
the channel. The channel's `send_packet` method takes
`&self`, but no code outside the spawn holds a reference. So
even if a later step wanted to send a hello (Defect 3) or a
heartbeat (Defect 5), there is nothing to send through.

**Why it shipped.** STEP-12.1 left `RelayRuntime`'s structure
oriented around the inbound-only path because the planned
12.3 work that needed the outbound surface had not happened
yet. Marking 12.3 complete without revisiting the
ownership of `channel` left the gap open.

**Required remediation.** Either wrap the channel in `Arc`
and clone it across the recv-loop spawn and a runtime-held
handle, or introduce an outbound-send mailbox owned by
`RelayRuntime` and drained by a third spawned task that
holds the channel. Either approach makes hello and
heartbeat sends reachable from the runtime's lifetime.

### Defect 7. The relay-session refresh supervisor is never started

**Where it lives.** `src/daemon/relay.rs:983-988`
(uses single-shot `zwift_relay::login`); supervisor is
`crates/zwift-relay/src/session.rs:232-311`.

**What is wrong.** Spec §4.2 and the parent plan
(`docs/plans/done/STEP-12-game-monitor.md` line 71) require a
relay-session refresh supervisor that calls
`/relay/session/refresh` at approximately 90 % of the
session's announced lifetime, falls back to a full re-login
on refresh failure, and surfaces lifecycle events for
downstream consumers. The supervisor exists and is fully
tested as `RelaySessionSupervisor` in
`crates/zwift-relay/src/session.rs`; the production path
never instantiates one. `DefaultSessionLogin::login`
(`src/daemon/relay.rs:983-988`) calls the single-shot
`zwift_relay::login` once at startup. After the announced
expiration elapses, no refresh occurs and the existing
session becomes invalid.

For a long-running daemon the typical session lifetime is
on the order of minutes to tens of minutes, depending on
what Zwift's relay layer reports in `LoginResponse.expiration`.
Without refresh, even a perfectly working orchestrator
(Defects 3, 4, 5, 6 fixed) would silently lose its session
inside the first run and never recover.

**Why it shipped.** STEP-09 left the supervisor as a
self-contained type with its own tests, expecting STEP-12
to wire it into the orchestrator. STEP-12.1 wired only the
single-shot login. The supervisor wiring was implicit in
the parent plan's "in scope" list and never produced a
visible defect during the unit-test runs because no test
runs long enough to cross the refresh deadline.

**Required remediation.** Replace the single-shot
`DefaultSessionLogin` with `RelaySessionSupervisor::start`,
have the orchestrator subscribe to its `SessionEvent`
broadcast, treat `LoginFailed` as a hard error after a
configurable number of consecutive failures, and surface
`Refreshed` records on the `ranchero::relay` tracing target.

### Defect 8. `ResolvedConfig.log_level` is read but never consulted

**Where it lives.** `src/config/mod.rs:419` (resolution);
`src/daemon/runtime.rs:30-67` and `src/daemon/runtime.rs:52`
(consumption — only `LogOpts { verbose, debug }` is forwarded
to `logging::install`); `src/logging/mod.rs:42-55`
(the directive picker only inspects `opts.verbose`,
`opts.debug`, and `RUST_LOG`).

**What is wrong.** The TOML schema supports
`[logging] level = "debug"` (or `trace`, `info`, `warn`,
`error`). `ResolvedConfig::resolve` reads the value into
`ResolvedConfig.log_level`. `daemon::start` and
`runtime::start` then ignore the field. An operator who sets
`level = "trace"` in `~/.config/ranchero/ranchero.toml` and
runs `ranchero start` (without `-v` or `--debug`) gets the
default `warn` directive in the foreground or
`warn,ranchero=info` in the background. There is no log
record indicating the configured level was discarded.

**Why it shipped.** Latent gap from STEP-04. The CLI flag
path (`-v` / `--debug`) was the only consumer of
`LogOpts` when STEP-04 landed, and the TOML field was added
to the schema for completeness without a corresponding
plumbing change. Subsequent steps did not revisit the
logging integration.

**Required remediation.** Extend `LogOpts` (or pass the
resolved level alongside it) so that `filter_directive`
considers the configured level as a base directive when
neither `-v`, `--debug`, nor `RUST_LOG` is set. The CLI
flags must continue to override the TOML setting; `RUST_LOG`
must continue to override everything (per the documented
precedence in `src/logging/mod.rs:30-41`).

### Defect 9. `print_auth_check` reports `Config::default()` URLs, not `cfg.zwift_endpoints`

**Where it lives.** `src/cli.rs:333` (constructs
`Config::default()`); `src/cli.rs:340-343, 409, 414` (prints
those URLs).

**What is wrong.** STEP-12.5 §F made the Zwift HTTPS
endpoints operator-overridable through
`ResolvedConfig.zwift_endpoints` so that an operator could
point ranchero at a staging instance, a self-hosted
`zwift-offline`, or a localhost mock. `RelayRuntime::start`
was updated to consume `cfg.zwift_endpoints`
(`src/daemon/relay.rs:525-530`). `print_auth_check` was
not. It still constructs `zwift_api::Config::default()` and
prints those production URLs, regardless of what the
operator's TOML or environment overrides actually are.

The result is that the pre-flight diagnostic — described in
its own docstring (`src/cli.rs:84-87`) as a way to
"confirm that config + credentials + endpoint configuration
all resolve before risking a real Keycloak round-trip" —
prints a different endpoint configuration than the one the
subsequent `start` will actually use. An operator pointing
at staging would see the production URLs in `auth-check`
and reasonably conclude the override was not picked up,
when in fact it would be honoured by `start`.

**Why it shipped.** STEP-12.5 §F changed
`ResolvedConfig::resolve` and `RelayRuntime::start` but did
not survey other consumers of the auth-host configuration.
`print_auth_check` was added in an earlier step and the §F
work did not revisit it.

**Required remediation.** Replace `Config::default()` in
`print_auth_check` with `zwift_api::Config` constructed
from `resolved.zwift_endpoints`, the same way
`RelayRuntime::start` does it
(`src/daemon/relay.rs:525-530`). The "Endpoints (from …)"
heading line should also be updated to indicate the source
of the values.

### Defect 10. `src/tui/keyring.rs` is a re-export shim

**Where it lives.** `src/tui/keyring.rs` (the entire file is
four lines that re-export `crate::credentials::*`); two
consumers: `src/tui/driver.rs:12`, `tests/tui.rs:5`.

**What is wrong.** The credential storage trait moved from
`crate::tui::keyring` to `crate::credentials` during
STEP-05. The shim was added for backward compatibility with
the two existing consumers. Per the global instruction to
"avoid backwards-compatibility hacks like ... re-exporting
types": the shim should be removed and the consumers
updated to import directly from `crate::credentials`.

**Why it shipped.** STEP-05 moved the trait but left the
shim "for now". Two consumers were updated at the same
time but the shim was never deleted afterwards.

**Required remediation.** Delete `src/tui/keyring.rs`,
remove the `pub mod keyring;` line and the `pub use
keyring::{...}` line from `src/tui/mod.rs:3, 8`, and
change the imports at `src/tui/driver.rs:12` and
`tests/tui.rs:5` to `use crate::credentials::KeyringStore;`
and `use ranchero::credentials::{InMemoryKeyringStore,
KeyringStore};` respectively.

### Observation 1. The monitor account is loaded but never used

Spec §1 describes the **monitor account** as the credential
under which the live relay stream is actually received,
specifically so that ranchero does not impersonate the
rider's own game session. `ResolvedConfig` carries
`monitor_email` and `monitor_password`
(`src/config/mod.rs:317-318`), the configure TUI saves them
to the keychain under the `monitor` role, and the
`ranchero auth-check` output reports them. The orchestrator
ignores them: `RelayRuntime::start_inner` reads only
`cfg.main_email` and `cfg.main_password`
(`src/daemon/relay.rs:678-685`).

This is consistent with the parent plan's narrowing of
STEP-12 to a single account for the connectivity proof, and
the monitor-account integration is reasonably deferred to
STEP-13 or later. It does mean that the TUI prompt and the
keychain entry for the monitor account are operator-visible
configuration that the daemon currently ignores; an operator
could reasonably expect the monitor credential to be in use.
Worth a note in the parent plan or in the configure TUI's
help text.

### Observation 2. `server_bind` / `server_port` / `server_https` have no consumer

`ResolvedConfig` carries `server_bind`, `server_port`, and
`server_https` (`src/config/mod.rs:319-321`), with full TOML
schema and `RANCHERO_SERVER_PORT` / `RANCHERO_SERVER_BIND`
environment overrides
(`src/config/mod.rs:350-364`). No consumer exists in `src/`
outside the test fixtures. The HTTP / WebSocket server is
explicitly STEP-17, so the absence of a consumer today is
expected. As with the monitor account, it is operator-
visible configuration with no current effect; worth a brief
parent-plan note.

### Minor finding 1. `start_inner` step numbering skips 5

The numbered comments in `src/daemon/relay.rs:677-756` go
1, 2, 3, 4, 6, 7, 8 — step 5 is missing. Either step 5 was
removed without renumbering or never existed. Cosmetic, but
characteristic of how the rest of the file landed.
Renumber to 1-7 when the structural changes for Defects 3
through 7 land.

### Minor finding 2. Hardcoded `athlete_id: 0` and `conn_id: 0`

`src/daemon/relay.rs:717-722` builds the `TcpChannelConfig`
with `athlete_id: 0` and `conn_id: 0`. The athlete id should
come from the watched athlete (or, as a placeholder before
STEP-12.5's watched-athlete switching is wired through to
production, from `/api/profiles/me`). The conn id should be
a per-channel counter (spec §4.5; sauce4zwift's
`getConnInc()`). Both are placeholders that work only
because the orchestrator never sends anything (Defects 3
and 5). The first write that goes out under a real Zwift
connection will surface either of these as a server-side
rejection or, worse, as an undiagnosed silent drop. Fix
when the structural changes for Defects 3 through 7 land,
not before — the placeholders are at least visibly
identifiable as placeholders today.

## Updated acceptance criteria

The acceptance criteria recorded earlier in this document
(under §Acceptance criteria for Defects 1 and 2) remain
correct as far as they go, but they describe a successful
start — not a successful operator workflow. The complete
operator-facing acceptance test, against a fully configured
ranchero installation with valid Zwift credentials, is:

```
ranchero start --debug --capture /tmp/x.cap &
sleep 30
ranchero stop
ranchero replay /tmp/x.cap
```

For STEP-12 to be honestly complete, that sequence must:

1. Start the daemon, perform OAuth, perform the relay-session
   login, send a TCP hello, establish the UDP channel with
   SNTP-style time sync, and begin the 1 Hz heartbeat —
   without panic, without error in the configured log file,
   and with the `ranchero::relay` target emitting at INFO at
   least the records `relay.login.ok`,
   `relay.tcp.connecting`, `relay.tcp.established`,
   `relay.udp.established`, and `relay.capture.opened`.

2. Over the 30-second window, accumulate inbound
   `ServerToClient` records in the capture file. The exact
   count depends on the Zwift world the watched athlete is
   in, but at the typical aggregate UDP cadence noted in
   spec §4.10 it should be in the tens to hundreds.

3. Process `ranchero stop` cleanly. Emit `relay.tcp.shutdown`,
   `relay.udp.shutdown`, and `relay.capture.closed`. Remove
   the pidfile and control socket. Exit with status 0.

4. Run `ranchero replay /tmp/x.cap` to completion without
   error, with non-zero counts for at least the inbound UDP
   row.

Defects 1 and 2 prevent the daemon from progressing past
auth. Defects 3 through 7 prevent the operator workflow
above from delivering its outcome even after Defects 1 and
2 are fixed: the file would contain only the format header
and the pidfile-cleanup sequence would still happen, but
nothing in `replay` would justify the operator's effort.
Defects 8 through 10 do not block the workflow but produce
silently wrong or misleading behaviour for an operator who
exercises related configuration paths.

## Updated cross-references

In addition to the cross-references for Defects 1 and 2:

- `docs/plans/done/STEP-12-game-monitor.md` — the parent
  plan. Its sub-step 12.1 acceptance ("TCP hello sent") is
  not satisfied by current production code (Defect 3); its
  sub-step 12.3 acceptance ("UDP heartbeat keeps connection
  alive") is not satisfied (Defects 4 and 5); its sub-step
  12.5 acceptance ("watched-athlete state and idle FSM
  drive runtime") is satisfied only through `#[cfg(test)]`
  injection (Observation in §"The bigger picture" above).
  All four sub-step status notes need correction.
- `docs/plans/done/STEP-09-relay-session.md` — the
  `RelaySessionSupervisor` was specified there for
  consumption by STEP-12; STEP-12 did not consume it
  (Defect 7).
- `docs/plans/done/STEP-10-udp-channel.md` — the UDP
  transport and channel were specified there for
  consumption by STEP-12; STEP-12 did not consume them
  (Defect 4).
- `crates/zwift-relay/src/tcp.rs:163-168` — the docstring
  on `TcpChannel::establish` that names the supervisor as
  responsible for the hello packet (Defect 3).
- `docs/plans/STEP-13-rolling-stats.md` and onward — these
  steps assume `RelayRuntime` reliably emits the
  `GameEvent::PlayerState` records they consume. With
  Defects 3, 4, 5, 7 unresolved, no `PlayerState` records
  are ever emitted in production, so a STEP-13 demo
  against a live Zwift session would observe an empty
  stream regardless of how STEP-13 is implemented.

## Updated honest framing

The original Defects 1 and 2 framed STEP-12 as substantially
complete with two surface-level gaps. The walkthrough
recorded in this addendum shows that STEP-12's
operator-facing contract (sustainable end-to-end
connectivity, capture file populated with live traffic) is
not delivered by the current code at all. The unit-test
suite passes because every sub-step's tests exercise its own
type in isolation through `#[cfg(test)]` injection points,
and no integration test takes the daemon through a real
auth → session → hello → UDP → heartbeat → capture →
inbound sequence end-to-end. The single full-scope test
(`tests/full_scope.rs`) uses bogus credentials against an
unroutable address and is satisfied as long as the
orchestrator opens the capture file before failing — a much
weaker contract than "the capture file contains live
records".

Reporting STEP-12.1, 12.3, 12.4, and 12.5 as complete on
the strength of their unit tests, without ever running the
full operator workflow against a live Zwift session, is
the underlying error this addendum corrects. The next
step is to fix Defects 1 and 2 (which unblock the
credential path), then Defects 3 through 7 (which unblock
the connectivity path), then re-run the operator
acceptance sequence above and only mark STEP-12 complete
when the resulting capture file actually contains live
records.

---

## Tests and implementation plans

This section specifies, for each finding, the red-state
tests that confirm the failure and the implementation
steps that bring those tests to green. Observations and
minor findings have documentation-only or cosmetic-only
plans.

Where one fix is a prerequisite for another, the
dependency is stated at the start of the subsection.
The recommended ordering across all findings is given
at the end.

---

### Defect 1 — tests and implementation

#### Red-state tests

**File:** `tests/daemon_lifecycle.rs` (extend existing
file). All three tests are subprocess tests using the
`assert_cmd` crate.

**Test 1:** `start_exits_nonzero_when_relay_start_fails`

- Arrange: write a minimal TOML config with no
  credentials and `auth_base = "http://127.0.0.1:1"` to
  a temporary file. Construct an `assert_cmd::Command`
  pointing at the `ranchero` binary with
  `start --foreground --config <tmpfile>`.
- Act: execute the command with a 5-second timeout.
- Assert: the command reports failure (non-zero exit
  code).
- Fails before fix: the process enters the degraded-mode
  UDS loop and never exits; the 5-second wait times out.

**Test 2:** `start_removes_pidfile_when_relay_start_fails`

- Arrange: same subprocess setup; add
  `--pidfile <tmpdir>/test.pid`.
- Assert: after the process exits, `test.pid` does not
  exist.
- Fails before fix: process never exits; assertion is
  never reached.

**Test 3:** `start_removes_socket_when_relay_start_fails`

- Arrange: same subprocess setup; the daemon's control
  socket path is under `<tmpdir>/test.sock` (pass via
  config or a dedicated flag if one exists).
- Assert: `test.sock` does not exist after exit.

#### Implementation steps

1. `src/daemon/runtime.rs:243-253` — remove the `match`
   block and replace it with a propagating error chain
   that preserves the log record:

   ```rust
   let runtime = super::relay::RelayRuntime::start(&cfg, capture_path)
       .await
       .inspect_err(|e| {
           tracing::error!(
               target: "ranchero::relay",
               error = %e,
               "relay.start.failed",
           );
       })
       .map_err(io::Error::other)?;
   ```

2. Change the type of `runtime` in the remainder of
   `run_daemon` from `Option<RelayRuntime>` to
   `RelayRuntime`. Replace the `if let Some(runtime) =
   runtime {` block at line 272 with direct usage:

   ```rust
   runtime.shutdown();
   if let Err(e) = runtime.join().await {
       tracing::warn!(
           target: "ranchero::relay",
           error = %e,
           "relay.join.error",
       );
   }
   ```

3. Remove the comment block at lines 236-242 that
   describes the degraded-mode rationale from
   STEP-12.5 §D. It is retracted by this document.

4. No change is required in `start()` at lines 30-79:
   `run_daemon` already returns `io::Result<()>`, and
   `start()` already calls `result?` at line 78.
   When `run_daemon` returns `Err`, the `?` propagates
   a `DaemonError::Io`, the pidfile is removed at
   line 71, and the socket is removed at line 72 before
   `result?` evaluates — cleanup already runs on the
   error path.

#### Green-state verification

All three subprocess tests exit within 5 seconds with
non-zero status. The pidfile and socket paths are absent
after exit. The `relay.start.failed` error record is
present in the process's log output.

---

### Defect 2 — tests and implementation

#### Red-state tests

**File:** inline `mod tests` block in
`src/config/mod.rs`, or a new `tests/config_resolution.rs`.

**Test 1:** `resolve_consults_keyring_for_absent_main_password`

```rust
let mut keyring = InMemoryKeyringStore::default();
keyring.set("main", KeyringEntry {
    username: "rider@example.com".into(),
    password: "keyring-secret".into(),
}).unwrap();
let cli = GlobalOpts {
    main_email: Some("rider@example.com".into()),
    ..Default::default()
};
let r = ResolvedConfig::resolve(
    &cli, &TestEnv::empty(), &keyring, None
).unwrap();
assert_eq!(r.main_password.unwrap().expose(), "keyring-secret");
```

Fails before fix: `resolve` takes three arguments; the
call does not compile.

**Test 2:** `resolve_cli_main_password_takes_precedence_over_keyring`

- Arrange: keyring has `"keyring-secret"` for `"main"`;
  `cli.mainpassword = Some("cli-secret".into())`.
- Assert: `r.main_password.unwrap().expose() == "cli-secret"`.

**Test 3:** `resolve_propagates_keyring_backend_error`

- Arrange: a `FailingKeyringStore` stub that always
  returns `Err(KeyringError::Backend("injected".into()))`.
- Assert: `resolve` returns `Err(ConfigError::KeyringError(_))`.

**Test 4:** `resolve_absent_keyring_entry_yields_none_password`

- Arrange: empty `InMemoryKeyringStore::default()`; no
  CLI flag.
- Assert: `r.main_password.is_none()`.

#### Implementation steps

1. `src/config/mod.rs` — add a variant to `ConfigError`:

   ```rust
   KeyringError(String),
   ```

   Add to the `Display` implementation:
   `"keyring error: {0}"`.

2. `src/config/mod.rs` — change the
   `ResolvedConfig::resolve` signature to:

   ```rust
   pub fn resolve(
       cli:     &GlobalOpts,
       env:     &dyn Env,
       keyring: &dyn crate::credentials::KeyringStore,
       file:    Option<ConfigFile>,
   ) -> Result<Self, ConfigError>
   ```

3. `src/config/mod.rs:370-378` — replace the password
   blocks:

   ```rust
   let main_password = match cli.mainpassword.clone() {
       Some(p) => Some(RedactedString::new(p)),
       None => match keyring.get("main") {
           Ok(Some(e)) => Some(RedactedString::new(e.password)),
           Ok(None)    => None,
           Err(e)      => return Err(ConfigError::KeyringError(e.to_string())),
       },
   };
   let monitor_password = match cli.monitorpassword.clone() {
       Some(p) => Some(RedactedString::new(p)),
       None => match keyring.get("monitor") {
           Ok(Some(e)) => Some(RedactedString::new(e.password)),
           Ok(None)    => None,
           Err(e)      => return Err(ConfigError::KeyringError(e.to_string())),
       },
   };
   ```

4. `src/cli.rs:195-220` (the `Start | Stop | Status`
   arm) — construct the keyring before calling `resolve`:

   ```rust
   let keyring = OsKeyringStore::new();
   let resolved = ResolvedConfig::resolve(
       &cli.global, &OsEnv, &keyring, Some(file)
   )?;
   ```

5. `src/cli.rs:221-227` (the `AuthCheck` arm) — the arm
   already creates a `keyring` after `resolve`. Move
   keyring construction before `resolve` and pass it:

   ```rust
   let keyring = OsKeyringStore::new();
   let resolved = ResolvedConfig::resolve(
       &cli.global, &OsEnv, &keyring, Some(file)
   )?;
   print_auth_check(&resolved, &keyring);
   ```

6. All test call sites that call `resolve` directly
   (in `tests/` and inline test modules) — add
   `&InMemoryKeyringStore::default()` as the third
   argument.

#### Green-state verification

All four new tests pass. All existing `resolve`-based
tests compile and pass after the call-site update. After
Defect 1 is also applied, the subprocess test for
Defect 1 progresses past the credential check and fails
at the network layer instead of the keychain layer.

---

### Defect 10 — tests and implementation

(Trivial structural change; fix early to reduce noise
in later compile cycles.)

#### Red-state "test"

After deleting `src/tui/keyring.rs`, `cargo build`
fails with two unresolved-import errors:

- `src/tui/driver.rs:12`: `use crate::tui::keyring::KeyringStore`
- `tests/tui.rs:5`: `use ranchero::tui::keyring::{...}`

These compile errors confirm the shim is load-bearing
and that consumers must be updated.

#### Implementation steps

1. Delete `src/tui/keyring.rs`.

2. `src/tui/mod.rs:3` — remove `pub mod keyring;`.

3. `src/tui/mod.rs:8` — change:

   ```rust
   pub use keyring::{InMemoryKeyringStore, KeyringStore};
   ```

   to:

   ```rust
   pub use crate::credentials::{InMemoryKeyringStore, KeyringStore};
   ```

4. `src/tui/driver.rs:12` — change:

   ```rust
   use crate::tui::keyring::KeyringStore;
   ```

   to:

   ```rust
   use crate::credentials::KeyringStore;
   ```

5. `tests/tui.rs:5` — change:

   ```rust
   use ranchero::tui::keyring::{InMemoryKeyringStore, KeyringStore};
   ```

   to:

   ```rust
   use ranchero::credentials::{InMemoryKeyringStore, KeyringStore};
   ```

#### Green-state verification

`cargo build` succeeds.
`cargo test -p ranchero -- tui` passes.

---

### Defect 9 — tests and implementation

#### Red-state test

**File:** inline `#[test]` in `src/cli.rs` test module,
or a new `tests/auth_check.rs`.

**Test:** `auth_check_reports_configured_endpoints_not_defaults`

Because `print_auth_check` currently writes directly to
stdout via `println!`, this test requires either
refactoring the function to accept a `&mut dyn Write`
parameter (the pattern already used by `print_follow_to`)
or using a stdout-capture helper. The writer approach is
cleaner.

```rust
let mut resolved = make_config("r@e.com", "secret");
resolved.zwift_endpoints = ZwiftEndpoints {
    auth_base: "http://auth.staging.example.com".into(),
    api_base:  "http://api.staging.example.com".into(),
};
let mut buf = Vec::<u8>::new();
print_auth_check_to(&mut buf, &resolved, &InMemoryKeyringStore::default());
let output = String::from_utf8(buf).unwrap();
assert!(
    output.contains("http://auth.staging.example.com"),
    "auth-check must print the configured auth endpoint",
);
assert!(
    !output.contains("auth.zwift.com"),
    "auth-check must not print the default production URL",
);
```

Fails before fix: output contains the hardcoded
`Config::default()` URL regardless of what
`resolved.zwift_endpoints` contains.

#### Implementation steps

1. `src/cli.rs:327` — rename `print_auth_check` to
   `print_auth_check_to` and add a writer parameter:

   ```rust
   fn print_auth_check_to<W: std::io::Write>(
       out:      &mut W,
       resolved: &crate::config::ResolvedConfig,
       keyring:  &dyn crate::credentials::KeyringStore,
   ) -> std::io::Result<()>
   ```

   Replace all `println!(...)` calls with
   `writeln!(out, ...)?`. Add a thin
   `print_auth_check` wrapper that calls
   `print_auth_check_to(&mut std::io::stdout(), ...)`.

2. `src/cli.rs:333` — replace:

   ```rust
   let cfg = Config::default();
   ```

   with:

   ```rust
   let cfg = zwift_api::Config {
       auth_base:  resolved.zwift_endpoints.auth_base.clone(),
       api_base:   resolved.zwift_endpoints.api_base.clone(),
       source:     zwift_api::DEFAULT_SOURCE.to_string(),
       user_agent: zwift_api::DEFAULT_USER_AGENT.to_string(),
   };
   ```

   This mirrors the construction at
   `src/daemon/relay.rs:525-530`.

3. `src/cli.rs:340` — update the heading line from:

   ```
   "Endpoints (from zwift_api::Config::default()):"
   ```

   to:

   ```
   "Endpoints (from config):"
   ```

4. `src/cli.rs:225` — update the dispatch call site to
   pass `&mut std::io::stdout()`.

#### Green-state verification

The new test passes. Running `ranchero auth-check` with
`RANCHERO_ZWIFT_AUTH_BASE=http://localhost:9999` prints
`http://localhost:9999` in the endpoints section. The
existing `auth-check` output format is otherwise
unchanged.

---

### Defect 8 — tests and implementation

#### Red-state tests

**File:** `src/logging/mod.rs` inline `mod tests` block
(extend the existing test module).

**Test 1:** `filter_directive_uses_configured_level_when_no_cli_flags`

```rust
let dir = filter_directive(
    LogOpts::default(), true, None, Some(LogLevel::Debug)
);
assert!(
    dir.contains("debug"),
    "configured log level must appear when no CLI flags are set, got {dir:?}",
);
```

Fails before fix: `filter_directive` takes three
arguments; the call does not compile.

**Test 2:** `filter_directive_verbose_overrides_configured_level`

```rust
let dir = filter_directive(
    LogOpts { verbose: true, ..Default::default() },
    true, None, Some(LogLevel::Warn),
);
assert!(
    dir.contains("ranchero=info"),
    "verbose flag must override configured level, got {dir:?}",
);
```

**Test 3:** `filter_directive_debug_overrides_configured_level`

```rust
let dir = filter_directive(
    LogOpts { debug: true, ..Default::default() },
    true, None, Some(LogLevel::Warn),
);
assert!(
    dir.contains("ranchero=debug"),
    "debug flag must override configured level, got {dir:?}",
);
```

**Test 4:** `filter_directive_rust_log_overrides_configured_level`

```rust
let dir = filter_directive(
    LogOpts::default(), true, Some("error"), Some(LogLevel::Trace),
);
assert_eq!(dir, "error");
```

#### Implementation steps

1. `src/logging/mod.rs:42` — change the
   `filter_directive` signature to:

   ```rust
   pub fn filter_directive(
       opts:             LogOpts,
       foreground:       bool,
       rust_log:         Option<&str>,
       configured_level: Option<crate::config::LogLevel>,
   ) -> String
   ```

2. In the body, add a branch for the configured level
   after the `opts.verbose || !foreground` branch:

   ```rust
   if opts.debug {
       "info,ranchero=debug".to_string()
   } else if opts.verbose || !foreground {
       "warn,ranchero=info".to_string()
   } else if let Some(level) = configured_level {
       format!("warn,ranchero={level}")
   } else {
       "warn".to_string()
   }
   ```

   `LogLevel` already implements `Display` with
   lowercase values (`trace`, `debug`, `info`, `warn`,
   `error`), so the format string works without an
   additional helper.

3. `src/logging/mod.rs:84` — change the `install`
   signature to:

   ```rust
   pub fn install(
       opts:             LogOpts,
       foreground:       bool,
       log_file:         &Path,
       configured_level: Option<crate::config::LogLevel>,
   ) -> io::Result<WorkerGuard>
   ```

   Update the internal `filter_directive` call at
   line 88 to pass `configured_level`.

4. `src/daemon/runtime.rs:52` — update the
   `logging::install` call to pass the configured level:

   ```rust
   let _log_guard = logging::install(
       log_opts, foreground, &cfg.log_file, Some(cfg.log_level)
   )?;
   ```

5. Update all existing callers of `filter_directive`
   in the test module to pass `None` as the fourth
   argument.

#### Green-state verification

All four new tests pass. All existing `filter_directive`
tests pass after the call-site update. Running
`ranchero start` with `[logging] level = "debug"` in
TOML and no `-v` flag produces records at the
`ranchero=debug` level.

---

### Defect 6 — tests and implementation

(Prerequisite for Defects 3, 4, and 5.)

#### Red-state test

**File:** `tests/relay_runtime.rs` (extend existing
file).

Extend `NoopTcpTransport` to record `write_all` calls:

```rust
struct RecordingTcpTransport {
    written: Arc<StdMutex<Vec<Vec<u8>>>>,
}
impl zwift_relay::TcpTransport for RecordingTcpTransport {
    async fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.written.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }
    async fn read_chunk(&self) -> io::Result<Vec<u8>> {
        std::future::pending::<()>().await;
        unreachable!()
    }
}
```

**Test:** `relay_runtime_exposes_outbound_tcp_send_path_after_start`

- Arrange: a `RecordingTcpFactory` that vends a
  `RecordingTcpTransport`.
- Act: call `RelayRuntime::start_with_deps`, then call
  a `send_tcp(bytes)` method on the returned runtime.
- Assert: `RecordingTcpTransport.written` contains the
  supplied bytes.
- Fails before fix: `RelayRuntime` exposes no `send_tcp`
  method; the call does not compile.

#### Implementation steps

1. `src/daemon/relay.rs` — locate the `RelayRuntime`
   struct. Add a field for a shared channel handle:

   ```rust
   tcp_channel: Arc<TcpChannel<T>>,
   ```

   where `T` is the transport type parameter on
   `RelayRuntime`.

2. In `start_inner`, before spawning the recv-loop at
   line 792, wrap the channel in `Arc` and clone it:

   ```rust
   let channel = Arc::new(channel);
   let channel_for_recv = Arc::clone(&channel);
   let join_handle = tokio::spawn(async move {
       recv_loop(channel_for_recv, ...).await
   });
   ```

   Store the original `channel` (the `Arc`) on
   `RelayRuntime`.

3. Update `recv_loop` at line 1011 to accept
   `Arc<TcpChannel<T>>` instead of `TcpChannel<T>`.

4. Expose a `send_tcp` method on `RelayRuntime`:

   ```rust
   pub async fn send_tcp(
       &self, payload: &[u8], hello: bool,
   ) -> io::Result<()> {
       self.tcp_channel.send_packet(payload, hello).await
   }
   ```

5. Verify that `TcpChannel<T>` is `Sync` when `T:
   Sync`. If `T` is currently only bounded by `Send`,
   add `+ Sync` to the relevant bounds in
   `crates/zwift-relay/src/tcp.rs` so `Arc<TcpChannel<T>>`
   can be held across async boundaries.

#### Green-state verification

The new test compiles and passes. The `RecordingTcpTransport`
receives exactly the bytes supplied to `send_tcp`. All
existing relay runtime tests continue to pass.

---

### Defect 3 — tests and implementation

(Requires Defect 6 resolved first.)

#### Red-state test

**File:** `tests/relay_runtime.rs`.

**Test:** `relay_runtime_sends_tcp_hello_after_established`

Use the `RecordingTcpTransport` infrastructure from
Defect 6. After `start_with_deps` returns, shut down
the runtime and inspect the recorded writes.

Assert:
- `written` is non-empty (at least one `write_all`
  call was made before shutdown).
- The first recorded bytes decode as a
  `zwift_relay::ClientToServer` (via
  `prost::Message::decode`) with `hello == true`.

Fails before fix: `written` is empty; no bytes are
written to the TCP channel before or after `start`.

#### Implementation steps

1. `src/daemon/relay.rs` — after the `Established`
   event wait (currently around line 756) and before
   the `Arc` wrapping from Defect 6, build and send
   the hello packet:

   ```rust
   // Step 5: identify this connection to the relay server.
   use prost::Message as _;
   let hello_payload = zwift_relay::ClientToServer {
       relay_id: session.relay_id as u32,
       conn_id:  tcp_config.conn_id,
       seqno:    1,
       ..Default::default()
   }
   .encode_to_vec();
   channel
       .send_packet(&hello_payload, true)
       .await
       .map_err(RelayRuntimeError::Io)?;
   tracing::info!(target: "ranchero::relay", "relay.tcp.hello.sent");
   ```

   The ordering must be: hello send → `Arc::new(channel)`
   → recv-loop spawn. The channel value is owned (not
   yet wrapped in `Arc`) at this point, so `send_packet`
   is called directly on the owned value before wrapping.

2. Apply Minor finding 1 simultaneously: renumber the
   step comments in `start_inner` to 1-7 after the
   hello-send step occupies the previously missing
   step 5.

#### Green-state verification

`written` contains at least one entry. The first entry
decodes as `ClientToServer` with `hello == true`. The
`relay.tcp.hello.sent` record appears in the traced
test run.

---

### Defect 4 — tests and implementation

(Requires Defect 3 resolved first.)

#### Red-state tests

**File:** `tests/relay_runtime.rs`.

**Test 1:** `relay_runtime_connects_udp_transport_after_tcp_hello`

Introduce `UdpTransportFactory` and a `StubUdpFactory`:

```rust
pub trait UdpTransportFactory: Send + Sync + 'static {
    type Transport: zwift_relay::UdpTransport;
    fn connect(
        &self,
        addr: std::net::SocketAddr,
    ) -> impl Future<Output = io::Result<Self::Transport>> + Send;
}

struct StubUdpFactory {
    connected: Arc<StdMutex<bool>>,
}
```

After `start_with_deps` (extended to accept a UDP
factory), assert `*connected.lock().unwrap() == true`.

Fails before fix: `UdpTransportFactory` does not exist;
`start_with_deps` accepts no UDP factory argument.

**Test 2:** `relay_runtime_logs_udp_established_at_info`

After `start_with_deps` and shutdown:

```rust
assert!(
    tracing_test::internal::logs_with_scope_contain(
        "ranchero", "relay.udp.established"
    ),
    "expected relay.udp.established at INFO",
);
```

Fails before fix: log record is never emitted.

#### Implementation steps

1. `src/daemon/relay.rs` — add the `UdpTransportFactory`
   trait (public, analogous to `TcpTransportFactory`).

2. Add `TokioUdpTransportFactory` as the production
   implementation, constructing
   `zwift_relay::TokioUdpTransport`.

3. In `start_inner`, after the TCP hello, select the
   UDP server, connect, establish the channel, and
   subscribe its events to `udp_events_tx`:

   ```rust
   // Step 6: establish the UDP channel.
   let udp_addr = find_best_udp_server(&session)
       .ok_or(RelayRuntimeError::NoUdpServer)?;
   let udp_transport = udp_factory.connect(udp_addr)
       .await
       .map_err(RelayRuntimeError::Io)?;
   let udp_channel = zwift_relay::UdpChannel::establish(
       udp_transport, udp_config,
   )
   .await
   .map_err(RelayRuntimeError::Udp)?;
   tracing::info!(target: "ranchero::relay", "relay.udp.established");
   let mut udp_events = udp_channel.events();
   let udp_tx = udp_events_tx.clone();
   tokio::spawn(async move {
       while let Ok(ev) = udp_events.recv().await {
           let _ = udp_tx.send(ev);
       }
   });
   ```

4. Extend `RelayRuntime::start_with_deps` and
   `start_with_deps_and_writer` to accept a
   `udp_factory: impl UdpTransportFactory` parameter.

5. `RelayRuntime::start` (the production entry at
   line 521) — construct `TokioUdpTransportFactory`
   and pass it.

#### Green-state verification

Both new tests pass. The `StubUdpFactory` records
one `connect` call. The `relay.udp.established` record
appears. Existing tests pass after the `start_with_deps`
call-site updates.

---

### Defect 5 — tests and implementation

(Requires Defect 4 resolved first.)

#### Red-state test

**File:** `tests/relay_runtime.rs`.

**Test:** `relay_runtime_sends_udp_heartbeat_at_one_hz_after_udp_established`

Extend `StubUdpTransport` to record datagrams passed
to `send_to`. Use `tokio::time::pause()` in the test
and `tokio::time::advance(Duration::from_secs(2))`
after `start_with_deps`.

Assert: at least one datagram was recorded within the
simulated 2-second window.

Fails before fix: no datagrams are recorded;
`HeartbeatScheduler` is never constructed.

#### Implementation steps

1. `src/daemon/relay.rs` — after `UdpChannel::establish`,
   construct and spawn the heartbeat:

   ```rust
   // Step 7: start the 1 Hz heartbeat.
   let heartbeat = HeartbeatScheduler::new(
       udp_channel.sink(),
       world_timer.clone(),
       watched_state.athlete_id(),
   );
   let heartbeat_handle = tokio::spawn(heartbeat.run());
   tracing::info!(target: "ranchero::relay", "relay.heartbeat.started");
   ```

2. Store `heartbeat_handle` on `RelayRuntime`. In
   `shutdown()`, abort the heartbeat task before issuing
   the channel shutdown:

   ```rust
   self.heartbeat_handle.abort();
   self.inner.shutdown_tx.send(()).ok();
   ```

#### Green-state verification

The recording transport receives at least one datagram
within 2 simulated seconds. The `relay.heartbeat.started`
record appears. The existing
`heartbeat_emits_at_one_hz` unit test continues to
pass unmodified.

---

### Defect 7 — tests and implementation

(Requires Defect 2 resolved; independent of Defects 3-6.)

#### Red-state tests

**File:** `tests/relay_runtime.rs`.

The current `SessionLogin` trait returns a single
`RelaySession` (single-shot). The supervisor is
long-running and emits a `SessionEvent` broadcast.
Wiring it in requires either extending the existing
trait to also return an event receiver, or replacing
the trait with a supervisor-shaped factory. The
supervisor-factory approach is cleaner and matches
the existing `TcpTransportFactory` pattern.

**Test 1:** `relay_runtime_produces_session_established_event_on_start`

Introduce a `StubSupervisorFactory` that returns a
stub supervisor emitting `SessionEvent::Established`
immediately. After `start_with_deps`, subscribe to
the supervisor's event receiver and assert at least
one `SessionEvent::Established` was produced.

Fails before fix: no supervisor factory trait exists;
`start_with_deps` accepts a `SessionLogin` (single-shot)
only.

**Test 2:** `relay_runtime_logs_session_refreshed_at_info`

Inject a stub supervisor that emits
`SessionEvent::Refreshed` immediately after start.

```rust
assert!(
    tracing_test::internal::logs_with_scope_contain(
        "ranchero", "relay.session.refreshed"
    ),
    "expected relay.session.refreshed at INFO",
);
```

Fails before fix: no supervisor subscription exists.

#### Implementation steps

1. `src/daemon/relay.rs` — introduce a
   `RelaySessionSupervisorFactory` trait (or a thin
   wrapper that delegates to
   `crates/zwift-relay/src/session.rs::RelaySessionSupervisor`):

   ```rust
   pub trait SessionSupervisorFactory: Send + Sync + 'static {
       fn start(
           &self,
       ) -> impl Future<
           Output = Result<zwift_relay::RelaySessionSupervisor, zwift_relay::SessionError>
       > + Send;
   }
   ```

2. Replace `DefaultSessionLogin` with
   `DefaultSessionSupervisorFactory`, whose `start()`
   calls `RelaySessionSupervisor::start(http_client, ...)`.

3. In `start_inner`, after session login, subscribe to
   the supervisor's event broadcast:

   ```rust
   let supervisor = session_factory.start().await
       .map_err(RelayRuntimeError::Session)?;
   let initial_session = supervisor.current().await;
   let mut session_events = supervisor.events();
   tokio::spawn(async move {
       while let Ok(event) = session_events.recv().await {
           match event {
               SessionEvent::Refreshed => tracing::info!(
                   target: "ranchero::relay",
                   "relay.session.refreshed",
               ),
               SessionEvent::LoginFailed => tracing::error!(
                   target: "ranchero::relay",
                   "relay.session.login_failed",
               ),
               _ => {}
           }
       }
   });
   ```

4. Store the supervisor handle on `RelayRuntime`. In
   `shutdown()`, call `supervisor.shutdown()` before
   aborting the heartbeat and issuing channel shutdown.

5. Extend `start_with_deps` to accept the new
   `SessionSupervisorFactory` parameter instead of the
   `SessionLogin` parameter.

#### Green-state verification

Both new tests pass. Long-running manual sessions no
longer lose their relay session silently at the expiry
boundary; the `relay.session.refreshed` record appears
in the log at each refresh cycle.

---

### Observation 1 — documentation plan

No code change required.

1. `docs/plans/done/STEP-12-game-monitor.md` — add a
   note under the "in scope" list: the monitor account
   is stored by the configure TUI and surfaced by
   `auth-check`, but the orchestrator currently uses
   only the main account for both authentication and
   stream reception. The monitor account will be
   activated in STEP-13.

2. The configure TUI prompt for the monitor account
   (`src/tui/model.rs` or the prompt text therein) —
   add a parenthetical: "(for future use — not
   consumed in the current release)".

---

### Observation 2 — documentation plan

No code change required.

1. `docs/plans/done/STEP-12-game-monitor.md` — add a
   note that `server_bind`, `server_port`, and
   `server_https` are schema-resident but have no
   consumer until STEP-17 introduces the HTTP /
   WebSocket server.

---

### Minor finding 1 — step renumbering plan

No test required.

When the structural changes for Defects 3 through 7
are applied, renumber the comment markers in
`start_inner` (`src/daemon/relay.rs:677-756`) from
`1, 2, 3, 4, 6, 7, 8` to a consecutive sequence
reflecting the new step order (the previously missing
step 5 is occupied by the hello-send step added by
Defect 3). Apply this during the Defect 3 edit so
the renumbering is a single coherent change.

---

### Minor finding 2 — placeholder values plan

No standalone test required; both values are verified
indirectly by the hello-packet test for Defect 3.

Defer filling in the real values until Defects 3
through 7 are resolved, at which point the hello
packet is actually transmitted and a bad value would
produce a server-side rejection.

1. `conn_id`: introduce a per-runtime atomic counter,
   incremented each time a new channel is opened.
   Starting value 1. Wire it into `TcpChannelConfig`
   when constructing the hello packet.

2. `athlete_id`: obtain it from the `/api/profiles/me`
   response during the auth step. Until that call is
   added, retain 0 but replace the silent zero with an
   explicit comment:
   `// placeholder: populate from /api/profiles/me`

---

### Recommended implementation order

Apply fixes in this sequence to minimise compilation
failures between steps:

| Step | Finding | Rationale |
|---|---|---|
| 1 | Defect 10 | Trivial shim removal; no runtime-behaviour impact |
| 2 | Defect 9 | One-line URL fix; no interaction with other defects |
| 3 | Defect 8 | Extends `filter_directive` and `install` signatures before other callers are modified |
| 4 | Defect 2 | Threads the keyring through `resolve`; modifies all `resolve` call sites |
| 5 | Defect 1 | Propagates error instead of swallowing; requires Defect 2 so the credential path reaches the network |
| 6 | Defect 7 | Session supervisor wiring; independent of the TCP/UDP connectivity changes |
| 7 | Defect 6 | Wraps the TCP channel in `Arc`; structural prerequisite for Defect 3 |
| 8 | Defect 3 | TCP hello send; requires Defect 6 |
| 9 | Defect 4 | UDP channel construction; requires Defect 3 |
| 10 | Defect 5 | Heartbeat spawn; requires Defect 4 |
| 11 | Minor 1 | Renumber step comments; apply during Defect 3 edit |
| 12 | Minor 2 | Fill `athlete_id` / `conn_id`; defer until Defect 3 is applied |
| 13 | Obs. 1, 2 | Documentation only; no ordering constraint |
