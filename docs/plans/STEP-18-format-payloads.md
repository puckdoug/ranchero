# Step 18 — v1/v2 payload formatters (field-for-field parity)

## Goal

Replace the placeholder formatters that STEP 17 left in
`src/web/http/mod.rs` with byte-faithful ports of sauce4zwift's
`_formatAthleteData` and `_formatAthleteDataV2`, plus the slice and
stream formatters they depend on, so that unmodified sauce4zwift widgets
read the same field names and shapes from the ranchero daemon
(spec §7.9, §7.12 `keepCase` hazard).

Concretely this step delivers:

- The v1 athlete formatter (`_formatAthleteData`, `stats.mjs:4388`) with
  every field present and correctly named.
- The v2 athlete formatter (`_formatAthleteDataV2`, `stats.mjs:4325`)
  with correct per-resource filtering.
- The bucket-stats formatters in both shapes (`_getBucketStats`
  `stats.mjs:2664`; `_getBucketStatsV2` `stats.mjs:2714`) and the
  underlying stat-shape primitives (`getStatsSlow`/`getStatsV2`/
  `getNPStatsSlow`/`getNPStatsV2`, `stats.mjs:196-345`).
- The slice formatters (`_formatDataSlice`/`_formatSegmentDataSlice`/
  `_formatEventDataSlice` and `_filterAthleteDataSlices`,
  `stats.mjs:1699-1762`) and the stream formatter
  (`_getAthleteStreams`, `stats.mjs:1782`).
- The four formatter-dependent routes that STEP 17 documented in the API
  directory but never registered: `/api/athlete/laps/v1/{id}`,
  `/api/athlete/segments/v1/{id}`, `/api/athlete/events/v1/{id}`,
  `/api/athlete/streams/v1/{id}`.
- An `ADV2QueryReductionEmitter`-equivalent in the subscription engine so
  that N WebSocket subscribers carrying identical v2 queries cost one
  serialization per emission.

Field names stay byte-identical to the JavaScript formatters (camelCase,
with the underscored internal names removed), because the field casing is
the contract the widgets read.

---

## Summary checklist (implementation, by phase)

Each item is either a failing test (`-T`) written first, or the
production code (`-I`) that turns it green. Write the `-T`, watch it fail,
then write the smallest `-I` that passes. Phases are ordered so each
builds on the last; within a phase the tests are independent.

### Phase 0 — Parity harness and deterministic builder

- [x] **18.0-T** A reusable test-support module builds a deterministic
      `AthleteData` from a fixed sample script, and a comparison helper
      asserts two `serde_json::Value`s are equal with float tolerance and
      with an exact key-set check at every nesting level.
- [x] **18.0-I** Add `tests/support/mod.rs` (deterministic builder +
      `assert_json_parity`) and a `tests/fixtures/format/` directory for
      authored golden JSON. No JavaScript is executed — see
      "Parity strategy" below.
- [x] **18.0b-I** Create `src/web/format.rs` and move the existing
      `format_athlete`/`format_athlete_v2` (and their helpers) out of
      `src/web/http/mod.rs` into it; update the `use` in `src/web/subs/mod.rs`
      (currently `crate::web::http::format_athlete`) and re-export from
      `src/web/mod.rs` as needed. Pure refactor: the existing
      `http_athlete_v1.rs`/`http_athlete_v2.rs`/`subs_event_payload.rs`
      tests stay green and are the safety net (no new test).

### Phase 1 — Stat-shape primitives in `zwift-stats` and `zwift-relay`

- [x] **18.1-T** `RollingAverage::joules` and `RollingPower::joules`
      return the cumulative value·time accumulator.
- [x] **18.1-I** Add `joules()` to `RollingAverage` (returns the existing
      `values_acc`) and expose it on `RollingPower` through its inner roll.
- [x] **18.2-T** `WorldTimer::to_server_time(wt)` and
      `to_local_time(wt)` match the JS arithmetic (`zwift.mjs:104-114`).
- [x] **18.2-I** Add both methods to
      `crates/zwift-relay/src/world_timer.rs` and reconcile the
      world-time unit (gap G5).
- [x] **18.3-T** `DataCollector::stats(ts_offset_ms)` returns a
      `SignalStats { avg, max, peaks, smooth }` POD whose `peaks` carry
      `{ period, avg, time, ts }` for every periodised window and whose
      `smooth` carries `{ period, avg }` for windows with period ≤ 1200 s.
