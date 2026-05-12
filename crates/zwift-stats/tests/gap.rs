// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{AthleteData, MostRecentState, RoadHistory, RoadKey, RoadGeometry, apply_gap,
                  compare_road_positions, RoadComparison};

struct StubGeometry {
    road_length_m: f64,
}

impl RoadGeometry for StubGeometry {
    fn road_distance(&self, _road: &RoadKey, start_pct: f64, end_pct: f64) -> f64 {
        (end_pct - start_pct).abs() * self.road_length_m
    }
}

fn record_road(
    history: &mut RoadHistory,
    course_id: u32,
    road_id: u32,
    reverse: bool,
    rpct: f64,
    world_time: f64,
) {
    let state = MostRecentState {
        world_time,
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
        road_time: rpct * 1_000_000.0 + 5000.0,
        reverse,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };
    history.record(&state, None);
}

// 15.21-T: apply_gap — gap field derivation

#[test]
fn gap_field_set_from_world_time_delta_in_seconds() {
    let env = StubGeometry { road_length_m: 1000.0 };
    let mut watching = AthleteData::new(1, 1, 1, 0.0, 0.0);
    let mut ad = AthleteData::new(2, 1, 1, 0.0, 0.0);

    // Watching is ahead at rpct=0.6, wt=100_000; target at rpct=0.5, wt=95_000.
    record_road(&mut watching.road_history, 1, 10, false, 0.6, 100_000.0);
    record_road(&mut ad.road_history, 1, 10, false, 0.5, 95_000.0);

    apply_gap(&mut ad, &watching, &env);

    // gap = (100_000 − 95_000) / 1000 = 5.0 seconds
    assert_eq!(ad.gap, Some(5.0));
}

#[test]
fn gap_negated_when_reversed_and_positive() {
    let env = StubGeometry { road_length_m: 1000.0 };
    let mut watching = AthleteData::new(1, 1, 1, 0.0, 0.0);
    let mut ad = AthleteData::new(2, 1, 1, 0.0, 0.0);

    // Target is ahead at rpct=0.6; watching is behind at rpct=0.5 → reversed=true.
    record_road(&mut watching.road_history, 1, 10, false, 0.5, 100_000.0);
    record_road(&mut ad.road_history, 1, 10, false, 0.6, 95_000.0);

    apply_gap(&mut ad, &watching, &env);

    // raw = (100_000 − 95_000) / 1000 = 5.0; negated because reversed=true → −5.0
    assert_eq!(ad.gap, Some(-5.0));
}

#[test]
fn gap_distance_signed_by_direction() {
    let env = StubGeometry { road_length_m: 1000.0 };

    // reversed=false: watching at 0.6 is ahead of target at 0.5 → positive distance
    {
        let mut watching = AthleteData::new(1, 1, 1, 0.0, 0.0);
        let mut ad = AthleteData::new(2, 1, 1, 0.0, 0.0);
        record_road(&mut watching.road_history, 1, 10, false, 0.6, 100_000.0);
        record_road(&mut ad.road_history, 1, 10, false, 0.5, 95_000.0);
        apply_gap(&mut ad, &watching, &env);
        // distance = |0.6 − 0.5| × 1000 = 100; reversed=false → +100
        assert_eq!(ad.gap_distance, Some(100.0));
    }

    // reversed=true: target at 0.6 is ahead of watching at 0.5 → negative distance
    {
        let mut watching = AthleteData::new(1, 1, 1, 0.0, 0.0);
        let mut ad = AthleteData::new(2, 1, 1, 0.0, 0.0);
        record_road(&mut watching.road_history, 1, 10, false, 0.5, 100_000.0);
        record_road(&mut ad.road_history, 1, 10, false, 0.6, 95_000.0);
        apply_gap(&mut ad, &watching, &env);
        // distance = 100; reversed=true → −100
        assert_eq!(ad.gap_distance, Some(-100.0));
    }
}

#[test]
fn is_gap_est_false_when_world_time_match() {
    let env = StubGeometry { road_length_m: 1000.0 };
    let mut watching = AthleteData::new(1, 1, 1, 0.0, 0.0);
    let mut ad = AthleteData::new(2, 1, 1, 0.0, 0.0);

    record_road(&mut watching.road_history, 1, 10, false, 0.6, 100_000.0);
    record_road(&mut ad.road_history, 1, 10, false, 0.5, 95_000.0);

    apply_gap(&mut ad, &watching, &env);

    assert!(!ad.is_gap_est, "direct road comparison should not be marked estimated");
}

#[test]
fn is_gap_est_true_when_world_time_missing() {
    let env = StubGeometry { road_length_m: 1000.0 };
    let mut watching = AthleteData::new(1, 1, 1, 0.0, 0.0);
    let mut ad = AthleteData::new(2, 1, 1, 0.0, 0.0);

    // Watching on road 10, target on road 99 — no shared road history.
    record_road(&mut watching.road_history, 1, 10, false, 0.6, 100_000.0);
    record_road(&mut ad.road_history, 1, 99, false, 0.5, 95_000.0);

    apply_gap(&mut ad, &watching, &env);

    assert!(ad.is_gap_est, "no road match should mark gap as estimated");
    assert_eq!(ad.gap, None, "no road match should leave gap as None");
}

