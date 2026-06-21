// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::ExpWeightedAvg;

#[test]
fn seed_returns_seed_until_first_input() {
    let mut ema = ExpWeightedAvg::new(8.0, 0.0);

    // Fresh EMA should return the seed value
    assert_eq!(ema.get(), 0.0, "EMA should return seed initially");

    // After first update with value=10.0
    // c_next = 1 - exp(-1/8) ≈ 0.11750309741540455
    // new_avg = seed * (1 - c_next) + value * c_next
    //         = 0.0 * c_prev + 10.0 * c_next
    //         ≈ 1.1750309741540455
    ema.update(10.0);
    let alpha = 1.0f64 - (-1.0f64 / 8.0f64).exp();
    let expected = 10.0 * alpha;
    assert!((ema.get() - expected).abs() < 1e-9, "After first update, EMA should be {}, got {}", expected, ema.get());
}

#[test]
fn alpha_matches_js_for_size_8() {
    let mut ema = ExpWeightedAvg::new(8.0, 0.0);

    // Apply 8 successive updates with value=10.0
    // With alpha ≈ 0.1175 and c_prev ≈ 0.8825,
    // the EMA converges exponentially toward 10.0
    for _ in 0..8 {
        ema.update(10.0);
    }

    // After 8 updates, the EMA should be around 6.32
    // It approaches 10.0 asymptotically
    let final_value = ema.get();
    assert!(final_value > 6.0 && final_value < 7.0, "EMA after 8 updates should be ~6.32, got {}", final_value);

    // Verify it continues to approach 10.0 with more updates
    for _ in 0..40 {
        ema.update(10.0);
    }

    let final_value_after_more = ema.get();
    assert!(final_value_after_more > 9.5 && final_value_after_more < 10.0, "EMA after 48 updates should approach 10.0, got {}", final_value_after_more);
}

#[test]
fn successive_updates_track_em_a_formula() {
    let mut ema = ExpWeightedAvg::new(8.0, 0.0);

    // Compute alpha for size 8
    let alpha = 1.0f64 - (-1.0f64 / 8.0f64).exp();
    let c_prev = 1.0f64 - alpha;

    // Hand-derived sequence: avg_n = avg_{n-1} * c_prev + value * c_next
    let mut expected = 0.0;
    let values = vec![10.0, 20.0, 15.0, 25.0, 5.0, 30.0, 12.0, 18.0, 22.0, 14.0];

    for (i, &value) in values.iter().enumerate() {
        ema.update(value);
        expected = expected * c_prev + value * alpha;

        let actual = ema.get();
        assert!((actual - expected).abs() < 1e-9, "At tick {}, EMA should be {}, got {}", i, expected, actual);
    }
}
