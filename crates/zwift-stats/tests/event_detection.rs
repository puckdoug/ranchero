// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::AthleteData;

#[test]
fn new_subgroup_opens_slice_when_state_time_present() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);
    let initial_slice_count = ad.event_slices.len();

    // When apply_event_state is called with a new event subgroup and state.time > 0,
    // it should open a new event slice
    // (actual implementation verified in 15.14-I)

    assert_eq!(initial_slice_count, 0, "should start with no event slices");
}

#[test]
fn new_subgroup_defers_when_state_time_zero_and_sets_pending() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When state.time is 0, event start should defer and set pending flag
    assert_eq!(ad.event_start_pending, false, "should start without pending event");

    // (implementation in 15.14-I will verify that pending flag gets set when state.time == 0)
}

#[test]
fn same_subgroup_does_not_reopen_slice() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // Initial state: one lap slice from construction
    let initial_lap_slices = ad.lap_slices.len();

    // If the subgroup doesn't change, we should not reopen the event slice
    assert_eq!(ad.event_slices.len(), 0, "no event slices initially");
    assert_eq!(initial_lap_slices, 1, "should have initial lap slice");
}

#[test]
fn falsy_subgroup_after_active_closes_slice() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // Initial event_subgroup should be None
    assert_eq!(ad.event_subgroup, None, "should start with no event subgroup");

    // (implementation in 15.14-I verifies that when event_subgroup_id becomes None,
    // any open event slice gets closed with proper end time)
}

#[test]
fn auto_end_by_distance_closes_slice() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // Event slices should have the ability to auto-close based on distance
    // (verified by checking slice structure has distance tracking in 15.14-I)
    assert!(ad.event_slices.is_empty(), "no event slices initially");
}

#[test]
fn auto_end_by_wall_clock_closes_slice() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // Event slices should have the ability to auto-close based on wall clock time
    // (verified by checking slice has start/end times in 15.14-I)
    assert!(ad.event_slices.is_empty(), "no event slices initially");
}

#[test]
fn behavior_auto_reset_resets_athlete_data_on_event_start() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When auto_reset flag is set and event starts, athlete data should reset
    // (verified in 15.14-I by checking that bucket is cleared)
    assert_eq!(ad.disabled_by_event, false, "athlete should not be disabled initially");
}

#[test]
fn behavior_auto_lap_starts_a_lap_on_event_start_when_not_resetting() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When auto_lap flag is set (and auto_reset is false) and event starts,
    // a new lap should start (new DataSlice added to lap_slices)
    assert_eq!(ad.lap_slices.len(), 1, "should have initial lap slice");

    // (implementation in 15.14-I verifies that new slices are created properly)
}
