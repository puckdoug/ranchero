// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{WBalAccumulator, Sample};

#[test]
fn recovery_below_cp_uses_exponential_term() {
    let mut acc = WBalAccumulator::new();
    acc.configure(200.0, 20000.0);

    // Drain wBal to approximately 10000
    // Depletion per second: (200 - 400) = -200 W
    // Need to deplete 10000 J: 10000 / 200 = 50 seconds
    for i in 0..50 {
        acc.accumulate(i as f64, Sample::Value(400.0));
    }

    let drained = acc.value().unwrap();
    assert!(drained < 10500.0 && drained > 9500.0, "wBal should be drained to ~10000, got {}", drained);

    // Recovery at 100 W (below CP of 200) for 1 second
    // Change = (200 - 100) * 1 * (20000 - drained) / 20000
    let change_expected = (200.0 - 100.0) * 1.0 * (20000.0 - drained) / 20000.0;
    let wbal_before = acc.value().unwrap();
    acc.accumulate(50.0, Sample::Value(100.0));
    let wbal_after = acc.value().unwrap();
    let actual_change = wbal_after - wbal_before;

    assert!((actual_change - change_expected).abs() < 1e-9, "Change should be {}, got {}", change_expected, actual_change);
}

#[test]
fn depletion_above_cp_uses_linear_term() {
    let mut acc = WBalAccumulator::new();
    acc.configure(200.0, 20000.0);

    acc.accumulate(0.0, Sample::Value(300.0));
    acc.accumulate(1.0, Sample::Value(300.0));

    // 300 W is above CP of 200, so linear branch: (200 - 300) * 1 = -100
    // wBal should be 20000 - 100 = 19900
    let wbal = acc.value().unwrap();
    assert!((wbal - 19900.0).abs() < 1e-6, "wBal should be ~19900, got {}", wbal);
}

#[test]
fn clamp_at_wprime_does_not_exceed() {
    let mut acc = WBalAccumulator::new();
    acc.configure(200.0, 20000.0);

    // Extended recovery from fresh state - should never exceed wPrime
    for t in 0..1000 {
        acc.accumulate(t as f64, Sample::Value(100.0));
        let wbal = acc.value().unwrap();
        assert!(wbal <= 20000.0, "wBal should not exceed wPrime, got {} at t={}", wbal, t);
    }
}

#[test]
fn wbal_can_go_negative_in_the_red() {
    let mut acc = WBalAccumulator::new();
    acc.configure(200.0, 20000.0);

    // Push 500 W for 100 seconds
    for t in 0..100 {
        acc.accumulate(t as f64, Sample::Value(500.0));
    }

    let wbal = acc.value().unwrap();
    assert!(wbal < 0.0, "wBal should go negative, got {}", wbal);
}
