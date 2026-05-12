// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::collector::DataCollector;
use zwift_stats::{
    collector::{DataCollectorOptions, PeakSnapshot, RollingWindow},
    RollingAverage, RollingPower, Sample,
};

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

#[test]
fn default_options_match_js_constants() {
    let opts = DataCollectorOptions::default();
    assert_eq!(opts.ideal_gap, 1.0, "ideal_gap default matches stats.mjs:99 defOptions");
    assert_eq!(opts.max_gap, 15.0, "max_gap default matches stats.mjs:99 defOptions");
    assert!(opts.active, "active default matches stats.mjs:99 defOptions");
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

#[test]
fn max_value_unaffected_by_pad_fills() {
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        max_gap: 15.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&[], opts);
    collector.add(0.0, 100.0);
    collector.add(1.0, 300.0);
    collector.add(2.0, 250.0);
    let max_before_gap = collector.max_value();
    assert_eq!(max_before_gap, 300.0);

    // Skip several seconds: 2.0 -> 10.0 is a gap well over ideal_gap but under
    // max_gap, so the rolling will insert soft pads carrying the previous
    // value. The collector's max_value must not move because it is gated on
    // the post-flush f64 the collector itself pushes.
    collector.add(10.0, 150.0);
    collector.add(11.0, 120.0);
    collector.flush();

    assert_eq!(
        collector.max_value(),
        300.0,
        "max_value reflects pushed values only, not pad fills"
    );
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

// R2A-T1: peak snapshot self-describes (period) and carries the rolling.
#[test]
fn peak_snapshot_carries_period_and_roll() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    // Fill the 60 s window and produce a peak.
    for t in 0..62 {
        collector.add(t as f64, 250.0);
    }

    let peaks = collector.peaks();
    let peak = peaks[0].as_ref().expect("60 s period should have a peak");

    assert_eq!(
        peak.period, 60.0,
        "snapshot.period self-describes which periodized window it came from"
    );

    let entry_roll = &collector.periodized()[0].roll;
    let entry_avg = entry_roll.avg(None).expect("entry rolling has an average");
    let peak_avg = peak.roll.avg(None).expect("snapshot rolling has an average");

    assert!(
        (peak_avg - entry_avg).abs() < 1e-9,
        "snapshot.roll.avg ({}) matches entry.roll.avg ({}) at peak time",
        peak_avg,
        entry_avg
    );
    assert_eq!(
        peak.roll.last_time(),
        entry_roll.last_time(),
        "snapshot.roll.last_time matches entry.roll.last_time at peak time"
    );
}

// R2A-T2: snapshot's roll is a deep clone, independent of the source.
#[test]
fn peak_snapshot_roll_is_independent_of_source() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    for t in 0..62 {
        collector.add(t as f64, 250.0);
    }

    // Capture an owned copy of the peak snapshot.
    let captured = collector.peaks()[0]
        .as_ref()
        .expect("peak should exist")
        .clone();
    let original_avg = captured.roll.avg(None);
    let original_last_time = captured.roll.last_time();
    assert!(original_avg.is_some(), "sanity: captured roll has data");
    assert!(original_last_time.is_some(), "sanity: captured roll has data");

    // Drive the source further. With a flat 250 W stream and the `>=`
    // comparison, the source's own peak keeps advancing. The captured copy
    // must not move with it.
    for t in 62..200 {
        collector.add(t as f64, 250.0);
    }

    assert_eq!(
        captured.roll.avg(None),
        original_avg,
        "captured snapshot's roll.avg is independent of subsequent source pushes"
    );
    assert_eq!(
        captured.roll.last_time(),
        original_last_time,
        "captured snapshot's roll.last_time is independent of subsequent source pushes"
    );
}

// R2A-T4: clone_continue carries the snapshot's roll forward as a deep copy.
#[test]
fn clone_continue_preserves_peak_rolls() {
    let periods = [60.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = DataCollector::<RollingAverage>::new(&periods, opts);

    for t in 0..62 {
        collector.add(t as f64, 250.0);
    }

    let cloned = collector.clone_continue();
    let cloned_avg = cloned.peaks()[0]
        .as_ref()
        .expect("clone has peak")
        .roll
        .avg(None);
    let cloned_last_time = cloned.peaks()[0].as_ref().unwrap().roll.last_time();

    // Drive the source further; cloned snapshot's roll must not change.
    for t in 62..200 {
        collector.add(t as f64, 250.0);
    }

    assert_eq!(
        cloned.peaks()[0].as_ref().unwrap().roll.avg(None),
        cloned_avg,
        "clone_continue snapshot's roll.avg unaffected by source updates"
    );
    assert_eq!(
        cloned.peaks()[0].as_ref().unwrap().roll.last_time(),
        cloned_last_time,
        "clone_continue snapshot's roll.last_time unaffected by source updates"
    );
}

// R2A-T6: peaks() return type is generic over R.
#[test]
fn peaks_method_returns_generic_snapshots() {
    let opts = DataCollectorOptions::default();

    let avg_collector = DataCollector::<RollingAverage>::new(&[60.0], opts);
    let _: Vec<Option<PeakSnapshot<RollingAverage>>> = avg_collector.peaks();

    let pow_collector = DataCollector::<RollingPower>::new(&[60.0], opts);
    let _: Vec<Option<PeakSnapshot<RollingPower>>> = pow_collector.peaks();
}
