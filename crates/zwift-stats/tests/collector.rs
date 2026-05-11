// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::collector::DataCollector;
use zwift_stats::{collector::DataCollectorOptions, RollingAverage, Sample};

// 14.5: DataCollector construction
#[test]
fn new_creates_primary_and_periodized_clones() {
    let periods = [60.0, 300.0];
    let opts = DataCollectorOptions::default();

    let collector = DataCollector::<RollingAverage>::new(&periods, opts);
    let periodized = collector.periodized();

    assert_eq!(collector.primary().size(), 0);
    assert_eq!(periodized.len(), 2, "two periodized entries");
    assert_eq!(periodized[0].period, 60.0, "first period is 60");
    assert_eq!(periodized[1].period, 300.0, "second period is 300");
}

#[test]
fn empty_periods_yields_primary_only() {
    let periods = [];
    let opts = DataCollectorOptions::default();
    let collector = DataCollector::<RollingAverage>::new(&periods, opts);
    assert!(collector.periodized().is_empty());
}

// 14.6: 1 s buffering
#[test]
fn add_buffers_until_ideal_gap_boundary() {
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&[], opts);
    assert_eq!(collector.add(0.0, 100.0), 0);
    assert_eq!(collector.add(0.5, 200.0), 0);
    assert_eq!(collector.primary().size(), 0);
    assert_eq!(collector.add(1.1, 50.0), 1);
    assert_eq!(collector.primary().size(), 1);
    assert_eq!(collector.primary().value_at(0), Some(Sample::Value(150.0)));
}

#[test]
fn flush_drains_partial_buffer() {
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&[], opts);
    collector.add(0.0, 100.0);
    collector.add(0.5, 200.0);
    assert_eq!(collector.primary().size(), 0);
    assert_eq!(collector.flush(), 1);
    assert_eq!(collector.primary().size(), 1);
    assert_eq!(collector.primary().value_at(0), Some(Sample::Value(150.0)));
    assert_eq!(collector.flush(), 0);
}

#[test]
fn round_option_rounds_flushed_mean() {
    let opts_no_round = DataCollectorOptions {
        ideal_gap: 1.0,
        round: false,
        ..Default::default()
    };
    let mut collector_no_round = DataCollector::<RollingAverage>::new(&[], opts_no_round);

    collector_no_round.add(0.0, 100.0);
    collector_no_round.add(0.5, 101.0);
    collector_no_round.flush();
    assert_eq!(
        collector_no_round.primary().value_at(0),
        Some(Sample::Value(100.5))
    );

    let opts_round = DataCollectorOptions {
        ideal_gap: 1.0,
        round: true,
        ..Default::default()
    };
    let mut collector_round = DataCollector::<RollingAverage>::new(&[], opts_round);
    collector_round.add(0.0, 100.0);
    collector_round.add(0.5, 101.0);
    collector_round.flush();
    assert_eq!(
        collector_round.primary().value_at(0),
        Some(Sample::Value(101.0))
    );
}

// 14.7: Max value tracking
#[test]
fn tracks_max_value_across_flushes() {
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&[], opts);
    collector.add(0.0, 100.0);
    collector.add(1.0, 250.0);
    collector.add(2.0, 200.0);
    collector.add(3.0, 180.0);
    assert_eq!(collector.max_value(), 250.0);
}

// 14.8: Periodized peak snapshots
#[test]
fn periodized_peak_snapshots_max_avg() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    for t in 0..62 {
        collector.add(t as f64, 250.0);
    }

    let peaks = collector.peaks();
    let peak = peaks[0].as_ref().unwrap();
    assert!((peak.snap_value - 250.0).abs() < 1e-6, "peak value should be 250");
}

#[test]
fn peak_does_not_update_until_window_is_full() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    for t in 0..31 {
        collector.add(t as f64, 250.0);
    }

    let peaks = collector.peaks();
    assert!(peaks[0].is_none());
}

#[test]
fn peak_uses_geq_comparison_not_strict_gt() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    for t in 0..62 {
        collector.add(t as f64, 100.0);
    }

    let first_peak_time = collector.peaks()[0].as_ref().unwrap().snap_time;

    for t in 62..123 {
        collector.add(t as f64, 100.0);
    }

    let second_peak_time = collector.peaks()[0].as_ref().unwrap().snap_time;
    assert!(second_peak_time > first_peak_time);
}

// 14.9: Collector clone
#[test]
fn clone_with_reset_creates_empty_snapshot() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    for t in 0..101 {
        collector.add(t as f64, 250.0);
    }

    assert!(collector.max_value() > 0.0);
    assert!(collector.peaks()[0].is_some());
    assert!(collector.primary().size() > 0);

    let reset_clone = collector.clone_reset();
    assert_eq!(reset_clone.max_value(), 0.0);
    assert!(reset_clone.peaks()[0].is_none());
    assert_eq!(reset_clone.primary().size(), 0);
}

#[test]
fn clone_without_reset_preserves_max_and_peaks() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    for t in 0..101 {
        collector.add(t as f64, 250.0);
    }

    let continue_clone = collector.clone_continue();
    assert_eq!(continue_clone.max_value(), collector.max_value());
    assert_eq!(
        continue_clone.peaks()[0].as_ref().unwrap().snap_value,
        collector.peaks()[0].as_ref().unwrap().snap_value
    );
    assert_eq!(continue_clone.primary().size(), collector.primary().size());

    // Check for deep copy
    collector.add(101.0, 300.0);
    collector.add(102.0, 300.0);
    collector.flush();

    assert_ne!(continue_clone.max_value(), collector.max_value());
}