- [x] **18.3-I** Add `SignalStats`, `PeakStat`, `SmoothStat` PODs and the
      `stats()` method to `DataCollector` (mirrors `getStatsV2`,
      `stats.mjs:196`). The v1/v2 JSON container difference is the
      formatter's job, not this method's.
- [x] **18.4-T** `PowerDataCollector::np_stats(ts_offset_ms)` returns an
      `NpStats` POD restricted to windows with period ≥ 300 s.
- [x] **18.4-I** Add `np_stats()` honouring the ≥ 300 s offset
      (`_npPeriodizedOfft`, `stats.mjs:265`) and the smooth bound
      (`_smoothPeriodizedLength`, `stats.mjs:129`).

### Phase 2 — v1 bucket-stats formatter

- [x] **18.5-T** A golden test asserts the v1 stats block for a
      deterministic bucket matches `tests/fixtures/format/v1_stats.json`,
      including the period-keyed `peaks`/`smooth` objects, the deprecated
      `wBal`/`timeInPowerZones` fields, `kj`, `np`, `tss`, and the `np`
      sub-block with no `max`.
- [x] **18.5-I** Implement `format_bucket_stats_v1(bucket, ad, ctx)` as a
      verbatim port of `_getBucketStats` (`stats.mjs:2664`).

### Phase 3 — v1 athlete formatter (full)

- [x] **18.6-T** Extend `tests/http_athlete_v1.rs` and add a golden so
      `/api/athlete/v1/{id}` returns every `_formatAthleteData` field with
      correct names/shapes for a deterministic athlete.
      (Implemented as `tests/format_v1_athlete.rs` — direct formatter call
      rather than HTTP so `age` is deterministic; golden at
      `tests/fixtures/format/v1_athlete.json`.)
- [ ] **18.6-I** Replace the stub `format_athlete` with a verbatim port of
      `_formatAthleteData` (`stats.mjs:4388`), including `format_state`
      (`_formatState`, `stats.mjs:4231`) and `event_or_route_info`
      (`_getEventOrRouteInfo`, `stats.mjs:4291`).
- [x] **18.7-T** Fields whose source data is absent serialise exactly as
      the JS does: `self`/`watching`/`isGapEst` omitted when falsy,
      `lastLap` `null` with one lap, `state` `null` with no state.
      (Five tests in `tests/format_v1_athlete.rs`.)
- [ ] **18.7-I** Use named `#[derive(Serialize)]` structs with
      `#[serde(skip_serializing_if)]` on the optional fields so the
      omit/null behaviour is fixed at compile time.

### Phase 4 — v2 bucket-stats and v2 athlete formatter

- [ ] **18.8-T** A golden test asserts the v2 stats block uses **arrays**
      for `peaks`/`smooth` (each element `{ period, avg }`), includes
      `max` on the `np` sub-block, and omits the deprecated fields, per
      `tests/fixtures/format/v2_stats.json`.
- [ ] **18.8-I** Implement `format_bucket_stats_v2` as a verbatim port of
      `_getBucketStatsV2` (`stats.mjs:2714`).
- [ ] **18.9-T** `/api/athlete/v2/{id}` with no query returns the base v2
      object; with `?resource=...` returns only the requested resources;
      `?stats=true` includes the extended stats; the resource whitelist is
      exactly
      `stats|state|athlete|lap|lastLap|laps|segments|events|timeInPowerZones`.
- [ ] **18.9b-T** `?resource=lastLap` populates a `lastLap` key (not
      `lap`); requesting both `lap` and `lastLap` returns two distinct,
      independent values. This is the fix for the JS bug (decision D1).
- [ ] **18.9-I** Replace the stub `format_athlete_v2` with a port of
      `_formatAthleteDataV2` (`stats.mjs:4325`); fix the `lastLap`
      assignment so it writes to `data.lastLap` (decision D1).

### Phase 5 — Slice formatters and laps/segments/events routes

- [ ] **18.10-T** Golden tests for `_formatDataSlice`,
      `_formatSegmentDataSlice`, `_formatEventDataSlice`, and
      `_filterAthleteDataSlices` (v1 shape).
- [ ] **18.10-I** Implement the three slice formatters and the filter as
      ports of `stats.mjs:1699-1762`.
