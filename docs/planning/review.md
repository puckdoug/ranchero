# Implementation review against ARCHITECTURE-AND-RUST-SPEC.md

**Date:** 2026-06-12
**Tree reviewed:** working tree on `main` at commit `90dc8e7` plus uncommitted
changes (the STEP-20 plan Step 19 persistence-wiring work in `src/daemon/`,
`src/web/`, and `tests/`).
**Method:** the spec (`docs/ARCHITECTURE-AND-RUST-SPEC.md`) was compared
section by section against the current code. Six parallel read-only sweeps
covered auth/REST, the relay protocol core, daemon orchestration, the stats
engine, the web surface, and persistence; every finding that contradicts the
STEP-20 plan checklist was then re-verified by hand against the working tree
before being recorded here. File:line references are to this tree.

**Relationship to STEP-20.** `STEP-20-additional-considerations.md` already
catalogs the 2026-05-23 gap analysis (items 20.21–20.28) and turns it into a
19-step plan whose checklist is fully ticked. This review does not repeat that
catalog; it records what is true in the tree *now* — which of those gaps are
genuinely closed, which remain open despite the ticked checklist, and what new
issues the closing work introduced.

---

## Overall assessment

The protocol layer, the math libraries, the formatters, and the compatibility
test battery are faithful to the spec and in good shape. The relay core
(framing, IV, header codec, session lifecycle, channel establishment,
reconnect, UDP inbound consumption, WorldUpdate decoding) now matches the spec
closely — the large relay gaps from 20.25 are closed. Auth and REST are at
parity, including the documented header mimicry.

The remaining distance to sauce4zwift parity is concentrated in one recurring
shape, the same shape the 2026-05-23 review identified: **libraries that are
complete and tested in isolation but have no production caller.** The most
serious instance is new: the relay's decoded game events (chat, ride-on,
state changes, segment results) are sent on a broadcast channel that the web
layer never subscribes to, so the event-stream work from plan Steps 10 and 14
is invisible to clients (finding G1). Behind that sit the still-unwired
profile fetch (G3), the absent 1 Hz nearby/groups processor (G2), the
unconfigured W′/zones accumulators (G4), the producer-less event-metadata
cache (G5), the unwired segment-leaderboard path (G6), the orphaned
`zwift-routes` crate (G7), and the inert KV store (G9).

