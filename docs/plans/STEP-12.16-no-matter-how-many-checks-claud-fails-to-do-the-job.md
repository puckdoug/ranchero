# STEP-12.16 — Smoke test resilience: course gate, reconnect, and timeout budgets

**Status:** review (2026-05-08).

After STEP-12.15 was completed, a real-world smoke test against a
healthy live account failed:

```
$ ranchero start --debug --capture output.cap
…
relay.auth.token.granted
relay.session.login.ok
relay.course_gate.suspended watched_athlete_id=550564
relay.start.failed error=watched athlete is not in a game (no course); waiting to resume
ranchero stopped
```

A subsequent third-pass audit against sauce4zwift's source revealed
that the course-gate defect is the most visible failure but **not
the only smoke-test risk**. Three additional behaviours diverge from
the reference, all relevant to the goal that the smoke run
"indefinitely without failure":

1. **Course gate aborts instead of suspending** when the watched
   athlete is not in a game (defect F6).
2. **Mid-session TCP shutdown exits the daemon** with no reconnect
   (defect F7); sauce schedules a full reconnect on every TCP
   shutdown.
3. **Startup handshake timeouts are six times tighter than the
   reference** (defect F8): ranchero gives Established 5 s and
   `udp_config` another 5 s, both as separate hard deadlines that
   exit on expiry; sauce wraps both in a single 30 s race that
   triggers a reconnect on expiry.

The smoke as written cannot run "indefinitely" while any of these
three remain. F6 fails the very first start when the watched athlete
is offline. F7 ends the run on the first TCP blip. F8 turns a
slow-server hiccup into a daemon exit instead of a retry.

A fourth class of differences — sauce retries indefinitely on auth
or session errors, retries on UDP connect errors, and pins the TCP
server choice across reconnects — is operationally important but
not smoke-blocking on a healthy first run. Those are catalogued in
section 6 as deferred follow-ups, not phases of this plan.

## 1. Reference behaviour (sauce4zwift)

Citations are against
`/Users/doug/Development/Zwift/sauce4zwift/src/zwift.mjs`.

### 1.1 Course gate

- **613-622** — `getPlayerState()` returns `null` on 404; no
  exception.
- **1706-1716** — `initPlayerState()` stores
  `s ? s.courseId : null`. A null courseId is a valid startup state.
- **1917-1922** — UDP channel setup is gated on
  `!this.suspended && this.courseId`. When `courseId` is null at
  startup, sauce logs `"User not in game: waiting for activity..."`
  and calls `this.suspend()`. The TCP relay session continues.
- **1973-1995** — `suspend()` and `resume()` are first-class
  methods, not error states. `resume()` calls `setUDPChannel()`
  once a course becomes available.
- **2031-2056** — `_refreshSelfState()` polls `getPlayerState()`
  on a self-tuning interval. On receipt of a state with a course,
  it transitions out of the suspended state.
- **2236-2237** — inbound TCP self-state also transitions out of
  the suspended state via `if (this.suspended) { this.resume(); }`.

### 1.2 Mid-session TCP shutdown

- **1869-1874** — `onTCPChannelShutdown(ch)` is invoked when the
  TCP channel emits `shutdown`. If the channel is the active
  session's channel and the daemon is not stopping, sauce calls
  `_schedConnectRetry()`.
- **1876-1883** — `_schedConnectRetry()` clears the previous
  retry timer, calls `disconnect()`, computes
  `delay = max(1000, 1000 * 1.2^backoffCount - elapsed)`, and
  schedules `connect()` again. There is no upper bound on retry
  attempts.

### 1.3 Startup handshake timeouts

- **1885-1923** — `activateSession()` constructs a single
  shutdown-or-30-second-timeout error promise (line 1888). Both
  the TCP hello (`sendPacket`) and the UDP-server-pools wait
  race against the same error promise. If either takes longer
  than 30 s, the session activation rejects and bubbles up to
  `_schedConnectRetry()` for a retry, not an exit.
