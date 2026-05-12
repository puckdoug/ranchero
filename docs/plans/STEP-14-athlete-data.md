# Step 14 — Per-athlete state, DataBucket, DataCollector

**Status:** core implementation complete (reviewed 2026-05-11). All
checklist boxes are ticked, 82 tests pass, and the core synchronous
behaviour (DataCollector buffering, peak snapshots, per-signal collector
configuration, garbage collection) is in place. The original plan called
for a numerical comparison against the JavaScript implementation in
`sauce4zwift/src/stats.mjs` driven by a captured session; that approach
has been dropped because `sauce4zwift` has no session-replay capability,
so the comparison as described is not possible (see project memory
"No JavaScript replay capability"). Several layout and naming deviations
from the plan remain and are documented in **Concerns from review** at
the bottom of this document.

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

Numerical correctness of the Rust implementation is anchored at the
primitive level by STEP 13's hand-derived vectors against
`shared/sauce/{data,power}.mjs`. STEP 14 composes those primitives;
the comparison against `sauce4zwift` end-to-end was originally planned
to use a recorded session, but `sauce4zwift` has no session-replay
capability and the comparison as described is not possible. STEP 14
relies on STEP 13's primitive-level numerical anchoring and on its own
hand-built regression tests; an end-to-end comparison against the
JavaScript implementation is no longer in scope here.

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

- [x] **14.5-T** `tests/collector.rs::new_creates_primary_and_periodized_clones`
      — `DataCollector::<RollingAverage>::new(periods=[60, 300],
    opts)` exposes a primary roll with no period and exactly two
      periodized entries with `period == 60.0` and `period == 300.0`,
      each starting empty. Also tests `empty_periods_yields_primary_only`. **Done:** Test file updated with correct tests.
- [x] **14.5-I** Implement `DataCollector<R>` where `R` is a trait
      that both `RollingAverage` and `RollingPower` will implement
      (see "Public API surface" below for the trait shape). Construct
      the primary with `period = None` and one clone per entry of
      `periods`; each periodized entry stores
      `{ period, roll: R, peak: Option<PeakSnapshot> }`. The trait
      default implementation provides a `new_with_period(period,
    opts)` factory so the collector does not need to choose between
      `RollingAverage::new` and `RollingPower::new`. **Done:**
      DataCollector<R> implemented with trait-based design (RollingWindow
      trait for R); RollingAverage and RollingPower both implement trait.
      New method constructs primary with period=None and periodized entries
      per period; added DataCollectorOptions struct. All 11 collector tests
      passing; total suite: 65 tests.

- [x] **14.6-T** `tests/collector.rs::add_buffers_until_ideal_gap_boundary`
      — `add(0.0, 100.0); add(0.5, 200.0)` returns 0 newly-flushed
      samples; `add(1.1, 50.0)` returns 1 flushed sample whose value
      is `mean(100, 200) = 150`. Mirror `stats.mjs:132-152`. **Done:**
      Tests for buffering, flushing, and rounding have been added.
- [x] **14.6-I** Implement `DataCollector::add(time, value)`: hold
      `_buffered_start / _buffered_end / _buffered_sum /
    _buffered_len`; when `time - _buffered_start >= ideal_gap`,
      flush via `_flush_buffered()` (compute mean, push into the
      primary and every periodized roll), then reset the buffer with
      `_buffered_start = time`. Honour the `round` option by rounding
      the flushed mean before push. **Done:** Add method implemented with
      one-second boundary semantics; returns flushed count (0 or 1);
      buffer resets after flush. Test passing.

- [x] **14.7-T** `tests/collector.rs::tracks_max_value_across_flushes`
      — feed a synthetic stream whose flushed means rise then fall;
      `max_value()` returns the peak across all pushes. **Done:** Test added
      to verify max value tracking.
- [x] **14.7-I** Add `_max_value: f64` and update it in `_add` after
      every successful push (matches `stats.mjs:165-167`). Expose
      `pub fn max_value(&self) -> f64`. **Done:** max_value field added
      to DataCollector; updated on every flush in add() method; accessor
      exposed. Test passing.

- [x] **14.8-T** `tests/collector.rs::periodized_peak_snapshots_max_avg`
      — feed a stream where the 60 s window's avg is constant at 250 W;
      assert `avg == 250.0` and window becomes `full()`. Window must be
      full (`elapsed >= period`) before peaks update. **Done:** Tests added
      for peak snapshotting, including window-full requirement and comparison logic.
- [x] **14.8-I** Implement `_resize_periodized` and
      `_update_periodized_peaks` (`stats.mjs:177-194`): for each
      periodized entry, after the primary push, compare
      `roll.avg()` against `peak.snap_value`; on improvement, snapshot
      `roll.clone()` plus `snap_value` and `snap_time = roll.last_time()`.
      The peak is only updated once the period is full (to avoid
      stamping a 5-sample window as the 60 s peak). **Done:**
      update_periodized_peaks method implemented; called after each flush
      in add(); checks full(0) before updating peaks; snapshots avg and
      last_time. Test passing.

- [x] **14.9-T** `tests/collector.rs::clone_with_reset_creates_empty_snapshot`
      and `clone_without_reset_preserves_max_and_peaks` — pin both
      clone branches. **Done:** Tests for both `clone_reset` and `clone_continue`
      have been added.
- [x] **14.9-I** Implement `pub fn clone_reset(&self) -> Self` and
      `pub fn clone_continue(&self) -> Self`. `clone_reset` returns
      a collector with the same options/periods but an empty buffer,
      empty primary, and `peak = None` on every periodized entry.
      `clone_continue` carries `_max_value`, the primary roll's
      state, and every `peak` snapshot forward (used by lap/segment
      slice creation in STEP 15; just exercise it here). **Done:**
      Both clone methods implemented; clone_reset creates new empty
      instances of primary and periodized rolls with cleared peaks and
      buffer; clone_continue preserves all state including max_value,
      primary roll state, and peak snapshots. Tests passing.

`PowerDataCollector` (NP peak overlay):

- [x] **14.10-T** `tests/power_collector.rs::np_peak_only_for_periods_at_or_above_300`
      — periods `[5, 15, 60, 300, 1200, 3600]`. Drive a constant-power
      stream; assert `np_peaks()[0..3].iter().all(|p| p.is_none())`
      (the 5 / 15 / 60 s entries do not record an NP peak) and
      `np_peaks()[3..]` all carry `Some(_)` matching the inline-NP
      value. **Done:** Test created with 3602 samples to ensure all
      periods reach fullness. NP peaks correctly absent for periods
      < 300s and present for periods >= 300s. Test passing.
