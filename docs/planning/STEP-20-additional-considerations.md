# Step 20 — Additional considerations (parking lot)

## Purpose

A running list of items to consider later. These items surface during
earlier-step work but do not justify pausing the current step to
resolve. Each entry should be self-contained: where it came from, what
the trade-off looks like, and when to revisit.

Triage when starting any new step: any item here that the new step
naturally touches gets pulled into that step's elaboration. Items left
behind here are either accepted or revisited at the end of the
porting effort.

---

## Implementation plan (built 2026-05-24)

This section turns the work that the 2026-05-24 answers committed to (items
20.21–20.28 plus the sub-items they activated in 20.17 / 20.19 / 20.20) into
ordered, test-first steps. The parking-lot items 20.1–20.20 stay parked: they are
conditional deferrals with their own "revisit when …" triggers and are *not* part
of this plan.

**Out of scope by decision** (do not build): write-back to Zwift — the write RPCs
and any write fetchers (QA1); a fourth `event_subgroups.sqlite` database (QD2);
repacking position into a `latlng` array — ranchero emits `lat`/`lng` scalars
(QE1).

**Conventions every step follows:**

- **Test-first.** Write the tests listed under "Tests first", watch them fail,
  then write the smallest code under "Implementation" that turns them green. Do
  not write implementation before the failing test exists.
- **Slow-test marking.** Any test that spawns a daemon or runs ≥ 100 ms must be
  `#[ignore]` with a reason starting `slow:` (see `README.md`). The fast set must
  stay fast.
- **Run.** Use the narrow command while iterating (`cargo test -p <crate>` or a
  single test path), and `cargo test -- --include-ignored` before calling a step
  done.
- **Field visibility.** POD/snapshot types expose `pub` fields; stateful
  aggregators keep fields private behind accessors.
- **Order.** Steps are listed in dependency order; the "Depends on" line in each
  step is authoritative. Several later steps can proceed in parallel once their
  dependencies are met.
- Plan-file moves into `docs/plans/done/` are Doug's, not the implementer's.

### Checklist

Each step is two checkboxes: write the failing tests (red), then implement until
they pass (green). Do not tick the implementation box while any of the step's
tests are still failing.

- **Step 1** — Profile fetch + `Profile` struct (`getProfiles` / `getProfile`, FTP)
  - [x] ① Tests (red)
  - [x] ② Implementation (green)
- **Step 2** — Read-through athlete profile cache + `athletes.sqlite` (JSON blob)
  - [x] ① Tests (red)
  - [x] ② Implementation (green)
- **Step 3** — Self identity (`self` = watched athlete)
  - [x] ① Tests (red)
  - [x] ② Implementation (green)
- **Step 4** — Per-tick recording I: current state, streams, grade, stale guard
  - [x] ① Tests (red)
  - [x] ② Implementation (green)
- **Step 5** — Per-tick recording II: road history, auto-lap, bucket growth, time/kJ splits
  - [x] ① Tests (red)
  - [x] ② Implementation (green)
- **Step 6** — Per-tick recording III: W′ balance + power zones
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 7** — Relay: watched position from stream + UDP server selection fix
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 8** — Relay: consume inbound UDP telemetry
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 9** — Relay: rebuild UDP on TCP reconnect
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 10** — Relay: WorldUpdate decode + new `GameEvent` variants
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 11** — Relay: Ghost drop, heartbeat content, multipleLogins, refresher self/429
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 12** — 1 Hz nearby/groups processor + event sources + gap estimation
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 13** — Event chain: proto tags 29/34 + `getEvent` + subgroup cache + detection
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 14** — Live event streams: chat, rideon, game-state, watching-athlete-change
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 15** — Segment leaderboards: fetchers + `segments.sqlite` + evictor + active-segment
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 16** — RPC read-only getter surface
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 17** — Route tables (`zwift-routes` crate) + route progress
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 18** — World-meta tables + position projection (lat/lng scalars)
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)
- **Step 19** — Persistence wiring audit + `GameEvent::PlayerState` cleanup
  - [ ] ① Tests (red)
  - [ ] ② Implementation (green)

### Step 1 — Profile fetch and `Profile` struct

**Goal.** Add the Zwift REST profile fetchers and extend `Profile`, giving the
rest of the plan a real source for athlete identity, name, and FTP.
**Resolves.** 20.26 (`getProfiles`, `getProfile`); the QC3 blocker (`Profile` is
`{ id }` only today, `crates/zwift-api/src/lib.rs:113`).
**Depends on.** Nothing.

**① Tests first (red)** — `crates/zwift-api/tests/` (wiremock, fast):
- `get_profile_parses_ftp_and_name`: mock `GET /api/profiles/{id}`; assert the
  parsed `Profile` exposes `id`, `first_name`, `last_name`,
  `ftp` (from `functional_threshold_power`), `weight`, and the privacy flag.
- `get_profiles_batch_returns_all`: mock the batch endpoint (sauce
  `zwift.mjs:559`); assert both ids come back, in request order.
- `get_profile_missing_ftp_is_none`: a profile without FTP → `ftp == None`, not 0.
- `get_profiles_propagates_auth_error`: a 401 surfaces an `Error`, not an empty
  result.

**② Implementation (green).**
- Extend `Profile` (`crates/zwift-api/src/lib.rs:113`) with `first_name`,
  `last_name`, `ftp: Option<f64>`, `weight: Option<f64>`, and the privacy flag;
  keep it POD with `pub` fields.
- Add `ZwiftAuth::get_profile(id)` (`/api/profiles/{id}`, sauce `zwift.mjs:541`)
  and `get_profiles(ids)` (batch, `zwift.mjs:559`), mirroring `get_profile_me`'s
  request shape and its `Accept: application/json` header (the 20.16 fix).

**Done when.** `cargo test -p zwift-api` passes the four tests and production code
can read a profile's FTP.

### Step 2 — Read-through athlete profile cache + `athletes.sqlite` (JSON blob)

**Goal.** A `WebState` in-memory profile cache, populated from `getProfiles`,
written through to `athletes.sqlite` (JSON-blob schema), and read by the
formatters so `athlete`, `ftp`, and `tss` stop being null.
**Resolves.** QC2 (live authoritative, SQLite is the cache), QD1 (JSON-blob
schema), 20.20 item 1 (gaps G1/G2), 20.17 item 1 (write side), 20.28 item 1.
**Depends on.** Step 1.

**① Tests first (red).**
- `crates/zwift-store/tests/`: `athletes_db_roundtrips_json_blob` — migrate to
  `athletes(id INTEGER PRIMARY KEY, data TEXT)`, upsert a full athlete JSON, and
  read it back intact including `marked`, `following`, `gender`, privacy.
  `athletes_db_marked_query` — `json_each(data,'$.marked')` returns marked ids
  (sauce `stats.mjs:2440`).
- web/format tests: `format_v1_populates_athlete_from_cache` — a cache entry with
  name + FTP makes `athlete` non-null and `ftp` set; `tss_computed_from_ftp` —
  with NP + FTP, `tss` is non-null (gap G2); `athlete_null_on_cache_miss` —
  no entry → `athlete: null` (parity).
- `profile_cache_serves_live_then_falls_back_to_sqlite`.

**② Implementation (green).**
- Replace the fixed-column `athletes` schema with the JSON-blob `data TEXT`
  schema in `crates/zwift-store`; `AthletesDb::upsert/get/touch` operate on the
  serialized athlete; add a `marked()` query.
- Add a profile cache to `WebState` (`src/web/state.rs`) populated from
  `get_profiles` (Step 1) and written through to `AthletesDb`; live data is
  authoritative, SQLite is the fallback consulted only before live data exists.
- Have `format_athlete_data_v1` / `format_athlete_v2` (`src/web/format.rs`) read
  the cache for `athlete`/`ftp` and compute `tss` from FTP.

**Done when.** `cargo test -p zwift-store` and the format tests pass; a watched
athlete with a known FTP shows name, FTP, and TSS.

### Step 3 — Self identity (`self` = watched athlete)

**Goal.** Populate `self_athlete_id` so the `self` aliases and self-comparisons
work.
**Resolves.** QC1, 20.19 item 3 (`TODO 17.36-I`).
**Depends on.** Nothing (small; placed early because self-only fields need it).

**① Tests first (red).**
- `web_state_self_id_equals_watched` — building `WebState` from a config with
  `watched_athlete_id = N` yields `self_athlete_id == Some(N)`.
- `self_alias_resolves_to_watched` — `GET /athlete/self` returns the watched
  athlete's record, and `apply_event_state`'s self-comparison uses the watched id
  (not the `0` fallback).

**② Implementation (green).**
- In `src/daemon/runtime.rs:305-306`, set
  `s.self_athlete_id = cfg.watched_athlete_id.map(|id| id as u32);` and remove
  `TODO 17.36-I`.
- Read `WebState.self_athlete_id` in the `self` aliases and `apply_event_state`.
- The monitor account's `auth.athlete_id()` is **not** used for self (see QC1).

**Done when.** `self` aliases resolve and the TODO is gone.

### Step 4 — Per-tick recording I: current state, streams, grade, stale guard

**Goal.** Wire the first slice of `_recordAthleteStats` / `_preprocessState` into
`route_player_state`: store the current state, record streams, publish grade, and
reject stale/duplicate packets.
**Resolves.** 20.21 (`most_recent_state`, `record_streams`, grade publication,
stale guard), QF3, the settled `road_time` finding.
**Depends on.** Step 1.

**① Tests first (red)** — `crates/zwift-stats/tests/` and the `src/web`
proto_to_stats tests:
- `records_most_recent_state` — after one ingest, `ad.most_recent_state` is
  `Some(state)` and the v1/v2 payload `state` is non-null.
- `records_streams` — distance/altitude/latlng/wbal stream vectors grow by one
  per tick.
- `publishes_grade` — `state.grade` equals the smoothed grade (today computed and
  discarded).
- `road_time_reverse_adjustment` — a reverse rider gives
  `road_time = 1005000 - raw`; forward gives `raw - 5000` (sauce `zwift.mjs:321`).
- `rejects_stale_or_duplicate_state` — a packet with `elapsed <= 0` versus the
  last is dropped and rolling sums are unchanged (`stats.mjs:3146`).

**② Implementation (green).**
- In `route_player_state` (`src/web/proto_to_stats.rs`): set `most_recent_state`,
  call `record_streams`, keep and publish the smoothed grade, and add the
  `elapsed <= 0` guard before ingest.
- Fix `ProtoView::road_time` (`src/web/proto_view.rs:75`) to apply the reverse
  adjustment from the direction bit.

**Done when.** `state` and `state.grade` appear in payloads; tests pass.

### Step 5 — Per-tick recording II: road history, auto-lap, bucket growth, time/kJ splits

**Goal.** Record road history, detect auto-laps, grow the per-lap/segment/event
buckets, and split work/follow/solo/coffee time and kJ.
**Resolves.** 20.21 (`road_history.record`, `auto_lap_check`, slice/bucket growth,
the four time and kJ splits) and the settled "DataCollector has no growth
mechanism" finding.
**Depends on.** Step 4.

**① Tests first (red)** — `crates/zwift-stats/tests/`:
- `data_collector_resize_grows_window` — a new growth method extends capacity and
  preserves existing samples.
- `data_slice_grows_after_new_from` — `DataSlice::new_from` (`slice.rs:22`) then
  growth yields a bucket that accumulates (today `clone_reset` leaves it empty).
- `auto_lap_detected_by_distance_and_time` — crossing the threshold
  (`stats.mjs:3092`) creates a lap; `lapCount` and `laps[]` update.
- `road_history_recorded` — `road_history.record` appends, respecting the reverse
  adjustment from Step 4.
- `work_follow_solo_coffee_split` — across ticks with draft/coffee state, the four
  time buckets and `workKj`/`followKj`/`soloKj` accumulate (`stats.mjs:3397-3463`).

**② Implementation (green).**
- Add a growth method to `DataCollector` (`crates/zwift-stats/src/collector.rs`)
  and make `DataSlice` growable.
- In `route_player_state`: call `road_history.record`, `auto_lap_check`,
  create/grow slices, and accumulate the time and kJ splits.

**Done when.** Per-lap stats and time/kJ splits are non-zero; tests pass.

### Step 6 — Per-tick recording III: W′ balance + power zones

**Goal.** Configure and accumulate W′ balance and time-in-power-zones from the
profile FTP.
**Resolves.** 20.21 (W′/zones), QC3.
**Depends on.** Step 2 (profile FTP), Step 4 (recording loop).

**① Tests first (red)** — `crates/zwift-stats/tests/`:
- `wbal_configured_from_profile` — with `cp = ftp` and `w_prime = 20000`,
  `WBalAccumulator` gives a non-null series that depletes above CP and recovers
  below (sauce `stats.mjs:2864-2867`).
- `wbal_defaults_when_no_cp` — missing CP falls back to FTP; missing W′ falls back
  to 20000 (`wPrimeDefault`, `stats.mjs:15`).
- `zones_configured_from_ftp` — `ZonesAccumulator::configure(ftp,
  getPowerZones(ftp))` then accumulation fills `timeInPowerZones`; a null FTP
  gives empty zones (parity).
- `get_power_zones_matches_coggan` — the ported `getPowerZones(ftp)` returns the
  expected boundaries (sauce `stats.mjs:1223-1241`).

**② Implementation (green).**
- Port `getPowerZones(ftp)` (Coggan zones).
- In `route_player_state`: on the first state and on FTP change, call
  `wBal.configure(cp, w_prime)` (CP = FTP fallback, W′ = 20000) and
  `timeInPowerZones.configure(ftp, zones)`, then accumulate each tick.

**Done when.** `wBal` and `timeInPowerZones` populate; tests pass.

### Step 7 — Relay: watched position from stream + UDP server selection fix

**Goal.** Feed the watched athlete's live `(x, y, courseId, portal)` into UDP
server selection, and fix the selection fall-through.
**Resolves.** 20.25 item 3, QF1.
**Depends on.** Nothing in this plan (relay-internal); pairs with Step 8.

**① Tests first (red)** — relay tests (mark daemon-spawning ones `#[ignore]`
`slow:`):
- `watched_position_updates_from_stream` — successive PlayerStates update
  `WatchedAthleteState` `(x, y, course)` (today seeded `(0,0)`/course 0;
  `observe_watched_player_state` / `switch_watched_athlete` are `#[cfg(test)]`,
  `relay.rs:2512,2530`).
- `recompute_udp_selection_uses_live_position` — selection evaluates against the
  live position, not `(0,0)`.
- `find_best_udp_server_no_swap_when_out_of_bounds` — with `use_first_in_bounds`
  and the rider outside every bound, returns `None` (no swap), matching sauce
  `zwift.mjs:2277-2299` (today falls through to nearest-centre, `relay.rs:921`).
