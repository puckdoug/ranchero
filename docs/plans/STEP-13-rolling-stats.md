# Step 13 — `zwift-stats` rolling primitives (stub)

## Goal

Port `shared/sauce/data.mjs` + `shared/sauce/power.mjs`:

- `RollingAverage<T>` — time-indexed ring, gap-fill semantics
  (`idealGap`, `maxGap`, `softPad`/`Break` sentinels).
- `RollingPower` — inlines NP (30 s rolling window, 4th-power mean,
  `(mean)^(1/4)`, threshold min active time 300 s) and optional XP.
- `calc_tss(seconds, np, ftp) = (s * np * (np/ftp)) / (ftp * 3600) * 100`.
- 1-second bucketing before rolling pushes.

## Tests-first outline

- Against a set of recorded `(t, value)` traces, assert that the
  rolling sums and NP values agree with the JS implementation to
  ≤ 1e-6.
- Boundary cases: empty window, single-sample, `maxGap` exceeded,
  `softPad`/`Break` sentinels restart the active-time accumulator.

To be fully elaborated when work on this step begins.