A process observation accompanies these: the STEP-20 plan checklist marks all
19 steps "Implementation (green)", but Steps 12, and parts of 2, 6, 13, 14,
15, 16, and 17, are demonstrably not wired in this tree (the
`getNearbyData` stub even carries a comment saying its producer "lands in
Step 12"). The tests written for those steps pass because they drive the
library functions directly; they do not assert that the daemon's production
loop reaches them. See finding P1.

---

## A. Gaps blocking sauce4zwift parity

Ordered so that each entry unblocks the ones after it where a dependency
exists.

### G1 — Relay game events never reach the web layer (two disconnected channels)

**Spec:** §5.6, §7.9 (chat / rideon / game-state / segment events delivered to
subscribers). **Severity: blocking — and it silently nullifies completed work.**

`run_daemon` creates a `GameEvent` broadcast channel for the web layer
(`src/daemon/runtime.rs:299-303`) and hands it to `WebState`. The relay
runtime, started a few lines later, creates its **own, separate** channel
inside `RelayRuntime::start_with_writer` (`src/daemon/relay.rs:1435`). The
only traffic that crosses from relay to web is the full-proto `PlayerState`
stream, forwarded by the bridge task (`src/daemon/runtime.rs:341-366`), which
re-emits `GameEvent::PlayerState` onto the web channel
(`src/web/proto_to_stats.rs:228`).

Everything else the relay now correctly decodes and emits — `Chat`, `RideOn`,
`SegmentResult` (`relay.rs:3420-3455`, dispatched at `relay.rs:3593` and
`:3758`), `StateChange` (`relay.rs:104`, `:2445-2452`), and the simultaneous-
login signal — is sent on the relay's internal channel, which nothing in the
daemon consumes. The subscription layer handles these variants correctly
(`src/web/subs/mod.rs:405-438`) and its tests pass, but only because the tests
inject events directly into the web channel. In production the chat, rideon,
and game-state streams are silent.

**Recommendation.** Pass the web channel into the relay runtime (a
`start_with_writer` variant taking the sender — `start_with_deps_and_events_tx`
at `relay.rs:1498` already exists for tests), or add a forwarder task beside
the bridge that copies `rt.events()` onto the web channel. Then add one
integration test that goes daemon-edge to subscriber: inject a frame at the
relay boundary and assert a `chat` event arrives on a WebSocket subscription.
That test shape (production loop in, subscriber out) is the one missing from
the suite, and it would have caught this and most of the findings below.

### G2 — No 1 Hz nearby/groups processor (STEP-20 item 20.22, plan Step 12 — not implemented)

**Spec:** §5.4, §5.6, §7.8 (gap computation, group clustering, `nearby` and
`groups` emissions). **Severity: blocking.**

`compute_groups` (`crates/zwift-stats/src/groups.rs:59`), `apply_gap`,
`apply_gaps_for_registry`, and `apply_gap_chained`
(`crates/zwift-stats/src/gap.rs`) have no caller outside tests. There is no
periodic task other than `gc_tick_loop` (`src/web/server.rs:209`) and the
relay's own loops. Consequences, all verified:

- `gap`, `gap_distance`, `is_gap_est`, and `group_id` are never populated, so
  `/nearby/v1` sorts on a gap that is always `None` (order is effectively
  arbitrary) and `/groups/*` returns empty groups.
- `getNearbyData` / `getGroupsData` RPCs are explicit stubs returning `[]`,
  with a comment that the producer "lands in Step 12"
  (`src/web/rpc.rs:312-323`).
- No `nearby`/`groups` WebSocket events are ever emitted (the earlier
  mis-delivery bug is fixed — `event_matches_athlete` now rejects these names,
  tested at `src/web/subs/mod.rs:481` — but nothing produces the correct
  array payloads either).

**Recommendation.** Implement plan Step 12 as written in STEP-20 (a 1 Hz task
sibling to `gc_tick_loop` that runs gap estimation and group clustering over
the registry, writes the results onto the athlete records, emits `nearby` and
`groups` v1/v2 arrays, and backs the two RPCs). Dependencies (per-tick
recording, `most_recent_state`, road history) are in place. Port sauce's
chained gap estimation (`_computeNearby`) — `apply_gap_chained` exists but the
walk across adjacent riders is what makes estimated gaps usable.

### G3 — Profile fetch pipeline has no production driver (20.26 / 20.20, plan Steps 1–2 — library only)

**Spec:** §3.3, §5.7 (athlete profiles, FTP, names; athletes DB). **Severity:
blocking — G4 and the `athlete`/`tss` payload fields depend on it.**

`get_profiles` (batch, `crates/zwift-api/src/lib.rs:487-537`) and
`get_profile` (`lib.rs:472-485`) are implemented and tested, but no production
code calls them; the only REST fetcher the daemon invokes is
`get_player_state` (`src/daemon/relay.rs:1994`, `:2002`). The two-layer
`ProfileCache` with SQLite write-through is implemented and tested
(`src/web/state.rs:20-72`, `tests/profile_cache.rs`), but
`ProfileCache::insert_live` has no production caller, so the cache — and
`athletes.sqlite` beneath it — stays empty at runtime. `AthletesDb::touch`
(last-seen stamps) is likewise never called.

Published consequence: `athlete` is `null` and `tss` is `null` for every
rider, permanently — sauce populates these within seconds of seeing a rider
(`_maybeUpdateAthleteFromServer`, `stats.mjs:3080`).

**Recommendation.** Add the production driver: on first sight of an athlete
id (and on a staleness interval), enqueue it for a batched `get_profiles`
call; write results through `ProfileCache::insert_live`; have the formatters
read name/weight/FTP from the cache. Call `touch` from the per-tick path.
Batch and de-duplicate requests the way sauce does rather than issuing one
fetch per rider per tick.

→ **Fetch policy decided (2026-06-12, Q6): match sauce.** Fetch every
athlete seen, batched, refreshed when stale.

### G4 — W′ balance and power zones are never configured (20.21 residue, plan Step 6 — half done)

**Spec:** §5.2, §5.4, §7.8. **Severity: blocking for the W′/zones widgets.**

The per-tick path does call `w_bal.accumulate` and zone accumulation inside
`record_streams` (`crates/zwift-stats/src/athlete.rs:262`), but an
unconfigured `WBalAccumulator` returns `None` from `accumulate`
(`crates/zwift-stats/src/wbal.rs:56-57`) and an unconfigured
`ZonesAccumulator` accumulates nothing. Nothing in `src/` calls
`WBalAccumulator::configure[_from_profile]` or `ZonesAccumulator::configure`.
So `wBal` is `null` and `timeInPowerZones` is empty in every payload, and the
`wbal` stream records only `None`s.

**Recommendation.** When a profile arrives in the cache (G3), configure both
accumulators from it — `configure_from_profile(ftp, cp, w_prime)` already
implements sauce's fallback rules (`wbal.rs:38-47`); zones from
`coggan_zones(ftp)`. Reconfigure on FTP change, as sauce does.

### G5 — Event-metadata cache has readers but no writer (20.19 item 4 / 20.26, plan Step 13 — partial)

**Spec:** §5.5 (event metadata via `getEventSubgroup`), QA3 answer: event
support is a v1 requirement. **Severity: blocking for event widgets.**

`WebState.event_subgroups` is read in the per-tick path
(`src/web/proto_to_stats.rs:193`) and by the `getCachedEvent(s)` RPCs
(`src/web/rpc.rs:331-352`), but no production code inserts into it.
`get_event` (`crates/zwift-api/src/lib.rs:552-572`) is implemented and never
called; the `getEvent` RPC is a stub returning `null`. So even though the
proto tags and detection plumbing landed, `apply_event_state` always sees a
cache miss: event slices, privacy flags, `eventLeader`/`eventSweeper`, and
the event `remaining*` fields never materialize.

**Recommendation.** On first sight of an unknown `event_subgroup_id` in
telemetry, fetch the event via `get_event`, populate `event_subgroups`, and
back the `getEvent` RPC with the same path. This is the QA3 chain's one
missing link.

### G6 — Segment leaderboards: fetchers, store, and evictor all unwired (20.17 item 2 / 20.26 / QA2, plan Step 15 — partial)

**Spec:** §5.7; QA2 answer keeps leaderboards in scope. **Severity: blocking
for leaderboard widgets; one part is a latent operational bug.**

`get_segment_results`, `get_live_segment_leaders`, and
`get_live_segment_leaderboard` (`crates/zwift-api/src/lib.rs:579-658`) have no
production caller. `SegmentsDb::put`/`get` are never called outside tests;
the `getSegmentResults` RPC returns `[]`. The TTL evictor task is fully
implemented (`src/daemon/stores.rs:23-42`) and exported, but **never
spawned** — `run_daemon` spawns only the web server and `gc_tick_loop`.

**Recommendation.** Wire the fetchers behind the active-segment path and the
RPC, write results through `SegmentsDb`, and spawn
`segments_evict_tick_loop` in `run_daemon` next to the other background
tasks. Spawning the evictor is a one-line change and should be done first —
if any writer arrives before it, the leaderboard table grows without bound.

### G7 — `zwift-routes` is an orphan crate (20.27 item 3 / QA4, plan Step 17 — library only)

**Spec:** §7.2, §7.8; QA4 answer: route progress is a v1 goal. **Severity:
blocking for `routeDistance` / route `remaining*`.**

`crates/zwift-routes` exists with route lookup, lead-in distance, and
remaining-info computations, each tested — but nothing depends on it: it is
not a dependency of the root crate (`Cargo.toml` lists no `zwift-routes`),
and no file outside the crate references it. By contrast `zwift-worlds` from
plan Step 18 *is* wired (lat/lng projection and altitude adjustment in
`src/web/proto_to_stats.rs:67-79`). Route distance, route percentage, and
route `remaining*` therefore stay absent from every payload.

**Recommendation.** Add the dependency and call the route computations from
the per-tick path (`route_player_state`), keyed by the rider's `route_id`,
mirroring sauce's `_computeRouteDistance` and the route half of
`_getEventOrRouteInfo`.

### G8 — Watched-athlete switching and `watching-athlete-change` have no production path

**Spec:** §5.6; STEP-20 item 20.24. **Severity: medium — single-athlete use
works; switching does not.**

`GameEvent::WatchingAthleteChange` is defined (`src/daemon/relay.rs:1246`)
and the subscription layer delivers it (`src/web/subs/mod.rs:435-438`), but
no code ever sends it — `grep` finds no `send(GameEvent::WatchingAthleteChange`
anywhere. `switch_watched_athlete` (`relay.rs:2602`) is production API but
has no caller, and `watching_id` is set once at boot
(`src/daemon/runtime.rs:307`). There is also no RPC or other control surface
through which you could request a switch.

**Recommendation.** Emit `WatchingAthleteChange` from
`switch_watched_athlete` and route it across the G1 forwarder.

→ **Trigger decided (2026-06-12, Q7): match sauce.** Follow the game's
watching state from telemetry and switch accordingly. This also makes the
parked items 20.13/20.14 (mid-ride course transitions) relevant again.

### G9 — KV store and settings persistence are inert (20.28 item 3 residue, plan Step 19 — partial)

**Spec:** §5.7, §7.10. **Severity: medium.**

`Stores::open` runs and the handles now reach `WebState` (`with_stores`), but
the KV handle is explicitly discarded — `kv: _` in
`src/web/state.rs:223` — and `getSetting`/`setSetting` operate on an
in-memory `HashMap` (`src/web/state.rs:108`), so all settings are lost on
restart. The athletes and segments handles are attached but, per G3/G6, have
no production traffic.

**Recommendation.** Back the settings RPCs with the KV store (read-through on
boot, write-through on `setSetting`), which also gives the `app` source's
`setting-change` event something real to announce once G1 is fixed.

---

## B. Functional defects (smaller, independently fixable)

### D1 — Cadence overflow clamp missing (spec §7.12)

Spec §4.11/§7.12: cadence values above `240 × 1e6 / 60` (4,000,000 µrev/s)
are Zwift lag-burst artifacts and "must be clamped or dropped" (sauce treats
them as 1). No clamp exists — `src/web/proto_to_stats.rs` converts
`cadence_u_hz` to RPM unguarded, and no constant resembling the limit appears
in the workspace. A lag burst would push garbage into the cadence rolling
windows and peaks. **Recommendation:** clamp at the proto-view or conversion
boundary, with a unit test using an over-limit value.

### D2 — `/nearby/v2` is unsorted

`nearby_v1_handler` sorts by gap; `nearby_v2_handler` iterates the registry
in `HashMap` order (`src/web/http/mod.rs:266-278`). Once G2 populates real
gaps, v2 clients would receive correctly-gapped but arbitrarily-ordered
arrays. **Recommendation:** apply the v1 sort in the v2 handler; fold into
the G2 work.

### D3 — `streams/*` events exist over HTTP only

`/athlete/streams/v1/...` serves stream arrays, but the subscription layer
has no producer for `streams/watching|self|{id}` events (no `"streams/"`
match in `src/web/subs/mod.rs`), which sauce emits (spec §5.6).

→ **Answer (2026-06-12, Q8): WebSocket push is critical — implement it for
v1.** Add a streams fanout; sauce emits slices on a short interval, not per
state.

### D4 — Stub RPC handlers that have real data behind them

The RPC surface (≈31 handlers) is far past the one-handler state of 20.23,
but several registered handlers return stubs where the supporting data
already exists or is one wiring step away (`src/web/rpc.rs:420-426` and
nearby):

- `getWorldMetas` returns `[]` and `getCourseId`/`getRoad`/`getRoute`/
  `getSegment` return `null`, despite `zwift-worlds` being wired into the
  per-tick path and `zwift-routes` existing (G7).
- `getWebServerURL` returns `null` — the bound address lives in the daemon's
  web handle but is not threaded into the registry.
- `getPowerProfile`, `getEventSubgroupEntrants`, `getEventSubgroupResults`
  return empty values (the latter two depend on G5).

**Recommendation:** back the geometry getters from `zwift-worlds`/
`zwift-routes`, thread the server URL through `WebState`, and leave the
event-dependent stubs until G5 — but mark each stub with the finding it waits
on so they read as pending, not done.

### D5 — `IdleFSM` is production dead code

The motion-based idle state machine the spec describes (§4.13: suspend UDP
after ~60 s of `speed == 0 && cadence == 0 && power == 0`) is implemented
(`src/daemon/relay.rs:1056-1151`) but called only from tests. Production
suspension is time-based instead: no fresh self state for 15 s
(`relay.rs:650-657`). An implemented-but-unwired state machine invites the
assumption that it is active.

→ **Resolved by Q2 (2026-06-12): connect `IdleFSM` to the receive path** so
suspension follows rider motion as the spec describes.

### D6 — State refresher polls the monitor account's id as "self" (still unresolved)

Already logged in detail in
[`refresher-self-id-bug.md`](refresher-self-id-bug.md) (2026-05-26); recorded
here so this review is the complete gap list. The code is unchanged from what
that document describes: `start` resolves `athlete_id` from the monitor
account's login (`src/daemon/relay.rs:1676`) and passes it to
`run_state_refresher` as `self_id` (`relay.rs:2311`). Because the monitor's
id differs from the watched athlete's, the `self_id != watched_id` branch
(`relay.rs:604-617`) polls the monitor account — which is not a rider and has
no player state to fetch. The fix proposed in that document (pass the watched
athlete's id, or the main rider's id once that account is represented in the
start path) has not been applied. The test that catches this
(`tests/relay_runtime.rs:3127`, which asserts every poll targets the watched
id) is expected to fail; the test-suite addendum at the end of this file
confirms the suite's current state.

---

## C. Deviations from the spec (all decided 2026-06-12)

These are places where the code disagreed with the spec. Each now has a
recorded decision; the document or code on the losing side of each decision
is listed for correction.

### V1 — Keyring service name

Spec §7.10: use service `"Zwift Credentials - Sauce for Zwift"` so an
existing Sauce install's credentials are picked up unchanged. Code:
service `"ranchero"`, with a comment stating the separation is intentional
(`src/credentials/mod.rs:6-18`). The two cannot both stand.

→ **Answer (2026-06-12): keep `"ranchero"`.** The intentional separation
stands. Amend spec §7.10 to describe the ranchero-owned keyring entry;
credentials from an existing sauce4zwift install are not picked up, and you
enter them once through `ranchero configure`. No further action in code.

### V2 — Idle-suspension model

Spec §4.13/§7.7: motion-based (all three of speed/cadence/power zero for
~60 s). Code: time-based (no fresh watched/self state for 15 s), with the
motion-based FSM present but unwired (D5). The time-based model suspends in
situations the spec's model would not (e.g. telemetry gaps while riding) and
does not suspend a rider who is stopped but still emitting states.

→ **Answer (2026-06-12, Q2): follow the spec as sauce does.** Connect
`IdleFSM` so suspension is motion-based; align any remaining time-based
behavior with sauce's. Code change required.

### V3 — State-refresher stale backoff

Spec §4.13: multiply the refresh interval by 1.02 on stale responses, 1.15 on
errors. Code: 1.15 on errors is present (`relay.rs:664-676`), but staleness
uses an adaptive expand/contract scheme (`relay.rs:658-660`) instead of the
1.02 multiplier.

→ **Answer (2026-06-12, Q3): keep ranchero's scheme.** Amend spec §4.13; no
code change.

### V4 — `game-state` event semantics

The subscription layer emits `game-state` on `GameEvent::StateChange`
(`src/web/subs/mod.rs:429-434`), which carries ranchero's daemon runtime
state (`Authenticating`, `TcpEstablished`, …). sauce's `game-state` stream
carries the game-client state object. A sauce widget subscribing to
`game-state` will receive structurally different data.

→ **Answer (2026-06-12, Q4): build the sauce-shaped game-state object and
emit it under `game-state`.** Move the daemon's connection lifecycle to its
own event name. Reusing an established name for different data is confusing
and unacceptable. Code change required.

---

## D. Code and design risks

### K1 — Duplicated inbound-decode logic in the TCP and UDP receive loops

Parked as 20.2 when the duplication was small. It is no longer small: both
arms now carry the NINJA-powerup drop, watched-position capture, WorldUpdate
decode, and pool updates (`relay.rs:3476+` and `:3697+`). Any future change
must be made twice; a missed twin produces transport-dependent behavior,
which is the hardest kind to notice. **Recommendation:** extract the shared
inbound path now that both arms are live; the parked item's "revisit when"
condition has been met.

### K2 — Blocking SQLite calls on the async runtime

`AthletesDb`/`SegmentsDb`/KV use synchronous `rusqlite` behind a mutex. Today
nothing calls them on hot paths (G3/G6/G9), but the recommendations above
will change that. **Recommendation:** when wiring the writers, route SQLite
work through `tokio::task::spawn_blocking` (or batch writes onto a dedicated
thread) rather than calling from async handlers.

### K3 — Token-refresh task gives up after one failure

The preemptive refresh task logs and exits on error
(`crates/zwift-api/src/lib.rs:1108-1121`); recovery then depends entirely on
the inline 401-refresh path. That is probably adequate (any subsequent API
call repairs the session), but the relay heartbeat does not make REST calls,
so a daemon that is otherwise healthy could run for a long time before
anything exercises the 401 path. **Recommendation:** have the refresh task reschedule
itself with backoff instead of exiting, or confirm the 401 path is reached
often enough in steady state (the 3 s state-refresher polls REST, which
likely suffices — verify and note it).

### K4 — No daemon-edge-to-subscriber integration test

Every gap in section A shares one property: unit tests pass because they call
the libraries directly, while the daemon's production loop never reaches
them. The suite has excellent piece-level coverage and replay-based metric
parity, but no test starts at `run_daemon`'s wiring (or a thin harness over
it) and asserts on what a WebSocket subscriber receives. **Recommendation:**
add one such test per event family (athlete, chat/rideon, nearby/groups,
game-state). G1 makes the case: every involved unit test passes while the
production path is severed.

