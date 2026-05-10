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
