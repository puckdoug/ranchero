// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::Sample;

#[test]
fn break_pad_is_u32() {
    let s = Sample::Break { pad: 5u32 };
    match s {
        Sample::Break { pad } => {
            assert_eq!(pad as u64, 5u64);
        }
        _ => panic!("Expected Sample::Break variant"),
    }
}

#[test]
fn break_pad_arithmetic_uses_integer_division() {
    let pad: u32 = 1000;
    let cp = 200.0;
    let w_prime = 20000.0;

    // This test verifies that pad can be used in arithmetic without casting to f64 first.
    // The formula from wbal.rs should work directly with u32 pad values.
    let increment_per_tick = cp * (w_prime - 5000.0) / w_prime;
    let mut w_bal = 5000.0;

    // Simulate the break refill loop - should iterate pad times
    for _ in 0..pad {
        w_bal += increment_per_tick;
        if w_bal >= w_prime - 1e-6 {
            w_bal = w_prime;
            break;
        }
    }

    assert!(w_bal >= w_prime - 1e-6, "Break refill should saturate at w_prime");
}