---

## E. Process finding

### P1 — The STEP-20 plan checklist overstates completion

The checklist (`STEP-20-additional-considerations.md` lines 48–110) marks all
19 steps "① Tests (red)" and "② Implementation (green)". The tree contradicts
this for several steps:

| Plan step | Claimed | Found in tree |
|---|---|---|
| 1–2 (profile fetch + cache) | complete | Library + cache complete; no production caller (G3) |
| 6 (W′ + zones) | complete | Accumulation wired; configuration absent (G4) |
| 12 (1 Hz nearby/groups) | complete | Not implemented; stubs reference it as pending (G2) |
| 13 (event chain) | complete | Detection plumbing present; metadata producer absent (G5) |
| 14 (live event streams) | complete | Subs layer complete; severed from the relay (G1) |
| 15 (segment leaderboards) | complete | Store + evictor exist; nothing fetches, writes, or spawns (G6) |
| 16 (RPC getters) | complete | Registered; several stubs where data exists (D4) |
| 17 (route tables) | complete | Crate exists; orphaned (G7) |

→ **Resolved (2026-06-12, Q5): there are no unmerged branches — the missing
work is missing.** The boxes were ticked without the production wiring being
in place. The STEP-20 file has since been moved to `done/` and its checklist
is archived as-is; this review (sections A and B) is now the document of
record for the remaining work. Going forward, a step's implementation should
not be called done without a production-path test (K4), not only library
tests.

