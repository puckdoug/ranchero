// SPDX-License-Identifier: AGPL-3.0-only
//! 18.1-T — `RollingAverage::joules` and `RollingPower::joules` return the
//! cumulative value·time accumulator (`values_acc`).
//!
//! Both tests fail to compile until 18.1-I adds `joules()` to `RollingAverage`
//! and delegates it on `RollingPower`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.1-T.

use zwift_stats::{RollingAverage, RollingAverageOptions, RollingPower, Sample};

fn unbounded_opts() -> RollingAverageOptions {
    RollingAverageOptions {
        ideal_gap:    None,
        max_gap:      None,
        active:       true,
        ignore_zeros: false,
    }
}

// process_add accumulates: values_acc += value * (times[i] - times[i-1]).
// At i=0 the gap is 0, so the first sample never contributes.
// Four 200 W samples at ts=0,1,2,3 → gaps 0+1+1+1=3 s → 200*3 = 600 J.
#[test]
fn rolling_average_joules_returns_values_acc() {
    let mut roll = RollingAverage::new(None, unbounded_opts());
    roll.add(0.0, Sample::Value(200.0), None);
    roll.add(1.0, Sample::Value(200.0), None);
    roll.add(2.0, Sample::Value(200.0), None);
    roll.add(3.0, Sample::Value(200.0), None);
    assert_eq!(roll.joules(), 600.0);
}

#[test]
fn rolling_average_joules_is_zero_for_empty_roll() {
    let roll = RollingAverage::new(None, unbounded_opts());
    assert_eq!(roll.joules(), 0.0);
}

#[test]
fn rolling_average_joules_is_zero_for_single_sample() {
    let mut roll = RollingAverage::new(None, unbounded_opts());
    roll.add(0.0, Sample::Value(500.0), None);
    assert_eq!(roll.joules(), 0.0, "a single sample has no preceding gap");
}

// RollingPower wraps RollingAverage; joules() must delegate to the inner roll.
// Three 300 W samples at ts=0,1,2 → gaps 0+1+1=2 s → 300*2 = 600 J.
#[test]
fn rolling_power_joules_delegates_to_inner_roll() {
    let mut roll = RollingPower::new(None, unbounded_opts());
    roll.add(0.0, Sample::Value(300.0), None);
    roll.add(1.0, Sample::Value(300.0), None);
    roll.add(2.0, Sample::Value(300.0), None);
    assert_eq!(roll.joules(), 600.0);
}

#[test]
fn rolling_power_joules_is_zero_for_empty_roll() {
    let roll = RollingPower::new(None, unbounded_opts());
    assert_eq!(roll.joules(), 0.0);
}
