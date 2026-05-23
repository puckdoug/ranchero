# STEP 18 — Parity Ledger

Every field emitted by the JavaScript formatters in `sauce4zwift/src/stats.mjs`
and `sauce4zwift/src/webserver.mjs`, with its implementation status in the
Rust port.

Status codes used in the tables:

| Code | Meaning |
|------|---------|
| ✅ | Implemented — field present, correct name and value |
| ⚠️ null | Field always present but currently `null`; why is noted |
| ⚠️ absent | Field omitted; why is noted |
| 🔲 deferred | Field not yet wired; target step noted |

Gap labels (`G1`–`G5`) refer to the cross-cutting gap entries in
`docs/plans/STEP-18-format-payloads.md`.

---

## `_formatAthleteData` — v1 athlete record (`stats.mjs:4388`)

Rust: `format_athlete_data_v1` in `src/web/format.rs`.

| JS field | Status | Notes |
|----------|--------|-------|
| `createdServerTime` | ✅ | `wt_offset as i64 + ZWIFT_EPOCH_MS` |
| `created` | ✅ | local clock |
| `updated` | ✅ | local clock |
| `age` | ✅ | `now - internal_updated` |
| `watching` | ✅ | omitted when false |
| `self` | ✅ | omitted when false |
| `courseId` | ✅ | |
| `athleteId` | ✅ | |
| `athlete` | ⚠️ null | G1: no athlete-profile cache yet |
| `stats` | ✅ | v1 shape with deprecated fields |
| `lap` | ✅ | last open lap slice |
| `lastLap` | ✅ | null when `lapCount == 1` |
| `lapCount` | ✅ | |
| `state` | ✅ | null when no state received |
| `eventSubgroupId` | ✅ | omitted when absent |
| `eventPosition` | ✅ | omitted when absent |
| `eventParticipants` | ✅ | omitted when absent |
| `gameState` | ⚠️ absent | G4: only for `self` with a live game session |
| `gap` | ✅ | omitted when absent |
| `gapDistance` | ✅ | omitted when absent |
| `isGapEst` | ✅ | omitted when false |
| `wBal` | ✅ | omitted when `hideWBal` privacy flag set |
| `timeInPowerZones` | ✅ | omitted when `hideFTP` privacy flag set |
| `eventLeader` | ⚠️ absent | G4: requires `_recentEventSubgroups` cache |
| `eventSweeper` | ⚠️ absent | G4: requires `_recentEventSubgroups` cache |
| `remaining` | ⚠️ absent | G4: requires event-subgroup cache or route metadata |
| `remainingMetric` | ⚠️ absent | G4: as above |
| `remainingType` | ⚠️ absent | G4: as above |
| `remainingEnd` | ⚠️ absent | G4: as above |
| `...ad.userDefined` | ⚠️ absent | G4: no `userDefined` producer in ranchero yet |

---

## `_formatAthleteDataV2` — v2 athlete record (`stats.mjs:4327`)

Rust: `format_athlete_v2` in `src/web/format.rs`.

### Base object (always present)

| JS field | Status | Notes |
|----------|--------|-------|
| `version` | ✅ | fixed `2` |
| `createdServerTime` | ✅ | |
| `created` | ✅ | |
| `updated` | ✅ | |
| `age` | ✅ | |
| `self` | ✅ | omitted when false |
| `watching` | ✅ | omitted when false |
| `courseId` | ✅ | |
| `athleteId` | ✅ | |
| `lapCount` | ✅ | |
| `eventSubgroupId` | ✅ | omitted when absent |
| `eventPosition` | ✅ | omitted when absent |
| `eventParticipants` | ✅ | omitted when absent |
| `gameState` | ⚠️ absent | G4: self-only, requires live session |
| `gap` | ✅ | omitted when absent |
| `gapDistance` | ✅ | omitted when absent |
| `isGapEst` | ✅ | omitted when false |
| `wBal` | ✅ | omitted when `hideWBal` set |
| `eventLeader` | ⚠️ absent | G4 |
| `eventSweeper` | ⚠️ absent | G4 |
| `remaining` | ⚠️ absent | G4 |
| `remainingMetric` | ⚠️ absent | G4 |
| `remainingType` | ⚠️ absent | G4 |
| `remainingEnd` | ⚠️ absent | G4 |
| `...ad.userDefined` | ⚠️ absent | G4 |

### Resource fields (present only when requested)

| JS resource / field | Status | Notes |
|---------------------|--------|-------|
| `athlete` | ⚠️ null | G1: no profile cache |
| `state` | ✅ | null when no state; `_formatState` shape |
| `timeInPowerZones` | ✅ | omitted when `hideFTP` set |
| `stats` | ✅ | v2 bucket-stats shape |
| `lap` | ✅ | v2 bucket-stats for current lap |
| `lastLap` | ✅ | D1 deviation: key is `lastLap`, not `lap` |
| `laps` | ✅ | array of v2 slice objects |
| `segments` | ✅ | array of v2 segment-slice objects |
| `events` | ✅ | array of v2 event-slice objects |

---

## `_getBucketStats` — v1 bucket stats (`stats.mjs:2664`)

