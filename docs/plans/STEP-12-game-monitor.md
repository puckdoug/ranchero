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
end-to-end was deferred to "STEP 12's supervisor". STEP-12, as
originally written, was a 26-line sketch covering only routing
decisions inside an already-running supervisor:
`findBestUDPServer`, idle-suspension FSM, and watched-athlete
switching. That sketch did not own the work that makes the
supervisor exist in the first place: the auth bootstrap, the
relay-session login wiring, the daemon integration, the CLI
plumbing, the heartbeat scheduler, the capture lifecycle, the
tracing log, the shutdown coordination, or the live validation.

This expanded plan replaces that sketch. STEP-12 now owns
sustainable end-to-end connectivity. The first sub-step, STEP-12.1,
delivers a TCP-only smoke that is bounded by a server-side timeout
(roughly 30 s without a UDP heartbeat); the rest of STEP-12 adds
the UDP channel and heartbeat that make the connection
indefinitely sustainable, plus the routing and suspension logic
that the original sketch already named.

## Sub-steps and order

| Sub-step | Scope | File |
|---|---|---|
| 12.1 | TCP-only end-to-end smoke. Auth + relay-session login + a single TCP channel + capture writer + tracing log + daemon and CLI integration. | [STEP-12.1-tcp-end-to-end-smoke.md](STEP-12.1-tcp-end-to-end-smoke.md) |
| 12.2 | `ranchero follow <file>` command for live capture-file tailing. Adds a `CaptureFollower` and a top-level subcommand that reads a wire-capture file as it is being written and prints each record (optionally decoded) to standard output. Used to validate STEP-12.1 from a second terminal and as a general protocol-level debugging aid for every subsequent sub-step. | [STEP-12.2-follow-command.md](STEP-12.2-follow-command.md) |
| 12.3 | UDP channel + 1 Hz heartbeat. Brings up the UDP transport, runs the existing hello-loop / SNTP-style time sync, and adds the heartbeat scheduler that prevents server-side timeout. After 12.3, the connection is sustainable indefinitely. | this file, "Sub-step 12.3" below |
| 12.4 | `udpConfigVOD` parsing + `findBestUDPServer`. Builds and updates the per-`(realm, courseId)` pool from inbound TCP messages and selects the right UDP server by the watched athlete's position. Adds per-course UDP reselection. | this file, "Sub-step 12.4" below |
| 12.5 | Idle suspension FSM + watched-athlete switching + the `GameEvent` enum that downstream consumers will subscribe to. | this file, "Sub-step 12.5" below |

12.1 and 12.2 have their own plan files because each is a focused
deliverable that stands on its own. 12.3, 12.4, and 12.5 are
described inline below; if any of them grows large enough to
warrant its own file during implementation, that file may be
extracted at that time.

## Scope

In scope:

- All of 12.1's scope (auth bootstrap, relay session,
  single TCP channel, capture writer, tracing log, daemon and
  CLI integration, removal of the STEP-11.6 Fix-D guard).
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
- Tracing log records for UDP events (`relay.udp.*`) on the
  same target prefix as the TCP events from 12.1.
- A graceful shutdown sequence that cancels outbound
  heartbeats first, then the UDP channel, then the TCP
  channel, then `flush_and_close()` on the capture writer,
  then the relay session supervisor.
- Sustained live validation: a multi-minute run against
  production Zwift confirming that the connection survives
  past the TCP-only timeout window and that all observed
  traffic is captured and logged.

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

The orchestrator lives in `src/daemon/relay.rs` (introduced by
STEP-12.1) and grows through 12.3 to 12.5. The 12.2 sub-step adds
a separate `ranchero follow` command surface that reads from the
capture files the orchestrator produces; it does not modify the
orchestrator itself. The component map at the end of STEP-12 is:

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

A recommended order. Each phase ends with a green `cargo test
--workspace` and, where the phase touches the network surface, a
manual smoke against production Zwift.

1. **STEP-12.1** — TCP-only smoke.
2. **STEP-12.2** — `ranchero follow <file>` command. Lands
   immediately after 12.1 so that every subsequent phase has
   an interactive way to watch traffic in a second terminal.
