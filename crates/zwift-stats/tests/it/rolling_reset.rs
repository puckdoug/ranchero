// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, RollingPower, Sample};

#[test]
fn reset_clears_state_keeps_options() {
    let period = Some(300.0);
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: Some(15.0),
        active: true,
        ignore_zeros: false,
    };
    let mut r = RollingAverage::new(period, opts);

    // Add some data
    r.add(0.0, Sample::Value(100.0), None);
    r.add(1.0, Sample::Value(200.0), None);

    assert_eq!(r.size(), 2);
    assert!(r.avg(None).is_some());
    assert!(r.active() > 0.0);

    // Reset
    r.reset();

    // State should be cleared
    assert_eq!(r.size(), 0);
    assert!(r.avg(None).is_none());
    assert_eq!(r.active(), 0.0);
    assert_eq!(r.elapsed(), 0.0);

    // But options should be preserved; new adds respect ideal_gap
    r.add(10.0, Sample::Value(300.0), None);
    r.add(11.0, Sample::Value(400.0), None);
    assert_eq!(r.size(), 2);
    assert!(r.avg(None).is_some());  // avg needs at least 2 samples (elapsed > 0)
}

#[test]
fn reset_on_rolling_power_clears_qnpa() {
    let period = Some(600.0);
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: Some(15.0),
        active: true,
        ignore_zeros: false,
    };
    let mut rp = RollingPower::new(period, opts);

    // Add 600 seconds of constant 200 W
    for i in 0..600 {
        rp.add(i as f64, Sample::Value(200.0), None);
    }

    // Should have an NP value
    assert!(rp.np(true).is_some());
    let np_before = rp.np(true).unwrap();
    assert!(np_before > 0.0);

    // Reset
    rp.reset();

    // Now NP should be None (no active time, no qnpa_*) even with force=true
    // (After reset, size == 0, so np(true) returns None due to no qnpa_count)
    let np_after = rp.np(true);
    assert!(np_after.is_none(), "after reset, np(true) should be None");
}
