// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, Sample};

#[test]
fn break_gap_splits_with_book_ends() {
    // Catastrophic gap > 3600 s (Garmin glitch handling).
    // Add sample at t=0 (value 100), then t=7200 (value 50).
    // Gap is 7200 s, which exceeds 3600 s breakGap threshold.
    // The algorithm splits this into:
    // - Leading zero pads (up to 1800 - ideal_gap)
    // - A Break sentinel covering the middle
    // - Trailing zero pads (from 7200 - 1800 onward)
    // With ideal_gap=1 (derived from hard-gap since none is set):
    // - bookEndTime = floor(3600/2) - 1 = 1800 - 1 = 1799
    // - Leading pads: t=1 to t=1799 (1799 samples)
    // - Break at t=1799 with pad=(7200-(1799*2))=3602
    // - Trailing pads: t=5401 to t=7199 (1799 samples)
    // Total: 1 (orig) + 1799 (lead) + 1 (break) + 1799 (trail) + 1 (new) = 5601 samples.

    let opts = RollingAverageOptions {
        ideal_gap: None,  // Hard-gap will use default 1.0.
        max_gap: None,
        ..Default::default()
    };
    let mut roll = RollingAverage::new(None, opts);

    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(7200.0, Sample::Value(50.0), None);

    // After adding with catastrophic gap, Break sentinel should be inserted.
    // Expected: ~5601 samples (1 + 1799 leading pads + 1 break + 1799 trailing pads + 1 new).
    assert_eq!(roll.size(), 5601, "should have 5601 samples with Break gap fill");
}
