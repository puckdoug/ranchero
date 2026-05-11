# Step 14 — Per-athlete state, DataBucket, DataCollector

**Status:** planned (2026-05-10).

## Goal

Stand up the per-athlete orchestration layer in `zwift-stats` that
composes the STEP 13 rolling primitives into the same structure
`sauce4zwift/src/stats.mjs` exposes:

- `DataCollector` — wraps a primary rolling window plus one clone per
  peak period; owns the one-second buffer that the daemon will feed.
  Tracks an all-time max value and per-period peak snapshots.
- `PowerDataCollector` — extends `DataCollector` with a parallel NP
  peak snapshot for periods at or above 300 s (the inline-NP minimum
  active time).
- `DataBucket` — holds one collector per signal: power (`PowerDataCollector`,
  periods `[5, 15, 60, 300, 1200, 3600]`), HR / speed / draft
  (`DataCollector`, periods `[60, 300, 1200, 3600]`), cadence
  (`DataCollector`, periods `[]`).
- `AthleteData` — the per-athlete record keyed by `athleteId` in the
  daemon's registry. STEP 14 ports the identity, lifecycle-timestamp,
  and bucket fields; the accumulators (`wBal`, `timeInPowerZones`,
  `smoothGrade`, `roadHistory`) and slice machinery
  (`lapSlices` / `eventSlices` / `segmentSlices`) belong to STEP 15.
- `AthleteRegistry` — `HashMap<athleteId, AthleteData>` plus the GC
  loop: drop riders whose `internal_accessed` has aged past the 1 h
  TTL, drop group metadata past the 90 s TTL.

Numerical parity with `stats.mjs` end-to-end is the load-bearing
acceptance criterion: a recorded telemetry stream replayed through
`DataBucket::ingest_*` must produce the same `avg / max / peaks /
peakNP` values the JS oracle records.

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

## Implementation checklist

The list below is split into explicit TDD pairs. `-T` items add the
listed tests and observe them fail. `-I` items add the smallest
production code that turns them green. Do not advance to the next
test pair until the current one is green; do not write code without
a failing test pinning the requirement first.

A pair is "done" only when its `-T` item produced a red test that the
`-I` item then turned green. If `-I` is empty because nothing needed
fixing, record that fact in the as-built notes rather than skipping
the entry.

Setup (no tests):

- [x] **14.1** Module skeleton. Add `src/collector.rs`,
      `src/data_bucket.rs`, `src/athlete.rs`, `src/periods.rs` with
      empty types and SPDX headers; wire them into `lib.rs` behind
      `pub mod` declarations. `cargo test -p zwift-stats` continues
      to pass with all existing tests green and zero new tests.
      **Done:** 49 tests passing, all four modules created with SPDX
      headers, `periods.rs` carries GC and peak-period constants.

`RollingAverage` extensions (used by `DataCollector`):

- [x] **14.2-T** `tests/rolling_full.rs::full_returns_true_when_elapsed_meets_period`
      and `full_offt_one_loop_evicts_one_sample` — pin the
      `roll.full()` semantics that `DataCollector._resizePeriodized`
      relies on (and which the JS `data.mjs:457-459` while-loop
      drives). **Done:** Test file created, 3 test cases defined. Tests
      fail (expected): no method `full()` on `RollingAverage` yet.
- [x] **14.2-I** Add `pub fn full(&self, offt: usize) -> bool` to
      `RollingAverage`: returns `true` when `period.is_some()` and
      `period <= times[length - 1] - times[offt + offt]`. Also
      expose `pub fn last_time(&self) -> Option<f64>` (returns
      `time_at(-1)`) — `_updatePeriodizedPeaks` reads it for
      `_snapTime`. **Done:** Both methods added to RollingAverage and
      RollingPower. 3 tests green.
- [x] **14.3-T** `tests/rolling_reset.rs::reset_clears_state_keeps_options`
      — push samples, call `reset`, assert `size == 0`,
      `avg == None`, but the next `add` honors the same
      `ideal_gap` / `max_gap` / `period`. **Done:** Test file created,
      2 test cases defined. Tests fail (expected): no method `reset()`
      on `RollingAverage` or `RollingPower` yet.
- [x] **14.3-I** Add `pub fn reset(&mut self)` to `RollingAverage`:
      clears `times`, `values`, `offt`, `length`, `active_acc`,
      `values_acc`; preserves `period` and the `*_gap` / `active` /
      `ignore_zeros` options. Mirror in `RollingPower::reset` (also
      clears `qnpa_*` / `xpa_*`). **Done:** Both methods added. 2 tests
      green. Total test suite: 54 passing.

`RollingPower` accessors (read-only fan-out helpers):

- [x] **14.4-T** `tests/rolling_power_accessors.rs::power_exposes_avg_active_elapsed_lasttime`
      — assert `RollingPower::avg(None)`, `active()`, `elapsed()`,
      `last_time()`, and `full(0)` agree with the inner
      `RollingAverage` after a hand-driven sequence. **Done:** Test
      file created, 2 test cases defined. Tests fail (expected): no
      forwarder methods on `RollingPower` yet.
- [x] **14.4-I** Forward `avg`, `active`, `elapsed`, `last_time`,
      `full`, `time_at`, `value_at`, `reset`, `entries` from
      `RollingPower` to its inner `RollingAverage`. Also add
      `pub fn rolling(&self) -> &RollingAverage` for tests that need
      direct access. **Done:** All 9 forwarders added to RollingPower.
      2 tests green. Total suite: 56 passing.

`DataCollector` core (no NP yet):

- [ ] **14.5-T** `tests/collector.rs::new_creates_primary_and_periodized_clones`
      — `DataCollector::<RollingAverage>::new(periods=[60, 300],
      opts)` exposes a primary roll with no period and exactly two
      periodized entries with `period == 60.0` and `period == 300.0`,
      each starting empty.
