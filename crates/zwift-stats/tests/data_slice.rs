// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{DataSlice, AthleteData};

#[test]
fn new_clones_bucket_reset_and_carries_identity() {
    let mut ad = AthleteData::new(1u32, 1u32, 1u8, 0.0, 0.0);

    let slice = DataSlice::new_from(&mut ad, 100.0);

    assert_eq!(slice.start, 100.0, "slice.start should be 100.0");
    assert_eq!(slice.end, None, "slice.end should be None initially");
    assert_eq!(slice.course_id, 1u32, "slice.course_id should match ad.course_id");
    assert_eq!(slice.bucket.power().max_value(), 0.0, "slice bucket should be reset");
}

#[test]
fn id_is_assigned_monotonically_per_athlete() {
    let mut ad = AthleteData::new(1u32, 1u32, 1u8, 0.0, 0.0);

    let slice1 = DataSlice::new_from(&mut ad, 0.0);
    let slice2 = DataSlice::new_from(&mut ad, 1.0);

    let id1_lower = slice1.id & 0xFFFFFFFF;
    let id2_lower = slice2.id & 0xFFFFFFFF;

    assert_eq!(id2_lower, id1_lower + 1, "Lower 32 bits should increment monotonically");
}

#[test]
fn id_carries_athlete_prefix_in_upper_32_bits() {
    let mut ad1 = AthleteData::new(1u32, 1u32, 1u8, 0.0, 0.0);
    let mut ad2 = AthleteData::new(2u32, 1u32, 1u8, 0.0, 0.0);

    let slice1 = DataSlice::new_from(&mut ad1, 0.0);
    let slice2 = DataSlice::new_from(&mut ad2, 0.0);

    let id1_upper = (slice1.id >> 32) & 0xFFFFFFFF;
    let id2_upper = (slice2.id >> 32) & 0xFFFFFFFF;

    assert_eq!(id1_upper, 1u64, "slice1 should have athlete_id 1 in upper bits");
    assert_eq!(id2_upper, 2u64, "slice2 should have athlete_id 2 in upper bits");

    // Lower 32 bits should be independent per athlete
    let id1_lower = slice1.id & 0xFFFFFFFF;
    let id2_lower = slice2.id & 0xFFFFFFFF;
    assert_eq!(id1_lower, id2_lower, "Lower bits should be the same counter value");
}

#[test]
fn end_starts_none_and_can_be_stamped_once() {
    let mut ad = AthleteData::new(1u32, 1u32, 1u8, 0.0, 0.0);

    let mut slice = DataSlice::new_from(&mut ad, 0.0);

    assert_eq!(slice.end, None, "slice.end should start as None");

    slice.close(100.0);
    assert_eq!(slice.end, Some(100.0), "slice.end should be set after close");

    // Calling close again should be a no-op
    slice.close(200.0);
    assert_eq!(slice.end, Some(100.0), "slice.end should not change on second close");
}

#[test]
fn slice_carries_course_and_sport_at_creation() {
    let mut ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 0.0);

    let slice = DataSlice::new_from(&mut ad, 0.0);

    // Mutate ad after slice creation
    ad.course_id = 99u32;

    // slice should still have the original value
    assert_eq!(slice.course_id, 42u32, "slice.course_id should not change when ad.course_id changes");
}