---

## F. Areas verified at parity (checked, no action needed)

For balance and future reference, these were examined to the same depth and
found conformant:

- **Relay protocol core** (spec §4.1–4.8, §7.4–7.7): login handshake, AES-128-
  GCM-4 with explicit zeroed IV bytes 0–1, header codec with last-known-value
  semantics, TCP/UDP framing differences, 25-shot UDP hello with median-by-
  lowest-latency time sync, watchdog at timeout/2, session refresh at 90%
  with re-login fallback, backoff 1000 ms × 1.2ⁿ, no TCP keepalive (with the
  spec-referenced comment), server-pool filtering and `find_best_udp_server`.
- **Relay live-data path repairs from 20.25**: UDP inbound is consumed and
  routed (`relay.rs:3697-3800`); TCP reconnect rebuilds UDP and the heartbeat
  (`relay.rs:2916-3261`); watched position is updated from the stream and
  drives UDP selection (`relay.rs:3711-3723`, `:540`); heartbeat carries
  portal/roadId/eventSubgroup (`relay.rs:835-859`); `multipleLogins` is
  detected (`relay.rs:3642-3645`); NINJA states are dropped in both arms.
- **Auth/REST** (spec §3, §7.5): password grant with the literal
  `Zwift Game Client` client id, 50% preemptive refresh, inline 401 refresh
  and retry, game-client header mimicry with `Config::source` override,
  capture sink, and the `auth-check` no-socket preflight.