- `find_best_udp_server_bounds_and_distance_match_upstream` — reconcile the
  bounds test and the distance reference (centre vs corner) with upstream (QF1).

**② Implementation (green).**
- Promote `observe_watched_player_state` / `switch_watched_athlete` out of
  `#[cfg(test)]` and call them from the recv loop / state-refresher.
- Fix `find_best_udp_server`: return `None` when nothing is in bounds under
  `use_first_in_bounds`; reconcile the bounds test and distance reference.

**Done when.** UDP server choice tracks the rider; tests pass.

### Step 8 — Relay: consume inbound UDP telemetry

**Goal.** Process inbound UDP `ServerToClient` instead of discarding it.
**Resolves.** 20.25 item 1 (high impact).
**Depends on.** Step 7 (UDP channel pointed at the right server).

**① Tests first (red)** — relay tests plus a capture fixture:
- `udp_inbound_player_states_reach_bridge` — injecting a UDP `ServerToClient`
  with player states emits `GameEvent::PlayerState`(s) (today the UDP recv arm is
  a no-op, `relay.rs:3448`).
- `udp_and_tcp_inbound_decode_identically` — the two transports share the decode
  path (extract the helper from 20.2 if convenient).

**② Implementation (green).**
- Replace the no-op `ChannelEvent::Inbound(_stc)` UDP arm (`relay.rs:3448`) with
  the same processing the TCP inbound branch uses: decode, then route player
  states and world updates to `game_events_tx`.

**Done when.** Live telemetry flows over UDP; tests pass.

### Step 9 — Relay: rebuild UDP on TCP reconnect

**Goal.** Re-establish the UDP channel and heartbeat after a TCP reconnect.
**Resolves.** 20.25 item 2.
**Depends on.** Step 8.

**① Tests first (red)** — relay tests (`#[ignore]` `slow:` if daemon-spawning):
- `tcp_reconnect_reestablishes_udp` — after a simulated TCP drop and reconnect, a
  new UDP channel and heartbeat open and `watched_id`/`game_events_tx` are
  retained (today discarded, `relay.rs:3056-3061`; `resume_udp` is single-shot).
- `udp_survives_multiple_reconnects` — two reconnects, UDP still flowing.

**② Implementation (green).**
- In `connection_manager` (`relay.rs:2851`): on reconnect, re-open UDP and the
  heartbeat, carrying `watched_id` / `game_events_tx`; make `resume_udp`
  reusable.

**Done when.** UDP keeps working across TCP reconnects; tests pass.

### Step 10 — Relay: WorldUpdate decode + new `GameEvent` variants

**Goal.** Decode `WorldUpdate` payloads and surface them as `GameEvent` variants.
**Resolves.** 20.25 item 4; the source for Step 14 (chat/rideon) and the live
SegmentResult half of Step 15.
**Depends on.** Step 8.

**① Tests first (red)** — relay/proto tests with crafted WorldUpdate payloads:
- `decodes_rideon_world_update` → a `GameEvent::RideOn { … }`.
- `decodes_chat_world_update` (SocialAction) → a `GameEvent::Chat { … }`.
- `decodes_segment_result_world_update` — payloadType 105 → a SegmentResult event.
- `unknown_world_update_is_ignored` — an unknown type does not panic.

**② Implementation (green).**
- Port sauce's WorldUpdate dispatch (`zwift.mjs:2164-2187`): `< 100` by nested
  protobuf name (RideOn, SocialAction, PlayerLeftWorld, PlayerRegisteredForEvent,
  NotableMoment, …), `≥ 100` via binary decoders (SegmentResult = 105).
- Add the new `GameEvent` variants (today only `PlayerState`, `Latency`,
  `StateChange`, `PoolSwap`) and emit them from the recv loop (replacing the
  timestamp-only read at `relay.rs:3354-3372`).

**Done when.** WorldUpdates decode into the new variants; tests pass.

### Step 11 — Relay: Ghost drop, heartbeat content, multipleLogins, refresher self/429

**Goal.** The smaller relay-parity items.
**Resolves.** QE2 (Ghost/NINJA drop), 20.25 item 5 (heartbeat content), item 6
(multipleLogins), item 7 + QF2 (refresher self/429).
**Depends on.** Step 7 (position), Step 3 (self id for the self-poll).

**① Tests first (red)** — relay/stats tests:
- `drops_player_state_when_ghost_powerup` — a state whose decoded `activePowerUp`
  (low 4 bits of `aux3`, proto field 20) is NINJA/Ghost (enum 6) is dropped
  (sauce `zwift.mjs:2194`).
- `heartbeat_includes_portal_roadid_eventsubgroup` —
  `HeartbeatScheduler::next_state` (`relay.rs:797`) forwards the watched
  athlete's `portal`, roadId, and `eventSubgroupId` (sauce `zwift.mjs:1942-1957`).
- `warns_on_multiple_logins` — a state with `multipleLogins` set logs a warning
  (sauce `zwift.mjs:2144`).
- `refresher_polls_self_when_self_ne_watching` and
  `refresher_suppresses_429_logging` (sauce `_refreshStates`, `zwift.mjs:1998`).

**② Implementation (green).**
- Decode `activePowerUp`; drop NINJA/Ghost states at ingest.
- Extend `HeartbeatScheduler::next_state` with portal/roadId/eventSubgroup.
- Detect `multipleLogins` and warn.
- State-refresher: also poll self when self ≠ watching; treat HTTP 429 as
  expected (no error log).

**Done when.** Tests pass.

### Step 12 — 1 Hz nearby/groups processor + event sources + gap estimation

**Goal.** A 1 Hz tick that computes nearby + groups, sets
gap/group-id/event-rank, emits the `nearby`/`groups` (v1 + v2) events, and fixes
the event-name routing bug.
**Resolves.** 20.22, 20.27 item 2 (EventSubgroupPlacements), and the settled
`event_matches_athlete` bug.
**Depends on.** Steps 4/5 (`most_recent_state` + road history).

**① Tests first (red)** — stats and web/subs tests:
- `compute_groups_sets_group_id` and `apply_gap_sets_gap_fields` (today `None`).
- `nearby_sorted_by_gap` — `/nearby/*` returns riders sorted by gap, not HashMap
  order (`http/mod.rs:247`).
- `groups_non_empty_when_group_id_set` (`http/mod.rs:297`).
- `event_matches_athlete_rejects_nearby_groups` — the bug fix: `nearby`/`groups`
  are not treated as per-athlete events (`src/web/subs/mod.rs`).
- `nearby_ws_emits_sorted_array_not_single_athlete` — the WS `nearby`
  subscription delivers a sorted array (today single-athlete payloads).
- `incremental_gap_estimation` — adjacent-rider chaining (`refSpeedForEst`,
  `incRP`) fills missing gaps (sauce `_computeNearby`).
- `event_subgroup_placements_processed` — `ev_subgroup_ps = 23` fills
  `eventPosition` / `eventParticipants`.

**② Implementation (green).**
- Add a 1 Hz tick task in `src/web/state.rs` (sibling to `gc_tick_loop:95`)
  running `compute_groups` + nearby, setting gap/group/event-rank, and emitting
  `nearby` / `groups` / `nearby/v2` / `groups/v2`.
- Add `nearby` / `groups` producers in `src/web/subs/`; fix
  `event_matches_athlete`.
- Port incremental gap estimation into `crates/zwift-stats/src/gap.rs`.
- Process `EventSubgroupPlacements`.

**Done when.** Nearby/groups widgets receive sorted arrays; tests pass.

### Step 13 — Event chain: proto tags 29/34 + `getEvent` + subgroup cache + detection

**Goal.** Make events detectable from telemetry and populate event-context fields.
**Resolves.** QA3, QB1, 20.27 item 1, 20.26 (`getEvent`), 20.19 item 4 (event
spread of G4).
**Depends on.** Step 1 (REST), Step 4 (recording), Step 12 (event rank, optional).

**① Tests first (red).**
- Capture verification (QB1): decode proto tags 29 and 34 from a sanitised
  capture and assert they carry the event-subgroup id and the event distance
  (cm), confirming the sauce reading over the zwift-offline labels. If the
  capture disproves it, the test records the correct meaning instead.
- `proto_view_exposes_event_subgroup_id_and_distance` — the accessors return the
  decoded values (today hardcoded `0` / `0.0`).
- `get_event_parses_subgroups` — wiremock `/api/events/{id}` (sauce
  `zwift.mjs:808`).
- `apply_event_state_detects_event` — a state with a non-zero subgroup id and a
  populated cache returns the event (today always `Idle`).
- `formatter_spreads_event_fields` — `eventLeader`, `eventSweeper`, `remaining*`
  present on a cache hit, absent on a miss (parity).

**② Implementation (green).**
- Reinterpret vendored proto tags 29/34 (`udp-node-msgs.proto:151,156`) per QB1:
  rename with a comment recording the deviation (precedent: `draft = 10`, line
  122); wire the `ProtoView` accessors.
- Add `ZwiftAuth::get_event(id)`; populate the in-memory `WebState.event_subgroups`
  cache from it (no SQLite, per QD2 — repopulate after restart).
- Make `apply_event_state` use the real subgroup id; spread the event half of
  `_getEventOrRouteInfo` in the formatters.

**Done when.** A rider in an event shows event context; tests pass.

### Step 14 — Live event streams: chat, rideon, game-state, watching-athlete-change

**Goal.** Emit the non-per-athlete event streams over WebSocket.
**Resolves.** 20.24, 20.20 item 2 (`gameState` field), the `app` / `setting-change`
source.
**Depends on.** Step 10 (WorldUpdate variants), Step 3 (self/watching).

**① Tests first (red)** — web/subs tests:
- `chat_stream_emits_on_world_update` and `rideon_stream_emits_on_world_update`
  (from the Step 10 variants; sauce `stats.mjs:2650,2591`).
- `game_state_stream_and_field` — a `game-state` producer plus the `gameState`
  formatter field for self (`stats.mjs:1250`, 20.20 item 2).
- `watching_athlete_change_emitted` — switching the watched athlete fires the
  event (`stats.mjs:2659`).
- `app_source_setting_change` — subscribing to `app` for `setting-change` works
  (today `create_delegation` knows only `stats` / `gameConnection`); ties to
  `getSetting` / `setSetting` in Step 16.

**② Implementation (green).**
- Add subs producers for `chat`, `rideon`, `game-state`, and
  `watching-athlete-change`; add a game-state object; register the `app` source.

**Done when.** Chat/rideon/game-state widgets receive data; tests pass.

### Step 15 — Segment leaderboards: fetchers + `segments.sqlite` + evictor + active-segment

**Goal.** Fetch segment results/leaderboards, cache them, evict expired rows, and
detect active segments.
**Resolves.** QA2, 20.26 (segment fetchers), 20.17 item 2 (evictor), 20.21
(`active_segment_check`).
**Depends on.** Step 5 (road history); segment **detection** also depends on
segment geometry from Step 18 — the fetchers/cache can land first.

**① Tests first (red).**
- `get_segment_results_parses` / `get_live_segment_leaders_parses` /
  `get_live_segment_leaderboard_parses` — wiremock (sauce `zwift.mjs:633-645`).
- `segments_db_evict_expired_called_on_schedule` — a scheduled task calls
  `SegmentsDb::evict_expired(now)` (the method exists and is tested; here wire
  the caller).
- `active_segment_detected` — crossing a segment boundary populates `segments[]`
  (`stats.mjs:3077`); gated on Step 18 geometry.

**② Implementation (green).**
- Add the three fetchers; write results into `segments.sqlite`; schedule
  `evict_expired`.
- Wire `active_segment_check` into `route_player_state` once Step 18's segment
  geometry is available.

**Done when.** Leaderboards are fetched, cached, and evicted; segment detection
works once geometry lands; tests pass.

### Step 16 — RPC read-only getter surface

**Goal.** Register the read-only RPC handlers widgets call (write actions excluded
per QA1).
**Resolves.** 20.23 (read-only subset).
**Depends on.** Steps 2, 12, 13, 15 (data to return).

**① Tests first (red)** — `src/web/rpc.rs` tests:
- One test per handler group asserting a registered handler returns the expected
  shape: athlete getters (`getAthlete`, `getAthletes`, `getAthleteData`,
  `getAthletesData`, `getAthleteLaps`, `getAthleteSegments`, `getAthleteStreams`,
  `getPlayerState`, `getPowerZones`, `getPowerProfile`); `getNearbyData` /
  `getGroupsData`; event getters (`getCachedEvent(s)`, `getEvent`,
  `getEventSubgroup`, `getEventSubgroupEntrants` / `Results`); `getSegmentResults`;
  `getChatHistory`; `getGameState`; geometry getters (`getWorldMetas`,
  `getCourseId`, `getRoad`, `getRoute`, `getSegment`, …); settings (`getSetting`,
  `setSetting`, `getDebugInfo`, `getWebServerURL`, `getZwiftLoginInfo`,
  `getZwiftConnectionInfo`).
- `write_rpcs_not_registered` — `setFollowing`, `giveRideon`, `updateAthlete`,
  etc. are absent (QA1).

**② Implementation (green).**
- Register the handlers in `RpcRegistry::new` (`src/web/rpc.rs:17`), each
  delegating to the data sources built in earlier steps.

**Done when.** Read RPCs resolve; write RPCs return `unknown rpc handler` by
design; tests pass.

### Step 17 — Route tables (`zwift-routes` crate) + route progress

**Goal.** Vendor route/curve tables and compute route progress.
**Resolves.** QA4, 20.27 item 3, the route half of `_getEventOrRouteInfo`.
**Depends on.** Step 13 (event/route info plumbing).

**① Tests first (red)** — new `crates/zwift-routes/tests/`:
- `route_lookup_by_id` — a known route resolves to its distance/segments.
- `compute_route_distance` — `_computeRouteDistance` (`stats.mjs:3197`) returns
  the expected metres for a sample position.
- `route_remaining_fields` — `routeDistance`, route %, and
  `remaining`/`remainingMetric`/`remainingType`/`remainingEnd` populate (today
  hardcoded `None`).

**② Implementation (green).**
- Create `crates/zwift-routes` (route + curve tables from `shared/routes.mjs` +
  `shared/curves.mjs`, spec §7.2 / §7.8).
- Port `_computeRouteDistance` and the route branch of `_getEventOrRouteInfo`;
  wire the formatter fields.

**Done when.** Route-progress fields populate; tests pass.

### Step 18 — World-meta tables + position projection (lat/lng scalars)

**Goal.** Vendor world-meta tables and project position to
altitude/lat/lng/x/y/roadCompletion/progress.
**Resolves.** 20.19 item 2 (G3), 20.27 item 4, QE1 (scalars), 20.26
(`getGameInfo` / world metas).
**Depends on.** Step 4 (state recording).