3. **12.3a** — `HeartbeatScheduler` and `WorldTimer` plumbing,
   tested against a mock UDP transport.
4. **12.3b** — Wire `UdpChannel::establish` into `RelayRuntime`
   using either Option A or Option B for initial UDP-server
   selection. Sustained live validation at the end of this
   phase confirms the connection survives past the TCP-only
   timeout.
5. **12.4a** — `udpConfigVOD` parsing into a `UdpPoolRouter`
   structure, with table-driven tests on synthetic inbound
   messages.
6. **12.4b** — `findBestUDPServer` port from
   `zwift.mjs:2295-2317`, with the table-driven tests listed
   above.
7. **12.4c** — Wire the router and the watched-athlete state
   into `RelayRuntime`, including UDP channel swap on server
   change.
8. **12.5a** — `IdleFSM` standalone, with state-transition
   tests.
9. **12.5b** — Wire the `IdleFSM` into the orchestrator: it
   shuts down UDP on the suspend transition and re-establishes
   on the resume transition.
10. **12.5c** — Watched-athlete switching, including the
    broadcast-channel control message that selects a new
    athlete.
11. **12.5d** — `GameEvent` enum and the `events()` broadcast
    surface. Existing emitters are reorganised to feed the
    enum.

## CLI and daemon integration

12.1 owns the CLI and `daemon::start` changes for the
orchestrator. 12.2 adds a separate `ranchero follow` command that
reads capture files; its CLI surface is described in its own plan
file. No additional CLI surface is added by 12.3, 12.4, or 12.5.
A future `--watch <athlete-id>` flag (and a control-socket message
that switches the watched athlete at runtime) is anticipated but
is deferred; by default the watched athlete is the logged-in user.

## Logging contract (extensions to 12.1)

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

- All four sub-steps' tests pass: the new tests in
  `src/daemon/relay.rs`, `tests/cli_args.rs`, and
  `tests/relay_runtime.rs` (added by 12.1), plus the unit
  tests for `HeartbeatScheduler`, `UdpPoolRouter`,
  `findBestUDPServer`, `IdleFSM`, and the `GameEvent` surface.
- `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` are
  both green.
- The sustained live-validation procedure has been performed
  for at least 30 minutes against production Zwift. The
  results are appended to this file under "Live validation
  results", showing no server-side timeout, the expected
  lifecycle records, and a non-zero capture-record count for
  both transports in both directions.
- `ranchero stop` performs a clean teardown that flushes the
  capture writer (zero truncation, every accepted record
  readable on replay) and shuts down the relay session.
- The capture file written during a 30-minute run is
  reproducibly readable by `ranchero replay`. The replay
  summary reports inbound and outbound counts for both UDP
  and TCP.

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

## Cross-references

- `docs/plans/STEP-12.1-tcp-end-to-end-smoke.md` — the first
  sub-step.
- `docs/plans/STEP-12.2-follow-command.md` — the
  `ranchero follow <file>` command for live capture-file
  tailing, which lands immediately after 12.1 so that 12.3
  onward can be validated interactively.
- `docs/plans/done/STEP-09-relay-session.md` — the relay
  session and supervisor used by `RelayRuntime`.
- `docs/plans/done/STEP-10-udp-channel.md` — the UDP channel
  with hello-loop and SNTP-style time sync.
- `docs/plans/done/STEP-11-tcp-channel.md` — the TCP channel.
- `docs/plans/done/STEP-11.5-wire-capture.md` — the capture
  mechanism wired into both channels.
- `docs/plans/done/STEP-11.6-capture-consistency-review.md` —
  the consistency review whose Fix-D guard is removed by 12.1.
- `docs/ARCHITECTURE-AND-RUST-SPEC.md` — §4.4
  (`ClientToServer` hello fields), §4.8 (UDP server
  selection), §4.13 (idle suspension), §7.12 (client-driven
  liveness model).
- sauce4zwift's `zwift.mjs:2295-2317` — the
  `findBestUDPServer` reference implementation.
