// SPDX-License-Identifier: AGPL-3.0-only
//! 20.S5 — `DataCollector::grow_period` adds a new period entry and
//! `DataSlice::new_from` propagates it through `clone_reset`.
//!
//! **Red state**: `DataCollector` has no `grow_period` method; both tests fail
//! to compile.
//!
//! **Green state** (Step 5 implementation):
//! - `DataCollector::grow_period(period)` appends a new `PeriodizedEntry` to
//!   `self.periodized`.
//! - `DataCollector::clone_reset` already extracts periods from
//!   `self.periodized`, so grown periods survive into slices automatically.

use zwift_stats::{AthleteData, DataCollector, DataCollectorOptions, DataSlice, RollingAverage};

// S5-1: `grow_period` must append a new periodized entry without disturbing
// existing entries or accumulated data.
//
// Fails to compile in red state: `DataCollector` has no `grow_period` method.
#[test]
fn data_collector_resize_grows_window() {
    let mut dc = DataCollector::<RollingAverage>::new(&[60.0], DataCollectorOptions::default());
    dc.add(1.0, 200.0);
    dc.grow_period(300.0);
    let periods: Vec<f64> = dc.periodized().iter().map(|e| e.period).collect();
    assert!(periods.contains(&300.0), "grow_period must add the 300s period");
    assert!(periods.contains(&60.0), "existing 60s period must be preserved");
}

// S5-2: A period grown on an `AthleteData` bucket collector must survive
// into a `DataSlice` created by `DataSlice::new_from`.
//
// `new_from` calls `ad.bucket.clone_reset()`, which rebuilds each
// `DataCollector` from its `periodized` vec — so the grown period is
// propagated automatically once `grow_period` exists.
//
// Fails to compile in red state: `grow_period` is absent on `DataCollector`.
#[test]
fn data_slice_grows_after_new_from() {
    let mut ad = AthleteData::new(1, 1, 1, 0.0, 0.0);
    // HR collector normally has periods [60, 300, 1200, 3600]; add 5.0 s.
    ad.bucket.hr_mut().grow_period(5.0);
    let slice = DataSlice::new_from(&mut ad, 1.0);
    let periods: Vec<f64> = slice.bucket.hr().periodized().iter().map(|e| e.period).collect();
    assert!(
        periods.contains(&5.0),
        "grown period must survive DataSlice::new_from via clone_reset; got {:?}",
        periods,
    );
}