- **Stats libraries** (spec §5.3, §7.8): rolling windows with gap-fill, NP
  (30 s window, 300 s gate), TSS formula, 1 s bucketing, correct peak-period
  sets, athlete/group GC TTLs and tick.
- **Per-tick recording from 20.21** (except G4): `most_recent_state`, streams,
  road history with the reverse road-time adjustment
  (`src/web/proto_view.rs:75-82`), work/follow/solo/coffee splits, grade
  computed and published, stale/duplicate guard, auto-lap, slice growth, and
  active-segment hooks are all wired in `route_player_state`
  (`src/web/proto_to_stats.rs:50-211`).
- **Web protocol** (spec §6.3, §7.9): `/api/socket` JSON frames, subscribe
  arg shape, 8 MB backpressure drop, v2 query reduction, in-tree `pages/`,
  HTTPS-if-certs-present, unknown subscription sources rejected.
- **Persistence substrate** (spec §7.10): WAL mode, JSON-blob athletes schema
  (the 20.28 item 1 objection is resolved by migration
  `0002_json_blob.sql`), in-memory event-subgroup cache per decision QD2.
- **Compatibility battery** (spec §7.11): AES vector against a Node-generated
  oracle, header round-trip across all 8 flag combinations, recorded-session
  login/decode, metric parity with synthetic oracles plus a recorded-ride
  golden, widget payload and headless-render snapshots — fast/slow gating per
  the project convention.