**① Tests first (red).**
- `altitude_adjusted_by_world_meta` —
  `(z - seaLevel + eleOffset) / 100 * physicsSlopeScale` (today raw `z / 100`).
- `latlng_projected` — `state.lat` / `state.lng` are real (today `0.0`), emitted
  as separate scalars, not a `latlng` array (QE1 — documented divergence).
- `web_mercator_x_y` and `road_completion_and_progress` populate.
- `get_game_info_parses` — wiremock `/api/game_info` (sauce `zwift.mjs:681`).

**② Implementation (green).**
- Vendor the world-meta tables; implement the altitude adjustment and projection
  in `src/web/proto_to_stats.rs` / `src/web/proto_view.rs` (replace the TODOs);
  keep `lat` / `lng` as scalars.
- Add `get_game_info` / world-meta data.

**Done when.** Map-position fields populate as scalars; tests pass.

### Step 19 — Persistence wiring audit + `GameEvent::PlayerState` cleanup

**Goal.** Confirm the store DBs are actually read and written in production
end-to-end, do the vestigial-variant cleanup, and optionally enrich
`ranchero status`.
**Resolves.** 20.28 item 3, QE3, 20.17 item 3 (optional).
**Depends on.** Steps 2 and 15 (the writers).

**① Tests first (red).**
- `stores_read_and_written_in_production` — a daemon run (or an integration
  harness) shows `upsert` / `get` / `put` / `evict_expired` called outside tests
  (today bound as `_stores`, `src/daemon/stores.rs` / `run_daemon`).
- `game_event_player_state_is_athlete_id_only` — the reduced variant compiles and
  the fanout still works (QE3).
- (optional) `status_reports_row_counts` — `ranchero status` shows `user_version`
  and per-table row counts (20.17 item 3).

**② Implementation (green).**
- Ensure the earlier steps' readers/writers use the opened DBs (so `_stores` is no
  longer inert); add the end-to-end assertion.
- Reduce `GameEvent::PlayerState` to `{ athlete_id }` (re-touch the ~6 test files
  and 2 relay tests).
- (optional) enrich `format_persistence_status`.

**Done when.** Persistence is exercised in production; the cleanup is done; tests
pass.

---

## Open items

### 20.1 — Virtual-time vs. real-time in async HTTP tests (from STEP 07)

**Where it came from.** The
`preemptive_refresh_fires_at_half_expires_in` test in
`crates/zwift-api/tests/auth.rs` originally used
`#[tokio::test(flavor = "current_thread", start_paused = true)]` plus
`tokio::time::advance(...)` so the half-life elapsed in virtual time
without a real-world wait. It deadlocked: after the scheduled
`tokio::time::sleep(expires_in / 2)` woke, the spawned refresh task
issued a `reqwest` round-trip to wiremock, which needs the IO driver
to make progress; however, on a `current_thread` runtime the reactor
only turns when the runtime parks, and the test task was busy
yielding, so the runtime never parked.

**Current resolution.** The test uses a 2 s `expires_in` (1 s
half-life) and a real `tokio::time::sleep(Duration::from_millis(2000))`.
This adds approximately 2 s of wall time to the suite and uses no
virtual-time machinery. A comment in the test explains the choice.

**Why this might come back.** Subsequent steps add more
time-driven background tasks against mock HTTP servers:

- STEP 09 — relay session refresh at ~90% of session lifetime.
- STEP 10 / 11 — UDP/TCP channel watchdogs (>30 s silent → reconnect),
  exponential backoff on reconnect.
- STEP 12 — `GameMonitor` supervision and reconnect cadence.

If several real-time waits accumulate to a noticeable suite slowdown
(for example, more than 5 s aggregate), revisiting is warranted.
Options:

1. **`flavor = "multi_thread"` + manual `tokio::time::pause()`** after
   the mock server is up. The IO driver runs on a worker thread, so
   reqwest can make progress while the test task yields. Cost: a
   `std::time::Instant` deadline loop in the test (since `tokio::time`
   is paused), which is awkward.
2. **Inject the clock and sleeper.** A `trait Clock` / `trait Sleeper`
   abstraction in `zwift-api` (and any other crate that schedules
   work) would let tests substitute a deterministic in-memory
   implementation, with no real sleeps and no interaction between
   virtual time and IO. Cost: an extra abstraction layer in
   production code, paid for by every consumer of the crate.
3. **Status quo.** Accept short real-time sleeps as the cost of
   testing time-driven behavior end-to-end through real `reqwest`
   and wiremock. Cost: the suite is a few seconds slower per such
   test.

**Decision rule.** Revisit when (a) total real-time test wait crosses
approximately 5 s, or (b) a flaky failure appears tied to scheduling
jitter on CI. Until then, the status quo is retained.

### 20.2 — Shared inbound-decode helper between UDP and TCP channels (from STEP 11)

**Where it came from.** STEP 11's plan recommended extracting
`process_inbound` (header decode → relay_id validation → IV state
mutation → AES-128-GCM-4 decrypt → `ServerToClient::decode`) into
a private module shared by `udp.rs` and `tcp.rs`. The two copies of
the function differ only in one constant: `ChannelType::UdpServer`
versus `ChannelType::TcpServer` in the IV construction.

**Current resolution.** The function was not extracted. Two
near-identical copies of `process_inbound` reside in
`crates/zwift-relay/src/udp.rs` and `crates/zwift-relay/src/tcp.rs`.
A shared helper parameterized on channel type would add one
indirection (passing the channel type as a parameter, or as a
generic) for one line of difference; this provides little value at
this step.

**Why this might come back.**

- A third channel type appears (the companion-app reverse channel is
  spec §6 out-of-scope today, but is listed there).
- The two copies begin to diverge; for example, one channel adds
  inbound envelope handling, error retry, metrics counters, or trace
  spans that the other does not need. At that point, either the
  divergence is real and the helper would have hidden it, or the
  divergence is a defect introduced by editing one copy and
  forgetting the other.
- A reviewer identifies the duplication as a code smell.

**Decision rule.** Extract when (a) the two copies have diverged
beyond the `ChannelType` constant in a way that would have been
caught by a shared helper, or (b) a third channel type is
implemented. Until then, the duplication is the lower-cost choice.

### 20.3 — HTTP-client and policy-string injection beyond URL override (from STEP-12.5 §F)

**Where it came from.** STEP-12.5 §F.3 closed the testability gap
on `RelayRuntime::start` by adding URL-only injection for the
Zwift auth and game-API endpoints: a `[zwift]` section in the
config file, `RANCHERO_ZWIFT_AUTH_BASE` and
`RANCHERO_ZWIFT_API_BASE` environment variables, and a
`zwift_endpoints` field on `ResolvedConfig`. Two larger
redesigns were considered alongside that work and deliberately
excluded from §F so they could be evaluated on their own merits
rather than introduced as a side effect of the testability fix.
STEP-12.5 §F.5 records the exclusion; this parking-lot entry is
the place to revisit it.

**Current resolution.** Both items are deferred. Neither is
required for the operator-facing capability or the test
infrastructure produced by §F.

1. **Injecting a higher-level HTTP-client trait into
   `ZwiftAuth`.** `zwift_api::ZwiftAuth` constructs a
   `reqwest::Client` internally.
   `ZwiftAuth::with_client(http, config)` already exists so
   callers can share a `reqwest::Client` for connection pooling
   across multiple instances (for example, the main and monitor
   accounts in a future multi-account configuration). A
   trait-based HTTP client would let tests substitute an
   in-memory transport and bypass `reqwest` and wiremock
   entirely, but URL-only injection — already exercised by
   every test in `crates/zwift-api/tests/auth.rs` and
   `crates/zwift-relay/tests/session.rs` — is sufficient to
   keep ranchero's tests away from production Zwift endpoints
   and matches the pattern the rest of the workspace uses.
2. **Surfacing `source` and `user_agent` to operator
   configuration.** These two `zwift_api::Config` fields
   default to `"Game Client"` and `"CNL/4.2.0"`. They are
   policy values that mimic Zwift's own client and have no
   operator-relevant effect on testability or
   staging-environment redirection. §F.3 leaves them at the
   library defaults rather than expanding the schema for
   fields no current deployment needs.

**Why this might come back.**

- A future spec or behaviour change requires Zwift identifying
  the client differently — for example, a self-hosted relay
  that refuses connections without a custom user agent, or a
  per-deployment differentiation scheme. At that point
  `source` and `user_agent` need an operator-facing knob and
  the schema work in §F.3.1 / §F.3.2 is the natural place to
  add them.
- Test infrastructure outgrows wiremock. Examples that would
  push toward a trait-based HTTP client: asserting request
  headers in a way wiremock does not support, injecting
  HTTP-level latency to exercise retry and backoff paths, or
  exercising connection-pool behaviour without real sockets.
- A consumer of `zwift-api` outside of ranchero needs to mock
  HTTP at a level wiremock cannot reach.

**Decision rule.** Revisit when (a) operator configuration of
`source` or `user_agent` is required by a real deployment
scenario, or (b) test infrastructure needs to substitute the
HTTP client itself, not just its target URL. Until then, the
URL-only injection plus `ZwiftAuth::with_client` is the
lower-cost choice.

### 20.4 — Configuration extensibility (from STEP-02, STEP-02.1)

**Where it came from.** Three items declared deferred in earlier
configuration work and never picked up:

- **Schema-version migrations.** STEP-02 deferred the migration
  story until a v2 schema actually exists. The current
  `serde`-derived parser reads a v1 schema only.
- **Configuration categories beyond v1.** STEP-02 listed mods,
  route overrides, and other sauce-only categories as
  out-of-scope until a real consumer needs them.
- **`--editing-mode` command-line flag.** STEP-02.1 added the
  `editing_mode = "default" | "vim"` field to the configuration
  file but deferred the corresponding CLI flag. Today the
  configuration file is the only way to choose.

**Current resolution.** The configuration parser in `src/config/`
accepts the v1 schema. There is no migrator and no v2 schema.
The TUI honours `editing_mode` from the file; no CLI override
exists.

**Why this might come back.** A deployment that needs per-mod
or per-route configuration, a schema change that is not
backwards-compatible with v1, or an operator who wants to switch
between vi and default editing modes without rewriting the
configuration file.

**Decision rule.** Revisit each sub-item when a concrete
deployment requirement appears. The migration framework only
becomes worth building once a v2 field is actually being added;
until then, a one-line `version = 1` check is enough.

### 20.5 — TUI vi-mode completeness and mouse support (from STEP-02.1, STEP-02.2)

**Where it came from.** STEP-02.2 ported a subset of vi
navigation; several motions and editing operations were
deferred, alongside two TUI-input items from STEP-02.1.
Specifically:

- `gg` and `G` (jump to first / last screen).
- `0`, `$`, `^` line motions in outer Normal mode.
- Numeric prefix counts (`3j`, `5l`, `2dd`).
- `c{motion}` and `s` (change and substitute).
- Cross-field paste from the edtui clipboard into the outer
  paste buffer (a `dw` inside an edtui field today does not
  populate `paste_buffer` in `src/tui/model.rs`).
- Custom `:` commands beyond the documented set
  (`:w`, `:wq`, `:x`, `:q`, `:q!`, `:u`, `:undo`).
- Redo (`Ctrl-R` / `:redo`).
- Mouse support, resize handling beyond ratatui's defaults,
  mouse cursor positioning within fields, and click-to-focus.
- Visual-mode selector widget for the log-level enumeration
  (currently a free-text field in the configuration TUI).

**Current resolution.** The TUI driver implements the subset
that covers everyday configuration editing. None of the items
above are present in `src/tui/`. The TUI runs in keyboard-only
mode; mouse events are ignored.

**Why this might come back.** A user who relies on full vi
muscle memory finds the gaps disruptive, or a deployment
context (for example, a remote SSH session over a
mouse-capable terminal) makes the keyboard-only choice
awkward. A reviewer who consistently expects a working
`Ctrl-R` would also push toward this.

**Decision rule.** Add motions on demand: when an item is
requested by a real user with a concrete workflow that the
omission blocks, port the equivalent edtui or
`tui-input`-level binding. The full mouse track-and-click set
is a larger piece of work; defer until the keyboard-only
choice is contested by a real user, not by a hypothetical
preference.

### 20.6 — Syntax highlighting in the Review-screen TOML preview (from STEP-02.1)

**Where it came from.** STEP-02.1 deferred syntax highlighting
in the Review screen's read-only TOML preview, on the basis
that a plain monospace render is sufficient for sanity-checking
a configuration before save.

**Current resolution.** The preview pane renders the serialised
TOML in the default style. No `tree-sitter`, `syntect`, or
grammar reference exists in `src/tui/`.

**Why this might come back.** A configuration grows large
enough that the human eye benefits from coloured key/value
distinction, or a future schema introduces nested tables and
arrays where mismatched delimiters become hard to spot in a
plain render.

**Decision rule.** Defer until the configuration schema crosses
roughly 50 lines in typical use, or a Review-screen rendering
defect is traced to mis-formatted TOML being hard to spot.

### 20.7 — Daemon log rotation, structured output, and shipping (from STEP-03, STEP-04)

**Where it came from.** STEP-03 and STEP-04 deferred three
related logging features:

- **Log rotation.** `src/logging/mod.rs` opens the daemon log
  file with `OpenOptions::new().create(true).append(true)`. No
  `tracing_appender::rolling` consumer is in place. A
  long-running daemon grows the file without bound.
- **JSON / structured log output.** Only `fmt::layer()` is
  configured. No JSON layer, no operator-selectable output
  format.
- **Log shipping to external collectors.** No OTLP, syslog, or
  vector-style sink. Operators who want centralised logging
  have to tail the file from outside the daemon.

**Current resolution.** Operators rotate the log externally
(`logrotate` or equivalent). Structured output is not
available; the daemon writes a human-readable line format only.

**Why this might come back.** The first long-running production
deployment will exhaust disk space without rotation. A
deployment under a centralised log policy will need either JSON
output or a shipping sink. These are operational must-haves
once ranchero is run anywhere other than a developer laptop.

**Decision rule.** Pick this up before the first deployment
intended to run for more than a week without supervision.
`tracing_appender::rolling` plus a `--log-format=json` CLI flag
is the minimum viable response; the shipping piece can wait
until a specific collector is required.

### 20.8 — Cross-platform daemon: Windows service and Linux capability drop (from STEP-03)

**Where it came from.** STEP-03 deferred two
operating-system-specific items:

- **Windows service integration.** The current daemon assumes a
  POSIX `fork`. Windows has no equivalent; a service-control
  shim using `windows-service` would be required.
