<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# 17.37 / 17.38 — Relay-to-web data path: design clarification

This note resolves the ambiguity that stalled the implementation of
17.38-I. Read it before touching `relay.rs`, `runtime.rs`, or the bridge.

## Why the work stalled

17.37-I was implemented one way; 17.38-I needs a different thing. The two
no longer fit together.

`route_player_state` (the registry populator the bridge must call) does
not read only the eleven scalar fields now on `GameEvent::PlayerState`.
It calls `apply_event_state`, which reads the proto through `ProtoView`
(`src/web/proto_view.rs`). `ProtoView` reads **five proto fields that the
scalar variant does not carry**:

| Proto field | ProtoView accessor | Used for |
|---|---|---|
| `aux3`      | `road_id()` (bits 8–23) | segment / road tracking |
| `road_time` | `road_time()`           | road position |
| `f19`       | `reverse()` (bit 2)     | direction of travel |
| `group_id`  | `group_id()`            | group membership |
| `time`      | `time()`                | per-rider event clock |

So a bridge that only had `GameEvent::PlayerState { athlete_id, power_w,
cadence_u_hz, speed_mm_h, world_time_ms, world, sport, distance, z, draft,
heartrate }` could not reconstruct a proto faithful enough to drive
`route_player_state`. **The bridge needs the whole `zwift_proto::PlayerState`,
not a scalar projection of it.**

## Facts established by reading the code

1. **The stats fanout only reads `athlete_id`.** `stats_fanout_task`
   (`src/web/subs/mod.rs:162`) matches `GameEvent::PlayerState { athlete_id,
   .. }`, then looks the athlete up in the registry. Every other field on
   the event is ignored. So the event on `web_state.game_events_tx` only
   needs to carry `athlete_id` plus enough for the registry to be ready.

2. **Nothing in production subscribes to the relay's `events()` stream.**
   `RelayRuntime::events()` (`relay.rs:2527`) is consumed only by relay
   unit tests today. In production the relay's `GameEvent::PlayerState`
   emission currently goes nowhere.

3. **`web_state.game_events_tx` is a separate channel from the relay's.**
   17.36-I created a daemon-owned channel and handed its sender to
   `WebState`. Nothing feeds it yet, which is why the production stats
   subscription path delivers no events.

4. **`bridge_player_state_event` already does the per-frame work.** It
   calls `route_player_state` (registry first), then emits
   `GameEvent::PlayerState` on `web_state.game_events_tx` (fanout second).
   The ordering guarantee 17.38-T checks is already correct. What is
   missing is (a) a source of full protos and (b) a task that drives this
   function per frame.

## The design: a dedicated proto stream

Add a second broadcast channel on the relay that carries the **full**
`zwift_proto::PlayerState`. The bridge subscribes to it; the lean
`GameEvent` enum is left alone.

```
                    relay recv loop / state refresher
                                │
              ┌─────────────────┴───────────────────┐
              │                                      │
   player_states_tx                          game_events_tx
   (zwift_proto::PlayerState)                (GameEvent — unchanged)
              │                                      │
        bridge task (run_daemon)              (relay's own consumers,
              │                                 today: tests only)
   1. route_player_state(&proto, …)  ← registry updated
   2. emit GameEvent::PlayerState     → web_state.game_events_tx
              │
        stats fanout (reads athlete_id, looks up registry)
```

Why a dedicated stream rather than embedding the proto in
`GameEvent::PlayerState`:

- It matches the plan's wording in 17.38-I ("subscribe to the relay's
  **proto stream**").
- `GameEvent` stays a lightweight signal enum. Embedding a large prost
  message would make every broadcast clone of *every* `GameEvent`
  (`StateChange`, `PoolSwap`, `Latency`) pay the size of the largest
  variant.
- `bridge_player_state_event` and the stats fanout need no change.
- All currently-passing tests stay green; the remaining work is additive.

### Known redundancy (accepted, documented)

`GameEvent::PlayerState` keeps its eleven scalar fields even though only
`athlete_id` is read downstream. The fields are harmless and removing them
is a separable cleanup that would re-touch six test files and two relay
tests for no functional gain. A future step may reduce the variant to
`{ athlete_id }`; this note records that the scalars are vestigial so the
next reader is not misled into thinking the bridge depends on them.

## Implementation steps

### Step A — relay surfaces the full proto (completes 17.37-I)

In `src/daemon/relay.rs`:

1. Add a field to `RuntimeInner` (near `game_events_tx`, line ~497):
   ```rust
   /// Broadcast of the full decoded `PlayerState` proto for the
   /// relay-to-web bridge. Carries every field — including aux3,
   /// road_time, f19, group_id, time — that `route_player_state`
   /// reads through `ProtoView` but `GameEvent::PlayerState` omits.
   player_states_tx: tokio::sync::broadcast::Sender<zwift_proto::PlayerState>,
   ```
2. Create the channel where `game_events_tx` is created and thread it into
   `RuntimeInner` at every construction site. Suggested capacity 4096 (the
   recv loop can deliver a burst of states per STC).
3. Mirror the sender onto `RelayRuntime` (alongside its `game_events_tx`
   field) so a public accessor can reach it.
4. Emit the full proto at the two sites that today build
   `GameEvent::PlayerState`:
   - recv loop, `for state in &stc.states` (line ~3272):
     `let _ = inner.player_states_tx.send(state.clone());`
   - state refresher (line ~599):
     `let _ = inner.player_states_tx.send(state.clone());`
   Keep the existing `GameEvent::PlayerState { … }` emission as-is.