- [ ] **14.5-I** Implement `DataCollector<R>` where `R` is a trait
      that both `RollingAverage` and `RollingPower` will implement
      (see "Public API surface" below for the trait shape). Construct
      the primary with `period = None` and one clone per entry of
      `periods`; each periodized entry stores
      `{ period, roll: R, peak: Option<PeakSnapshot> }`. The trait
      default implementation provides a `new_with_period(period,
      opts)` factory so the collector does not need to choose between
      `RollingAverage::new` and `RollingPower::new`.

- [ ] **14.6-T** `tests/collector.rs::add_buffers_until_ideal_gap_boundary`
      — `add(0.0, 100.0); add(0.5, 200.0)` returns 0 newly-flushed
      samples; `add(1.1, 50.0)` returns 1 flushed sample whose value
      is `mean(100, 200) = 150`. Mirror `stats.mjs:132-152`.
- [ ] **14.6-I** Implement `DataCollector::add(time, value)`: hold
      `_buffered_start / _buffered_end / _buffered_sum /
      _buffered_len`; when `time - _buffered_start >= ideal_gap`,
      flush via `_flush_buffered()` (compute mean, push into the
      primary and every periodized roll), then reset the buffer with
      `_buffered_start = time`. Honour the `round` option by rounding
      the flushed mean before push.

- [ ] **14.7-T** `tests/collector.rs::tracks_max_value_across_flushes`
      — feed a synthetic stream whose flushed means rise then fall;
      `max_value()` returns the peak across all pushes.
- [ ] **14.7-I** Add `_max_value: f64` and update it in `_add` after
      every successful push (matches `stats.mjs:165-167`). Expose
      `pub fn max_value(&self) -> f64`.

- [ ] **14.8-T** `tests/collector.rs::periodized_peak_snapshots_max_avg`
      — feed a stream where the 60 s window's avg rises from 100 W
      to 250 W to 200 W; assert `peaks()[0].avg == 250.0` and
      `peaks()[0].time` matches the timestamp at which the 250 W
      window was reached. Window must be full (`elapsed >= period`)
      before peaks update.
- [ ] **14.8-I** Implement `_resize_periodized` and
      `_update_periodized_peaks` (`stats.mjs:177-194`): for each
      periodized entry, after the primary push, compare
      `roll.avg()` against `peak.snap_value`; on improvement, snapshot
      `roll.clone()` plus `snap_value` and `snap_time = roll.last_time()`.
      The peak is only updated once the period is full (to avoid
      stamping a 5-sample window as the 60 s peak).

- [ ] **14.9-T** `tests/collector.rs::clone_with_reset_creates_empty_snapshot`
      and `clone_without_reset_preserves_max_and_peaks` — pin both
      branches of `stats.mjs:201-218`.
- [ ] **14.9-I** Implement `pub fn clone_reset(&self) -> Self` and
      `pub fn clone_continue(&self) -> Self`. `clone_reset` returns
      a collector with the same options/periods but an empty buffer,
      empty primary, and `peak = None` on every periodized entry.
      `clone_continue` carries `_max_value`, the primary roll's
      state, and every `peak` snapshot forward (used by lap/segment
      slice creation in STEP 15; just exercise it here).

`PowerDataCollector` (NP peak overlay):

- [ ] **14.10-T** `tests/power_collector.rs::np_peak_only_for_periods_at_or_above_300`
      — periods `[5, 15, 60, 300, 1200, 3600]`. Drive a 600 s
      constant-power stream; assert
      `np_peaks()[0..3].iter().all(|p| p.is_none())` (the 5 / 15 /
      60 s entries do not record an NP peak) and `np_peaks()[3..]`
      all carry `Some(_)` matching the inline-NP value.
- [ ] **14.10-I** Implement `PowerDataCollector` as
      `DataCollector<RollingPower>` plus a `_np_periodized_offt:
      usize` and a `peak_np: Option<NpPeakSnapshot>` per periodized
      entry. Override `_update_periodized_peaks` to also call
      `roll.np(false)` and snapshot when the period is at or above
      `MIN_WEIGHTED_POWER_PERIOD` (300 s). The override is achieved
      via a method on the collector trait (not generic-over-power);
      see "Design decisions".

- [ ] **14.11-T** `tests/power_collector.rs::np_peak_survives_clone_continue`
      — drive a stream that produces a real NP peak, call
      `clone_continue()`, assert the cloned collector reports the
      same `np_peaks()`. After `clone_reset()`, NP peaks are `None`.
- [ ] **14.11-I** Extend the clone methods on `PowerDataCollector` to
      copy / clear `peak_np` per the JS at `stats.mjs:255-263`.

`DataBucket` (the five-signal aggregate):

- [ ] **14.12-T** `tests/data_bucket.rs::default_construction_matches_js_signals`
      — `DataBucket::new(start)` exposes the five signal collectors
      with the periods, `ignore_zeros`, and `round` flags from the
      JS table at `stats.mjs:2697-2714` (see the "Signal table"
      below). All time / kJ accumulators start at zero; `start` is
      stored verbatim.
- [ ] **14.12-I** Implement `DataBucket` with `start: f64`,
      `coffee_time / work_time / follow_time / solo_time: f64`,
      `work_kj / follow_kj / solo_kj: f64`, and the five collectors
      named `power`, `hr`, `speed`, `cadence`, `draft`. Construct
      each with the JS-matching options.

- [ ] **14.13-T** `tests/data_bucket.rs::ingest_routes_to_correct_collector`
      — `bucket.ingest_power(t, w)` lands in `bucket.power` only;
      assert `bucket.hr.max_value() == 0.0` after a power-only
      stream. Mirror for HR / speed / cadence / draft.
- [ ] **14.13-I** Implement `pub fn ingest_power(&mut self, t: f64,
      watts: f64)`, and matching methods for hr / speed / cadence /
      draft. Each method delegates to the corresponding collector's
      `add(t, value)`. No proto types; this is the seam the daemon
      (STEP 17) will wire.

- [ ] **14.14-T** `tests/data_bucket.rs::clone_reset_creates_slice_template`
      and `clone_continue_preserves_session_totals` — pin the two
      clone behaviours used by `_createDataSlice` (reset for a fresh
      lap / segment) versus session-wide carry-forward.
