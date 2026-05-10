// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, Sample};

#[test]
fn clone_independent_writes() {
    // Clone a RollingAverage and verify mutations are independent.
    // Original: add t=0 (value 10), t=50 (value 20).
    // Clone and mutate: add t=100 (value 30).
    // Original should remain size 2, clone should be size 3.

    let opts = RollingAverageOptions::default();
    let mut roll = RollingAverage::new(None, opts);
    roll.add(0.0, Sample::Value(10.0), None);
    roll.add(50.0, Sample::Value(20.0), None);

    let mut cloned = roll.clone();
    cloned.add(100.0, Sample::Value(30.0), None);

    assert_eq!(roll.size(), 2, "original should have 2 samples");
    assert_eq!(cloned.size(), 3, "clone should have 3 samples");
}

#[test]
fn slice_returns_subwindow() {
    // Create rolling with samples at t=0, 10, 20, 30, 40.
    // Slice from offset 1 to 3 should return samples at indices 1 and 2 (t=10, t=20).
    // Sliced window should have size 2 and elapsed time of 10 seconds (from t=10 to t=20).

    let opts = RollingAverageOptions::default();
    let mut roll = RollingAverage::new(None, opts);
    roll.add(0.0, Sample::Value(10.0), None);
    roll.add(10.0, Sample::Value(20.0), None);
    roll.add(20.0, Sample::Value(30.0), None);
    roll.add(30.0, Sample::Value(40.0), None);
    roll.add(40.0, Sample::Value(50.0), None);

    let sliced = roll.slice(1, 3);
    assert_eq!(sliced.size(), 2, "slice(1, 3) should have 2 samples");
    assert_eq!(
        sliced.elapsed(),
        10.0,
        "elapsed time from t=10 to t=20 should be 10 seconds"
    );
}

#[test]
fn time_at_negative_index() {
    // Create rolling with samples at t=0, 10, 20, 30.
    // time_at(-1) should return the last sample's time (t=30).
    // time_at(-2) should return t=20.
    // time_at(0) should return t=0 (first live sample).

    let opts = RollingAverageOptions::default();
    let mut roll = RollingAverage::new(None, opts);
    roll.add(0.0, Sample::Value(1.0), None);
    roll.add(10.0, Sample::Value(2.0), None);
    roll.add(20.0, Sample::Value(3.0), None);
    roll.add(30.0, Sample::Value(4.0), None);

    assert_eq!(
        roll.time_at(-1),
        Some(30.0),
        "time_at(-1) should return last sample time"
    );
    assert_eq!(
        roll.time_at(-2),
        Some(20.0),
        "time_at(-2) should return second-to-last time"
    );
    assert_eq!(roll.time_at(0), Some(0.0), "time_at(0) should return first time");
    assert_eq!(roll.time_at(10), None, "time_at(10) out of bounds should return None");
}

#[test]
fn entries_yields_offset_to_length() {
    // Create rolling with samples at t=0, 10, 20, 30.
    // After eviction with period=15, window is (30-15)..30 = 15..30.
    // Samples t=0, 10 are outside window. Remaining: t=20, 30.
    // entries() should yield only t=20, 30 (the live samples after offt adjustment).
    // Count should match size().

    let opts = RollingAverageOptions::default();
    let mut roll = RollingAverage::new(Some(15.0), opts);
    roll.add(0.0, Sample::Value(1.0), None);
    roll.add(10.0, Sample::Value(2.0), None);
    roll.add(20.0, Sample::Value(3.0), None);
    roll.add(30.0, Sample::Value(4.0), None);

    let entry_count = roll.entries().count();
    assert_eq!(
        entry_count,
        roll.size(),
        "entries() count should match size() after eviction"
    );
    assert_eq!(
        entry_count, 2,
        "should have 2 live entries after period eviction (t=20, 30)"
    );
}
