// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, Sample};

#[test]
fn soft_pad_inserts_value_filler() {
    // ideal_gap = 1.0, pad threshold = 1.61803.
    // Add sample at t=0 (value 100), then t=3 (value 200).
    // Gap is 3.0, which exceeds threshold, so soft pads at t=1 and t=2 with Pad(200).
    // Result: 4 samples total (1 original + 2 pads + 1 new).

    let opts = zwift_stats::RollingAverageOptions {
        ideal_gap: Some(1.0),
        ..Default::default()
    };
    let mut roll = RollingAverage::new(None, opts);

    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(3.0, Sample::Value(200.0), None);

    // After adding the second sample with a large gap, soft pads should be inserted.
    assert_eq!(roll.size(), 4, "should have 4 samples after soft-pad insertion");
}

#[test]
fn pad_threshold_excludes_borderline() {
    // With ideal_gap = 1.0, threshold = 1.61803.
    // Gap of 1.6 should NOT pad (< 1.61803); gap of 1.7 should pad.

    let opts = zwift_stats::RollingAverageOptions {
        ideal_gap: Some(1.0),
        ..Default::default()
    };

    // Test gap 1.6 (should NOT trigger soft padding).
    let mut roll = RollingAverage::new(None, opts.clone());
    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(1.6, Sample::Value(200.0), None);
    assert_eq!(roll.size(), 2, "gap 1.6 should NOT trigger soft padding");

    // Test gap 1.7 (should trigger soft padding).
    let mut roll2 = RollingAverage::new(None, opts);
    roll2.add(0.0, Sample::Value(100.0), None);
    roll2.add(1.7, Sample::Value(200.0), None);
    assert_eq!(roll2.size(), 3, "gap 1.7 should trigger soft padding (insert 1 pad)");
}
