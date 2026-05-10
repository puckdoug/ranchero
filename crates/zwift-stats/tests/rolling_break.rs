// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, Sample};

#[test]
fn break_gap_splits_with_book_ends() {
    // Catastrophic gap > 3600 s (Garmin glitch handling).
    // Sample at t=0, then t=7200. Gap (7200) > breakGap (3600) triggers split.
    // ideal_gap is None, so the hard-gap fallback (1.0) is used.
    //
    // bookEndTime = floor(3600/2) - 1 = 1799
    // Leading loop:  i = 1..1798   (while i < bookEndTime) -> 1798 pads
    // Break sentinel at t=1799 with pad = 7200 - (1799 * 2) = 3602
    // Trailing loop: i = 5401..7199 (while i < gap)        -> 1799 pads
    // Plus original (t=0) and new (t=7200)                 -> 2 samples
    // Total: 1 + 1798 + 1 + 1799 + 1 = 3600 samples.

    let opts = RollingAverageOptions {
        ideal_gap: None,
        max_gap: Some(5.0),  // Triggers hard-gap since gap (7200) > max_gap (5).
        ..Default::default()
    };
    let mut roll = RollingAverage::new(None, opts);

    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(7200.0, Sample::Value(50.0), None);

    assert_eq!(
        roll.size(),
        3600,
        "catastrophic gap should produce 3600 samples with Break sentinel"
    );
}
