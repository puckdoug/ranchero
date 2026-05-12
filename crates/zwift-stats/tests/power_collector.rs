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

#[test]
fn np_peak_snapshot_records_lasttime() {
    let periods = [300.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = PowerDataCollector::new(&periods, opts);

    // Drive 350 samples so the 300 s period becomes full and an NP peak is
    // recorded. The latest flushed sample's timestamp must match the snapshot.
    for t in 0..351 {
        collector.add(t as f64, 250.0);
    }

    let np_peaks = collector.np_peaks();
    let np_peak = np_peaks[0].as_ref().expect("NP peak should be present");

    let last_time = collector.periodized()[0]
        .roll
        .last_time()
        .expect("last_time should be available");

    assert!(
        (np_peak.snap_time - last_time).abs() < 1e-9,
        "snap_time ({}) matches roll.last_time() ({})",
        np_peak.snap_time,
        last_time
    );
}

#[test]
fn np_peak_uses_geq_comparison() {
    let periods = [300.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = PowerDataCollector::new(&periods, opts);

    // First batch: produces the initial NP peak.
    for t in 0..351 {
        collector.add(t as f64, 250.0);
    }
    let first_snap_time = collector.np_peaks()[0]
        .as_ref()
        .expect("first NP peak should be present")
        .snap_time;

    // Second batch: identical power, so NP value is the same. With a strict
    // `>` comparison the snap_time would not advance; with `>=` it does.
    for t in 351..401 {
        collector.add(t as f64, 250.0);
    }
    let second_snap_time = collector.np_peaks()[0]
        .as_ref()
        .expect("second NP peak should be present")
        .snap_time;

    assert!(
        second_snap_time > first_snap_time,
        "NP peak snap_time advances on equal-value pushes (>= comparison): \
         first={}, second={}",
        first_snap_time,
        second_snap_time
    );
}

#[test]
fn np_peak_cleared_by_clone_reset() {
    let periods = [300.0, 1200.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = PowerDataCollector::new(&periods, opts);

    for t in 0..351 {
        collector.add(t as f64, 250.0);
    }
    assert!(
        collector.np_peaks()[0].is_some(),
        "original collector has NP peak"
    );

    let reset_clone = collector.clone_reset();
    let reset_peaks = reset_clone.np_peaks();
    assert!(reset_peaks[0].is_none(), "clone_reset clears 300 s NP peak");
    assert!(reset_peaks[1].is_none(), "clone_reset clears 1200 s NP peak");
}

// R2A-T3: NP peak snapshot self-describes (period) and carries the rolling.
#[test]
fn np_peak_snapshot_carries_period_and_roll() {
    let periods = [300.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = PowerDataCollector::new(&periods, opts);

    // Fill the 300 s period to produce an NP peak.
    for t in 0..351 {
        collector.add(t as f64, 250.0);
    }

    let np_peaks = collector.np_peaks();
    let np_peak = np_peaks[0].as_ref().expect("300 s NP peak should exist");

    assert_eq!(
        np_peak.period, 300.0,
        "snapshot.period self-describes which periodized window it came from"
    );

    let entry_roll = &collector.periodized()[0].roll;
    let entry_np = entry_roll
        .np(false)
        .expect("entry rolling has NP at peak time");
    let peak_np = np_peak
        .roll
        .np(false)
        .expect("snapshot rolling has NP at peak time");

    assert!(
        (peak_np - entry_np).abs() < 1e-9,
        "snapshot.roll.np ({}) matches entry.roll.np ({}) at peak time",
        peak_np,
        entry_np
    );
}

// R2A-T5: clone_continue carries the NP snapshot's roll forward as a deep copy.
#[test]
fn clone_continue_preserves_np_peak_rolls() {
    let periods = [300.0];
    let opts = DataCollectorOptions {
        ideal_gap: 1.0,
        ..Default::default()
    };
    let mut collector = PowerDataCollector::new(&periods, opts);

    for t in 0..351 {
        collector.add(t as f64, 250.0);
    }

    let cloned = collector.clone_continue();
    let cloned_np = cloned.np_peaks()[0]
        .as_ref()
        .expect("clone has NP peak")
        .roll
        .np(false);

    // Drive the source further; cloned snapshot's roll must not change.
    for t in 351..500 {
        collector.add(t as f64, 250.0);
    }

    assert_eq!(
        cloned.np_peaks()[0].as_ref().unwrap().roll.np(false),
        cloned_np,
        "clone_continue NP snapshot's roll.np unaffected by source updates"
    );
}
