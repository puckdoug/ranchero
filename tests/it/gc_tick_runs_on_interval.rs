// SPDX-License-Identifier: AGPL-3.0-only
//! 17.32-T — `gc_tick_loop` runs `registry.gc(now)` once per
//! `GC_TICK_INTERVAL_SECS` and emits a `tracing::debug!` event carrying
//! `athletes_dropped` and `groups_dropped` fields that match the `GcReport`
//! struct.
//!
//! The test uses `tokio::time::pause()` and `tokio::time::advance()` so it
//! completes in microseconds rather than waiting ~63 real seconds.
//!
//! Fails to compile until `gc_tick_loop` is added to `src/web/state.rs`
//! (item 17.32-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.32-T.

use std::sync::Arc;
use std::time::Duration;

use ranchero::web::WebState;
use ranchero::web::gc_tick_loop;
use zwift_stats::GC_TICK_INTERVAL_SECS;

#[tokio::test]
#[tracing_test::traced_test]
async fn gc_tick_loop_fires_and_emits_debug_event() {
    tokio::time::pause();

    let state = Arc::new(WebState::new());
    let interval = Duration::from_secs_f64(GC_TICK_INTERVAL_SECS);

    tokio::spawn(gc_tick_loop(Arc::clone(&state), interval));

    // Let the spawned task start and consume the immediate first tick, leaving
    // it blocked on the second tick (which fires after one full interval).
    tokio::task::yield_now().await;

    // Advance past one full interval so the loop's tick fires.
    tokio::time::advance(interval + Duration::from_millis(1)).await;

    // Let the task run the GC and emit the debug event.
    tokio::task::yield_now().await;

    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "athletes_dropped"),
        "gc_tick_loop must emit a debug! event carrying the athletes_dropped field",
    );
    assert!(
        tracing_test::internal::logs_with_scope_contain("ranchero", "groups_dropped"),
        "gc_tick_loop must emit a debug! event carrying the groups_dropped field",
    );
}
