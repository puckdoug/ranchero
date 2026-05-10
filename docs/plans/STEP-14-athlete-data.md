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

## Inputs deferred from STEP 13

This step is the first consumer of the rolling primitives. The
following pieces of `zwift-stats` are stood up here, not in STEP 13:

- `DataCollector` / `DataBucket` per-signal wiring — one collector
  per signal, each owning a primary `RollingAverage` (or
  `RollingPower` for the power signal, with `inline_np = true`)
  plus one clone per peak period. Source: `src/stats.mjs:92-219`,
  `2697-2733`.
- Peak-period clone fan-out at `[5, 15, 60, 300, 1200, 3600]` s for
  power and `[60, 300, 1200, 3600]` s for the rest, each clone
  carrying its own `_snapValue` / `_snapTime` peak record. Source:
  `src/stats.mjs:177-194` (`_updatePeriodizedPeaks`,
  `_resizePeriodized`).
- Multi-bucket orchestration: `DataCollector::add(time, value)` is
  the ingestion entry point that owns the one-second buffer
  (`zwift_stats::OneSecondBucket` from STEP 13), flushes on
  `idealGap` boundaries, and forwards the boundary mean into the
  primary roll plus every periodized clone. Source:
  `src/stats.mjs:132-194`.

Each of these depends on `RollingAverage`, `RollingPower`,
`OneSecondBucket`, and `calc_tss` already being green; STEP 13's
parity vectors are the gate that lets this step build on top
without re-deriving the rolling math.

## Tests-first outline

- Feed a recorded `PlayerState` stream, verify each signal's avg / max /
  peaks match the JS reference.
- GC ticks at 10 s and evicts correctly.

To be fully elaborated when work on this step begins.
