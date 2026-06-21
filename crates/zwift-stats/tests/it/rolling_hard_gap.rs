// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, Sample};

#[test]
fn hard_gap_inserts_zero_filler() {
    // max_gap = 5.0, ideal_gap = None (no soft-padding).
    // Add sample at t=0 (value 100), then t=10.0 (value 50).
    // Gap is 10.0 > max_gap (5.0), so hard-gap triggers.
    // Hard-gap uses ideal_gap derived from max_gap (or default to 1.0).
    // With a fallback ideal_gap of 1.0, hard-gap inserts pads at t=1,2,...,9.
    // Result: 11 samples (1 original + 9 pads + 1 new).

    let opts = RollingAverageOptions {
        ideal_gap: None,  // No soft-padding; only hard-gap.
        max_gap: Some(5.0),
        ..Default::default()
    };
    let mut roll = RollingAverage::new(None, opts);

    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(10.0, Sample::Value(50.0), None);

    // Gap 10.0 exceeds max_gap (5.0), hard-gap inserts zero pads.
    // Expected: 11 samples (original + 9 pads + new).
    assert_eq!(roll.size(), 11, "should have 11 samples after hard-gap zero-pad insertion");
}

#[test]
fn explicit_active_false_zero_pads() {
    // When caller explicitly passes active=Some(false), hard-gap padding is triggered
    // regardless of max_gap threshold.
    // Add sample at t=0 (value 100), then t=2 (value 200) with active=Some(false).
    // Gap is 2.0 < max_gap (5.0), so hard-gap wouldn't normally trigger.
    // But active=false forces zero-padding regardless.
    // With ideal_gap defaulting to 1.0 (from hard-gap fallback), one pad at t=1.
    // Result: 3 samples (1 original + 1 zero-pad + 1 new).

    let opts = RollingAverageOptions {
        ideal_gap: None,  // No soft-padding; only hard-gap / active=false.
        max_gap: Some(5.0),
        ..Default::default()
    };
    let mut roll = RollingAverage::new(None, opts);

    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(2.0, Sample::Value(200.0), Some(false));

    // active=false should force zero-padding even though gap (2.0) is below max_gap (5.0).
    // Expected: 3 samples (original at t=0, pad at t=1, new at t=2).
    assert_eq!(roll.size(), 3, "active=false should trigger zero-padding");
}
