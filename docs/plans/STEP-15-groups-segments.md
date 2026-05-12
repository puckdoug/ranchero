# Step 15 — Groups, laps, segments, W' balance, zones

## Goal

Build the higher-level analytic layer on top of STEP 14's per-athlete
record. Each item below adds a new accumulator, detector, or container
on `AthleteData`, plus the supporting pure types in `zwift-stats`:

- **`ZonesAccumulator`** — Z1..Z7 seconds for power (Coggan or polarized,
  optional overlapping sweetspot). Generic enough to also drive an HR
  variant later.
- **`WBalAccumulator`** — Froncioni / Skiba / Clarke differential W' balance,
  emitting one `wbal` sample per ingestion tick.
- **Event detection** — driven by `state.eventSubgroupId`. Opens / closes
  an event `DataSlice`, stamps event-privacy flags (`hidewbal`, `hideftp`,
  `hidethehud`).
- **Lap detection** — manual (`start_athlete_lap`) and automatic
  (`auto_lap_check`) by cumulative distance or time.
- **Segment detection** — `active_segment_check` walks current road
  segments against road history and opens / closes segment slices.
- **Group classification** — `compute_groups` (spec §5.5): greedy
  clustering by gap (2.0 s, or 0.8 s without draft), followed by
  Jaccard-based identity preservation across ticks (threshold `> 0.5`).
- **Slice machinery** — `DataSlice` struct plus the four containers
  (`lap_slices`, `event_slices`, `segment_slices`, `active_segments`)
  on `AthleteData`.
- **Road history** — the three-tier sliding window the segment and
  gap logic walk.
- **`PlayerStateView` trait** — a read-only accessor trait that lets
  STEP 15's detectors stay agnostic of whether the last-seen state is
  the hand-written `MostRecentState` struct (STEP 15 today) or the
  `zwift-proto` `PlayerState` (STEP 17, possibly). The struct gains
  the seven fields the new detectors reach for (`road_id`,
  `road_time`, `reverse`, `event_subgroup_id`, `group_id`, `time`,
  `event_distance`) and implements the trait. STEP 17 may later
  implement the trait directly on the proto type without touching
  the call sites.
- **Gap fields** — `gap`, `gap_distance`, `is_gap_est` on
  `AthleteData`, written by group classification.
- **`GroupMeta` enrichment** — `identity_set: HashSet<u32>` so
  Jaccard can run between ticks.

Numerical anchoring continues the STEP 14 posture: every accumulator
is anchored against hand-derived analytic values to ≤ 1e-9, every
JS-port behaviour is pinned by a Rust-only fixture at ≤ 1e-6. There
is no end-to-end JavaScript comparison (see project memory
"No JavaScript replay capability").

## Summary checklist

The list below is split into explicit TDD pairs. `-T` adds the listed
tests and observes them fail. `-I` adds the smallest production code
that turns them green. Do not advance to the next pair until the
current one is green; do not write code without a failing test
pinning the requirement first. A pair is "done" only when its `-T`
item produced a red test that the `-I` item then turned green.

Setup (no tests):

- [x] **15.1** Module skeleton. Add `src/zones.rs`, `src/wbal.rs`,
      `src/slice.rs`, `src/road_history.rs`, `src/groups.rs`,
      `src/laps.rs`, `src/segments.rs`, `src/events.rs` with empty
      types and SPDX headers; wire them into `lib.rs` behind
      `pub mod` declarations. `cargo test -p zwift-stats` stays
      green; no new tests yet.

`ZonesAccumulator` (power zones; HR-zone variant is parametric on the input):

- [x] **15.2-T** `tests/zones_definitions.rs::coggan_zones_at_ftp_250_match_js_table`,
      `polarized_zones_at_ftp_250`, `sweetspot_zone_fascat_and_coggan`.
- [x] **15.2-I** Add free functions `coggan_zones(ftp)`,
      `polarized_zones(ftp)`, `sweetspot_zone(ftp, kind)` in
      `src/zones.rs`. Return `Vec<Zone>` / `Zone` matching the JS
      tables exactly. `Zone::to` carries `Option<f64>` (`None` is
      the unbounded upper bound).
- [x] **15.3-T** `tests/zones_accumulator.rs::accumulate_credits_top_down_with_break_on_non_overlap`,
      `accumulate_continues_iteration_on_overlap_for_sweetspot`,
      `accumulate_handles_zero_and_top_bounds`,
      `accumulate_first_tick_yields_zero_elapsed`.
- [x] **15.3-I** Implement `ZonesAccumulator` in `src/zones.rs`:
      `configure(ftp, zones)`, `accumulate(time, value)`,
      `value()` returns `&[ZoneTime]`. Reproduces the JS
      reverse-iteration with `from < value <= to`, sorting
      overlap zones to the tail so the iteration hits them first
      and continues past them. First-tick elapsed is 0; subsequent
      tick adds `time - _time_offset`.
- [x] **15.4-T** `tests/zones_accumulator.rs::reset_clears_value_and_ftp`,
      `clone_continue_carries_state`, `clone_reset_starts_fresh`.
- [x] **15.4-I** Add `reset()`, `clone_reset()`, `clone_continue()`
      to `ZonesAccumulator`.

`Sample::Break.pad` type amendment (STEP 13 carry-over):

- [x] **15.5-T** `tests/sample_break.rs::break_pad_is_u32` — a
      compile-fence test that constructs `Sample::Break { pad: 5 }`
      and reads `pad` as a `u32`. Existing tests / call sites that
      construct or match on `Break.pad` must be updated to the new
      type as part of the change.
- [x] **15.5-I** Amend `src/sample.rs`: change
      `Sample::Break { pad: f64 }` to `Sample::Break { pad: u32 }`.
      This is a STEP 13 type touch-up; STEP 13's tests must
      continue to pass after the amendment. All `Break { pad: … }`
      construction sites (search the workspace) update to integer
      literals; any read sites that did arithmetic on `pad` switch
      to the integer-typed value. No semantic change — STEP 13
      always produced integral pad counts.

