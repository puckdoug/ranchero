# Step 28 — Watched-athlete switching + real game-state event (G8 + V4)

Source: `review.md` findings **G8** and **V4**. Order-of-work item 8. Both
concern the game's session state: who is being watched, and what the
`game-state` stream carries.

## Goal

1. The watched athlete follows the game: when the spectated rider changes in
   Zwift, the daemon notices from telemetry, switches, and emits
   `watching-athlete-change`.
2. The `game-state` event carries a real, sauce-shaped game-state object; the
   daemon's own connection lifecycle moves to its own event name.

## Decisions in force

- **Q7 (2026-06-12): match sauce.** Follow the game's watching state from
  telemetry. This also reactivates parked items 20.13/20.14 (mid-ride course
  transitions).
- **Q4 (2026-06-12): build the sauce-shaped game-state object and emit it
  under `game-state`.** Move the connection lifecycle to its own name (e.g.
  `connection-state`). Reusing an established event name for different data is
  unacceptable.

## Background the implementer needs

- `GameEvent::WatchingAthleteChange` is defined (`src/daemon/relay.rs:1246`)
  and the subscription layer delivers it (`src/web/subs/mod.rs:435-438`), but
  nothing ever sends it.
- `switch_watched_athlete` (`relay.rs:2602`) is production API with no
  caller; `watching_id` is set once at boot (`src/daemon/runtime.rs:307`).
- `game-state` is currently emitted on `GameEvent::StateChange`
  (`src/web/subs/mod.rs:429-434`), carrying daemon runtime states
  (`Authenticating`, `TcpEstablished`, …) — not game data.
- The relay already tracks watched position from the stream; the "who am I
  watching" signal in the telemetry must be identified (sauce's
  `_updateWatchingState`, `zwift.mjs:2260`, is the reference).

## Tests first

### Watched-athlete switching (G8)

- [ ] **28.1-T** When telemetry indicates the game is watching a different
      rider, `switch_watched_athlete` is called with the new id and a
      `WatchingAthleteChange { athlete_id }` is emitted. Drive via injected
      states in the relay harness.
- [ ] **28.1-I** Detect the game's watching signal in the recv path; call
      `switch_watched_athlete` and emit `WatchingAthleteChange`. Route it to
      the web layer via the Step 21 forwarder.
- [ ] **28.2-T** A subscriber on `watching-athlete-change` receives the event
      end-to-end (relay boundary → subscriber).
- [ ] **28.2-I** Covered by 28.1-I + Step 21; lock with the test.

### Game-state object + connection-stream rename (V4)

- [ ] **28.3-T** A new `connection-state` event carries the daemon lifecycle
      values previously sent under `game-state`; a subscriber receives them.
- [ ] **28.3-I** Move the `StateChange`-driven emission to a
      `connection-state` event name in the subscription layer.
- [ ] **28.4-T** `game-state` now carries a sauce-shaped object (world,
      course, in-event flag, …) built from telemetry; a subscriber receives
      it and it matches the sauce field shape.
- [ ] **28.4-I** Build the game-state object from the watched athlete's
      state and emit it under `game-state`. (May need a new `GameEvent`
      variant or a state-derived producer.)
- [ ] **28.5-T** `getGameState` RPC returns the same object.
- [ ] **28.5-I** Back the RPC with the game-state object.

## Acceptance criteria

- Changing the spectated rider in Zwift switches the daemon's watched athlete
  and fires `watching-athlete-change`.
- `game-state` carries game data in sauce's shape; `connection-state` carries
  the lifecycle; no name is overloaded.
- Fast suite green.

## Dependencies

- **Step 21** (event forwarding) is required for both events to reach
  subscribers.

## Deferred

- Mid-ride course-transition handling (parked 20.13/20.14) becomes relevant
  again with switching live, but is its own follow-up, not part of this step.
