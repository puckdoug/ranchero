// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingPower, RollingAverageOptions, Sample};

#[test]
fn power_exposes_avg_active_elapsed_lasttime() {
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: Some(15.0),
        active: true,
        ignore_zeros: false,
    };
    let mut rp = RollingPower::new(None, opts);

    // Add a hand-driven sequence
    rp.add(0.0, Sample::Value(100.0), None);
    rp.add(1.0, Sample::Value(200.0), None);
    rp.add(2.0, Sample::Value(150.0), None);

    // Verify forwarders match the inner rolling
    assert_eq!(rp.elapsed(), 2.0, "elapsed matches rolling");
    assert_eq!(rp.active(), 2.0, "active matches rolling");

    let avg = rp.avg(None);
    assert!(avg.is_some(), "avg returns Some");
    // avg = (200*1 + 150*1) / 2.0 = 175.0
    assert!((avg.unwrap() - 175.0).abs() < 1e-9, "avg value correct (weighted by gaps)");

    let last = rp.last_time();
    assert_eq!(last, Some(2.0), "last_time returns correct timestamp");

    assert!(!rp.full(0), "full(0) returns false (no period set)");
}

#[test]
fn power_full_matches_inner_full() {
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: None,
        active: true,
        ignore_zeros: false,
    };
    let mut rp = RollingPower::new(Some(10.0), opts);

    // Add 10 samples (times 0..9), elapsed=9
    for i in 0..10 {
        rp.add(i as f64, Sample::Value(100.0), None);
    }
    assert!(!rp.full(0), "not full when elapsed < period");

    // Add 11th sample (time 10), elapsed=10
    rp.add(10.0, Sample::Value(100.0), None);
    assert!(rp.full(0), "full when elapsed >= period");
}
