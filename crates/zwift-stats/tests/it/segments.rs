// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use zwift_stats::{AthleteData, MostRecentState, active_segment_check, stop_segment, Segment, SegmentLookup};

// Test-only in-memory SegmentLookup implementation.
struct InMemoryEnv {
    by_road: HashMap<(u32, u32, bool), Vec<Segment>>,
    by_id: HashMap<u32, Segment>,
}

impl InMemoryEnv {
    fn new() -> Self {
        InMemoryEnv { by_road: HashMap::new(), by_id: HashMap::new() }
    }

    fn add(&mut self, seg: Segment) {
        self.by_road
            .entry((seg.course_id, seg.road_id, seg.reverse))
            .or_default()
            .push(seg.clone());
        self.by_id.insert(seg.id, seg);
    }
}

impl SegmentLookup for InMemoryEnv {
    fn road_segments(&self, course_id: u32, road_id: u32, reverse: bool) -> &[Segment] {
        self.by_road
            .get(&(course_id, road_id, reverse))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn segment(&self, id: u32) -> Option<&Segment> {
        self.by_id.get(&id)
    }
}

fn make_state(course_id: u32, road_id: u32, road_time: f64) -> MostRecentState {
    MostRecentState {
        world_time: 0.0,
        speed: 0.0,
        power: 0.0,
        heartrate: 0,
        cadence: 0,
        draft: 0.0,
        distance: 0.0,
        altitude: 0.0,
        lat: 0.0,
        lng: 0.0,
        course_id,
        road_id,
        road_time,
        reverse: false,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    }
}

// road_time = p * 1_000_000 + 5000 where p is the 0..1 road completion fraction
fn road_time_at(progress: f64) -> f64 {
    progress * 1_000_000.0 + 5_000.0
}

// 15.18-T: active_segment_check — opening logic

#[test]
fn start_within_first_5_percent_opens_slice() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 2000.0 });

    // progress = 4% — within first 5%, segment should open
    let state = make_state(42, 10, road_time_at(0.04));
    active_segment_check(&mut ad, &state, &env, 10.0);

    assert_eq!(ad.active_segments.len(), 1, "should open segment within first 5%");
}

#[test]
fn start_within_first_150m_overrides_5_percent_for_short_segments() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    // distance = 500 m => 150 / 500 = 0.30 threshold, larger than 5%
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 500.0 });

    // progress = 15% — beyond 5% but within 150 m / 500 m = 30%
    let state = make_state(42, 10, road_time_at(0.15));
    active_segment_check(&mut ad, &state, &env, 10.0);

    assert_eq!(ad.active_segments.len(), 1, "150 m override should open short segment");
}

#[test]
fn outside_progress_does_not_open_slice() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    // Segment occupies only the back half of the road
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.5, road_finish: 1.0, distance: 1000.0 });

    // progress = (0.3 - 0.5) / (1.0 - 0.5) = -0.4, i.e. before the segment
    let state = make_state(42, 10, road_time_at(0.3));
    active_segment_check(&mut ad, &state, &env, 10.0);

    assert!(ad.active_segments.is_empty(), "should not open segment before it starts");
}

#[test]
fn exit_after_open_stops_segment() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 0.5, distance: 1000.0 });

    // Open: progress = 2% (within first 5% of segment)
    let open_state = make_state(42, 10, road_time_at(0.01));
    active_segment_check(&mut ad, &open_state, &env, 10.0);
    assert_eq!(ad.active_segments.len(), 1, "should open segment");

    // Exit: past segment finish so progress > 1
    let exit_state = make_state(42, 10, road_time_at(0.6));
    active_segment_check(&mut ad, &exit_state, &env, 20.0);
    assert!(ad.active_segments.is_empty(), "should close segment on exit");
    assert_eq!(ad.segment_slices.len(), 1, "closed slice should remain in segment_slices");
}

#[test]
fn multiple_concurrent_segments_tracked_independently() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 0.5, distance: 1000.0 });
    env.add(Segment { id: 2, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 2000.0 });

    // Both segments start at 0.0, so at 3% progress both are in their opening zone
    let state = make_state(42, 10, road_time_at(0.03));
    active_segment_check(&mut ad, &state, &env, 10.0);

    assert_eq!(ad.active_segments.len(), 2, "both segments should be active concurrently");
}

// 15.19-T: stop_segment — completion thresholds

