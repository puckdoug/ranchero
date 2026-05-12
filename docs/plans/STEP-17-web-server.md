# Step 17 — HTTP + WebSocket server (stub)

## Goal

Replace `sauce4zwift/src/webserver.mjs` with an axum-based server. Must
serve the exact JSON protocol widgets expect (spec §6.3 / §7.9).

- `GET /api/socket` — WebSocket upgrade. Per-client JSON frames:
  ```
  → { "type":"request", "method":"subscribe|unsubscribe|rpc",
      "uid": <int>, "arg": {...} }
  ← { "type":"response", "uid", "success", "data" }
  ← { "type":"event",    "uid":<subId>, "success":true, "data": <...> }
  ```
- `/pages/*` — static file server rooted at a configurable path
  (default: `./pages` relative to binary). The widget tree is
  vendored once from sauce4zwift's `pages/` into ranchero and
  maintained in-tree thereafter; the server must not resolve through
  any path that points back at the sauce4zwift checkout.
- Bind to `server.bind:server.port` from config (default
  `127.0.0.1:1080`).
- HTTPS auto-enables if `./https/{key,cert}.pem` exists.
- Backpressure: drop clients that exceed 8 MB buffered.

## Tests-first outline

- End-to-end: spawn the server, connect a test WS client, drive
  subscribe / event / unsubscribe flows, assert exact JSON frames.
- Backpressure: feed a stuck client; socket is closed after threshold.
- HTTPS conditional: cert files present → TLS listener, absent → HTTP.

To be fully elaborated when work on this step begins.

## Inputs deferred from STEP 14

STEP 14 ships the `zwift-stats` orchestration layer (`DataCollector`,
`DataBucket`, `AthleteData`, `AthleteRegistry`) as a synchronous,
proto-free crate. The daemon-side glue that drives it lives here
because it is where the game monitor (STEP 12), the stats engine
(STEP 14 / 15), and the web server first meet on the same task
graph:

- **GC cron tick.** `AthleteRegistry::gc(now)` must be driven on a
  recurring interval. STEP 14 fixes the default at
  `GC_TICK_INTERVAL_SECS = 62.768` (from `stats.mjs:3553`) but
  records this as an "Open verification point" against the original
  stub's "10 s" figure. STEP 17 decides the production cadence,
  confirms or revises the constant, and wires the
  `tokio::time::interval(GC_TICK_INTERVAL_SECS)` driver. Tests must
  drive `gc()` synchronously rather than relying on the interval —
  the constant is the only operational tunable.
- **PlayerState → `AthleteData::ingest_*` routing.** STEP 12
  delivers decoded `PlayerState` records from the game monitor.
  STEP 14 exposes signal-by-signal `ingest_power / ingest_hr /
  ingest_speed / ingest_cadence / ingest_draft` on `AthleteData`,
  taking raw `f64` values. STEP 17 owns the proto-to-stats
  translation: extracting the right fields from the decoded
  `PlayerState`, converting units where the proto and the stats
  engine disagree, and calling `registry.upsert(...)` followed by
  the five `ingest_*` calls. This is the seam that lets
  `zwift-stats` stay proto-free.
- **`AthleteRegistry::upsert` identity-field handling for
  mid-session course or sport changes.** STEP 14's
  `AthleteRegistry::upsert(athlete_id, course_id, sport, world_time,
  now)` uses the `course_id` and `sport` arguments only on first
  insert; on the existing-athlete path, it calls
  `AthleteData::record_update(world_time, now)`, which advances the
  timestamps and leaves `course_id` and `sport` untouched. (The
  plan's `record_update` signature deliberately takes no identity
  fields.) STEP 17 needs to decide what to do when the same
  `athlete_id` appears with a new `course_id` (rider switched
  worlds or rode through a world boundary) or a new `sport` (rider
  changed activity in the same session). Options to evaluate:
  (a) Overwrite the identity fields on every upsert and accept
  silent drift from the JavaScript behaviour; (b) Detect the
  change and reset the relevant collectors; (c) Treat the change
  as a session boundary and replace the `AthleteData` record
  entirely. The decision lives here because STEP 17 is the first
  place that sees real `PlayerState` records and can confirm what
  the game actually emits across course or sport changes. Record
  the choice in this step's as-built notes and, if option (b) or
  (c) is selected, add a corresponding method on `AthleteRegistry`
  or `AthleteData` in `zwift-stats` so that STEP 17 stays
  proto-free.
- **`MostRecentState` proto-type decision.** STEP 14 ships a
  hand-written `MostRecentState` struct with the minimal field set
  the parity tests need. If STEP 17's proto-routing code reaches
  for fields not yet in the struct, the cleanest move is to make
  `MostRecentState` a re-export of the `zwift-proto` `PlayerState`
  type rather than re-deriving the struct field-by-field. Decide
  this here so `zwift-stats` does not accumulate a parallel proto
  shape over time. Record the decision in this step's as-built
  notes.
- **GC tick interval confirmation.** Resolve STEP 14 Open
  Verification Point 1 (62.768 s vs the stub's 10 s) before
  scheduling the interval. The 10 s figure may have been a typo
  for `_zwiftMetaRefresh` at `stats.mjs:3565`; confirm from a fresh
  read of `stats.mjs` and pin the answer in this step.