- [ ] **14.14-I** Implement `pub fn clone_reset(&self) -> Self` and
      `pub fn clone_continue(&self) -> Self` on `DataBucket`.
      `clone_reset` zeroes the time / kJ accumulators and calls
      `clone_reset` on every collector; `clone_continue` carries
      everything forward. (Slice machinery itself is STEP 15; the
      methods exist now so that step has the seam.)

`AthleteData` (identity + bucket + GC timestamps):

- [ ] **14.15-T** `tests/athlete_data.rs::new_initialises_identity_and_timestamps`
      — `AthleteData::new(athlete_id, course_id, sport, world_time,
      now)` exposes those four identity fields verbatim and sets
      `created == updated == now`, `internal_created ==
      internal_updated == internal_accessed == now`,
      `wt_offset == world_time`, `distance_offset == 0.0`.
      `bucket.start == now`.
- [ ] **14.15-I** Implement the `AthleteData` struct with the STEP 14
      subset of fields from spec §5.2:
      `athlete_id: u32`, `course_id: u32`, `sport: u8`,
      `created: f64`, `updated: f64`, `wt_offset: f64`,
      `distance_offset: f64`, `internal_created: f64`,
      `internal_updated: f64`, `internal_accessed: f64`,
      `most_recent_state: Option<…>` (typed as a generic
      `Option<MostRecentState>` placeholder struct holding only the
      fields STEP 14 needs — see "Open verification points"),
      `bucket: DataBucket`. Fields deferred to STEP 15 are
      explicitly enumerated in a `// STEP 15:` comment block.

- [ ] **14.16-T** `tests/athlete_data.rs::touch_updates_internal_accessed`
      — call `touch(now + 5.0)`, assert
      `internal_accessed == now + 5.0`, `internal_updated` and
      `internal_created` unchanged.
- [ ] **14.16-I** Implement `pub fn touch(&mut self, now: f64)` that
      sets `internal_accessed = now`. Add `pub fn record_update(&mut
      self, world_time: f64, now: f64)` that sets `updated`,
      `internal_updated`, and `internal_accessed` (called by the
      daemon when a `PlayerState` arrives).

- [ ] **14.17-T** `tests/athlete_data.rs::ingest_routes_through_bucket`
      — call `ad.ingest_power(t, watts)` and assert
      `ad.bucket.power.max_value() == watts` (after one full
      `ideal_gap` flush). Mirror for HR / speed / cadence / draft.
- [ ] **14.17-I** Add forwarding methods on `AthleteData` that
      delegate straight to `self.bucket.ingest_*` and bump
      `internal_updated` / `internal_accessed`. The daemon never
      reaches into `bucket` directly (encapsulation: future step may
      route to slice collectors as well).

`AthleteRegistry` and GC:

- [ ] **14.18-T** `tests/athlete_registry.rs::insert_get_remove_round_trip`
      — `registry.upsert(state, now)` returns a mutable reference;
      a second `upsert` for the same `athlete_id` returns the same
      record (does not allocate a new one). `registry.get(id)` and
      `registry.len()` work as expected.
- [ ] **14.18-I** Implement `pub struct AthleteRegistry { athletes:
      HashMap<u32, AthleteData> }` with `upsert`, `get`, `get_mut`,
      `len`, `is_empty`, and `iter`. `upsert` is the entry point
      that creates a new `AthleteData` on first sight and calls
      `record_update` on subsequent calls.

- [ ] **14.19-T** `tests/athlete_registry.rs::gc_evicts_athletes_past_ttl`
      — insert two athletes, advance their `internal_accessed` to
      `now - 3601.0` and `now - 3599.0` respectively, call
      `registry.gc(now)`, assert only the second survives.
- [ ] **14.19-I** Implement `pub fn gc(&mut self, now: f64)` that
      iterates `athletes`, drops every entry whose
      `internal_accessed < now - ATHLETE_GC_TTL_SECS`. Constants
      live in `periods.rs`: `ATHLETE_GC_TTL_SECS = 3600.0`,
      `GROUP_GC_TTL_SECS = 90.0`, `GC_TICK_INTERVAL_SECS = 62.768`
      (the JS interval at `stats.mjs:3553`; the stub's "10 s"
      claim is a test-side knob, not the production default — see
      "Open verification points").

- [ ] **14.20-T** `tests/athlete_registry.rs::gc_evicts_groups_past_ttl`
      — register two stub group metas with `accessed` at
      `now - 91.0` and `now - 89.0`, call `gc(now)`, assert only
      the second survives. (Group classification is STEP 15; this
      step ships the GC seam plus a `GroupMeta` placeholder so the
      eviction logic is testable now.)
