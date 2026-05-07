# STEP-12.15 — Post-12.14 review: gaps that remain after the C/M block

**Status:** review (2026-05-07).

After STEP-12.12 (logging) and STEP-12.14 (the nine-round side-by-side
review against sauce4zwift) were completed, the codebase was
re-reviewed against the basic smoke test:

```
ranchero start --capture output.cap
sleep 5
ranchero follow output.cap
```

Every Critical and Material item the STEP-12.14 plan called out as
"must land before the first successful trace" is in place and the
full workspace test suite passes. The findings below are the gaps
the C/M block did not cover. None of them block the 5-second smoke
provided the network is healthy and the watched athlete is in a
game; all of them will surface on the first run that diverges from
that ideal.

## 0. Verification of STEP-12.14 implementation

Spot-checks against the live tree confirm the C-block and M-block
landed:

| Item | Citation |
| ---- | -------- |
| **C5** UDP port 3024 hardcoded | `src/daemon/relay.rs:67-83` (`pick_initial_udp_target`) |
| **N10** ack-seqno reads tag 5 (`stc_f5`) | `crates/zwift-relay/src/udp.rs:316` |
| **N11** `player_count` from `stc.states` | `crates/zwift-relay/src/udp.rs:611-612` |
| **N2** split TCP/UDP `connId` counters | `src/daemon/relay.rs:121-132` |
| **C1** generic `lb_course=0` pool selection | `src/daemon/relay.rs:1644-1646` |
| **C2 / R1** course gate via `get_player_state(watched_id)` | `src/daemon/relay.rs:1462-1486`; `crates/zwift-api/src/lib.rs:456-478` |
| **C3** post-establish "I'm watching" send | `src/daemon/relay.rs:1727-1748` |
| **C4 + N13 + R2** heartbeat content + shared `WorldTimer` | `src/daemon/relay.rs:1706-1707`, `:703-722` |
| **C6 / C7 / C8 / N3 / N4** HTTP impersonation | `crates/zwift-api/src/lib.rs:46-47, :307-309, :612-619, :848-850` |
| **M1** UDP hello header consistency | `crates/zwift-relay/src/udp.rs:519-525` |
| **M2 / L3 / N6 / N7** `last_world_update_ts` tracked + threaded into TCP hello | `src/daemon/relay.rs:2448-2469`, `:1600-1614` |

`cargo test --workspace` is green at the start of this review.

## 1. Findings

Five gaps remain. Each is described with the symptom you will
observe on a real run, the citation for the relevant code, and the
fix's expected scope.

### F1 — `validate_startup` does not check `watched_athlete_id` (UX blocker for the smoke)

**Symptom.** If `[zwift] watched_athlete_id` is not set in the
config, `start_all_inner` returns
`RelayRuntimeError::NoWatchedAthlete` (`src/daemon/relay.rs:1462-1464`)
*after* the double-fork has completed. The parent process has
already exited 0, so your shell sees `ranchero started (pid N)` and
no error. A subsequent `ranchero status` reports "not running"; the
actual cause is only visible in the daemon's log file.

**Why this matters for the smoke.** Anyone running the smoke for
the first time on a fresh install will hit this if they only set
the credentials and skip the watched-athlete field. The 5-second
sleep then runs against a daemon that died at startup, and `ranchero
follow output.cap` reads a capture file containing only the format
header.

**Citation.** `src/daemon/validate.rs:62-137` checks credentials,
pidfile dir, log-file dir, and capture-file path. There is no
analogue to `S-1` for the watched athlete. The validator's
`make_config` test helper at `src/daemon/validate.rs:171` already
threads a `watched_athlete_id: None` default, so the field is
visible to the validator.

**Scope.** Add `S-5` alongside `S-1` in `validate_startup`: when
`relay_enabled` is true, error if `watched_athlete_id` is `None`. A
new `StartupValidationError::MissingWatchedAthleteId` variant and
the matching `Display` arm. One-line check, one new variant, three
or four tests parallel to `S-1a`–`S-1h`.

### F2 — Production daemon path bypasses the L5 retry / backoff loop

**Symptom.** A single transient `TcpConnect` failure terminates the
daemon at startup. The L5 work in STEP-12.14 (exponential backoff,
`1.2^attempt`, capped at 5 min, up to 50 retries) never runs.

**Citation.**
- The retry wrapper exists at `src/daemon/relay.rs:1306-1370`
  (`start_with_all_deps`).
- The production entry point is `RelayRuntime::start_with_writer`
  at `src/daemon/relay.rs:1144-1186`. It calls `start_all_inner`
  directly — no retry wrapper.