5. Add the public accessor:
   ```rust
   /// Subscribe to the stream of full `PlayerState` protos. The
   /// relay-to-web bridge consumes this to populate the athlete
   /// registry. Only protos sent after the subscribe call are seen.
   pub fn player_states(&self)
       -> tokio::sync::broadcast::Receiver<zwift_proto::PlayerState> {
       self.player_states_tx.subscribe()
   }
   ```

### Step B — rewrite 17.37-T to test the real contract

`tests/relay_surfaces_player_state.rs` currently asserts the eleven scalar
fields exist on `GameEvent::PlayerState` (a compile-time check). That check
now passes but does not test what 17.38 needs. Replace it with a runtime
test that drives a proto through the relay and asserts the **proto stream**
delivers it with the road/group/time fields intact:

- Start a runtime via `start_with_deps` (as the existing relay test at
  line ~4800 does).
- `let mut proto_rx = runtime.player_states();`
- Inject an `Inbound` STC carrying a `PlayerState` with `aux3`,
  `road_time`, `f19`, `group_id`, and `time` set to non-default values.
- Assert `proto_rx.recv()` yields a proto whose `aux3`, `road_time`,
  `f19`, `group_id`, `time` match — proving full fidelity, not a scalar
  projection.
- Mark `#[ignore = "slow: …"]` only if it exceeds the 100 ms dev-loop
  budget; a single inject/recv round trip should not.

### Step C — spawn the bridge task in `run_daemon` (completes 17.38-I)

In `src/daemon/runtime.rs::run_daemon`, after the relay runtime is started
and only when it exists:

```rust
let bridge_abort = runtime.as_ref().map(|rt| {
    let mut proto_rx = rt.player_states();
    let web_state    = Arc::clone(&web_state);
    tokio::spawn(async move {
        loop {
            match proto_rx.recv().await {
                Ok(proto) => {
                    let epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default();
                    let now           = epoch.as_secs_f64();   // same clock as gc_tick_loop
                    let wall_clock_ms = epoch.as_millis() as u64;
                    crate::web::proto_to_stats::bridge_player_state_event(
                        &proto, &web_state, now, wall_clock_ms,
                    );
                }
                Err(broadcast::error::RecvError::Closed)    => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }).abort_handle()
});
```

`now` and `wall_clock_ms` both come from `SystemTime` so `now` is on the
same clock the GC uses (`gc_tick_loop` derives its `now` the same way).

Shutdown: after the main select loop breaks, abort the bridge before relay
teardown so no frame is processed against a half-torn-down registry:

```rust
if let Some(h) = bridge_abort { h.abort(); }
web_handle.stop().await;
// … existing runtime.shutdown() / join …
```

(When `runtime.shutdown()` drops the relay the proto sender closes and the
task would exit on its own; the explicit abort makes the ordering
deterministic, as the plan asks.)

### Step D — verify

- `cargo test` — fast suite stays green; the rewritten 17.37-T passes.
- `cargo test --test relay_feeds_web_registry -- --ignored` — 17.38-T
  passes (already does; confirms no regression).
- `cargo test --test daemon_web_state_wired -- --ignored` — 17.36-T still
  passes.
- A manual or `--ignored` end-to-end check that, with the relay enabled,
  a real proto reaching the recv loop produces a WS `stats` event is the
  true acceptance signal but needs a live relay; the in-process 17.38-T
  is the automated stand-in.

## As built — where the implementation diverged from the steps above

All of 17.37/17.38 is implemented and the verification in Step D passed.
The steps above are the original design; four details changed during
implementation. The main plan's checklist (`STEP-17-web-server.md`) already
reflects these; they are recorded here so this note matches the code.

1. **Accessor reaches the sender through `self.inner`, not a mirrored field
   (Step A points 3 and 5).** `player_states_tx` lives only on
   `RuntimeInner`. The accessor is
   `pub fn player_states(&self) -> Receiver<…> { self.inner.player_states_tx.subscribe() }`.
   This leaves the `RelayRuntime` struct and both its constructors
   untouched — fewer construction sites to thread the sender through than
   mirroring the field would have needed.

2. **No `.clone()` on publish (Step A point 4).** `zwift_proto::PlayerState`
   implements `Copy`, so the two emission sites are
   `inner.player_states_tx.send(state)` (state refresher, owned value) and
   `inner.player_states_tx.send(*state)` (recv loop, `&PlayerState`). The
   `state.clone()` shown in Step A would trip `clippy::clone_on_copy`.

3. **The behavioural 17.37 test is a relay unit test, not the integration
   file (Step B).** `start_with_deps` and `inject_event` are `#[cfg(test)]`
   and unreachable from `tests/`, so the fidelity test
   (`player_state_proto_surfaced_on_inbound_with_full_fidelity`) lives in
   `src/daemon/relay.rs`. The integration file
   `tests/relay_surfaces_player_state.rs` was instead rewritten as a
   compile-time check that the public `player_states()` API exists and
   returns `broadcast::Receiver<zwift_proto::PlayerState>`.

4. **Bridge task uses fully-qualified `RecvError` paths (Step C).** The
   match arms are `tokio::sync::broadcast::error::RecvError::Closed` /
   `…::Lagged(_)` since `run_daemon` has no `broadcast` import; otherwise
   the task is as shown.

## What is intentionally NOT in scope

These are carried forward in
[STEP-20-additional-considerations.md](STEP-20-additional-considerations.md)
item 20.19 (with the event-subgroup cache population deferral) so they are
not forgotten:

- Reducing `GameEvent::PlayerState` to `{ athlete_id }` (separable cleanup;
  see "Known redundancy").
- World-meta altitude adjustment and lat/lng projection (already deferred
  with TODOs in `proto_to_stats.rs` / `proto_view.rs`).
- `self_athlete_id` sourcing in `WebState` (17.36-I left an inline TODO).
