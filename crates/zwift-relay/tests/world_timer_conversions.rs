// SPDX-License-Identifier: AGPL-3.0-only
//! 18.2-T — `WorldTimer::to_server_time` and `to_local_time` convert a
//! world-time value (ms since Zwift epoch) to a Unix-epoch timestamp (ms),
//! mirroring `zwift.mjs:104-114`.
//!
//! Both tests fail to compile until 18.2-I adds the two methods to WorldTimer.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.2-T.

use zwift_relay::{WorldTimer, ZWIFT_EPOCH_MS};

// to_server_time(wt) = wt + ZWIFT_EPOCH_MS
// Converts a world-time offset (ms since Zwift epoch) to a Unix timestamp.
// The offset stored on the timer does NOT affect server time.
#[test]
fn to_server_time_adds_zwift_epoch_to_world_time() {
    let timer = WorldTimer::new();
    let wt: i64 = 1_000_000;
    assert_eq!(timer.to_server_time(wt), wt + ZWIFT_EPOCH_MS);
}

#[test]
fn to_server_time_is_independent_of_offset() {
    let timer = WorldTimer::new();
    timer.adjust_offset(5_000);
    let wt: i64 = 1_000_000;
    // to_server_time should not change when the local clock is shifted
    assert_eq!(timer.to_server_time(wt), wt + ZWIFT_EPOCH_MS);
}

#[test]
fn to_server_time_handles_zero_world_time() {
    let timer = WorldTimer::new();
    assert_eq!(timer.to_server_time(0), ZWIFT_EPOCH_MS);
}

// to_local_time(wt) = wt + ZWIFT_EPOCH_MS - offset_ms
// The local clock correction is subtracted so a positive offset (local behind
// server) yields a smaller local timestamp for the same world event.
#[test]
fn to_local_time_equals_server_time_at_zero_offset() {
    let timer = WorldTimer::new();
    let wt: i64 = 1_000_000;
    assert_eq!(timer.to_local_time(wt), timer.to_server_time(wt));
}

#[test]
fn to_local_time_subtracts_positive_offset() {
    let timer = WorldTimer::new();
    timer.adjust_offset(5_000);
    let wt: i64 = 1_000_000;
    assert_eq!(timer.to_local_time(wt), wt + ZWIFT_EPOCH_MS - 5_000);
}

#[test]
fn to_local_time_adds_back_negative_offset() {
    let timer = WorldTimer::new();
    timer.adjust_offset(-3_000);
    let wt: i64 = 2_000_000;
    assert_eq!(timer.to_local_time(wt), wt + ZWIFT_EPOCH_MS + 3_000);
}
