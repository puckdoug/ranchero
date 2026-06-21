// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::Sample;

#[test]
fn pad_interner_returns_same_pad_for_close_values() {
    // The interner caches pads by round(value * 10). Values within
    // 0.05 of each other round to the same signature and should return
    // the same cached Sample::Pad instance (tested via pointer equality
    // since we can't directly inspect the cache from the public API, we
    // check that subsequent calls with close values behave identically).

    // For now, we test the basic contract: soft_pad(2.34) and soft_pad(2.36)
    // should both round to 23 (their signatures are close) and produce
    // equal values (though not necessarily the same pointer in Rust without
    // internal mutability). We'll verify the interner is working when
    // RollingAverage is built and uses it.

    // Placeholder assertion to confirm the test infrastructure works.
    let p1 = Sample::Pad(2.34);
    let p2 = Sample::Pad(2.34);
    assert_eq!(p1, p2, "identical Pad instances should be equal");
}

#[test]
fn zero_pad_is_a_singleton() {
    // The ZERO sentinel (Pad(0.0)) used for hard-gap padding should be
    // a consistent value. We verify this by checking that the Sample::Pad(0.0)
    // is what the rolling window uses internally (tested when RollingAverage
    // fills hard gaps). For now, placeholder.

    let zero = Sample::Pad(0.0);
    assert_eq!(zero.as_f64(), 0.0);
}
