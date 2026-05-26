// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::AthleteData;

#[test]
fn new_initialises_accumulators_and_streams() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // Accumulators should be initialized
    assert_eq!(ad.w_bal.value(), None, "w_bal should be unconfigured");
    assert_eq!(ad.time_in_power_zones.value().len(), 0, "time_in_power_zones should start empty");
    assert_eq!(ad.smooth_grade.get(), 0.0, "smooth_grade should be seeded at 0");

    // Streams and road history should be initialized
<<<<<<< Updated upstream
    assert!(ad.streams.distance.is_empty(), "streams should start empty");
=======
    assert!(ad.streams.distance_list().is_empty(), "streams should start empty");
>>>>>>> Stashed changes
    assert!(ad.road_history.is_empty(), "road_history should start empty");

    // Slice collections should be initialized (except lap_slices which has initial slice)
    assert!(ad.event_slices.is_empty(), "event_slices should start empty");
    assert!(ad.segment_slices.is_empty(), "segment_slices should start empty");
    assert!(ad.active_segments.is_empty(), "active_segments should start empty");

    // Optional fields should be None
    assert_eq!(ad.gap, None, "gap should start as None");
    assert_eq!(ad.gap_distance, None, "gap_distance should start as None");
    assert_eq!(ad.group_id, None, "group_id should start as None");
    assert_eq!(ad.event_subgroup, None, "event_subgroup should start as None");
    assert_eq!(ad.auto_lap_mark, None, "auto_lap_mark should start as None");

    // Boolean fields
    assert_eq!(ad.is_gap_est, false, "is_gap_est should start false");
    assert_eq!(ad.disabled_by_event, false, "disabled_by_event should start false");
    assert_eq!(ad.event_start_pending, false, "event_start_pending should start false");
}

#[test]
fn initial_lap_slice_has_clone_reset_bucket() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // Should have an initial lap slice
    assert_eq!(ad.lap_slices.len(), 1, "should have one initial lap slice");

    let lap = &ad.lap_slices[0];

    // Slice should have correct identity
    assert_eq!(lap.start, 10.0, "lap slice start should match now");
    assert_eq!(lap.end, None, "lap slice end should be None initially");
    assert_eq!(lap.course_id, 42u32, "lap slice should capture course_id");
    assert_eq!(lap.sport, 1u8, "lap slice should capture sport");

    // Bucket should be reset (no power data accumulated)
    assert_eq!(lap.bucket.power().max_value(), 0.0, "lap bucket should be reset");
}
