// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::AthleteRegistry;

// 14.18: AthleteRegistry upsert/get/len
#[test]
fn insert_get_remove_round_trip() {
    let mut registry = AthleteRegistry::new();

    let now = 100.0;
    let athlete_id = 123u32;
    let course_id = 6u32;
    let sport = 0u8;
    let world_time = 1_700_000_000.0;

    // First upsert creates a new athlete
    let ad1 = registry.upsert(athlete_id, course_id, sport, world_time, now);
    assert_eq!(ad1.athlete_id, athlete_id);
    assert_eq!(registry.len(), 1, "registry should have one athlete");

    // Second upsert for same athlete_id returns the same record
    {
        let ad2 = registry.upsert(athlete_id, course_id, sport, world_time, now + 5.0);
        assert_eq!(ad2.athlete_id, athlete_id);
        assert_eq!(
            ad2.internal_accessed, now + 5.0,
            "second upsert should update internal_accessed"
        );
    }
    assert_eq!(registry.len(), 1, "registry should still have one athlete");

    // get() retrieves the athlete
    let ad_get = registry.get(athlete_id);
    assert!(ad_get.is_some(), "get should find the athlete");
    assert_eq!(ad_get.unwrap().athlete_id, athlete_id);

    // get_mut() allows mutation
    let ad_mut = registry.get_mut(athlete_id);
    assert!(ad_mut.is_some(), "get_mut should find the athlete");

    // Nonexistent athlete returns None
    assert!(
        registry.get(999).is_none(),
        "get should return None for nonexistent athlete"
    );
}

// 14.19: AthleteRegistry GC for athletes
#[test]
fn gc_evicts_athletes_past_ttl() {
    let mut registry = AthleteRegistry::new();

    let now = 100.0;

    // Insert two athletes
    let athlete_old = 111u32;
    let athlete_young = 222u32;

    registry.upsert(athlete_old, 6, 0, 1_700_000_000.0, now);
    registry.upsert(athlete_young, 6, 0, 1_700_000_000.0, now);

    // Manually advance their internal_accessed to past values
    // (in real usage, touch() or ingest methods would update this)
    if let Some(ad) = registry.get_mut(athlete_old) {
        ad.internal_accessed = now - 3601.0; // Past TTL
    }
    if let Some(ad) = registry.get_mut(athlete_young) {
        ad.internal_accessed = now - 3599.0; // Within TTL
    }

    assert_eq!(registry.len(), 2, "should have two athletes before gc");

    // Run garbage collection
    let gc_report = registry.gc(now);

    // Verify old athlete was evicted
    assert_eq!(registry.len(), 1, "should have one athlete after gc");
    assert!(
        registry.get(athlete_old).is_none(),
        "old athlete should be evicted"
    );
    assert!(
        registry.get(athlete_young).is_some(),
        "young athlete should survive"
    );
    assert_eq!(
        gc_report.athletes_dropped, 1,
        "gc_report should indicate one athlete dropped"
    );
}

#[test]
fn gc_no_op_when_nothing_expired() {
    let mut registry = AthleteRegistry::new();
    let now = 5000.0;

    // Three athletes and two groups, all well within their TTLs.
    registry.upsert(101, 6, 0, 1_700_000_000.0, now);
    registry.upsert(102, 6, 0, 1_700_000_000.0, now);
    registry.upsert(103, 6, 0, 1_700_000_000.0, now);
    registry.touch_group(201, now);
    registry.touch_group(202, now);

    let athletes_before = registry.len();
    let groups_before = registry.groups_len();

    let report = registry.gc(now);

    assert_eq!(report.athletes_dropped, 0, "no athletes evicted");
    assert_eq!(report.groups_dropped, 0, "no groups evicted");
    assert_eq!(registry.len(), athletes_before, "athlete count unchanged");
    assert_eq!(registry.groups_len(), groups_before, "group count unchanged");

    // A second call with the same `now` is also a no-op (idempotency).
    let report_again = registry.gc(now);
    assert_eq!(report_again.athletes_dropped, 0);
    assert_eq!(report_again.groups_dropped, 0);
    assert_eq!(registry.len(), athletes_before);
    assert_eq!(registry.groups_len(), groups_before);
}

// 14.20: AthleteRegistry GC for groups
#[test]
fn gc_evicts_groups_past_ttl() {
    let mut registry = AthleteRegistry::new();

    let now = 100.0;

    let group_old = 111u32;
    let group_young = 222u32;

    // Create two groups with different accessed timestamps
    // touch_group creates or updates the accessed timestamp
    registry.touch_group(group_old, now - 91.0); // Past TTL
    registry.touch_group(group_young, now - 89.0); // Within TTL

    assert_eq!(registry.groups_len(), 2, "should have two groups before gc");

    // Run garbage collection
    let gc_report = registry.gc(now);

    // Verify old group was evicted
    assert_eq!(registry.groups_len(), 1, "should have one group after gc");
    assert!(
        registry.group(group_old).is_none(),
        "old group should be evicted"
    );
    assert!(
        registry.group(group_young).is_some(),
        "young group should survive"
    );
    assert_eq!(
        gc_report.groups_dropped, 1,
        "gc_report should indicate one group dropped"
    );
}

#[test]
fn gc_evicts_athletes_and_groups_in_one_pass() {
    let mut registry = AthleteRegistry::new();
    let now = 10_000.0;

    // One athlete past the athlete TTL, one within it.
    registry.upsert(111, 6, 0, 1_700_000_000.0, now);
    registry.upsert(222, 6, 0, 1_700_000_000.0, now);
    if let Some(ad) = registry.get_mut(111) {
        ad.internal_accessed = now - 3601.0;
    }
    if let Some(ad) = registry.get_mut(222) {
        ad.internal_accessed = now - 100.0;
    }

    // One group past the group TTL, one within it.
    registry.touch_group(333, now - 91.0);
    registry.touch_group(444, now - 10.0);

    assert_eq!(registry.len(), 2);
    assert_eq!(registry.groups_len(), 2);

    let report = registry.gc(now);

    assert_eq!(
        report.athletes_dropped, 1,
        "one athlete evicted in the combined pass"
    );
    assert_eq!(
        report.groups_dropped, 1,
        "one group evicted in the combined pass"
    );

    assert_eq!(registry.len(), 1, "only the within-TTL athlete remains");
    assert_eq!(registry.groups_len(), 1, "only the within-TTL group remains");
    assert!(registry.get(111).is_none());
    assert!(registry.get(222).is_some());
    assert!(registry.group(333).is_none());
    assert!(registry.group(444).is_some());
}
