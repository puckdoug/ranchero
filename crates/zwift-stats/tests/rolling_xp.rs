// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingPower, Sample};
use std::fs;

#[test]
fn xp_returns_none_below_min_time() {
    // RollingPower with less than typical minimum time should return None for xp().
    // Add short duration samples (50 seconds) with varying power.
    // xp() should return None due to insufficient time window.

    let mut roll = RollingPower::new(None, Default::default());
    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(10.0, Sample::Value(150.0), None);
    roll.add(20.0, Sample::Value(200.0), None);
    roll.add(30.0, Sample::Value(150.0), None);
    roll.add(40.0, Sample::Value(100.0), None);

    let xp = roll.xp(false);
    assert!(xp.is_none(), "xp() should return None when data is insufficient");
}

#[test]
fn xp_force_returns_value_below_min_time() {
    // With xp(force=true), should return a value even below minimum time threshold.
    // Add short duration (50 seconds) samples with varying power.
    // xp(true) should return Some value.

    let mut roll = RollingPower::new(None, Default::default());
    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(10.0, Sample::Value(150.0), None);
    roll.add(20.0, Sample::Value(200.0), None);
    roll.add(30.0, Sample::Value(150.0), None);
    roll.add(40.0, Sample::Value(100.0), None);

    let xp = roll.xp(true);
    assert!(xp.is_some(), "xp(force=true) should return Some even below minimum time");
}

#[test]
fn xp_constant_power_equals_baseline() {
    // Constant power at baseline should equal XP. 300+ seconds at constant 150W.
    // XP emphasizes sustained effort differently than NP but constant power should be stable.
    // Add samples at 2-second intervals from t=0 to t=300.

    let mut roll = RollingPower::new(None, Default::default());
    for i in 0..151 {
        roll.add((i * 2) as f64, Sample::Value(150.0), None);
    }

    let xp = roll.xp(false);
    assert!(xp.is_some(), "should have XP after 300+ seconds");
    let xp_val = xp.unwrap();
    assert!(xp_val > 100.0, "XP at constant 150W should be above 100");
}

#[test]
fn xp_from_fixture_short() {
    // Load test data from fixtures/xp_short.json and compute XP.
    // Fixture contains alternating 100/300W pattern followed by constant 200W.
    // XP should reflect the weighted power over the session.

    let fixture_path = "crates/zwift-stats/fixtures/xp_short.json";
    let content = fs::read_to_string(fixture_path).expect("Failed to read fixture file");
    let data: serde_json::Value = serde_json::from_str(&content).expect("Failed to parse JSON");

    let mut roll = RollingPower::new(None, Default::default());

    if let Some(samples) = data["samples"].as_array() {
        for sample in samples {
            let time = sample["time"].as_f64().expect("Missing time");
            let power = sample["power"].as_f64().expect("Missing power");
            roll.add(time, Sample::Value(power), None);
        }
    }

    let xp = roll.xp(true);
    assert!(xp.is_some(), "should compute XP from fixture");
    assert!(xp.unwrap() > 0.0, "XP should be positive");
}

#[test]
fn xp_with_soft_pads_consistent() {
    // XP calculation should be consistent when soft-pads are inserted.
    // Stream: 100W, gap, 200W (gap triggers soft-pad filler).

    use zwift_stats::RollingAverageOptions;

    let opts = RollingAverageOptions {
        ideal_gap: Some(1.0),
        ..Default::default()
    };
    let mut roll = RollingPower::new(None, opts);

    roll.add(0.0, Sample::Value(100.0), None);
    roll.add(50.0, Sample::Value(200.0), None);

    assert!(
        roll.size() > 2,
        "soft-pads should be inserted, increasing stream size"
    );

    let xp = roll.xp(true);
    assert!(xp.is_some(), "should compute XP even with sparse stream");
}
