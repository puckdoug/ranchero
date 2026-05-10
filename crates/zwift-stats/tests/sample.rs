// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{Sample, is_active_value};

#[test]
fn sample_is_active_value() {
    // Non-zero values are active regardless of ignore_zeros.
    assert!(is_active_value(Sample::Value(5.0), false));
    assert!(is_active_value(Sample::Value(5.0), true));

    // Zero values: active when ignore_zeros = false, inactive when true.
    // (Mirrors JS `_isActiveValue`: zero passes through unless ignoreZeros is set.)
    assert!(is_active_value(Sample::Value(0.0), false), "Value(0) should be active with ignore_zeros=false");
    assert!(!is_active_value(Sample::Value(0.0), true), "Value(0) should be inactive with ignore_zeros=true");

    // Pads are never active, regardless of value or flag.
    assert!(!is_active_value(Sample::Pad(0.0), false));
    assert!(!is_active_value(Sample::Pad(5.0), false));
    assert!(!is_active_value(Sample::Pad(0.0), true));
    assert!(!is_active_value(Sample::Pad(5.0), true));

    // Break sentinels are never active.
    assert!(!is_active_value(Sample::Break { pad: 1000.0 }, false));
    assert!(!is_active_value(Sample::Break { pad: 1000.0 }, true));
}