- [ ] **18.11-T** `/api/athlete/laps/v1/{id}`,
      `/api/athlete/segments/v1/{id}`, `/api/athlete/events/v1/{id}`
      return the filtered, formatted slice arrays; 404 for an unknown id;
      `self`/`watching` aliases resolve.
- [ ] **18.11-I** Register the three routes with `{active: true}`
      filtering, matching `webserver.mjs:323-336,399-401`.

### Phase 6 — Stream formatter and streams route

- [ ] **18.12-T** A golden test asserts the stream object keys
      (`time, power, speed, hr, cadence, draft, active, distance,
      altitude, latlng, wbal`) and the `active` predicate.
- [ ] **18.12-I** Implement `format_athlete_streams` as a port of
      `_getAthleteStreams` (`stats.mjs:1782`).
- [ ] **18.13-T** `/api/athlete/streams/v1/{id}` returns the stream
      object; 404 for an unknown id.
- [ ] **18.13-I** Register the route (`webserver.mjs:402`).

### Phase 7 — Resource-filter parity (corrects the STEP 17 deferral)

- [ ] **18.14-T** A v2 filter test pins the exact whitelist and proves
      unknown resource names are ignored (not errors), matching
      `parseAthleteDataV2Query` (`webserver.mjs:278`).
- [ ] **18.14-I** Make the v2 resource filter operate on the assembled
      formatter output (top-level resource names only — there is no
      nested-path filtering in the reference; see correction C1).

### Phase 8 — v2 query-reduction memoisation

- [ ] **18.15-T** Two subscribers with identical `(source, event, query)`
      cause exactly one formatter call per emission (counting test
      double); two subscribers with different queries cause two.
- [ ] **18.15-I** Extend the delegation key to include the v2 query and
      format once per delegation in the fanout task.
- [ ] **18.16-T** Subscribers with overlapping-but-unequal queries share
      one upstream computation and each receive only their requested
      fields; the chosen merge matches the cost model.
- [ ] **18.16-I** Port `createQueryStrategies`/`computeQueryCost`/
      `createFilterGroups` (`stats.mjs:750-841`) in full (decision D2).

### Phase 9 — Parity ledger and deferral log

- [ ] **18.17** Write `docs/planning/STEP-18-parity-ledger.md`: every JS
      formatter field with status (implemented / null-or-absent-and-why /
      deferred-to-step-NN), so no gap is silently dismissed.

---

## Current stub state (what STEP 17 left)

STEP 17 built the registry, the HTTP/WebSocket server, the routing, the
RPC surface, and the subscription engine, but left the payload shapes as
placeholders. The relevant facts in the tree today:

- `format_athlete` (`src/web/http/mod.rs:77`) returns only `athleteId`,
  `courseId`, `lapCount`, an empty `stats: {}`, an empty `lap: {}`, and
  the `watching`/`self` flags. None of the stats, state, gap, or event
  fields are present.
- `format_athlete_v2` (`src/web/http/mod.rs:98`) emits placeholder values
  (`{}`, `null`, `[]`) per requested resource rather than real data.
- The four routes for laps, segments, events, and streams appear in the
  API directory listing (`src/web/http/mod.rs:55-58`) but are **not**
  registered in `configure_api` (`src/web/http/mod.rs:372`).
- The subscription fanout (`src/web/subs/mod.rs:170`) calls
  `format_athlete` (the v1 stub) and keys delegations by `source/event`
  only, with no notion of a v2 query.
- The `zwift-stats` collectors expose `peaks()`, `np_peaks()`,
  `max_value()`, `primary()`, and `periodized()`, but no `getStats*`-style
  shaping method and no `joules()` accessor.
- `WorldTimer` (`crates/zwift-relay/src/world_timer.rs`) exposes `now()`
  and `server_now()` but no `to_server_time(wt)` / `to_local_time(wt)`.

The data the formatters read is already populated: `AthleteData`
(`crates/zwift-stats/src/athlete.rs`) carries `bucket`, `lap_slices`,
`event_slices`, `segment_slices`, `streams`, `w_bal`,
`time_in_power_zones`, `gap`/`gap_distance`/`is_gap_est`,
`event_subgroup`, `event_privacy`, and the `most_recent_state` snapshot.

---

## Reference map (JavaScript → Rust)

