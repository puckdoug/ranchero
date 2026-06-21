// SPDX-License-Identifier: AGPL-3.0-only
//! 18.4-T — `PowerDataCollector::np_stats(ts_offset_ms)` returns an `NpStats`
//! POD that covers only periods ≥ 300 s, matching `_npPeriodizedOfft`
//! (`stats.mjs:265`).
//!
//! All tests fail to compile until 18.4-I adds `NpStats`, `NpPeakStat`, and
//! the `np_stats()` method.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.4-T.

use zwift_stats::collector::{DataCollectorOptions, NpStats, PowerDataCollector};

fn default_opts() -> DataCollectorOptions {
    DataCollectorOptions::default()
}

// Power collector with the standard 6 periods from DataBucket.
fn standard_power() -> PowerDataCollector {
    PowerDataCollector::new(&[5.0, 15.0, 60.0, 300.0, 1200.0, 3600.0], default_opts())
}

#[test]
fn np_stats_covers_only_periods_at_or_above_300s() {
    let mut p = standard_power();
    for i in 0..5u32 {
        p.add(i as f64, 200.0);
    }
    p.flush();
    let s: NpStats = p.np_stats(0.0);
    // Periods 5, 15, 60 are all below 300 s and must be absent.
    // Periods 300, 1200, 3600 qualify → 3 slots.
    assert_eq!(s.peaks.len(), 3, "np_stats must have one slot per period ≥ 300 s");
}

#[test]
fn np_stats_all_none_when_no_window_is_filled() {
    let mut p = standard_power();
    for i in 0..5u32 {
        p.add(i as f64, 200.0);
    }
    p.flush();
    let s: NpStats = p.np_stats(0.0);
    assert!(
        s.peaks.iter().all(|e| e.is_none()),
        "no NP window is full after 5 s; all peaks must be None",
    );
}

#[test]
fn np_stats_300s_peak_fills_after_sufficient_data() {
    // 300 s window needs more than 300 data points (1 per second).
    let mut p = PowerDataCollector::new(&[5.0, 15.0, 60.0, 300.0], default_opts());
    for i in 0..=305u32 {
        p.add(i as f64, 200.0);
    }
    p.flush();
    let s: NpStats = p.np_stats(0.0);
    // Only one qualifying period (300 s) with this setup.
    assert_eq!(s.peaks.len(), 1);
    let peak = s.peaks[0].as_ref().expect("300 s NP peak must be filled after 305 s");
    assert_eq!(peak.period, 300.0);
    // NP of sustained 200 W is 200 W (constant power → NP = avg).
    assert!((peak.avg - 200.0).abs() < 5.0, "NP must be ~200 W; got {}", peak.avg);
}

#[test]
fn np_peak_stat_has_max_field() {
    // max is the highest NP seen across the session; it must be accessible.
    // This test verifies the field exists by reading it (not that it has a
    // specific value — that belongs in the formatter tests).
    let mut p = PowerDataCollector::new(&[300.0], default_opts());
    for i in 0..=305u32 {
        p.add(i as f64, 200.0);
    }
    p.flush();
    let s: NpStats = p.np_stats(0.0);
    let peak = s.peaks[0].as_ref().expect("300 s NP peak must be filled");
    // max must be ≥ avg (it is the session-high NP, not the window NP).
    assert!(peak.max >= peak.avg, "max NP must be at least as large as the window avg NP");
}