- `daemon::start` → `runtime::start` → `start_with_writer` is the
  full call chain (`src/daemon/runtime.rs:239-258`).

**Why this is not a 5-second-smoke blocker.** A first attempt that
succeeds is unaffected. The defect surfaces on transient network
failures, mid-run reconnects, or DNS hiccups.

**Scope.** Either:
- (a) Move the retry loop's body into a small helper used by both
  `start_with_writer` and `start_with_all_deps`; or
- (b) Delete `start_with_writer`'s direct call site and route
  through `start_with_all_deps` with the production factories.

(b) is mechanically cheaper but pulls all four DI factories into
the production constructor; (a) keeps the surface area smaller.
Either way the change is small.

### F3 — Production path uses a stub `SessionSupervisor`; the real one is dead code

**Symptom.** Once the initial relay-session login succeeds, the
session is never refreshed. After roughly 50 minutes (the
`SESSION_REFRESH_FRACTION` of the typical 90-minute session
expiry) the AES key the channels are using becomes invalid and the
data plane silently dies. The supervisor-event handler in
`start_all_inner` that would re-issue manifests, pin the TCP server
across reconnects (L4), and log re-login transitions (N14) is
subscribing to a dead `broadcast::channel` and never fires.

**Citation.**
- `DefaultSessionSupervisorFactory::start` at
  `src/daemon/relay.rs:2333-2348` calls `zwift_relay::login` once
  and wraps the result in `DefaultSessionSupervisorHandle`.
- `DefaultSessionSupervisorHandle::subscribe_events` at
  `src/daemon/relay.rs:2309-2311` constructs an empty
  `broadcast::channel(1)` and returns the `Receiver`, dropping the
  `Sender`. Every recv on that receiver returns `Closed`
  immediately.
- The real supervisor lives at
  `crates/zwift-relay/src/session.rs:259-343`
  (`RelaySessionSupervisor`) and is never called from `src/`.
- The supervisor-event handler that would consume real events is at
  `src/daemon/relay.rs:1775-1874`. Its loop sees `Err(Closed)` on
  the first recv and exits.

**Why this is not a 5-second-smoke blocker.** Sessions do not
expire in five seconds.

**Scope.** Replace `DefaultSessionSupervisorFactory` with one that
calls `RelaySessionSupervisor::start` and returns a handle wrapping
the live supervisor's `current()` / `events()` / `shutdown()`
methods. The trait shape (`SessionSupervisorHandle`,
`SessionSupervisorFactory`) already matches the supervisor's API;
the change is mostly mechanical. Tests for the supervisor itself
already exist; the new work is the production-side wiring and one
end-to-end test that `start_with_writer` propagates a
`SessionEvent::Refreshed` into the manifest stream.

### F4 — `HeartbeatScheduler` does not gate on `inner.suspended`

**Symptom.** Once the state-refresher (`run_state_refresher` at
`src/daemon/relay.rs:537-594`) sets `inner.suspended = true` after
15 seconds of no inbound self-state, heartbeats continue at 1 Hz
regardless. STEP-12.14 batch C said "gate heartbeat ticks on
suspended == false" (Cb).

**Citation.**
- `HeartbeatScheduler` at `src/daemon/relay.rs:658-770` owns
  `world_timer`, `athlete_id`, `watching_rider_id`, `course_id`
  and an interval ticker. It holds no reference to
  `RuntimeInner`.
- `HeartbeatScheduler::run` at `src/daemon/relay.rs:735-769` calls
  `send_one()` unconditionally on every tick.
- `RuntimeInner::suspended` at `src/daemon/relay.rs:458` is
  written by the refresher and cleared by the recv-loop at
  `src/daemon/relay.rs:2439-2444`, but no consumer reads it for
  gating.

**Why this is not a 5-second-smoke blocker.** The 15-second
suspend threshold cannot fire in five seconds.

**Scope.** Thread an `Arc<RuntimeInner>` (or a narrower
`Arc<AtomicBool>` for just the suspend flag) into
`HeartbeatScheduler::new` and check it inside `run`'s tick loop
before `send_one`. The constructor at
`src/daemon/relay.rs:671-687` and the call site at
`src/daemon/relay.rs:1756-1762` are the only two places to update.

### F5 — `auth.logout()` and `auth.leave()` are tracing-only

**Symptom.** On clean shutdown the server-side session lingers
for up to 90 minutes. STEP-12.14 N9 said the daemon should issue
`/api/users/logout` and `/relay/worlds/1/leave` HTTP requests on
shutdown.