| JavaScript (`sauce4zwift/src/...`) | Lines | Rust target |
|---|---|---|
| `stats.mjs` `_formatAthleteData` | 4388 | `format_athlete` (v1) |
| `stats.mjs` `_formatAthleteDataV2` | 4325 | `format_athlete_v2` |
| `stats.mjs` `_formatState` | 4231 | `format_state` |
| `stats.mjs` `_getEventOrRouteInfo` | 4291 | `event_or_route_info` |
| `stats.mjs` `_applyAthletePrivacyFilter` | 2363 | `apply_athlete_privacy` |
| `stats.mjs` `_getBucketStats` (v1) | 2664 | `format_bucket_stats_v1` |
| `stats.mjs` `_getBucketStatsV2` | 2714 | `format_bucket_stats_v2` |
| `stats.mjs` `getStatsSlow`/`getStatsV2` | 221 / 196 | `DataCollector::stats` + v1/v2 render |
| `stats.mjs` `getNPStatsSlow`/`getNPStatsV2` | 308 / 280 | `PowerDataCollector::np_stats` + render |
| `stats.mjs` `_formatDataSlice` | 1741 | `format_data_slice` |
| `stats.mjs` `_formatSegmentDataSlice` | 1699 | `format_segment_slice` |
| `stats.mjs` `_formatEventDataSlice` | 1719 | `format_event_slice` |
| `stats.mjs` `_filterAthleteDataSlices` | 1726 | `filter_slices` |
| `stats.mjs` `_getAthleteStreams` | 1782 | `format_athlete_streams` |
| `stats.mjs` `ADV2QueryReductionEmitter` | 750 | query-keyed delegation, `src/web/subs/` |
| `webserver.mjs` `parseAthleteDataV2Query` | 278 | `parse_resources` + `stats` flag |
| `webserver.mjs` laps/segments/events/streams routes | 399-402 | `configure_api` |
| `shared/sauce/power.mjs` `joules` | 290 | `RollingAverage::joules` |
| `zwift.mjs` `WorldTimer.toServerTime/toLocalTime` | 104-114 | `WorldTimer::to_*_time` |

Placement (decision D3): the formatters move out of
`src/web/http/mod.rs` into a new `src/web/format.rs` module (item
18.0b), leaving the routing in `http`, so the HTTP routes, the WebSocket
fanout, and the query-reduction engine all call one shared set of
formatter functions. This keeps the byte-parity logic in one auditable
place.

---

## Parity strategy: authored golden fixtures (no JavaScript replay)

The original outline said "compare Rust-formatted JSON bytes against
JS-formatted JSON bytes for a captured trace." That is **not feasible
here**: ranchero cannot drive sauce4zwift against a recorded ride
(session capture is a ranchero-only addition), and no build, test, or
runtime path may resolve through the `sauce4zwift` symlink. So the JS
formatters cannot be executed to produce reference output.

Parity is therefore enforced by **golden JSON fixtures authored by hand
from the JavaScript source**, checked into `tests/fixtures/format/`. The
contract has two halves, both asserted by `assert_json_parity`
(item 18.0):

1. **Key-set parity** — the exact set of keys at every nesting level must
   match the JS formatter. This is the direct guard against the
   `keepCase` hazard (§7.12): a renamed or snake-cased field fails the
   test even when its value happens to match.