Rust: `format_bucket_stats_v1` in `src/web/format.rs`.
The `includeDeprecated` flag is `true` for the main `stats` block and `false`
for `lap`/`lastLap`.

| JS field | Status | Notes |
|----------|--------|-------|
| `elapsedTime` | ✅ | |
| `activeTime` | ✅ | |
| `coffeeTime` | ✅ | rounded to seconds |
| `workTime` | ✅ | rounded to seconds |
| `followTime` | ✅ | rounded to seconds |
| `soloTime` | ✅ | rounded to seconds |
| `workKj` | ✅ | |
| `followKj` | ✅ | |
| `soloKj` | ✅ | |
| `wBal` (deprecated) | ✅ | only when `includeDeprecated`; omitted when `hideWBal` set |
| `timeInPowerZones` (deprecated) | ✅ | only when `includeDeprecated`; empty array when `hideFTP` |
| `power.avg` | ✅ | |
| `power.max` | ✅ | |
| `power.peaks` | ✅ | period-keyed object (`{"5": {...}, "15": {...}, ...}`) |
| `power.smooth` | ✅ | period-keyed object of bare numbers |
| `power.np` | ✅ | |
| `power.tss` | ✅ | null when no FTP (G2) |
| `power.kj` | ✅ | |
| `power.wBal` (deprecated) | ✅ | only when `includeDeprecated` |
| `power.timeInZones` (deprecated) | ✅ | only when `includeDeprecated` |
| `np.avg` | ✅ | |
| `np.peaks` | ✅ | period-keyed object, periods ≥ 300 s only |
| `np.smooth` | ✅ | period-keyed object, periods ≥ 300 s and ≤ 1200 s |
| (no `np.max` in v1) | ✅ | correctly absent |
| `speed` | ✅ | full `getStatsSlow` shape |
| `hr` | ✅ | full `getStatsSlow` shape |
| `cadence` | ✅ | full `getStatsSlow` shape |
| `draft.avg/max/peaks/smooth` | ✅ | |
| `draft.kj` | ✅ | |

---

## `_getBucketStatsV2` — v2 bucket stats (`stats.mjs:2714`)

Rust: `format_bucket_stats_v2` in `src/web/format.rs`.

| JS field | Status | Notes |
|----------|--------|-------|
| `elapsedTime` | ✅ | |
| `activeTime` | ✅ | |
| `coffeeTime` | ✅ | |
| `workTime` | ✅ | |
| `followTime` | ✅ | |
| `soloTime` | ✅ | |
| `workKj` | ✅ | |
| `followKj` | ✅ | |
| `soloKj` | ✅ | |
| `power.avg/max` | ✅ | |
| `power.peaks` | ✅ | array of `{period, avg, time, ts}` |
| `power.smooth` | ✅ | array of `{period, avg}`, periods ≤ 1200 s |
| `power.np` | ✅ | |
| `power.tss` | ✅ | null when no FTP (G2) |
| `power.kj` | ✅ | |
| `np.avg` | ✅ | |
| `np.max` | ✅ | sourced from `power.stats().max` |
| `np.peaks` | ✅ | array, periods ≥ 300 s |
| `np.smooth` | ✅ | array, periods ≥ 300 s and ≤ 1200 s |
| `speed` | ✅ | v2 array shape |
| `hr` | ✅ | v2 array shape |
| `cadence` | ✅ | v2 array shape |
| `draft.avg/max/peaks/smooth` | ✅ | v2 array shape |
| `draft.kj` | ✅ | |

---

## `_formatState` — state snapshot (`stats.mjs:4231`)

Rust: `format_state` in `src/web/format.rs`.

The JS implementation spreads the full raw state object and sets private
(underscore-prefixed) fields to `undefined`.  The public fields that remain
depend on which computed fields sauce4zwift's processing pipeline has populated
on the state record.

| JS field | Status | Notes |
|----------|--------|-------|
| `worldTime` | ✅ | |
| `speed` | ✅ | |
| `power` | ✅ | |
| `heartrate` | ✅ | |
| `cadence` | ✅ | |
| `draft` | ✅ | |
| `distance` | ✅ | |
| `altitude` | ✅ | |
| `courseId` | ✅ | |
| `roadId` | ✅ | |
| `roadTime` | ✅ | |
| `reverse` | ✅ | |
| `eventSubgroupId` | ✅ | |
| `groupId` | ✅ | |
| `time` | ✅ | local relay time |
| `eventDistance` | ✅ | |
| `latlng` | ✅ | 19.7: repacked from separate `lat`/`lng` scalars into `[lat, lng]` array matching sauce4zwift |
| `lat` | ⚠️ absent | removed in 19.7 — superseded by `latlng` array |
| `lng` | ⚠️ absent | removed in 19.7 — superseded by `latlng` array |
| `x` | ⚠️ absent | G3: Mercator projection; not computed by ranchero |
| `y` | ⚠️ absent | G3: Mercator projection; not computed by ranchero |
| `roadCompletion` | ⚠️ absent | G3: ratio of roadTime to total road length; not computed |
| `progress` | ⚠️ absent | G3: route completion ratio; not computed |

