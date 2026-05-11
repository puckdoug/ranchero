// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::collector::{DataCollectorOptions, PowerDataCollector};

// 14.10: NP peak only for periods >= 300
#[test]
fn np_peak_only_for_periods_at_or_above_300() {
    let periods = [5.0, 15.0, 60.0, 300.0, 1200.0, 3600.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = PowerDataCollector::new(&periods, opts);

    // Add 3602 samples to ensure all periods including 3600 become full
    // (need one extra to flush the last buffer)
    for t in 0..3602 {
        collector.add(t as f64, 200.0);
    }

    let np_peaks = collector.np_peaks();

    // Periods [5, 15, 60] should not have NP peaks (not >= 300)
    assert!(np_peaks[0].is_none(), "5s period has no NP peak");
    assert!(np_peaks[1].is_none(), "15s period has no NP peak");
    assert!(np_peaks[2].is_none(), "60s period has no NP peak");

    // Periods [300, 1200, 3600] should have NP peaks (>= 300 and full)
    assert!(np_peaks[3].is_some(), "300s period has NP peak");
    assert!(np_peaks[4].is_some(), "1200s period has NP peak");
    assert!(np_peaks[5].is_some(), "3600s period has NP peak");

    // For constant power, NP should equal the power value
    if let Some(np_peak) = &np_peaks[3] {
        assert!((np_peak.snap_value - 200.0).abs() < 1e-6, "300s NP peak matches constant power");
    }
}

// 14.11: NP peak survives clone_continue
#[test]
fn np_peak_survives_clone_continue() {
    let periods = [300.0, 1200.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = PowerDataCollector::new(&periods, opts);

    for t in 0..601 {
        collector.add(t as f64, 250.0);
    }

    let np_peaks_original = collector.np_peaks();
    assert!(np_peaks_original[0].is_some(), "original has NP peak");

    let cloned = collector.clone_continue();
    let np_peaks_cloned = cloned.np_peaks();

    assert_eq!(
        np_peaks_cloned[0].as_ref().map(|p| p.snap_value),
        np_peaks_original[0].as_ref().map(|p| p.snap_value),
        "cloned collector has same NP peak value"
    );

    let reset_clone = collector.clone_reset();
    let np_peaks_reset = reset_clone.np_peaks();
    assert!(np_peaks_reset[0].is_none(), "reset clone has no NP peaks");
}