2. **Value parity** — for a deterministic input (item 18.0's builder),
   the numeric and structural values match, with a small `f64` tolerance
   on sums and exact match on counts, periods, and zone times.

Each deterministic input is a fixed sample script (a known sequence of
`ingest_power`/`ingest_hr`/… calls at known timestamps, then
`flush_all`), so the golden values are reproducible and reviewable.
Authoring rule: each golden is derived by reading the JS formatter and
computing the expected value from the script — never by running
JavaScript. The end-to-end "point an unmodified widget at the daemon"
check is STEP 19's job (spec §7.11 item 5); STEP 18 owns the field-shape
contract.

---

## Cross-cutting gaps and how each is handled

Several `_formatAthleteData` fields read data ranchero does not yet
compute. The key realisation is that the JavaScript itself emits
`undefined` (omitted) or `null` for these fields when its own source data
is absent — so emitting the same null/absent value is *parity-correct*,
not a shortcut, provided the field key behaves identically. Each gap below
states the handling and the follow-up.

- **G1 — Athlete profile cache.** `_formatAthleteData` sets
  `athlete: this._applyAthletePrivacyFilter(athlete, ad)` from
  `_athletesCache`. ranchero has no profile cache in `WebState` yet (it
  arrives with the athletes DB in STEP 16). Handling: emit `athlete:
  null` (v1) and, for v2, include the `athlete` resource as `null` when
  requested. Parity holds for any athlete absent from the JS cache.
  Follow-up: wire the cache in STEP 16 and revisit.
- **G2 — TSS without FTP.** `tss` is computed only when `athlete?.ftp` is
  known. Without G1 there is no FTP, so `tss` is `undefined` (omitted) —
  matching JS for an athlete with no FTP.
- **G3 — `state` field-name mapping.** v1 always includes
  `state: _formatState(mostRecentState)`, and the widgets read the
  sauce4zwift proto field names. ranchero decodes the zwift-offline
  proto2 tree (different names) and stores a reduced `MostRecentState`.
  Handling: `format_state` emits the widget-facing names for the fields
  ranchero computes (`worldTime`, `speed`, `power`, `heartrate`,
  `cadence`, `draft`, `distance`, `altitude`, `courseId`, `roadId`,
  `roadTime`, `reverse`, `groupId`, `sport`, `time`); fields ranchero does
  not yet derive (`latlng`/`x`/`y`, `eventDistance`, `heading`,
  `roadCompletion`, `progress`) are listed in the parity ledger as
  deferred, not emitted. This is the one gap where ranchero *has* data in
  a different shape, so it needs explicit mapping rather than null.
- **G4 — Event/route info, gameState, eventPosition/Participants,
  userDefined.** `_getEventOrRouteInfo` needs the event-subgroup cache
  (populated by a deferred background fetch) and route distance (not
  computed); `gameState` needs the game-connection state; `eventPosition`,
  `eventParticipants`, and `userDefined` have no producer yet. Handling:
  emit exactly as JS does when its source is absent — the event/route
  spread contributes nothing, and the scalar fields are omitted. Recorded
  in the ledger.
- **G5 — World-time unit reconciliation.** `to_server_time`/
  `to_local_time` operate in milliseconds (`wt + epoch`), but
  `AthleteData::wt_offset` is currently set from `world_time` in
  **seconds** (`ProtoView::world_time` divides by 1000). The timestamp
  fields (`createdServerTime`, peak `ts`, `startServerTime`) must use a
  single consistent unit. Pin the unit in 18.2-I (convert at the formatter
  boundary) and add a regression test for the chosen convention.

These gaps mean STEP 18 reaches full field-shape parity and full value
parity for everything ranchero computes; the residual `null`/absent
fields are parity-consistent and tracked in the ledger (Phase 9).

---

## Phase detail

### Phase 0 — Parity harness and deterministic builder

**18.0.** Add `tests/support/mod.rs` exposing:

- `build_athlete(script: &[Sample]) -> AthleteData` — constructs an
  `AthleteData`, ingests a fixed script, and calls `bucket.flush_all()`
  so single-point ingestion is visible (see the `flush_all` note in
  `data_bucket.rs`).
- `assert_json_parity(actual: &Value, golden: &Value)` — recursively
  asserts identical key sets at every object level and value equality with
  an `f64` tolerance on numbers.

Golden files live in `tests/fixtures/format/`.

### Phase 1 — Stat-shape primitives

**18.1.** `joules()` returns the cumulative value·time accumulator
(`values_acc`), matching `joules()` in `shared/sauce/power.mjs:290`
(`return this._valuesAcc`). Add to `RollingAverage`; expose on
`RollingPower` through `self.rolling().joules()`.

**18.2.** Add to `WorldTimer`: `to_server_time(wt) = wt + ZWIFT_EPOCH_MS`
and `to_local_time(wt) = wt + ZWIFT_EPOCH_MS - offset_ms`
(`zwift.mjs:104-114`, where the JS `_epoch` equals `ZWIFT_EPOCH_MS`).
Resolve gap G5 here.

**18.3 / 18.4.** Add the shaping PODs to
`crates/zwift-stats/src/collector.rs`:

```rust
pub struct PeakStat   { pub period: f64, pub avg: Option<f64>, pub time: Option<f64>, pub ts: Option<f64> }
pub struct SmoothStat { pub period: f64, pub avg: Option<f64> }
pub struct SignalStats { pub avg: Option<f64>, pub max: f64, pub peaks: Vec<PeakStat>, pub smooth: Vec<SmoothStat> }
pub struct NpStats     { pub avg: Option<f64>, pub max: f64, pub peaks: Vec<PeakStat>, pub smooth: Vec<SmoothStat> }
```

- `DataCollector::stats(ts_offset_ms)` walks `periodized`, reading each
  window's peak snapshot (`peak.snap_value` → `avg`, `peak.snap_time` →
  `time`, `ts = ts_offset_ms + time * 1000`) and the live `roll.avg()` for
  `smooth`. `smooth` is emitted only for windows with period ≤ 1200 s
  (`maxSmoothPeriod`, `stats.mjs:16`).
