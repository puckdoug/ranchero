// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::DataBucket;

// 14.12: DataBucket default construction
#[test]
fn default_construction_matches_js_signals() {
    let start = 1234567.0;
    let bucket = DataBucket::new(start);

    // Verify start is stored
    assert_eq!(bucket.start(), start);

    // Verify accumulators start at zero
    assert_eq!(bucket.coffee_time(), 0.0);
    assert_eq!(bucket.work_time(), 0.0);
    assert_eq!(bucket.follow_time(), 0.0);
    assert_eq!(bucket.solo_time(), 0.0);
    assert_eq!(bucket.work_kj(), 0.0);
    assert_eq!(bucket.follow_kj(), 0.0);
    assert_eq!(bucket.solo_kj(), 0.0);

    // Verify power collector: periods [5,15,60,300,1200,3600], ignore_zeros=false, round=true
    assert_eq!(bucket.power().periodized().len(), 6);
    assert_eq!(bucket.power().periodized()[0].period, 5.0);
    assert_eq!(bucket.power().periodized()[1].period, 15.0);
    assert_eq!(bucket.power().periodized()[2].period, 60.0);
    assert_eq!(bucket.power().periodized()[3].period, 300.0);
    assert_eq!(bucket.power().periodized()[4].period, 1200.0);
    assert_eq!(bucket.power().periodized()[5].period, 3600.0);

    // Verify hr collector: periods [60,300,1200,3600], ignore_zeros=true, round=true
    assert_eq!(bucket.hr().periodized().len(), 4);
    assert_eq!(bucket.hr().periodized()[0].period, 60.0);
    assert_eq!(bucket.hr().periodized()[1].period, 300.0);
    assert_eq!(bucket.hr().periodized()[2].period, 1200.0);
    assert_eq!(bucket.hr().periodized()[3].period, 3600.0);

    // Verify speed collector: periods [60,300,1200,3600], ignore_zeros=true, round=false
    assert_eq!(bucket.speed().periodized().len(), 4);
    assert_eq!(bucket.speed().periodized()[0].period, 60.0);
    assert_eq!(bucket.speed().periodized()[1].period, 300.0);
    assert_eq!(bucket.speed().periodized()[2].period, 1200.0);
    assert_eq!(bucket.speed().periodized()[3].period, 3600.0);

    // Verify cadence collector: periods [], ignore_zeros=true, round=true
    assert_eq!(bucket.cadence().periodized().len(), 0);

    // Verify draft collector: periods [60,300,1200,3600], ignore_zeros=false, round=true
    assert_eq!(bucket.draft().periodized().len(), 4);
    assert_eq!(bucket.draft().periodized()[0].period, 60.0);
    assert_eq!(bucket.draft().periodized()[1].period, 300.0);
    assert_eq!(bucket.draft().periodized()[2].period, 1200.0);
    assert_eq!(bucket.draft().periodized()[3].period, 3600.0);
}

// 14.13: Ingest routes to correct collector
#[test]
fn ingest_routes_to_correct_collector() {
    let mut bucket = DataBucket::new(0.0);

    // Add power only
    bucket.ingest_power(1.0, 250.0);
    bucket.ingest_power(2.0, 200.0);
    bucket.ingest_power(3.0, 300.0);

    // Power should have max value
    assert!(bucket.power().max_value() > 0.0, "power has data");

    // Other collectors should be empty
    assert_eq!(bucket.hr().max_value(), 0.0, "hr empty after power only");
    assert_eq!(bucket.speed().max_value(), 0.0, "speed empty after power only");
    assert_eq!(bucket.cadence().max_value(), 0.0, "cadence empty after power only");
    assert_eq!(bucket.draft().max_value(), 0.0, "draft empty after power only");
}

#[test]
fn ingest_hr_routes_only_to_hr() {
    let mut bucket = DataBucket::new(0.0);

    bucket.ingest_hr(1.0, 150.0);
    bucket.ingest_hr(2.0, 160.0);

    assert_eq!(bucket.power().max_value(), 0.0, "power empty after hr only");
    assert!(bucket.hr().max_value() > 0.0, "hr has data");
    assert_eq!(bucket.speed().max_value(), 0.0, "speed empty after hr only");
    assert_eq!(bucket.cadence().max_value(), 0.0, "cadence empty after hr only");
    assert_eq!(bucket.draft().max_value(), 0.0, "draft empty after hr only");
}

#[test]
fn ingest_speed_routes_only_to_speed() {
    let mut bucket = DataBucket::new(0.0);

    bucket.ingest_speed(1.0, 35.0);
    bucket.ingest_speed(2.0, 40.0);

    assert_eq!(bucket.power().max_value(), 0.0, "power empty after speed only");
    assert_eq!(bucket.hr().max_value(), 0.0, "hr empty after speed only");
    assert!(bucket.speed().max_value() > 0.0, "speed has data");
    assert_eq!(bucket.cadence().max_value(), 0.0, "cadence empty after speed only");
    assert_eq!(bucket.draft().max_value(), 0.0, "draft empty after speed only");
}

