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
    // Parity test: linearly increasing power.
    // Power: 100, 110, 120, 130, 140 at 10-second intervals.
    // Each value's gap covers 10 seconds except the first (gap=0).
    // Weighted sum: 100*0 + 110*10 + 120*10 + 130*10 + 140*10 = 5000
    // Total time: 40 seconds
    // Avg: 5000 / 40 = 125

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
        (avg.unwrap() - 125.0).abs() < 1e-6,
        "linear ramp should have avg ~125"
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
    // Parity test: soft-padding should not corrupt the weighted average calculation.
    // Create a stream where soft-padding will be triggered and verify consistency.
    // Stream: constant 150W with no gaps should have avg ~150W even if calculated differently.

    let opts_with_pad = RollingAverageOptions {
        ideal_gap: Some(10.0),
        ..Default::default()
    };

    let mut roll = RollingAverage::new(None, opts_with_pad);
    // Add samples 5 seconds apart (below soft-pad threshold)
    for i in 0..10 {
        roll.add((i * 5) as f64, Sample::Value(150.0), None);
    }

    let avg = roll.avg(Some(false));
    assert!(avg.is_some());
    assert!(
        (avg.unwrap() - 150.0).abs() < 1e-3,
        "constant power should have avg ~150W regardless of padding"
    );
}