- [ ] **14.20-I** Add `pub struct GroupMeta { id: u32, accessed: f64
      }` and `groups: HashMap<u32, GroupMeta>` to `AthleteRegistry`.
      Extend `gc` to drop groups past `GROUP_GC_TTL_SECS`. Group
      classification (deciding which athletes belong to which
      group, populating `identity_set`) is out of scope; STEP 15
      will populate the map (see STEP 15's "Inputs deferred from
      STEP 14").

Recorded-stream parity:

- [ ] **14.21-T** Generate `tests/fixtures/athlete_stream.json` via a
      hand-run Node script `tests/fixtures/gen_athlete_vectors.mjs`
      that drives `DataCollector` / `PowerDataCollector` directly
      (importing `shared/sauce/{data,power}.mjs` and the small
      DataCollector helper extracted out of `stats.mjs:92-320`)
      against a captured ride. The JSON carries
      `{ inputs: [{ time, power, hr, speed, cadence, draft }, …],
      outputs: { power: { avg, max, peaks: [...], np_peaks: [...]
      }, hr: { ... }, … } }`. Add `tests/stream_parity.rs` cases
      that load the fixture, replay it through `DataBucket::ingest_*`,
      and assert every numeric output agrees with the embedded
      oracle to ≤ 1e-6.
- [ ] **14.21-I** Resolve any deltas the parity tests surface
      (typically off-by-one in the periodized-peak full-window gate
      or in the 1 s buffer flush boundary). When green, this step's
      acceptance criteria are met.

## Tests-first plan (detail)

Every test file lives in `crates/zwift-stats/tests/*.rs`. The bullets
below correspond to the checklist items above.

### 14.2 `RollingAverage::full` — `tests/rolling_full.rs`

| Test                                              | Asserts                                                                                                                                                              |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `full_returns_true_when_elapsed_meets_period`     | `period = 60`. After 59 samples at 1 Hz, `full(0) == false`; after the 60th sample, `full(0) == true`.                                                               |
| `full_offt_one_loop_evicts_one_sample`            | Same setup, push the 61st sample, then `while r.full(1) { r.pop(); }` runs exactly once. Mirrors `data.mjs:457-459`.                                                 |
| `full_returns_false_when_period_is_none`          | A periodless `RollingAverage` always returns `false` from `full`, regardless of how many samples it holds. (Matches the JS `if (this._period == null) return false`.) |

### 14.3 `RollingAverage::reset` — `tests/rolling_reset.rs`

| Test                                  | Asserts                                                                                                                                                                                                                |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `reset_clears_state_keeps_options`    | After `add(0, 100); add(1, 200); reset();`, `size() == 0`, `avg(None) == None`, `active() == 0`, `elapsed() == 0`. Calling `add(2, 300)` then proceeds as if the rolling were brand new but with the original options. |
| `reset_on_rolling_power_clears_qnpa`  | A `RollingPower` with 600 s of data, after `reset`, returns `None` from `np(true)` (no `qnpa_*` accumulation left).                                                                                                    |

### 14.4 `RollingPower` accessors — `tests/rolling_power_accessors.rs`

| Test                                          | Asserts                                                                                                                                                                                  |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `power_exposes_avg_active_elapsed_lasttime`   | After a hand-built sequence, `RollingPower::avg(None)`, `active()`, `elapsed()`, `last_time()` agree with the same calls on `power.rolling()` to the bit (these are pure forwarding).    |
| `power_full_matches_inner_full`               | `RollingPower::full(0)` equals `power.rolling().full(0)`.                                                                                                                                |

### 14.5 `DataCollector` construction — `tests/collector.rs`

| Test                                              | Asserts                                                                                                                                                                                                       |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new_creates_primary_and_periodized_clones`       | `DataCollector::<RollingAverage>::new(periods=[60.0, 300.0], opts)` exposes `primary().size() == 0`, `periodized().len() == 2`, `periodized()[0].period == 60.0`, `periodized()[1].period == 300.0`.          |
| `empty_periods_yields_primary_only`               | `periods=[]` gives `periodized().len() == 0`. `cadence` in JS uses this shape — there is no peak fan-out for cadence.                                                                                         |
| `default_options_match_js_constants`              | `RollingAverageOptions::default()` plus the collector's enforced overrides yields `ideal_gap = Some(1.0)`, `max_gap = Some(15.0)`, `active = true` (matches `defOptions` in `stats.mjs:99`).                  |

### 14.6 1 s buffering — `tests/collector.rs`

| Test                                              | Asserts                                                                                                                                                                                  |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `add_buffers_until_ideal_gap_boundary`            | `add(0.0, 100.0)` returns 0; `add(0.5, 200.0)` returns 0 (still inside the 1 s window); `add(1.1, 50.0)` returns 1, and `primary().value_at(0)` equals `Sample::Value(150.0)` (mean).    |
| `flush_drains_partial_buffer`                     | After `add(0.0, 100.0); add(0.5, 200.0)`, `flush()` returns 1 and `primary().value_at(0) == Sample::Value(150.0)`. `flush()` again returns 0.                                            |
| `round_option_rounds_flushed_mean`                | Same buffered values with `round = true` produce `Sample::Value(150.0)`; with `round = false` and unequal samples, the mean is preserved as-is.                                          |

### 14.7 Max value tracking — `tests/collector.rs`

| Test                                              | Asserts                                                                                                                                                                                  |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tracks_max_value_across_flushes`                 | A handcrafted stream whose flushed means are `[100, 250, 200, 180]` produces `max_value() == 250.0` at every later push.                                                                 |
| `max_value_unaffected_by_pad_fills`               | A gap that triggers soft-pad insertion does not raise `max_value()`. (JS reads `value` directly, not `Sample`; we must mirror that by gating on the post-flush `f64`.)                   |

### 14.8 Periodized peak snapshots — `tests/collector.rs`

| Test                                              | Asserts                                                                                                                                                                                                                                                                                       |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `periodized_peak_snapshots_max_avg`               | Stream of 60 s at 100 W, then 60 s at 250 W, then 60 s at 200 W. The 60 s peak is `Some(snap)` with `snap.avg == 250.0` and `snap.time` matching the timestamp at which the 250 W window first became full.                                                                                  |
| `peak_does_not_update_until_window_is_full`       | Period 60 s, push 30 s of 250 W data. `peaks()[0]` is `None` (the window is not yet full and JS does not update the peak — `data.mjs:457` only enters the update branch when `roll.full()`).                                                                                                  |
| `peak_uses_geq_comparison_not_strict_gt`          | Push 60 s at 100 W, then a 60 s window also averaging 100 W. The peak's `snap_time` advances (JS uses `>=`; this is documented behaviour).                                                                                                                                                    |

### 14.9 Collector clone — `tests/collector.rs`

| Test                                              | Asserts                                                                                                                                                                                  |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clone_with_reset_creates_empty_snapshot`         | After driving a stream that produces a 250 W peak, `clone_reset()` returns a collector with `max_value() == 0.0`, `peaks()` all `None`, primary `size() == 0`.                          |
| `clone_without_reset_preserves_max_and_peaks`     | `clone_continue()` returns a collector whose `max_value()` and `peaks()` match the source. Subsequent writes on either side do not affect the other (deep-copied state).                |

### 14.10 — 14.11 Power collector NP peaks — `tests/power_collector.rs`

| Test                                              | Asserts                                                                                                                                                                                                                                                              |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `np_peak_only_for_periods_at_or_above_300`        | Periods `[5, 15, 60, 300, 1200, 3600]`, 600 s of 200 W. `np_peaks()[0..3]` are all `None`; `np_peaks()[3]` carries `Some(snap)` with `snap.avg ≈ 200.0`. The 1200 / 3600 s entries also have NP peaks (the period is not yet full but inline NP applies as soon as `active() ≥ 300 s`). |
| `np_peak_snapshot_records_lasttime`               | After driving a stream that produces an NP peak, the snapshot's `snap_time` matches the inner roll's `last_time()` at the moment the peak was set.                                                                                                                  |
| `np_peak_uses_geq_comparison`                     | A flat 200 W stream produces successive peaks at every push (the comparison is `>=` per `stats.mjs:280-287`).                                                                                                                                                       |
| `np_peak_survives_clone_continue`                 | `clone_continue()` carries `peak_np` forward.                                                                                                                                                                                                                       |
| `np_peak_cleared_by_clone_reset`                  | `clone_reset()` clears `peak_np` to `None` on every entry.                                                                                                                                                                                                          |

### 14.12 — 14.14 `DataBucket` — `tests/data_bucket.rs`

| Test                                              | Asserts                                                                                                                                                                                                                                                                |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default_construction_matches_js_signals`         | `bucket.power.periodized().len() == 6` (5/15/60/300/1200/3600); `bucket.hr.periodized().len() == 4` (60/300/1200/3600); `bucket.speed.periodized().len() == 4`; `bucket.cadence.periodized().len() == 0`; `bucket.draft.periodized().len() == 4`. Also pins each collector's `ignore_zeros` and `round` flag from the JS table. |
| `start_and_accumulators_initialise_correctly`     | `bucket.start == constructor_arg`, `coffee_time == work_time == follow_time == solo_time == 0.0`, `work_kj == follow_kj == solo_kj == 0.0`.                                                                                                                            |
| `ingest_routes_to_correct_collector`              | A power-only stream produces non-zero `bucket.power.max_value()` and `bucket.hr.max_value() == 0.0` (and likewise for the other three).                                                                                                                                |
| `clone_reset_creates_slice_template`              | `bucket.clone_reset()` zeros all time / kJ accumulators and produces empty collectors. `bucket.start` carries forward — the clone is a *slice template*, so `start` is the only timing field that must be re-set by the caller after cloning. (Pin in test, not auto.) |
| `clone_continue_preserves_session_totals`         | `bucket.clone_continue()` carries forward all time / kJ accumulators and all collector state.                                                                                                                                                                          |

### 14.15 — 14.17 `AthleteData` — `tests/athlete_data.rs`

| Test                                              | Asserts                                                                                                                                                                                                                            |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new_initialises_identity_and_timestamps`         | `AthleteData::new(123, 6, 0, 1_700_000_000.0, 100.0)` exposes `athlete_id == 123`, `course_id == 6`, `sport == 0`, `wt_offset == 1_700_000_000.0`, `created == updated == internal_created == internal_updated == internal_accessed == 100.0`, `bucket.start == 100.0`. |
| `touch_updates_internal_accessed`                 | `touch(now + 5.0)` sets only `internal_accessed`.                                                                                                                                                                                  |
| `record_update_advances_updated_and_accessed`     | `record_update(world_time + 5.0, now + 5.0)` advances `updated`, `internal_updated`, and `internal_accessed`; leaves `created` and `internal_created` alone.                                                                       |
| `ingest_routes_through_bucket`                    | `ad.ingest_power(t, w)` lands in `ad.bucket.power.max_value()`; same for hr / speed / cadence / draft.                                                                                                                             |
| `ingest_bumps_internal_accessed`                  | After `ingest_power(t + 5.0, 200.0)` with the stream advancing past a 1 s boundary (so the buffer flushes), `internal_accessed` reflects the latest call's `now`.                                                                  |

### 14.18 — 14.20 `AthleteRegistry` and GC — `tests/athlete_registry.rs`

| Test                                              | Asserts                                                                                                                                                                                                                                  |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `insert_get_remove_round_trip`                    | First `upsert(state, now)` creates a new `AthleteData`; second `upsert` for the same id returns the same record and updates timestamps via `record_update`.                                                                              |
| `gc_evicts_athletes_past_ttl`                     | Two athletes; the older one (`internal_accessed = now - 3601.0`) is evicted by `registry.gc(now)`; the younger one (`now - 3599.0`) survives.                                                                                            |
| `gc_no_op_when_nothing_expired`                   | All athletes within the TTL → `gc` is idempotent (no eviction, no allocation surprise).                                                                                                                                                  |
| `gc_evicts_groups_past_ttl`                       | Two `GroupMeta` entries; the older (`accessed = now - 91.0`) is evicted; the younger (`now - 89.0`) survives. Groups whose `accessed` is within the TTL are unaffected even when the athletes referenced by them have been GC'd already. |
| `gc_evicts_athletes_and_groups_in_one_pass`       | Mixed expiry: one athlete and one group expired. `gc(now)` evicts both in one pass and returns `(athletes_dropped, groups_dropped) == (1, 1)`.                                                                                           |

### 14.21 Recorded-stream parity — `tests/stream_parity.rs`

A captured ride (5–15 minutes, 1 Hz telemetry covering all five
signals) replayed through `DataBucket::ingest_*` must produce, at
end-of-stream, the same `avg / max / peaks / np_peaks` the JS oracle
records. Tolerance ≤ 1e-6 for `f64` comparisons.

### Reference vector strategy

`tests/fixtures/gen_athlete_vectors.mjs` is a Node script (run by hand,
checked in for reproducibility, **not** invoked from CI) that:

1. Reads `(time, power, hr, speed, cadence, draft)` rows from a
   captured ride.
2. Imports `shared/sauce/{data,power}.mjs` and the
   `DataCollector` / `PowerDataCollector` classes (extracted from
   `stats.mjs:92-320` into a small standalone helper to keep the
   import surface narrow — `stats.mjs` itself pulls in too many
   dependencies for a parity oracle).
3. Replays the stream through one bucket per signal with the JS
   defaults from `stats.mjs:2697-2714`.
4. Writes `{ inputs: [...], outputs: { power: { ... }, hr: { ... }, ... } }`
   to JSON.

The script lives **inside** `crates/zwift-stats/tests/fixtures/` per
the no-`sauce4zwift`-runtime-dep rule (the script is not on any build
path; the committed JSON is what the Rust tests consume).

## Crate layout

```
crates/zwift-stats/
├── Cargo.toml          — unchanged
├── src/
│   ├── lib.rs          — adds `pub mod collector;` `pub mod data_bucket;` `pub mod athlete;` `pub mod periods;` and re-exports
│   ├── sample.rs       — unchanged
│   ├── rolling.rs      — adds full(), last_time(), reset()
│   ├── power.rs        — adds avg/active/elapsed/last_time/full/time_at/value_at/reset/entries/rolling() forwarders
│   ├── helpers.rs      — unchanged
│   ├── bucket.rs       — unchanged (OneSecondBucket; STEP 13)
│   ├── periods.rs      — DEFAULT_POWER_PERIODS, DEFAULT_LONG_PERIODS, MIN_WEIGHTED_POWER_PERIOD, ATHLETE_GC_TTL_SECS, GROUP_GC_TTL_SECS, GC_TICK_INTERVAL_SECS
│   ├── collector.rs    — Collector trait, DataCollector<R>, PowerDataCollector, PeakSnapshot, NpPeakSnapshot
│   ├── data_bucket.rs  — DataBucket
│   └── athlete.rs      — AthleteData, AthleteRegistry, GroupMeta
└── tests/
    ├── …existing STEP 13 tests…
    ├── rolling_full.rs
    ├── rolling_reset.rs
    ├── rolling_power_accessors.rs
    ├── collector.rs
    ├── power_collector.rs
    ├── data_bucket.rs
    ├── athlete_data.rs
    ├── athlete_registry.rs
    ├── stream_parity.rs
    └── fixtures/
        ├── gen_athlete_vectors.mjs
        └── athlete_stream.json
```

Every public item is re-exported from `lib.rs` so callers
`use zwift_stats::{DataCollector, PowerDataCollector, DataBucket,
AthleteData, AthleteRegistry};` without navigating internal module
paths.

## Public API surface (proposed)

### Constants (`periods`)

```rust
pub const DEFAULT_POWER_PERIODS: &[f64] = &[5.0, 15.0, 60.0, 300.0, 1200.0, 3600.0];
pub const DEFAULT_LONG_PERIODS:  &[f64] = &[60.0, 300.0, 1200.0, 3600.0];
pub const MIN_WEIGHTED_POWER_PERIOD: f64 = 300.0;

pub const ATHLETE_GC_TTL_SECS:  f64 = 3600.0;
pub const GROUP_GC_TTL_SECS:    f64 = 90.0;
pub const GC_TICK_INTERVAL_SECS: f64 = 62.768; // stats.mjs:3553
```

### `Collector` trait (`collector`)

```rust
/// What a `DataCollector` needs from its inner rolling type. Implemented
/// for both `RollingAverage` and `RollingPower` so `DataCollector` can be
/// generic.
pub trait Collector: Clone {
    fn new_with_period(period: Option<f64>, opts: RollingAverageOptions) -> Self;
    fn add(&mut self, ts: f64, value: f64, active: Option<bool>);
    fn avg(&self, active: Option<bool>) -> Option<f64>;
    fn last_time(&self) -> Option<f64>;
    fn full(&self, offt: usize) -> bool;
    fn reset(&mut self);
    fn ideal_gap(&self) -> f64; // returns the configured ideal_gap (default 1.0)
}
```

Note that `Collector::add` takes raw `f64`, not `Sample` — the inner
implementation is responsible for wrapping in `Sample::Value(v)`. This
keeps `DataCollector` callers free of the `Sample` enum.

### `DataCollector<R: Collector>` (`collector`)

```rust
pub struct PeakSnapshot {
    pub period:    f64,
    pub snap_value: f64,
    pub snap_time: f64,
    pub roll:      // a clone of the rolling at the moment of the peak
}

pub struct DataCollector<R: Collector> {
    primary:     R,
    periodized:  Vec<PeriodizedEntry<R>>,
    max_value:   f64,
    round:       bool,
    /* 1 s buffer */
    buf_start: f64, buf_end: f64, buf_sum: f64, buf_len: u32,
}

pub struct PeriodizedEntry<R> {
    pub period: f64,
    pub roll:   R,
    pub peak:   Option<PeakSnapshot>,
}

impl<R: Collector> DataCollector<R> {
    pub fn new(periods: &[f64], opts: DataCollectorOptions) -> Self;
    pub fn add(&mut self, time: f64, value: f64) -> usize;  // count of newly-flushed samples
    pub fn flush(&mut self) -> usize;
    pub fn primary(&self) -> &R;
    pub fn periodized(&self) -> &[PeriodizedEntry<R>];
    pub fn max_value(&self) -> f64;
    pub fn peaks(&self) -> Vec<Option<PeakSnapshot>>;       // one entry per period
    pub fn clone_reset(&self) -> Self;
    pub fn clone_continue(&self) -> Self;
}

pub struct DataCollectorOptions {
    pub ideal_gap:    f64,
    pub max_gap:      f64,
    pub active:       bool,
    pub ignore_zeros: bool,
    pub round:        bool,
}
```

### `PowerDataCollector` (`collector`)

```rust
pub struct NpPeakSnapshot { pub period: f64, pub snap_value: f64, pub snap_time: f64, pub roll: RollingPower }

pub struct PowerDataCollector {
    inner:                DataCollector<RollingPower>,
    np_periodized_offt:   usize,
    peak_np:              Vec<Option<NpPeakSnapshot>>, // same length as inner.periodized
}

impl PowerDataCollector {
    pub fn new(periods: &[f64], opts: DataCollectorOptions) -> Self; // forces inline_np = true on inner
    pub fn add(&mut self, time: f64, watts: f64) -> usize;
    pub fn primary(&self) -> &RollingPower;
    pub fn periodized(&self) -> &[PeriodizedEntry<RollingPower>];
    pub fn max_value(&self) -> f64;
    pub fn peaks(&self) -> Vec<Option<PeakSnapshot>>;
    pub fn np_peaks(&self) -> &[Option<NpPeakSnapshot>];
    pub fn clone_reset(&self) -> Self;
    pub fn clone_continue(&self) -> Self;
}
```

### `DataBucket` (`data_bucket`)

```rust
pub struct DataBucket {
    pub start:        f64,

    pub coffee_time:  f64,
    pub work_time:    f64,
    pub follow_time:  f64,
    pub solo_time:    f64,

    pub work_kj:      f64,
    pub follow_kj:    f64,
    pub solo_kj:      f64,

    pub power:    PowerDataCollector,
    pub speed:    DataCollector<RollingAverage>,
    pub hr:       DataCollector<RollingAverage>,
    pub cadence:  DataCollector<RollingAverage>,
    pub draft:    DataCollector<RollingPower>,
}

impl DataBucket {
    pub fn new(start: f64) -> Self;
    pub fn ingest_power  (&mut self, time: f64, watts:  f64);
    pub fn ingest_hr     (&mut self, time: f64, bpm:    f64);
    pub fn ingest_speed  (&mut self, time: f64, mps:    f64);
    pub fn ingest_cadence(&mut self, time: f64, rpm:    f64);
    pub fn ingest_draft  (&mut self, time: f64, draft:  f64);
    pub fn clone_reset   (&self) -> Self;
    pub fn clone_continue(&self) -> Self;
}
```

### Signal table (matches `stats.mjs:2697-2714`)

| Signal  | Inner type        | Periods                          | `ignore_zeros` | `round` | Notes                                       |
| ------- | ----------------- | -------------------------------- | -------------- | ------- | ------------------------------------------- |
| power   | `RollingPower`    | `[5,15,60,300,1200,3600]`        | false          | true    | inline NP; uses `PowerDataCollector` for NP peaks |
| speed   | `RollingAverage`  | `[60,300,1200,3600]`             | true           | false   |                                             |
| hr      | `RollingAverage`  | `[60,300,1200,3600]`             | true           | true    |                                             |
| cadence | `RollingAverage`  | `[]`                             | true           | true    | no peak fan-out (matches JS)                |
| draft   | `RollingPower`    | `[60,300,1200,3600]`             | false          | true    | uses inline NP machinery but NP is not exposed for draft (regular `DataCollector<RollingPower>`, not `PowerDataCollector`) |

### `AthleteData` (`athlete`)

```rust
pub struct AthleteData {
    pub athlete_id:    u32,
    pub course_id:    u32,
    pub sport:        u8,
    pub created:      f64,
    pub updated:      f64,
    pub wt_offset:    f64,
    pub distance_offset: f64,
    pub internal_created:  f64,
    pub internal_updated:  f64,
    pub internal_accessed: f64,
    pub most_recent_state: Option<MostRecentState>,
    pub bucket:       DataBucket,
    // STEP 15+: wBal, timeInPowerZones, smoothGrade, streams, roadHistory,
    // STEP 15+: lapSlices, eventSlices, segmentSlices, activeSegments
}

pub struct MostRecentState {
    pub world_time: f64,
    pub speed:      f64,
    pub power:      f64,
    pub heartrate:  u16,
    pub cadence:    u16,
    pub draft:      f64,
    pub distance:   f64,
    pub altitude:   f64,
    // Other PlayerState fields are added as they are needed; STEP 14 only
    // pins the ones the parity tests reach for.
}

impl AthleteData {
    pub fn new(athlete_id: u32, course_id: u32, sport: u8, world_time: f64, now: f64) -> Self;
    pub fn touch(&mut self, now: f64);
    pub fn record_update(&mut self, world_time: f64, now: f64);
    pub fn ingest_power  (&mut self, now: f64, time: f64, watts:  f64);
    pub fn ingest_hr     (&mut self, now: f64, time: f64, bpm:    f64);
    pub fn ingest_speed  (&mut self, now: f64, time: f64, mps:    f64);
    pub fn ingest_cadence(&mut self, now: f64, time: f64, rpm:    f64);
    pub fn ingest_draft  (&mut self, now: f64, time: f64, draft:  f64);
}
```

### `AthleteRegistry` and `GroupMeta` (`athlete`)

```rust
pub struct GroupMeta {
    pub id:       u32,
    pub accessed: f64,
    // identity_set, etc., land in STEP 15 (see STEP 15 "Inputs deferred from STEP 14")
}

pub struct AthleteRegistry {
    athletes: HashMap<u32, AthleteData>,
    groups:   HashMap<u32, GroupMeta>,
}

pub struct GcReport { pub athletes_dropped: usize, pub groups_dropped: usize }

impl AthleteRegistry {
    pub fn new() -> Self;

    pub fn upsert(&mut self, athlete_id: u32, course_id: u32, sport: u8,
                  world_time: f64, now: f64) -> &mut AthleteData;
    pub fn get(&self, id: u32) -> Option<&AthleteData>;
    pub fn get_mut(&mut self, id: u32) -> Option<&mut AthleteData>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &AthleteData)>;

    pub fn touch_group(&mut self, id: u32, now: f64);                // upsert-or-touch
    pub fn group(&self, id: u32) -> Option<&GroupMeta>;
    pub fn groups_len(&self) -> usize;

    pub fn gc(&mut self, now: f64) -> GcReport;
}
```

### Errors

There are no fallible APIs in this step. Every operation either
returns the value or `Option<…>`. The same posture as STEP 13.

## Acceptance criteria

- `cargo test -p zwift-stats` is green from a clean checkout.
- Every checklist item 14.1 – 14.21 has at least one test and at
  least one production-code change (or a recorded "no change needed"
  in the as-built notes).
- `tests/stream_parity.rs` passes to ≤ 1e-6 against the checked-in JS
  oracle JSON.
- No `unsafe`. No `unwrap` outside test code (`expect("invariant: …")`
  with a stated invariant is acceptable for state-machine assertions
  inside `_resize_periodized` / `_update_periodized_peaks`).
- SPDX header `// SPDX-License-Identifier: AGPL-3.0-only` at the top
  of every new `.rs` file.
