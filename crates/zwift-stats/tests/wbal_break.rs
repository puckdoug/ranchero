// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{WBalAccumulator, Sample};

#[test]
fn break_sample_refills_until_full() {
    let mut acc = WBalAccumulator::new();
    acc.configure(200.0, 20000.0);

    // Drain wBal to 5000
    for _ in 0..1000 {
        acc.accumulate(0.0, Sample::Value(400.0));
    }

    let drained = acc.value().unwrap();
    assert!(drained < 5500.0 && drained > 4500.0, "wBal should be ~5000, got {}", drained);

    // Push a Break sample with pad=1000
    // The geometric series should saturate near 20000 well before 1000 iterations
    acc.accumulate(1.0, Sample::Break { pad: 1000u32 });

    let wbal_after_break = acc.value().unwrap();
    assert!((wbal_after_break - 20000.0).abs() < 1e-6, "wBal should refill to ~20000, got {}", wbal_after_break);
}

#[test]
fn break_sample_short_circuits_when_within_epsilon() {
    let mut acc = WBalAccumulator::new();
    acc.configure(200.0, 20000.0);

    // Drive wBal close to wPrime
    for t in 0..100 {
        acc.accumulate(t as f64, Sample::Value(100.0));
    }

    // Fine-tune to get very close to wPrime (within epsilon)
    let mut wbal = acc.value().unwrap();
    while wbal < 19999.9999999 {
        acc.accumulate(wbal, Sample::Value(100.0));
        wbal = acc.value().unwrap();
    }

    // Push a Break sample with pad=10
    // The loop should early-exit on the epsilon check
    acc.accumulate(wbal + 1.0, Sample::Break { pad: 10u32 });

    let final_wbal = acc.value().unwrap();
    assert_eq!(final_wbal, 20000.0, "wBal should be exactly 20000.0 after break epsilon early-exit");
}