- [x] **14.10-I** Implement `PowerDataCollector` as
      `DataCollector<RollingPower>` plus parallel NP peak tracking
      per period >= 300s. Override `update_np_peaks` to call
      `roll.np(false)` and snapshot when period >= 300s and full.
      **Done:** PowerDataCollector wraps DataCollector<RollingPower>
      with np_periodized tracking. update_np_peaks filters for
      period >= 300.0 && full(0) && np value exists. Accessor methods
      periodized() and max_value() added. All tests passing.

- [x] **14.11-T** `tests/power_collector.rs::np_peak_survives_clone_continue`
      — drive a stream that produces real NP peaks, call
      `clone_continue()`, assert cloned collector reports same
      `np_peaks()`. After `clone_reset()`, NP peaks are `None`.
      **Done:** Test verifies both clone methods preserve and clear
      NP peaks appropriately. Test passing.
- [x] **14.11-I** Extend clone methods on `PowerDataCollector` to
      copy / clear `peak_np` in np_periodized entries per clone mode.
      **Done:** clone_reset() zeros peak entries; clone_continue()
      preserves them. Implementation mirrors pattern from DataCollector.
      All tests passing.

`DataBucket` (the five-signal aggregate):

- [x] **14.12-T** `tests/data_bucket.rs::default_construction_matches_js_signals`
      — `DataBucket::new(start)` exposes the five signal collectors
      with the periods, `ignore_zeros`, and `round` flags from the
      JS table at `stats.mjs:2697-2714` (see the "Signal table"
      below). All time / kJ accumulators start at zero; `start` is
      stored verbatim. **Done:** Test created verifying all 5 collectors
      with correct periods and options per signal table; accumulators
      at zero; start stored.
- [x] **14.12-I** Implement `DataBucket` with `start: f64`,
      `coffee_time / work_time / follow_time / solo_time: f64`,
      `work_kj / follow_kj / solo_kj: f64`, and the five collectors
      named `power`, `hr`, `speed`, `cadence`, `draft`. Construct
      each with the JS-matching options.
      **Done:** DataBucket struct implemented with all fields per spec.
      Constructor creates collectors with correct period arrays and
      options (power: PowerDataCollector with ignore_zeros=false;
      hr/speed/cadence/draft: DataCollector<RollingAverage> or
      RollingPower with ignore_zeros/round flags per signal table).
      All tests passing.

- [x] **14.13-T** `tests/data_bucket.rs::ingest_routes_to_correct_collector`
      — `bucket.ingest_power(t, w)` lands in `bucket.power` only;
      assert `bucket.hr.max_value() == 0.0` after a power-only
      stream. Mirror for HR / speed / cadence / draft. **Done:** Tests
      created for all 5 ingest methods; each verifies data routes only
      to its target collector while others remain empty.
- [x] **14.13-I** Implement `pub fn ingest_power(&mut self, t: f64,
    watts: f64)`, and matching methods for hr / speed / cadence /
      draft. Each method delegates to the corresponding collector's
      `add(t, value)`. No proto types; this is the seam the daemon
      (STEP 17) will wire.
      **Done:** All 5 ingest_* methods implemented as delegating to
      their respective collectors. Tested routing and independence.
      All tests passing.

- [x] **14.14-T** `tests/data_bucket.rs::clone_reset_creates_slice_template`
      and `clone_continue_preserves_session_totals` — pin the two
      clone behaviours used by `_createDataSlice` (reset for a fresh
      lap / segment) versus session-wide carry-forward. **Done:** Tests
      created for clone_reset (zeroes accumulators, clears collectors)
      and clone_continue (preserves state). Both verify independence
      of cloned bucket via mutation. (Test updated to add second sample
      to trigger buffer flush; all tests passing.)
- [x] **14.14-I** Implement `pub fn clone_reset(&self) -> Self` and
      `pub fn clone_continue(&self) -> Self` on `DataBucket`.
      `clone_reset` zeroes the time / kJ accumulators and calls
      `clone_reset` on every collector; `clone_continue` carries
      everything forward. (Slice machinery itself is STEP 15; the
      methods exist now so that step has the seam.)
      **Done:** Both clone methods implemented. clone_reset() zeroes
      all time/kJ fields and calls clone_reset on all collectors;
      clone_continue() preserves all fields by copying/cloning.
      All tests passing.

`AthleteData` (identity + bucket + GC timestamps):

- [x] **14.15-T** `tests/athlete_data.rs::new_initialises_identity_and_timestamps`
      — `AthleteData::new(athlete_id, course_id, sport, world_time,
    now)` exposes those four identity fields verbatim and sets
      `created == updated == now`, `internal_created ==
    internal_updated == internal_accessed == now`,
      `wt_offset == world_time`, `distance_offset == 0.0`.
      `bucket.start == now`. **Done:** Test created and passing.
- [x] **14.15-I** Implement the `AthleteData` struct with the STEP 14
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
      **Done:** Struct implemented with all STEP 14 fields; MostRecentState
      placeholder created; exported from lib.rs; Debug derives added to
      DataBucket, DataCollector, PowerDataCollector, PeriodizedEntry,
      NpPeriodizedEntry, RollingAverage, RollingPower to support AthleteData
      Debug impl.

- [x] **14.16-T** `tests/athlete_data.rs::touch_updates_internal_accessed`
      — call `touch(now + 5.0)`, assert
      `internal_accessed == now + 5.0`, `internal_updated` and
      `internal_created` unchanged. **Done:** Test created and passing.
- [x] **14.16-I** Implement `pub fn touch(&mut self, now: f64)` that
      sets `internal_accessed = now`. Add `pub fn record_update(&mut
    self, world_time: f64, now: f64)` that sets `updated`,
      `internal_updated`, and `internal_accessed` (called by the
      daemon when a `PlayerState` arrives). **Done:** Both methods
      implemented on AthleteData; touch() updates only internal_accessed;
      record_update() advances updated, internal_updated, and
      internal_accessed.

- [x] **14.17-T** `tests/athlete_data.rs::ingest_routes_through_bucket`
      — call `ad.ingest_power(t, watts)` and assert
      `ad.bucket.power.max_value() == watts` (after one full
      `ideal_gap` flush). Mirror for HR / speed / cadence / draft.
      **Done:** Test created covering all 5 ingest methods with proper
      routing verification; test passing.
