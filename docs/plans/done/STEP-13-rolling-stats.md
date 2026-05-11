# Step 13 — `zwift-stats` rolling primitives

**Status:** planned (2026-05-10).

## Goal

Stand up the `crates/zwift-stats` crate with the time-indexed rolling
primitives that drive every per-athlete metric in the project:

- `RollingAverage` — a time-indexed ring with gap-fill semantics
  (`ideal_gap`, `max_gap`, soft-`Pad` and `Break` sentinels) and O(1)
  average / active-time queries.
- `RollingPower` — extends `RollingAverage` with an inline Normalized
  Power accumulator (30 s rolling window, fourth-power mean,
  `(mean)^(1/4)`, returned only after `weighted_min_time` ≥ 300 s of
  active samples) and an optional XP accumulator.
- `calc_tss(np, seconds, ftp) = (seconds · np · (np / ftp)) /
(ftp · 3600) · 100` (algebraically `seconds · np² / (ftp² · 36)`).
- A small **one-second bucketer** that averages sub-second samples
  before they are pushed into the rolling window. The bucketer is the
  primitive on which STEP 14's `DataCollector` will compose its
  per-signal pipelines; it is built here because it has no per-athlete
  state and is exercised by the rolling tests anyway.
- Free-function helpers required by the algorithms above:
  `recommended_time_gaps`, `corrected_rolling_average`,
  `corrected_rolling_power`, `peak_average`, `peak_np`.

