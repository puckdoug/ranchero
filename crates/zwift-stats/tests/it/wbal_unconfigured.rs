// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{WBalAccumulator, Sample};

#[test]
fn unconfigured_yields_none() {
    let mut acc = WBalAccumulator::new();

    // Fresh accumulator should return None
    assert_eq!(acc.value(), None, "Unconfigured accumulator should return None");

    // Accumulating without configuration should still return None
    acc.accumulate(0.0, Sample::Value(200.0));
    assert_eq!(acc.value(), None, "Accumulating on unconfigured accumulator should return None");
}

#[test]
fn accumulator_clone_continue_carries_w_bal() {
    let mut acc = WBalAccumulator::new();
    acc.configure(200.0, 20000.0);

    // Drive wBal to approximately 5000
    // Depletion per second: (200 - 400) = -200 W
    // Need to deplete 15000 J: 15000 / 200 = 75 seconds
    for i in 0..75 {
        acc.accumulate(i as f64, Sample::Value(400.0));
    }

    let wbal_source = acc.value().unwrap();
    assert!(wbal_source < 5500.0 && wbal_source > 4500.0, "wBal should be ~5000, got {}", wbal_source);

    // Clone the accumulator
    let mut acc_clone = acc.clone_continue();

    // Clone should carry the same wBal state
    assert_eq!(acc_clone.value().unwrap(), wbal_source, "clone_continue should carry wBal state");

    // Subsequent writes should diverge
    acc.accumulate(75.0, Sample::Value(100.0)); // Recovery
    acc_clone.accumulate(75.0, Sample::Value(500.0)); // Depletion

    let wbal_source_after = acc.value().unwrap();
    let wbal_clone_after = acc_clone.value().unwrap();

    assert!(wbal_source_after > wbal_source, "Source should recover");
    assert!(wbal_clone_after < wbal_source, "Clone should deplete");
}

// C-7.2: cp and w_prime accessors
#[test]
fn cp_and_w_prime_accessors_return_none_when_unconfigured() {
    let acc = WBalAccumulator::new();
    assert_eq!(acc.cp(), None, "cp must be None before configure");
    assert_eq!(acc.w_prime(), None, "w_prime must be None before configure");
}

#[test]
fn cp_and_w_prime_accessors_return_configured_values() {
    let mut acc = WBalAccumulator::new();
    acc.configure(220.0, 18000.0);
    assert_eq!(acc.cp(), Some(220.0), "cp must return the configured value");
    assert_eq!(acc.w_prime(), Some(18000.0), "w_prime must return the configured value");
}
