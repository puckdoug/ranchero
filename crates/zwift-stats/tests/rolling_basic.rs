// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, Sample};
use approx::assert_abs_diff_eq;

#[test]
fn empty_rolling_has_zero_elapsed_and_no_avg() {
    let roll = RollingAverage::new(None, Default::default());
    assert_eq!(roll.elapsed(), 0.0, "empty rolling should have zero elapsed");
    assert_eq!(roll.active(), 0.0, "empty rolling should have zero active time");
    assert_eq!(roll.size(), 0, "empty rolling should have size 0");
    assert_eq!(roll.avg(None), None, "empty rolling should have no average");
}

#[test]
fn single_sample_has_zero_elapsed() {
    let mut roll = RollingAverage::new(None, Default::default());
    roll.add(0.0, Sample::Value(100.0), None);

    // With a single sample, elapsed time is 0 (gap to the sample itself is 0).
    assert_eq!(roll.elapsed(), 0.0, "single sample has zero elapsed time");
    // Active accumulator starts at the *gap* — there's no gap before the first sample.
    assert_eq!(roll.active(), 0.0, "single sample has zero active time");
    // Average divides by elapsed (or active if active=true); with zero denominator, None.
    assert_eq!(roll.avg(None), None, "single sample has no average (zero elapsed)");
    assert_eq!(roll.size(), 1, "single sample has size 1");
}

#[test]
fn two_samples_avg() {
    let mut roll = RollingAverage::new(None, Default::default());
    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(1.0, Sample::Value(200.0), None);

    // Elapsed is time between first and last sample: 1.0 - 0.0 = 1.0.
    assert_eq!(roll.elapsed(), 1.0, "two samples 1 s apart have elapsed=1");
    // Active accumulator: first sample contributes 0 (no gap before it),
    // second sample contributes its gap (1.0).
    assert_eq!(roll.active(), 1.0, "two samples have active=1 (gap of second)");
    // Average: (100*0 + 200*1) / 1 = 200.
    assert_abs_diff_eq!(roll.avg(None).unwrap(), 200.0, epsilon = 1e-9);
    // Average with active=true: same as above (both samples are active).
    assert_abs_diff_eq!(roll.avg(Some(true)).unwrap(), 200.0, epsilon = 1e-9);
}

#[test]
fn accumulators_are_o1() {
    // Push 1000 samples and verify O(1) average computation matches from-scratch sum.
    let mut roll = RollingAverage::new(None, Default::default());
    let mut manual_active_sum = 0.0;
    let mut manual_value_weighted_sum = 0.0;

    for i in 0..1000 {
        let t = i as f64;
        let value = (i % 100 + 1) as f64; // Cycle through 1..100.
        let gap = if i > 0 { 1.0 } else { 0.0 }; // Gap to this sample.

        roll.add(t, Sample::Value(value), None);

        if i > 0 {
            manual_active_sum += gap;
            manual_value_weighted_sum += value * gap;
        }
    }

    let computed_active = roll.active();
    let computed_avg = roll.avg(None).unwrap();
    let expected_avg = manual_value_weighted_sum / manual_active_sum;

    assert_abs_diff_eq!(computed_active, manual_active_sum, epsilon = 1e-9);
    assert_abs_diff_eq!(computed_avg, expected_avg, epsilon = 1e-9);
}
