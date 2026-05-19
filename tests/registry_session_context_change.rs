// SPDX-License-Identifier: AGPL-3.0-only
//! 17.29-T — A world (course) or sport change between consecutive states for
//! the same athlete triggers a session-context update inside
//! `route_player_state`.
//!
//! Two scenarios are tested:
//!
//! **A. World change only (`world` changes, `sport` stays the same)**
//! - First call: athlete 3001, world=3, sport=0, distance=1000 m, z=500
//!   (altitude=5.0 m), power=300 W.  The distance delta of 1000 m with a
//!   5 m altitude gain produces a non-zero smooth-grade value.
//! - Second call: same athlete, world=5, sport=0, distance=0, z=0,
//!   power=100 W.
//!
//! After the second call:
//! - `course_id` updated to 5.
//! - `sport` remains 0 (unchanged).
//! - `lap_slices.len()` == 2 (initial lap + one context-change lap).
//! - `distance_offset` == 1000.0 (bumped by the first call's distance).
//! - `smooth_grade.get()` == 0.0 (accumulator reset; grade forced to zero
//!   for this frame, matching sauce4zwift's `state.grade = 0` line in
//!   `_preprocessState`).
//! - Power bucket retains the first call's 300 W peak (collectors are NOT
//!   reset on context change).
//!
//! **B. Sport change**
//! - Same structure; `sport` changes from 0 to 1 while `world` stays the
//!   same.  Verifies that `sport` is overwritten and a new lap is started.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.29-T.

use std::sync::Arc;

use ranchero::web::{proto_to_stats, WebState};
use zwift_proto::PlayerState;
use zwift_stats::ExpWeightedAvg;
use zwift_stats::periods::SMOOTH_GRADE_WINDOW;

fn make_proto(id: i64, world: i32, sport: i32, distance: i32, z: f32, power: i32) -> PlayerState {
    PlayerState {
        id:       Some(id),
        world:    Some(world),
        sport:    Some(sport),
        distance: Some(distance),
        z:        Some(z),
        power:    Some(power),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Scenario A — world change
// ---------------------------------------------------------------------------

#[test]
fn world_change_triggers_session_context_update() {
    let state = Arc::new(WebState::new());

    // First call: course 3, 1000 m distance, altitude 5 m (z=500), power 300 W.
    // dist_delta = 1000, alt_delta = 5.0  →  raw_grade = 0.005.
    let p1 = make_proto(3001, 3, 0, 1000, 500.0, 300);
    proto_to_stats::route_player_state(&p1, &state, 1.0, 0);

    // Confirm the first call produced a non-zero grade so the later reset
    // assertion is meaningful.
    let grade_after_first = {
        let registry = state.registry.read().unwrap();
        let ad = registry.get(3001).expect("athlete must exist after first call");
        ad.smooth_grade.get()
    };
    let mut reference_ewa = ExpWeightedAvg::new(SMOOTH_GRADE_WINDOW, 0.0);
    let expected_first_grade = reference_ewa.update(0.005);
    assert!(
        (grade_after_first - expected_first_grade).abs() < 1e-9,
        "smooth_grade after first call must be {expected_first_grade}; got {grade_after_first}",
    );
    assert!(
        grade_after_first.abs() > 1e-9,
        "smooth_grade after first call must be non-zero for the reset assertion to be meaningful",
    );

    // Second call: world changes to 5; distance and altitude reset to 0 at
    // the start of the new course.  Power drops to 100 W.
    let p2 = make_proto(3001, 5, 0, 0, 0.0, 100);
    proto_to_stats::route_player_state(&p2, &state, 2.0, 0);

    let mut registry = state.registry.write().unwrap();
    let ad = registry.get_mut(3001).expect("athlete must exist after second call");
    ad.bucket.flush_all();

    // course_id updated; sport unchanged.
    assert_eq!(ad.course_id, 5, "course_id must be updated to the new world");
    assert_eq!(ad.sport, 0,  "sport must remain 0 (it did not change)");

    // A new lap must have been started: initial lap (from AthleteData::new)
    // plus one context-change lap.
    assert_eq!(
        ad.lap_slices.len(), 2,
        "a new lap must be started on context change; got {} slices",
        ad.lap_slices.len()
    );

    // distance_offset must be bumped by the first call's proto distance (1000).
    assert!(
        (ad.distance_offset - 1000.0).abs() < 1e-9,
        "distance_offset must be 1000.0 after context change; got {}",
        ad.distance_offset
    );

    // smooth_grade must be 0.0: the accumulator is reset and the grade is
    // forced to zero for the context-change frame (matches sauce4zwift's
    // `state.grade = 0` in `_preprocessState`).
    assert!(
        ad.smooth_grade.get().abs() < 1e-9,
        "smooth_grade must be 0.0 after context change; got {}",
        ad.smooth_grade.get()
    );

    // Power bucket must NOT have been reset: the 300 W peak from the first
    // call must still be present.
    assert!(
        (ad.bucket.power_mut().max_value() - 300.0).abs() < 1e-9,
        "power bucket must retain 300 W peak from first call; got {}",
        ad.bucket.power_mut().max_value()
    );
}

// ---------------------------------------------------------------------------
// Scenario B — sport change
// ---------------------------------------------------------------------------

#[test]
fn sport_change_triggers_session_context_update() {
    let state = Arc::new(WebState::new());

    // First call: course 7, sport 0 (cycling), some distance and power.
    let p1 = make_proto(3002, 7, 0, 500, 0.0, 200);
    proto_to_stats::route_player_state(&p1, &state, 1.0, 0);

    // Second call: same world, sport changes to 1 (running).
    let p2 = make_proto(3002, 7, 1, 0, 0.0, 150);
    proto_to_stats::route_player_state(&p2, &state, 2.0, 0);

    let registry = state.registry.read().unwrap();
    let ad = registry.get(3002).expect("athlete must exist after second call");

    assert_eq!(ad.course_id, 7, "course_id must remain 7 (world did not change)");
    assert_eq!(ad.sport, 1, "sport must be updated to 1");

    assert_eq!(
        ad.lap_slices.len(), 2,
        "a new lap must be started on sport change; got {} slices",
        ad.lap_slices.len()
    );

    assert!(
        (ad.distance_offset - 500.0).abs() < 1e-9,
        "distance_offset must be 500.0 after sport change; got {}",
        ad.distance_offset
    );

    assert!(
        ad.smooth_grade.get().abs() < 1e-9,
        "smooth_grade must be 0.0 after sport change; got {}",
        ad.smooth_grade.get()
    );
}