This crate is the foundation under STEPS 14 (`AthleteData` /
`DataBucket` / `DataCollector`), 15 (zones / W'-balance / segments) and
17–18 (the published JSON metrics that widgets consume). Numerical
parity with `shared/sauce/data.mjs` and `shared/sauce/power.mjs` is the
load-bearing acceptance criterion: STEP 19's compatibility battery
will replay recorded streams through both implementations and assert
agreement to ≤ 1e-6.

The remaining content of `power.mjs` falls into two groups:

- **Live-data-core consumers, deferred to later steps.**
  - `cogganZones` / `polarizedZones` / `sweetspotZone` are called
    from `src/stats.mjs:1225-1241` — port lives in STEP 15.
  - `makeIncWPrimeBalDifferential` is called from `src/stats.mjs:382`
    (the W'-balance accumulator) — port lives in STEP 15.
- **Analysis-page-only consumers, out of scope for ranchero v1.**
  - `rank` / `rankLevel` / `rankBadge` / `rankRequirements` —
    consumed by `pages/src/analysis.mjs:152` and `:1262` only.
  - `calcPwHrDecoupling` / `calcPwHrDecouplingFromRoll` — consumed
    by `pages/src/analysis.mjs:215` only.
  - `cyclingPowerEstimate`, `cyclingPowerVelocitySearch`,
    `cyclingPowerFastestVelocitySearch`,
    `cyclingPowerVelocitySearchMultiPosition`,
    `cyclingDraftDragReduction`, `seaLevelPower` — no callsite in
    `src/stats.mjs`; they back the analysis page's modelling tools.

  Spec §7.1 explicitly excludes "GUI, Electron widgets" from v1.
  These functions are pure and self-contained, so a future
  `zwift-stats::analysis` module can pick them up without churning
  the core — they are not deleted, just not on this critical path.
  STEP 15 records the same exclusion list so future readers do not
  re-discover it from scratch.

## Implementation checklist

The list below is split into explicit TDD pairs. `-T` items add the
listed tests and observe them fail. `-I` items add the smallest
production code that turns them green. Do not advance to the next
test pair until the current one is green; do not write code without
a failing test pinning the requirement first.

Setup (no tests):

- [x] **13.1** Crate skeleton (`zwift-stats`) — `Cargo.toml`, `lib.rs`
      with module stubs, SPDX header. `cargo test -p zwift-stats`
      runs and passes with zero tests collected.

`Sample` enum and pad interner:

- [x] **13.2-T** Add `tests/rolling.rs::sample_is_active_value`
      asserting `Value(0)`, `Value(5)`, `Pad(_)`, and `Break { .. }`
      classify correctly under both `ignore_zeros` flags. Compile
      error confirms `Sample` does not yet exist.
- [x] **13.2-I** Implement `Sample` (`Value | Pad | Break`) plus
      `is_active_value(s, ignore_zeros)`. Tests green.
- [x] **13.3-T** Add `pad_interner_returns_same_pad_for_close_values`
      and `zero_pad_is_a_singleton`. Tests fail (no interner).
- [x] **13.3-I** Implement `soft_pad(value)` keyed by
      `round(value * 10)` and a `ZERO` constant for hard pads. Tests
      green.

`RollingAverage` no-gap path:

- [x] **13.4-T** Add `empty_rolling_has_zero_elapsed_and_no_avg`,
      `single_sample_has_zero_elapsed`, `two_samples_avg`, and
      `accumulators_are_o1`. Tests fail (no `RollingAverage`).
- [x] **13.4-I** Implement `RollingAverage::new`, the no-gap branch
      of `add`, `process_add`, and the `avg` / `elapsed` / `active`
      accessors with O(1) accumulators. Tests green.

Gap-fill (soft, hard, catastrophic):

- [x] **13.5-T** Add `soft_pad_inserts_value_filler` and
      `pad_threshold_excludes_borderline`. Tests fail.
- [x] **13.5-I** Implement the `gap > ideal_gap · 1.61803` soft-pad
      branch in `add` (insert `Pad(value)` at `ideal_gap` spacing).
      Tests green.
- [x] **13.6-T** Add `hard_gap_inserts_zero_filler` and
      `explicit_active_false_zero_pads`. Tests fail.
- [x] **13.6-I** Implement the `max_gap` / `active == Some(false)`
      hard-gap branch (`ZERO` filler at `ideal_gap` spacing). Tests
      green.
- [x] **13.7-T** Add `break_gap_splits_with_book_ends` covering the
      `gap > 3600 s` "Garmin glitch" path. Test fails.
- [x] **13.7-I** Implement the catastrophic-gap branch (zero-pad
      half-hour book-ends + single `Break { pad }` sentinel). Test
      green.

Period eviction and accessors:

- [x] **13.8-T** Add `period_eviction_keeps_window_bounded`,
      `eviction_decrements_accumulators`, and
      `full_with_offt_one_evicts_correctly`. Tests fail.
- [x] **13.8-I** Implement `resize`, `shift`, `process_shift`, and
      the `while full({offt: 1}) shift()` eviction loop. Tests green.
- [x] **13.9-T** Add `clone_independent_writes`,
      `slice_returns_subwindow`, `time_at_negative_index`, and
      `entries_yields_offset_to_length`. Tests fail.
- [x] **13.9-I** Implement `clone_with`, `slice`, `pop`, `time_at` /
      `value_at`, `times` / `values` slice accessors, and the
      `entries` iterator. Tests green.

Bulk import and free-function helpers:

- [x] **13.10-T** Add `import_data_matches_serial_add` and
      `import_reduce_finds_peak_window`. Tests fail.
- [x] **13.10-I** Implement `import_data` and `import_reduce` (the
      walk-and-snapshot loop used by `peak_*`). Tests green.
- [x] **13.11-T** Add `recommended_time_gaps_mode_and_max`,
      `corrected_rolling_average_returns_none_for_short_streams`, and
      `peak_average_finds_max_window`. Tests fail.
- [x] **13.11-I** Implement `recommended_time_gaps`,
      `corrected_rolling_average`, `corrected_rolling_power`, and
      `peak_average`. Tests green.

Inline NP:

- [x] **13.12-T** Add `np_returns_none_below_300s_active`,
      `np_force_returns_value_below_min_time`,
      `np_constant_power_equals_power`, `np_known_vector`, and
      `np_with_soft_pads_matches_oracle`. Tests fail (no
      `RollingPower`).
- [x] **13.12-I** Implement `RollingPower::new` with the inline-NP
      state, the `process_add` contribution
      (`(rollSum / rollSize)^4`), and `np(force)` returning
      `(total / count)^(1/4)` once `active() ≥ 300 s`. Tests green.
- [x] **13.13-T** Add `np_after_eviction_matches_recompute`. Test
      fails (eviction does not yet reverse NP contributions).
- [x] **13.13-I** Implement `RollingPower::process_shift` to reverse
      one saved `qnpa` contribution per shifted sample (mirrors
      `power.mjs:243-250`). Test green.

Inline XP:

- [x] **13.14-T** Add the XP-mirror counterparts of the inline-NP
      tests against `fixtures/xp_short.json`. Tests fail.
- [x] **13.14-I** Implement the inline-XP state machine
      (`samplesPerWindow = 25 / ideal_gap`, attenuation /
      sample-weight pair, idle decay loop) and `xp(force)`. Tests
      green.

TSS:

- [x] **13.15-T** Add `tss_at_threshold`, `tss_zero_seconds`,
      `tss_zero_ftp_returns_none`, and `tss_known_vector`. Tests fail.
- [x] **13.15-I** Implement `calc_tss(np, seconds, ftp)`. Tests green.

One-second bucket:

- [x] **13.16-T** Add `bucket_emits_mean_on_boundary`,
      `bucket_round_to_int`, and `bucket_flush_drains_remainder`.
      Tests fail.
- [x] **13.16-I** Implement `OneSecondBucket::new` / `add` / `flush`.
      Tests green.

Recorded-stream parity:

- [x] **13.17-T** Run `tests/fixtures/gen_vectors.mjs` once to emit
      the JSON oracles, then add `tests/parity.rs` cases that load
      each `*.json` fixture, drive both `RollingAverage::avg` and
      `RollingPower::np` / `xp` through it, and assert every numeric
      output agrees with the embedded oracle to ≤ 1e-6.
- [x] **13.17-I** Resolve any deltas the parity tests surface
      (typically off-by-one in the gap-fill branch or the inline-NP
      ring). When green, this step's acceptance criteria are met.

A pair is "done" only when its `-T` item produced a red test that the
`-I` item then turned green; if `-I` is empty because nothing needed
fixing (most likely at 13.17), record that fact in the as-built notes
rather than skipping the entry.

## Scope

| In scope                                                                                                    | Out of scope (where it goes)                                                                                                                                       |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `RollingAverage` over `f64` (with `Sample` enum for sentinels).                                             | `DataCollector` / `DataBucket` per-signal wiring (STEP 14).                                                                                                        |
| `RollingPower` with optional inline NP and inline XP.                                                       | Peak-period clone fan-out, 5 / 15 / 60 / 300 / 1200 / 3600 s (STEP 14).                                                                                            |
| `calc_tss`.                                                                                                 | Multi-bucket orchestration (`DataCollector::add`) and one-second bucket fan-in (STEP 14).                                                                          |
| `recommended_time_gaps`, `corrected_rolling_average`, `corrected_rolling_power`, `peak_average`, `peak_np`. | Power / HR zone definitions and the time-in-zones accumulator (STEP 15).                                                                                           |
| `OneSecondBucket` (per-stream sub-second averaging).                                                        | W'-balance accumulator wrapping `RollingPower` (STEP 15).                                                                                                          |
| Numerical parity vectors against the JS reference.                                                          | Cycling-power estimator, drag reduction, sea-level power, ranking, decoupling — out of scope for ranchero v1; recorded in STEP 15 as the canonical exclusion list. |
|                                                                                                             | The compatibility test battery as a whole (STEP 19); this step ships only the per-function vectors.                                                                |

Source-of-truth references for every algorithm:

- `RollingAverage` — `sauce4zwift/shared/sauce/data.mjs:251-536`.
- `Pad` / `Break` / soft-pad cache — `data.mjs:227-248`.
- `recommendedTimeGaps` — `data.mjs:185-201`.
- `correctedRollingAverage` / `peakAverage` — `data.mjs:539-574`.
- `RollingPower` and the inline NP / XP state machines —
  `sauce4zwift/shared/sauce/power.mjs:161-319`.
- `calcNP` / `calcXP` (free-function fallbacks) —
  `power.mjs:383-456`.
- `calcTSS` — `power.mjs:459-464`.
- `weightedMinTime` (300 s) — `power.mjs:3`.
- 1-second bucketing — `sauce4zwift/src/stats.mjs:92-153`
  (`DataCollector.flushBuffered` / `add`).

## Crate layout

```
crates/zwift-stats/
├── Cargo.toml          — workspace member, AGPL-3.0-only
├── src/
│   ├── lib.rs          — re-exports + module-level docs
│   ├── sample.rs       — Sample enum, Pad interner, is_active predicate
│   ├── rolling.rs      — RollingAverage core
│   ├── power.rs        — RollingPower (inline NP + optional XP), calc_tss
│   ├── helpers.rs      — recommended_time_gaps, corrected_*, peak_*
│   └── bucket.rs       — OneSecondBucket
└── tests/
    ├── rolling.rs          — RollingAverage hand-computed cases
    ├── rolling_gaps.rs     — Pad / Break gap-fill cases
    ├── rolling_period.rs   — period eviction, clone, slice, pop
    ├── rolling_power_np.rs — inline NP correctness + minimum active time
    ├── rolling_power_xp.rs — inline XP correctness
    ├── tss.rs              — calc_tss against hand vectors
    ├── helpers.rs          — recommended_time_gaps + peak_* against hand vectors
    ├── bucket.rs           — OneSecondBucket against hand vectors
    ├── parity.rs           — recorded-stream parity vs the JS oracle
    └── fixtures/
        ├── README.md       — how vectors were generated
        ├── gen_vectors.mjs — Node script that imports `shared/sauce/{data,power}.mjs` and emits .json oracles
        ├── steady_state.json
        ├── soft_pad.json
        ├── hard_gap.json
        ├── break_gap.json
        ├── np_short.json   — 600 s of 1-Hz watts; expected NP value
        └── np_with_pads.json
```

Every public item is re-exported from `lib.rs` so callers
`use zwift_stats::{RollingAverage, RollingPower, calc_tss};` without
navigating internal module paths (same convention as `zwift-relay`).

## Dependencies

```toml
[dependencies]
thiserror = "1"

[dev-dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
approx = "0.5"   # readable f64 epsilon assertions in tests
```

Notes:

- **No `tokio`, no `tracing`.** This crate is synchronous and pure.
  The async wiring lives at the daemon layer (STEP 17).
- **No `prost` / proto types.** Rolling math operates on
  `(time: f64 seconds, value: f64)` pairs. `PlayerState` decoding is
  the consumer's job.
- **`f64` is the only sample type for now.** `RollingAverage<T>` was
  hinted at in the stub; you should still leave `Sample` and the
  accumulator types parametric only if a non-numeric callsite emerges.
  Until then, premature abstraction would cost more than it saves.
- **`approx` is dev-only.** Production code never compares floats with
  hand-rolled tolerances.

## Public API surface (proposed)

### `Sample` (`sample`)

```rust
/// One entry in a rolling window. Mirrors the JS `Number | Pad | Break`
/// triad: the discriminant tells `RollingAverage` whether the value
/// counts toward active time and whether eviction must skip it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sample {
    /// A real telemetry reading.
    Value(f64),
    /// A synthetic filler sample inserted by the gap-fill rules.
    /// Soft pads carry the previous value (`getSoftPad(value)`); hard
    /// pads carry zero (the cached `ZERO`). The discriminant alone is
    /// enough to keep them out of `is_active`.
    Pad(f64),
    /// A long-gap sentinel inserted between zero-padded book-ends in
    /// the > 3600 s "Garmin glitch" path. `pad` is the count of
    /// seconds the sentinel itself spans.
    Break { pad: f64 },
}

impl Sample {
    pub fn as_f64(self) -> f64;            // 0.0 for Break
    pub fn is_pad_or_break(self) -> bool;
}
```

The JS `_isActiveValue` predicate becomes a free function on
`Sample` plus a flag bag (`ignore_zeros`):

```rust
pub fn is_active_value(s: Sample, ignore_zeros: bool) -> bool;
```

### `RollingAverage` (`rolling`)

```rust
pub struct RollingAverageOptions {
    pub ideal_gap:    Option<f64>,   // seconds
    pub max_gap:      Option<f64>,
    pub active:       bool,          // controls full() / avg() default
    pub ignore_zeros: bool,
}

pub struct RollingAverage {
    /* private */
}

impl RollingAverage {
    pub fn new(period: Option<f64>, opts: RollingAverageOptions) -> Self;

    pub fn add(&mut self, ts: f64, value: Sample, active: Option<bool>);
    pub fn import_data(&mut self, times: &[f64], values: &[Sample], active: Option<&[bool]>);

    pub fn avg(&self, active: Option<bool>) -> Option<f64>;
    pub fn elapsed(&self) -> f64;
    pub fn active(&self) -> f64;
    pub fn full(&self, offt: usize, active: Option<bool>) -> bool;

    pub fn shift(&mut self);
    pub fn pop(&mut self);
    pub fn slice(&self, start: f64, end: Option<f64>) -> Self;
    pub fn clone_with(&self, opts: CloneOptions) -> Self;

    pub fn first_time(&self, no_pad: bool) -> Option<f64>;
    pub fn last_time(&self, no_pad: bool) -> Option<f64>;
    pub fn size(&self) -> usize;

    pub fn times(&self, offt: usize, len: Option<usize>) -> &[f64];
    pub fn values(&self, offt: usize, len: Option<usize>) -> &[Sample];
    pub fn time_at(&self, i: isize) -> Option<f64>;
    pub fn value_at(&self, i: isize) -> Option<Sample>;
    pub fn entries(&self) -> impl Iterator<Item = (f64, Sample)> + '_;
}
```

Internal state mirrors the JS object: `_times: Vec<f64>`,
`_values: Vec<Sample>`, `_offt: usize`, `_length: usize`,
`_active_acc: f64`, `_values_acc: f64`. `process_add` /
`process_shift` / `process_pop` are private methods so `RollingPower`
can override them via a trait or an inlined hook (see "Design
decisions").

### `RollingPower` (`power`)

```rust
pub struct RollingPowerOptions {
    pub rolling: RollingAverageOptions,
    pub inline_np: bool,
    pub inline_xp: bool,
    pub disable_inline_np_resize: bool,
    pub disable_inline_xp_resize: bool,
}

pub struct RollingPower { /* private */ }

impl RollingPower {
    pub fn new(period: Option<f64>, opts: RollingPowerOptions) -> Self;

    pub fn add(&mut self, ts: f64, watts: f64, active: Option<bool>);
    pub fn avg(&self, active: Option<bool>) -> Option<f64>;
    pub fn np(&self, force: bool) -> Option<f64>;
    pub fn xp(&self, force: bool) -> Option<f64>;
    pub fn joules(&self) -> f64;

    pub fn rolling(&self) -> &RollingAverage;     // for callers that need the time series
    pub fn rolling_mut(&mut self) -> &mut RollingAverage;
}
```

`np()` and `xp()` return `None` when `active() < weighted_min_time`
unless `force = true` (mirrors `power.mjs:266-288`). `weighted_min_time`
is a `pub const`; do not expose a setter at this step (the JS setter
exists for off-line analysis, which the project does not run).

### `calc_tss` (`power`)

```rust
/// `(seconds · np · (np / ftp)) / (ftp · 3600) · 100`. Returns `None`
/// for `ftp == 0.0` (the JS divides by zero and returns `NaN` /
/// `Infinity`; we surface the bad input).
pub fn calc_tss(np: f64, seconds: f64, ftp: f64) -> Option<f64>;
```

### `OneSecondBucket` (`bucket`)

```rust
/// Buffers sub-second `(time, value)` pairs and emits one mean per
/// `ideal_gap` window. `add()` returns `Some((end_time, mean))` when
/// the next sample crosses the boundary; otherwise `None`.
pub struct OneSecondBucket { /* private */ }

impl OneSecondBucket {
    pub fn new(ideal_gap: f64, round_to_int: bool) -> Self;
    pub fn add(&mut self, time: f64, value: f64) -> Option<(f64, f64)>;
    pub fn flush(&mut self) -> Option<(f64, f64)>;
}
```

### Errors

There are no fallible APIs in this crate. Every operation either
returns the value or `Option<f64>` (`None` means "not enough data
yet" or "min active time not reached"). Bad inputs (e.g. timestamps
going backwards) panic in debug and saturate in release — the same
behaviour the JS reference exhibits. Document this on `RollingAverage::add`.

## Tests-first plan

Every test file lives in `crates/zwift-stats/tests/*.rs`. The bullets
below correspond to checklist items 13.1 – 13.17.

### 13.1 Crate skeleton — `tests/lib.rs`

Just `#[test] fn it_compiles() {}`. Confirms `cargo test -p zwift-stats`
runs. Delete the file once 13.2 lands a real test.

### 13.2 — 13.3 `Sample` and pad interner — `tests/rolling.rs`

| Test                                             | Asserts                                                                                                                                                                                             |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sample_is_active_value`                         | `Value(0.0)` is inactive; `Value(5.0)` is active; `Pad(_)` is inactive regardless; `Break { .. }` is inactive. With `ignore_zeros = false`, `Value(0.0)` is still inactive (matches JS truthiness). |
| `pad_interner_returns_same_pad_for_close_values` | `soft_pad(2.34)` and `soft_pad(2.34)` produce equal `Pad(2.3)` (signature rounds `value * 10`).                                                                                                     |
| `zero_pad_is_a_singleton`                        | `zero_pad()` is `Pad(0.0)`; calling twice returns equal samples.                                                                                                                                    |

### 13.4 `RollingAverage` no-gap path — `tests/rolling.rs`

| Test                                        | Asserts                                                                                                                                                                                                     |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `empty_rolling_has_zero_elapsed_and_no_avg` | `r.avg(None).is_none()`, `r.elapsed() == 0.0`, `r.active() == 0.0`, `r.size() == 0`.                                                                                                                        |
| `single_sample_has_zero_elapsed`            | After `add(t=0, Value(100), None)`, `elapsed == 0`, `active == 0`, `avg == None` (JS divides by zero — we return `None`).                                                                                   |
| `two_samples_avg`                           | `add(0, 100); add(1, 200);` → `elapsed == 1`, `active == 1`, `avg == 200.0` (JS multiplies by gap, second sample is the only one whose gap counts). Hand-derive against `processAdd` in `data.mjs:415-422`. |
| `accumulators_are_o1`                       | After 1 000 `add` calls, `avg` returns the same value as a from-scratch sum / time computation (≤ 1e-9).                                                                                                    |

### 13.5 — 13.7 Gap-fill — `tests/rolling_gaps.rs`

| Test                                | Asserts                                                                                                                                                                                                                         |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `soft_pad_inserts_value_filler`     | `ideal_gap = 1`, `pad threshold = 1.61803`. `add(0, 100); add(3, 200);` inserts `Pad(200)` at `t = 1` and `t = 2` before the real sample at `t = 3`. (`getSoftPad(value)` is called with the _new_ value — see `data.mjs:401`.) |
| `hard_gap_inserts_zero_filler`      | `max_gap = 5`. `add(0, 100); add(10, 50);` inserts `Pad(0)` at `t = 1..9` (one per `ideal_gap`). The active accumulator does not advance across these.                                                                          |
| `explicit_active_false_zero_pads`   | `add(0, 100); add(2, 200, active = Some(false));` inserts `Pad(0)` at `t = 1` regardless of `max_gap` (JS `data.mjs:379`).                                                                                                      |
| `break_gap_splits_with_book_ends`   | `add(0, 100); add(7200, 200);` produces zero-pads for the leading half-hour, a single `Break { pad }` covering the middle, then zero-pads for the trailing half-hour, exactly matching `data.mjs:382-393`.                      |
| `pad_threshold_excludes_borderline` | `gap = 1.6` with `ideal_gap = 1` does **not** pad (`< 1.61803`); `gap = 1.7` does.                                                                                                                                              |

### 13.8 Period eviction — `tests/rolling_period.rs`

| Test                                   | Asserts                                                                                                                                                                              |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `period_eviction_keeps_window_bounded` | `period = 5`, push samples at `t = 0..10`. After every push, `elapsed <= 5`.                                                                                                         |
| `eviction_decrements_accumulators`     | After eviction, `avg` matches a from-scratch recompute over `r.values()` (≤ 1e-9).                                                                                                   |
| `full_with_offt_one_evicts_correctly`  | The `while (this.full({offt: 1}))` loop in `data.mjs:457-459` is reproduced verbatim: a window with `elapsed == period` does not evict, but pushing one more shifts a single sample. |

### 13.9 Accessors / clone / slice / pop — `tests/rolling_period.rs`

| Test                              | Asserts                                                                                                                                                                                                           |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clone_independent_writes`        | After `clone`, writes to the original do not affect the clone. (Note: the JS clone shares `_times` / `_values` and only forks indices; the Rust port copies them — this is a deliberate divergence; document it.) |
| `slice_returns_subwindow`         | `slice(start, end)` walks `shift()` then `pop()` until bounds match, exactly as `data.mjs:294-308`.                                                                                                               |
| `time_at_negative_index`          | `time_at(-1)` returns the last sample's timestamp; `time_at(-2)` returns the second-to-last.                                                                                                                      |
| `entries_yields_offset_to_length` | `entries().count() == size()` and the iterator skips evicted prefix.                                                                                                                                              |

### 13.10 `import_data` / `import_reduce` — `tests/rolling.rs`

| Test                              | Asserts                                                                                                                  |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `import_data_matches_serial_add`  | `r.import_data(&times, &values, None)` produces the same final state as a loop of `add` calls.                           |
| `import_reduce_finds_peak_window` | A 600-sample stream with a known 60-second peak average; `import_reduce` returns a clone whose `avg() == expected_peak`. |

### 13.11 Helpers — `tests/helpers.rs`

| Test                                                       | Asserts                                                                                                                     |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `recommended_time_gaps_mode_and_max`                       | A handcrafted timestamp stream returns `{ ideal: <mode>, max: round(max(ideal, median)) * 4 }` matching `data.mjs:185-201`. |
| `corrected_rolling_average_returns_none_for_short_streams` | `times.len() < 2` → `None`; last timestamp `< period` → `None`.                                                             |
| `peak_average_finds_max_window`                            | Compose `corrected_rolling_average` + `import_reduce`; matches a hand-computed peak.                                        |

### 13.12 Inline NP — `tests/rolling_power_np.rs`

| Test                                    | Asserts                                                                                                                                                                                                                        |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `np_returns_none_below_300s_active`     | 299 s of constant 200 W → `np(false) == None`.                                                                                                                                                                                 |
| `np_force_returns_value_below_min_time` | Same stream, `np(true) == Some(_)`.                                                                                                                                                                                            |
| `np_constant_power_equals_power`        | 600 s of constant 200 W → `np(false) ≈ 200.0` (≤ 1e-9).                                                                                                                                                                        |
| `np_known_vector`                       | A 600 s `(t, watts)` fixture (`fixtures/np_short.json`) with the JS oracle's NP value baked in; `np(false)` matches to ≤ 1e-6.                                                                                                 |
| `np_with_soft_pads_matches_oracle`      | Same as above, but the input has irregular timestamps that trigger soft-pad insertion; the inline NP must match `calcNP` over the post-pad value sequence (the JS contract: NP runs on the padded stream, not the raw stream). |

### 13.13 Inline NP eviction — `tests/rolling_power_np.rs`

| Test                                  | Asserts                                                                                                                                                                                                   |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `np_after_eviction_matches_recompute` | Bound a 600 s window with `period = 300`; after every `shift()`, `np(false)` equals a from-scratch `calc_np` over the remaining values (≤ 1e-9). This is the inline-NP analogue of `accumulators_are_o1`. |

### 13.14 Inline XP — `tests/rolling_power_xp.rs`

Mirrors 13.12 against the XP fixtures
(`fixtures/xp_short.json`). Asserted to ≤ 1e-6 against the JS oracle.

### 13.15 TSS — `tests/tss.rs`

| Test                        | Asserts                                                                                                              |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `tss_at_threshold`          | `calc_tss(ftp, 3600.0, ftp) == Some(100.0)`.                                                                         |
| `tss_zero_seconds`          | `calc_tss(np, 0.0, ftp) == Some(0.0)`.                                                                               |
| `tss_zero_ftp_returns_none` | `calc_tss(200.0, 600.0, 0.0) == None`.                                                                               |
| `tss_known_vector`          | A handful of `(np, seconds, ftp, expected)` rows generated by calling `calcTSS` directly in Node; matches to ≤ 1e-9. |

### 13.16 One-second bucketer — `tests/bucket.rs`

| Test                            | Asserts                                                                                                                                                                                                                  |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `bucket_emits_mean_on_boundary` | `add(0.0, 100); add(0.5, 200); add(1.1, 50);` → second `add` returns `None`, third returns `Some((0.5, 150.0))` (mean of the prior window flushed before storing the new sample). Hand-derived from `stats.mjs:143-152`. |
| `bucket_round_to_int`           | With `round_to_int = true`, the emitted mean is `round(150.4) == 150`.                                                                                                                                                   |
| `bucket_flush_drains_remainder` | After `add` with no boundary cross, `flush()` emits the buffered mean and clears state.                                                                                                                                  |

### 13.17 Recorded-stream parity — `tests/parity.rs`

For each of `np_short.json`, `np_with_pads.json`, `xp_short.json`, and
two longer captures, drive both `RollingAverage::avg` and
`RollingPower::np` / `xp` through the trace and assert agreement with
the embedded JS oracle to ≤ 1e-6. These vectors are the single
strongest gate against regression: every later step that touches
rolling math must keep them green.

### Reference vector strategy

`tests/fixtures/gen_vectors.mjs` is a small Node script (run by hand,
checked in for reproducibility, **not** invoked from CI) that:

1. Reads `(time, watts)` pairs from a captured ride (or generates a
   synthetic one for boundary cases).
2. Imports `shared/sauce/data.mjs` and `shared/sauce/power.mjs`
   directly.
3. Runs `correctedRollingPower(... { inlineNP: true })`,
   `correctedRollingAverage`, etc.
4. Writes `{ inputs: { times, values }, options: { ... }, outputs: { avg, np, xp, ... } }` to JSON.

The Rust test reads the JSON, reconstructs the inputs, runs the same
options through the Rust impl, and asserts every numeric output
matches to ≤ 1e-6. This is the same oracle pattern STEP 08 uses for
the AES-GCM-4 vectors — the JS reference is the source of truth, the
script is reproducible, and CI does not need a Node toolchain.

The script lives **inside** `crates/zwift-stats/tests/fixtures/`
because the per-conversation no-sauce4zwift-runtime-dep rule prohibits
build / test paths through `sauce4zwift/`. The script is not on any
build path — it is a one-shot generator that you run, then check in
the JSON it produces. The committed JSON is what the Rust tests
consume.

## Acceptance criteria

- `cargo test -p zwift-stats` is green from a clean checkout.
- Every checklist item 13.1 – 13.17 has at least one test and at
  least one production-code change.
- All numerical parity tests in `tests/parity.rs` pass to ≤ 1e-6
  against the checked-in JS oracle JSON.
- No `unsafe`. No `unwrap` outside test code (`expect("invariant: …")`
  with a stated invariant is acceptable for state-machine assertions
  inside `process_*`).
- SPDX header `// SPDX-License-Identifier: AGPL-3.0-only` at the top
  of every `.rs` file.

## Open verification points

These are claims that should be confirmed before declaring the step
complete. None block tests; the implementation can be written and
tested against either choice. Record any decision in the as-built
notes appended to this file.

1. **`Sample` discriminant for `Pad(0)` vs `Value(0)`.** The JS
   `_isActiveValue` distinguishes them via `instanceof Pad`. The
   Rust port's correctness depends on this discrimination surviving
   every pathway (`process_add`, `process_shift`, `is_active_value`,
   `first_time / last_time` with `no_pad: true`, `import_reduce`
   skipping). A property test that drives every pathway is a
   reasonable belt-and-braces guard.

2. **Clone semantics: shared vs owned `_times` / `_values`.** The JS
   clone shares `_times` and `_values` with the parent (it forks only
   `_offt`, `_length`, `_active_acc`, `_values_acc`). Sharing in
   Rust would require `Rc<Vec<f64>>` plus copy-on-write on `add`.
   The plan above copies on clone. Verify whether the per-period
   peak-clone fan-out in STEP 14 produces enough clone pressure to
   matter; if it does, switch to `Arc<RwLock<…>>` or a custom CoW
   structure later. Until then, copying is correct, simple, and
   testable.

3. **`weighted_min_time` is a `pub const`, not a global.** The JS
   `setWeightedPowerMinTime` setter mutates a module-level `let`
   binding. Ranchero never changes it from 300 s in the live-data
   path; if a future analysis pathway needs to, parameterise
   `RollingPower::new` rather than reintroducing global mutable
   state.

4. **TSS algebraic form.** The stub spelled `(s · np · (np / ftp))
/ (ftp · 3600) · 100`; the JS spells `((joules · intensity) /
ftpHourJoules) · 100` with `joules = power · duration`,
   `intensity = power / ftp`. They are algebraically identical. The
   Rust impl can use either; the test vectors will confirm parity.

5. **Sample type generic vs concrete.** The original stub mentioned
   `RollingAverage<T>`. The plan above commits to `f64` because no
   non-numeric callsite exists today and `Sample` already carries
   the sentinel discrimination. If a future signal (e.g. categorical
   group identity) needs rolling tracking, the right move is a
   parallel `RollingMode` type, not a generic widening of
   `RollingAverage`.

## Design decisions worth pre-committing

- **Pure crate, no async.** Same posture as `zwift-relay`'s codec
  layer. The only inputs are `f64` pairs and option structs; the
  only outputs are `Option<f64>` and accessor slices. STEP 14
  composes this crate with the `tokio` reactor.
- **Tests live in `tests/`, not `src/`.** Project convention: every
  crate has integration tests only, no `#[cfg(test)] mod tests`
  inside the source.
- **Inline NP / XP are flags on `RollingPowerOptions`, not separate
  types.** Two reasons. First, JS's runtime polymorphism is not free
  to translate as separate Rust types (a `RollingPower<NoNP, WithXP>`
  matrix would explode quickly). Second, the JS API is itself
  flag-based and the consumer code in STEP 14 will set the flags
  conditionally per peak period.
- **`process_add` / `process_shift` / `process_pop` are methods on
  `RollingAverage` and not a trait.** `RollingPower` composes
  `RollingAverage` (delegation) rather than inheriting it. This
  avoids the JS-style `super.processAdd(i)` pattern and keeps the
  call graph readable. The cost is a handful of explicit forwarding
  methods on `RollingPower`; the benefit is that the inline-NP /
  XP state machine lives in one file (`power.rs`) and the rolling
  invariants live in another (`rolling.rs`).
- **No streams / async iterators in this step.** `import_data` takes
  borrowed slices because the consumer (STEP 14) already has the
  sample buffered. A streaming variant can be added when STEP 17
  measures the cost.
- **Float comparison policy.** Production code never compares floats
  for equality. Tests use `approx::abs_diff_eq!(actual, expected,
epsilon = 1e-9)` for hand-derived vectors, `epsilon = 1e-6` for
  parity vectors against the JS oracle. The wider tolerance for
  parity is justified by IEEE-754 differences across V8 and Rust's
  `powf`.

## Wiring into the workspace

- `crates/zwift-stats/` is picked up by the existing `members =
["crates/*"]` glob in the root `Cargo.toml`; no edit is needed
  there until a consumer (STEP 14) starts depending on it.
- `Cargo.lock` will refresh automatically the first time
  `cargo test -p zwift-stats` runs.
- The root `ranchero` crate gains a `zwift-stats = { path = "..." }`
  dependency only when STEP 14 needs it. STEP 13 itself ships no
  CLI surface.
- License header `// SPDX-License-Identifier: AGPL-3.0-only` at the
  top of every `.rs` file (matches `zwift-proto`, `zwift-api`,
  `zwift-relay`).
