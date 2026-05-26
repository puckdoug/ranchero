// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use zwift_stats::{
    AthleteData, MostRecentState,
    EventBehavior, EventStateOutcome,
    apply_event_state,
};
use zwift_stats::athlete::EventSubgroup;

fn make_state(time: f64, event_subgroup_id: u32, event_distance: f64) -> MostRecentState {
    MostRecentState {
        world_time: time * 1000.0,
        speed: 0.0,
        power: 0.0,
        heartrate: 0,
        cadence: 0,
        draft: 0.0,
        distance: 0.0,
        altitude: 0.0,
        lat: 0.0,
        lng: 0.0,
        course_id: 1,
        road_id: 1,
        road_time: 5000.0,
        reverse: false,
        event_subgroup_id,
        group_id: 0,
        time,
        event_distance,
        grade: 0.0,
    }
}

fn no_behavior() -> EventBehavior {
    EventBehavior { auto_reset: false, auto_lap: false }
}

fn make_subgroup(id: u32) -> EventSubgroup {
    EventSubgroup {
        id,
        course_id: 1,
        ..Default::default()
    }
}

fn sg_map(sg: EventSubgroup) -> HashMap<u32, EventSubgroup> {
    let mut m = HashMap::new();
    m.insert(sg.id, sg);
    m
}

// 15.14-T: new subgroup with time > 0 opens an event slice.
#[test]
fn new_subgroup_opens_slice_when_state_time_present() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let state = make_state(5.0, 7, 0.0);
    let sgs = sg_map(make_subgroup(7));

    let outcome = apply_event_state(&mut ad, &state, 99, &sgs, no_behavior(), 10.0, 0);

    assert!(
        matches!(outcome, EventStateOutcome::Started { .. }),
        "expected Started, got {outcome:?}",
    );
    assert_eq!(ad.event_slices.len(), 1, "should open one event slice");
    assert!(ad.event_subgroup.is_some(), "event_subgroup should be set");
}

// 15.14-T: new subgroup with time == 0 defers start and sets pending flag.
#[test]
fn new_subgroup_defers_when_state_time_zero_and_sets_pending() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let state = make_state(0.0, 7, 0.0);
    let sgs = sg_map(make_subgroup(7));

    let outcome = apply_event_state(&mut ad, &state, 99, &sgs, no_behavior(), 10.0, 0);

    assert!(
        matches!(outcome, EventStateOutcome::StartPending),
        "expected StartPending, got {outcome:?}",
    );
    assert!(ad.event_start_pending, "event_start_pending should be true");
    assert!(ad.event_slices.is_empty(), "no slice opened when deferred");
}

// 15.14-T: same subgroup ID on successive calls does not reopen the slice.
#[test]
fn same_subgroup_does_not_reopen_slice() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let sgs = sg_map(make_subgroup(7));

    apply_event_state(&mut ad, &make_state(5.0, 7, 0.0), 99, &sgs, no_behavior(), 10.0, 0);
    let outcome = apply_event_state(&mut ad, &make_state(6.0, 7, 0.0), 99, &sgs, no_behavior(), 11.0, 0);

    assert!(
        matches!(outcome, EventStateOutcome::Idle),
        "second call with same subgroup should be Idle, got {outcome:?}",
    );
    assert_eq!(ad.event_slices.len(), 1, "still only one event slice");
}

// 15.14-T: subgroup_id 0 after an active event closes the open slice.
#[test]
fn falsy_subgroup_after_active_closes_slice() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let sgs = sg_map(make_subgroup(7));

    apply_event_state(&mut ad, &make_state(5.0, 7, 0.0), 99, &sgs, no_behavior(), 10.0, 0);
    let outcome = apply_event_state(
        &mut ad, &make_state(10.0, 0, 0.0), 99, &HashMap::new(), no_behavior(), 15.0, 0,
    );

    assert!(
        matches!(outcome, EventStateOutcome::Ended { .. }),
        "expected Ended, got {outcome:?}",
    );
    assert_eq!(ad.event_slices.len(), 1, "closed slice present");
    assert!(ad.event_slices[0].end.is_some(), "slice should have an end timestamp");
    assert!(ad.event_subgroup.is_none(), "event_subgroup cleared");
}

// 15.14-T: slice auto-closes when event_distance exceeds end_distance.
#[test]
fn auto_end_by_distance_closes_slice() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut sg = make_subgroup(7);
    sg.end_distance = 1000.0;
    let sgs = sg_map(sg);

    apply_event_state(&mut ad, &make_state(5.0, 7, 0.0), 99, &sgs, no_behavior(), 10.0, 0);
    let outcome = apply_event_state(
        &mut ad, &make_state(10.0, 7, 1001.0), 99, &sgs, no_behavior(), 15.0, 0,
    );

    assert!(
        matches!(outcome, EventStateOutcome::Ended { .. }),
        "expected Ended when distance exceeded, got {outcome:?}",
    );
    assert!(ad.event_slices[0].end.is_some(), "slice closed by distance");
}

// 15.14-T: slice auto-closes when wall_clock_ms exceeds end_ts.
#[test]
fn auto_end_by_wall_clock_closes_slice() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    let mut sg = make_subgroup(7);
    sg.end_ts = 5000;
    let sgs = sg_map(sg);

    apply_event_state(&mut ad, &make_state(5.0, 7, 0.0), 99, &sgs, no_behavior(), 10.0, 0);
    let outcome = apply_event_state(
        &mut ad, &make_state(10.0, 7, 0.0), 99, &sgs, no_behavior(), 15.0, 6000,
    );

    assert!(
        matches!(outcome, EventStateOutcome::Ended { .. }),
        "expected Ended when wall clock exceeded, got {outcome:?}",
    );
    assert!(ad.event_slices[0].end.is_some(), "slice closed by wall clock");
}

// 15.14-T: auto_reset=true resets the athlete bucket when the event starts.
#[test]
fn behavior_auto_reset_resets_athlete_data_on_event_start() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    ad.bucket.ingest_power(10.0, 300.0);
    let sgs = sg_map(make_subgroup(7));
    let behavior = EventBehavior { auto_reset: true, auto_lap: false };

    apply_event_state(&mut ad, &make_state(10.0, 7, 0.0), 99, &sgs, behavior, 10.0, 0);

    assert_eq!(
        ad.bucket.power().max_value(), 0.0,
        "bucket should be cleared by auto_reset",
    );
}

// 15.14-T: auto_lap=true starts a new lap (no reset) when the event starts.
#[test]
fn behavior_auto_lap_starts_a_lap_on_event_start_when_not_resetting() {
    let mut ad = AthleteData::new(1, 42, 1, 0.0, 10.0);
    ad.bucket.ingest_power(10.0, 200.0);
    let initial_laps = ad.lap_slices.len();
    let sgs = sg_map(make_subgroup(7));
    let behavior = EventBehavior { auto_reset: false, auto_lap: true };

    apply_event_state(&mut ad, &make_state(10.0, 7, 0.0), 99, &sgs, behavior, 10.0, 0);

    assert_eq!(
        ad.lap_slices.len(),
        initial_laps + 1,
        "auto_lap should start a new lap slice",
    );
    let prev = &ad.lap_slices[ad.lap_slices.len() - 2];
    assert!(prev.end.is_some(), "previous lap slice should be closed");
}