- No new dependencies. The existing dev-dependency on `serde_json`
  (added in STEP 13 for the parity fixtures) covers the new fixture
  load path.

## Open verification points

These are claims that should be confirmed before declaring the step
complete. None block tests; the implementation can be written and
tested against either choice. Record any decision in the as-built
notes appended to this file.

1. **GC tick interval.** The original stub said "GC ticks at 10 s",
   but `stats.mjs:3553` actually uses 62 768 ms. The Rust port
   defaults to that interval (exposed as `GC_TICK_INTERVAL_SECS =
   62.768`). The 10 s figure may have been a typo for the
   `_zwiftMetaRefresh` value at `stats.mjs:3565`. Decide whether to
   keep 62.768 s or honour the stub before STEP 17 wires the daemon
   loop.

2. **Periodized clone fan-out: independent rolls vs shared backing
   storage.** The JS `RollingAverage::clone({period})` shares
   `_times` / `_values` and forks only the index pointers. STEP 13
   chose to copy on clone, so STEP 14's `DataCollector` will push
   each flushed sample into every clone independently. Cost: the
   gap-fill computation runs N+1 times per push (one primary, N
   periodized). For peak fan-outs of 4 / 6, this is negligible at
   1 Hz; reconfirm under load in STEP 19. The alternative
   (`Arc<Vec<f64>>` with copy-on-clone semantics) is left for a
   later optimisation if the parity tests' wall-clock cost is too
   high.