`WBalAccumulator` (CP + W'):

- [x] **15.6-T** `tests/wbal_recovery.rs::recovery_below_cp_uses_exponential_term`,
      `depletion_above_cp_uses_linear_term`,
      `clamp_at_wprime_does_not_exceed`,
      `wbal_can_go_negative_in_the_red`.
- [x] **15.6-I** Add `WBalAccumulator` in `src/wbal.rs`:
      `configure(cp, w_prime)`, `accumulate(time, sample)`,
      `value()`, `reset()`. Internal closure mirrors
      `makeIncWPrimeBalDifferential` (power.mjs:804-826).
- [x] **15.7-T** `tests/wbal_break.rs::break_sample_refills_until_full`,
      `break_sample_short_circuits_when_within_epsilon`.
- [x] **15.7-I** Handle `Sample::Break { pad }` in
      `WBalAccumulator::accumulate`: loop `pad` (a `u32` per 15.5)
      ticks, add `cp * (w_prime - w_bal) / w_prime` per tick,
      early-exit when `w_bal >= w_prime - 1e-6`.
- [x] **15.8-T** `tests/wbal_unconfigured.rs::unconfigured_yields_none`,
      `accumulator_clone_continue_carries_w_bal`.
- [x] **15.8-I** When `cp` or `w_prime` is `None`, every
      `accumulate` returns `None`. Add `clone_reset` /
      `clone_continue`.

`smooth_grade` helper (8-sample exponential weighted moving average):

- [x] **15.9-T** `tests/exp_weighted_avg.rs::seed_returns_seed_until_first_input`,
      `alpha_matches_js_for_size_8`,
      `successive_updates_track_em_a_formula`.
- [x] **15.9-I** Add `pub fn exp_weighted_avg(size: f64, seed: f64) -> ExpWeightedAvg`
      to `src/helpers.rs`. `ExpWeightedAvg::update(value) -> f64`,
      `ExpWeightedAvg::get() -> f64`, `ExpWeightedAvg::size() -> f64`.
      Matches `expWeightedAvg(size=2, seed=0)` (data.mjs:19-27).

`DataSlice` (snapshot wrapper around `DataBucket`):

- [x] **15.10-T** `tests/data_slice.rs::new_clones_bucket_reset_and_carries_identity`,
      `id_is_assigned_monotonically_per_athlete`,
      `id_carries_athlete_prefix_in_upper_32_bits`,
      `end_starts_none_and_can_be_stamped_once`,
      `slice_carries_course_and_sport_at_creation`.
- [x] **15.10-I** Add `DataSlice` in `src/slice.rs` with fields
      `id, start, end, course_id, sport, bucket` plus the
      segment / event-specific extension fields named in the
      Public API section. `DataSlice::new_from(ad: &mut
  AthleteData, start: f64)` calls `ad.bucket.clone_reset()`
      and pulls the next id from a per-`AthleteData` `u32`
      counter, then packs it as
      `((ad.athlete_id as u64) << 32) | (counter as u64)` so the
      id is globally unique across athletes without coordination.

`AthleteData` extension — supporting fields:

- [x] **15.11-T** `tests/athlete_data_extensions.rs::new_initialises_accumulators_and_streams`,
      `initial_lap_slice_has_clone_reset_bucket`.
- [x] **15.11-I** Add the following fields to `AthleteData`:
      `w_bal: WBalAccumulator`,
      `time_in_power_zones: ZonesAccumulator`,
      `smooth_grade: ExpWeightedAvg`,
      `streams: Streams`,
      `road_history: RoadHistory`,
      `lap_slices: Vec<DataSlice>`,
      `event_slices: Vec<DataSlice>`,
      `segment_slices: Vec<DataSlice>`,
      `active_segments: HashMap<u32, DataSlice>`,
      `gap: Option<f64>`, `gap_distance: Option<f64>`,
      `is_gap_est: bool`,
      `group_id: Option<u32>`,
      `event_subgroup: Option<EventSubgroup>`,
      `event_privacy: EventPrivacy`,
      `disabled_by_event: bool`,
      `event_start_pending: bool`,
      `auto_lap_mark: Option<f64>`.
      `AthleteData::new(...)` pushes the initial open lap slice
      (matches `_createAthleteData`, stats.mjs:2856).

`PlayerStateView` trait + `MostRecentState` extension:

- [x] **15.12-T** `tests/player_state_view.rs::most_recent_state_implements_view_trait`,
      `view_trait_exposes_road_event_and_group_fields`.
- [x] **15.12-I** Define `PlayerStateView` in `src/athlete.rs` as
      a read-only accessor trait. Extend `MostRecentState` with
      the new fields needed by STEP 15 (`lat`, `lng`, `road_id`,
      `road_time`, `reverse`, `event_subgroup_id`, `group_id`,
      `time`, `event_distance`) and implement `PlayerStateView`
      for it. Every STEP 15 detector that reads state takes `&dyn
  PlayerStateView` (or `impl PlayerStateView`) so STEP 17 can
      later implement the trait on the `zwift-proto::PlayerState`
      type without changing call sites.

`RoadHistory` (the three-tier ladder):

- [x] **15.13-T** `tests/road_history.rs::same_road_no_shift_appends_to_a`,
      `road_change_shifts_a_to_b_b_to_c`,
      `direction_change_shifts_when_delta_below_minus_001`,
      `direction_change_wipes_a_when_delta_in_minus_001_to_0`,
      `course_change_resets_b_and_c`,
      `first_state_seeds_aroad_without_shift`.
- [x] **15.13-I** Implement `RoadHistory::record(state, prev)`
      mirroring `_recordAthleteRoadHistory` (stats.mjs:3043-3084).
      Use a `road_sig(course_id, road_id, reverse)` free function
      defined alongside `RoadHistory`; the long-term home for
      route / segment tables is the `zwift-routes` crate (STEP 17
      wires it).

Event detection and privacy:

- [x] **15.14-T** `tests/event_detection.rs::new_subgroup_opens_slice_when_state_time_present`,
      `new_subgroup_defers_when_state_time_zero_and_sets_pending`,
      `same_subgroup_does_not_reopen_slice`,
      `falsy_subgroup_after_active_closes_slice`,
      `auto_end_by_distance_closes_slice`,
      `auto_end_by_wall_clock_closes_slice`,
      `behavior_auto_reset_resets_athlete_data_on_event_start`,
      `behavior_auto_lap_starts_a_lap_on_event_start_when_not_resetting`.
- [ ] **15.14-I** Implement `apply_event_state(ad, state,
  self_athlete_id, sg_lookup, behavior, now, wall_clock_ms)`
      in `src/events.rs`. `sg_lookup: &HashMap<u32,
  EventSubgroup>` is provided by the caller (the daemon,
      STEP 17, owns the subgroup cache). `behavior: EventBehavior`
      carries `auto_reset` and `auto_lap` flags. The function
      calls `trigger_event_start(...)`,
      `trigger_event_end(...)`, manages
      `event_start_pending`, and writes `event_privacy` /
      `disabled_by_event` flags. `trigger_event_start` mirrors
      `stats.mjs:2904-2939`: if `auto_reset` and the bucket has
      data, reset the athlete; else if `auto_lap` and the bucket
      has data, start a lap.
- [ ] **15.15-T** `tests/event_privacy.rs::self_athlete_skips_privacy_flags`,
      `non_self_hidewbal_sets_hide_w_bal`,
      `non_self_hideftp_sets_hide_ftp`,
      `hidethehud_sets_disabled_by_event`,
      `nooverlays_sets_disabled_by_event`.
- [ ] **15.15-I** Apply the four tag rules from
      `stats.mjs:2985-2989`: skip privacy assignment when
      `state.athlete_id == self_athlete_id`; otherwise stamp
      `event_privacy.hide_w_bal`, `event_privacy.hide_ftp`,
      `disabled_by_event` from `sg.all_tags`.

Manual and automatic laps:

- [ ] **15.16-T** `tests/laps.rs::start_athlete_lap_closes_open_slice_and_appends_new`,
      `start_athlete_lap_returns_new_slice_id`,
      `start_athlete_lap_clones_bucket_via_clone_reset`.
- [ ] **15.16-I** Implement `start_athlete_lap(ad: &mut
  AthleteData, now: f64) -> u64` in `src/laps.rs`. Stamp the
      current open lap's `end = Some(now)`, create a new
      `DataSlice` via `clone_reset`, push, return its id.
- [ ] **15.17-T** `tests/laps.rs::auto_lap_by_distance_threshold_triggers_at_each_interval`,
      `auto_lap_by_time_threshold_triggers_at_each_interval`,
      `auto_lap_mark_resets_on_course_change`,
      `auto_lap_first_call_seeds_mark_without_lapping`.
- [ ] **15.17-I** Implement `auto_lap_check(ad, state, cfg, now)
  -> bool` returning `true` when a lap was started.
      Mirrors `_autoLapCheck` (stats.mjs:3032-3041).

Segment detection:

- [ ] **15.18-T** `tests/segments.rs::start_within_first_5_percent_opens_slice`,
      `start_within_first_150m_overrides_5_percent_for_short_segments`,
      `outside_progress_does_not_open_slice`,
      `exit_after_open_stops_segment`,
      `multiple_concurrent_segments_tracked_independently`.
- [ ] **15.18-I** Implement `active_segment_check(ad, state, env,
  now)` in `src/segments.rs`. `env: &dyn SegmentLookup` is a
      trait with `road_segments(course_id, road_id, reverse) ->
  &[Segment]` and `segment(id) -> Option<&Segment>`; STEP 15
      ships a test-only in-memory implementation. The real
      table-backed implementation lands in `zwift-routes`
      (STEP 17).
- [ ] **15.19-T** `tests/segments.rs::stop_marks_incomplete_when_no_road_history`,
      `stop_marks_complete_when_long_segment_at_or_above_90_percent`,
      `stop_marks_incomplete_when_long_segment_below_90_percent`,
      `stop_thresholds_60_percent_for_400_to_1000m`,
      `stop_thresholds_25_percent_for_below_400m`,
      `stop_walks_road_history_a_b_c_until_match`.
- [ ] **15.19-I** Implement `stop_segment(...)` per
      `stats.mjs:1997-2045`. Walk `road_history` tiers, compute
      completion fraction over `(road_start, road_finish)`
      (mirroring reverse), apply the 0.90 / 0.60 / 0.25 threshold
      based on `segment.distance`.

Gap computation (`compare_road_positions`):

- [ ] **15.20-T** `tests/gap.rs::same_road_same_direction_uses_delta_rpct`,
      `same_road_negative_delta_marks_reversed`,
      `cross_tier_two_back_resolves_via_b_road`,
      `cross_tier_three_back_resolves_via_c_road`,
      `no_connection_returns_none`,
      `boundary_error_term_001_admits_near_matches`.
- [ ] **15.20-I** Implement `compare_road_positions(p1, p2, env)
  -> Option<RoadComparison>` in `src/road_history.rs`.
      `RoadGeometry` is a trait with `road_distance(road,
  start_pct, end_pct) -> f64` (test stub returns straight-line
      `(end - start) * road.length_metres`).
      `RoadComparison { world_time: f64, distance: f64,
  reversed: bool }`.
- [ ] **15.21-T** `tests/gap.rs::gap_field_set_from_world_time_delta_in_seconds`,
      `gap_negated_when_reversed_and_positive`,
      `gap_distance_signed_by_direction`,
      `is_gap_est_false_when_world_time_match`,
      `is_gap_est_true_when_world_time_missing`.
- [ ] **15.21-I** Add `apply_gap(ad, watching, env)` that fills
      the three `gap` / `gap_distance` / `is_gap_est` fields.
      Estimation fallback via
      `exp_weighted_avg(10, max(10, watching.speed))` is included.

Group classification:

- [ ] **15.22-T** `tests/groups.rs::singleton_riders_get_group_id_none`,
      `two_riders_within_2_second_gap_form_one_group`,
      `gap_above_2_seconds_splits_group`,
      `gap_above_0_8_without_draft_splits_group`,
      `gap_at_or_below_0_8_with_no_draft_keeps_group`.
- [ ] **15.22-I** Implement `compute_groups(nearby, watching_idx,
  prior_groups, next_id, now) -> Vec<Group>` in
      `src/groups.rs`. First pass: clump by gap thresholds
      (2.0 s, or 0.8 s when `draft == 0`).
- [ ] **15.23-T** `tests/groups.rs::aggregate_weight_skips_zero_weight_athletes`,
      `aggregate_power_and_draft_use_member_count`,
      `aggregate_speed_uses_median_not_mean`,
      `aggregate_heartrate_skips_none_entries`,
      `group_gap_is_zero_for_watching_group`,
      `group_gap_is_head_for_group_ahead_tail_for_group_behind`,
      `last_group_length_uses_head_and_tail_consistently_not_zero`.
- [ ] **15.23-I** Second pass of `compute_groups`: aggregate
      `weight / power / draft / heartrate / speed`, fill per-group
      `gap` and `is_gap_est` from the edge nearest watching.
      Compute `length_time` / `length_distance` consistently for
      every group, including the last one (fix the JS bug at
      `stats.mjs:4506-4509` which produces 0 for the last group).
- [ ] **15.24-T** `tests/groups_identity.rs::singleton_group_does_not_create_group_meta`,
      `multi_rider_group_creates_fresh_meta_when_no_prior_match`,
      `jaccard_above_0_5_reuses_prior_group_id`,
      `jaccard_exactly_0_5_creates_fresh_meta_strict_threshold`,
      `members_who_left_get_group_id_cleared_only_if_still_pointing_to_meta`,
      `prior_meta_used_once_per_tick_greedy_first_wins`.
- [ ] **15.24-I** Third pass: build `identity_set: HashSet<u32>`
      per multi-rider group, scan `prior_groups` for best Jaccard
      (skipping already-used metas), reuse if `> 0.5`, else mint a
      fresh id via `*next_id += 1`. Update outgoing
      `prior_groups` map; clear stale `group_id` on left-behind
      athletes; assign `group_id` to all members.

`GroupMeta` enrichment:

- [ ] **15.25-T** `tests/groups_identity.rs::group_meta_carries_identity_set`,
      `gc_drops_meta_past_ttl_with_identity_set_intact`.
- [ ] **15.25-I** Extend `GroupMeta` with
      `identity_set: HashSet<u32>`. The constructor used by
      `AthleteRegistry::touch_group` (seed-only path) initialises
      it to `HashSet::new()`; `compute_groups` is responsible for
      filling it before insertion into the prior-groups map.

Streams:

- [ ] **15.26-T** `tests/streams.rs::record_streams_appends_distance_altitude_latlng_per_tick`,
      `latlng_uses_custom_type_with_named_fields`,
      `wbal_sample_appended_per_tick_when_accumulator_configured`,
      `wbal_stream_carries_none_when_accumulator_unconfigured`.
- [ ] **15.26-I** Add the `Streams` struct in `src/streams.rs`
      along with a custom `LatLng` type (named fields, not a
      tuple — this is a Rust port, not a transliteration of the
      JS `[lat, lng]` array):
      `pub struct LatLng { pub lat: f64, pub lng: f64 }`.
      `Streams { pub distance: Vec<f64>, pub altitude: Vec<f64>,
  pub latlng: Vec<LatLng>, pub wbal: Vec<Option<f64>> }`.
      Add `AthleteData::record_streams(state)` that appends one
      entry per call.

Rust-only regression fixture:

- [ ] **15.27-T** Add `tests/step15_regression.rs` and
      `tests/fixtures/step15_session.json`. The fixture is a
      hand-built ~60-tick stream covering: a power waveform that
      visits every Coggan zone, a CP-bounded W' deplete-then-
      recover sequence, two simulated nearby riders with a gap
      that crosses 2.0 s mid-stream, and one synthetic segment
      entry/exit. The test loads it, runs the full STEP 15
      pipeline against stub `SegmentLookup` / `RoadGeometry`
      implementations, and asserts a checked-in expected-output
      JSON to ≤ 1e-6.
- [ ] **15.28-I** No implementation work expected if **15.27-T**
      is checked in against the implementation written for 15.2 –
      15.26. If the regression fails on first run, fix the
      implementation, not the fixture; record the resolution in
      the as-built notes.

## Tests-first plan (detail)

Every test file lives in `crates/zwift-stats/tests/*.rs`. The bullets
below correspond to the checklist items above.

### 15.2 Zone definitions — `tests/zones_definitions.rs`

| Test                                     | Asserts                                                                                                                                                                                                  |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `coggan_zones_at_ftp_250_match_js_table` | `coggan_zones(250.0)` returns 7 zones with `(from, to)` boundaries `[(0, 137.5), (137.5, 187.5), (187.5, 225.0), (225.0, 262.5), (262.5, 300.0), (300.0, 375.0), (375.0, None)]`. Labels match `Z1..Z7`. |
| `polarized_zones_at_ftp_250`             | Returns 3 zones `[(100.0, 200.0), (200.0, 250.0), (250.0, None)]`. Note Z1 starts at FTP × 0.40, not 0.                                                                                                  |
| `sweetspot_zone_fascat_and_coggan`       | `sweetspot_zone(250.0, SweetspotKind::Fascat)` returns `{zone: "SS", from: 210.0, to: 242.5, overlap: true}`. `SweetspotKind::Coggan` returns `{from: 220.0, to: 232.5}`.                                |

### 15.3 Zone accumulator — `tests/zones_accumulator.rs`

| Test                                                      | Asserts                                                                                                                                                                                                                                                  |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `accumulate_credits_top_down_with_break_on_non_overlap`   | FTP 250, Coggan zones, push `(t=0.0, value=300)` then `(t=1.0, value=300)`. After both ticks, `value()[4].time == 1.0` (Z5), all other zones at 0. The iteration starts at Z7 (top of the sorted array) and breaks on the first non-overlap match at Z5. |
| `accumulate_continues_iteration_on_overlap_for_sweetspot` | Configure Coggan zones plus Fascat sweetspot. Push `(0.0, 220)` then `(1.0, 220)`. After both ticks, sweetspot has 1.0 s AND Z3 has 1.0 s (sweetspot's `overlap: true` means the loop continues; 220 W is within Z3's `(187.5, 225.0]`).                 |
| `accumulate_handles_zero_and_top_bounds`                  | A value of `0.0` hits no zone (`from = 0` is exclusive). A value of `1e9` hits Z7 (`to = None` means `+inf`).                                                                                                                                            |
| `accumulate_first_tick_yields_zero_elapsed`               | First call to `accumulate(5.0, 200.0)` adds 0 to every zone (`time - _time_offset == 0`). Second call to `accumulate(6.0, 200.0)` adds 1.0 to the matching zone.                                                                                         |

### 15.4 Zone accumulator lifecycle — `tests/zones_accumulator.rs`

| Test                           | Asserts                                                                                                                                                                            |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `reset_clears_value_and_ftp`   | After driving a stream, `reset()` returns the accumulator to `ftp == None`, `value() == &[]`. Subsequent `accumulate` returns without crediting (matches JS `_accumulatorAbsent`). |
| `clone_continue_carries_state` | Drive a stream, `clone_continue()`, then drive both sides independently. The clone's `value()` carries the source state at the moment of the clone; subsequent writes diverge.     |
| `clone_reset_starts_fresh`     | `clone_reset()` returns an accumulator with the same `ftp` and zones but `value()` entries all `time == 0`.                                                                        |

### 15.5 `Sample::Break.pad` amendment — `tests/sample_break.rs`

| Test                                         | Asserts                                                                                                                                                        |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `break_pad_is_u32`                           | `let s = Sample::Break { pad: 5u32 };` compiles. Pattern-matching `Sample::Break { pad }` exposes `pad` as `u32` (test reads `pad as u64` and asserts `== 5`). |
| `break_pad_arithmetic_uses_integer_division` | A test that ports a STEP 13 break-handling code path to the new type; ensures no `f64::floor` or `as u32` casts are needed at construction sites.              |

### 15.6 W' balance core — `tests/wbal_recovery.rs`

| Test                                      | Asserts                                                                                                                                                                                                                                                           |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `recovery_below_cp_uses_exponential_term` | `configure(cp=200, w_prime=20000)`. Drive 400 W for enough ticks to drain wBal to ≈ 10000. Then push `(t, 100)` for 1 s. The change is `(200 - 100) * 1 * (20000 - 10000) / 20000 = 50` J per tick. Hand-derive the trajectory; the Rust output agrees to ≤ 1e-9. |
| `depletion_above_cp_uses_linear_term`     | `configure(cp=200, w_prime=20000)`. Push `(0, 300)` then `(1, 300)`. `value() ≈ 20000 - 100 = 19900`. (`(200 - 300) * 1 = -100`; linear branch, no scaling.)                                                                                                      |
| `clamp_at_wprime_does_not_exceed`         | `configure(cp=200, w_prime=20000)`. Push extended recovery from fresh state. `value() <= 20000.0` always.                                                                                                                                                         |
| `wbal_can_go_negative_in_the_red`         | `configure(cp=200, w_prime=20000)`. Push 500 W for 100 s. `value() < 0.0` (the JS does not clamp the lower bound).                                                                                                                                                |

### 15.7 W' break handling — `tests/wbal_break.rs`

| Test                                              | Asserts                                                                                                                                                                                                                                          |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `break_sample_refills_until_full`                 | After draining wBal to 5000, push `Sample::Break { pad: 1000 }` (u32 per 15.5). With cp=200, wPrime=20000, the geometric series saturates near 20000 well before iteration 1000. Test asserts `value() ≈ 20000.0` after (within 1e-6 of wPrime). |
| `break_sample_short_circuits_when_within_epsilon` | After driving wBal to 19999.9999999, push `Sample::Break { pad: 10 }`. Final `value() == 20000.0` exactly (the loop early-exits on the epsilon check).                                                                                           |

### 15.8 W' lifecycle — `tests/wbal_unconfigured.rs`

| Test                                       | Asserts                                                                                                           |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| `unconfigured_yields_none`                 | A fresh `WBalAccumulator::new()` has `value() == None`; `accumulate(0.0, Sample::Value(200.0))` returns `None`.   |
| `accumulator_clone_continue_carries_w_bal` | After driving wBal to 5000, `clone_continue().value() == Some(5000.0)`. Subsequent writes on either side diverge. |

### 15.9 EMA helper — `tests/exp_weighted_avg.rs`

| Test                                    | Asserts                                                                                                                   |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `seed_returns_seed_until_first_input`   | `exp_weighted_avg(8.0, 0.0).get() == 0.0`. After `update(10.0)`, `get() ≈ 1.1750309741540455` (`c_next = 1 - exp(-1/8)`). |
| `alpha_matches_js_for_size_8`           | After 8 successive `update(10.0)` calls starting from seed 0, the EMA approaches the JS reference value to ≤ 1e-9.        |
| `successive_updates_track_em_a_formula` | Hand-derived `avg_n = avg_{n-1} * c_prev + value * c_next` agreement to ≤ 1e-9 across 10 ticks.                           |

### 15.10 DataSlice — `tests/data_slice.rs`

| Test                                           | Asserts                                                                                                                                                                                                           |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new_clones_bucket_reset_and_carries_identity` | `DataSlice::new_from(&mut ad, t)` exposes `start == t`, `end == None`, `course_id == ad.course_id`, `sport == ad.sport`, `bucket.power().max_value() == 0.0` (reset).                                             |
| `id_is_assigned_monotonically_per_athlete`     | Two consecutive `new_from` calls on the same `AthleteData` return ids whose lower 32 bits are `n` and `n + 1`.                                                                                                    |
| `id_carries_athlete_prefix_in_upper_32_bits`   | Two `AthleteData` with different `athlete_id` values produce slice ids that, when the upper 32 bits are extracted, equal those `athlete_id` values. The lower 32 bits are independent counters per `AthleteData`. |
| `end_starts_none_and_can_be_stamped_once`      | `slice.end == None`. `slice.close(now)` stamps `end = Some(now)`. Calling `close` again is a no-op (matches JS `if (slice.end) return;`).                                                                         |
| `slice_carries_course_and_sport_at_creation`   | Mutating `ad.course_id` after slice creation does not change the slice's recorded `course_id`.                                                                                                                    |

### 15.11 AthleteData extensions — `tests/athlete_data_extensions.rs`

| Test                                       | Asserts                                                                                                                                                                                                     |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new_initialises_accumulators_and_streams` | `ad.w_bal.value() == None`, `ad.time_in_power_zones.value().is_empty()`, `ad.streams.distance.is_empty()`, `ad.road_history.a.is_empty()`, `ad.lap_slices.len() == 1`, `ad.lap_slices[0].end == None`.      |
| `initial_lap_slice_has_clone_reset_bucket` | `ad.lap_slices[0].bucket.power().max_value() == 0.0` even after the original bucket has been ingested into. (Re-check by mutating `ad.bucket` post-construction and observing the lap slice is unaffected.) |

### 15.12 `PlayerStateView` trait + `MostRecentState` extension — `tests/player_state_view.rs`

| Test                                             | Asserts                                                                                                                                                                                               |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `most_recent_state_implements_view_trait`        | `fn takes_view(_: &dyn PlayerStateView) {}` compiles when called with `&MostRecentState`.                                                                                                             |
| `view_trait_exposes_road_event_and_group_fields` | `view.road_id()`, `view.road_time()`, `view.reverse()`, `view.event_subgroup_id()`, `view.group_id()`, `view.time()`, `view.event_distance()` return the values stamped into the underlying struct.   |
| `view_trait_exposes_step_14_fields`              | `view.world_time()`, `view.power()`, `view.heartrate()`, `view.speed()`, `view.cadence()`, `view.draft()`, `view.distance()`, `view.altitude()` return the values stamped into the underlying struct. |

### 15.13 RoadHistory — `tests/road_history.rs`

| Test                                                    | Asserts                                                                                                                                      |
| ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `same_road_no_shift_appends_to_a`                       | Two states on the same road, both with rpct ascending. `road_history.a.len() == 2`, `b == None`, `c == None`.                                |
| `road_change_shifts_a_to_b_b_to_c`                      | Three states each on a different road. After the third: `a_road.sig == r3`, `b_road.sig == r2`, `c_road.sig == r1`.                          |
| `direction_change_shifts_when_delta_below_minus_001`    | Two states on the same road, rpct 0.5 → 0.4 (delta = -0.1 < -0.01). The tier shifts as if the road changed.                                  |
| `direction_change_wipes_a_when_delta_in_minus_001_to_0` | Two states on the same road, rpct 0.5 → 0.495 (delta = -0.005, in `(-0.01, 0)`). `a.len() == 1` (the wipe ran, then the new state appended). |
| `course_change_resets_b_and_c`                          | Two states on different courses: `b == None`, `c == None`, `a` carries only the new state.                                                   |
| `first_state_seeds_aroad_without_shift`                 | First-ever call: `a_road.sig == r1`, `b_road == None`, `c_road == None`, `a.len() == 1`.                                                     |

### 15.14 Event detection — `tests/event_detection.rs`

| Test                                                               | Asserts                                                                                                                                                                                                                              |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `new_subgroup_opens_slice_when_state_time_present`                 | State carries `event_subgroup_id = Some(42)`, `time > 0`. `event_slices.len() == 1`, slice carries `event_subgroup_id == Some(42)`.                                                                                                  |
| `new_subgroup_defers_when_state_time_zero_and_sets_pending`        | State carries `event_subgroup_id = Some(42)`, `time == 0`. `event_slices.is_empty()`, `event_start_pending == true`. Next tick with `time > 0` opens the slice.                                                                      |
| `same_subgroup_does_not_reopen_slice`                              | Two ticks with the same subgroup id; the existing slice's `end == None` and no new slice was appended.                                                                                                                               |
| `falsy_subgroup_after_active_closes_slice`                         | Tick 1 with subgroup 42, tick 2 with `event_subgroup_id = None`. The slice from tick 1 has `end == Some(now_tick_2)`.                                                                                                                |
| `auto_end_by_distance_closes_slice`                                | Subgroup has `end_distance = Some(1000)`, state's `event_distance > 1000`. Slice closes.                                                                                                                                             |
| `auto_end_by_wall_clock_closes_slice`                              | Subgroup has `end_ts = Some(t_end)`, wall-clock-ms argument is past `t_end`. Slice closes.                                                                                                                                           |
| `behavior_auto_reset_resets_athlete_data_on_event_start`           | `EventBehavior { auto_reset: true, auto_lap: false }`. Athlete starts the event with non-zero `bucket.power().max_value()`. After `apply_event_state`, the bucket has been reset (max_value back to 0.0); a new event slice is open. |
| `behavior_auto_lap_starts_a_lap_on_event_start_when_not_resetting` | `EventBehavior { auto_reset: false, auto_lap: true }`. Athlete starts the event with `lap_slices.len() == 1`. After `apply_event_state`, `lap_slices.len() == 2` (the prior open lap was closed, a fresh one was started).           |
| `behavior_neither_does_not_reset_or_lap`                           | `EventBehavior { auto_reset: false, auto_lap: false }`. After `apply_event_state`, `bucket.power().max_value()` is unchanged and `lap_slices.len()` is unchanged.                                                                    |

### 15.15 Event privacy — `tests/event_privacy.rs`

| Test                                | Asserts                                                                                                             |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `self_athlete_skips_privacy_flags`  | `state.athlete_id == self_athlete_id`; subgroup has `all_tags = ["hidewbal"]`. `event_privacy.hide_w_bal == false`. |
| `non_self_hidewbal_sets_hide_w_bal` | Different athlete; tag `hidewbal`. `event_privacy.hide_w_bal == true`.                                              |
| `non_self_hideftp_sets_hide_ftp`    | Tag `hideftp`. `event_privacy.hide_ftp == true`.                                                                    |
| `hidethehud_sets_disabled_by_event` | Tag `hidethehud`. `disabled_by_event == true`.                                                                      |
| `nooverlays_sets_disabled_by_event` | Tag `nooverlays`. `disabled_by_event == true`.                                                                      |

### 15.16 Manual laps — `tests/laps.rs`

| Test                                                  | Asserts                                                                                                                                                                  |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `start_athlete_lap_closes_open_slice_and_appends_new` | Initial `ad.lap_slices.len() == 1`. After `start_athlete_lap(&mut ad, 100.0)`, `lap_slices.len() == 2`, `lap_slices[0].end == Some(100.0)`, `lap_slices[1].end == None`. |
| `start_athlete_lap_returns_new_slice_id`              | Returns the new slice's `id`.                                                                                                                                            |
| `start_athlete_lap_clones_bucket_via_clone_reset`     | New slice's `bucket.power().max_value() == 0.0` even after the original bucket had peaks.                                                                                |

### 15.17 Auto laps — `tests/laps.rs`

| Test                                                       | Asserts                                                                                                                                                                                                   |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `auto_lap_by_distance_threshold_triggers_at_each_interval` | `AutoLapConfig { metric: Distance, threshold: 1000.0 }`. `auto_lap_check` at `state.distance = 0` seeds the mark and returns `false`. At 999 → `false`. At 1000 → `true` (mark = 1000). At 2000 → `true`. |
| `auto_lap_by_time_threshold_triggers_at_each_interval`     | `AutoLapConfig { metric: Time, threshold: 60.0 }`. Similar arithmetic on `state.time`.                                                                                                                    |
| `auto_lap_mark_resets_on_course_change`                    | After setting `ad.auto_lap_mark = None` (simulating the course-change branch of the preprocessing step), the next `auto_lap_check` reseeds without triggering a lap.                                      |
| `auto_lap_first_call_seeds_mark_without_lapping`           | `lap_slices.len()` unchanged after the first call; `auto_lap_mark == Some(state_value)`.                                                                                                                  |

### 15.18 Segment start — `tests/segments.rs`

| Test                                                             | Asserts                                                                                                                                                                                                                                |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `start_within_first_5_percent_opens_slice`                       | Segment with `road_start = 0.20`, `road_finish = 0.40`, `distance = 5000`. State at `road_time` such that progress = 0.025 (< 0.05). `start_segment` is called; `active_segments.contains_key(seg.id)`.                                |
| `start_within_first_150m_overrides_5_percent_for_short_segments` | Segment with `distance = 100`, `road_start = 0.20`, `road_finish = 0.21`. Threshold is `max(0.05, 150 / 100)` per JS expression, which makes the entry window cover essentially the entire segment. Entering anywhere inside opens it. |
| `outside_progress_does_not_open_slice`                           | State at progress = 0.5 on a normal-length segment. `progress > 0.05` AND `progress > 150 / distance`. `active_segments` stays empty.                                                                                                  |
| `exit_after_open_stops_segment`                                  | Open a segment, then advance state to a different road (the segment leaves the `active` set for this tick). `active_segments.is_empty()`, `segment_slices.len() == 1`, the slice carries `end == Some(now)`.                           |
| `multiple_concurrent_segments_tracked_independently`             | Two overlapping segments on the same road, enter both within their windows. `active_segments.len() == 2`. Exit one → `len() == 1`.                                                                                                     |

### 15.19 Segment completion — `tests/segments.rs`

| Test                                                           | Asserts                                                                                                                                          |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `stop_marks_incomplete_when_no_road_history`                   | Set up a segment with no matching tier in `road_history`. `stop_segment` → `slice.incomplete == Some(true)`.                                     |
| `stop_marks_complete_when_long_segment_at_or_above_90_percent` | Segment with `distance = 2000`. Road history carries a final `(rpct, wt)` past `start + 0.9 * (end - start)`. `slice.incomplete == Some(false)`. |
| `stop_marks_incomplete_when_long_segment_below_90_percent`     | Same segment, history only reaches `start + 0.5 * (end - start)`. `slice.incomplete == Some(true)`.                                              |
| `stop_thresholds_60_percent_for_400_to_1000m`                  | `distance = 500`. 65% completion → complete. 55% → incomplete.                                                                                   |
| `stop_thresholds_25_percent_for_below_400m`                    | `distance = 200`. 30% → complete. 20% → incomplete.                                                                                              |
| `stop_walks_road_history_a_b_c_until_match`                    | The matching tier is the `c` tier (two roads back). Completion still resolves correctly.                                                         |

### 15.20 Road-position comparison — `tests/gap.rs`

| Test                                          | Asserts                                                                                                                              |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `same_road_same_direction_uses_delta_rpct`    | `p1.aRoad == p2.aRoad`. p1 rpct = 0.6, p2 rpct = 0.5. `tiers == 1`, `reversed == false`. Distance = `road_distance(road, 0.5, 0.6)`. |
| `same_road_negative_delta_marks_reversed`     | p1 rpct = 0.4, p2 rpct = 0.5. `reversed == true`.                                                                                    |
| `cross_tier_two_back_resolves_via_b_road`     | `p2.aRoad == p1.bRoad`, last p1.b's rpct ≥ p2's current rpct. `tiers == 2`, `reversed == false`.                                     |
| `cross_tier_three_back_resolves_via_c_road`   | `p2.aRoad == p1.cRoad`. `tiers == 3`.                                                                                                |
| `no_connection_returns_none`                  | None of the tier-match cases apply. Returns `None`.                                                                                  |
| `boundary_error_term_001_admits_near_matches` | Same as `cross_tier_two_back`, but with last-rpct = `p2.rpct - 0.005` (within 0.01 slop). Resolves.                                  |

### 15.21 Gap fields — `tests/gap.rs`

| Test                                             | Asserts                                                                                          |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `gap_field_set_from_world_time_delta_in_seconds` | Watching at world time T, target at T - 5000 (ms). `ad.gap == Some(5.0)`.                        |
| `gap_negated_when_reversed_and_positive`         | `reversed == true`, raw `(T_watch - T_rp) / 1000 == 5.0`. `ad.gap == Some(-5.0)`.                |
| `gap_distance_signed_by_direction`               | `reversed == true`, `rp.distance == 100`. `ad.gap_distance == Some(-100.0)`. Else `Some(100.0)`. |
| `is_gap_est_false_when_world_time_match`         | `is_gap_est == false`.                                                                           |
| `is_gap_est_true_when_world_time_missing`        | Estimation fallback path. `gap == None`, `is_gap_est == true`.                                   |

### 15.22 Group clumping — `tests/groups.rs`

| Test                                            | Asserts                                                                                                                                                                      |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `singleton_riders_get_group_id_none`            | `nearby = [{a:1, gap:0}, {a:2, gap:5}]` (gap > 2 s). After `compute_groups`, both groups in the returned `Vec<Group>` are singletons; both members carry `group_id == None`. |
| `two_riders_within_2_second_gap_form_one_group` | `nearby = [{a:1, gap:0, draft:50}, {a:2, gap:1.5, draft:50}]`. Both share the same `group_id != None`.                                                                       |
| `gap_above_2_seconds_splits_group`              | `nearby = [{a:1, gap:0, draft:50}, {a:2, gap:2.1, draft:50}]`. Different group ids (or both `None` if singletons).                                                           |
| `gap_above_0_8_without_draft_splits_group`      | `nearby = [{a:1, gap:0, draft:0}, {a:2, gap:1.0, draft:0}]`. Different group ids (no draft, 1.0 > 0.8).                                                                      |
| `gap_at_or_below_0_8_with_no_draft_keeps_group` | `nearby = [{a:1, gap:0, draft:0}, {a:2, gap:0.7, draft:0}]`. Same group id.                                                                                                  |

### 15.23 Group aggregates — `tests/groups.rs`

| Test                                                         | Asserts                                                                                                                                                                |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aggregate_weight_skips_zero_weight_athletes`                | Three riders in one group with weights `[Some(70), None, Some(80)]`. Group `weight == Some(75.0)` (mean over the two valid weights).                                   |
| `aggregate_power_and_draft_use_member_count`                 | Two riders, `power = [200, 300]`. Group `power == 250.0`.                                                                                                              |
| `aggregate_speed_uses_median_not_mean`                       | Three riders, `speed = [10, 100, 20]`. Group `speed == 20.0` (median).                                                                                                 |
| `aggregate_heartrate_skips_none_entries`                     | Two riders, `heartrate = [None, Some(150.0)]`. Group `heartrate == Some(150.0)`.                                                                                       |
| `group_gap_is_zero_for_watching_group`                       | Watching is in group N. Group N's `gap == 0`.                                                                                                                          |
| `group_gap_is_head_for_group_ahead_tail_for_group_behind`    | Group N has riders at gap 5.0, 5.5 (head, tail). Watching is at gap 0 (behind). Group N's `gap == 5.0` (head). Mirror for groups behind watching: gap is the tail.     |
| `last_group_length_uses_head_and_tail_consistently_not_zero` | Three groups, last group has riders at gap 10.0 and 11.5. `last_group.length_time == 1.5` (head-to-tail), not 0.0. Pins the fix for the JS bug at stats.mjs:4506-4509. |

### 15.24 Jaccard identity — `tests/groups_identity.rs`

| Test                                                                   | Asserts                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `singleton_group_does_not_create_group_meta`                           | Single-rider group; `prior_groups.len()` unchanged after `compute_groups`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `multi_rider_group_creates_fresh_meta_when_no_prior_match`             | No prior groups. After compute, `prior_groups.len() == 1`, the group's `id == initial_next_id`. `next_id` incremented.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `jaccard_above_0_5_reuses_prior_group_id`                              | Prior group `{1, 2, 3}` with id 100. New group `{1, 2, 3, 4}`. Jaccard = `3/4 = 0.75`. Matches; new group's `id == 100`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `jaccard_exactly_0_5_creates_fresh_meta_strict_threshold`              | Prior group `{1, 2, 3}` with id 100. New group `{1, 2, 4}`. Jaccard = `2/4 = 0.5`, _not_ `> 0.5`. New group mints a fresh id; `prior_groups` has both entries (the old one until GC, the new one with the fresh id).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `members_who_left_get_group_id_cleared_only_if_still_pointing_to_meta` | Prior meta id=100, identity*set `{1, 2, 3}`. Athletes 1/2/3 carry `group_id = Some(100)` from prior tick. New group `{1, 2, 4, 5}` matches at Jaccard 2/5 = 0.4 → does \_not* match. Adjust the test so Jaccard is `> 0.5` (new group `{1, 2, 3, 4, 5}` ∩ `{1,2,3}` = 3, ∪ = 5, Jaccard = 0.6 → match). Athlete 3 was in the prior set but is still in the new set, so its `group_id` stays. Modify: new group `{1, 2, 4, 5, 6}` ∩ `{1,2,3}` = 2, ∪ = 6, Jaccard = 0.33 → does not match. Use a setup that meets `> 0.5` while leaving someone behind: prior `{1, 2, 3, 4}`, new `{1, 2, 3, 5}`. Jaccard = 3/5 = 0.6 → match. Athlete 4 (left behind) had `group_id = Some(100)`; after compute, `athlete_4.group_id == None`. A second athlete with `group_id = Some(999)` (unrelated) stays `Some(999)`. |
| `prior_meta_used_once_per_tick_greedy_first_wins`                      | Two new groups both score `> 0.5` against the same prior meta. The first iterated group wins the id; the second mints a fresh one.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |

### 15.25 GroupMeta identity_set — `tests/groups_identity.rs`

| Test                                              | Asserts                                                                                                                                                                                                                                       |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `group_meta_carries_identity_set`                 | After `compute_groups` produces a multi-rider group, the stored `GroupMeta.identity_set == HashSet::from([a1, a2, ...])`.                                                                                                                     |
| `gc_drops_meta_past_ttl_with_identity_set_intact` | Insert a group with a populated `identity_set`, age `accessed` past TTL, `gc(now)` drops it; the `GcReport.groups_dropped == 1`. Identity-set contents do not affect TTL eviction (covered by STEP 14 already, repeated here for confidence). |

### 15.26 Streams — `tests/streams.rs`

| Test                                                        | Asserts                                                                                                                                            |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `record_streams_appends_distance_altitude_latlng_per_tick`  | After three calls to `ad.record_streams(state)` with different distances, `streams.distance.len() == 3`.                                           |
| `latlng_uses_custom_type_with_named_fields`                 | `streams.latlng[0].lat == state.lat` and `.lng == state.lng`. The Rust port uses a named-field `LatLng` struct rather than a tuple.                |
| `wbal_sample_appended_per_tick_when_accumulator_configured` | `WBalAccumulator` configured with cp = 200, w_prime = 20000. Each `accumulate` returns `Some(value)`. `streams.wbal` carries `Some(...)` per tick. |
| `wbal_stream_carries_none_when_accumulator_unconfigured`    | Unconfigured `WBalAccumulator`. `streams.wbal.len() == N`, every entry `None`.                                                                     |

### 15.27 Regression fixture — `tests/step15_regression.rs`

A hand-built ~60-tick session driving every STEP 15 surface:

| Section        | Content                                                                                                                                 |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Power waveform | `[100, 100, 100, 200, 200, 300, 300, 400, 400, ...]` covering every Coggan zone at FTP=250.                                             |
| W' trace       | Same waveform, CP=250, W'=20000. Expected `value()` per tick checked in.                                                                |
| Nearby riders  | Two synthetic riders with gap trace `[1.5, 1.5, 2.5, 2.5, 1.0, 1.0]` to exercise both the cohesion and split paths.                     |
| Segment        | One synthetic segment from rpct 0.20 to 0.30 on a stub road, with the rider entering at progress 0.02 and exiting after progress > 1.0. |

Expected outputs (per tick) recorded in `fixtures/step15_session_expected.json`:

- `time_in_power_zones.value()` after the final tick
- `wbal_trace[..]` (one per tick)
- `group_assignments[..]` (athlete_id → group_id per tick)
- `segment_slices[0].incomplete`

Tolerance: ≤ 1e-6 absolute for f64 fields.

## Resolved decisions

The questions raised during the elaboration of this plan have all
been decided. Each entry below records the chosen option, points
at where the implementation lives, and names the fallback (if any)
that a future step may revisit.

1. **`Streams` lives in `src/streams.rs`; `latlng` uses a named-field
   `LatLng` type.** This is a Rust port, not a transliteration of
   the JS array-of-arrays shape. The `Streams` struct sits in its
   own module so that the stream channels (distance, altitude,
   latlng, wbal) stay discoverable alongside the slice machinery
   without being entangled with `DataSlice`'s identity / lifecycle
   fields. `LatLng` is a `pub struct LatLng { pub lat: f64, pub
lng: f64 }` — the named fields prevent the common `(lng, lat)`
   transposition bug that tuples invite. Pinned by
   15.26-T `latlng_uses_custom_type_with_named_fields`.

2. **`Env` interface stays in `zwift-stats` as a trait surface.**
   `SegmentLookup`, `RoadGeometry`, and the free `road_sig` /
   `from_road_sig` functions are defined inside `zwift-stats`.
   STEP 15 ships test stubs only; the real table-backed
   implementations land in `zwift-routes` when STEP 17 wires the
   daemon. Keeps `zwift-stats` dependency-light and proto-free.

3. **`Sample::Break.pad` becomes `u32`.** This is a STEP 13
   touch-up. `pad` represents an iteration count and the type
   should reflect that — STEP 13's `f64` was a transliteration
   from JavaScript that should not survive the port. The change
   touches every construction site of `Sample::Break` in the
   workspace; STEP 13's existing tests continue to pass after
   the amendment because they always produced integral values.
   Pinned by 15.5-T `break_pad_is_u32`.

4. **`streams.wbal` carries `None` when the accumulator is
   unconfigured.** `Vec<Option<f64>>` preserves the per-tick
   alignment with `streams.distance`, `.altitude`, and
   `.latlng`. The future analysis pages depend on the tick
   alignment. Pinned by 15.26-T
   `wbal_stream_carries_none_when_accumulator_unconfigured`.

5. **`DataSlice` id is per-`AthleteData` counter packed with the
   athlete id prefix.** The id is
   `((athlete_id as u64) << 32) | (counter as u64)` where
   `counter: u32` lives on `AthleteData` itself. This is
   globally unique without coordination across athletes,
   monotonic within an athlete, and self-describing (the
   originating athlete is recoverable from the id alone). No
   process-global allocator; the counter advances inside
   `DataSlice::new_from(&mut ad, start)`. Pinned by 15.10-T
   `id_is_assigned_monotonically_per_athlete` and
   `id_carries_athlete_prefix_in_upper_32_bits`.

6. **`PlayerStateView` trait is introduced in STEP 15.** The
   hand-written `MostRecentState` struct grows the seven new
   fields (`road_id`, `road_time`, `reverse`,
   `event_subgroup_id`, `group_id`, `time`, `event_distance`)
   and implements `PlayerStateView`. Every STEP 15 detector
   that reads state takes `&dyn PlayerStateView` or
   `impl PlayerStateView`. STEP 17 may then implement the
   trait directly on the proto type without touching the call
   sites. STEP 17's "MostRecentState proto-type decision"
   bullet remains the gate for whether the proto type
   implements the trait or whether the daemon keeps mapping
   into the hand-written struct. STEP 17 has been updated to
   reflect this. Pinned by 15.12-T
   `most_recent_state_implements_view_trait`.

7. **The JS group-length bug at `stats.mjs:4506-4509` is fixed.**
   The Rust port computes `length_time` and `length_distance`
   for the last group using head-and-tail consistently with
   inner groups, not with `tail` doubled. ranchero exists to
   feed visualization; a stable zero for the last group is
   genuinely wrong. Pinned by 15.23-T
   `last_group_length_uses_head_and_tail_consistently_not_zero`.

8. **Eight new files, one per concept.** `src/zones.rs`,
   `src/wbal.rs`, `src/slice.rs`, `src/streams.rs`,
   `src/road_history.rs`, `src/groups.rs`, `src/laps.rs`,
   `src/segments.rs`, `src/events.rs`. Names map 1-to-1 with
   the checklist sections and the JS source headings; each
   detector is independently discoverable. (Decision 1 split
   `streams.rs` out of `slice.rs`, bringing the count to nine.)

9. **`apply_event_state` takes an `EventBehavior` config struct.**
   `EventBehavior { auto_reset: bool, auto_lap: bool }` lets
   STEP 15 cover both branches (auto-reset vs auto-lap vs
   neither) with tests rather than punting the branch logic to
   STEP 17. STEP 17 then only needs to wire the settings into
   the struct. STEP 17 has been updated to reflect this.
   Pinned by 15.14-T `behavior_auto_reset_resets_athlete_data_on_event_start`,
   `behavior_auto_lap_starts_a_lap_on_event_start_when_not_resetting`,
   and `behavior_neither_does_not_reset_or_lap`.

10. **Segment start window uses the JS `5% OR 150m / distance`
    formula.** The expression `progress < 0.05 || progress <
SEGMENT_START_WINDOW_METRES / segment.distance` is preserved
    literally — the comparison `progress < 150 / distance` is
    dimensionally consistent because `progress` is a fraction
    of `(road_finish - road_start)` and the right-hand side is
    the same fraction expressed for the 150 m short-sprint
    window. Pinned by 15.18-T
    `start_within_first_150m_overrides_5_percent_for_short_segments`.

## Crate layout

```
crates/zwift-stats/
├── Cargo.toml          — unchanged (no new dependencies)
├── src/
│   ├── lib.rs          — adds `pub mod zones; pub mod wbal; pub mod slice;`
│   │                     `pub mod streams; pub mod road_history;`
│   │                     `pub mod groups; pub mod laps; pub mod segments;`
│   │                     `pub mod events;` and re-exports the public types
│   ├── rolling.rs      — unchanged
│   ├── power.rs        — unchanged
│   ├── helpers.rs      — adds `exp_weighted_avg`
│   ├── bucket.rs       — unchanged
│   ├── periods.rs      — adds GROUP_GAP_THRESHOLD_S, GROUP_GAP_THRESHOLD_NO_DRAFT_S,
│   │                     JACCARD_MATCH_THRESHOLD, NEARBY_MAX_GAP_S,
│   │                     BOUNDARY_ERROR_TERM, SEGMENT_START_WINDOW_PCT,
│   │                     SEGMENT_START_WINDOW_METRES, SEGMENT_COMPLETION_*,
│   │                     W_PRIME_DEFAULT, ROAD_TIME_OFFSET, ROAD_TIME_SCALE
│   ├── collector.rs    — unchanged
│   ├── data_bucket.rs  — unchanged
│   ├── sample.rs       — type-touch-up: `Sample::Break.pad: u32`
│   │                     (STEP 13 amendment; see resolved decision 3)
│   ├── athlete.rs      — adds the STEP 15 fields and methods on AthleteData,
│   │                     extends MostRecentState, defines PlayerStateView trait,
│   │                     extends GroupMeta
│   ├── zones.rs        — Zone, ZoneTime, ZonesAccumulator,
│   │                     coggan_zones, polarized_zones, sweetspot_zone
│   ├── wbal.rs         — WBalAccumulator
│   ├── slice.rs        — DataSlice (id packed as
│   │                     `(athlete_id << 32) | per-athlete counter`)
│   ├── streams.rs      — LatLng, Streams
│   ├── road_history.rs — RoadHistory, RoadKey, RoadPoint, RoadGeometry trait,
│   │                     road_sig, from_road_sig,
│   │                     RoadComparison, compare_road_positions
│   ├── groups.rs       — Group, NearbyEntry, compute_groups, apply_gap
│   ├── laps.rs         — AutoLapConfig, AutoLapMetric,
│   │                     start_athlete_lap, auto_lap_check
│   ├── segments.rs     — Segment, SegmentLookup trait,
│   │                     active_segment_check, start_segment, stop_segment
│   └── events.rs       — EventSubgroup, EventPrivacy, EventBehavior,
│                         EventStateOutcome,
│                         apply_event_state, trigger_event_start,
│                         trigger_event_end
└── tests/
    ├── …existing STEP 13 / 14 tests…
    ├── zones_definitions.rs
    ├── zones_accumulator.rs
    ├── sample_break.rs
    ├── wbal_recovery.rs
    ├── wbal_break.rs
    ├── wbal_unconfigured.rs
    ├── exp_weighted_avg.rs
    ├── data_slice.rs
    ├── athlete_data_extensions.rs
    ├── player_state_view.rs
    ├── road_history.rs
    ├── event_detection.rs
    ├── event_privacy.rs
    ├── laps.rs
    ├── segments.rs
    ├── gap.rs
    ├── groups.rs
    ├── groups_identity.rs
    ├── streams.rs
    ├── step15_regression.rs
    └── fixtures/
        ├── step15_session.json
        └── step15_session_expected.json
```

Every public item is re-exported from `lib.rs` so callers can write
`use zwift_stats::{ZonesAccumulator, WBalAccumulator, DataSlice,
RoadHistory, compute_groups, ...};` without navigating internal
module paths.

## Public API surface (proposed)

### Constants (`periods`)

```rust
pub const GROUP_GAP_THRESHOLD_S:           f64 = 2.0;
pub const GROUP_GAP_THRESHOLD_NO_DRAFT_S:  f64 = 0.8;
pub const JACCARD_MATCH_THRESHOLD:         f64 = 0.5;   // strict `>`
pub const NEARBY_MAX_GAP_S:                f64 = 900.0; // 15 minutes
pub const BOUNDARY_ERROR_TERM:             f64 = 0.01;
pub const SEGMENT_START_WINDOW_PCT:        f64 = 0.05;
pub const SEGMENT_START_WINDOW_METRES:     f64 = 150.0;
pub const SEGMENT_COMPLETION_LONG:         f64 = 0.90;
pub const SEGMENT_COMPLETION_MID:          f64 = 0.60;
pub const SEGMENT_COMPLETION_SHORT:        f64 = 0.25;
pub const SEGMENT_LONG_MIN_METRES:         f64 = 1000.0;
pub const SEGMENT_MID_MIN_METRES:          f64 = 400.0;
pub const W_PRIME_DEFAULT:                 f64 = 20000.0;
pub const ROAD_TIME_OFFSET:                f64 = 5000.0; // stats.mjs:3475
pub const ROAD_TIME_SCALE:                 f64 = 1.0e6;
```

### Zones (`zones`)

```rust
pub struct Zone {
    pub zone:    &'static str,
    pub from:    f64,
    pub to:      Option<f64>,    // None ⇒ unbounded upward
    pub overlap: bool,
}

pub struct ZoneTime {
    pub zone: &'static str,
    pub time: f64,
}

#[derive(Clone, Copy)]
pub enum SweetspotKind { Fascat, Coggan }

pub fn coggan_zones(ftp: f64) -> Vec<Zone>;
pub fn polarized_zones(ftp: f64) -> Vec<Zone>;
pub fn sweetspot_zone(ftp: f64, kind: SweetspotKind) -> Zone;

pub struct ZonesAccumulator {
    ftp:         Option<f64>,
    zones:       Vec<Zone>,
    value:       Vec<ZoneTime>,
    time_offset: f64,
}

impl ZonesAccumulator {
    pub fn new() -> Self;
    pub fn configure(&mut self, ftp: f64, zones: Vec<Zone>);
    pub fn accumulate(&mut self, time: f64, value: f64);
    pub fn value(&self) -> &[ZoneTime];
    pub fn ftp(&self) -> Option<f64>;
    pub fn reset(&mut self);
    pub fn clone_reset(&self) -> Self;
    pub fn clone_continue(&self) -> Self;
}
```

### W' balance (`wbal`)

```rust
pub struct WBalAccumulator {
    cp:          Option<f64>,
    w_prime:     Option<f64>,
    w_bal:       Option<f64>,
    time_offset: f64,
}

impl WBalAccumulator {
    pub fn new() -> Self;
    pub fn configure(&mut self, cp: f64, w_prime: f64);
    /// Returns `Some(new_wbal)` after this tick, or `None` if unconfigured.
    pub fn accumulate(&mut self, time: f64, sample: Sample) -> Option<f64>;
    pub fn value(&self) -> Option<f64>;
    pub fn cp(&self) -> Option<f64>;
    pub fn w_prime(&self) -> Option<f64>;
    pub fn reset(&mut self);
    pub fn clone_reset(&self) -> Self;
    pub fn clone_continue(&self) -> Self;
}
```

### Exponential weighted average (`helpers`)

```rust
#[derive(Clone, Copy, Debug)]
pub struct ExpWeightedAvg {
    avg:    f64,
    c_prev: f64,
    c_next: f64,
    size:   f64,
}

pub fn exp_weighted_avg(size: f64, seed: f64) -> ExpWeightedAvg;

impl ExpWeightedAvg {
    pub fn update(&mut self, value: f64) -> f64;
    pub fn get(&self) -> f64;
    pub fn size(&self) -> f64;
}
```

### Slice (`slice`)

```rust
pub struct DataSlice {
    pub id:                   u64,
    pub start:                f64,
    pub end:                  Option<f64>,
    pub course_id:            u32,
    pub sport:                u8,
    pub bucket:               DataBucket,

    // Lap / event / segment-specific extension fields. None when
    // not applicable; the producer (`start_athlete_lap`,
    // `trigger_event_start`, `start_segment`) writes the relevant
    // subset.
    pub segment_id:           Option<u32>,
    pub start_world_time:     Option<f64>,
    pub event_subgroup_id:    Option<u32>,
    pub start_event_distance: Option<f64>,
    pub end_event_distance:   Option<f64>,
    pub incomplete:           Option<bool>, // None until stop_segment runs
}

impl DataSlice {
    /// Pulls the next per-athlete counter from `ad`, packs it with
    /// the athlete id, clones the bucket via `clone_reset`, and
    /// stamps `start`, `course_id`, `sport`.
    pub fn new_from(ad: &mut AthleteData, start: f64) -> Self;
    pub fn close(&mut self, end: f64);
}
```

`DataSlice` follows the project field-visibility convention for
plain data containers: every field is `pub`. The struct exposes
two helpers (`new_from`, `close`) because both have non-trivial
behaviour (id packing, end-stamping idempotence). The slice id
is `((ad.athlete_id as u64) << 32) | (counter as u64)` where
`counter: u32` advances on `AthleteData` itself — globally unique
across athletes, monotonic within an athlete, and self-describing.

### Streams (`streams`)

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LatLng {
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug, Default)]
pub struct Streams {
    pub distance: Vec<f64>,
    pub altitude: Vec<f64>,
    pub latlng:   Vec<LatLng>,
    pub wbal:     Vec<Option<f64>>,
}
```

`LatLng` is a named-field struct rather than a tuple so callers
read `point.lat` / `point.lng` instead of `point.0` / `point.1`,
which prevents the common `(lng, lat)` transposition bug. Both
types are POD: every field is `pub`.

### Road history (`road_history`)

```rust
#[derive(Clone, Copy, Debug)]
pub struct RoadKey {
    pub course_id: u32,
    pub road_id:   u32,
    pub reverse:   bool,
    pub sig:       u64,
}

#[derive(Clone, Copy, Debug)]
pub struct RoadPoint {
    pub rpct: f64,
    pub wt:   f64,
}

#[derive(Debug, Default)]
pub struct RoadHistory {
    pub a_road: Option<RoadKey>,
    pub b_road: Option<RoadKey>,
    pub c_road: Option<RoadKey>,
    pub a:      Vec<RoadPoint>,
    pub b:      Option<Vec<RoadPoint>>,
    pub c:      Option<Vec<RoadPoint>>,
}

impl RoadHistory {
    pub fn record(&mut self, state: &MostRecentState, prev: Option<&MostRecentState>);
}

pub fn road_sig(course_id: u32, road_id: u32, reverse: bool) -> u64;
pub fn from_road_sig(sig: u64) -> RoadKey;

pub trait RoadGeometry {
    fn road_distance(&self, road: &RoadKey, start_pct: f64, end_pct: f64) -> f64;
}

pub struct RoadComparison {
    pub world_time: f64,
    pub distance:   f64,
    pub reversed:   bool,
}

pub fn compare_road_positions(
    p1: &RoadHistory,
    p2: &RoadHistory,
    env: &dyn RoadGeometry,
) -> Option<RoadComparison>;
```

### Segments (`segments`)

```rust
#[derive(Clone, Debug)]
pub struct Segment {
    pub id:          u32,
    pub course_id:   u32,
    pub road_id:     u32,
    pub reverse:     bool,
    pub road_start:  f64,
    pub road_finish: f64,
    pub distance:    f64,
}

pub trait SegmentLookup {
    fn road_segments(&self, course_id: u32, road_id: u32, reverse: bool) -> &[Segment];
    fn segment(&self, id: u32) -> Option<&Segment>;
}

pub fn active_segment_check(
    ad:    &mut AthleteData,
    state: &MostRecentState,
    env:   &dyn SegmentLookup,
    now:   f64,
);
```

### Laps (`laps`)

```rust
#[derive(Clone, Copy, Debug)]
pub enum AutoLapMetric { Distance, Time }

#[derive(Clone, Copy, Debug)]
pub struct AutoLapConfig {
    pub metric:    AutoLapMetric,
    /// Metres for Distance; seconds for Time.
    pub threshold: f64,
}

/// Returns the new lap slice's id.
pub fn start_athlete_lap(ad: &mut AthleteData, now: f64) -> u64;

/// Returns true if a lap was started this call.
pub fn auto_lap_check(
    ad:    &mut AthleteData,
    state: &MostRecentState,
    cfg:   AutoLapConfig,
    now:   f64,
) -> bool;
```

### Events (`events`)

```rust
#[derive(Clone, Debug)]
pub struct EventSubgroup {
    pub id:           u32,
    pub course_id:    u32,
    pub all_tags:     Vec<String>,
    pub end_ts:       Option<f64>,
    pub end_distance: Option<f64>,
    // Other fields land as the web surface (STEP 17) needs them.
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EventPrivacy {
    pub hide_w_bal: bool,
    pub hide_ftp:   bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EventBehavior {
    pub auto_reset: bool,
    pub auto_lap:   bool,
}

pub enum EventStateOutcome {
    Idle,
    Started      { slice_id: u64 },
    StartPending,
    Ended        { slice_id: u64 },
}

pub fn apply_event_state(
    ad:               &mut AthleteData,
    state:            &MostRecentState,
    self_athlete_id:  u32,
    sg_lookup:        &HashMap<u32, EventSubgroup>,
    behavior:         EventBehavior,
    now_monotonic:    f64,
    now_wall_clock_ms: f64,
) -> EventStateOutcome;
```

### Groups (`groups`)

```rust
#[derive(Clone, Debug)]
pub struct NearbyEntry {
    pub athlete_id: u32,
    pub gap:        f64,
    pub draft:      f64,
    pub weight:     Option<f64>,
    pub power:      f64,
    pub heartrate:  Option<f64>,
    pub speed:      f64,
    pub is_gap_est: bool,
}

#[derive(Clone, Debug)]
pub struct Group {
    pub id:              Option<u32>,    // None for singletons
    pub members:         Vec<u32>,       // athlete_ids in order
    pub weight:          Option<f64>,
    pub power:           f64,
    pub draft:           f64,
    pub heartrate:       Option<f64>,
    pub speed:           f64,
    pub gap:             f64,
    pub is_gap_est:      bool,
    pub length_time:     f64,
    pub length_distance: f64,
}

pub fn compute_groups(
    nearby:        &[NearbyEntry],
    watching_idx:  usize,
    prior_groups:  &mut HashMap<u32, GroupMeta>,
    next_id:       &mut u32,
    now:           f64,
) -> Vec<Group>;

/// Applies the `gap` / `gap_distance` / `is_gap_est` fields on
/// `ad` against the watching athlete. Mutates `ad` only.
pub fn apply_gap(
    ad:       &mut AthleteData,
    watching: &AthleteData,
    env:      &dyn RoadGeometry,
);
```

### AthleteData extension (`athlete`)

```rust
pub struct AthleteData {
    // STEP 14 fields (unchanged).
    pub athlete_id:        u32,
    pub course_id:         u32,
    pub sport:             u8,
    pub created:           f64,
    pub updated:           f64,
    pub wt_offset:         f64,
    pub distance_offset:   f64,
    pub internal_created:  f64,
    pub internal_updated:  f64,
    pub internal_accessed: f64,
    pub most_recent_state: Option<MostRecentState>,
    pub bucket:            DataBucket,

    // STEP 15 fields.
    pub w_bal:               WBalAccumulator,
    pub time_in_power_zones: ZonesAccumulator,
    pub smooth_grade:        ExpWeightedAvg,
    pub streams:             Streams,
    pub road_history:        RoadHistory,
    pub lap_slices:          Vec<DataSlice>,
    pub event_slices:        Vec<DataSlice>,
    pub segment_slices:      Vec<DataSlice>,
    pub active_segments:     HashMap<u32, DataSlice>,
    pub gap:                 Option<f64>,
    pub gap_distance:        Option<f64>,
    pub is_gap_est:          bool,
    pub group_id:            Option<u32>,
    pub event_subgroup:      Option<EventSubgroup>,
    pub event_privacy:       EventPrivacy,
    pub disabled_by_event:   bool,
    pub event_start_pending: bool,
    pub auto_lap_mark:       Option<f64>,

    /// Per-athlete counter for `DataSlice` ids.
    pub(crate) slice_counter: u32,
}

pub struct MostRecentState {
    // STEP 14 fields.
    pub world_time: f64,
    pub speed:      f64,
    pub power:      f64,
    pub heartrate:  u16,
    pub cadence:    u16,
    pub draft:      f64,
    pub distance:   f64,
    pub altitude:   f64,

    // STEP 15 fields.
    pub lat:               f64,
    pub lng:               f64,
    pub road_id:           u32,
    pub road_time:         f64,
    pub reverse:           bool,
    pub event_subgroup_id: Option<u32>,
    pub group_id:          Option<u32>,
    pub time:              f64,        // race timer
    pub event_distance:    f64,
}

pub struct GroupMeta {
    pub id:           u32,
    pub accessed:     f64,
    pub identity_set: HashSet<u32>,   // STEP 15 addition
}
```

### `PlayerStateView` trait (`athlete`)

```rust
/// Read-only accessor surface over the most-recently-seen state.
/// `MostRecentState` implements this directly; STEP 17 may also
/// implement it on `zwift_proto::PlayerState` so the daemon can
/// drive the detectors without translating through the
/// hand-written struct.
pub trait PlayerStateView {
    // STEP 14 surface.
    fn world_time(&self) -> f64;
    fn speed(&self)      -> f64;
    fn power(&self)      -> f64;
    fn heartrate(&self)  -> u16;
    fn cadence(&self)    -> u16;
    fn draft(&self)      -> f64;
    fn distance(&self)   -> f64;
    fn altitude(&self)   -> f64;
    fn lat(&self)        -> f64;
    fn lng(&self)        -> f64;

    // STEP 15 additions.
    fn road_id(&self)           -> u32;
    fn road_time(&self)         -> f64;
    fn reverse(&self)           -> bool;
    fn event_subgroup_id(&self) -> Option<u32>;
    fn group_id(&self)          -> Option<u32>;
    fn time(&self)              -> f64;
    fn event_distance(&self)    -> f64;

    /// `LatLng` convenience built from `lat()` / `lng()`.
    fn latlng(&self) -> LatLng { LatLng { lat: self.lat(), lng: self.lng() } }
}

impl PlayerStateView for MostRecentState { /* … direct field reads … */ }
```

Every STEP 15 detector that reads the last-seen state takes
`&dyn PlayerStateView` or `impl PlayerStateView` so STEP 17 can
later implement the trait on the proto type without touching call
sites.

### Errors

There are no fallible APIs in this step. Every operation returns
the value or `Option<…>`. Same posture as STEP 13 and 14.

## Design decisions worth pre-committing

- **Free functions, not methods.** `compute_groups`, `apply_gap`,
  `start_athlete_lap`, `auto_lap_check`, `active_segment_check`,
  `apply_event_state` are free functions taking `&mut AthleteData`,
  not methods on `AthleteData`. The JS reference has them as
  methods on `StatsProcessor`, which is the orchestrator-level
  type — the Rust equivalent is the daemon (STEP 17).
  `AthleteData` itself stays a focused state record, not a
  god-object.
- **Trait surface for the environment.** `RoadGeometry` and
  `SegmentLookup` are traits, not concrete types. STEP 15 ships
  test stubs only. STEP 17 wires the `zwift-routes`
  implementations. (Resolved decision 2.)
- **Trait surface for state reads.** `PlayerStateView` is the
  read accessor trait every detector uses. `MostRecentState`
  implements it; STEP 17 may also implement it on the proto
  type. (Resolved decision 6.)
- **No async.** Same posture as STEP 13 and 14.
- **No new dependencies.** Everything STEP 15 needs is in the
  standard library or already a dependency of the workspace.
- **Slice id allocator is per-`AthleteData` with athlete-id
  prefix.** A `u32` counter on `AthleteData` plus
  `((athlete_id as u64) << 32) | (counter as u64)` packing.
  Globally unique without coordination, monotonic per athlete,
  self-describing. (Resolved decision 5.)
- **Stateful aggregators keep private fields; POD types are `pub`.**
  Per the project field-visibility convention:
  `ZonesAccumulator`, `WBalAccumulator`, `RoadHistory` are
  stateful and expose private fields through accessors. `Zone`,
  `ZoneTime`, `Segment`, `RoadKey`, `RoadPoint`,
  `RoadComparison`, `NearbyEntry`, `Group`, `DataSlice`,
  `LatLng`, `Streams`, `EventSubgroup`, `EventPrivacy`,
  `EventBehavior`, `MostRecentState` are POD and expose `pub`
  fields.
- **Float comparison policy.** Same as STEP 14: `epsilon = 1e-9`
  for analytic vectors, `epsilon = 1e-6` for fixture-based
  regression.

## Acceptance criteria

- `cargo test -p zwift-stats` is green from a clean checkout.
- Every checklist item 15.1 – 15.28 has at least one test and at
  least one production-code change (or a recorded "no change
  needed" in the as-built notes).
- `tests/step15_regression.rs` passes to ≤ 1e-6 against the
  checked-in fixture (Rust-only regression test).
- No `unsafe`. No `unwrap` outside test code. State-machine
  assertions inside `compute_groups` and
  `compare_road_positions` use `expect("invariant: …")` with a
  stated invariant.
- SPDX header `// SPDX-License-Identifier: AGPL-3.0-only` at the
  top of every new `.rs` file.
- No new dependencies.

## Out of scope for STEP 15

These items are named so that a future reader landing on this
plan can confirm they are not expected here:

- **`zwift-routes` crate stand-up.** STEP 15 ships the trait
  surface (`SegmentLookup`, `RoadGeometry`) and a test stub.
  Real route / curve / segment tables land with `zwift-routes`
  (spec §7.2).
- **Proto-type implementation of `PlayerStateView`.** STEP 15
  defines the trait and implements it for the hand-written
  `MostRecentState`. STEP 17 decides whether to also implement
  the trait directly on `zwift_proto::PlayerState` and bypass
  the hand-written struct, or to keep mapping into it.
- **Daemon orchestration.** STEP 15 ships free functions and
  trait surfaces; STEP 17 wires them into the ingestion loop.
- **FIT export.** Spec §7.1 marks it out of scope for v1.
- **Analysis-only `power.mjs` exports.** STEP 13's "Out of scope
  for ranchero v1" table covers `rank*`, `calcPwHrDecoupling*`,
  `cyclingPowerEstimate*`, `cyclingDraftDragReduction`,
  `seaLevelPower`, `calcWPrimeBalIntegralStatic`, and
  `calcWPrimeBalDifferential` (the one-shot stream form).
- **End-to-end JavaScript comparison.** Same posture as STEP 14:
  sauce4zwift has no session-replay capability (project memory
  "No JavaScript replay capability").

## Wiring into the workspace

- No `Cargo.toml` change needed: `zwift-stats` is already a
  workspace member; STEP 15 introduces no new dependencies.
- The root `ranchero` crate gains a `zwift-stats` dependency
  only when STEP 17 wires the daemon. STEP 15 itself ships no
  CLI surface and no daemon integration.
- License header `// SPDX-License-Identifier: AGPL-3.0-only` at
  the top of every new `.rs` file (matches the rest of the
  workspace).