#[test]
fn stop_marks_incomplete_when_no_road_history() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 2000.0 });

    // Open the segment
    let open = make_state(42, 10, road_time_at(0.02));
    active_segment_check(&mut ad, &open, &env, 10.0);

    // Close it — road_history is empty, so stop_segment should mark incomplete
    let close = make_state(42, 99, road_time_at(0.0));  // different road_id exits all segments
    active_segment_check(&mut ad, &close, &env, 20.0);

    let slice = ad.segment_slices.last().expect("segment slice present");
    assert!(slice.incomplete, "no road history should mark incomplete");
}

#[test]
fn stop_marks_complete_when_long_segment_at_or_above_90_percent() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    // Long segment (> 1000 m) requires 90% completion
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 1500.0 });

    let open = make_state(42, 10, road_time_at(0.02));
    active_segment_check(&mut ad, &open, &env, 10.0);

    // Rider reaches 92% before exiting
    let near_end = make_state(42, 10, road_time_at(0.92));
    ad.road_history.record(&near_end, Some(&open));

    let exit = make_state(42, 99, road_time_at(0.0));
    active_segment_check(&mut ad, &exit, &env, 20.0);

    let slice = ad.segment_slices.last().expect("segment slice present");
    assert!(!slice.incomplete, "92% on long segment should be complete");
}

#[test]
fn stop_marks_incomplete_when_long_segment_below_90_percent() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 1500.0 });

    let open = make_state(42, 10, road_time_at(0.02));
    active_segment_check(&mut ad, &open, &env, 10.0);

    // Rider only reaches 80% before exiting
    let partial = make_state(42, 10, road_time_at(0.80));
    ad.road_history.record(&partial, Some(&open));

    let exit = make_state(42, 99, road_time_at(0.0));
    active_segment_check(&mut ad, &exit, &env, 20.0);

    let slice = ad.segment_slices.last().expect("segment slice present");
    assert!(slice.incomplete, "80% on long segment should be incomplete");
}

#[test]
fn stop_thresholds_60_percent_for_400_to_1000m() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    // Mid-range segment (400–1000 m) requires 60%
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 700.0 });

    let open = make_state(42, 10, road_time_at(0.02));
    active_segment_check(&mut ad, &open, &env, 10.0);

    // Rider reaches 62%
    let progress = make_state(42, 10, road_time_at(0.62));
    ad.road_history.record(&progress, Some(&open));

    let exit = make_state(42, 99, road_time_at(0.0));
    active_segment_check(&mut ad, &exit, &env, 20.0);

    let slice = ad.segment_slices.last().expect("segment slice present");
    assert!(!slice.incomplete, "62% on 700 m segment should be complete");
}

#[test]
fn stop_thresholds_25_percent_for_below_400m() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    // Short segment (< 400 m) requires only 25%
    env.add(Segment { id: 1, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 200.0 });

    let open = make_state(42, 10, road_time_at(0.02));
    active_segment_check(&mut ad, &open, &env, 10.0);

    // Rider reaches 30%
    let progress = make_state(42, 10, road_time_at(0.30));
    ad.road_history.record(&progress, Some(&open));

    let exit = make_state(42, 99, road_time_at(0.0));
    active_segment_check(&mut ad, &exit, &env, 20.0);

    let slice = ad.segment_slices.last().expect("segment slice present");
    assert!(!slice.incomplete, "30% on 200 m segment should be complete");
}

#[test]
fn stop_walks_road_history_a_b_c_until_match() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut env = InMemoryEnv::new();
    // Segment is on road 20 (not the road the athlete is on when the segment opens)
    env.add(Segment { id: 1, course_id: 42, road_id: 20, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 1500.0 });

    // Athlete was on road 20 and crossed 92% — recorded in road history
    let on_seg_road = MostRecentState {
        road_id: 20,
        road_time: road_time_at(0.92),
        ..make_state(42, 20, road_time_at(0.92))
    };
    ad.road_history.record(&on_seg_road, None);

    // Now athlete is on road 10 — open segment (uses road 20 history)
    env.add(Segment { id: 2, course_id: 42, road_id: 10, reverse: false, road_start: 0.0, road_finish: 1.0, distance: 500.0 });
    let open = make_state(42, 10, road_time_at(0.02));
    active_segment_check(&mut ad, &open, &env, 10.0);

    // Stop segment 1 directly — road_history tier should have road 20 entry
    stop_segment(&mut ad, &open, 1, &env, 20.0);

    let slice = ad.segment_slices.iter().find(|s| s.segment_id == Some(1)).expect("slice for seg 1");
    assert!(!slice.incomplete, "road 20 at 92% found in road history should be complete");
}
