// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RollingPower, Sample};

#[test]
fn np_after_eviction_matches_recompute() {
    // After period eviction removes old samples, NP should match a fresh recompute from remaining samples.
    // Create rolling with period=300 (3-minute window).
    // Add 400 seconds of samples at constant 150W.
    // Period eviction should keep only the last 300 seconds.
    // Fresh recompute of that window should yield same NP.

    use zwift_stats::RollingAverageOptions;

    let opts = RollingAverageOptions::default();
    let mut roll = RollingPower::new(Some(300.0), opts);

    // Add 400 seconds of 150W power at 10-second intervals.
    for i in 0..40 {
        roll.add((i * 10) as f64, Sample::Value(150.0), None);
    }

    let np_with_eviction = roll.np(false);

    // Create fresh rolling with same period and add only the last 300 seconds worth.
    let mut fresh = RollingPower::new(Some(300.0), opts);
    for i in 10..40 {
        fresh.add((i * 10) as f64, Sample::Value(150.0), None);
    }

    let np_fresh = fresh.np(false);

    assert!(
        np_with_eviction.is_some() && np_fresh.is_some(),
        "both should have NP > 300s"
    );
    assert!(
        (np_with_eviction.unwrap() - np_fresh.unwrap()).abs() < 2.0,
        "NP after eviction should match fresh recompute"
    );
}