- `PowerDataCollector::np_stats(ts_offset_ms)` does the same over the NP
  peaks, restricted to windows with period ≥ 300 s
  (`minWeightedPowerPeriod`, `stats.mjs:17`); its `smooth` reads
  `roll.np()`.

The container/keying difference between v1 and v2 is **not** encoded in
these PODs — they return ordered `Vec`s; the formatter renders them as the
v1 period-keyed object or the v2 array.

Concrete window layout for the standard configuration (from
`data_bucket.rs`):

| Signal | periods (s) | peaks | smooth (period ≤ 1200) |
|---|---|---|---|
| power | 5,15,60,300,1200,3600 | all 6 | 5,15,60,300,1200 |
| power NP | (same) | 300,1200,3600 | 300,1200 |
| hr/speed/draft | 60,300,1200,3600 | all 4 | 60,300,1200 |
| cadence | (none) | none | none |

### Phase 2 — v1 bucket stats

**18.5.** `format_bucket_stats_v1(bucket, ad, ctx)` ports `_getBucketStats`
(`stats.mjs:2664`). Shape points to capture in the golden:

- `peaks`/`smooth` are **objects keyed by the period string**
  (`getStatsSlow`, `stats.mjs:221`): `peaks: { "5": {...}, "15": {...} }`.
- v1 `smooth` values are **bare numbers** keyed by period
  (`smooth[period] = roll.avg()`), not `{period, avg}` objects.
- The `np` sub-block uses `getNPStatsSlow` and has **no `max`**.
- `power` carries the deprecated `wBal`, `timeInZones`, plus `np`, `tss`,
  `kj`; the top-level block carries deprecated `wBal`, `timeInPowerZones`;
  `coffeeTime`/`workTime`/`followTime`/`soloTime` are `round(ms / 1000)`;
  `draft` carries its own `kj`.
- `ctx` supplies `now`, `athlete` (for FTP/TSS — `null` per G1), and the
  `includeDeprecated` flag (true for v1).

### Phase 3 — v1 athlete formatter

**18.6 / 18.7.** Port `_formatAthleteData` (`stats.mjs:4388`) field by
field into `format_athlete`, replacing the stub. Build it from named
`#[derive(Serialize)]` structs with `#[serde(skip_serializing_if =
"Option::is_none")]` on the conditionally-present fields so the JS
`undefined`-omission and `null` semantics are exact:

- `createdServerTime` = `to_server_time(ad.wt_offset)`; `created`,
  `updated` from `ad`; `age` = `now - ad.internal_updated`.
- `watching`/`self` present only when true; `isGapEst` present only when
  true.
- `athlete` via `apply_athlete_privacy` (G1 → `null`).
- `stats` = `format_bucket_stats_v1(bucket)`; `lap` = last lap slice;
  `lastLap` = previous lap slice or `null`; `lapCount`.
- `state` = `format_state(most_recent_state)` or `null` (G3).
- `gap`, `gapDistance`; `wBal` (omitted when `hide_w_bal`);
  `timeInPowerZones` (omitted when `hide_ftp`).
- spread of `event_or_route_info(state)` (G4 → empty when no source);
  `eventSubgroupId`, `eventPosition`, `eventParticipants`, `gameState`
  per G4.
- The existing `tests/http_athlete_v1.rs` assertions stay valid (they
  check a subset); add the golden assertion for the full shape.

### Phase 4 — v2 bucket stats and v2 athlete formatter

**18.8.** `format_bucket_stats_v2` ports `_getBucketStatsV2`
(`stats.mjs:2714`): `peaks`/`smooth` are **arrays** (`getStatsV2`), each
`smooth` element is `{period, avg}`, the `np` sub-block **has `max`**, and
none of the deprecated fields appear.