3. **`Collector` trait vs concrete `RollingAverage`.** The plan
   above introduces a `Collector` trait so `DataCollector<R>` can be
   generic over `RollingAverage` and `RollingPower`. The alternative
   (two non-generic struct types `DataCollector` and
   `PowerDataCollector` with shared fields by composition) avoids
   the trait but duplicates the buffer / max-value logic. The trait
   route is preferred but if it produces awkward bounds when
   `PowerDataCollector` overrides peak-update behaviour, fall back
   to the duplicated-struct variant — record the decision and the
   reason.

4. **`DataCollector` peak comparison.** The JS uses `>=` so a
   constant stream's peak `snap_time` advances on every push
   (`stats.mjs:185-189`). The Rust port mirrors this. Pin it in a
   test rather than leaving it as a folkloric detail.

5. **`PowerDataCollector` for `draft`.** The JS uses
   `new DataCollector(Sauce.power.RollingPower, longPeriods, {round: true})`
   — that is, a regular `DataCollector` with `RollingPower` as the
   inner type, *not* a `PowerDataCollector`. So draft does not get
   NP peaks. The Rust port matches this: `bucket.draft` is
   `DataCollector<RollingPower>`, not `PowerDataCollector`. Pin
   in `default_construction_matches_js_signals`.

6. **`MostRecentState` shape.** The JS `mostRecentState` is the
   raw protobuf `PlayerState` object. STEP 14 only needs the fields
   the parity tests probe; the rest are added as later steps reach
   for them. The struct lives in `athlete.rs` because it is purely
   an internal cache, not a wire type. If a later step needs the
   full `PlayerState`, the cleanest move is to make
   `MostRecentState` an alias for the `zwift-proto` type at that
   point — STEP 14 stays proto-free.