- **Scope hygiene** (spec §7.1): no GUI/FIT/mods/companion/Sentry
  half-implementations; no build, test, or runtime path resolves through the
  `sauce4zwift` symlink; no `todo!()`/`unimplemented!()`; the few
  `#[allow(dead_code)]` are documented (except as noted in D5).

**Test suite:** `cargo test` (fast set) was run on this tree as part of the
review; the result is recorded in the addendum at the end of this file.

---

## G. Open questions

Questions the code cannot settle on its own. Each one explains the
background, the choice to be made, and what depends on the answer. Q numbers
are independent of the answered question groups in STEP-20.

**All eight questions were answered on 2026-06-12.** The answers are recorded
inline below and folded into the findings they resolve (V1–V4, D3, D5, G3,
G8, P1).

### Q1 — Keyring service name (V1) — ANSWERED

→ **Answer (2026-06-12): keep `"ranchero"`.** See V1 for the resolution;
spec §7.10 is to be amended.

### Q2 — Which idle-suspension design is the intended one? (V2, D5)

**Background.** The daemon suspends its UDP relay channel when there is
nothing worth receiving, to reduce load on Zwift's relay servers. The spec
(copying sauce4zwift) says to decide this from the rider's telemetry: if
speed, cadence, and power are all zero for about 60 seconds — the rider has
stopped — suspend, and resume the moment any of them is non-zero again. What
the code actually does is different: it suspends when *no telemetry at all*
has arrived for the watched athlete in 15 seconds, whatever the last values
were. The spec's design was fully written (`IdleFSM`,
`src/daemon/relay.rs:1056-1151`) but was never connected to the receive path;
only its unit tests call it.

**Practical difference.** The time-based rule suspends during stream
interruptions even while the rider is actively riding, which can cause
suspend/resume churn mid-ride. And it does not suspend when a stopped rider
keeps emitting zero-value states (for example, standing at a start banner) —
exactly the case the spec's rule was designed to catch.

**The question.** Should ranchero (a) keep the time-based rule, delete
`IdleFSM`, and amend spec §4.13 to describe what is actually built, or
(b) connect `IdleFSM` so suspension follows rider motion as the spec
describes? If (b), a follow-up: does the 15-second no-data rule stay as a
second trigger or go away?

→ **Answer (2026-06-12): (b) — follow the spec as sauce does.** Connect
`IdleFSM` so suspension is driven by rider motion. Where sauce runs both
mechanisms (it also slows its polling after 15 seconds without data), match
sauce's behavior rather than inventing a ranchero-specific combination.

### Q3 — Accept the polling-slowdown scheme, or copy sauce's exactly? (V3)

**Background.** Besides the live stream, the daemon polls Zwift's REST API
every few seconds for the watched athlete's current state (the "state
refresher"). Both sauce and ranchero slow this polling down when the
responses stop being useful, to avoid needless API traffic. sauce's rule:
each stale response (no newer data than last time) multiplies the polling
interval by 1.02 — a very gentle drift upward; each error multiplies it by
1.15. ranchero matches the 1.15-on-error rule, but for staleness it does
something different: once no fresh data has arrived for 15 seconds, it moves
the interval halfway toward 30 seconds on each poll, and when fresh data
resumes it moves halfway back toward 3 seconds (`relay.rs:658-660`).