- **Privileged-capabilities drop on Linux.** When the daemon is
  started by a process with elevated capabilities (for example
  to bind a low port), it should drop everything not strictly
  required after binding. The current process inherits whatever
  the parent had.

**Current resolution.** Ranchero runs on Linux and macOS only.
The daemon does not drop capabilities on Linux. Neither item is
required for the current deployment target (a developer or a
single user running the daemon on their own machine).

**Why this might come back.** A Windows port is requested, or
ranchero is deployed under `systemd` with `AmbientCapabilities`
and a security audit asks for a defence-in-depth capability
drop after `setsockopt`.

**Decision rule.** Windows: defer until a Windows port is on
the roadmap. Capability drop: defer until ranchero is packaged
for `systemd` or another supervisor that grants ambient
capabilities; at that point the drop is two `caps`-crate calls
and a test that verifies effective and permitted sets are empty
after binding.

### 20.9 — `ranchero follow` enhancements (from STEP-12.2)

**Where it came from.** STEP-12.2 implemented a polling
follower for the capture file. Five enhancements were deferred:

- **File-system event notification.** The follower polls. An
  `inotify` (Linux) / `kqueue` (BSD/macOS) watch via the
  `notify` crate would reduce wake-ups and improve latency on
  small writes.
- **Capture-file rotation support.** If the capture writer ever
  rotates (see 20.7), the follower must reopen the new file.
  No reopen logic exists today.
- **JSON output mode for `--decode`.** Today the decoded form
  is human-readable text only.
- **Filter flags.** Direction (inbound/outbound), transport
  (UDP/TCP), and message-type filters are not exposed.
- **"From offset" or "from timestamp" follower mode.** The
  follower starts at end-of-file. Replaying a window from the
  middle of a long capture is not supported.

**Current resolution.** The follower works for the documented
`ranchero start --capture out.cap; sleep 5; ranchero follow
out.cap` flow. Anything beyond that requires external tools.

**Why this might come back.** A debugging session that needs to
inspect a specific traffic class (for example, only TCP
inbound), an automation scenario that pipes JSON into another
tool, or a capture-file rotation choice that breaks the
follower.

**Decision rule.** Pick up filter flags and JSON output the
first time a debugging session would have benefited from them.
File-system event notification is a latency optimisation;
defer until polling becomes a bottleneck. Rotation support
becomes mandatory the same day capture rotation is enabled.

### 20.10 — `RelayRuntime::start_*` consolidation (from STEP-12.11)

**Where it came from.** STEP-12.11 deferred retiring the
`start_inner` and `start_with_deps*` family of entry points.
Each was introduced for a specific test-injection need; the
overlap between them has grown.

**Current resolution.** `src/daemon/relay.rs` exposes
`start_with_all_deps` (used by tests), `start_with_deps`
(legacy), and `start_inner` (legacy). Production code calls
`start`. The duplication is real but stable.

**Why this might come back.** A new injection point is needed
that does not fit any existing entry, forcing yet another
overload, or a refactor of `start_all_inner` exposes the
duplication as a maintenance hazard.

**Decision rule.** Consolidate when (a) a fourth entry point
would otherwise be added, or (b) a behaviour change requires
editing all of `start_inner`, `start_with_deps`, and
`start_with_all_deps` in lock-step. Until then, the
duplication is the lower-cost choice.

### 20.11 — Relay-protocol cosmetic and niche items (from STEP-12.14, STEP-12.15)

**Where it came from.** Three relay-protocol items were
flagged in earlier reviews and explicitly deferred because the
server tolerates the current behaviour or the items did not
unblock the trace they were investigating:

- **Portal-pool handling (STEP-12.14 §k3).** Sauce honours UDP
  pools keyed by a portal `(realm, course)` pair when the
  watched athlete is on a portal road. Ranchero's
  `find_best_udp_server` falls back to the generic `(0, 0)`
  pool. A stub test exists at
  `tests/relay_runtime.rs:portal_pool_handled_via_portal_key`;
  no production code reads the portal key.
- **TCP non-hello flag=0 cleanup and hello SEQNO=0 omission
  (STEP-12.14 §M3 / §k1).** A header-encoding cosmetic
  difference from sauce. The Zwift relay tolerates both.
- **Proto-fork items N1, N12, C11 (STEP-12.15).** Marked in
  STEP-12.15 as "fix only if C5 + C6/7/8 don't unblock the
  trace, and they did".

**Current resolution.** None of the three has any operational
effect on the smoke test or daily use. They remain visible in
the source plans for future reference.

**Why this might come back.** Portal-pool handling becomes
relevant the moment a watched athlete enters a portal road and
a UDP pool actually exists for that portal; the fall-back
behaviour will then send packets to a sub-optimal server. The
cosmetic header items become relevant only if a future relay
server tightens validation.

**Decision rule.** Portal-pool handling: implement once a
captured trace shows portal pools being received but ignored.
Cosmetic items: implement only if a server-side change makes
the current behaviour an error.

### 20.12 — Auth and session resilience: broader retry, error counting (from STEP-12.16)

**Where it came from.** STEP-12.16's "deferred follow-ups"
section called out three resilience gaps that did not block
the smoke test on a healthy first run:

- **Auth and session-login retry across all error categories.**
  `start_with_retry` currently retries only `TcpConnect`,
  `NoUdpConfig`, and `EstablishedTimeout`. A transient 503 from
  the auth endpoint exits the daemon. Sauce retries every
  error category through `_schedConnectRetry`.
- **UDP error-count threshold reconnect.** Sauce's
  `incErrorCount()` (`zwift.mjs:1934-1939`) calls
  `_schedConnectRetry` after every 10 UDP errors. Ranchero has
  no equivalent counter and no equivalent reconnect trigger.
- **Reconnecting at the auth or session layer beyond F3 / F4.**
  F3 handles session refresh; F4 handles TCP-channel shutdown.
  A failure in the auth round-trip itself, or a session
  re-establishment that succeeds at the supervisor level but
  fails downstream, has no broader retry envelope.

**Current resolution.** A mistyped password is a fatal exit (a
defensible choice for a real deployment). A transient auth-side
503 is also fatal (a less defensible choice). UDP errors are
counted only at the channel level; no error budget feeds back
into reconnect.

**Why this might come back.** The first multi-day production
run will exhibit transient auth-side and UDP-side errors that
the current implementation cannot ride through.

**Decision rule.** Pick this up once a multi-day smoke run
exposes the failure mode. Implementation: extend the
`start_with_retry` retryable-error set; introduce a
per-channel UDP error counter that signals
`reconnect_needed.notify_one()` after a threshold; classify
auth errors as retryable / non-retryable based on the response
code (5xx and connection errors retryable; 4xx fatal).

### 20.13 — Mid-ride course transitions and resume reuse (from STEP-12.16)

**Where it came from.** STEP-12.16 §7 declared mid-ride course
transitions out of scope for the smoke-test-resilience plan.
The resume code path was implemented for "athlete enters a
game while the daemon is suspended" only.

**Current resolution.** When the watched athlete enters a game
the first time, the daemon transitions out of suspended state,
brings UDP up, and emits `relay.runtime.resumed`. When the
watched athlete moves between courses mid-ride (for example,
crossing a portal into a different world), no equivalent
transition fires. UDP packets continue on the channel
established for the original course; if that pool is no longer
optimal, throughput suffers but the session does not break.

**Why this might come back.** A user who frequently uses portals
or world-hop events sees stale UDP-server choice; the multi-UDP
swap (item 20.14) is the natural place to attach this logic
once that feature exists.

**Decision rule.** Implement alongside item 20.14. The reuse
case is "feed the new course id through the same
`resume_udp_tx` channel and have it close the old UDP channel
after a 60-second grace window".

### 20.14 — Completion of partial implementations (from STEP-12.11, STEP-12.14, STEP-12.16)

**Where it came from.** Two placeholders are wired into the
runtime but the underlying behaviour is not actually executed:

- **Sticky TCP server selection across reconnects** (STEP-12.11,
  restated in STEP-12.14 §L4 and STEP-12.16). The pinned IP is
  tracked at `src/daemon/relay.rs:1693` and the supervisor-event
  handler emits `relay.runtime.tcp_server_pinned`, but the
  reconnect path does not actually re-establish on the pinned
  address — it picks `tcp_servers[0]` from whatever set the
  supervisor most recently emitted.
- **Multi-UDP-channel with grace-shutdown swap** (STEP-12.14
  §L6). `recompute_udp_selection` at `src/daemon/relay.rs:519`
  emits `relay.udp.channel.grace_shutdown` and broadcasts
  `GameEvent::PoolSwap`, but the body of the spawned 60-second
  grace task contains a literal `// Placeholder: actual channel
  transfer is implemented in L6` comment. The new channel is
  not actually opened; the old channel is not actually closed.

**Current resolution.** The trace events fire and the symbols
exist, so the contract surface looks complete from outside. The
behavioural contract is not honoured. STEP-12.20 lists these as
"implemented (partial)" rather than missing, on the grounds
that the wiring is real even though the body is not.

**Why this might come back.** A reconnect during a mid-session
TCP shutdown picks an arbitrary server rather than the pinned
one; a multi-UDP swap on a portal entry never actually happens.
Both turn into observable behavioural defects the moment the
respective scenario is exercised.

**Decision rule.** Complete L4 (TCP pinning) the first time a
real reconnect picks a different server than the one pinned;
the failure is silent today but visible in
`relay.tcp.connect.attempt` traces. Complete L6 (UDP swap)
either alongside item 20.13 or when the first portal-entry
trace shows the swap is needed. Both are well-scoped pieces of
work — neither warrants its own STEP, but both should be
elevated out of "parking lot" once a concrete scenario is in
hand.

### 20.15 — State-refresher cadence (acknowledged out of scope; from STEP-12.16)

**Where it came from.** STEP-12.16 §7 explicitly excluded
operator-tunable state-refresher cadence. The 3-second minimum
/ 30-second expanding / 5-minute cap behaviour from STEP-12.14
§L1 is the locked-in choice.

**Current resolution.** The cadence values are constants in
`src/daemon/relay.rs:584-587`. There is no operator override.
This is recorded here for completeness, not because the values
are expected to change.

**Why this might come back.** A deployment scenario where the
3-second minimum poll rate causes detectable load on the Zwift
auth endpoint (in aggregate across many ranchero instances), or
a debugging scenario where the operator wants to force a
slower or faster cadence for reproduction.

**Decision rule.** Do not add the knob until a concrete
scenario justifies it. If the scenario appears, the change is
mechanical: thread two `Duration` fields through
`ResolvedConfig` into the state-refresher.

### 20.16 — Auth-failure response-body diagnostics (from STEP-12.17)

**Where it came from.** STEP-12.17 fixed the missing
`Accept: application/json` header on `get_profile_me`, which had
caused a real-account smoke run to fail with
`relay.auth.profile.failed status=200 variant="BadSchema"`. The
incident exposed two diagnostic shortcomings that turned a single
header omission into a hard-to-investigate failure:

- The `BadSchema` trace records only `status` and
  `variant="BadSchema"`; the response Content-Type — the most
  diagnostic field for "200 but wrong body type" — is not
  captured.
- The `Error::AuthFailedBadSchema` message
  (`crates/zwift-api/src/lib.rs:73-74`) renders as
  `"authentication failed: unexpected response shape: expected
  value at line 1 column 1"` once the serde error is appended.
  The serde "line 1 column 1" string buries the actionable
  signal (which is "the body is not JSON at all").

**Current resolution.** Both diagnostic improvements were noted
during STEP-12.17 but kept out of the in-plan fix on the principle
that the immediate fix (adding the missing header) is the smallest
change that makes the smoke pass. The diagnostics make the *next*
failure of the same class self-diagnosing; the smoke does not need
them today.

**Why this might come back.** Any future failure where a Zwift
endpoint returns a 200 with an unexpected body type — server
rolling out a new content type, an account-flagged response, an
intermediate proxy reformatting bodies — produces the same opaque
`BadSchema` error today. The first time that recurs, this entry
becomes the cheapest path to a self-diagnosing trace and a
self-explanatory error message.

**Decision rule.** Implement when (a) a second `BadSchema`
incident happens on a different endpoint or under different
conditions, and reading `relay.auth.profile.failed` traces is no
longer enough to identify the cause; or (b) the operator-facing
error message at the daemon-exit boundary is rewritten for any
other reason, at which point folding the body prefix in costs
nothing. The change is local: one extra `tracing` field on the
BadSchema branch in `get_profile_me`, and one extra argument to
the `AuthFailedBadSchema` error variant carrying a body-prefix
slice.

### 20.17 — SQLite persistence deferrals (from STEP-16)

**Where it came from.** STEP-16 shipped `zwift-store` with three
SQLite databases (`store.sqlite`, `athletes.sqlite`,
`segments.sqlite`) but explicitly excluded six items from its
"Out of scope" section. None of the six is tracked elsewhere; this
entry is the parking lot for all of them so a future reader can find
them without re-reading STEP-16.

The six items split into three deferred-work items (expected to land
in a later step), one spec-level deferral (FIT export), and two
deliberate non-features (no encryption at rest, no operational
hygiene tooling). They are grouped here because they share a single
subsystem and decision-making context.

**Deferred work, expected in a later step:**

1. **Live `AthleteData` → `athletes.sqlite` persistence.** STEP-16
   built `AthletesDb::upsert`/`touch`/`get` but no caller writes
   ingest data into them. The store is exercised only by its own
   tests. The natural home is the step that joins `zwift-stats`
   ingest to persistence — STEP-16 calls this out as "probably
   STEP 18+" without committing.
2. **Background eviction for the segments cache.**
   `SegmentsDb::evict_expired(now) -> Result<usize>` exists and is
   tested, but no scheduled task calls it. The natural home is
   whichever step first writes leaderboards into the cache
   (segment-leaderboard fetcher, not yet planned). Until that step
   lands, the cache is unused and unbounded growth is not a risk.
3. **Schema introspection in `ranchero status`.** The persistence
   block today is bytes-only. A future enhancement could report
   `user_version`, row counts per table, or the oldest/newest
   `last_seen` in the athletes cache. No step has committed to
   this; it is a low-priority operator-ergonomic item.

**Spec-level deferral:**

