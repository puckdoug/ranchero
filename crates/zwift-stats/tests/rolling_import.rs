// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, Sample};

#[test]
fn import_data_matches_serial_add() {
    // Create two rolling windows:
    // - One using add() for each sample
    // - One using import_data() with all samples at once
    // Both should produce identical accumulators and size.

    let data = vec![
        (0.0, Sample::Value(10.0)),
        (10.0, Sample::Value(20.0)),
        (20.0, Sample::Value(30.0)),
        (30.0, Sample::Value(40.0)),
        (40.0, Sample::Value(50.0)),
    ];

    let opts = RollingAverageOptions::default();

    // Serial add
    let mut serial = RollingAverage::new(None, opts);
    for (ts, val) in &data {
        serial.add(*ts, *val, None);
    }

    // Import all at once
    let mut imported = RollingAverage::new(None, opts);
    imported.import_data(&data);

    assert_eq!(serial.size(), imported.size(), "size should match");
    assert_eq!(serial.active(), imported.active(), "active() should match");
    assert!(
        (serial.avg(Some(false)).unwrap() - imported.avg(Some(false)).unwrap()).abs() < 0.01,
        "avg() should match"
    );
}

#[test]
fn import_reduce_finds_peak_window() {
    // Create a rolling with a known peak: constant high value (100) from t=50 to t=150,
    // surrounded by lower values (10) before and after.
    // import_reduce with a window should find the t=50..150 range as peak.

    let data = vec![
        (0.0, Sample::Value(10.0)),
        (10.0, Sample::Value(10.0)),
        (20.0, Sample::Value(10.0)),
        (50.0, Sample::Value(100.0)),
        (60.0, Sample::Value(100.0)),
        (100.0, Sample::Value(100.0)),
        (150.0, Sample::Value(100.0)),
        (160.0, Sample::Value(10.0)),
        (170.0, Sample::Value(10.0)),
    ];

    let opts = RollingAverageOptions::default();
    let mut roll = RollingAverage::new(Some(120.0), opts); // 2-minute window

    let (peak_avg, peak_ts_range) = roll.import_reduce(&data, None);

    assert!(peak_avg > 80.0, "peak average should be high (>80)");
    assert!(peak_ts_range.end - peak_ts_range.start >= 100.0, "peak range should be ~100+ seconds");
}