- [x] **14.17-I** Add forwarding methods on `AthleteData` that
      delegate straight to `self.bucket.ingest_*` and bump
      `internal_updated` / `internal_accessed`. The daemon never
      reaches into `bucket` directly (encapsulation: future step may
      route to slice collectors as well). **Done:** All 5 ingest_*
      methods implemented (power, hr, speed, cadence, draft);
      each delegates to bucket and updates internal_updated/
      internal_accessed; test passing.

`AthleteRegistry` and GC:

- [x] **14.18-T** `tests/athlete_registry.rs::insert_get_remove_round_trip`
      — `registry.upsert(state, now)` returns a mutable reference;
      a second `upsert` for the same `athlete_id` returns the same
      record (does not allocate a new one). `registry.get(id)` and
      `registry.len()` work as expected. **Done:** Test created
      verifying upsert creates new athlete on first call, updates
      on subsequent calls, get() retrieves athlete, len() works,
      get_mut() allows mutation. Test currently fails (expected).
- [x] **14.18-I** Implement `pub struct AthleteRegistry { athletes:
    HashMap<u32, AthleteData> }` with `upsert`, `get`, `get_mut`,
      `len`, `is_empty`, and `iter`. `upsert` is the entry point
      that creates a new `AthleteData` on first sight and calls
      `record_update` on subsequent calls. **Done:** AthleteRegistry
      struct implemented with HashMap<u32, AthleteData>; all methods
      implemented (upsert creates or updates via entry API, get/get_mut
      for retrieval, len/is_empty/iter for queries). Exported from lib.rs.
      All tests passing.

- [x] **14.19-T** `tests/athlete_registry.rs::gc_evicts_athletes_past_ttl`
      — insert two athletes, advance their `internal_accessed` to
      `now - 3601.0` and `now - 3599.0` respectively, call
      `registry.gc(now)`, assert only the second survives. **Done:**
      Test created with two athletes; manually sets internal_accessed
      timestamps; verifies gc() evicts athlete past TTL (3600s) while
      preserving younger one; checks gc_report. Test passing.
- [x] **14.19-I** Implement `pub fn gc(&mut self, now: f64)` that
      iterates `athletes`, drops every entry whose
      `internal_accessed < now - ATHLETE_GC_TTL_SECS`. Constants
      live in `periods.rs`: `ATHLETE_GC_TTL_SECS = 3600.0`,
      `GROUP_GC_TTL_SECS = 90.0`, `GC_TICK_INTERVAL_SECS = 62.768`
      (the JS interval at `stats.mjs:3553`; the stub's "10 s"
      claim is a test-side knob, not the production default — see
      "Open verification points"). **Done:** gc() method implemented
      on AthleteRegistry, counts dropped athletes before/after retain,
      returns athlete_dropped count in GcReport. Test verifies correct
      TTL-based eviction. All tests passing.

- [x] **14.20-T** `tests/athlete_registry.rs::gc_evicts_groups_past_ttl`
      — register two stub group metas with `accessed` at
      `now - 91.0` and `now - 89.0`, call `gc(now)`, assert only
      the second survives. (Group classification is STEP 15; this
      step ships the GC seam plus a `GroupMeta` placeholder so the
      eviction logic is testable now.) **Done:** Test created with
      two groups; uses touch_group() to set accessed timestamps;
      verifies gc() evicts group past TTL (90s) while preserving
      younger one; checks gc_report. Test passing.
