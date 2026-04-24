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

## Tests-first outline

- W' balance: same CP + W' inputs as a JS reference trace → agreement
  to ≤ 1e-6 per sample.
- Group clustering: synthetic nearby-rider tables → identical group
  assignments to JS reference.
- Segment start/stop: hand-built road history → correct entries in
  `activeSegments` + `segmentSlices`.

To be fully elaborated when we start work on this step.