7. **Peak snapshot storage cost.** Each peak snapshot clones the
   entire periodized rolling at the moment of the peak (matches
   JS). For a 3 600 s window at 1 Hz this is 3 600 `f64` pairs ≈
   58 KB per snapshot, per athlete, per signal. With 100 athletes
   × 5 signals × 6 periods, worst-case footprint is ≈ 174 MB —
   acceptable but worth flagging. If STEP 19 measures pressure,
   the lighter `{snap_value, snap_time}`-only snapshot is a viable
   downgrade since the cloned roll is read only by analysis-page
   features that ranchero v1 does not implement.

## Design decisions worth pre-committing

- **`DataCollector` is generic, `PowerDataCollector` is concrete.**
  `DataCollector<R: Collector>` covers HR / speed / cadence / draft;
  `PowerDataCollector` is a concrete wrapper around
  `DataCollector<RollingPower>` that adds the NP peak overlay. JS's
  inheritance pattern (`PowerDataCollector extends DataCollector`)
  becomes composition in Rust.
- **Trait method `Collector::add(time, f64, active)` takes raw
  `f64`.** The `Sample` enum stays an internal detail of the
  rolling primitives; consumers (STEP 14 onwards) work in `f64`.
  The trait's `add` impl wraps in `Sample::Value(v)` before calling
  the inner `RollingAverage::add`.
