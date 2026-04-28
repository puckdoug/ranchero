# Step 12 — GameMonitor orchestration (stub)

## Goal

The thin coordinator that owns one TCP channel + N UDP channels,
consumes `udpConfigVOD` updates from `ServerToClient` messages, and
routes to the correct UDP pool based on the watched athlete's current
`(realm, courseId)` and position (spec §4.8, §4.13).

- `findBestUDPServer(pool, x, y)` — port spec §4.8 exactly:
  `useFirstInBounds` short-circuit else min-Euclidean-distance.
- Idle suspension: when watched athlete shows `_speed = 0 &&
  _cadence = 0 && power = 0` for ~60 s, shut down UDP; resume on any
  non-zero field (spec §4.13).
- Emits a `GameEvent` enum (player state, world update, latency, state
  change) to downstream consumers.

## Tests-first outline

- `findBestUDPServer` table-driven tests over synthetic pools.
- Suspension FSM: forced idle → shutdown after 60 s; mid-shutdown
  activity resumes immediately.
- Watched-athlete switch triggers UDP reselection.

To be fully elaborated when work on this step begins.