**Practical difference.** ranchero backs off faster when data goes quiet and
recovers faster when it returns; sauce drifts up slowly and only fully
recovers on fresh data. Neither is obviously wrong. The only real stakes are
slightly more or fewer REST requests in particular windows, and whether
"parity with sauce" is taken literally here.

**The question.** Keep ranchero's scheme and amend spec §4.13, or replace it
with sauce's 1.02 multiplier for exact parity?

→ **Answer (2026-06-12): keep ranchero's scheme.** Amend spec §4.13 to
describe the halfway-toward-30-seconds expansion and halfway-toward-3-seconds
recovery as the intended design. No code change.

### Q4 — What should the `game-state` event carry? (V4)

**Background.** sauce4zwift publishes an event stream named `game-state`
whose payload describes the state of the game session being watched — which
world and course the rider is in, whether they are in an event, and so on.
Widgets subscribe to it to know what the game is doing. ranchero currently
reuses that event name for something else: it emits `game-state` whenever the
daemon's own connection lifecycle changes, with values like `Authenticating`,
`SessionLoggedIn`, `TcpEstablished` (`src/web/subs/mod.rs:429-434`). A sauce
widget subscribed to `game-state` would therefore receive ranchero
connection-status values instead of game information, and would not render
correctly.

**The question.** Three options:
(a) build a sauce-shaped game-state object from telemetry, emit it under
`game-state`, and move the connection lifecycle to a new event name such as
`connection-state` — this is what widget compatibility requires;
(b) keep the current behavior and document that game-state widgets will not
work against ranchero;
(c) remove the event until a real game-state source exists.

→ **Answer (2026-06-12): (a).** Reusing an established event name for
different data is confusing and unacceptable. Build the sauce-shaped
game-state object, emit it under `game-state`, and move the daemon's
connection lifecycle to its own event name.

### Q5 — Is the missing work on an unmerged branch, or is the checklist wrong? (P1)

**Background.** The STEP-20 plan checklist shows all 19 steps with both
boxes ticked ("Tests red", "Implementation green"). This review found eight
of those steps incomplete in the tree on `main` — the table in P1 lists them.
The clearest example: the `getNearbyData` RPC stub's own comment says its
producer "lands in Step 12", while Step 12's boxes are ticked. Two
explanations fit the evidence: the work was finished on a branch that has not
been merged into `main`, or the boxes were ticked when the supporting library
code was finished even though the daemon never calls it.

**The question.** Does such a branch exist? If yes, the path forward is to
merge it and re-run this review's checks against the result. If no, the
checklist should be corrected to match section A of this review and the
affected steps re-opened.

→ **Answer (2026-06-12): there are no unmerged branches — missing work is
missing.** The STEP-20 file has since been moved to `done/`, so its checklist
is archived as-is; this review (sections A and B) is now the document of
record for the remaining work.

### Q6 — How many athlete profiles should the daemon fetch from Zwift? (G3)

**Background.** A rider's name, weight, and FTP are not in the live
telemetry stream; they must be fetched from Zwift's REST API. Without them,
every athlete in every payload shows `athlete: null`, and TSS and W′ balance
cannot be computed at all (they need FTP). sauce fetches the profile of every
athlete it sees on course — batched into requests of many ids at once, and
refreshed when stale — which on a busy course means API requests covering
hundreds of riders. The caution about copying that exactly: ranchero would
generate the same API load, and Zwift's tolerance of a non-game client doing
bulk profile fetches is unknown (the same caution that applies to the
`Source` header mimicry).

**The question.** How aggressive should v1 be?
(a) Match sauce: fetch everyone seen, batched.
(b) Fetch only the watched athlete plus riders currently nearby.
(c) Fetch only the watched athlete.
Option (c) is the smallest API footprint but leaves the nearby list without
names; (b) is a middle ground; (a) is full parity. The plumbing is the same
in all three cases — this only sets the fetch policy.

→ **Answer (2026-06-12): (a) — match sauce.** Fetch the profile of every
athlete seen, batched and refreshed when stale, as sauce does.

### Q7 — What triggers a change of watched athlete? (G8)

**Background.** The "watched" athlete is the rider whose data fills the main
widgets. Today ranchero fixes it at daemon start from
`watched_athlete_id` in the config, and it never changes while running. In
sauce, watching follows the game: when you change who you are spectating
inside Zwift, sauce notices from the telemetry and switches everything over,
emitting a `watching-athlete-change` event so widgets refresh. ranchero has
the switching function (`switch_watched_athlete`) and the event type, but
nothing ever calls or emits them, and there is no other way to request a
switch while the daemon runs.