- **`DataCollector::add` returns the number of newly-flushed
  samples** (matches the JS return convention at `stats.mjs:159` —
  `return this.roll._length - len`). Callers that do not care
  ignore the return; future callers (STEP 15 segment timer) will
  use it to know when a sample landed.
- **Pure crate, no async.** Matches STEP 13's posture. The async
  glue (cron-driving `gc()` every 62.768 s, routing real
  `PlayerState` decodes into `ingest_*`) lives in the daemon at
  STEP 17. STEP 14 ships only the synchronous core.
- **No proto types in `zwift-stats`.** `MostRecentState` is a
  plain Rust struct with the fields STEP 14 probes; it is not a
  re-export of the prost-generated `PlayerState`. STEP 17 owns the
  proto → stats translation; this keeps the `zwift-stats` crate's
  dependency tree narrow and its tests fast.
- **Tests live in `tests/`.** Project convention: every crate has
  integration tests only.
- **Float comparison policy.** `approx::abs_diff_eq!` with
  `epsilon = 1e-9` for hand-derived vectors, `epsilon = 1e-6` for
  parity vectors against the JS oracle.

## Wiring into the workspace

- No `Cargo.toml` change needed at this step: `zwift-stats` is
  already a member; STEP 14 introduces no new dependencies.
- The root `ranchero` crate gains a `zwift-stats = { path = "..." }`
  dependency only when STEP 17 wires the daemon. STEP 14 itself
  ships no CLI surface and no daemon integration.
- License header `// SPDX-License-Identifier: AGPL-3.0-only` at the
  top of every new `.rs` file (matches the rest of the workspace).