**Resolved in 19.7:** `format_state` now emits `latlng: [lat, lng]` matching
sauce4zwift.  The separate `lat`/`lng` scalar fields have been dropped.  The
underlying world-coordinate pipeline (`x`/`y`/`roadCompletion`/`progress`)
remains deferred to STEP 20 §20.19/§20.20.

---

## `_formatDataSlice` — data slice, v1 shape (`stats.mjs:1741`)

Rust: `format_data_slice` in `src/web/format.rs`.

| JS field | Status | Notes |
|----------|--------|-------|
| `id` | ✅ | |
| `stats` | ✅ | v1 bucket-stats shape (no deprecated fields) |
| `active` | ✅ | true when slice is open (`end == None`) |
| `startIndex` | ✅ | always 0 (unbounded primary roll) |
| `endIndex` | ✅ | inclusive last index |
| `startServerTime` | ✅ | |
| `start` | ✅ | first power-roll time; `null` when empty |
| `end` | ✅ | last power-roll time; `null` when empty or open |
| `sport` | ✅ | |
| `courseId` | ✅ | |

---

## `_formatSegmentDataSlice` — segment slice, v1 shape (`stats.mjs:1699`)

Rust: `format_segment_slice` in `src/web/format.rs`.  Extends `_formatDataSlice`.

| JS field | Status | Notes |
|----------|--------|-------|
| *(all `_formatDataSlice` fields)* | ✅ | |
| `segmentId` | ✅ | |
| `eventSubgroupId` | ✅ | |
| `startEventDistance` | ✅ | |
| `endEventDistance` | ✅ | |
| `incomplete` | ✅ | |

---

## `_formatEventDataSlice` — event slice, v1 shape (`stats.mjs:1719`)

Rust: `format_event_slice` in `src/web/format.rs`.  Extends `_formatDataSlice`.

| JS field | Status | Notes |
|----------|--------|-------|
| *(all `_formatDataSlice` fields)* | ✅ | |
| `eventSubgroupId` | ✅ | |

---

## v2 slice shapes

Rust: `format_data_slice_v2`, `format_segment_slice_v2`, `format_event_slice_v2`
in `src/web/format.rs`.  Same fields as v1 counterparts; the only difference is
the `stats` field — v2 shape is a `_getBucketStatsV2` object when `stats=true`,
or `null` when `stats=false`.

| JS field | Status | Notes |
|----------|--------|-------|
| *(all `_formatDataSlice` fields)* | ✅ | |
| `stats` | ✅ | v2 stats object or `null` depending on `?stats=true` |
| *(segment extras)* | ✅ | same as v1 segment |
| *(event extras)* | ✅ | same as v1 event |

---

## `_getAthleteStreams` — rolling time-series streams (`stats.mjs:1782`)

Rust: `format_athlete_streams` in `src/web/format.rs`.

| JS field | Status | Notes |
|----------|--------|-------|
| `time` | ✅ | from primary power roll |
| `power` | ✅ | |
| `speed` | ✅ | |
| `hr` | ✅ | |
| `cadence` | ✅ | |
| `draft` | ✅ | |
| `active` | ✅ | `Value → true`, `Pad(0) → false`, `Pad(v≠0) → true`, `Break → false` |
| `distance` | ✅ | from `ad.streams.distance` |
| `altitude` | ✅ | from `ad.streams.altitude` |
| `latlng` | ✅ | serialised as `[[lat, lng], ...]` pairs |
| `wbal` | ✅ | from `ad.streams.wbal` |

---

## Deferred gaps summary

| Gap | Description | Affects | Target |
|-----|-------------|---------|--------|
| G1 | Athlete profile cache absent | `athlete` field null everywhere | STEP 16 / post-18 |
| G2 | No FTP without profile (G1) | `tss` null everywhere | STEP 16 / post-18 |
| G3 | World-coordinate pipeline not computed | `state.x/y`, `state.roadCompletion`, `state.progress` absent; `state.latlng` array resolved in 19.7 | STEP 20 §20.19/§20.20 |
| G4 | Event/route metadata, game session | `gameState`, `eventLeader/Sweeper`, `remaining*` absent; `userDefined` absent | post-18 |
| G5 | World-time unit (seconds vs ms) | resolved in 18.2-I | closed |

---

## Decisions recorded

| ID | Decision | Rationale |
|----|----------|-----------|
| D1 | `lastLap` resource writes to `data.lastLap`, not `data.lap` | JS bug at `stats.mjs:4376` overwrites the `lap` key; ranchero fixes this |
| D2 | Query-reduction engine ported (`computeQueryCost`, `createQueryStrategies`, `createFilterGroups`) | Full parity with `ADV2QueryReductionEmitter`; enables cost-based strategy selection |
| D3 | Formatters live in `src/web/format.rs`, not `src/web/http/mod.rs` | Shared by HTTP routes, WebSocket fanout, and query-reduction engine |
| D4 | Gap #2 (v2 WebSocket fanout) confirmed closed in 19.8 | `stats_fanout_task_v2` routes `athlete/watching/v2` correctly; resource filtering verified end-to-end |
