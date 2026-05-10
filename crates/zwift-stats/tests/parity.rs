// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingAverage, RollingAverageOptions, RollingPower, Sample};

#[test]
fn parity_constant_power_avg() {
    // Parity test: constant power stream should have avg == power.
    // Create fixture data and verify RollingAverage::avg matches oracle.

    let samples = vec![
        (0.0, 150.0),
        (10.0, 150.0),
        (20.0, 150.0),
        (30.0, 150.0),
        (40.0, 150.0),
    ];

    let mut roll = RollingAverage::new(None, Default::default());
    for (ts, power) in &samples {
        roll.add(*ts, Sample::Value(*power), None);
    }

    let avg = roll.avg(Some(false));
    assert!(avg.is_some());
    assert!(
        (avg.unwrap() - 150.0).abs() < 1e-6,
        "constant power should have avg == power"
    );
}

#[test]
fn parity_linear_ramp_avg() {
    // Parity test: linearly increasing power should have avg == midpoint.
    // Power: 100, 110, 120, 130, 140 at 10-second intervals.
    // Avg should be ~120 (the middle value).

    let samples = vec![
        (0.0, 100.0),
        (10.0, 110.0),
        (20.0, 120.0),
        (30.0, 130.0),
        (40.0, 140.0),
    ];

    let mut roll = RollingAverage::new(None, Default::default());
    for (ts, power) in &samples {
        roll.add(*ts, Sample::Value(*power), None);
    }

    let avg = roll.avg(Some(false));
    assert!(avg.is_some());
    assert!(
        (avg.unwrap() - 120.0).abs() < 1e-6,
        "linear ramp should have avg ~120"
    );
}

#[test]
fn parity_np_constant_power() {
    // Parity test: RollingPower::np for constant power should equal the power.

    let mut roll = RollingPower::new(None, Default::default());
    for i in 0..31 {
        roll.add((i * 10) as f64, Sample::Value(175.0), None);
    }

    let np = roll.np(false);
    assert!(np.is_some());
    assert!(
        (np.unwrap() - 175.0).abs() < 1e-3,
        "NP for constant 175W should equal 175W"
    );
}

#[test]
fn parity_xp_constant_power() {
    // Parity test: RollingPower::xp for constant power should be stable.

    let mut roll = RollingPower::new(None, Default::default());
    for i in 0..31 {
        roll.add((i * 10) as f64, Sample::Value(175.0), None);
    }

    let xp = roll.xp(false);
    assert!(xp.is_some());
    let xp_val = xp.unwrap();
    assert!(xp_val > 100.0 && xp_val < 250.0, "XP should be reasonable for 175W");
}

#[test]
fn parity_soft_pad_consistency() {
    // Parity test: soft-padding should produce consistent results.
    // Two rolling windows: one with explicit values, one with gaps (soft-pad filled).
    // avg() should be close.

    let opts_with_pad = RollingAverageOptions {
        ideal_gap: Some(1.0),
        ..Default::default()
    };

    let mut roll_with_pad = RollingAverage::new(None, opts_with_pad);
    roll_with_pad.add(0.0, Sample::Value(100.0), None);
    roll_with_pad.add(50.0, Sample::Value(200.0), None);

    let avg_with_pad = roll_with_pad.avg(Some(false));

    // Create a more densely sampled version (no soft-padding needed)
    let mut roll_dense = RollingAverage::new(None, Default::default());
    for i in 0..=50 {
        let power = 100.0 + ((i as f64 / 50.0) * 100.0);
        roll_dense.add(i as f64, Sample::Value(power), None);
    }

    let avg_dense = roll_dense.avg(Some(false));

    assert!(avg_with_pad.is_some() && avg_dense.is_some());
    // Averages should be in the same ballpark (both around 150W)
    assert!(
        (avg_with_pad.unwrap() - avg_dense.unwrap()).abs() < 10.0,
        "soft-pad and dense samples should yield similar averages"
    );
}
