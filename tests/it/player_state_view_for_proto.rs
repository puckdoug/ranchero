// SPDX-License-Identifier: AGPL-3.0-only
//! 17.30-T — `impl PlayerStateView for zwift_proto::PlayerState` produces the
//! same `active_segments` outcome as `MostRecentState` carrying identical
//! logical values.
//!
//! The test builds:
//! - A `zwift_proto::PlayerState` with explicit proto field values.
//! - A `MostRecentState` with the equivalent decoded values.
//!
//! Both are passed to `active_segment_check` against a fresh `AthleteData` and
//! a single-segment lookup.  The test then asserts that:
//! 1. The resulting `active_segments` key sets are identical.
//! 2. Segment 42 is open after the call (the rider is inside the opening window).
//!
//! Proto field encoding used in the fixture:
//! - `world`     → `course_id`      (direct cast)
//! - `road_time` → `road_time()`    (direct cast to f64)
//! - `world_time`→ `world_time()`   (ms → s: / 1000.0)
//! - `aux3`      → `road_id()` via `decode_road_id`: `(aux3 >> 8) & 0xffff`
//!   For road_id = 7: `aux3 = 7 << 8 = 1792`
//! - `f19`       → `reverse()` via `decode_reverse`: `(f19 & 0b100) == 0`
//!   For reverse = false (forward): bit 2 must be set → `f19 = 4`
//!
//! Segment geometry:
//!   course_id=3, road_id=7, reverse=false, road_start=0.0, road_finish=1.0,
//!   distance=2000 m.
//! Opening threshold = max(SEGMENT_START_WINDOW_PCT=0.05,
//!                         SEGMENT_START_WINDOW_METRES/2000=0.075) = 0.075.
//! road_time = 35000 → rpct = (35000 - 5000) / 1_000_000 = 0.03 (inside window).
//!
//! Fails to compile until `ProtoView` implementing `PlayerStateView` is added
//! in `src/web/proto_view.rs` (item 17.30-I).  Rust's orphan rule prevents
//! implementing a foreign trait for a foreign type directly, so a newtype
//! `ProtoView<'a>(&'a PlayerState)` is used instead.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.30-T.

use std::collections::BTreeSet;

use zwift_proto::PlayerState;
use zwift_stats::{
    AthleteData, MostRecentState, PlayerStateView,
    active_segment_check, Segment, SegmentLookup,
};
use ranchero::web::proto_view::ProtoView;

// ---------------------------------------------------------------------------
// Fixture builder
// ---------------------------------------------------------------------------

/// Build a `PlayerState` with the fields `active_segment_check` reads.
///
/// - `course_id` → `proto.world`
/// - `road_id`   → encoded in `proto.aux3`: `road_id << 8`
/// - `reverse`   → encoded in `proto.f19`: forward bit (bit 2); `reverse=false`
///   requires bit 2 set → `f19 = 4`; `reverse=true` → `f19 = 0`
/// - `road_time` → `proto.road_time` (raw integer, cast to f64 by the impl)
/// - `world_time`→ `proto.world_time` (ms; impl divides by 1000)
fn make_state(
    course_id:     i32,
    road_id:       u32,
    reverse:       bool,
    road_time_raw: i32,
    world_time_ms: i64,
) -> PlayerState {
    let aux3 = road_id << 8;
    let f19  = if reverse { 0u32 } else { 4u32 };
    PlayerState {
        world:      Some(course_id),
        road_time:  Some(road_time_raw),
        aux3:       Some(aux3),
        f19:        Some(f19),
        world_time: Some(world_time_ms),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Minimal SegmentLookup for the test
// ---------------------------------------------------------------------------

struct SingleSegmentEnv(Segment);

impl SegmentLookup for SingleSegmentEnv {
    fn road_segments(&self, course_id: u32, road_id: u32, reverse: bool) -> &[Segment] {
        let s = &self.0;
        if s.course_id == course_id && s.road_id == road_id && s.reverse == reverse {
            std::slice::from_ref(s)
        } else {
            &[]
        }
    }

    fn segment(&self, id: u32) -> Option<&Segment> {
        if self.0.id == id { Some(&self.0) } else { None }
    }
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
fn active_segment_check_proto_and_most_recent_state_produce_identical_result() {
    let env = SingleSegmentEnv(Segment {
        id:          42,
        course_id:   3,
        road_id:     7,
        reverse:     false,
        road_start:  0.0,
        road_finish: 1.0,
        distance:    2000.0,
    });

    // road_time = 35000 → rpct = (35000 - 5000) / 1_000_000 = 0.03
    // Opening window = max(0.05, 150/2000) = 0.075 → rider is inside.
    let proto = make_state(3, 7, false, 35000, 1000);

    let mrs = MostRecentState {
        world_time:        1.0,
        speed:             0.0,
        power:             0.0,
        heartrate:         0,
        cadence:           0,
        draft:             0.0,
        distance:          0.0,
        altitude:          0.0,
        lat:               0.0,
        lng:               0.0,
        course_id:         3,
        road_id:           7,
        road_time:         35000.0,
        reverse:           false,
        event_subgroup_id: 0,
        group_id:          0,
        time:              0.0,
        event_distance:    0.0,
        grade:             0.0,
        x:                 0.0,
        y:                 0.0,
        progress:          0.0,
    };

    let mut ad_proto = AthleteData::new(1, 3, 0, 1.0, 1.0);
    let mut ad_mrs   = AthleteData::new(2, 3, 0, 1.0, 1.0);

    active_segment_check(&mut ad_proto, &ProtoView(&proto) as &dyn PlayerStateView, &env, 1.0);
    active_segment_check(&mut ad_mrs,   &mrs              as &dyn PlayerStateView, &env, 1.0);

    let keys_proto: BTreeSet<u32> = ad_proto.active_segments.keys().copied().collect();
    let keys_mrs:   BTreeSet<u32> = ad_mrs.active_segments.keys().copied().collect();

    assert_eq!(
        keys_proto, keys_mrs,
        "active_segments key set must be identical for proto view and MostRecentState"
    );

    // Confirm the segment was actually opened (not a vacuous empty == empty).
    assert!(
        keys_proto.contains(&42),
        "segment 42 must be open: rider at rpct=0.03 is inside the 0.075 opening window"
    );
}
