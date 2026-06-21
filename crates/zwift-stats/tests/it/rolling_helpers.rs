// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{recommended_time_gaps, corrected_rolling_average, peak_average};

#[test]
fn recommended_time_gaps_mode_and_max() {
    // Given a stream of time gaps, recommended_time_gaps should return
    // a tuple (mode, max) where mode is the most frequent gap and max is the largest.
    // Gaps: [1, 1, 1, 2, 2, 5] -> mode=1, max=5.

    let gaps = vec![1.0, 1.0, 1.0, 2.0, 2.0, 5.0];
    let (mode, max) = recommended_time_gaps(&gaps);

    assert_eq!(mode, 1.0, "mode should be the most frequent gap");
    assert_eq!(max, 5.0, "max should be the largest gap");
}

#[test]
fn corrected_rolling_average_returns_none_for_short_streams() {
    // corrected_rolling_average should return None if data is insufficient.
    // Pass a short value array (10 values) with a high threshold (300.0).
    // It should return None for streams too short to meet the threshold.

    let values = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];
    let threshold = 300.0;

    let result = corrected_rolling_average(&values, threshold);
    assert!(
        result.is_none(),
        "should return None for short streams below threshold"
    );
}

#[test]
fn peak_average_finds_max_window() {
    // peak_average should find the maximum average over a sliding window.
    // Times: [0, 10, 20, 50, 60, 100, 150, 160, 170]
    // Values: [10, 10, 10, 100, 100, 100, 100, 10, 10]
    // Window period: 120 seconds. Peak should be in the high (100) region.

    let times = vec![0.0, 10.0, 20.0, 50.0, 60.0, 100.0, 150.0, 160.0, 170.0];
    let values = vec![10.0, 10.0, 10.0, 100.0, 100.0, 100.0, 100.0, 10.0, 10.0];
    let period = 120.0;

    let peak = peak_average(period, &times, &values);
    assert!(peak.is_some(), "peak_average should return Some for valid data");
    assert!(peak.unwrap() > 80.0, "peak average should be high (>80)");
}