#[test]
fn ingest_cadence_routes_only_to_cadence() {
    let mut bucket = DataBucket::new(0.0);

    bucket.ingest_cadence(1.0, 90.0);
    bucket.ingest_cadence(2.0, 95.0);

    assert_eq!(bucket.power().max_value(), 0.0, "power empty after cadence only");
    assert_eq!(bucket.hr().max_value(), 0.0, "hr empty after cadence only");
    assert_eq!(bucket.speed().max_value(), 0.0, "speed empty after cadence only");
    assert!(bucket.cadence().max_value() > 0.0, "cadence has data");
    assert_eq!(bucket.draft().max_value(), 0.0, "draft empty after cadence only");
}

#[test]
fn ingest_draft_routes_only_to_draft() {
    let mut bucket = DataBucket::new(0.0);

    bucket.ingest_draft(1.0, 0.5);
    bucket.ingest_draft(2.0, 0.8);

    assert_eq!(bucket.power().max_value(), 0.0, "power empty after draft only");
    assert_eq!(bucket.hr().max_value(), 0.0, "hr empty after draft only");
    assert_eq!(bucket.speed().max_value(), 0.0, "speed empty after draft only");
    assert_eq!(bucket.cadence().max_value(), 0.0, "cadence empty after draft only");
    assert!(bucket.draft().max_value() > 0.0, "draft has data");
}

// 14.14: Clone behaviors
#[test]
fn clone_reset_creates_slice_template() {
    let mut bucket = DataBucket::new(0.0);

    // Add data to all collectors (need multiple samples to trigger flush with ideal_gap=1.0)
    bucket.ingest_power(1.0, 250.0);
    bucket.ingest_power(2.0, 250.0);
    bucket.ingest_hr(1.0, 150.0);
    bucket.ingest_hr(2.0, 150.0);

    // Set time accumulators
    bucket.set_work_time(100.0);
    bucket.set_work_kj(500.0);

    assert!(bucket.power().max_value() > 0.0);
    assert_eq!(bucket.work_time(), 100.0);

    // Clone reset
    let reset_bucket = bucket.clone_reset();

    // Accumulators should be zero
    assert_eq!(reset_bucket.work_time(), 0.0, "work_time reset");
    assert_eq!(reset_bucket.work_kj(), 0.0, "work_kj reset");
    assert_eq!(reset_bucket.coffee_time(), 0.0, "coffee_time reset");
    assert_eq!(reset_bucket.follow_time(), 0.0, "follow_time reset");
    assert_eq!(reset_bucket.solo_time(), 0.0, "solo_time reset");
    assert_eq!(reset_bucket.follow_kj(), 0.0, "follow_kj reset");
    assert_eq!(reset_bucket.solo_kj(), 0.0, "solo_kj reset");

    // Collectors should be empty
    assert_eq!(reset_bucket.power().max_value(), 0.0, "power reset");
    assert_eq!(reset_bucket.hr().max_value(), 0.0, "hr reset");

    // Original unchanged
    assert!(bucket.power().max_value() > 0.0);
    assert_eq!(bucket.work_time(), 100.0);
}

#[test]
fn clone_continue_preserves_session_totals() {
    let mut bucket = DataBucket::new(0.0);

    // Add data and set accumulators
    bucket.ingest_power(1.0, 250.0);
    bucket.ingest_power(2.0, 200.0);
    bucket.ingest_hr(1.0, 150.0);

    let original_power_max = bucket.power().max_value();
    let original_hr_max = bucket.hr().max_value();

    bucket.set_work_time(100.0);
    bucket.set_work_kj(500.0);
    bucket.set_coffee_time(30.0);

    // Clone continue
    let continue_bucket = bucket.clone_continue();

    // Accumulators should be preserved
    assert_eq!(continue_bucket.work_time(), 100.0, "work_time preserved");
    assert_eq!(continue_bucket.work_kj(), 500.0, "work_kj preserved");
    assert_eq!(continue_bucket.coffee_time(), 30.0, "coffee_time preserved");

    // Collector data should be preserved
    assert_eq!(
        continue_bucket.power().max_value(),
        original_power_max,
        "power max preserved"
    );
    assert_eq!(
        continue_bucket.hr().max_value(),
        original_hr_max,
        "hr max preserved"
    );

    // Verify deep copy by modifying original
    bucket.set_work_time(200.0);
    assert_eq!(continue_bucket.work_time(), 100.0, "cloned bucket independent");
}