**Citation.**
- `RelayRuntime::shutdown` at `src/daemon/relay.rs:2196-2212`
  emits `relay.runtime.logout` and `relay.runtime.leave` trace
  records but issues no HTTP traffic.
- `crates/zwift-api/src/lib.rs` defines no `logout` or `leave`
  methods on `ZwiftAuth`.

**Why this is not a 5-second-smoke blocker.** The smoke does not
exercise shutdown cleanup beyond what `ranchero stop` (or the
SIGTERM the smoke script never sends) would invoke. Server-side
session lingering does not affect the capture file.

**Scope.** Add `ZwiftAuth::logout()` (POST `/api/users/logout`)
and `ZwiftAuth::leave()` (POST `/relay/worlds/1/leave`); call them
from `RelayRuntime::shutdown` before notifying the recv-loop.
Best-effort: failures are logged but do not affect the rest of the
shutdown sequence.

## 2. Priority for the smoke

If you want the smoke
(`ranchero start --capture output.cap; sleep 5; ranchero follow output.cap`)
to be a clean repeatable test, **F1 alone** is enough. The other
four items will not trip a five-second window.

For any run longer than ten minutes, add **F2** (transient
TcpConnect failure ends the run). For any run longer than 50
minutes, add **F3** (the data plane silently dies at session-key
rotation time). **F4** matters once you start exercising the
suspend / resume flow. **F5** is server-hygiene work and matters
only if you run the daemon repeatedly against the same account in
quick succession.

Suggested order:

1. F1 — pre-flight watched-athlete check.
2. F2 — wire the L5 retry into the production path.
3. F3 — wire the real `RelaySessionSupervisor` into the production path.
4. F4 — gate heartbeat ticks on `suspended == false`.
5. F5 — implement `logout` and `leave` HTTP calls.

## 3. Implementation plan

Each item below is a TDD pair: `Na` writes failing tests against
the contract; `Nb` is the implementation that makes them pass.
None of these depend on each other; they can land in any order.

### Phase 1 — F1: pre-flight `watched_athlete_id` check

- [ ] **1a** — Tests for F1 in `src/daemon/validate.rs`'s `tests`
      module (parallel to `S-1a`–`S-1h`):
  - `validate_relay_enabled_no_watched_athlete_id_returns_error` —
    relay enabled, both monitor credentials present,
    `watched_athlete_id = None` → error includes a
    `MissingWatchedAthleteId` variant.
  - `validate_relay_disabled_skips_watched_athlete_check` — relay
    disabled, `watched_athlete_id = None` → ok.
  - `validate_relay_enabled_watched_athlete_set_is_ok` — relay
    enabled, all three present → ok.
  - `validate_emits_all_missing_relay_fields_together` — relay
    enabled, all three of email/password/watched-athlete missing →
    error contains all three variants in declaration order.
- [ ] **1b** — Implementation for F1:
  - Extend `StartupValidationError` with a
    `MissingWatchedAthleteId` variant and matching `Display`
    arm at `src/daemon/validate.rs:8-29`.
  - In the `if cfg.relay_enabled` block at
    `src/daemon/validate.rs:69-76`, push
    `MissingWatchedAthleteId` when `cfg.watched_athlete_id`
    is `None`.

### Phase 2 — F2: wire L5 retry into production path

- [ ] **2a** — Tests for F2 in `tests/relay_runtime.rs`:
  - `start_with_writer_retries_on_transient_tcp_connect` — a
    TCP factory that fails the first connect with
    `io::ErrorKind::ConnectionRefused` and succeeds on the
    second attempt completes successfully; the trace contains
    one `relay.runtime.connect_retry attempt=1` record.
  - `start_with_writer_propagates_permanent_errors_immediately` —
    a missing-credential failure surfaces without a retry
    delay.
- [ ] **2b** — Implementation for F2:
  - Extract the retry loop body from
    `start_with_all_deps` (`src/daemon/relay.rs:1306-1370`)
    into a private helper that takes the same
    `Arc<…>` factories.
  - Call the helper from both `start_with_all_deps` and
    `start_with_writer` (`src/daemon/relay.rs:1144-1186`).
  - Confirm the existing `backoff_ms_for` helper at
    `src/daemon/relay.rs:113-116` is reused unchanged.

### Phase 3 — F3: wire real `RelaySessionSupervisor` into production path

- [ ] **3a** — Tests for F3 in `tests/relay_runtime.rs`:
  - `start_with_writer_subscribes_to_real_supervisor_events` —
    drive a `RelaySessionSupervisor` stub-equivalent (or the
    real supervisor against a wiremock token endpoint) and
    assert `relay.session.refreshed` appears in the trace
    after a synthetic refresh.
  - `start_with_writer_records_fresh_manifest_on_supervisor_relogin`
    — a `SessionEvent::LoggedIn` with a fresh AES key writes
    a new `SessionManifest` to the capture file (visible via
    `CaptureReader`).
