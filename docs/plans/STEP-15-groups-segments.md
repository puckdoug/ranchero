# Step 15 — Groups, laps, segments, W' balance, zones (stub)

## Goal

Higher-level stats on top of STEP 14:

- `ZonesAccumulator` for power Z1..Z7 seconds, plus HR zones.
- `WBalAccumulator` — CP + W' model, streams `wbal` samples.
- Event detection via `state.eventSubgroupId` (trigger start/end, apply
  privacy flags).
- Lap detection — manual + automatic by distance/time + route-specific
  weld tables from `shared/routes.mjs`.
- Segment detection — `Env.getRoadSegments(courseId, roadId, reverse)` +
  road-history walk.
- `_computeGroups` (spec §5.5): greedy-Jaccard clustering by gap, 2 s
  threshold (0.8 s without draft).

## Inputs deferred from STEP 13

The remaining `power.mjs` content that the live-data core actually
calls lands here:

- **Power / HR zone definitions.** Port `cogganZones`,
  `polarizedZones`, and `sweetspotZone` (`shared/sauce/power.mjs:856-893`),
  consumed by `src/stats.mjs:1225-1241`. The `ZonesAccumulator` then
  walks a `RollingAverage` (the primary roll from STEP 14's
  power `DataCollector`) and credits each `idealGap` window to the
  matching zone.
- **W'-balance accumulator.** Port `makeIncWPrimeBalDifferential`
  (`shared/sauce/power.mjs:804-826`) — the differential
  Froncioni / Skiba / Clarke algorithm — consumed by
  `src/stats.mjs:382`. The accumulator wraps `RollingPower` (STEP 13)
  with the CP / W' state and emits a `wbal` sample per ingestion
  tick. The integral form (`calcWPrimeBalIntegralStatic`) and the
  one-shot stream form (`calcWPrimeBalDifferential`) are not
  consumed by the live-data core; bring them across only if a
  segment / lap closer needs the static integral.

## Inputs deferred from STEP 14

STEP 14 ships only the identity, lifecycle-timestamp, and bucket
subset of `AthleteData` (spec §5.2). The remaining fields and the
slice machinery are this step's responsibility because they all
depend on the higher-level accumulators or detection logic this step
introduces. Each item below is a field on `AthleteData` in the JS
reference (`stats.mjs:2817-2900`); STEP 14's `AthleteData` struct
carries a `// STEP 15:` comment block enumerating the same list as a
forward reference.

- **`wBal: WBalAccumulator`** — driven by the W'-balance accumulator
  this step introduces (see "Inputs deferred from STEP 13" above).
  Emits `wbal` samples that land in `streams.wbal`.
- **`timeInPowerZones: ZonesAccumulator`** — Z1..Z7 seconds; driven
  by the power-zone definitions ported above. The HR-zone analogue
  is parallel.
- **`smoothGrade: expWeightedAvg(8)`** — exponentially weighted
  grade accumulator. The free function lives in `shared/sauce/data.mjs`
  alongside `RollingAverage`; port it into `zwift-stats::helpers`
  (or `zwift-stats::accumulators`) when STEP 15 needs it.
- **`streams: { distance[], altitude[], latlng[], wbal[] }`** —
  unbounded time-series append buffers (one entry per ingestion
  tick). Consumed by the published metrics in spec §5.4 (Elevation,
  Position) and by the post-session slice machinery.
- **`roadHistory: { aRoad, bRoad, cRoad, a[], b, c }`** — three-tier
  sliding window of road segments, consumed by `_activeSegmentCheck`.
  This is the lookup STEP 15's segment detection walks.
- **Slice machinery — `DataSlice` and the four slice containers.**
  STEP 14 ships `DataBucket::clone_reset` and `clone_continue` so
  the seam exists; STEP 15 introduces the `DataSlice` struct itself
  (snapshot of a bucket plus its identity / start / end) and the
  four containers on `AthleteData`:
  - `lapSlices: Vec<DataSlice>` — closed laps; populated by manual
    `startAthleteLap` and automatic `_autoLapCheck`.
  - `eventSlices: Vec<DataSlice>` — closed events; populated by
    `triggerEventStart` / `triggerEventEnd`.
  - `segmentSlices: Vec<DataSlice>` — closed segments; populated
    by the `_activeSegmentCheck` exit branch.
  - `activeSegments: HashMap<SegmentId, DataSlice>` — segments
    currently being recorded (snapshots taken at segment start,
    materialised as a closed `DataSlice` on exit).