- The udp_server_pools wait is satisfied by the
  `udpServerPoolsUpdated` event emitted from `onInPacket()` when
  a `udpConfigVOD` push arrives (line 2154-2166).

## 2. Current ranchero behaviour

### 2.1 Course gate (F6)

`src/daemon/relay.rs:1544-1553` returns
`RelayRuntimeError::WatchedAthleteNotInGame` as a fatal error when
`state.world` is `None`. The error variant's own message at
`:218-223` says "watched athlete is not in a game (no course);
waiting to resume" — but no waiting actually occurs. The retry
wrapper at `:1241` only retries `TcpConnect` failures, so the
error propagates to the orchestrator and the daemon exits.

The supporting `run_state_refresher` machinery (`:537-594`) and the
`inner.suspended` flag (`:1571`) both already exist; they are
wired for the mid-session-idle case (no inbound self-state for
15 s) but not for the at-startup case.

### 2.2 Mid-session TCP shutdown (F7)

`src/daemon/relay.rs:2630-2633` (the `recv_loop`'s handling of
`TcpChannelEvent::Shutdown`) returns `Ok(())` and the recv-loop
exits. The orchestrator's `join_handle` completes; the daemon
process terminates. There is no reconnect path. The supervisor at
`crates/zwift-relay/src/session.rs` handles relay-session refresh
and re-login, but a TCP-channel shutdown is not in its scope.

### 2.3 Startup handshake timeouts (F8)

- `src/daemon/relay.rs:1763` — `established_deadline = 5 s`. On
  expiry the runtime returns `EstablishedTimeout` and exits.
- `src/daemon/relay.rs:1818` — `udp_config_deadline = 5 s`,
  separate from the above. On expiry returns `NoUdpConfig` and
  exits.

Combined budget: 10 s split across two non-retryable gates. Sauce's
budget: 30 s in a single race that retries on expiry.

## 3. Why this matters for the smoke

`ranchero start --capture output.cap; sleep 5; ranchero follow output.cap`,
expected to run indefinitely:

- **F6** fails the first start when the watched athlete is not in
  Zwift at the moment the command runs. This is the observed
  failure mode.
- **F7** ends the run on the first TCP blip. Zwift's relay servers
  are not infinitely stable; even a brief network hiccup or a
  server-side rotation will close the TCP session. Sauce
  reconnects; ranchero exits.
- **F8** ends the run if the relay takes more than 5 s to send
  Established or `udp_config`. On a healthy day this is unlikely;
  on a slow day it converts a transient slow start into a
  permanent failure.

## 4. Implementation plan

Each phase is a TDD pair: `Na` writes failing tests against the
contract; `Nb` is the implementation that makes them pass.

### Phase 1 — F6 Phase A: replace the fatal error with a suspended start