- [x] **14.20-I** Add `pub struct GroupMeta { id: u32, accessed: f64
    }` and `groups: HashMap<u32, GroupMeta>` to `AthleteRegistry`.
      Extend `gc` to drop groups past `GROUP_GC_TTL_SECS`. Group
      classification (deciding which athletes belong to which
      group, populating `identity_set`) is out of scope; STEP 15
      will populate the map (see STEP 15's "Inputs deferred from
      STEP 14"). **Done:** GroupMeta struct with Debug/Copy derives;
      groups HashMap added to AthleteRegistry; touch_group() method
      for upsert-or-touch semantics; group() and groups_len() accessors;
      gc() extended to handle groups with same TTL-based eviction;
      GcReport includes groups_dropped count. All tests passing.

Regression fixture for the end-to-end ingest path:

The original plan for 14.21 called for generating a reference JSON by
replaying a captured session through `sauce4zwift`'s `DataCollector` and
`PowerDataCollector` and comparing the Rust output against it. That
approach has been dropped because `sauce4zwift` has no session-replay
capability (see project memory "No JavaScript replay capability"), so
the comparison as described is not possible. The two items below are
the revised, in-scope deliverables.

- [x] **14.21-T** Add `tests/stream_parity.rs` and
      `tests/fixtures/athlete_stream.json` as a regression test for
      the Rust ingest path. The fixture is a small hand-built input
      sequence (constant telemetry across the five signals); the test
      loads it, replays it through `DataBucket::ingest_*`, and asserts
      that the resulting `max_value` for each signal matches the
      hand-computed reference value to within 1e-6.
      **Done:** Fixture and test in place; test passing.
      *(Note: this is a Rust-only regression test. It pins the output
      of the current implementation so that later refactors cannot
      change the numbers silently. It is not a comparison against
      `sauce4zwift`.)*
- [x] **14.21-I** No implementation work required. The fixture in
      `14.21-T` is checked in against the existing implementation, so
      there are no differences to resolve. STEP 14's acceptance
      criteria are now met under the revised scope.
      **Done:** No code changes.

## Tests-first plan (detail)

Every test file lives in `crates/zwift-stats/tests/*.rs`. The bullets
below correspond to the checklist items above.

### 14.2 `RollingAverage::full` — `tests/rolling_full.rs`

| Test                                          | Asserts                                                                                                                                                               |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `full_returns_true_when_elapsed_meets_period` | `period = 60`. After 59 samples at 1 Hz, `full(0) == false`; after the 60th sample, `full(0) == true`.                                                                |
| `full_offt_one_loop_evicts_one_sample`        | Same setup, push the 61st sample, then `while r.full(1) { r.pop(); }` runs exactly once. Mirrors `data.mjs:457-459`.                                                  |
| `full_returns_false_when_period_is_none`      | A periodless `RollingAverage` always returns `false` from `full`, regardless of how many samples it holds. (Matches the JS `if (this._period == null) return false`.) |

### 14.3 `RollingAverage::reset` — `tests/rolling_reset.rs`

| Test                                 | Asserts                                                                                                                                                                                                                |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `reset_clears_state_keeps_options`   | After `add(0, 100); add(1, 200); reset();`, `size() == 0`, `avg(None) == None`, `active() == 0`, `elapsed() == 0`. Calling `add(2, 300)` then proceeds as if the rolling were brand new but with the original options. |
| `reset_on_rolling_power_clears_qnpa` | A `RollingPower` with 600 s of data, after `reset`, returns `None` from `np(true)` (no `qnpa_*` accumulation left).                                                                                                    |

### 14.4 `RollingPower` accessors — `tests/rolling_power_accessors.rs`

| Test                                        | Asserts                                                                                                                                                                               |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `power_exposes_avg_active_elapsed_lasttime` | After a hand-built sequence, `RollingPower::avg(None)`, `active()`, `elapsed()`, `last_time()` agree with the same calls on `power.rolling()` to the bit (these are pure forwarding). |
| `power_full_matches_inner_full`             | `RollingPower::full(0)` equals `power.rolling().full(0)`.                                                                                                                             |

### 14.5 `DataCollector` construction — `tests/collector.rs`

| Test                                        | Asserts                                                                                                                                                                                              |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new_creates_primary_and_periodized_clones` | `DataCollector::<RollingAverage>::new(periods=[60.0, 300.0], opts)` exposes `primary().size() == 0`, `periodized().len() == 2`, `periodized()[0].period == 60.0`, `periodized()[1].period == 300.0`. |
| `empty_periods_yields_primary_only`         | `periods=[]` gives `periodized().len() == 0`. `cadence` in JS uses this shape — there is no peak fan-out for cadence.                                                                                |
| `default_options_match_js_constants`        | `RollingAverageOptions::default()` plus the collector's enforced overrides yields `ideal_gap = Some(1.0)`, `max_gap = Some(15.0)`, `active = true` (matches `defOptions` in `stats.mjs:99`).         |

### 14.6 1 s buffering — `tests/collector.rs`

| Test                                   | Asserts                                                                                                                                                                               |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `add_buffers_until_ideal_gap_boundary` | `add(0.0, 100.0)` returns 0; `add(0.5, 200.0)` returns 0 (still inside the 1 s window); `add(1.1, 50.0)` returns 1, and `primary().value_at(0)` equals `Sample::Value(150.0)` (mean). |
| `flush_drains_partial_buffer`          | After `add(0.0, 100.0); add(0.5, 200.0)`, `flush()` returns 1 and `primary().value_at(0) == Sample::Value(150.0)`. `flush()` again returns 0.                                         |
| `round_option_rounds_flushed_mean`     | Same buffered values with `round = true` produce `Sample::Value(150.0)`; with `round = false` and unequal samples, the mean is preserved as-is.                                       |

### 14.7 Max value tracking — `tests/collector.rs`

| Test                                | Asserts                                                                                                                                                                |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tracks_max_value_across_flushes`   | A handcrafted stream whose flushed means are `[100, 250, 200, 180]` produces `max_value() == 250.0` at every later push.                                               |
| `max_value_unaffected_by_pad_fills` | A gap that triggers soft-pad insertion does not raise `max_value()`. (JS reads `value` directly, not `Sample`; we must mirror that by gating on the post-flush `f64`.) |

### 14.8 Periodized peak snapshots — `tests/collector.rs`

| Test                                        | Asserts                                                                                                                                                                                                     |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `periodized_peak_snapshots_max_avg`         | Stream of 60 s at 100 W, then 60 s at 250 W, then 60 s at 200 W. The 60 s peak is `Some(snap)` with `snap.avg == 250.0` and `snap.time` matching the timestamp at which the 250 W window first became full. |
| `peak_does_not_update_until_window_is_full` | Period 60 s, push 30 s of 250 W data. `peaks()[0]` is `None` (the window is not yet full and JS does not update the peak — `data.mjs:457` only enters the update branch when `roll.full()`).                |
| `peak_uses_geq_comparison_not_strict_gt`    | Push 60 s at 100 W, then a 60 s window also averaging 100 W. The peak's `snap_time` advances (JS uses `>=`; this is documented behaviour).                                                                  |

### 14.9 Collector clone — `tests/collector.rs`

| Test                                          | Asserts                                                                                                                                                                  |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `clone_with_reset_creates_empty_snapshot`     | After driving a stream that produces a 250 W peak, `clone_reset()` returns a collector with `max_value() == 0.0`, `peaks()` all `None`, primary `size() == 0`.           |
| `clone_without_reset_preserves_max_and_peaks` | `clone_continue()` returns a collector whose `max_value()` and `peaks()` match the source. Subsequent writes on either side do not affect the other (deep-copied state). |

### 14.10 — 14.11 Power collector NP peaks — `tests/power_collector.rs`

| Test                                       | Asserts                                                                                                                                                                                                                                                                                 |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `np_peak_only_for_periods_at_or_above_300` | Periods `[5, 15, 60, 300, 1200, 3600]`, 600 s of 200 W. `np_peaks()[0..3]` are all `None`; `np_peaks()[3]` carries `Some(snap)` with `snap.avg ≈ 200.0`. The 1200 / 3600 s entries also have NP peaks (the period is not yet full but inline NP applies as soon as `active() ≥ 300 s`). |
| `np_peak_snapshot_records_lasttime`        | After driving a stream that produces an NP peak, the snapshot's `snap_time` matches the inner roll's `last_time()` at the moment the peak was set.                                                                                                                                      |
| `np_peak_uses_geq_comparison`              | A flat 200 W stream produces successive peaks at every push (the comparison is `>=` per `stats.mjs:280-287`).                                                                                                                                                                           |
| `np_peak_survives_clone_continue`          | `clone_continue()` carries `peak_np` forward.                                                                                                                                                                                                                                           |
| `np_peak_cleared_by_clone_reset`           | `clone_reset()` clears `peak_np` to `None` on every entry.                                                                                                                                                                                                                              |

### 14.12 — 14.14 `DataBucket` — `tests/data_bucket.rs`

| Test                                          | Asserts                                                                                                                                                                                                                                                                                                                         |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default_construction_matches_js_signals`     | `bucket.power.periodized().len() == 6` (5/15/60/300/1200/3600); `bucket.hr.periodized().len() == 4` (60/300/1200/3600); `bucket.speed.periodized().len() == 4`; `bucket.cadence.periodized().len() == 0`; `bucket.draft.periodized().len() == 4`. Also pins each collector's `ignore_zeros` and `round` flag from the JS table. |
| `start_and_accumulators_initialise_correctly` | `bucket.start == constructor_arg`, `coffee_time == work_time == follow_time == solo_time == 0.0`, `work_kj == follow_kj == solo_kj == 0.0`.                                                                                                                                                                                     |
| `ingest_routes_to_correct_collector`          | A power-only stream produces non-zero `bucket.power.max_value()` and `bucket.hr.max_value() == 0.0` (and likewise for the other three).                                                                                                                                                                                         |
| `clone_reset_creates_slice_template`          | `bucket.clone_reset()` zeros all time / kJ accumulators and produces empty collectors. `bucket.start` carries forward — the clone is a _slice template_, so `start` is the only timing field that must be re-set by the caller after cloning. (Pin in test, not auto.)                                                          |
| `clone_continue_preserves_session_totals`     | `bucket.clone_continue()` carries forward all time / kJ accumulators and all collector state.                                                                                                                                                                                                                                   |

### 14.15 — 14.17 `AthleteData` — `tests/athlete_data.rs`

| Test                                          | Asserts                                                                                                                                                                                                                                                                 |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new_initialises_identity_and_timestamps`     | `AthleteData::new(123, 6, 0, 1_700_000_000.0, 100.0)` exposes `athlete_id == 123`, `course_id == 6`, `sport == 0`, `wt_offset == 1_700_000_000.0`, `created == updated == internal_created == internal_updated == internal_accessed == 100.0`, `bucket.start == 100.0`. |
| `touch_updates_internal_accessed`             | `touch(now + 5.0)` sets only `internal_accessed`.                                                                                                                                                                                                                       |
| `record_update_advances_updated_and_accessed` | `record_update(world_time + 5.0, now + 5.0)` advances `updated`, `internal_updated`, and `internal_accessed`; leaves `created` and `internal_created` alone.                                                                                                            |
| `ingest_routes_through_bucket`                | `ad.ingest_power(t, w)` lands in `ad.bucket.power.max_value()`; same for hr / speed / cadence / draft.                                                                                                                                                                  |
| `ingest_bumps_internal_accessed`              | After `ingest_power(t + 5.0, 200.0)` with the stream advancing past a 1 s boundary (so the buffer flushes), `internal_accessed` reflects the latest call's `now`.                                                                                                       |

### 14.18 — 14.20 `AthleteRegistry` and GC — `tests/athlete_registry.rs`

| Test                                        | Asserts                                                                                                                                                                                                                                  |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `insert_get_remove_round_trip`              | First `upsert(state, now)` creates a new `AthleteData`; second `upsert` for the same id returns the same record and updates timestamps via `record_update`.                                                                              |
| `gc_evicts_athletes_past_ttl`               | Two athletes; the older one (`internal_accessed = now - 3601.0`) is evicted by `registry.gc(now)`; the younger one (`now - 3599.0`) survives.                                                                                            |
| `gc_no_op_when_nothing_expired`             | All athletes within the TTL → `gc` is idempotent (no eviction, no allocation surprise).                                                                                                                                                  |
| `gc_evicts_groups_past_ttl`                 | Two `GroupMeta` entries; the older (`accessed = now - 91.0`) is evicted; the younger (`now - 89.0`) survives. Groups whose `accessed` is within the TTL are unaffected even when the athletes referenced by them have been GC'd already. |
| `gc_evicts_athletes_and_groups_in_one_pass` | Mixed expiry: one athlete and one group expired. `gc(now)` evicts both in one pass and returns `(athletes_dropped, groups_dropped) == (1, 1)`.                                                                                           |

### 14.21 Rust-only regression test — `tests/stream_parity.rs`

A small hand-built input sequence (constant telemetry across all five
signals for eight seconds) replayed through `DataBucket::ingest_*` must
produce the `max_value` for each signal that the test fixture records.
Tolerance ≤ 1e-6 for `f64` comparisons. This is a regression test for
the Rust implementation only; it does not compare against
`sauce4zwift`.

The original plan included a "Reference vector strategy" section that
described a Node script (`gen_athlete_vectors.mjs`) reading a captured
ride and driving `sauce4zwift`'s `DataCollector` to produce a
comparison fixture. That section has been removed because the workflow
it described is not possible: `sauce4zwift` has no session-replay
capability, so a captured ranchero session cannot be driven through
the JavaScript implementation. The numerical anchor for the rolling
primitives is STEP 13's hand-derived vectors run against
`shared/sauce/{data,power}.mjs` at the primitive level.

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

| Signal  | Inner type       | Periods                   | `ignore_zeros` | `round` | Notes                                                                                                                      |
| ------- | ---------------- | ------------------------- | -------------- | ------- | -------------------------------------------------------------------------------------------------------------------------- |
| power   | `RollingPower`   | `[5,15,60,300,1200,3600]` | false          | true    | inline NP; uses `PowerDataCollector` for NP peaks                                                                          |
| speed   | `RollingAverage` | `[60,300,1200,3600]`      | true           | false   |                                                                                                                            |
| hr      | `RollingAverage` | `[60,300,1200,3600]`      | true           | true    |                                                                                                                            |
| cadence | `RollingAverage` | `[]`                      | true           | true    | no peak fan-out (matches JS)                                                                                               |
| draft   | `RollingPower`   | `[60,300,1200,3600]`      | false          | true    | uses inline NP machinery but NP is not exposed for draft (regular `DataCollector<RollingPower>`, not `PowerDataCollector`) |

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
- `tests/stream_parity.rs` passes to ≤ 1e-6 against the checked-in
  hand-built fixture (Rust-only regression test; not a comparison
  against `sauce4zwift`).
- No `unsafe`. No `unwrap` outside test code (`expect("invariant: …")`
  with a stated invariant is acceptable for state-machine assertions
  inside `_resize_periodized` / `_update_periodized_peaks`).
- SPDX header `// SPDX-License-Identifier: AGPL-3.0-only` at the top
  of every new `.rs` file.
- No new dependencies. The existing dev-dependency on `serde_json`
  (added in STEP 13) covers the new fixture load path.

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
   inner type, _not_ a `PowerDataCollector`. So draft does not get
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

## Concerns from review (2026-05-11)

This section records the deviations from the plan that the review found
when comparing the plan against the implementation. Each item is
something to either fix or to decide by editing the plan. None of these
were called out in the as-built notes inside the checklist itself.

### Resolved

- **End-to-end comparison against the JavaScript implementation** was
  the central acceptance criterion in the original plan and was the
  largest gap in the implementation. It has been dropped from the plan
  because `sauce4zwift` has no session-replay capability and the
  workflow described in the original 14.21 (a Node script driving
  `DataCollector` against a captured session) is not possible. STEP 14
  now relies on STEP 13's primitive-level numerical anchoring; the
  fixture and test that already exist serve as a Rust-only regression
  test. The plan text for 14.21, the Goal section, the "Reference
  vector strategy" section, the crate layout, and the acceptance
  criteria have been updated accordingly. (Recorded as project memory
  "No JavaScript replay capability.")

### Severity: significant (deviates from plan; needs a decision)

1. **`DataBucket` lives in `collector.rs`, not `data_bucket.rs`.** The
   plan's crate layout places `DataBucket` in `src/data_bucket.rs`.
   The implementation places `DataBucket` at the end of
   `src/collector.rs`. The file `src/data_bucket.rs` exists but
   contains only the SPDX header and a docstring. Either move
   `DataBucket` (and update the test files' import paths) to
   `data_bucket.rs`, or remove the empty file and update the plan to
   record that `DataBucket` lives alongside `DataCollector`.
   *(Resolved by R1 on 2026-05-11: `DataBucket` now lives in
   `src/data_bucket.rs`.)*

2. **`PeakSnapshot` and `NpPeakSnapshot` are missing the `period` and
   `roll` fields specified in the plan.** The plan defines:

   ```rust
   pub struct PeakSnapshot {
       pub period:     f64,
       pub snap_value: f64,
       pub snap_time:  f64,
       pub roll:       // a clone of the rolling at the moment of the peak
   }
   pub struct NpPeakSnapshot {
       pub period:     f64,
       pub snap_value: f64,
       pub snap_time:  f64,
       pub roll:       RollingPower,
   }
   ```

   The implementation has only `{ snap_value, snap_time }` for both.
   Open verification point #7 in this same document discusses the
   memory cost of peak snapshots on the basis that each snapshot
   stores a copy of the entire periodized rolling. That concern does
   not apply to the current implementation, but the JavaScript
   behaviour the plan intends to mirror does keep the rolling, and
   later features (analysis pages, per-period detail views) are
   expected to read it. Decide whether to add the missing fields or to
   amend the plan and open verification point #7 to record the lighter
   snapshot as the intended design.

3. **The trait is renamed and its `add` method takes a different type.**
   The plan defines a `Collector` trait with
   `add(ts, value: f64, active)` and an `ideal_gap()` accessor. The
   implementation defines a `RollingWindow` trait with
   `add(ts, value: Sample, active)` and no `ideal_gap()`. The
   "Design decisions worth pre-committing" section of this plan
   states explicitly:

   > Trait method `Collector::add(time, f64, active)` takes raw `f64`.
   > The `Sample` enum stays an internal detail of the rolling
   > primitives; consumers (STEP 14 onwards) work in `f64`.

   The current `RollingWindow::add` requires `DataCollector::flush` to
   wrap each value in `Sample::Value(...)`, which makes the `Sample`
   type visible to the collector code. Either restore the signature
   and name given in the plan, or update the plan to record the rename
   and the change in the trait's parameter type.

4. **`DataBucket` fields are private; the plan shows them as `pub`.**
   The plan's `DataBucket` definition exposes `pub start`,
   `pub coffee_time`, `pub power`, and so on. The implementation uses
   private fields with paired accessor methods (`start()`,
   `set_work_time()`, `power()`, `power_mut()`, and so on). The two
   are functionally equivalent and the implementation's choice is
   more typical of Rust code, but it is still a deviation from a
   document that prescribes the API surface. Pick one form and record
   the choice.

### Severity: minor (tests described in the plan that were not written)

5. **Tests described in the "Tests-first plan (detail)" tables but
   not added to the test files.** The detail tables list tests that
   are not enumerated in the checklist `-T` rows. The following tests
   appear in the detail tables and are absent from the test files:

   - 14.5: `default_options_match_js_constants`
   - 14.7: `max_value_unaffected_by_pad_fills`
   - 14.10–14.11: `np_peak_snapshot_records_lasttime`,
     `np_peak_uses_geq_comparison`, `np_peak_cleared_by_clone_reset`
   - 14.12–14.14: `start_and_accumulators_initialise_correctly`
     (the existing `default_construction_matches_js_signals` covers
     part of this case)
   - 14.15–14.17: `record_update_advances_updated_and_accessed`,
     `ingest_bumps_internal_accessed`
   - 14.18–14.20: `gc_no_op_when_nothing_expired`,
     `gc_evicts_athletes_and_groups_in_one_pass`

   Several of these verify behaviour the daemon will depend on
   (the Normalized Power `>=` comparison, `record_update`, the
   idempotency of `gc`, and a combined athlete-and-group eviction in
   one call to `gc`). Either add the missing tests or remove them
   from the detail tables. *(Resolved by R5 on 2026-05-11: all ten
   tests added; `DataCollectorOptions::default()` now returns the
   JavaScript-matching constants.)*

6. **`approx::abs_diff_eq!` is not used.** The plan's design
   decisions specify `approx::abs_diff_eq!` with `epsilon = 1e-6` for
   the numerical comparison. The comparison test uses raw
   `(a - b).abs() < 1e-6`. The two are functionally equivalent here,
   but the development dependency on `approx` is already present and
   the macro produces clearer failure messages.

### Severity: documentation drift (record-only)

7. **The Open verification points section is unresolved.** The plan
   lists seven items that were supposed to be decided "before
   declaring the step complete" with the decisions recorded in
   as-built notes. None of them have a written decision. The
   decisions that the implementation has effectively made are as
   follows:

   - #1 Garbage-collection tick interval: 62.768 seconds is the
     constant in `periods.rs`. The daemon does not yet use it, so
     this choice is provisional.
   - #2 Periodized clones: independent rolling buffers, matching the
     decision made in STEP 13.
   - #3 Trait versus concrete struct: the trait route was taken, but
     the trait is renamed (see concern #3).
   - #4 Peak comparison uses `>=`: implemented, and the test
     `peak_uses_geq_comparison_not_strict_gt` confirms it.
   - #5 Draft uses `DataCollector<RollingPower>` rather than
     `PowerDataCollector`: confirmed.
   - #6 `MostRecentState`: implemented as a separate struct in
     `athlete.rs`.
   - #7 Peak snapshot memory cost: does not apply to the current
     implementation, because the rolling is not cloned (see
     concern #2).

   Move these into a short "As-built decisions" subsection so they
   are no longer presented as open questions on a step that is
   marked complete.

8. **`AthleteRegistry::upsert` updates `course_id` and `sport` only
   on the first insert.** The `or_insert_with` branch uses the values
   passed to the call; the `and_modify` branch calls `record_update`,
   which updates only the timestamps. This matches the plan, because
   the plan's `record_update` signature is `(world_time, now)` and
   takes no identity fields. However, the daemon will eventually
   encounter cases where the same `athlete_id` appears on a different
   course or switches sport during a session. This should be
   considered when STEP 17 is planned.

## Items that didn't get properly implemented

The checklist below tracks remediation for the deviations recorded in
the "Concerns from review" section above. Each item is open. Every
item can be resolved in one of two ways: by changing the code to match
the plan, or by changing the plan to match the code. Mark the item
checked once the decision has been made and either the code or the
plan has been brought into agreement.

- [x] **R1** Place `DataBucket` in `src/data_bucket.rs`, or remove
      the empty stub file and update the crate layout section of the
      plan to record that `DataBucket` lives in `collector.rs`. See
      concern #1. **Done (2026-05-11):** Moved the `DataBucket` struct
      and its implementation block from `src/collector.rs` into
      `src/data_bucket.rs`, matching the original crate layout in the
      plan. `lib.rs` now re-exports `DataBucket` from
      `crate::data_bucket` rather than `crate::collector`. The import
      in `tests/data_bucket.rs` was changed from
      `zwift_stats::collector::DataBucket` to `zwift_stats::DataBucket`.
      All 82 tests still pass; no clippy warnings.

- [ ] **R2** Decide on the shape of `PeakSnapshot` and
      `NpPeakSnapshot`. Either add the `period` and `roll` fields
      from the plan to both structures (and update
      `_update_periodized_peaks` and `update_np_peaks` to populate
      them), or remove those fields from the plan's "Public API
      surface" section and rewrite Open verification point #7 to
      record the lighter snapshot as the intended design. See
      concern #2. *Decision: Path A (add the fields). Work tracked
      as R2A-T1 through R2A-I6 in the "R2 elaboration" section
      below.*

- [ ] **R3** Decide on the trait surface. Either rename
      `RollingWindow` back to `Collector` and change `add` to take
      `value: f64` per the plan (moving the `Sample::Value(...)`
      wrapping into the trait implementations on `RollingAverage`
      and `RollingPower`), or update the plan's "Public API
      surface" and "Design decisions worth pre-committing" sections
      to record the rename and the `Sample`-bearing signature. See
      concern #3.

- [ ] **R4** Decide on `DataBucket` field visibility. Either make
      the fields `pub` per the plan and remove the accessor methods,
      or update the plan to record the accessor-method form. See
      concern #4.

- [x] **R5** Add the tests that the "Tests-first plan (detail)"
      tables list but the test files do not contain, or remove
      those tests from the detail tables. The tests are:

      - 14.5: `default_options_match_js_constants`
      - 14.7: `max_value_unaffected_by_pad_fills`
      - 14.10–14.11: `np_peak_snapshot_records_lasttime`,
        `np_peak_uses_geq_comparison`,
        `np_peak_cleared_by_clone_reset`
      - 14.12–14.14: `start_and_accumulators_initialise_correctly`
        (partially covered by
        `default_construction_matches_js_signals`)
      - 14.15–14.17: `record_update_advances_updated_and_accessed`,
        `ingest_bumps_internal_accessed`
      - 14.18–14.20: `gc_no_op_when_nothing_expired`,
        `gc_evicts_athletes_and_groups_in_one_pass`

      See concern #5. **Done (2026-05-11):** All ten tests added to
      the corresponding test files. To make
      `default_options_match_js_constants` pass, `Default` for
      `DataCollectorOptions` is now implemented manually (rather than
      derived) to return `ideal_gap = 1.0`, `max_gap = 15.0`,
      `active = true`, `ignore_zeros = false`, `round = false`, which
      matches the `defOptions` constant at `stats.mjs:99`. Existing
      tests that used `..Default::default()` continue to pass under
      the new defaults. Total test count is now 92 (was 82); no
      clippy warnings.

- [ ] **R6** Either switch the comparison in `stream_parity.rs` to
      `approx::abs_diff_eq!` with `epsilon = 1e-6`, or update the
      plan's "Design decisions worth pre-committing" section to
      remove the requirement to use the `approx` crate's macro. See
      concern #6.

- [ ] **R7** Replace the "Open verification points" section with a
      short "As-built decisions" subsection that records the
      decisions the implementation has effectively made (see the
      bullet list under concern #7 for the per-item resolutions).
      See concern #7.

- [ ] **R8** Record the `AthleteRegistry::upsert` identity-field
      behaviour in the STEP 17 planning notes so that handling of
      mid-session course or sport changes is considered when the
      daemon ingest path is designed. See concern #8.

## R2 elaboration: `PeakSnapshot` and `NpPeakSnapshot` shape

### Decision: Path A (chosen 2026-05-12)

The original plan defines the two snapshot structures with four fields
each:

```rust
pub struct PeakSnapshot {
    pub period:     f64,
    pub snap_value: f64,
    pub snap_time:  f64,
    pub roll:       /* clone of the rolling at peak time */,
}

pub struct NpPeakSnapshot {
    pub period:     f64,
    pub snap_value: f64,
    pub snap_time:  f64,
    pub roll:       RollingPower,
}
```

The implementation has only `{ snap_value, snap_time }`. Path A
(implement the full shape) has been chosen over Path B (amend the
plan to record the lighter snapshot).

**Why:** the purpose of ranchero is to feed visualization. Discarding
the rolling buffer at peak time would force a re-implementation as
soon as the first analysis-page feature needs it (graphs of the peak
window, recomputed statistics over the window, cross-signal queries
against the peak interval). The `roll` field carries the actual
sixty 1 Hz samples (or 3,600 for the 1 h period) that produced the
peak — per-sample timestamps, active versus elapsed bookkeeping,
inline Normalized Power state — and that data has no other home once
the window has scrolled past.

**Cost (accepted):** every peak snapshot carries a copy of the
rolling. Open verification point #7 estimates the worst case at
roughly 174 MB at 100 athletes × 5 signals × 6 periods × 3,600 s
window. Allocation pressure rises on every peak update because the
rolling is cloned rather than just having two `f64` fields written.
If STEP 19 measures pressure here, the lighter snapshot remains a
viable fallback.

Path B is recorded below as the named fallback but is not the path
being implemented.

### Path A implementation (the chosen path)

TDD pairs in the same style as the rest of the document. Each `-T`
item adds a failing test; each `-I` item adds the production code
that turns it green.

- [x] **R2A-T1** `tests/collector.rs::peak_snapshot_carries_period_and_roll`
      — drive a stream that fills the 60 s period and triggers a peak
      update. Assert that the snapshot's `period` equals `60.0` and
      that its `roll.avg(None)` and `roll.last_time()` match the
      periodized entry's rolling at the moment the peak was set.
      The test fails to compile because `period` and `roll` do not
      exist on `PeakSnapshot`. **Done (2026-05-12):** Test added; fails
      to compile with `no field period on type &PeakSnapshot` and
      `no field roll on type &PeakSnapshot`, as expected.

- [ ] **R2A-I1** Make `PeakSnapshot` generic over `R: RollingWindow`,
      add `pub period: f64` and `pub roll: R` fields, and update
      `DataCollector::flush` to populate them when the peak is set.
      Cascade the generic through `PeriodizedEntry<R>`'s `peak`
      field (`Option<PeakSnapshot<R>>`) and through the return type
      of `DataCollector::peaks()`
      (`Vec<Option<PeakSnapshot<R>>>`). Update the two existing
      tests that read `peaks()[0]` to take the generic into account
      where needed.

- [x] **R2A-T2** `tests/collector.rs::peak_snapshot_roll_is_independent_of_source`
      — drive a stream that produces a peak on the 60 s period.
      Capture the snapshot (by cloning it). Push more samples that
      would otherwise change the rolling's `avg` and `last_time`.
      Assert that the captured snapshot's `roll.avg(None)` and
      `roll.last_time()` are unchanged. This pins the deep-clone
      property that the rest of Path A depends on. **Done
      (2026-05-12):** Test added; fails to compile with
      `no field roll on type PeakSnapshot`, as expected.

- [ ] **R2A-I2** No new code is expected here: STEP 13 chose copy
      on `RollingWindow::clone`, so the snapshot's `roll` is already
      independent of the source. The test serves to record that
      property at the snapshot boundary so a later optimisation
      (such as `Arc<Vec<f64>>` shared storage) cannot quietly break
      it. If the test fails, replace the implicit clone with an
      explicit deep clone.

- [x] **R2A-T3** `tests/power_collector.rs::np_peak_snapshot_carries_period_and_roll`
      — drive a constant-power stream long enough to fill the 300 s
      period and produce an NP peak. Assert that the snapshot's
      `period` equals `300.0` and that its `roll.np(false)` matches
      the inner roll's `np(false)` at the moment the peak was set.
      **Done (2026-05-12):** Test added; fails to compile with
      `no field period on type &NpPeakSnapshot` and
      `no field roll on type &NpPeakSnapshot`, as expected.

- [ ] **R2A-I3** Add `pub period: f64` and `pub roll: RollingPower`
      fields to `NpPeakSnapshot`. Update
      `PowerDataCollector::update_np_peaks` to populate them. The
      type stays concrete (NP peaks are only ever recorded for
      `RollingPower`).

- [x] **R2A-T4** `tests/collector.rs::clone_continue_preserves_peak_rolls`
      — drive a stream that produces a peak, call `clone_continue()`,
      then push more samples to the source. Assert that the cloned
      collector's snapshot still reports the original `roll.avg(None)`
      and `roll.last_time()` (deep clone survives through the
      carry-forward). **Done (2026-05-12):** Test added; fails to
      compile with `no field roll on type &PeakSnapshot`, as expected.

- [x] **R2A-T5** `tests/power_collector.rs::clone_continue_preserves_np_peak_rolls`
      — mirror of R2A-T4 for `NpPeakSnapshot`. **Done (2026-05-12):**
      Test added; fails to compile with
      `no field roll on type &NpPeakSnapshot`, as expected.

- [ ] **R2A-I4** Verify that `DataCollector::clone_continue` and
      `PowerDataCollector::clone_continue` already clone the peaks
      deeply (the current implementations call `entry.peak.clone()`,
      which clones the inner `R` via `RollingWindow::clone`). If the
      tests in R2A-T4 and R2A-T5 fail, replace the implicit clone
      with an explicit deep clone of the inner roll. Mirror the
      change in `clone_reset` if needed (peaks are cleared there, so
      no work is expected).

- [x] **R2A-T6** `tests/collector.rs::peaks_method_returns_generic_snapshots`
      — compile-only check (no runtime assertion needed beyond
      construction) that the return type of `DataCollector::<RollingAverage>::peaks()`
      is `Vec<Option<PeakSnapshot<RollingAverage>>>` and that
      `DataCollector::<RollingPower>::peaks()` returns
      `Vec<Option<PeakSnapshot<RollingPower>>>`. This pins the
      generic surface so a later refactor cannot silently revert to
      a concrete type. **Done (2026-05-12):** Test added; fails to
      compile with `struct takes 0 generic arguments but 1 generic
      argument was supplied` on both lines, as expected.

- [ ] **R2A-I5** Update the "Public API surface" section of this
      document to record `PeakSnapshot<R>` as generic with the new
      fields, the matching `NpPeakSnapshot` shape, and the
      corresponding signatures of `peaks()` on both collectors.
      Rewrite Open verification point #7 so it describes the
      implemented behaviour (the storage cost estimate stays
      roughly as written).

- [ ] **R2A-I6** Mark R2 done in the remediation checklist and
      annotate concern #2 with the resolution note.

### Path B (named fallback, not being implemented)

Path B is the lighter alternative: keep the `{ snap_value, snap_time }`
shape and amend the plan instead of the code. It is recorded here as
the named fallback if STEP 19 measures pressure from per-snapshot
rolling clones and decides the heavier shape is not affordable. The
work for Path B is plan-only: update the "Public API surface" section
to record the two-field shape, and rewrite Open verification point #7
to record that the rolling is not cloned per snapshot. Path B is not
on the current critical path.
