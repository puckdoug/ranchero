// SPDX-License-Identifier: AGPL-3.0-only
//! 18.3-T — `DataCollector::stats(ts_offset_ms)` returns a `SignalStats` POD
//! whose `peaks` carries one slot per periodized window (None if not yet
//! filled) and whose `smooth` carries entries for windows with period ≤ 1200 s.
//!
//! All tests fail to compile until 18.3-I adds `SignalStats`, `PeakStat`,
//! `SmoothStat`, and the `stats()` method.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.3-T.

use zwift_stats::collector::{DataCollector, DataCollectorOptions, SignalStats};
use zwift_stats::RollingAverage;

fn default_opts() -> DataCollectorOptions {
    DataCollectorOptions::default()
}

// Add `n` consecutive samples at ts=0..n-1 and flush.
fn fill(collector: &mut DataCollector<RollingAverage>, n: u32, watts: f64) {
    for i in 0..n {
        collector.add(i as f64, watts);
    }
    collector.flush();
}

#[test]
fn stats_peaks_has_one_slot_per_period() {
    let mut c = DataCollector::<RollingAverage>::new(&[60.0, 300.0], default_opts());
    fill(&mut c, 65, 200.0);
    let s: SignalStats = c.stats(0.0);
    assert_eq!(s.peaks.len(), 2, "one peak slot per configured period");
}

#[test]
fn stats_peak_fills_once_window_is_covered() {
    let mut c = DataCollector::<RollingAverage>::new(&[60.0, 300.0], default_opts());
    fill(&mut c, 65, 200.0);
    let s: SignalStats = c.stats(0.0);
    assert!(s.peaks[0].is_some(), "60 s window must be filled after 65 s of data");
    assert!(s.peaks[1].is_none(), "300 s window must not be filled after only 65 s");
}

#[test]
fn stats_peak_fields_are_plausible() {
    let mut c = DataCollector::<RollingAverage>::new(&[60.0], default_opts());
    fill(&mut c, 65, 200.0);
    let s: SignalStats = c.stats(0.0);
    let peak = s.peaks[0].as_ref().expect("60 s peak must be present");
    assert_eq!(peak.period, 60.0);
    assert!((peak.avg - 200.0).abs() < 1.0, "peak avg must be ~200 W; got {}", peak.avg);
    assert!(peak.ts >= 0.0, "ts must be non-negative");
}

#[test]
fn stats_max_reflects_highest_value() {
    let mut c = DataCollector::<RollingAverage>::new(&[], default_opts());
    c.add(0.0, 100.0);
    c.add(1.0, 350.0);
    c.add(2.0, 200.0);
    c.flush();
    let s: SignalStats = c.stats(0.0);
    assert_eq!(s.max, 350.0);
}

#[test]
fn stats_avg_is_some_after_data_ingested() {
    let mut c = DataCollector::<RollingAverage>::new(&[], default_opts());
    fill(&mut c, 5, 200.0);
    let s: SignalStats = c.stats(0.0);
    assert!(s.avg.is_some(), "avg must be Some after data is ingested");
    assert!((s.avg.unwrap() - 200.0).abs() < 1.0);
}

#[test]
fn stats_avg_is_none_for_empty_collector() {
    let c = DataCollector::<RollingAverage>::new(&[], default_opts());
    let s: SignalStats = c.stats(0.0);
    assert!(s.avg.is_none(), "avg must be None when no data has been ingested");
}

#[test]
fn stats_smooth_excludes_periods_above_1200s() {
    let mut c = DataCollector::<RollingAverage>::new(
        &[60.0, 300.0, 1200.0, 3600.0],
        default_opts(),
    );
    fill(&mut c, 65, 200.0);
    let s: SignalStats = c.stats(0.0);
    assert!(
        s.smooth.iter().all(|e| e.period <= 1200.0),
        "smooth must not contain any period > 1200 s",
    );
}
