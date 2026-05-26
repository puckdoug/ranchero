// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 15 — a scheduled task calls `SegmentsDb::evict_expired(now)` on a
// fixed interval. Mirrors the `gc_tick_loop` shape (see
// `tests/gc_tick_runs_on_interval.rs`): a tokio loop that ticks every
// `interval` and calls `evict_expired`. The first tick is skipped so the
// evictor does not run immediately at startup.
//
// `SegmentsDb::evict_expired` and the unit tests in
// `crates/zwift-store/tests/segments_evict*.rs` already cover the SQL side;
// this test wires the **scheduling** side and fails until the loop is
// added in src/daemon/stores.rs (or wherever the implementer chooses to
// expose it).
//
// Uses `tokio::time::pause()` + `advance()` so the test runs in
// microseconds rather than waiting `EVICT_TICK_INTERVAL_SECS` of real time.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ranchero::daemon::segments_evict_tick_loop;
use zwift_store::SegmentsDb;

/// After one full `interval`, `evict_expired` has run at least once and a
/// row whose `expires_at` has passed is gone. A second row whose
/// `expires_at` is still in the future is preserved.
#[tokio::test]
async fn segments_evict_tick_loop_runs_evict_on_interval() {
    tokio::time::pause();

    // Open a fresh DB in a per-test tempdir.
    let tmp  = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("segments.sqlite");
    let db   = Arc::new(SegmentsDb::open(&path).expect("open segments.sqlite"));

    // Insert one already-expired row and one still-valid row, anchored to a
    // synthetic `now` (the loop reads `SystemTime::now()`, which is unaffected
    // by `tokio::time::pause()`, so we use real wall-clock for the row
    // timestamps).
    let real_now = SystemTime::now();
    let payload  = b"opaque-payload".to_vec();
    db.put(101, &payload, Duration::from_secs(0),     real_now - Duration::from_secs(60))
        .expect("put expired row");
    db.put(202, &payload, Duration::from_secs(3_600), real_now)
        .expect("put valid row");

    // Sanity: the expired row is unreachable via `get` (its TTL is zero,
    // anchored 60 s ago), but it is still on disk until `evict_expired` runs.
    assert!(
        db.get(101, real_now).expect("get").is_none(),
        "expired row must be unreachable via get",
    );
    assert!(
        db.get(202, real_now).expect("get").is_some(),
        "valid row must be reachable via get",
    );

    // Spawn the loop.
    let interval = Duration::from_secs(60);
    tokio::spawn(segments_evict_tick_loop(Arc::clone(&db), interval));

    // Let the spawned task start and consume its skipped first tick.
    tokio::task::yield_now().await;

    // Advance past one full interval so the scheduled tick fires.
    tokio::time::advance(interval + Duration::from_millis(1)).await;

    // Let the task perform the eviction.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    // The expired row must be physically gone — a fresh `get` against a
    // *far-future* now would still report `None` for an evicted row, but a
    // re-insert at the same id with a long TTL succeeds and is visible,
    // which is the cleanest cross-check that the row was deleted (rather
    // than merely shadowed by the expiry filter).
    db.put(101, &payload, Duration::from_secs(3_600), real_now)
        .expect("re-insert after eviction must succeed");
    assert!(
        db.get(101, real_now).expect("get").is_some(),
        "after eviction + re-insert, the row is reachable again",
    );

    // The unrelated valid row was not touched.
    assert!(
        db.get(202, real_now).expect("get").is_some(),
        "valid row must survive the eviction tick",
    );
}