- **`mostRecentState` enrichment.** STEP 14 ships a minimal
  `MostRecentState` with the fields the parity tests need
  (`world_time`, `speed`, `power`, `heartrate`, `cadence`, `draft`,
  `distance`, `altitude`). STEP 15's group / segment / lap logic
  needs more (`roadId`, `roadTime`, `reverse`, `eventSubgroupId`,
  `groupId`); extend the struct as those fields are reached for.
  The "make it a proto-type alias" question is recorded against
  STEP 17 (daemon glue).
- **Gap fields — `gap`, `gapDistance`, `isGapEst`.** Computed by
  `compareRoadPositions` against the watched athlete; produced as a
  side-effect of group classification. Add as plain `f64` /
  `Option<f64>` / `bool` fields on `AthleteData` when group
  classification lands.
- **Event / privacy fields — `groupId: Option<u32>`,
  `eventSubgroup: Option<EventSubgroup>`, `eventPrivacy:
  EventPrivacy`, `disabledByEvent: bool`.** Set by event detection
  (`triggerEventStart` applies `hidewbal` / `hideFTP` / `hidethehud`
  privacy flags). The exact shape of `EventSubgroup` follows the
  `ZwiftAPI.getEventSubgroup()` response and is a STEP 15 detail.
- **`GroupMeta::identity_set: HashSet<u32>`.** STEP 14's GC seam
  only carries `{ id, accessed }`. The greedy-Jaccard clustering
  (`_computeGroups`, spec §5.5) needs the per-group athlete set so
  it can compute set-difference between successive ticks; STEP 15
  adds the field and the populator (`stats.mjs:4513-4581`).

## Out of scope for ranchero v1

The remaining `power.mjs` exports are pure functions used **only**
by sauce4zwift's `pages/src/analysis.mjs` post-ride analysis page.
Spec §7.1 excludes "GUI, Electron widgets, hotkeys, macOS window
control" from v1, so these are not ported here. They are listed by
name so a future reader following the breadcrumb from STEP 13 lands
on the canonical answer:

| Function(s) | JS source | Live-data callsite | Decision |
|---|---|---|---|
| `rank`, `rankLevel`, `rankBadge`, `rankRequirements` | `power.mjs:90-150` | none — used by `pages/src/analysis.mjs:152, :1262` | Out of scope (UI). |
| `calcPwHrDecoupling`, `calcPwHrDecouplingFromRoll` | `power.mjs:829-853` | none — used by `pages/src/analysis.mjs:215` | Out of scope (UI). |
| `cyclingPowerEstimate`, `cyclingPowerVelocitySearch`, `cyclingPowerFastestVelocitySearch`, `cyclingPowerVelocitySearchMultiPosition` | `power.mjs:512-728` | none in `src/stats.mjs` | Out of scope (analysis modelling). |
| `cyclingDraftDragReduction` | `power.mjs:531-568` | none in `src/stats.mjs` | Out of scope (analysis modelling). |
| `seaLevelPower` | `power.mjs:467-480` | none in `src/stats.mjs` | Out of scope (analysis modelling). |
| `calcWPrimeBalIntegralStatic`, `calcWPrimeBalDifferential` | `power.mjs:742-801` | none — `stats.mjs` uses the incremental form | Defer; revisit if segment/lap closing needs the static integral. |

These functions are pure and self-contained, so a future
`zwift-stats::analysis` module can pick them up unchanged. None of
them are blockers for the live-data core.

## Tests-first outline

- W' balance: same CP + W' inputs as a JS reference trace → agreement
  to ≤ 1e-6 per sample.
- Group clustering: synthetic nearby-rider tables → identical group
  assignments to JS reference.
- Segment start/stop: hand-built road history → correct entries in
  `activeSegments` + `segmentSlices`.

To be fully elaborated when work on this step begins.
