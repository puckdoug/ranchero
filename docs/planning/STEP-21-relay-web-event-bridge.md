# Step 21 — Forward relay game events to the web layer (G1 + K4)

Source: `review.md` finding **G1** (with **K4** as the verifying test).
Order-of-work item 1. This is the highest-leverage fix: the chat, ride-on,
segment-result, and game-state event work from earlier steps is already
correct in the subscription layer but never reaches a client, because the
relay and the web layer use two different broadcast channels.

## Goal

Every `GameEvent` the relay decodes in production reaches WebSocket
subscribers. After this step, a frame injected at the relay boundary produces
the matching `chat` / `rideon` / `game-state` event on a subscribed socket.

## Background the implementer needs

- `run_daemon` builds the channel the web layer listens on
  (`src/daemon/runtime.rs:299-303`) and hands it to `WebState`
  (`and_game_events`).
- `RelayRuntime::start_with_writer` builds a **separate** channel
  (`src/daemon/relay.rs:1435`); the relay emits all its events there.
- Only `PlayerState` crosses today, and it crosses indirectly: the bridge
  task (`src/daemon/runtime.rs:341-366`) consumes the full-proto stream, runs
  `route_player_state`, then emits `GameEvent::PlayerState` onto the web
  channel (`src/web/proto_to_stats.rs:228`). This ordering — registry
  updated *before* the fanout sees the event — is deliberate.

**Critical subtlety.** The relay *also* emits `GameEvent::PlayerState` on its
own channel (`relay.rs:624`, `:3528`, `:3709`). If you simply share one
channel between relay and web, `PlayerState` is emitted twice and the
ordering guarantee breaks (the relay's direct emit bypasses
`route_player_state`). Therefore the bridge must remain the sole producer of
`PlayerState` on the web channel, and the new forwarder must forward **every
variant except `PlayerState`**.

## Tests first

`-T` is a failing test; `-I` is the implementation that turns it green.

- [ ] **21.1-T** `tests/relay_web_event_bridge.rs` — start a daemon-like
      harness wiring a `RelayRuntime` (with stub transports, as in
      `tests/relay_runtime.rs`) to a `WebState` through the production
      forwarder. Inject a `WorldUpdate` carrying a chat/SocialAction payload
      at the relay boundary; assert a subscriber on `stats` / `chat` receives
      one event with the expected fields. This is the K4 "edge-to-subscriber"
      test the suite is missing.
- [ ] **21.1-I** Add a forwarder task in `run_daemon`, beside the bridge,
      that subscribes via `runtime.events()` (`relay.rs:2617`) and re-sends
      every variant **except `GameEvent::PlayerState`** onto
      `web_state.game_events_tx`. Handle `Lagged` by continuing and `Closed`
      by exiting, mirroring the bridge loop.
- [ ] **21.2-T** Same harness: inject a `RideOn` `WorldUpdate`; assert a
      `rideon` subscriber receives it.
- [ ] **21.2-I** No new code expected beyond 21.1-I (the forwarder is
      variant-agnostic); this test exists to lock the second event family.
- [ ] **21.3-T** Ordering guard: inject a `PlayerState` proto and assert the
      web `athlete/{id}` subscriber receives **exactly one** event (not two),
      proving the forwarder does not duplicate what the bridge already emits.
- [ ] **21.3-I** If 21.3-T fails, the forwarder is forwarding `PlayerState`;
      exclude that variant.
- [ ] **21.4-T** `game-state` reaches a subscriber when the relay emits
      `StateChange` (note: V4 / Step 28 later changes what `game-state`
      *carries*; here only verify the StateChange path is delivered, so this
      test asserts on the current StateChange payload shape).
- [ ] **21.4-I** Covered by 21.1-I; lock with the test.

## Acceptance criteria

- A frame injected at the relay boundary produces the matching web event on a
  subscribed socket, for `chat`, `rideon`, and `game-state`.
- `PlayerState` is delivered exactly once and still after `route_player_state`
  has run (registry-before-fanout preserved).
- `cargo test --test relay_web_event_bridge` passes; full fast suite stays
  green. Mark any test that spawns a daemon `#[ignore]` with a `slow:` reason.

## Dependencies

None — this is the first item and unblocks visibility of Steps 24/26/28 work.

## Deferred

Replacing the StateChange payload with a real game-state object is **Step 28**
(finding V4). This step only carries whatever `StateChange` currently emits.
