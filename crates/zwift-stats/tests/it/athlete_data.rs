// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::AthleteData;

// 14.15: AthleteData initialization
#[test]
fn new_initialises_identity_and_timestamps() {
    let athlete_id = 123u32;
    let course_id = 6u32;
    let sport = 0u8;
    let world_time = 1_700_000_000.0;
    let now = 100.0;

    let ad = AthleteData::new(athlete_id, course_id, sport, world_time, now);

    // Verify identity fields stored verbatim
    assert_eq!(ad.athlete_id, athlete_id);
    assert_eq!(ad.course_id, course_id);
    assert_eq!(ad.sport, sport);

    // Verify timestamp fields initialized to now
    assert_eq!(ad.created, now);
    assert_eq!(ad.updated, now);
    assert_eq!(ad.internal_created, now);
    assert_eq!(ad.internal_updated, now);
    assert_eq!(ad.internal_accessed, now);

    // Verify offset fields
    assert_eq!(ad.wt_offset, world_time);
    assert_eq!(ad.distance_offset, 0.0);

    // Verify bucket start matches now
    assert_eq!(ad.bucket.start(), now);
}

// 14.16: AthleteData touch method
#[test]
fn touch_updates_internal_accessed() {
    let now = 100.0;
    let mut ad = AthleteData::new(123, 6, 0, 1_700_000_000.0, now);

    // Record original created timestamps
    let created = ad.created;
    let internal_created = ad.internal_created;
    let internal_updated = ad.internal_updated;

    // Touch at now + 5.0
    let touch_time = now + 5.0;
    ad.touch(touch_time);

    // Verify internal_accessed updated
    assert_eq!(ad.internal_accessed, touch_time, "internal_accessed should be updated");

    // Verify other timestamps unchanged
    assert_eq!(ad.created, created, "created should not change");
    assert_eq!(ad.internal_created, internal_created, "internal_created should not change");
    assert_eq!(ad.internal_updated, internal_updated, "internal_updated should not change");
}

#[test]
fn record_update_advances_updated_and_accessed() {
    let world_time = 1_700_000_000.0;
    let now = 100.0;
    let mut ad = AthleteData::new(123, 6, 0, world_time, now);

    let created = ad.created;
    let internal_created = ad.internal_created;

    let new_world_time = world_time + 5.0;
    let new_now = now + 5.0;
    ad.record_update(new_world_time, new_now);

    assert_eq!(
        ad.updated, new_world_time,
        "record_update advances `updated` to the new world_time"
    );
    assert_eq!(
        ad.internal_updated, new_now,
        "record_update advances `internal_updated` to the new now"
    );
    assert_eq!(
        ad.internal_accessed, new_now,
        "record_update advances `internal_accessed` to the new now"
    );

    assert_eq!(ad.created, created, "`created` is unchanged");
    assert_eq!(
        ad.internal_created, internal_created,
        "`internal_created` is unchanged"
    );
}

// 14.17: AthleteData ingest routing
#[test]
fn ingest_routes_through_bucket() {
    let now = 0.0;
    let mut ad = AthleteData::new(123, 6, 0, 1_700_000_000.0, now);

    // Test power routing (add 2 samples to trigger flush with ideal_gap=1.0)
    ad.ingest_power(now, 1.0, 250.0);
    ad.ingest_power(now, 2.0, 250.0);
    assert!(
        ad.bucket.power().max_value() > 0.0,
        "power ingest should route to bucket.power"
    );

    // Test that other collectors remain empty
    assert_eq!(ad.bucket.hr().max_value(), 0.0, "hr should be empty");
    assert_eq!(ad.bucket.speed().max_value(), 0.0, "speed should be empty");
    assert_eq!(ad.bucket.cadence().max_value(), 0.0, "cadence should be empty");
    assert_eq!(ad.bucket.draft().max_value(), 0.0, "draft should be empty");

    // Test HR routing
    let mut ad = AthleteData::new(123, 6, 0, 1_700_000_000.0, now);
    ad.ingest_hr(now, 1.0, 150.0);
    ad.ingest_hr(now, 2.0, 150.0);
    assert!(
        ad.bucket.hr().max_value() > 0.0,
        "hr ingest should route to bucket.hr"
    );
    assert_eq!(ad.bucket.power().max_value(), 0.0, "power should be empty");

    // Test speed routing
    let mut ad = AthleteData::new(123, 6, 0, 1_700_000_000.0, now);
    ad.ingest_speed(now, 1.0, 35.0);
    ad.ingest_speed(now, 2.0, 35.0);
    assert!(
        ad.bucket.speed().max_value() > 0.0,
        "speed ingest should route to bucket.speed"
    );
    assert_eq!(ad.bucket.power().max_value(), 0.0, "power should be empty");

    // Test cadence routing
    let mut ad = AthleteData::new(123, 6, 0, 1_700_000_000.0, now);
    ad.ingest_cadence(now, 1.0, 90.0);
    ad.ingest_cadence(now, 2.0, 90.0);
    assert!(
        ad.bucket.cadence().max_value() > 0.0,
        "cadence ingest should route to bucket.cadence"
    );
    assert_eq!(ad.bucket.power().max_value(), 0.0, "power should be empty");

    // Test draft routing
    let mut ad = AthleteData::new(123, 6, 0, 1_700_000_000.0, now);
    ad.ingest_draft(now, 1.0, 0.5);
    ad.ingest_draft(now, 2.0, 0.5);
    assert!(
        ad.bucket.draft().max_value() > 0.0,
        "draft ingest should route to bucket.draft"
    );
    assert_eq!(ad.bucket.power().max_value(), 0.0, "power should be empty");
}

#[test]
fn ingest_bumps_internal_accessed() {
    let now = 100.0;
    let mut ad = AthleteData::new(123, 6, 0, 1_700_000_000.0, now);
    let initial_accessed = ad.internal_accessed;

    // Two samples a second apart so the buffer flushes on the second call.
    let later_now = now + 5.0;
    ad.ingest_power(now, 1.0, 200.0);
    ad.ingest_power(later_now, 2.0, 200.0);

    assert!(
        ad.internal_accessed > initial_accessed,
        "ingest must bump internal_accessed past its initial value"
    );
    assert_eq!(
        ad.internal_accessed, later_now,
        "internal_accessed reflects the latest call's `now`"
    );
    assert!(
        ad.bucket.power().max_value() > 0.0,
        "buffer flushed (sanity check that the test exercised the flush path)"
    );
}
