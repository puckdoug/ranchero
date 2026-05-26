// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 15 — `active_segment_check` is wired into `route_player_state`.
//
// Crossing a segment boundary while the watched athlete is on the segment's
// road must open and then close a segment slice on the athlete's
// `AthleteData`. The check logic itself is exercised in
// `crates/zwift-stats/tests/segments.rs`; this test wires it into the
// production path through `route_player_state`, which is what Step 15 (item ②)
// has to add.
//
// The plan notes this test is "gated on Step 18 geometry" — meaning the real
// segment geometry tables ship in Step 18. We sidestep that here by attaching
// a hand-rolled in-memory `SegmentLookup` to `WebState` (the same trick used
// by the zwift-stats unit tests), so the wiring assertion does not depend on
// a vendored table landing first.
//
// Fails today because:
//   1. `WebState` has no segment-env attachment point.
//   2. `route_player_state` does not call `active_segment_check`.

use std::collections::HashMap;
use std::sync::Arc;

use ranchero::web::WebState;
use ranchero::web::proto_to_stats::route_player_state;
use zwift_proto::PlayerState;
use zwift_stats::{Segment, SegmentLookup};

// ---------------------------------------------------------------------------
// In-memory segment env (same shape as the zwift-stats unit tests use).
// ---------------------------------------------------------------------------

struct InMemoryEnv {
    by_road: HashMap<(u32, u32, bool), Vec<Segment>>,
    by_id:   HashMap<u32, Segment>,
}

impl InMemoryEnv {
    fn new() -> Self {
        Self { by_road: HashMap::new(), by_id: HashMap::new() }
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

// road_time wire encoding (sauce zwift.mjs:321):
// road_time = progress * 1_000_000 + 5_000 on forward roads.
fn road_time_at(progress: f64) -> u32 {
    (progress * 1_000_000.0 + 5_000.0) as u32
}

fn make_player_state(
    athlete_id: i64,
    course_id:  i32,
    world_time: u64,
    road_id:    u32,
    road_time:  u32,
    distance:   u32,
) -> PlayerState {
    PlayerState {
        id:          Some(athlete_id),
        world:       Some(course_id),
        world_time:  Some(world_time as i64),
        road_time:   Some(road_time as i32),
        distance:    Some(distance as i32),
        // The "current road" is packed in f19's bits in production wire
        // frames; we use proto_view's exposed accessors via the public
        // `route_player_state` path, so we set the underlying fields that
        // make `ProtoView::road_id` / `reverse` / `course_id` resolve to
        // the test values. If the implementer needs a different proto
        // field to drive these, the test will need a corresponding tweak.
        f19:         Some(road_id),
        ..Default::default()
    }
}

// ==========================================================================
// Wiring test
// ==========================================================================

/// After two `route_player_state` calls — one that enters the start window
/// of a segment and one that exits past the finish — the athlete's
/// `segment_slices` contains a closed slice for that segment.
#[test]
fn active_segment_detected_through_route_player_state() {
    // Build a WebState that owns an InMemoryEnv via the new (Step 15)
    // SegmentLookup attachment point. The exact builder name is the
    // implementer's choice; this test expects `with_segment_env`.
    let mut env = InMemoryEnv::new();
    env.add(Segment {
        id:          1,
        course_id:   42,
        road_id:     10,
        reverse:     false,
        road_start:  0.0,
        road_finish: 1.0,
        distance:    2_000.0,
    });

    let athlete_id: i64 = 99;
    let state = WebState::new().with_segment_env(Arc::new(env));
    // The watched athlete must be the one we're feeding state for, so the
    // wiring's "ingest produces a slice on AthleteData" assertion holds.
    let state = Arc::new(state.with_watching(athlete_id as u32));

    // Frame 1: inside the start window (4% progress) on road 10, course 42.
    let p1 = make_player_state(athlete_id, 42, 1_000, 10, road_time_at(0.04), 0);
    route_player_state(&p1, &state, 1.0, 1_000);

    // Frame 2: past the finish line (110% progress). active_segment_check
    // closes the slice and pushes it into `segment_slices`.
    let p2 = make_player_state(athlete_id, 42, 2_000, 10, road_time_at(1.10), 2_200);
    route_player_state(&p2, &state, 2.0, 2_000);

    // Inspect the athlete's data and assert a slice was recorded.
    let registry = state.registry.read().unwrap();
    let ad = registry
        .get(athlete_id as u32)
        .expect("athlete must be registered after route_player_state");
    assert!(
        ad.segment_slices.iter().any(|s| s.segment_id == Some(1)),
        "crossing segment 1 must push a closed slice into segment_slices; got {:?}",
        ad.segment_slices.iter().map(|s| s.segment_id).collect::<Vec<_>>(),
    );
    assert!(
        ad.active_segments.is_empty(),
        "after exiting the segment, active_segments must be empty",
    );
}
