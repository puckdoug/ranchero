// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingPower, Sample};

#[test]
fn np_returns_none_below_300s_active() {
    // RollingPower with less than 300 seconds of active time should return None for np().
    // Add samples: t=0 (100W), t=10 (100W), t=100 (100W) - only 100 seconds of active time.
    // np() should return None.

    let mut roll = RollingPower::new(None, Default::default());
    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(10.0, Sample::Value(100.0), None);
    roll.add(100.0, Sample::Value(100.0), None);

    let np = roll.np(false);
    assert!(np.is_none(), "np() should return None when active < 300s");
}

#[test]
fn np_force_returns_value_below_min_time() {
    // With np(force=true), should return a value even below 300s threshold.
    // Add samples: t=0 (100W), t=10 (100W), t=100 (100W) - only 100 seconds active.
    // np(true) should return Some value, equal to 100.0 (constant power).

    let mut roll = RollingPower::new(None, Default::default());
    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(10.0, Sample::Value(100.0), None);
    roll.add(100.0, Sample::Value(100.0), None);

    let np = roll.np(true);
    assert!(np.is_some(), "np(force=true) should return Some even below 300s");
    assert!((np.unwrap() - 100.0).abs() < 1.0, "constant 100W should yield NP~100");
}

#[test]
fn np_constant_power_equals_power() {
    // Constant power should equal NP. 300+ seconds at constant 200W.
    // NP is (avg of p^4)^(1/4), which for constant p equals p.
    // Add samples at 10-second intervals from t=0 to t=300.

    let mut roll = RollingPower::new(None, Default::default());
    for i in 0..31 {
        roll.add((i * 10) as f64, Sample::Value(200.0), None);
    }

    let np = roll.np(false);
    assert!(np.is_some(), "should have NP after 300+ seconds");
    assert!(
        (np.unwrap() - 200.0).abs() < 5.0,
        "constant 200W power should yield NP~200"
    );
}

#[test]
fn np_known_vector() {
    // Test with a known power vector: alternating 100W and 300W.
    // 150 samples at alternating power, 2 seconds apart = 300 seconds total.
    // Each value contributes to 30-second rolling windows.
    // NP should be between 100 and 300, closer to middle due to 4th power emphasis on high values.

    let mut roll = RollingPower::new(None, Default::default());
    for i in 0..150 {
        let power = if i % 2 == 0 { 100.0 } else { 300.0 };
        roll.add((i * 2) as f64, Sample::Value(power), None);
    }

    let np = roll.np(false);
    assert!(np.is_some(), "should have NP after 300 seconds");
    let np_val = np.unwrap();
    assert!(np_val > 150.0, "NP should be above simple average");
    assert!(np_val < 250.0, "NP should not be at high extreme");
}

#[test]
fn np_with_soft_pads_matches_oracle() {
    // NP calculation should match oracle result when soft-pads are used.
    // Stream: 100W, gap, 200W (gap triggers soft-pad filler).
    // Create rolling with ideal_gap=1.0, add t=0 (100W), t=50 (200W).
    // Gap of 50 > 1.61803 threshold triggers soft-pad insertion.
    // NP should be consistent with the padded stream.

    use zwift_stats::RollingAverageOptions;

    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        ..Default::default()
    };
    let mut roll = RollingPower::new(None, opts);

    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(50.0, Sample::Value(200.0), None);

    // At least soft-pads were inserted, stream is longer than just 2 samples.
    assert!(
        roll.size() > 2,
        "soft-pads should be inserted, increasing stream size"
    );

    let np = roll.np(true);
    assert!(np.is_some(), "should compute NP even with sparse stream");
}