- [ ] **3b** — Implementation for F3:
  - Replace the body of `DefaultSessionSupervisorFactory::start`
    (`src/daemon/relay.rs:2333-2348`) with a call to
    `zwift_relay::RelaySessionSupervisor::start(auth, config)`.
  - Replace `DefaultSessionSupervisorHandle`'s fields and impl
    (`src/daemon/relay.rs:2299-2315`) with delegations to the
    real supervisor's `current()` / `events()` / `shutdown()`
    methods. Hold the supervisor in an `Arc` so `Handle` can be
    cheap to clone.
  - The supervisor-event handler at
    `src/daemon/relay.rs:1775-1874` is unchanged: it now sees
    real `SessionEvent` traffic instead of an immediately-closed
    receiver.

### Phase 4 — F4: gate heartbeat on `suspended`

- [ ] **4a** — Tests for F4 in `src/daemon/relay.rs`'s `tests`
      module (alongside the existing `HeartbeatScheduler` tests
      around `src/daemon/relay.rs:3180`):
  - `heartbeat_skips_send_when_suspended_flag_is_set` — set
    the suspend flag before `run`; advance the tokio test
    clock past two intervals; assert the recording sink saw
    zero sends.
  - `heartbeat_resumes_send_when_suspended_clears` — flag
    set, then cleared between ticks; assert one send after
    the clear.
- [ ] **4b** — Implementation for F4:
  - Extend `HeartbeatScheduler` (`src/daemon/relay.rs:658-666`)
    with a `suspended: Arc<AtomicBool>` field; update
    `HeartbeatScheduler::new` (`:671-687`) to take it as the
    last argument.
  - In `HeartbeatScheduler::run`'s tick loop
    (`:735-769`), add `if self.suspended.load(…) { continue; }`
    before `send_one`. The trace event should still fire so
    the operator can see the gate engaging.
  - Update the call site at `src/daemon/relay.rs:1756-1762` to
    pass `Arc::clone(&inner.suspended)` (or wrap the field in
    its own `Arc` if the existing layout makes that awkward).

### Phase 5 — F5: implement `logout` and `leave` HTTP calls

- [ ] **5a** — Tests for F5 in `crates/zwift-api/tests/auth.rs`:
  - `logout_posts_to_users_logout_with_bearer` — wiremock
    expects POST `/api/users/logout` with `Authorization`,
    `Source`, `Platform`, `User-Agent`.
  - `leave_posts_to_relay_worlds_1_leave_with_bearer` — same
    against `/relay/worlds/1/leave`.
  - `logout_failure_does_not_panic` — server returns 500;
    the call returns `Err` cleanly.
- [ ] **5b** — Implementation for F5:
  - Add `ZwiftAuth::logout()` and `ZwiftAuth::leave()` in
    `crates/zwift-api/src/lib.rs` mirroring `do_refresh`'s
    header set (`:840-870` region).
  - In `RelayRuntime::shutdown`
    (`src/daemon/relay.rs:2196-2212`), invoke both via
    best-effort `tokio::spawn` calls before `notify_one`.
    Failures log a `relay.runtime.logout_failed` /
    `relay.runtime.leave_failed` warning and continue.

## 4. Out of scope

- The supervisor-event handler's L4 (`tcp_server_pinned`) trace at
  `src/daemon/relay.rs:1791-1801` will start firing once F3 lands;
  that is not a separate work item.
- The TUI's existing watched-athlete-ID field already covers the
  configure path for F1; no TUI work is needed.
- The proto-fork items (N1, N12, C11) remain deferred; STEP-12.14
  flagged them as "fix only if C5 + C6/7/8 don't unblock the
  trace", and the C-block did unblock it in this review.

## 5. Verification gate

After each phase, run:

```
cargo test --workspace
```

After the full set lands, run the smoke against a live account in
a Zwift game:

```
ranchero start --capture output.cap
sleep 5
ranchero follow output.cap output.cap
```

Confirm:
- `ranchero status` reports "running" during the sleep.
- `output.cap` contains a `SessionManifest` followed by at least
  one outbound TCP record, one outbound UDP record (the
  post-establish "I'm watching" send), and at least one inbound
  UDP record.
- `ranchero follow` prints those records as they land.
- `ranchero stop` returns 0 and the capture file is left readable
  (the writer task's `relay.capture.writer.closed` rollup appears
  in the log).
