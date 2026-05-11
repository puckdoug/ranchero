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

    // After 59 samples at 1 Hz, elapsed < period
    for i in 0..59 {
        r.add(i as f64, Sample::Value(100.0), None);
    }
    assert!(!r.full(0), "window not yet full at 59 samples");

    // After the 60th sample, elapsed >= period
    r.add(59.0, Sample::Value(100.0), None);
    assert!(r.full(0), "window full at 60 samples (elapsed == 60)");
}

#[test]
fn full_offt_one_loop_evicts_one_sample() {
    let period = 60.0;
    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        max_gap: None,
        active: true,
        ignore_zeros: false,
    };
    let mut r = RollingAverage::new(Some(period), opts);

    // Push 60 samples; window is full
    for i in 0..60 {
        r.add(i as f64, Sample::Value(100.0), None);
    }
    assert!(r.full(0), "window full");

    // Push 61st sample; now full(1) should be true (JS `while (this.full({offt: 1}))` loop)
    r.add(60.0, Sample::Value(100.0), None);
    assert!(r.full(1), "full(1) true after 61st sample");

    // Pop once, now full(1) should be false
    r.pop();
    assert!(!r.full(1), "full(1) false after pop");
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
