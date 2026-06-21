// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, Sample};

#[test]
fn period_eviction_keeps_window_bounded() {
    // Create a rolling window with period=100 (bounded to 100 seconds).
    // Add samples at t=0, 50, 100, 150, 200.
    // At each add, any samples older than (current_ts - period) should evict.
    // After t=200, samples at t=0 and t=50 should be evicted (outside 200-100=100 window).
    // Remaining: t=100, t=150, t=200 (3 samples in the live window).

    let opts = RollingAverageOptions {
        ideal_gap: None,
        max_gap: None,
        active: false,
        ignore_zeros: false,
    };
    let mut roll = RollingAverage::new(Some(100.0), opts);

    roll.add(0.0, Sample::Value(10.0), None);
    roll.add(50.0, Sample::Value(20.0), None);
    roll.add(100.0, Sample::Value(30.0), None);
    roll.add(150.0, Sample::Value(40.0), None);
    roll.add(200.0, Sample::Value(50.0), None);

    // After final add, window should contain only samples within (200-100)..200 = 100..200.
    // That is: t=100, t=150, t=200. Evicted: t=0, t=50.
    assert_eq!(
        roll.size(),
        3,
        "period eviction should keep only 3 samples in bounded window"
    );
}

#[test]
fn eviction_decrements_accumulators() {
    // Create a rolling window with period=100.
    // Add samples: t=0 (value 10), t=50 (value 20), t=150 (value 30).
    // After t=150, window is (150-100)..150 = 50..150.
    // Samples at t=0 (inactive gap) are evicted, only t=50 and t=150 remain.
    // Accumulators (active_acc, values_acc) should reflect only live samples.

    let opts = RollingAverageOptions {
        ideal_gap: None,
        max_gap: None,
        active: false,
        ignore_zeros: false,
    };
    let mut roll = RollingAverage::new(Some(100.0), opts);

    roll.add(0.0, Sample::Value(10.0), None);
    roll.add(50.0, Sample::Value(20.0), None);
    roll.add(150.0, Sample::Value(30.0), None);

    // After eviction, only t=50 and t=150 are live.
    // Gap from t=50 to t=150 is 100 seconds (active time for value 20.0).
    // Value 30.0 has gap to end = 0 (no time after it yet).
    // active_acc should be ~100, values_acc should be ~100*20 = 2000.
    let active = roll.active();
    assert!(active > 90.0 && active < 110.0, "active_acc should be ~100");
}

#[test]
fn full_with_offt_one_evicts_correctly() {
    // Create a rolling window with period=50.
    // Add samples: t=0, t=25, t=50, t=75, t=100.
    // After t=100, eviction window is (100-50)..100 = 50..100.
    // Expected evicted: t=0, t=25. Remaining: t=50, t=75, t=100.
    // With offt=1 (one sample evicted), size should be 3.

    let opts = RollingAverageOptions {
        ideal_gap: None,
        max_gap: None,
        active: false,
        ignore_zeros: false,
    };
    let mut roll = RollingAverage::new(Some(50.0), opts);

    roll.add(0.0, Sample::Value(1.0), None);
    roll.add(25.0, Sample::Value(2.0), None);
    roll.add(50.0, Sample::Value(3.0), None);
    roll.add(75.0, Sample::Value(4.0), None);
    roll.add(100.0, Sample::Value(5.0), None);

    assert_eq!(roll.size(), 3, "should have 3 live samples after eviction");
}
