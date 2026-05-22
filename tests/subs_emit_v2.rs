// SPDX-License-Identifier: AGPL-3.0-only
//! 18.20-T — `emit_v2` orchestration: the formatter is called once per batch
//! regardless of how many listeners share it; each listener receives only the
//! resources it requested.
//!
//! Two tests:
//!
//! 1. `calls_formatter_once_for_shared_batch` — two listeners with overlapping
//!    non-stats queries (`A = ["stats","lap"]`, `B = ["stats"]`) share one
//!    batch.  The counting double verifies one formatter invocation.  Listener A
//!    receives both resources; listener B has `lap` removed by its filter group.
//!
//! 2. `single_listener_identity` — a single listener produces one formatter call
//!    and the result passes through unchanged (empty mask, no stats_mask).
//!
//! `emit_v2` returns one `Value` per listener in the same order as the input
//! slice.
//!
//! Fails to compile until 18.20-I adds `emit_v2` to `ranchero::web::subs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.20-T.

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use serde_json::json;
use ranchero::web::subs::{AthleteV2Query, QueryListener, emit_v2};

fn listener(stats: bool, resources: &[&str]) -> QueryListener {
    QueryListener {
        query: AthleteV2Query {
            stats,
            resources: resources.iter().map(|s| s.to_string()).collect(),
        },
    }
}

/// Two listeners with overlapping non-stats queries share one batch; the
/// formatter is called exactly once.
///
/// Scenario:
///   A = { resources: ["stats", "lap"], stats: false } → mask = []      → both resources
///   B = { resources: ["stats"],        stats: false } → mask = ["lap"] → "lap" removed
///
/// Both have `stats: false` so `create_query_strategies` produces a single
/// combined batch, and the formatter is invoked once.
#[test]
fn calls_formatter_once_for_shared_batch() {
    let listeners = vec![
        listener(false, &["stats", "lap"]),  // A: wants stats and lap
        listener(false, &["stats"]),          // B: wants stats only
    ];

    let call_count = AtomicUsize::new(0);
    let results = emit_v2(&listeners, |_query| {
        call_count.fetch_add(1, Relaxed);
        // Return a payload containing all resources the batch might request.
        json!({
            "version": 2,
            "stats":   { "avg": 200.0 },
            "lap":     { "avg": 195.0 },
        })
    });

    // Both listeners share one batch → formatter must be called exactly once.
    assert_eq!(call_count.load(Relaxed), 1,
        "formatter must be called once when all listeners share a batch");

    // emit_v2 returns one Value per listener in input order.
    assert_eq!(results.len(), 2, "must return one result per listener");

    // Listener A (stats + lap) must receive both resources.
    assert!(results[0]["stats"].is_object(),
        "listener A must receive stats; got {}", results[0]);
    assert!(results[0]["lap"].is_object(),
        "listener A must receive lap; got {}", results[0]);

    // Listener B (stats only) must have lap removed by its mask.
    assert!(results[1]["stats"].is_object(),
        "listener B must receive stats; got {}", results[1]);
    assert!(results[1].get("lap").is_none() || results[1]["lap"].is_null(),
        "listener B must not receive lap (not in its query); got {}", results[1]);
}

/// A single listener produces one formatter call; the result is passed through
/// unchanged (empty mask, no stats_mask).
#[test]
fn single_listener_identity() {
    let listeners = vec![listener(false, &["stats"])];

    let call_count = AtomicUsize::new(0);
    let results = emit_v2(&listeners, |_query| {
        call_count.fetch_add(1, Relaxed);
        json!({ "version": 2, "stats": { "avg": 210.0 } })
    });

    assert_eq!(call_count.load(Relaxed), 1,
        "formatter must be called exactly once for a single listener");
    assert_eq!(results.len(), 1, "must return one result for one listener");
    assert_eq!(results[0]["version"], 2,
        "result must match formatter output; got {}", results[0]);
    assert!(results[0]["stats"].is_object(),
        "result must include stats; got {}", results[0]);
}
