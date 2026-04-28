# Step 14 — Per-athlete state, DataBucket, DataCollector (stub)

## Goal

Port the `AthleteData` record (spec §5.2) and `DataBucket` /
`DataCollector` (spec §5.3) into `zwift-stats`:

- One `DataCollector` per signal (power, hr, speed, cadence, draft).
- Each `DataCollector` holds a primary rolling + cloned rolling per
  peak period: power `[5, 15, 60, 300, 1200, 3600]`, others `[60, 300,
  1200, 3600]`.
- GC: drop `AthleteData` after 1 h unseen; groups after 90 s (spec §5.2
  / §9 runtime knobs).

## Tests-first outline

- Feed a recorded `PlayerState` stream, verify each signal's avg / max /
  peaks match the JS reference.
- GC ticks at 10 s and evicts correctly.

To be fully elaborated when work on this step begins.
