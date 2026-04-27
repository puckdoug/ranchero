// SPDX-License-Identifier: AGPL-3.0-only
//
// `WorldTimer` math + clone-as-handle tests. Mirrors `class WorldTimer`
// at `zwift.mjs:89-123`. Pure timekeeping; no network, no async.

use std::time::{SystemTime, UNIX_EPOCH};

use zwift_relay::{WorldTimer, ZWIFT_EPOCH_MS};

fn unix_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch?")
        .as_millis() as i64
}

#[test]
fn world_timer_now_subtracts_epoch_at_zero_offset() {
    // At zero offset, `now()` should be `unix_now_ms() - ZWIFT_EPOCH_MS`,
    // within a small tolerance for the time elapsed between the two
    // `SystemTime::now()` reads (the one inside `WorldTimer::now()`
    // and our local `unix_now_ms()`).
    let timer = WorldTimer::new();
    let before = unix_now_ms() - ZWIFT_EPOCH_MS;
    let observed = timer.now();
    let after = unix_now_ms() - ZWIFT_EPOCH_MS;
    assert!(
        observed >= before && observed <= after + 50,
        "expected now() in [{before}, {after}+50ms], got {observed}",
    );
    assert_eq!(timer.offset_ms(), 0);
}

#[test]
fn world_timer_adjust_offset_shifts_now_by_diff() {
    let timer = WorldTimer::new();
    let before = timer.now();
    timer.adjust_offset(1_000);
    let after = timer.now();
    let diff = after - before;
    assert!(
        (995..=1_050).contains(&diff),
        "expected now() to advance by ~1000 ms after adjust_offset(+1000); diff = {diff}",
    );
    assert_eq!(timer.offset_ms(), 1_000);
}

#[test]
fn world_timer_clones_share_state() {
    // `WorldTimer` is a clone-as-handle: clones share one offset
    // because the inner state is `Arc<Mutex<…>>`. STEP 12 will pass
    // a single `WorldTimer` into multiple channels; they must all
    // see SNTP corrections from any one of them.
    let a = WorldTimer::new();
    let b = a.clone();
    a.adjust_offset(2_500);
    assert_eq!(b.offset_ms(), 2_500);
    b.adjust_offset(-500);
    assert_eq!(a.offset_ms(), 2_000);
}

#[test]
fn world_timer_offset_ms_is_cumulative() {
    let timer = WorldTimer::new();
    timer.adjust_offset(100);
    timer.adjust_offset(-30);
    timer.adjust_offset(7);
    assert_eq!(timer.offset_ms(), 77);
}

#[test]
fn world_timer_server_now_does_not_subtract_epoch() {
    // `server_now()` is `now()` + ZWIFT_EPOCH_MS: it lives in the
    // unix-epoch frame, useful for log timestamps reflecting the
    // corrected wall clock.
    let timer = WorldTimer::new();
    let world = timer.now();
    let server = timer.server_now();
    let diff = server - world;
    // Allow for a few-ms gap between the two reads in `now()` /
    // `server_now()`.
    assert!(
        (ZWIFT_EPOCH_MS..=ZWIFT_EPOCH_MS + 50).contains(&diff),
        "expected server_now() - now() ≈ ZWIFT_EPOCH_MS, got {diff}",
    );
}