- [x] **1a** — Tests added to `tests/relay_runtime.rs` (the
      orchestrator-level harness already has the stub DI
      infrastructure for this scenario):
  - `start_with_watched_athlete_not_in_game_starts_suspended` —
    new `WatchedAthleteOfflineAuth` stub returns `Ok(Some(state))`
    with `state.world = None`. Asserts the runtime returns
    `Ok(_)` and the trace contains
    `relay.runtime.suspended_no_course`. Currently FAILS with
    `WatchedAthleteNotInGame`.
  - `start_with_watched_athlete_not_logged_in_starts_suspended` —
    new `WatchedAthleteNoStateAuth` stub returns `Ok(None)`
    (sauce's 404 case at `zwift.mjs:613-622`). Same assertions.
    Currently FAILS with `WatchedAthleteNotInGame`.
  - `start_with_watched_athlete_in_game_proceeds_normally` —
    existing `StubAuth` (returns `world = Some(1)`); asserts
    startup succeeds AND the new
    `relay.runtime.suspended_no_course` trace does NOT fire on
    the happy path. Currently passes (the trace event does not
    yet exist, so the negative assertion holds).
  - The `inner.suspended is true` check from the original
    contract was deferred from the test layer to the trace
    assertion: the lifecycle event is the public-observable
    contract, and adding a `pub fn is_suspended()` accessor on
    `RelayRuntime` is implementation work that belongs in 1b.
- [x] **1b** — Implementation:
  - `RelayRuntimeError::WatchedAthleteNotInGame` removed.
  - Course gate emits `relay.runtime.suspended_no_course` on the
    `None` branch and continues; `inner.suspended` initialised
    to `course_id.is_none()`.
  - `course_id` is now `Option<i32>` throughout `start_all_inner`.
  - UDP-connect, heartbeat spawn, and the post-establish "I'm
    watching" send are all wrapped in `if let Some(course_id_val)
    = course_id`; `heartbeat_abort` is now
    `Option<tokio::task::AbortHandle>` (Phase 2b UDP-deferral
    incorporated here to keep the pre-existing
    `course_gate.rs::start_all_inner_suspends_when_watched_athlete_has_no_course`
    test passing).
  - `tests/course_gate.rs::start_all_inner_suspends_when_watched_athlete_has_no_course`
    updated: now asserts `result.is_ok()` (spec changed from fatal
    error to suspended start in STEP-12.16 §F6).
  - All 4 course_gate tests and all relay_runtime Phase 1a tests
    pass; full suite green (`cargo test --workspace`).

### Phase 2 — F6 Phase B: defer UDP and heartbeat startup until a course is known

Bringing TCP up without UDP and heartbeat matches sauce's
`setUDPChannel()`-deferred path. The supervisor-event handler,
state-refresher, recv-loop, and capture writer all start as today.

- [x] **2a** — Tests (added to `tests/relay_runtime.rs`):
  - `suspended_start_does_not_create_udp_channel` — `RecordingUdpFactory`
    verifies `connected == false` after a suspended start.
  - `suspended_start_does_not_spawn_heartbeat` — `RecordingUdpFactory`
    verifies `written.is_empty()` after a suspended start (heartbeat task
    not spawned → no UDP packets written).  Negative trace assertion
    avoided: `relay.heartbeat.started` fires in many concurrent tests and
    pollutes the global log buffer.
  - `suspended_start_still_establishes_tcp` — `relay.tcp.established`
    trace is present (positive assertion, safe in parallel).
- [x] **2b** — Implementation (incorporated into Phase 1b):
  - UDP-connect block, heartbeat spawn, and the post-establish "I'm
    watching" send are all wrapped in `if let Some(course_id_val) =
    course_id { ... } else { None }`.
  - `heartbeat_abort` is `Option<tokio::task::AbortHandle>`, `None` when
    suspended.
  - All Phase 2a tests pass; full suite green (`cargo test --workspace`).

### Phase 3 — F6 Phase C: resume on first observed course

The state refresher already polls `auth.get_player_state(watched_id)`
on a 3 s minimum cadence. Today it transitions out of the suspended
state only via the recv-loop's inbound self-state path
(`:2565-2570`). The polled-state path needs the same transition,
and a new "bring UDP up now" helper.

- [x] **3a** — Tests (added to `tests/relay_runtime.rs`):
  - `state_refresher_resumes_when_watched_athlete_enters_game` —
    new `TransitioningAuth` stub: call 1 (startup course gate)
    returns `world: None` (suspended start); call 2 (first
    state-refresher poll after 3 s) returns `world: Some(7)`.
    `RecordingUdpFactory::connected` must be true after 4 s of
    paused-time advance; `relay.runtime.resumed` and `course_id`
    must appear in traces.  Currently RED (state refresher does
    not call resume_udp).
  - `recv_loop_self_state_with_world_transitions_out_of_suspended` —
    starts suspended; injects `TcpChannelEvent::Inbound` carrying
    the watched athlete's `PlayerState { id: 54321, world: Some(7)
    }`.  `RecordingUdpFactory::connected` must be true after 100 ms.
    Currently RED (recv-loop clears suspended flag but does not call
    resume_udp).
  - Negative trace assertion removed from
    `start_with_watched_athlete_in_game_proceeds_normally`: the
    `relay.runtime.suspended_no_course` event is emitted by Phase
    3a tests running in parallel and would contaminate the global
    log buffer; the positive contract (startup succeeds) is
    sufficient.
- [x] **3b** — Implementation:
  - New `resume_udp_tx: Mutex<Option<UnboundedSender<i32>>>` field
    added to `RuntimeInner`; populated `Some(tx)` when daemon starts
    suspended, `None` when it starts in-game.
  - New `initial_udp_addr: Mutex<Option<SocketAddr>>` field added to
    `RuntimeInner`; the recv-loop sets it when the first generic
    (lb_realm=0, lb_course=0) udp_config pool arrives over TCP.
  - New standalone `resume_udp` async function: receives course_id
    on the channel, waits up to 200 ms for `initial_udp_addr` to be
    set, connects UDP, establishes channel, sends post-establish "I'm
    watching", spawns heartbeat, clears `suspended`, emits
    `relay.runtime.resumed` with `course_id` field.
  - `run_state_refresher`: when `state.world` is `Some(course_id)`,
    takes and sends to `resume_udp_tx` (idempotent: take() returns
    None after first caller).
  - `recv_loop` inbound-state branch: on `suspended.swap(false)`
    returning true, checks `resume_udp_tx`:
    - Some → no-course resume path: take, send course_id; relay.runtime.resumed
      emitted by resume task after UDP is up.
    - None → idle-resume path: emit `relay.runtime.resumed` directly
      (UDP already set up; existing behaviour preserved).
  - New `resume_udp_abort: Option<AbortHandle>` field on
    `RelayRuntime`; aborted in `shutdown()`.
  - Test `recv_loop_self_state_with_world_transitions_out_of_suspended`
    amended: `sleep(50ms)` before the inject so the recv-loop
    processes the pending udp_config push (populating
    `initial_udp_addr`) before the resume fires.
  - Full suite green (`cargo test --workspace`).

### Phase 4 — F7: auto-reconnect on mid-session TCP shutdown

- [x] **4a** — Tests added to `tests/relay_runtime.rs`:
  - New stubs: `CountingRepeatableTcpFactory` (shared `Arc<AtomicU32>`
    counter, vends udp_config push on every connect) and
    `ReconnectSequenceTcpFactory` (succeed → refuse → succeed).
  - `tcp_channel_shutdown_mid_session_triggers_reconnect` — injects
    `TcpChannelEvent::Shutdown` after established; asserts
    `tcp_connect_count == 2` after 2 s and trace contains
    `relay.tcp.reconnect.scheduled`.  Currently RED (count stays 1).
  - `tcp_reconnect_increments_attempt_counter_on_repeated_failures` —
    `ReconnectSequenceTcpFactory`; injects Shutdown; asserts count == 3
    and trace contains `relay.tcp.reconnect.attempt attempt=1` and
    `attempt=2`.  Currently RED (count stays 1).
  - `tcp_reconnect_stops_on_explicit_shutdown` — injects Shutdown then
    calls `runtime.shutdown()` immediately; asserts count == 1.
    GREEN from start (regression guard for Phase 4b).
- [ ] **4b** — Implementation:
  - In `recv_loop`, when `TcpChannelEvent::Shutdown` arrives and
    the runtime has not been asked to stop, do not return
    `Ok(())`. Instead, signal a reconnect to a new
    `tcp_reconnect_loop` task spawned at startup.
  - The reconnect task owns the same factories as
    `start_with_retry` and re-runs the steps from "TCP connect"
    onward (TCP connect, hello, `udp_config` wait, UDP setup if
    course is known) using the existing exponential-backoff
    helper. The auth, session, and capture writer are NOT
    re-created.
  - Use a shutdown flag (the existing `Notify` plus an
    `AtomicBool` for "stopping requested") so an in-flight
    reconnect aborts on `runtime.shutdown()`.
  - **Logging contract** (matching the existing
    `relay.tcp.connecting` / `…established` / `…timeout`
    namespace, not `relay.runtime.*`):
    - `relay.tcp.reconnect.scheduled` (info) with `delay_ms` and
      `reason` ("shutdown" | "handshake_timeout") when the
      reconnect is queued.
    - `relay.tcp.reconnect.attempt` (info) with `attempt` and
      `backoff_ms`. On a failed attempt, include
      `error = %e` in the same record (do not split into a
      separate warn).
    - `relay.tcp.reconnect.succeeded` (info) with `attempts`.
    - `relay.tcp.reconnect.failed` (warn) with `attempts` and
      `error = %e` after the configured retry budget exhausts;
      this terminates the daemon, parallel to the F2 startup
      retries giving up.
  - The new traces are log-only; no capture-file record. The
    capture writer continues across reconnects so post-reconnect
    TCP/UDP frames land in the same file.

### Phase 5 — F8: extend startup handshake timeouts to match the reference

- [ ] **5a** — Tests:
  - `tcp_established_waits_at_least_30_seconds_before_timing_out`
    — TCP factory delays `Established` by 6 s; assert startup
    succeeds (today this fails after 5 s).
  - `udp_config_waits_at_least_30_seconds_before_timing_out` —
    same, with the udp_config push delayed.
  - `combined_handshake_timeout_is_30_seconds` — both events
    arrive together at 25 s; assert success.
  - `handshake_timeout_triggers_reconnect_not_exit` — the
    handshake never completes; assert the daemon enters the F7
    reconnect loop instead of exiting (depends on Phase 4
    landing first).
  - `handshake_timeout_emits_reconnect_scheduled_with_reason` —
    the handshake never completes; assert the trace contains
    `relay.tcp.reconnect.scheduled reason="handshake_timeout"`.
- [ ] **5b** — Implementation:
  - Replace the 5 s deadline at `:1763` and 5 s deadline at
    `:1818` with a single 30 s budget covering both gates,
    matching sauce's `activateSession()` race semantics. Hoist
    the budget to a `HANDSHAKE_BUDGET` constant near the top of
    the file for test injection.
  - On expiry, do not return a fatal error; route into the F7
    reconnect path via the shared shutdown signal.
  - **Logging contract**:
    - On expiry, emit `relay.tcp.handshake.timeout` (warn) with
      `phase` ("established" | "udp_config") and
      `elapsed_ms` before transitioning to the reconnect path.
      This preserves visibility now that the formerly-fatal
      `EstablishedTimeout` and `NoUdpConfig` errors no longer
      reach the orchestrator.
    - The subsequent reconnect emits the Phase 4 lifecycle
      traces.

### Phase 6 — Trace-event audit (new lifecycle events)

- [ ] **6a** — Tests:
  - `suspended_start_emits_runtime_suspended_no_course`.
  - `resume_emits_runtime_resumed_with_course_id`.
  - `tcp_reconnect_emits_full_lifecycle_traces` —
    `scheduled` → `attempt` → `succeeded`.
  - `handshake_timeout_emits_warn_event_before_reconnect`.
- [ ] **6b** — Implementation:
  - Add `course_id` to the existing `relay.runtime.resumed` trace
    so the operator can confirm which course the daemon resumed
    on.
  - Emit `relay.runtime.suspended_no_course` (info) at the gate
    in Phase 1.
  - Remove the lower-level `relay.course_gate.suspended` once
    Phase 1 lands; the higher-level lifecycle event subsumes it.
  - Add the F7 reconnect-lifecycle traces named in Phase 4 and
    the F8 handshake-timeout trace named in Phase 5.

### Phase 7 — Logging-coverage remediation for STEP-12.14 / STEP-12.15

A third-pass audit of the already-merged C/M/L/N items
(STEP-12.14) and F1-F5 items (STEP-12.15) revealed four logging
gaps. None block the smoke test directly, but each hides
operationally relevant state changes that the conventions in
STEP-12.12 ("log shit properly") were meant to prevent. Fixing
them inside this plan keeps the daemon's lifecycle log readable
once F6-F8 add their own traces.

- [ ] **7a** — Tests:
  - `logout_success_emits_runtime_logout_succeeded` — drive a
    successful `logout()` in the shutdown path (a wiremock 200);
    assert the trace contains `relay.runtime.logout_succeeded`
    at info level.
  - `leave_success_emits_runtime_leave_succeeded` — same for
    `leave()`.
  - `connect_retry_logs_underlying_error_cause` — first
    `start_with_retry` attempt fails with
    `TcpConnect(ConnectionRefused)`; assert the
    `relay.runtime.connect_retry` record contains
    `error = …refused…`, not just `attempt` and `backoff_ms`.
  - `heartbeat_tick_suspended_fires_only_on_edge` — set
    suspend, advance the clock by 5 ticks, clear suspend; the
    trace contains exactly one `relay.heartbeat.suspended`
    (entry) and one `relay.heartbeat.resumed` (exit), not five
    per-tick events.
  - `validation_failure_emits_structured_trace` — call
    `validate_startup` with `watched_athlete_id = None`; assert
    the daemon log contains a
    `ranchero.startup.validation_failed` event listing the
    missing field by name.
- [ ] **7b** — Implementation:
  - **F5 success traces.** In `RelayRuntime::shutdown`
    (`src/daemon/relay.rs:2294-2334`), have the spawned
    `logout()` / `leave()` tasks emit
    `relay.runtime.logout_succeeded` / `…leave_succeeded`
    (info) on `Ok`, parallel to the existing `_failed` warn on
    `Err`. The intent traces (`relay.runtime.logout` /
    `…leave`) already fire pre-spawn; this completes the pair.
  - **F2 retry cause.** In `start_with_retry`
    (`src/daemon/relay.rs:1212-1220`), thread the previous
    iteration's `RelayRuntimeError::TcpConnect(io_error)` into
    the `relay.runtime.connect_retry` record as
    `error = %prev_err`. The `last_error` capture at line 1241
    already holds the value; pass it through.
  - **F4 heartbeat suspend traces.** Replace the per-tick
    `relay.heartbeat.tick_suspended` (trace) at
    `src/daemon/relay.rs:788-792` with edge-only events:
    - `relay.heartbeat.suspended` (info) emitted exactly once
      when the loop observes the flag transition from
      `false → true` (track the prior value across iterations).
    - `relay.heartbeat.resumed` (info) on the
      `true → false` transition.
    - Remove the per-tick spam. A 30 s suspension currently
      emits 30 trace records; the edge-only version emits two
      info records. The visibility for "is the heartbeat
      gated?" comes from the higher-level
      `relay.runtime.suspended_idle` /
      `relay.runtime.suspended_no_course` lifecycle events;
      the heartbeat-scoped events confirm the scheduler
      itself observed the flag.
  - **F1 validation trace.** When `validate_startup` returns
    `Err(StartupValidationErrors)` and the daemon is about to
    exit before the fork, emit `ranchero.startup.validation_failed`
    (warn) with a `missing` field listing the variant names
    (e.g. `"missing_email,missing_watched_athlete_id"`). Today
    the message reaches stderr only and does not land in the
    daemon log file because the fork has not yet happened. Wire
    the call site in `src/daemon/runtime.rs` (or wherever
    `validate_startup` is invoked) to emit the trace before
    the early return so the log file captures the rejection
    when the parent's stderr is not being read.

#### Logging-contract summary for the new STEP-12.16 events

| Event | Level | Target | Phase | Capture |
| --- | --- | --- | --- | --- |
| `relay.runtime.suspended_no_course` | info | `ranchero::relay` | 1 | log only |
| `relay.runtime.resumed` (course_id field added) | info | `ranchero::relay` | 3 | log only |
| `relay.tcp.reconnect.scheduled` | info | `ranchero::relay` | 4 | log only |
| `relay.tcp.reconnect.attempt` | info | `ranchero::relay` | 4 | log only |
| `relay.tcp.reconnect.succeeded` | info | `ranchero::relay` | 4 | log only |
| `relay.tcp.reconnect.failed` | warn | `ranchero::relay` | 4 | log only |
| `relay.tcp.handshake.timeout` | warn | `ranchero::relay` | 5 | log only |
| `relay.runtime.logout_succeeded` | info | `ranchero::relay` | 7 | log only |
| `relay.runtime.leave_succeeded` | info | `ranchero::relay` | 7 | log only |
| `relay.heartbeat.suspended` (edge) | info | `ranchero::relay` | 7 | log only |
| `relay.heartbeat.resumed` (edge) | info | `ranchero::relay` | 7 | log only |
| `ranchero.startup.validation_failed` | warn | `ranchero::daemon` | 7 | log only |

All new events are control-plane / lifecycle. None corresponds to
a wire frame, so none belongs in the capture file. Wire-relevant
events (TCP/UDP frames, sessions, manifests) continue to flow
through the existing capture writer untouched.

## 5. Verification gate

After each phase, run:

```
cargo test --workspace
```

After all phases land, the original smoke must pass with the
watched athlete **not** in a game at start time:

```
ranchero start --capture output.cap
sleep 5
ranchero follow output.cap
```

Confirm:

- `ranchero status` reports "running" during the sleep.
- `ranchero follow` shows the `SessionManifest` and a
  `relay.runtime.suspended_no_course` lifecycle marker in the
  daemon log; the capture file may contain only the format header
  and the manifest while suspended.
- After the watched athlete enters a Zwift game, the daemon
  resumes within one state-refresher poll (≤ 3 s plus any
  in-flight delay). The capture begins receiving UDP records and
  the log shows `relay.runtime.resumed` with a `course_id` field.
- A simulated mid-session TCP drop (kill the relay TCP socket
  with a packet filter, or use `iptables` / `pfctl` to reject
  one connection) produces a
  `relay.runtime.tcp_reconnect.scheduled` followed by a
  `relay.runtime.tcp_reconnect.succeeded`; the daemon does NOT
  exit; capture continues after a brief gap.
- A handshake delay of 10 s (introducible with `tc qdisc` or a
  proxy) completes successfully rather than failing with
  `EstablishedTimeout` or `NoUdpConfig`.
- `ranchero stop` returns 0 cleanly in all states (suspended,
  resumed, mid-reconnect).

## 6. Deferred follow-ups (not in this plan)

These differences from sauce are operationally relevant but do not
block the smoke test on a healthy first run. Each warrants a
separate plan when prioritised.

- **Auth and session login retry.** Sauce retries every error
  category through `_schedConnectRetry`. Ranchero retries only
  `TcpConnect`. A mistyped password is currently a fatal exit; on
  a real installation that is arguably the right behaviour, but a
  transient 503 from the auth endpoint is not.
- **TCP server pinning across reconnects.** Sauce sticks to the
  last-used TCP server (`zwift.mjs:1818-1822`); ranchero always
  picks `tcp_servers[0]`. Across F7 reconnects the new TCP
  server choice may differ from the original.
- **UDP error count threshold reconnect.** Sauce's
  `incErrorCount()` (`:1934-1939`) calls `_schedConnectRetry` after
  every 10 UDP errors. Ranchero has no equivalent. Once F7 lands,
  this becomes a smaller follow-up: add a UDP error counter that
  feeds the same reconnect path.
- **TCP server pool refresh on re-login.** Already handled by F3
  (STEP-12.15): the `RelaySessionSupervisor` emits
  `LoggedIn { tcp_servers, … }` on every re-login, and the
  supervisor-event handler can choose the new server.

## 7. Out of scope for STEP-12.16

- Changing the state refresher's cadence. The 3 s minimum / 30 s
  expanding / 5 min cap behaviour from STEP-12.14 §L1 stands.
- Reusing the resume code path for a future "athlete switched
  courses mid-ride" transition. That belongs in a separate plan;
  the resume-on-first-course case is the only fix needed for the
  smoke test.
- Reconnecting at the auth or session layer. F3 handles session
  refresh; F4 (this plan, Phase 4) handles TCP-channel shutdown.
  Anything beyond those two is the deferred follow-up section.