// ---------------------------------------------------------------------------
// 15.20-T: compare_road_positions
// ---------------------------------------------------------------------------

fn make_state_raw(
    course_id: u32,
    road_id: u32,
    reverse: bool,
    rpct: f64,
    world_time: f64,
) -> MostRecentState {
    MostRecentState {
        world_time,
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
        road_time: rpct * 1_000_000.0 + 5000.0,
        reverse,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    }
}

#[test]
fn same_road_same_direction_uses_delta_rpct() {
    let env = StubGeometry { road_length_m: 1000.0 };
    let mut p1 = RoadHistory::new();
    let mut p2 = RoadHistory::new();
    record_road(&mut p1, 1, 10, false, 0.6, 1000.0);
    record_road(&mut p2, 1, 10, false, 0.5, 500.0);

    let result = compare_road_positions(&p1, &p2, &env);

    assert!(result.is_some(), "same road should resolve");
    let rp: RoadComparison = result.unwrap();
    assert!(!rp.reversed, "p1 ahead of p2 → not reversed");
    assert!((rp.distance - 100.0).abs() < 1e-9, "distance = |0.6−0.5| × 1000");
}

#[test]
fn same_road_negative_delta_marks_reversed() {
    let env = StubGeometry { road_length_m: 1000.0 };
    let mut p1 = RoadHistory::new();
    let mut p2 = RoadHistory::new();
    record_road(&mut p1, 1, 10, false, 0.4, 1000.0);
    record_road(&mut p2, 1, 10, false, 0.5, 500.0);

    let result = compare_road_positions(&p1, &p2, &env);

    assert!(result.is_some(), "same road should resolve");
    assert!(result.unwrap().reversed, "p1 behind p2 → reversed");
}

#[test]
fn cross_tier_two_back_resolves_via_b_road() {
    let env = StubGeometry { road_length_m: 1000.0 };

    // p1 was on road 10 (rpct=0.8), then moved to road 20 → road 10 in tier B.
    let s_road10 = make_state_raw(1, 10, false, 0.8, 1000.0);
    let s_road20 = make_state_raw(1, 20, false, 0.1, 2000.0);
    let mut p1 = RoadHistory::new();
    p1.record(&s_road10, None);
    p1.record(&s_road20, Some(&s_road10));

    // p2 is currently on road 10 at rpct=0.5; p1.b last rpct (0.8) ≥ 0.5.
    let mut p2 = RoadHistory::new();
    record_road(&mut p2, 1, 10, false, 0.5, 500.0);

    let result = compare_road_positions(&p1, &p2, &env);

    assert!(result.is_some(), "p2.aRoad == p1.bRoad should resolve");
    assert!(!result.unwrap().reversed, "p1 was ahead on road 10 → not reversed");
}

#[test]
fn cross_tier_three_back_resolves_via_c_road() {
    let env = StubGeometry { road_length_m: 1000.0 };

    // p1 visited road 10 → road 20 → road 30; road 10 ends up in tier C.
    let s10 = make_state_raw(1, 10, false, 0.8, 1000.0);
    let s20 = make_state_raw(1, 20, false, 0.5, 2000.0);
    let s30 = make_state_raw(1, 30, false, 0.1, 3000.0);
    let mut p1 = RoadHistory::new();
    p1.record(&s10, None);
    p1.record(&s20, Some(&s10));
    p1.record(&s30, Some(&s20));

    // p2 is on road 10 at rpct=0.5; p1.c last rpct (0.8) ≥ 0.5.
    let mut p2 = RoadHistory::new();
    record_road(&mut p2, 1, 10, false, 0.5, 500.0);

    let result = compare_road_positions(&p1, &p2, &env);

    assert!(result.is_some(), "p2.aRoad == p1.cRoad should resolve");
}

#[test]
fn no_connection_returns_none() {
    let env = StubGeometry { road_length_m: 1000.0 };
    let mut p1 = RoadHistory::new();
    let mut p2 = RoadHistory::new();
    record_road(&mut p1, 1, 10, false, 0.6, 1000.0);
    record_road(&mut p2, 1, 99, false, 0.5, 500.0);  // unrelated road

    let result = compare_road_positions(&p1, &p2, &env);

    assert!(result.is_none(), "no shared road → None");
}

#[test]
fn boundary_error_term_001_admits_near_matches() {
    let env = StubGeometry { road_length_m: 1000.0 };

    // p1 was on road 10 at rpct=0.495 (just below p2.rpct=0.5), then moved to road 20.
    // Without the 0.01 boundary term: 0.495 < 0.5 → no match.
    // With the boundary term: 0.495 ≥ 0.5 − 0.01 = 0.49 → match.
    let s10 = make_state_raw(1, 10, false, 0.495, 1000.0);
    let s20 = make_state_raw(1, 20, false, 0.1, 2000.0);
    let mut p1 = RoadHistory::new();
    p1.record(&s10, None);
    p1.record(&s20, Some(&s10));

    let mut p2 = RoadHistory::new();
    record_road(&mut p2, 1, 10, false, 0.5, 500.0);

    let result = compare_road_positions(&p1, &p2, &env);

    assert!(result.is_some(), "rpct within 0.01 slop should resolve");
}
