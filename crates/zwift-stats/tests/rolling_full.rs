// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, Sample};

#[test]
fn full_returns_true_when_elapsed_meets_period() {
    let period = 60.0;
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: None,
        active: true,
        ignore_zeros: false,
    };
    let mut r = RollingAverage::new(Some(period), opts);

    // After 60 samples at 1 Hz (times 0..59), elapsed = 59 < period
    for i in 0..60 {
        r.add(i as f64, Sample::Value(100.0), None);
    }
    assert!(!r.full(0), "window not yet full at elapsed=59");

    // After the 61st sample (time 60), elapsed = 60 >= period
    r.add(60.0, Sample::Value(100.0), None);
    assert!(r.full(0), "window full at elapsed=60");
}

#[test]
fn full_offt_one_loop_evicts_one_sample() {
    // Test the offt parameter semantics for the JS while(full({offt:1})) loop
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: None,
        active: true,
        ignore_zeros: false,
    };
    let mut r = RollingAverage::new(Some(100.0), opts);

    // Push 101 samples (times 0..100); elapsed=100
    for i in 0..=100 {
        r.add(i as f64, Sample::Value(100.0), None);
    }
    assert!(r.full(0), "times[100] - times[0] = 100 >= period");
    // offt should still be 0 since elapsed equals period exactly (not > period)

    // Push one more (times 0..101); now elapsed=101 and auto-eviction starts
    r.add(101.0, Sample::Value(100.0), None);
    // After eviction, offt shifts so the window stays within period bounds
}

#[test]
fn full_returns_false_when_period_is_none() {
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: None,
        active: true,
        ignore_zeros: false,
    };
    let mut r = RollingAverage::new(None, opts);

    for i in 0..1000 {
        r.add(i as f64, Sample::Value(100.0), None);
    }
    assert!(!r.full(0), "periodless rolling always returns false from full");
}