**The question.** For v1, should switching
(a) follow the game as sauce does (requires identifying the game's
"who am I watching" signal in the telemetry and acting on it),
(b) be exposed as an RPC (for example `setWatching`) so a client can switch
manually,
(c) stay fixed for the lifetime of the daemon, documented as a v1 limit?
The answer decides whether the `watching-athlete-change` stream can ever
fire, and whether the parked items 20.13/20.14 (mid-ride course transitions)
have a purpose.

→ **Answer (2026-06-12): (a) — match sauce.** Follow the game's watching
state from telemetry, switch the watched athlete accordingly, and emit
`watching-athlete-change`. The parked items 20.13/20.14 stay relevant.

### Q8 — Do stream charts need live push, or is fetch-on-demand enough? (D3)

**Background.** "Streams" are the accumulated per-ride arrays — power,
speed, heart rate, altitude, position over time — that chart-style widgets
draw. sauce delivers them two ways: a one-shot fetch, and WebSocket
subscriptions (`streams/watching` and similar) that push updates as the ride
progresses. ranchero has the one-shot HTTP route
(`/athlete/streams/v1/...`) but no WebSocket push, so a sauce chart widget
that subscribes would stay empty, while one that polls works today.

**The question.** Should v1 implement the WebSocket push for streams, or
accept fetch-only and record which widgets that excludes? The answer mostly
depends on which widgets you intend to support first — if none of them are
chart widgets that subscribe, this can wait.

→ **Answer (2026-06-12): WebSocket push is critical.** Implement the
`streams/*` subscription push for v1; fetch-only is not acceptable.

---

## Suggested order of work

All decisions are in (questions Q1–Q8 answered 2026-06-12), so every item
below is ready to be planned and built.

1. **G1** (channel forwarding) with the K4 integration test — a small change
   that makes already-completed event work visible to clients.
2. **G3 → G4** (profile driver fetching every athlete seen, then W′/zones
   configuration) — unblocks `athlete`, `tss`, `wBal`, `timeInPowerZones`.
3. **G2** (1 Hz processor) + **D2** — nearby/groups, the most-used widgets.
4. **G5** (event metadata producer) — completes the QA3 chain.
5. **D3** (streams WebSocket push — decided critical for v1).
6. **G6** (segment leaderboards; spawn the evictor first).
7. **G7** (wire `zwift-routes`) and the geometry half of **D4**.
8. **G8** (follow the game's watching state, emit `watching-athlete-change`)
   and **V4** (real game-state object; rename the connection stream).
9. **G9, D1, D5, D6** — settings persistence, the cadence clamp, wiring
   `IdleFSM`, and the refresher self-id fix (`refresher-self-id-bug.md`
   proposes the change).
10. **K1** (shared inbound decode) before the next change to the recv loops.
11. Spec amendments from the decided deviations: §7.10 (keyring, V1) and
    §4.13 (polling slowdown, V3).

---

## Addendum — test-suite status and timing (2026-06-12)

**Result: 593 passed, 1 failed, 50 ignored.** The single failure is the D6
bug:
`tests/relay_runtime.rs::state_refresh_polls_get_player_state_on_self_tuning_interval`
— "refresher must poll the watched athlete's ID (54321), NOT the monitor's
(12345); saw 12345". This is exactly the
[`refresher-self-id-bug.md`](refresher-self-id-bug.md) failure, fixed by
**Step 30**. No other test fails.

### Timing — what is actually slow, and what is not

I measured the same default set at **363 s, then 95 s, then 30 s** on the
same tree. The variance is the finding: the wall time is dominated by **cargo
build-lock contention**, not by the tests themselves.

- A built integration-test binary, run directly, executes in **~0.05 s**.
- The same binary via `cargo test --test X` takes **~1 s** — that second is
  cargo's per-invocation overhead (build-graph and freshness check), not test
  work.
- A clean `cargo test` with nothing else touching cargo: **30 s**, of which
  only ~4.4 s is CPU. The remaining ~26 s is cargo orchestrating **92
  separate integration-test binaries** plus intermittent waits on the shared
  build lock.
- The 95 s and 363 s figures were measured while **other cargo processes ran
  concurrently** (during the review, my own measurement loops; in normal use,
  the editor's `rust-analyzer`, PID seen holding
  `target/debug/.cargo-lock`). Every concurrent `cargo` invocation blocks on
  that one lock, so the suite balloons. This is the "so slow as to be
  useless" experience.
- The slow-marker convention is healthy: **50 tests are `#[ignore =
  "slow: …"]`**, and the six binaries that first appeared to take 40–80 s each
  all run in ~1 s in isolation — they were contention artifacts, not unmarked
  slow tests. (This is unlike the 2026-05-23 regression, which was genuinely
  unmarked slow tests.)

**So the problem to fix is structural, not a few mismarked tests:** 92
separate integration binaries (heavy to build and orchestrate, each linking
the full dependency tree) plus a build lock shared with `rust-analyzer`. A
dedicated plan, [`STEP-20.9-test-suite-speed.md`](STEP-20.9-test-suite-speed.md),
addresses both and runs first, ahead of Step 21. Target: a clean default set
well under one minute (consolidation should bring it toward ~10 s) and
resilient to editor lock contention.
