// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{AthleteData, start_athlete_lap};

// 15.16: Manual lap starting

#[test]
fn start_athlete_lap_closes_open_slice_and_appends_new() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);
    let initial_count = ad.lap_slices.len();

    // When start_athlete_lap is called, the current open lap should be closed
    // and a new lap slice should be appended
    start_athlete_lap(&mut ad, 15.0);

    assert_eq!(ad.lap_slices.len(), initial_count + 1, "should append new lap slice");
    assert!(ad.lap_slices[0].end.is_some(), "previous slice should have end timestamp");
}

#[test]
fn start_athlete_lap_returns_new_slice_id() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When start_athlete_lap is called, it should return the id of the new slice
    let new_id = start_athlete_lap(&mut ad, 15.0);

    assert_eq!(new_id, ad.lap_slices[1].id, "should return new slice id");
}

#[test]
fn start_athlete_lap_clones_bucket_via_clone_reset() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When start_athlete_lap is called, the new slice should have a clone_reset bucket
    start_athlete_lap(&mut ad, 15.0);

    assert_eq!(ad.lap_slices.len(), 2, "should have two lap slices");
    assert!(ad.lap_slices[1].end.is_none(), "new slice should not have end timestamp yet");
}

// 15.17: Automatic lap detection

use zwift_stats::{auto_lap_check, AutoLapConfig, AutoLapMetric, MostRecentState};

#[test]
fn auto_lap_by_distance_threshold_triggers_at_each_interval() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);
    let state = MostRecentState {
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
        course_id: 0,
        road_id: 1,
        road_time: 0.0,
        reverse: false,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    };

    // First call seeds the mark
    let cfg = AutoLapConfig { metric: AutoLapMetric::Distance, threshold: 10000.0 };
    auto_lap_check(&mut ad, &state, &cfg, 10.0);
    assert_eq!(ad.auto_lap_mark, Some(0.0), "first call should seed mark");

    // When distance exceeds threshold, lap should trigger
    let mut state2 = state;
    state2.event_distance = 10000.0;
    let lapped = auto_lap_check(&mut ad, &state2, &cfg, 20.0);
    assert!(lapped, "should trigger lap at distance threshold");
}

#[test]
fn auto_lap_by_time_threshold_triggers_at_each_interval() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);
    let state = MostRecentState {
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
        course_id: 0,
        road_id: 1,
        road_time: 0.0,
        reverse: false,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    };

    // First call seeds the mark
    let cfg = AutoLapConfig { metric: AutoLapMetric::Time, threshold: 3600.0 };
    auto_lap_check(&mut ad, &state, &cfg, 10.0);
    assert_eq!(ad.auto_lap_mark, Some(0.0), "first call should seed mark");

    // When time exceeds threshold, lap should trigger
    let mut state2 = state;
    state2.time = 3600.0;
    let lapped = auto_lap_check(&mut ad, &state2, &cfg, 3610.0);
    assert!(lapped, "should trigger lap at time threshold");
}

#[test]
fn auto_lap_mark_resets_on_course_change() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);
    let state = MostRecentState {
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
        course_id: 0,
        road_id: 1,
        road_time: 0.0,
        reverse: false,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    };

    let cfg = AutoLapConfig { metric: AutoLapMetric::Distance, threshold: 10000.0 };
    auto_lap_check(&mut ad, &state, &cfg, 10.0);
    assert!(ad.auto_lap_mark.is_some(), "should have mark after first call");

    // Course change should reset the mark
    let mut state2 = state;
    state2.road_id = 2;
    auto_lap_check(&mut ad, &state2, &cfg, 20.0);
    // After course change, the mark should be reset
}

#[test]
fn auto_lap_first_call_seeds_mark_without_lapping() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);
    let state = MostRecentState {
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
        course_id: 0,
        road_id: 1,
        road_time: 0.0,
        reverse: false,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    };

    let cfg = AutoLapConfig { metric: AutoLapMetric::Distance, threshold: 10000.0 };

    // The first call should seed the mark without creating a lap
    let lapped = auto_lap_check(&mut ad, &state, &cfg, 10.0);
    assert!(!lapped, "first call should not trigger lap");
    assert_eq!(ad.auto_lap_mark, Some(0.0), "first call should seed mark with event_distance");
}