4. **FIT export of finished sessions.** Deferred past v1 per the
   spec stub (`stats.mjs:2057` in sauce4zwift's `exportFIT`) and
   CLAUDE.md. Not a STEP-NN item — a v2 concern. Listed here only
   so a reader of STEP-16 can find the trail.

**Deliberate non-features (the "current resolution" is "never,
unless the threat model changes"):**

5. **Encryption at rest for the SQLite files.** Credentials live in
   the OS keyring (STEP-05); the SQLite files contain no secrets
   (athlete profiles, KV settings, cached leaderboards). Adding
   SQLCipher or equivalent would cost a vendored fork of
   `rusqlite`, an operator-managed key, and migration tooling for
   existing on-disk DBs, in exchange for protecting data that is
   not sensitive. Revisit only if the schema grows to hold
   personally-identifying or financially-sensitive data, or a
   deployment context (multi-tenant, regulated industry) requires
   encryption-at-rest as a checkbox.
6. **Backups, vacuum scheduling, integrity checks.** SQLite's
   defaults plus WAL are sufficient for a single-user daemon.
   `VACUUM` reclaims space after large deletes (which do not happen
   in v1: athletes accumulate, segments expire-and-overwrite but
   the row count stays bounded). `PRAGMA integrity_check` is a
   recovery tool, not a steady-state task. Operator backups
   (`cp` while the daemon is stopped, or `.backup` over the SQLite
   CLI while it is running) sit outside ranchero by design — the
   same place log rotation sits today (see 20.7).

**Why this might come back.**

- Items 1 and 2 come back the moment the upstream subsystem
  (`AthleteData` ingest, segment-leaderboard fetcher) is wired in
  and would otherwise duplicate the in-memory state across
  restarts.
- Item 3 comes back when an operator reports that the persistence
  block is too thin to diagnose a real symptom (for example,
  "athletes cache is huge — how many rows is that").
- Item 4 comes back if v2 takes FIT export off the deferred list.
- Items 5 and 6 come back only on a threat-model or
  deployment-context change. Neither is expected.

**Decision rule.**

- Items 1 and 2: pull into the step that introduces the upstream
  subsystem. Do not implement speculatively.
- Item 3: implement on first operator request that the current
  bytes-only line is insufficient. Each new field is a one-line
  addition to `format_persistence_status` plus a corresponding
  `KvStore` / `AthletesDb` / `SegmentsDb` accessor.
- Item 4: tracked by the spec, not by this plan; no action here.
- Items 5 and 6: do not implement. Re-evaluate only on an explicit
  deployment-context or schema change that invalidates the
  reasoning above.

### 20.18 — Web-server feature deferrals (from STEP 17)

**Where it came from.** STEP 17's "Out of scope for STEP 17" section listed
six sauce4zwift web-server features that ranchero deliberately does not build
in the initial web-server step. None pointed to a concrete later step; this
entry is the parking lot for all six so a future reader finds them without
re-reading STEP 17. (The formatter-dependent routes and the v2 deep
resource filter from that same section are *not* here — they have a concrete
home in STEP 18 and are recorded there.)

1. **Per-message WebSocket compression.** Ranchero encodes the full frame on
   every emission; sauce's three-buffer no-re-encode write pattern is also
   skipped. The Rust encoder is fast enough at the expected localhost traffic
   volume. Revisit when a non-localhost deployment scenario makes wire size or
   re-encode cost matter.
2. **Mod web roots and the mod-management surface.** Ranchero has no mod
   loader; the mod-management RPCs and `/mods/<mod-id>/` static mounts wait
   for a step that introduces mods.
3. **Native window manifests (`window-manifests.json`,
   `getWebWindowManifests`).** Ranchero has no native window manager
   equivalent to Electron's `BrowserWindow`; the RPC stays unregistered until
   a native-window concept exists.
4. **Browser-source assets and the patron / EULA pages.** Vendored into
   `pages/` because the tree is copied wholesale, but no route or RPC supports
   them functionally. Revisit if a future step introduces a browser-source
   workflow.
5. **HTTPS certificate provisioning (ACME / Let's Encrypt).** Operators bring
   their own certs today. Automated provisioning is a later step, driven by a
   deployment that needs it.
6. **WebSocket authentication.** Sauce serves the WebSocket with no auth
   (loopback only by default); ranchero matches. This is a deliberate
   match-sauce decision, not pending work — binding to `0.0.0.0` is the
   operator's responsibility. Recorded here for completeness.

**Why this might come back.** Items 1–5 each return with the deployment
scenario named in their text (non-localhost traffic, a mod loader, a native
window manager, a browser-source workflow, automated cert management). Item 6
returns only if ranchero's threat model changes to require authenticated
WebSocket access.

**Decision rule.** Items 1–5: pull into the step that introduces the named
capability; do not implement speculatively. Item 6: do not implement unless
the deployment model stops being loopback-by-default.

### 20.19 — Relay-to-web data-path follow-ups (from STEP 17)

**Where it came from.** The relay-to-web bridge (STEP 17 items 17.36–17.38,
detailed in `STEP-17-relay-web-bridge-design.md`) is functional and tested,
but four gaps were deliberately left open. Three were listed in the design
note's "What is intentionally NOT in scope"; one is the event-subgroup cache
population deferred inside the proto-to-stats section ("out of scope for
17.31"). Grouped here because they share the bridge / proto-to-stats
subsystem.

1. **Reduce `GameEvent::PlayerState` to `{ athlete_id }`.** The variant
   carries eleven scalar fields but only `athlete_id` is read downstream (the
   stats fanout looks the athlete up in the registry; the full proto travels
   on the dedicated `player_states` stream). The surplus scalars are
   vestigial and harmless. Reducing the variant is a mechanical cleanup that
   would re-touch six test files and two relay tests for no functional gain.
2. **World-meta altitude adjustment and lat/lng projection.**
   `route_player_state` and `ProtoView` currently store raw `proto.z / 100`
   as altitude (no `(z - seaLevel + eleOffset) / 100 * physicsSlopeScale`
   adjustment) and return `0.0` for `lat`/`lng`. Both need the world-meta
   tables — a STEP-14-era data file not yet vendored. TODOs mark the spots in
   `src/web/proto_to_stats.rs` and `src/web/proto_view.rs`.

   **STEP 18 dependency (gap G3).** The `_formatState` formatter
   (`src/web/format.rs::format_state`) inherits this gap. STEP 18 leaves the
   following state fields absent because they all need the world-meta
   projection: `state.latlng` (sauce4zwift's `[lat, lng]` pair),
   `state.x`/`state.y` (Web-Mercator projection), `state.roadCompletion`,
   and `state.progress`. There is also a named **deviation**: where
   sauce4zwift emits a single `latlng: [lat, lng]` array, ranchero emits
   separate `lat`/`lng` scalar fields. When the world-meta tables are
   vendored, decide whether to repack `lat`/`lng` into a `latlng` array in
   `format_state` (full parity) or keep the scalars as a documented API
   extension. See `docs/planning/STEP-18-parity-ledger.md` (`_formatState`
   table) and STEP 19's widget-compatibility note.
3. **`self_athlete_id` sourcing in `WebState`.** `run_daemon` cannot yet
   determine the logged-in athlete's own id at boot, so `self_athlete_id` is
   `None` (inline `TODO 17.36-I`). The `self` aliases in the athlete
   endpoints and the `apply_event_state` self-comparison fall back to `0`
   until it is sourced from the monitor/self identity.
4. **Event-subgroup cache population.** `WebState.event_subgroups` exists and
   `apply_event_state` reads it, but no background fetch fills it (out of
   scope for 17.31). Every lookup misses, so `apply_event_state` returns
   `Idle` — matching sauce4zwift's behaviour while its own background fetch is
   pending. A real population task (fetch event subgroups from the Zwift API
   and refresh the cache) is the deferred work.

   **STEP 18 dependency (gap G4, part).** The `_getEventOrRouteInfo` spread
   in both `format_athlete_data_v1` and `format_athlete_v2`
   (`src/web/format.rs`) depends on this cache. Until it is populated, the
   spread fields `eventLeader`, `eventSweeper`, `remaining`,
   `remainingMetric`, `remainingType`, and `remainingEnd` are absent —
   parity-correct, because sauce4zwift omits them too when its own cache
   misses. They become available when this population task lands. See
   `docs/planning/STEP-18-parity-ledger.md`.

**Why this might come back.** Item 1 is pure cleanup — pick it up if the
vestigial fields ever cause confusion. Item 2 returns when a widget needs
true altitude/grade or map position. Item 3 returns as soon as any feature
must distinguish the logged-in rider (the `self` endpoint aliases are already
degraded without it). Item 4 returns when event/sub-group widgets need live
event context rather than always-`Idle`.

**Decision rule.** Item 1: cleanup-only, no trigger required; do it when
touching the variant for another reason. Item 2: implement with the
world-meta table vendoring (a data-file step). Item 3: implement the moment a
feature depends on self-identity — it is the highest-priority of the four.
Item 4: implement alongside the event-subgroup fetcher when event widgets are
built.

### 20.20 — Formatter data-source deferrals (from STEP 18)

**Where it came from.** STEP 18 ported every v1/v2 payload formatter to
field-for-field parity, but several formatter fields read data ranchero does
not yet compute. The formatters emit `null` or omit those fields, which is
parity-correct because sauce4zwift does the same when its own source is
absent (see the gap discussion in `STEP-18-format-payloads.md` and the
field-by-field status in `docs/planning/STEP-18-parity-ledger.md`). Two of
the STEP 18 gaps (G3 state world-coordinates, and the event/route spread
half of G4) already have a home in 20.19 items 2 and 4 and are cross-referenced
there. The remaining STEP 18 data-source gaps have no other home and are
collected here so they are not forgotten.

1. **Athlete-profile read cache — `athlete` field and FTP/TSS (gaps G1, G2).**
   `_formatAthleteData`/`_formatAthleteDataV2` read `this._athletesCache`
   to populate the `athlete` field (name, FTP, weight, privacy) and to
   compute `tss` from FTP. Ranchero's formatters
   (`format_athlete_data_v1`, `format_athlete_v2` in `src/web/format.rs`)
   have no profile cache in `WebState`, so they pass `athlete: null` and
   `ftp: None` — which makes `tss` null everywhere. This is the **read**
   cache the formatters consume, distinct from the **write**-side
   persistence in 20.17 item 1 (`AthleteData` → `athletes.sqlite`). The
   work is to populate an in-memory profile cache in `WebState` (sourced
   from the Zwift API profile fetch and/or `athletes.sqlite`) and have the
   formatters read it. Closing this also closes G2 automatically, since
   `tss` only needs the FTP that the profile carries.
2. **`gameState` (gap G4, part).** `_formatAthleteData`/`_formatAthleteDataV2`
   include `gameState: self ? this._gameState : undefined` — emitted only
   for the logged-in rider, sourced from the game-connection state.
   Ranchero has no game-connection state object yet, so the formatters emit
   `game_state: None` (omitted). Returns when a game-connection state
   producer exists (related to, but separate from, the `gameConnection`
   subscription source stubbed in `src/web/subs/mod.rs`).
3. **`...userDefined` spread (gap G4, part).** Both formatters spread
   `...ad.userDefined` as their last step — arbitrary caller-supplied
   key/value pairs merged into the payload. Ranchero's `AthleteData` has no
   `userDefined` map and no producer for one, so nothing is spread. Returns
   when a feature needs to attach user-defined fields to the athlete payload.

**Why this might come back.** Item 1 returns the moment any widget needs the
athlete's name/FTP or a real TSS — it is the highest-impact of the three,
because two visible fields are null without it. Item 2 returns when a
game-state widget (or the `gameConnection` source) is built. Item 3 returns
only if a feature introduces user-defined athlete fields.

**Decision rule.** Item 1: implement with (or immediately after) the profile
cache wiring into `WebState`; pull 20.17 item 1 alongside it if persistence
is the chosen source. Items 2 and 3: pull into the step that introduces the
named producer; do not implement speculatively.

---

## Items found in the final implementation review (2026-05-23)

The items above (20.1–20.20) are deliberate deferrals: each was a conscious
trade-off made during a step, with the rest of that step finished around it.
The items below (20.21–20.28) are different in character. They came out of a
final cross-check of the whole implementation against sauce4zwift and the
spec, and several are **not optional polish — they block functional parity**.

The shape of the finding is consistent: the supporting libraries
(`zwift-stats` primitives, the `src/web/format.rs` formatters, the codec, the
v2 query-reduction engine) are faithful to sauce4zwift and well-tested in
isolation, which is why STEP 18 and STEP 19 passed. What is missing is the
production *wiring* that drives those libraries end-to-end: the per-tick
recording pipeline, the 1 Hz nearby/groups processor, the RPC handler
registrations, the live event-stream producers, the UDP inbound consumption,
the WorldUpdate decoders, and the profile/event/segment REST fetchers. Each
was confirmed by reading the production code and by a workspace-wide search
showing the relevant library functions are reached only from tests.

These should be triaged into real implementation steps, not left to accrete.
The priority ordering across all eight is given at the end of 20.28.

### 20.21 — Production per-tick stats recording is not wired into `route_player_state`

**Where it came from.** Final review. `zwift-stats` ports sauce4zwift's
`_recordAthleteStats` / `_preprocessState` building blocks faithfully and they
are unit-tested, but the production ingest path — `route_player_state` in
`src/web/proto_to_stats.rs` — invokes only a fraction of them. Reading the
function confirms it calls `registry.upsert`, the five `ingest_*` methods, an
in-place `smooth_grade.update` whose result is discarded, and
`apply_event_state`, and nothing else. A workspace search confirms
`most_recent_state = …`, `record_streams`, `road_history.record`,
`active_segment_check`, `auto_lap_check`, `compute_groups`, `apply_gap`,
`clone_reset`, `resize`, and `WBalAccumulator`/`ZonesAccumulator::configure`
are reached only from tests.

**Current resolution.** The library exists; the wiring does not.
Consequences in published payloads:

| Missing wiring | sauce4zwift source | Published-field impact |
|---|---|---|
| `ad.most_recent_state = state` | `_recordAthleteStats` `stats.mjs:3493` | `state` object always `null` in v1/v2 athlete records; deprives gap/group/segment of current position |
| streams recording (`record_streams`) | `_recordAthleteStats` | `streams/*` (distance/altitude/latlng/wbal) empty |
| road-history recording (`road_history.record`) | `_recordAthleteRoadHistory` `stats.mjs:3103` | gap and segment-completion have no road data |
| work/follow/solo/coffee time + kJ split | `_recordAthleteStats` `stats.mjs:3397-3463` | `workTime`/`followTime`/`soloTime`/`coffeeTime`/`workKj`/`followKj`/`soloKj` always 0 |
| W' and zones `configure` + accumulate | `_updateAthleteDataFromDatabase` `stats.mjs:2863` | `wBal` always `null`; `timeInPowerZones` always empty (also needs an FTP/CP source — see 20.26) |
| slice growth (a `resize`-equivalent) | `stats.mjs:3471-3491` | `lap`/`lastLap`/`laps`/`segments`/`events` bucket stats always empty; `lapCount` works |
| auto-lap detection (`auto_lap_check`) | `_autoLapCheck` `stats.mjs:3092` | no automatic laps |
| active-segment detection (`active_segment_check`) | `_activeSegmentCheck` `stats.mjs:3077` | `segments[]` always empty |
| grade publication | `_preprocessState` | `state.grade` never published (computed then discarded) |
| stale/duplicate-state guard | `_preprocessState` `stats.mjs:3146` (rejects `elapsed<0`/`==0`) | out-of-order/duplicate packets are ingested unconditionally; risk to rolling-window sums |

Two structural notes. `DataSlice::new_from` calls `clone_reset()`, producing
an empty bucket, and `DataCollector` has no `resize` method — so even when a
slice is created it cannot grow. And (verify) `ProtoView` road_time does not
apply sauce's reverse adjustment (`reverse ? 1005000 - roadTime : roadTime -
5000`, `zwift.mjs:321`), so road positions would be wrong for reverse riders
once road history is recorded.

**Why this might come back.** Every overlay widget that reads `state.*`,
per-lap/segment/event numbers, W'bal, zone time, or work/draft kJ shows blank
or zero today. This is the largest single block of missing parity.

**Decision rule.** Not optional for parity. Frame as a dedicated step that
ports `_recordAthleteStats` + `_preprocessState` into `route_player_state` (or
a sibling), drawing the W'/zone configuration from the profile source in
20.26. Until then `zwift-stats` is exercised only by its own tests.

### 20.22 — 1 Hz states-processor: nearby, groups, gap, group identity, event rank

**Where it came from.** Final review. sauce4zwift runs a 1000 ms
`_statesProcessor` loop (`stats.mjs:4182`) that calls `_computeNearby`
(`stats.mjs:4427`) then `_computeGroups` (`stats.mjs:4542`), sets each
athlete's `gap`/`gapDistance`/`isGapEst`/`groupId`, and emits the `nearby` and
`groups` events (v1 and v2). ranchero has no equivalent loop — the only
periodic web-layer task is `gc_tick_loop` in `src/web/state.rs:95`.
`compute_groups` (`crates/zwift-stats/src/groups.rs`) and `apply_gap`
(`crates/zwift-stats/src/gap.rs`) are never called outside tests.

**Current resolution.** Three distinct gaps:

1. **No periodic processor.** `gap`/`gapDistance`/`isGapEst` are always
   `None`; `group_id` is always `None`. The HTTP `/nearby/*` and `/groups/*`
   routes compute on-demand from the registry, so they return *something*, but
   nearby is unsorted (HashMap order, `src/web/http/mod.rs:247`) with no gap
   filtering, and groups group by `group_id` which is always `None`, so groups
   always come back empty (`src/web/http/mod.rs:297`).
2. **No `nearby`/`groups` event source, plus a latent wrong-data bug.** Over
   WebSocket there is no producer for `nearby`/`groups`/`nearby/v2`/`groups/v2`.
   Worse, `event_matches_athlete` (`src/web/subs/mod.rs`) returns `true` for
   these non-athlete event names, so a client that subscribes to `nearby`
   currently receives a stream of single-athlete v1 payloads (one per inbound
   `PlayerState`) instead of the expected sorted array. `emit_v2` formats one
   athlete, not an array.
3. **Incremental gap estimation not ported.** `_computeNearby` splits riders
   into ahead/behind, sorts by `gapDistance`, and walks adjacent riders to
   infer each missing gap (`refSpeedForEst`, `incRP` chaining). ranchero's
   `apply_gap` implements only the simple per-athlete case (direct road
   comparison, else a single speed-EMA fallback).

Related: `ServerToClient.eventPositions` / `EventSubgroupPlacements`
(`stats.mjs:2530-2551`; proto field `ev_subgroup_ps = 23`) is never processed,
so `eventPosition`/`eventParticipants` are always absent even though the
formatters read them.

**Why this might come back.** `nearby` and `groups` drive the most-used
overlay widgets; both are blank/empty today, and the WS `nearby` subscription
delivers wrong-shaped data.

**Decision rule.** Not optional for parity. Add a 1 Hz tick task (sibling to
`gc_tick_loop`) that runs nearby + groups, plus `nearby`/`groups` event
sources in `src/web/subs/`; fix `event_matches_athlete` so the array streams
are not mis-delivered as single athletes. Depends on 20.21 (needs
`most_recent_state` and road history).

### 20.23 — RPC handler surface: only `getVersion` is registered

**Where it came from.** Final review. The spec (§6.1, line 125) notes
sauce4zwift registers "~50 RPC handlers". ranchero's `RpcRegistry::new`
(`src/web/rpc.rs:17`) registers exactly one in-scope handler, `getVersion`.
The RPC plumbing itself — HTTP `/api/rpc/v1` and `/v2`, the WebSocket `rpc`
method, dispatch, argument coercion, base64url decoding — is present and
correct; there is simply nothing registered behind it.

**Current resolution.** Roughly 50 core in-scope handlers are missing (≈75
once borderline ones are included). Any widget calling an RPC gets
`unknown rpc handler`. Grouped by area (excluding window/mod/hotkey/updater/
Electron-shell/companion handlers, which are out of scope per spec §6):

- **Athlete data / control:** `getAthlete`, `getAthletes`, `updateAthlete`,
  `getAthleteData`, `getAthletesData`, `updateAthleteData`, `getAthleteLaps`,
  `getAthleteSegments`, `getAthleteEvents`, `getAthleteStreams`,
  `getPlayerState`, `startLap`, `resetStats`, `getPowerZones`,
  `getPowerProfile`.
- **Nearby / groups (RPC twins of the routes):** `getNearbyData`,
  `getGroupsData`.
- **Social / following:** `getFollowingAthletes`, `getFollowerAthletes`,
  `getMarkedAthletes`, `searchAthletes`, `setFollowing`, `setNotFollowing`,
  `giveRideon`, `toggleMarkedAthlete`, `removeFollower` (the write actions are
  borderline — they write to Zwift, beyond the read-only live-data core).
- **Events:** `getCachedEvent(s)`, `getEvent`, `getEventSubgroup`,
  `getEventSubgroupEntrants`, `getEventSubgroupResults`, `addEventSubgroupSignup`,
  `deleteEventSignup`, `loadOlderEvents`, `loadNewerEvents` (signup actions
  borderline). Ties to 20.19 item 4 and 20.26.
- **Segments / chat / game state:** `getSegmentResults` (ties to 20.26),
  `getChatHistory` (ties to 20.24), `getGameState` (ties to 20.20 item 2).
- **World/route/segment geometry (`Env`):** `getWorldMetas`, `getCourseId`,
  `getRoad`, `getCourseRoads`, `getRoute`, `getCourseRoutes`, `getSegment`,
  `getCourseSegments`, `getRoadSegments` (ties to 20.27 route/world-meta
  tables).
- **App / settings / connection:** `getSetting`, `setSetting` (emits
  `setting-change` on the `app` source — ties to 20.24), `getDebugInfo`,
  `getWebServerURL`, `getZwiftLoginInfo`, `getZwiftConnectionInfo`,
  `reconnectZwift`, `zwiftLogout`, `resetStorageState`, `resetAthletesDB`.
- **Borderline / lower value:** workout handlers (`getWorkouts`, `getWorkout`,
  `getWorkoutCollection(s)`, `getWorkoutSchedule`), file-replay handlers
  (`fileReplayLoad`/`Play`/`Stop`/… — ranchero has a CLI replay path instead),
  `getIRLMapTile`, `putState`, `getQueue`, deprecated `getAthleteStats`/
  `updateAthleteStats`, `exportFIT` (FIT is spec-deferred, see 20.17 item 4).

**Why this might come back.** Browser widgets mix WebSocket subscriptions with
RPC calls; the RPC half is almost entirely unavailable.

**Decision rule.** Not optional for parity, but stage it: implement the
read-only athlete/nearby/groups/event/segment/geometry getters first (these
are what widgets call most), then settings, then the write actions
(`setFollowing`, `giveRideon`, …) once a decision is made on whether ranchero
performs write-back to Zwift at all. Many getters depend on 20.21/20.22 (data
to return), 20.26 (REST fetchers), and 20.27 (geometry tables).

### 20.24 — Live event-stream producers: chat, rideon, game-state, watching-athlete-change

**Where it came from.** Final review. sauce4zwift's `stats` emitter produces
`rideon` (`stats.mjs:2591`), `chat` (`stats.mjs:2650`), `game-state`
(`stats.mjs:1250`), and `watching-athlete-change` (`stats.mjs:2659`) in
addition to the per-athlete streams. ranchero produces only the per-athlete
streams (`athlete/{id}`, `athlete/watching`, `athlete/self`, and their v2
forms, via `bridge_player_state_event` → `GameEvent::PlayerState`).

**Current resolution.**

- **`chat` / `rideon`.** Inbound `WorldUpdate`s are iterated in the recv loop
  (`src/daemon/relay.rs:3354-3372`) only to advance `last_world_update_ts`;
  the payloads (RideOn, SocialAction/chat) are never decoded. There is no
  `GameEvent` variant for them (the enum has only `PlayerState`, `Latency`,
  `StateChange`, `PoolSwap`) and no subs handling. This shares its root cause
  with the relay-side WorldUpdate decoding gap in 20.25.
- **`game-state` / `watching-athlete-change`.** No producer. `watching_id` is
  set once at boot (`src/daemon/runtime.rs:305`) and never changes, so no
  watched-athlete-change event ever fires; there is no game-state object. This
  is adjacent to 20.20 item 2 (the `gameState` *formatter field* and the
  stubbed `gameConnection` subscription source) but distinct: those are the
  field and the source registration, not these two emitter streams.
- **Subscription sources.** `create_delegation` (`src/web/subs/mod.rs`)
  recognises only `source == "stats"` (real) and `source == "gameConnection"`
  (parks forever). A widget subscribing to the `app` source for
  `setting-change` (sauce `app.mjs:142`) gets `unknown source`; this depends
  on `setSetting` from 20.23.

**Why this might come back.** Chat-overlay, ride-on notification, and
game-state widgets receive nothing; widgets that re-render on a
watched-athlete switch never update.

**Decision rule.** Implement the WorldUpdate decoders and new `GameEvent`
variants alongside 20.25 (same relay-side decode), then add the corresponding
subs producers. `game-state`/`watching-athlete-change` come with the
watched-athlete-following work in 20.25 and the game-state producer in 20.20
item 2.

### 20.25 — Relay live-data path completeness

**Where it came from.** Final review of `src/daemon/relay.rs` against
`zwift.mjs`. Distinct from the relay items already parked (20.11–20.16): these
concern whether the live telemetry path actually functions in production.

**Current resolution.**

1. **Inbound UDP `ServerToClient` is decoded but discarded.** sauce processes
   UDP inbound identically to TCP (`zwift.mjs:1860` `inPacket` handler); the
   per-rider live stream arrives primarily over UDP at 10+ Hz (spec
   §4.10/§4.11). In ranchero the UDP channel exists only as a heartbeat sink:
   the recv-loop UDP arm is a no-op (`relay.rs:3448`,
   `ChannelEvent::Inbound(_stc)`), and the only sender into the UDP event
   channel in production is the test-only `inject_udp_event`. All telemetry
   reaching the web bridge comes from the TCP inbound branch plus the 3 s
   state-refresher poll. **High impact.**
2. **TCP reconnect does not re-establish UDP.** `connection_manager`
   (`relay.rs:2851`) reconnects TCP and re-sends the hello but explicitly
   discards `watched_id`/`game_events_tx` (`relay.rs:3056-3061`) and never
   opens a new UDP channel or heartbeat; `resume_udp` is single-shot. After
   any TCP drop the daemon runs on TCP only for the rest of the session. sauce
   rebuilds UDP on every reconnect (`_schedConnectRetry`, `zwift.mjs:1869`).
3. **Watched-athlete position is never updated from the stream.** sauce's
   `_updateWatchingState` (`zwift.mjs:2260`) feeds the rider's live
   `(x, y, courseId, portal)` into `findBestUDPServer` on every state.
   ranchero's `observe_watched_player_state` and `switch_watched_athlete`
   (`relay.rs:2512`, `:2530`) are `#[cfg(test)]`; the initial
   `WatchedAthleteState` seeds position `(0,0)` / course 0 even though startup
   already polled the real world. So `recompute_udp_selection` always
   evaluates against `(0,0)`. `find_best_udp_server` is real code fed stale
   zeros — the "UDP server follows the rider" mechanism is inert. This is the
   upstream cause that makes 20.13/20.14 moot until fixed.
4. **WorldUpdate payloads are never decoded or dispatched.** sauce decodes
   every `WorldUpdate` (`zwift.mjs:2164-2187`): payloadType < 100 by nested
   protobuf name (RideOn, SocialAction, PlayerLeftWorld, PlayerRegisteredFor­
   Event, NotableMoment, …), ≥ 100 via `binaryWorldUpdateDecoders`
   (SegmentResult = 105, etc.). ranchero reads only the timestamp. No decoders,
   no `GameEvent` variants. Source for the `chat`/`rideon` streams (20.24) and
   live `SegmentResult` (spec §4.12, §8).
5. **Heartbeat omits portal/roadId/eventSubgroup.** `broadcastPlayerState`
   (`zwift.mjs:1942-1957`) forwards the watched athlete's `portal`, `_flags2`
   (roadId), and `eventSubgroupId`. `HeartbeatScheduler::next_state`
   (`relay.rs:797`) sends only id/just-watching/watching-id/world/world_time,
   with `course_id` fixed at construction. Distinct from 20.11 (that is
   receive-side portal-pool selection; this is send-side content).
6. **No `multipleLogins` detection.** sauce warns when `pb.multipleLogins` is
   set (the monitor account logged in elsewhere, `zwift.mjs:2144`). No
   reference anywhere in ranchero. Diagnostic only, but it is the signal that
   another client has displaced this session.
7. **State-refresher only polls the watched athlete.** sauce also polls self
   when self ≠ watching (`_refreshStates`, `zwift.mjs:1998`), and suppresses
   logging on HTTP 429. ranchero issues one `get_player_state(watched_id)` and
   treats all errors alike. Low impact under the single-athlete model; matters
   once item 3 lands.

Lower-confidence observations to weigh, not yet assert: `find_best_udp_server`
falls through to nearest-Euclidean when `use_first_in_bounds` matches nothing,
where sauce returns "no swap"; and sauce drops player states for
`activePowerUp === 'NINJA'` (`zwift.mjs:2194`), which ranchero does not — a
deliberate decision is warranted on the NINJA privacy drop.

**Why this might come back.** Items 1–4 mean the UDP path is effectively
non-functional for live telemetry, server-following, post-reconnect recovery,
and world events. The daemon's reason for existing is the live stream.

**Decision rule.** Items 1, 2, 4 are not optional for parity — pull into a
relay-completion step. Item 3 is the prerequisite for 20.13/20.14 and should
land with them. Items 5–7 are smaller and can ride along.

### 20.26 — REST fetchers for live data

**Where it came from.** Final review of `crates/zwift-api/src/lib.rs` against
sauce4zwift's `ZwiftAPI`. ranchero implements OAuth (login/refresh, with 50%
preemptive refresh and 401 inline retry — verified at parity), `get_profile_me`,
`get_player_state`, `logout`, `leave`. The live-data fetchers below are absent.
These are the *producers* that several already-parked caches assume exist.

**Current resolution.**

| Missing method | sauce4zwift | Consumer / why it matters |
|---|---|---|
| `getProfiles` (batch, protobuf `/api/profiles`) | `zwift.mjs:559`, driven on every state via `_maybeUpdateAthleteFromServer` `stats.mjs:3080` | The producer beneath 20.20 item 1 (read cache → `athlete` field, name/FTP), 20.17 item 1 (write to `athletes.sqlite`), and the W'/zone configure in 20.21. Without it `athlete:null` and `tss:null` permanently. **Highest impact.** |
| `getEvent` (protobuf `/api/events/{id}`) | `zwift.mjs:808`, via `getEventSubgroup` `stats.mjs:1332` | The producer beneath 20.19 item 4 (event-subgroup cache); without it `eventLeader`/`eventSweeper`/`remaining*` stay absent and event detection (20.27) has no metadata. |
| `getSegmentResults` (`/api/segment-results`) + `getLiveSegmentLeaders` + `getLiveSegmentLeaderboard` | `zwift.mjs:633-645` | The only writers `segments.sqlite` was built for. 20.17 item 2 names only the evictor. Note sauce caches leaderboards in memory (2 s TTL), so ranchero's `segments.sqlite` is a ranchero-original design, not a sauce parity requirement. |
| `getProfile` (single, `/api/profiles/{id}`) | `zwift.mjs:541` | Backs the on-demand `getAthlete` RPC (20.23). |
| `getActivities` (`/api/profiles/{id}/activities`) | `zwift.mjs:599` | Backs activity-list RPCs. Lower priority. |
| `getGameInfo` (`/api/game_info`) | `zwift.mjs:681` | World/segment metadata sync; relates to the world-meta vendoring in 20.27 / 20.19 item 2. |

**Why this might come back.** `getProfiles` and `getEvent` are blocking
dependencies for whole clusters of already-parked work (20.17/20.19/20.20/20.21).
Those items quietly assume a fetcher that does not exist.

**Decision rule.** Implement `getProfiles` first — it unblocks the athlete
profile cache, FTP/TSS, and W'/zone configuration in one move. `getEvent` next
(events). Segment-leaderboard fetchers only if segment leaderboards are kept in
scope; otherwise reconsider whether `segments.sqlite` should exist at all (see
20.28). `getProfile`/`getActivities`/`getGameInfo` are on-demand and lower
priority.

### 20.27 — Proto fields and static-table vendoring

**Where it came from.** Final review. Several computations are structurally
dead because the data they read was never vendored.

**Current resolution.**

1. **`eventSubgroupId` / `eventDistance` proto fields missing.**
   `apply_event_state` *is* called in production (`proto_to_stats.rs:104`), but
   `ProtoView::event_subgroup_id()` is hardcoded to `0` and `event_distance()`
   to `0.0` because the vendored `udp-node-msgs.proto` `PlayerState` does not
   expose them under those names — see QB1: wire tags 29 and 34 exist but are
   labelled `groupId` / `dist_lat`, where sauce's fork reads them as
   `eventSubgroupId` / `_eventDistance`. So `apply_event_state` always sees
   `0` and returns `Idle`; events are never detected from telemetry, and
   event end-by-distance / event privacy flags never fire. Distinct from
   20.19 item 4, which covers the event-subgroup metadata *cache*, not the
   missing *proto fields* that feed it. Needs a decision on extending the
   vendored proto2 schema.
2. **`EventSubgroupPlacements` not processed.** Proto field `ev_subgroup_ps =
   23` exists but is unused; `eventPosition`/`eventParticipants` never written
   (see also 20.22).
3. **Route tables / `zwift-routes` crate absent.** Spec §7.2 lists
   `zwift-routes` ("on demand") and §7.8 makes segment/route detection depend
   on `shared/routes.mjs` + `shared/curves.mjs`. The crate does not exist in
   `crates/`. Without it, `_computeRouteDistance` (`stats.mjs:3197`) and the
   route branch of `_getEventOrRouteInfo` (`stats.mjs:4293`) cannot be ported:
   `routeDistance`, route %, and `remaining`/`remainingMetric`/`remainingType`/
   `remainingEnd` for routes stay absent (the formatters hardcode them `None`
   with a "requires route/event metadata" comment). The event half of
   `_getEventOrRouteInfo` is referenced in 20.19 item 4; the route half is new
   here.
4. **World-meta tables.** Needed for altitude adjustment, lat/lng projection,
   and `state.x`/`y`/`roadCompletion`/`progress` — already parked in 20.19
   item 2; cross-referenced here because `getGameInfo` (20.26) and
   `getWorldMetas` (20.23) are the same data family.

**Why this might come back.** Items 1–2 block all event detection; item 3
blocks route progress and is a prerequisite for faithful segment detection.

**Decision rule.** Item 1 (proto fields) is cheap once the schema decision is
made and unblocks the whole event chain — do it early. Item 3 (route tables)
is a data-vendoring step on the scale of the spec's `zwift-routes` crate;
schedule it deliberately. Item 2 rides along with 20.22.

### 20.28 — Persistence schema and live usage

**Where it came from.** Final review of `crates/zwift-store` and
`src/daemon/stores.rs` against sauce's `db.mjs`/`storage.mjs` and the DB
definitions in `stats.mjs`. Beyond the deferrals already in 20.17:

**Current resolution.**

1. **`athletes` table schema cannot hold the full athlete object.** sauce
   stores each athlete as a JSON blob (`athletes(id INTEGER PK, data TEXT)`)
   and queries it with `json_each(data, '$.marked')` to load marked athletes
   (`stats.mjs:2440-2447`). ranchero uses fixed columns (`fname, lname, ftp,
   weight, badges, last_seen`), which cannot represent `marked`, `following`,
   `gender`, `type`, `avatar`, privacy flags, power-source, etc. The
   marked-athletes user feature has no column at all. 20.17 item 1 assumes the
   existing schema is adequate; for sauce parity it is not.
2. **`event_subgroups.sqlite` is missing entirely.** sauce persists
   subgroup→event mappings (`stats.mjs:3582-3597`) so event context survives
   restarts. ranchero has no such DB; `WebState.event_subgroups` is in-memory
   only (20.19 item 4 covers populating that in-memory cache, not persisting
   it). A fourth sauce DB with no ranchero counterpart.
3. **The three store DBs are opened but never read or written in production.**
   `Stores::open` runs at daemon start, but `run_daemon` binds the result as
   `_stores` and nothing in the runtime/relay/web layers calls
   `upsert`/`touch`/`put`/`get`/`evict_expired`. In practice the SQLite layer
   is exercised only by its own crate tests. This is the broad version of the
   specific writers noted in 20.17 items 1–2.

**Why this might come back.** Item 1 blocks marked/followed-athlete features;
item 3 means restarts lose all cached state (settings, athlete profiles).

**Decision rule.** Decide item 1's schema before wiring 20.17 item 1's writer
(a JSON-blob `data` column matches sauce and avoids re-migration). Item 2 only
if event persistence across restarts is wanted. Item 3 resolves naturally as
20.17/20.20/20.26 wire real readers and writers; until then, note in
`ranchero status` (or a comment) that persistence is structurally present but
inert.

---

### Priority ordering across 20.21–20.28

A suggested order, by how much each unblocks:

1. **20.26 `getProfiles`** — unblocks athlete profile cache, FTP/TSS, and
   W'/zone configuration in one move.
2. **20.21 per-tick recording pipeline** — turns most published fields from
   blank/zero into real values (depends on 1).
3. **20.25 items 1, 2, 4 (UDP inbound, reconnect UDP, WorldUpdate decode)** —
   makes the live telemetry path actually function.
4. **20.22 1 Hz nearby/groups processor** — the most-used overlay widgets
   (depends on 2).
5. **20.27 item 1 (event proto fields)** + **20.26 `getEvent`** — event
   detection chain.
6. **20.23 RPC handlers** (read-only getters first) — the RPC half of the
   widget API (depends on 2/4 for data).
7. **20.24 chat/rideon/game-state streams** — alongside 20.25 item 4.
8. **20.27 item 3 (route tables)** and **20.28 (persistence schema)** —
   larger, more independent pieces; schedule deliberately.

The honest summary: items 1–4 above are the difference between a daemon that
serves a faithful-but-mostly-empty payload shape (today) and one that serves
live data comparable to sauce4zwift. STEP 18/19 verified the shapes and the
isolated math; they did not — and were not designed to — verify the
end-to-end production data path, which is what 20.21–20.28 record.

---

## Open questions raised by the implementation review (2026-05-24)

Items 20.21–20.28 cannot be fully implemented until several decisions are made
that reading the code cannot settle on its own. They are scope choices, parity
deviations, and data-source selections that need an explicit answer recorded
here before the work is turned into implementation steps. The questions are
grouped by theme; the highest-leverage answers are in groups A and B, because
almost every item in the priority ordering above depends on them. Each question
notes what it controls and which 20.N items it touches.

### A. Scope forks — answer these first; they decide how much of the rest exists

**QA1 — Does ranchero write back to Zwift at all?** The project is described as
a read-only live-data core, but 20.23 lists roughly ten write actions
(`setFollowing`, `setNotFollowing`, `giveRideon`, `toggleMarkedAthlete`,
`updateAthlete`, `removeFollower`, event sign-ups) and 20.26 implies write
fetchers. If write-back is out of scope, these should be removed from the
missing-handler list rather than deferred, so they stop reading as gaps.
*Controls:* the write half of 20.23 and any write methods in 20.26.
→ **Answer (2026-05-24): No — ranchero does not write back to Zwift.** The write
actions above are out of scope; remove them from the 20.23 missing-handler
inventory rather than listing them as deferred, and drop any write fetchers from
20.26. Only read-only getters and the data they need remain.

**QA2 — Are segment leaderboards in scope?** This decides 20.26
(`getSegmentResults` / `getLiveSegmentLeaders` / `getLiveSegmentLeaderboard`),
20.17 item 2 (the evictor), and whether `segments.sqlite` should exist at all
(20.28). sauce keeps leaderboards in a 2-second in-memory cache, so
`segments.sqlite` is a ranchero-original design with no parity justification. If
leaderboards are out of scope, should `segments.sqlite` be removed rather than
wired? *Controls:* the 20.26 segment row, 20.17 item 2, the 20.28 segment store.
→ **Answer (2026-05-24): Keep them.** Segment leaderboards stay in scope, so the
segment fetchers (20.26), the 20.17 item 2 evictor, and `segments.sqlite` (20.28)
are all retained and wired.

**QA3 — How deep does event support go for v1?** *(question expanded 2026-05-24
after it was unclear.)* In Zwift an "event" is a structured group activity a
rider signs up for — a race, group ride, or workout — organised into sub-groups
(the A/B/C/D categories). When the watched athlete is in an event, sauce4zwift
surfaces event-relative context in the published payload: the sub-group the rider
is in, the event leader and sweeper riders, distance and time remaining in the
event, the rider's position and the participant count within the event, and
event-relative ranking. Producing any of that needs the full chain below, which
is why this single answer governs so much downstream work:

  telemetry must carry the event-subgroup id (QB1) → the daemon fetches the event
  metadata (`getEvent`, 20.26) → caches the subgroup→event mapping (20.19 item 4,
  optionally persisted as 20.28 item 2) → the formatters spread the resulting
  fields (`eventLeader`, `eventSweeper`, `remaining*`, `eventPosition`,
  `eventParticipants`).

The decision is whether that whole chain is a v1 goal. *Controls:* 20.19 item 4,
the event handlers in 20.23, and parts of 20.26 / 20.27 / 20.28.
→ **Answer (2026-05-24): Required — event support is a v1 goal, not optional.**
Viewing data during an event is the main use of sauce4zwift, so the daemon must
build the full chain above (QB1 proto fields → `getEvent` → subgroup cache →
formatter spread). **Correction:** the earlier note that omitting these fields is
"parity-correct because sauce omits them too" is misleading — sauce omits them
only in the brief window before its own caches populate, then shows them; it does
not run with event data permanently absent. The same caveat applies to the
"parity-correct to omit" phrasing in 20.19 item 4 and 20.20: it describes a
transient startup state, never an acceptable shipped state.

**QA4 — Is route progress (the `zwift-routes` crate) a v1 goal?** 20.27 item 3
requires a whole crate (route and curve tables, spec §7.2 / §7.8) before
`routeDistance`, route percentage, and route `remaining*` can be computed; the
spec lists the crate as "on demand". Do we schedule that data-vendoring work
now, or leave the route fields permanently absent? *Controls:* 20.27 item 3 and
the route half of `_getEventOrRouteInfo`.
→ **Answer (2026-05-24): Yes.** The `zwift-routes` crate is a v1 goal; schedule
20.27 item 3 as a deliberate data-vendoring step, which unblocks `routeDistance`,
route percentage, and the route `remaining*` fields.

### B. Proto schema and vendoring

**QB1 — How do we obtain `eventSubgroupId` / `eventDistance` on `PlayerState`?**
(20.27 item 1, the cheap unblock for the whole event chain.)
→ **Research finding (2026-05-24) — the premise was wrong; the fields are not
missing, they are differently labelled.** The two reverse-engineered schemas
disagree about the *same* wire tags:

| Wire tag | zwift-offline (vendored) | sauce4zwift (`zwift.proto`) |
|---|---|---|
| 29 | `int64 groupId` (`udp-node-msgs.proto:151`) | `int32 eventSubgroupId` (`zwift.proto:33`) |
| 34 | `float dist_lat` (`udp-node-msgs.proto:156`) | `float _eventDistance` // cm (`zwift.proto:38`) |

Both tags already exist in the vendored proto. Tag 29 is a varint in both
readings; tag 34 is a float in both. So this is not an "add a field" change — it
is a decision about *which interpretation of tags 29 and 34 is correct*, and that
needs a real capture to settle (does tag 29 carry a group id or a sub-group id;
does tag 34 carry lateral distance or event distance in cm). There is precedent
for preferring sauce's reading where it actively uses a field: the vendored proto
already overrides zwift-offline on `draft = 10` (`udp-node-msgs.proto:122`, "the
zoffline annotation … is rejected"). The remaining work is to verify against a
capture, then rename/reinterpret the vendored fields (with a comment recording
the deviation) rather than adding new ones. **This also corrects the wording in
20.27 item 1**, which says the PlayerState "has no such field"; it has the tags
under other names. *Controls:* all event detection from telemetry; still needs
the capture check.

### C. Data sources and identity

**QC1 — What is "self" under the two-account model?** (20.19 item 3; confirmed
`None` today at `src/daemon/runtime.rs:305-306`.) Is the self athlete id the main
account's id from `get_profile_me`, while the monitor account only receives the
stream? And when no watched athlete is configured, should `watching_id` default
to self? The `self` endpoint aliases and the `apply_event_state` self-comparison
are degraded until this identity model is written down. *Controls:* 20.19 item 3,
part of 20.25 item 7.
→ **Answer (corrected 2026-05-24): "self" is the watched athlete, sourced from
`cfg.watched_athlete_id`; the monitor/watcher account is never self.** Confirmed
against the code: the daemon logs into the relay with the **monitor** account
(`src/daemon/relay.rs:1595-1608`), so `auth.athlete_id()` is the monitor's id — a
pure relay conduit, *not a rider*. The rider whose overlay is served — "self" —
is the watched athlete, which the runtime already holds as `cfg.watched_athlete_id`
and which is mandatory (`relay.rs:1637-1639`); the code comment at
`relay.rs:1631-1633` draws exactly this distinction. So `self_athlete_id` should
simply be set to `cfg.watched_athlete_id` (the same value as `watching_id` at
`runtime.rs:305`), which resolves `TODO 17.36-I` directly — no relay subscription
is required. The watcher id equals self only in the degenerate, discouraged setup
where the monitor account is itself the watched athlete. (My two earlier answers
were wrong: the main account is not the relay-session identity, `get_profile_me`
is not the source of self, and there is no "default watching to self" — the
watched athlete is always explicitly configured.)

**QC2 — Where does the athlete profile cache read from: live API, SQLite, or
read-through both?** (20.20 item 1 / 20.26 / 20.17 item 1.) `getProfiles` is the
producer; `athletes.sqlite` is the store. Do the formatters read a `WebState`
cache filled from `getProfiles`, backed by SQLite, or only one of those? This
also decides whether the write-side persistence (20.17 item 1) is implemented in
the same step. *Controls:* 20.20 item 1, 20.17 item 1.
→ **Answer (2026-05-24): read-through, live stream authoritative.** The Zwift
data stream is always the authoritative, live, current source; `athletes.sqlite`
is the cache. So the formatters read live profile data (refreshed from
`getProfiles`), the daemon writes it through to `athletes.sqlite`, and the cache
is consulted only as a fallback before live data is available. This pairs the
20.17 item 1 writer with the 20.20 item 1 read cache.

**QC3 — What feeds W′-balance and power-zone configuration: FTP only, or CP and
W′?** (20.21, dependent on 20.26.) `WBalAccumulator` /
`ZonesAccumulator::configure` need the numbers sauce draws in
`_updateAthleteDataFromDatabase`. *Controls:* the W′ and zone portions of 20.21.
→ **Research finding (2026-05-24): the Zwift profile carries FTP only.** From
sauce's `_updateAthleteDataFromDatabase` (`stats.mjs:2864-2871`):
`cp = athlete.cp || athlete.ftp`, `wPrime = athlete.wPrime || 20000`
(`wPrimeDefault`, `stats.mjs:15`), `ftp = athlete.ftp`; then
`wBal.configure(cp, wPrime)` and `timeInPowerZones.configure(ftp,
getPowerZones(ftp))`. The Zwift live profile only provides FTP
(`functional_threshold_power`, `stats.mjs:527`); `cp` and `wPrime` are sauce
athlete-database values (user-entered), not part of the Zwift profile. So for
ranchero: configure W′ with `cp = profile.ftp` and `w_prime = 20000.0` until a
user-supplied CP/W′ override exists, and configure zones with `ftp =
profile.ftp` plus a port of `getPowerZones(ftp)`. **Blocker found:** ranchero's
`Profile` struct (`crates/zwift-api/src/lib.rs:113`) currently holds only `id` —
it must be extended to parse FTP before either QC2 or QC3 can be wired.

### D. Persistence schema

**QD1 — Migrate the `athletes` table from fixed columns to a JSON-blob
`data TEXT` column?** (20.28 item 1.) The current fixed columns
(`fname, lname, ftp, weight, badges, last_seen`) cannot hold `marked`,
`following`, `gender`, privacy flags, and the like, and sauce queries
`json_each(data, '$.marked')`. The recommendation is the JSON-blob schema to
match sauce and avoid a second migration; decide before the 20.17 item 1 writer
is built. *Controls:* 20.28 item 1, 20.17 item 1, the marked/followed-athlete
features.
→ **Answer (2026-05-24): Agreed.** Adopt the JSON-blob `data TEXT` schema and
build the 20.17 item 1 writer against it.

**QD2 — Add a fourth database (`event_subgroups.sqlite`) for event persistence
across restarts?** (20.28 item 2.)
→ **Your question back: "what events make sense after a restart to justify
this?"** Answer: `event_subgroups.sqlite` would persist subgroup→event mappings,
so that after a daemon restart a player-state that references an `eventSubgroupId`
can be resolved to its event metadata (leaders, distance, route) without
re-fetching. The only scenario it actually helps is a restart *while a rider is
mid-event*: with the cache, event context is available immediately; without it,
the daemon re-fetches the mapping from the Zwift API on the next state. For a
single-user live daemon a mid-event restart is rare, and the in-memory cache
(20.19 item 4) can simply be repopulated from the API on demand.
**Recommendation: do not add the fourth DB for v1** — keep event subgroups in
memory and repopulate after restart; revisit only if mid-event restart
resilience is explicitly wanted. *Controls:* 20.28 item 2.
→ **Answer (2026-05-24): Confirmed — do not create this DB.** A cache restored
from disk after a restart would be unsafe to trust: during the downtime the event
data may have changed, so it must be re-read from the Zwift API regardless. The
persisted copy would never be used as-is, which makes the DB pointless. Keep
event subgroups in memory only and always re-read after restart.

### E. Output shape and parity deviations

**QE1 — `latlng: [lat, lng]` array (full parity) or separate `lat` / `lng`
scalars (documented extension)?** (20.19 item 2.) *(value of each, added
2026-05-24, since the trade-off was not stated.)* sauce4zwift emits the rider's
position as a single two-element array, `latlng: [lat, lng]`. ranchero currently
computes the two numbers separately and would emit `lat` and `lng` as separate
scalar fields.
  - **Value of matching the array:** unmodified sauce4zwift overlay widgets read
    `state.latlng` as a two-element array, so emitting the array means those
    widgets work without changes — which is the entire point of the field-for-
    field parity in STEP 18/19. Cost: ranchero must repack its two scalars into
    an array in `format_state`.
  - **Value of the separate scalars:** marginally simpler internally and
    arguably cleaner JSON, but any widget expecting `latlng` breaks; it is a
    documented API deviation that only makes sense if ranchero is *not* trying to
    serve sauce's existing widgets.
  - **Recommendation:** emit the `latlng` array for parity, unless serving
    unmodified sauce widgets is explicitly a non-goal. Moot until the world-meta
    tables are vendored (20.19 item 2). *Controls:* the `_formatState` deviation
    in 20.19 item 2.
→ **Answer (2026-05-24): Separate scalars.** ranchero will implement its own
widgets; once those are proven working, the implementation is expected to diverge
from sauce4zwift deliberately, and this is the first such divergence. Emit `lat`
and `lng` as separate scalar fields and document the deviation. Serving
unmodified sauce widgets is therefore *not* a binding constraint going forward —
weigh future "parity vs. divergence" questions with that in mind.

**QE2 — NINJA power-up privacy drop: match sauce or not?** *(explained
2026-05-24.)* In Zwift, power-ups are in-game items; "NINJA" is the
invisibility / stealth power-up that makes a rider disappear from other riders'
screens for a period. sauce4zwift honours that by *dropping* a rider's player
state while `activePowerUp === 'NINJA'` — it does not surface telemetry for a
rider who has chosen to be invisible. The power-up type is the low 4 bits of the
packed `aux3` field (`udp-node-msgs.proto:139`), and `NINJA` is enum value 6
(`udp-node-msgs.proto:106`). ranchero does not decode `activePowerUp` and never
drops these states. The question: should ranchero replicate this privacy
behaviour (decode `activePowerUp`, drop states when it is NINJA)? It is a
privacy/parity decision. *Controls:* a receive-side filter in 20.25.
→ **Answer (2026-05-24): Yes — replicate the drop.** Not masking the data while
the power-up is active would be a cheat: it would reveal a rider who has chosen to
be hidden. Naming note — the in-game name for this power-up is **Ghost**; `NINJA`
is only the internal proto/wire enum name (value 6). Decode `activePowerUp` and
drop the state while the Ghost/NINJA power-up is in use.

**QE3 — Reduce `GameEvent::PlayerState` to `{ athlete_id }` (cleanup,
yes/no)?** *(context added 2026-05-24.)* `GameEvent` is the internal broadcast
enum the relay layer sends to the web bridge. Its `PlayerState` variant carries
eleven scalar fields today, but only `athlete_id` is read downstream — the stats
fanout looks the athlete up in the registry, and the full proto travels
separately on the dedicated `player_states` stream. The other ten scalars are
copied but never used (vestigial and harmless). The cleanup is to shrink the
variant to `{ athlete_id }`; cost is re-touching about six test files and two
relay tests for no functional change. **Recommendation: low-priority cleanup —
do it opportunistically when the variant is edited for another reason** (for
example, while adding the new `GameEvent` variants for chat/rideon in
20.24 / 20.25). *Controls:* 20.19 item 1 only.

### F. Smaller match-sauce confirmations

**QF1 — `find_best_udp_server` fall-through.**
→ **Research finding (2026-05-24): confirmed, plus two adjacent deviations.**
Reading both implementations:
  - **Confirmed (the original observation):** when `use_first_in_bounds` is set
    but no server is in bounds, ranchero (`src/daemon/relay.rs:921-941`) falls
    through to the nearest-centre server, whereas sauce (`zwift.mjs:2277-2299`)
    returns `undefined` — i.e. "no swap".
  - **Also observed — in-bounds test:** sauce checks only upper bounds
    (`x <= xBound && y <= yBound`); ranchero checks all four
    (`x_bound_min..=x_bound`, `y_bound_min..=y_bound`).
  - **Also observed — distance reference:** sauce measures Euclidean distance to
    the bound corner (`xBound - x`); ranchero measures to the bound *centre*
    (`(x_bound_min + x_bound) / 2`).
  The two extra differences may be deliberate ranchero choices (ranchero added
  `x_bound_min` / `y_bound_min`, which sauce has no concept of) or drift from an
  older upstream — they should be reconciled against current upstream
  `findBestUDPServer`, not assumed to be bugs. **Recommendation:** match current
  upstream — return no-swap when nothing is in bounds — and decide deliberately
  on the bounds-test and centre-vs-corner differences. Low impact (only matters
  when a pool exists and the rider is outside every server's bounds). *Controls:*
  a branch in 20.25.

**QF2 — State-refresher: should it also poll self, and quiet HTTP 429s?**
*(reworded 2026-05-24.)* sauce's `_refreshStates` (`zwift.mjs:1998`) is the REST
fallback that polls the watched rider's state between live packets. Two
behaviours ranchero does not replicate: (1) when self ≠ watching, sauce *also*
polls self's own state, so the logged-in rider's data stays fresh even while
watching someone else; and (2) sauce suppresses log noise on HTTP 429
(rate-limit) responses, treating them as expected backpressure rather than
errors — ranchero issues one `get_player_state(watched_id)` and logs all errors
alike. The question: adopt both behaviours? Both are low impact under the
single-watched-athlete model and only matter once self-identity (QC1) and live
watching-from-stream (20.25 item 3) are in place. **Recommendation: adopt both
when self-identity lands; trivial additions.** *Controls:* 20.25 item 7.

**QF3 — Stale / duplicate-state guard.** (20.21.) Confirm we add sauce's
`_preprocessState` rejection of `elapsed <= 0` to protect the rolling-window
sums. *Controls:* the ingest guard in 20.21.
→ **Answer (2026-05-24): ok — add the guard.**

### Process

**QP1 — Are 20.21–20.28 being turned into formal `STEP-NN` plan files**, in the
priority order proposed above? The first four priorities (`getProfiles` →
per-tick recording → UDP inbound / reconnect / WorldUpdate decode → 1 Hz
nearby/groups) are tightly dependency-chained and are the difference between an
empty and a live payload, so they likely want to be sequenced steps rather than
parallel work.
→ **Answer (2026-05-24): yes, that is the next step.** The scope questions that
were open (QA3, QD2, QE1, QE2, QC1) are all decided as of this round, so the
20.21–20.28 items can now be cut into sequenced STEP-NN files in the priority
order above. The only remaining item is an implementation-time check, not a scope
decision: QB1's verification of proto tags 29 / 34 against a real capture, which
folds into the proto step.

### Already settled by this review (verified, no longer open)

Three claims that earlier entries marked "(verify)" or "lower-confidence" were
checked against the code on 2026-05-24 and are confirmed. They are implementation
tasks, not open questions:

- **road_time reverse adjustment is missing.** `ProtoView::road_time`
  (`src/web/proto_view.rs:75`) returns `road_time.unwrap_or(0)` with no
  `reverse ? 1005000 - roadTime : roadTime - 5000` adjustment. Must be added
  before road history is meaningful (20.21).
- **`DataCollector` has no growth mechanism.** Only the `RollingAverage`
  primitive (`crates/zwift-stats/src/rolling.rs:117`) has `resize`;
  `DataSlice::new_from` (`crates/zwift-stats/src/slice.rs:22`) calls
  `clone_reset`, producing an empty bucket that cannot grow. A growth mechanism
  is required before lap / segment / event bucket stats can fill (20.21).
- **`event_matches_athlete` mis-delivers arrays.** It returns `true` for the
  `nearby` / `groups` event names, so those subscriptions currently receive
  single-athlete payloads instead of arrays — a confirmed bug to fix, not a
  decision (20.22).

---

## How to use this file

When a step encounters a decision that is acceptable in this version
but worth revisiting later:

- Add a numbered subsection under **Open items** (`20.N — short
  title`).
- State where it came from, the current resolution, why it might come
  back, and a decision rule for when to revisit. Keep it concise:
  parking-lot entries should be readable within a minute.
- When an item is resolved or pulled into a step, move it to a
  **Resolved** section at the bottom, or delete it if the resolution
  was to retain the current approach.