**18.9.** `format_athlete_v2` ports `_formatAthleteDataV2`
(`stats.mjs:4325`):

- Base object always present: `version: 2`, `createdServerTime`,
  `created`, `updated`, `age`, `self`, `watching`, `courseId`,
  `athleteId`, `lapCount`, `eventSubgroupId`, `eventPosition`,
  `eventParticipants`, `gameState`, `gap`, `gapDistance`, `isGapEst`,
  `wBal`, the event/route spread, and `userDefined`.
- When `resources` is empty, the JS returns the base object only (no v1
  `stats`/`lap`); when present, it adds exactly the requested resources:
  `athlete`, `state`, `timeInPowerZones`, `stats`, `lap`, `lastLap`,
  `laps`, `segments`, `events`.
- Decision D1 (fix, not verbatim): the JS writes the `lastLap` value into
  `data.lap` (`stats.mjs:4376`), overwriting `lap` — a bug. ranchero
  deviates: the `lastLap` resource writes to `data.lastLap`, leaving `lap`
  intact, so requesting both yields two independent values. Mark the
  deviation with a comment citing the JS line. This is the one
  intentional departure from verbatim parity in this step; record it in
  the parity ledger.

### Phase 5 — Slice formatters and routes

**18.10.** Port the three slice formatters and `_filterAthleteDataSlices`:

- `format_data_slice` (`stats.mjs:1741`): `id`, `stats` (v1 bucket stats
  when no `version`, else v2 or `null` per the `stats` flag), `active` =
  `slice.end is None`, `startIndex`/`endIndex` from the power-roll
  offsets, `startServerTime` = `to_server_time(ad.wt_offset) +
  (slice.start - ad.internal_created)`, `start`/`end` from the roll times,
  `sport`, `courseId`.
- `format_segment_slice` adds `segmentId`, `eventSubgroupId`,
  `startEventDistance`, `endEventDistance`, `incomplete`.
- `format_event_slice` adds `eventSubgroupId`.
- `filter_slices(startTime, endTime, active)` (`stats.mjs:1726`): when
  `active` is true, keep open slices; otherwise drop them.

**18.11.** Register `/api/athlete/laps/v1/{id}`,
`/api/athlete/segments/v1/{id}`, `/api/athlete/events/v1/{id}` in
`configure_api`, each calling the slice formatters with `{active: true}`
(matching `webserver.mjs:399-401`). Resolve `self`/`watching` via
`resolve_athlete_id`; 404 on unknown id.

### Phase 6 — Stream formatter and route

**18.12.** `format_athlete_streams` ports `_getAthleteStreams`
(`stats.mjs:1782`). Output keys, in order: `time`, `power`, `speed`, `hr`,
`cadence`, `draft` (from each collector's roll `times()`/`values()`,
mapped through `Sample::as_f64`), then `active`, then the
`AthleteData::streams` keys `distance`, `altitude`, `latlng`, `wbal`. The
`active` predicate (`!!+x || !(x instanceof Pad)`) maps to:
`Value(_) => true`, `Pad(v) => v != 0.0`, `Break => false`.

**18.13.** Register `/api/athlete/streams/v1/{id}` (`webserver.mjs:402`);
404 on unknown id. No version/query handling — the JS handler passes no
options.

### Phase 7 — Resource-filter parity

**Correction C1.** The STEP 17 deferral claimed the query "supports
nested paths such as `resource=lap.distance`." It does not. The reference
(`parseAthleteDataV2Query`, `webserver.mjs:278`, and
`_formatAthleteDataV2`, `stats.mjs:4325`) filters by **top-level resource
name only**, against the fixed whitelist
`stats|state|athlete|lap|lastLap|laps|segments|events|timeInPowerZones`.
Building a nested-path filter would add behaviour the widgets never call.

**18.14.** The real parity work is: (a) the exact whitelist, (b) unknown
names ignored rather than rejected, and (c) the correct per-resource shape
— produced by Phase 4. The "depth" that genuinely exists is that
`laps`/`segments`/`events` are arrays of slices whose per-slice `stats` is
included only when `stats=true`; that behaviour is exercised here against
the goldens.

### Phase 8 — v2 query-reduction memoisation

**18.15 (required).** Today the delegation key is `source/event`
(`src/web/subs/mod.rs:98`). Extend it to `source/event/query`, where
`query` is the canonical encoding of `(resources, stats)`. Subscribers
with identical queries then share one `DelegationHandle`; the fanout task
formats once with the v2 formatter and clones the `Value` to each sink
(the clone path already exists). The test uses a counting formatter double
to assert one serialization per emission for identical queries and two for
differing queries — the contract from the original outline.

**18.16 (decision D2 — implement in full).** Port
`ADV2QueryReductionEmitter` (`stats.mjs:750`) completely: it merges
overlapping-but-unequal queries into the cheapest single upstream
computation (`computeQueryCost`, `createQueryStrategies`) and then masks
each listener's payload down to its own query (`createFilterGroups`).
Structure the port as three units mirroring the JS, each test-driven:

- `compute_query_cost(query)` — the resource→cost table with the ×5
  stats multiplier (`stats.mjs:780-803`); unit test pins the costs.
- `create_query_strategies(listeners)` — the split-vs-combined candidate
  set (`stats.mjs:752-778`); unit test on a known listener set.
- `create_filter_groups(batch)` — per-listener masking, including the
  `laps`/`segments`/`events` stats-mask when a non-stats listener rides a
  stats batch (`stats.mjs:805-840`); unit test verifies each listener
  receives only its requested fields.

The emitter then chooses the lowest-cost strategy per emission, formats
once per batch, and applies each group's filter before fan-out. 18.15's
identical-query memoisation is the degenerate case of this machinery.

### Phase 9 — Parity ledger

**18.17.** `docs/planning/STEP-18-parity-ledger.md` lists every field of
each JS formatter with one of: implemented; emitted null/absent because
the JS does the same when its source is absent (G1, G2, G4);
deferred-with-mapping (G3 state fields); or deferred to a named later
step. Also record the one intentional deviation from verbatim parity: the
`lastLap` fix (D1). This satisfies the rule that nothing is silently
dismissed.

---

## Acceptance criteria

- `cargo test` (fast set) and `cargo test -- --include-ignored` pass.
- Every golden in `tests/fixtures/format/` matches both the key-set and
  value checks for its deterministic input.
- `/api/athlete/v1/{id}`, `/api/athlete/v2/{id}`, `/api/nearby/v1`,
  `/api/nearby/v2`, `/api/groups/v1`, `/api/groups/v2`, and the four new
  laps/segments/events/streams routes return the formatter output, with
  404 for unknown ids and alias resolution for `self`/`watching`.
- Two WebSocket subscribers with identical v2 queries cause one
  serialization per emission.
- The parity ledger (Phase 9) accounts for every JS formatter field.

---

## Resolved decisions

- **D1 — `lastLap` quirk: fix.** Deviate from the JS bug
  (`stats.mjs:4376`) — the `lastLap` resource writes to `data.lastLap`,
  not `data.lap`. This is the one intentional departure from verbatim
  parity; covered by test 18.9b and recorded in the parity ledger.
- **D2 — Query-reduction depth: implement in full.** Port the complete
  `ADV2QueryReductionEmitter` (cost model, strategy selection, filter
  groups) in Phase 8 (18.16), not just identical-query memoisation.
- **D3 — Formatter module location: extract.** Move the formatters into a
  new `src/web/format.rs` (item 18.0b); routing stays in `http`.

## Out of scope (carried to later steps)

- Athlete profile cache and FTP-dependent values (STEP 16).
- Full `state` field set beyond what ranchero computes (G3 deferred
  fields).
- End-to-end widget rendering against the daemon (STEP 19, spec §7.11
  item 5).
- `mods/v1` payloads and the mods source.

## Deferred-from-STEP-17 items — disposition

The three items STEP 17 deferred here are now folded into the phases
above:

- **Deep resource-filter parity for v2 endpoints** → Phase 7. Note the
  correction C1: the reference has no nested-path (`lap.distance`) filter;
  the genuine work is the exact whitelist plus the formatter-defined
  per-resource shapes.
- **`GET /api/athlete/streams/v1/:id`** → Phase 6 (route 18.13) on top of
  the stream formatter (18.12).
- **`GET /api/athlete/laps/v1/:id`, `/segments/v1/:id`, `/events/v1/:id`**
  → Phase 5 (routes 18.11) on top of the slice formatters (18.10). The
  STEP 17 note's shorthand (`/api/athlete/laps`) was imprecise; the real
  routes carry the `/v1/:id` suffix, as the API directory already lists.
