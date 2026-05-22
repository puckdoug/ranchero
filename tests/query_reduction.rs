// SPDX-License-Identifier: AGPL-3.0-only
//! 18.16-T — query-reduction helpers: `compute_query_cost`,
//! `create_query_strategies`, `create_filter_groups`.
//!
//! Ports the cost model and merge logic from
//! `ADV2QueryReductionEmitter` (stats.mjs:750).
//!
//! Three tests:
//! 1. `compute_query_cost_pins_cost_table` — pins every entry in the JS
//!    cost table including the ×5 stats amplifier.
//! 2. `create_query_strategies_returns_correct_batches` — verifies the
//!    split (non-stats / stats) strategy and the combined strategy for a
//!    mixed listener set; a pure non-stats set yields one strategy only.
//! 3. `create_filter_groups_groups_by_mask_signature` — verifies that
//!    listeners with identical (mask, stats_mask) signatures share one
//!    filter group, and listeners with different signatures are separated.
//!
//! Fails to compile until 18.16-I adds `QueryListener`, `QueryBatch`,
//! `FilterGroup`, `compute_query_cost`, `create_query_strategies`, and
//! `create_filter_groups` to `ranchero::web::subs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.16-T.

use std::collections::HashSet;
use ranchero::web::subs::{
    AthleteV2Query, QueryListener, QueryBatch, FilterGroup,
    compute_query_cost, create_query_strategies, create_filter_groups,
};

fn query(stats: bool, resources: &[&str]) -> AthleteV2Query {
    AthleteV2Query {
        stats,
        resources: resources.iter().map(|s| s.to_string()).collect(),
    }
}

fn listener(stats: bool, resources: &[&str]) -> QueryListener {
    QueryListener { query: query(stats, resources) }
}

// ---------------------------------------------------------------------------
// compute_query_cost
// ---------------------------------------------------------------------------

/// Pins every entry in the JS cost table.
///
/// From `computeQueryCost` (stats.mjs:795):
///   statsF = stats ? 5 : 1
///   state: 0.5, timeInPowerZones: 0.4, athlete: 1.5
///   stats/lap/lastLap: 1 * statsF
///   laps: 2 * statsF, segments: 4 * statsF, events: 1.5 * statsF
///   unknown: 1.0
#[test]
fn compute_query_cost_pins_cost_table() {
    let tol = 0.001_f64;

    // Cheap scalar resources — not amplified by stats flag
    let diff = (compute_query_cost(&query(false, &["state"])) - 0.5).abs();
    assert!(diff < tol, "state cost must be 0.5, got {}", compute_query_cost(&query(false, &["state"])));

    let diff = (compute_query_cost(&query(false, &["timeInPowerZones"])) - 0.4).abs();
    assert!(diff < tol, "timeInPowerZones cost must be 0.4");

    let diff = (compute_query_cost(&query(false, &["athlete"])) - 1.5).abs();
    assert!(diff < tol, "athlete cost must be 1.5");

    // stats/lap/lastLap: cost = 1 * statsF
    let diff = (compute_query_cost(&query(false, &["stats"])) - 1.0).abs();
    assert!(diff < tol, "stats cost (no stats flag) must be 1.0");

    let diff = (compute_query_cost(&query(true, &["stats"])) - 5.0).abs();
    assert!(diff < tol, "stats cost (stats flag) must be 5.0");

    let diff = (compute_query_cost(&query(false, &["lap"])) - 1.0).abs();
    assert!(diff < tol, "lap cost (no stats flag) must be 1.0");

    let diff = (compute_query_cost(&query(true, &["lastLap"])) - 5.0).abs();
    assert!(diff < tol, "lastLap cost (stats flag) must be 5.0");

    // laps: 2 * statsF
    let diff = (compute_query_cost(&query(false, &["laps"])) - 2.0).abs();
    assert!(diff < tol, "laps cost (no stats flag) must be 2.0");

    let diff = (compute_query_cost(&query(true, &["laps"])) - 10.0).abs();
    assert!(diff < tol, "laps cost (stats flag) must be 10.0");

    // segments: 4 * statsF
    let diff = (compute_query_cost(&query(false, &["segments"])) - 4.0).abs();
    assert!(diff < tol, "segments cost (no stats flag) must be 4.0");

    let diff = (compute_query_cost(&query(true, &["segments"])) - 20.0).abs();
    assert!(diff < tol, "segments cost (stats flag) must be 20.0");

    // events: 1.5 * statsF
    let diff = (compute_query_cost(&query(false, &["events"])) - 1.5).abs();
    assert!(diff < tol, "events cost (no stats flag) must be 1.5");

    let diff = (compute_query_cost(&query(true, &["events"])) - 7.5).abs();
    assert!(diff < tol, "events cost (stats flag) must be 7.5");

    // Unknown resource defaults to 1.0
    let diff = (compute_query_cost(&query(false, &["no_such_resource"])) - 1.0).abs();
    assert!(diff < tol, "unknown resource cost must be 1.0");

    // Multiple resources sum their individual costs
    // state(0.5) + stats(1.0) + lap(1.0) = 2.5
    let diff = (compute_query_cost(&query(false, &["state", "stats", "lap"])) - 2.5).abs();
    assert!(diff < tol, "state+stats+lap costs must sum to 2.5");
}

// ---------------------------------------------------------------------------
// create_query_strategies
// ---------------------------------------------------------------------------

/// All non-stats listeners: one combined strategy, one batch, stats=false.
#[test]
fn create_query_strategies_all_non_stats_yields_one_strategy() {
    let listeners = vec![
        listener(false, &["stats"]),
        listener(false, &["lap"]),
    ];
    let strategies = create_query_strategies(&listeners);
    assert_eq!(strategies.len(), 1,
        "all non-stats listeners must yield exactly one (combined) strategy");

    let [ref combined] = strategies[..] else { panic!("expected one strategy") };
    assert_eq!(combined.len(), 1, "combined strategy must have one batch");

    assert!(!combined[0].query.stats,
        "combined batch must have stats=false when no listener has stats");
    let resources: HashSet<_> = combined[0].query.resources.iter().cloned().collect();
    assert!(resources.contains("stats"),   "combined batch must include 'stats' resource");
    assert!(resources.contains("lap"),     "combined batch must include 'lap' resource");
    assert_eq!(combined[0].listeners.len(), 2, "combined batch must include all listeners");
}

/// Mixed stats/non-stats listeners: two strategies (split + combined).
///
/// The split strategy separates the non-stats and stats listeners into
/// two independent batches so each can be formatted once at the right
/// fidelity.  The combined strategy covers all listeners with a single
/// stats=true batch, trading over-formatting for one fewer formatting call.
#[test]
fn create_query_strategies_mixed_stats_yields_split_and_combined() {
    let listeners = vec![
        listener(false, &["stats"]),       // index 0 — non-stats listener
        listener(true,  &["laps"]),        // index 1 — stats listener
    ];
    let strategies = create_query_strategies(&listeners);
    assert_eq!(strategies.len(), 2,
        "mixed stats/non-stats listeners must yield two strategies");

    // One strategy must have two batches (the split strategy).
    let split = strategies.iter().find(|s| s.len() == 2)
        .expect("one strategy must have two batches");
    let non_stats_batch = split.iter().find(|b| !b.query.stats)
        .expect("split strategy must contain a non-stats batch");
    let stats_batch = split.iter().find(|b| b.query.stats)
        .expect("split strategy must contain a stats batch");
    assert!(non_stats_batch.query.resources.contains(&"stats".to_string()),
        "non-stats batch must include the 'stats' resource");
    assert!(stats_batch.query.resources.contains(&"laps".to_string()),
        "stats batch must include the 'laps' resource");
    assert_eq!(non_stats_batch.listeners.len(), 1, "non-stats batch has one listener");
    assert_eq!(stats_batch.listeners.len(), 1,     "stats batch has one listener");

    // The other strategy must have one batch (the combined strategy).
    let combined = strategies.iter().find(|s| s.len() == 1)
        .expect("one strategy must have one batch");
    assert!(combined[0].query.stats,
        "combined batch must have stats=true when any listener requests stats");
    let res: HashSet<_> = combined[0].query.resources.iter().cloned().collect();
    assert!(res.contains("stats"), "combined batch must include 'stats'");
    assert!(res.contains("laps"),  "combined batch must include 'laps'");
    assert_eq!(combined[0].listeners.len(), 2, "combined batch must include all listeners");
}

// ---------------------------------------------------------------------------
// create_filter_groups
// ---------------------------------------------------------------------------

/// Listeners with different mask signatures are placed in distinct groups.
///
/// Scenario: batch has `laps` with stats=true.
/// - l0 (stats=false, resources=[laps]): mask=[], stats_mask=["laps"]
/// - l1 (stats=true,  resources=[laps]): mask=[], stats_mask=None
/// Distinct signatures → two groups.
#[test]
fn create_filter_groups_separates_by_stats_mask() {
    let batch = QueryBatch {
        query: query(true, &["laps"]),
        listeners: vec![
            listener(false, &["laps"]),   // wants laps without stats
            listener(true,  &["laps"]),   // wants laps with stats
        ],
    };
    let groups = create_filter_groups(&batch);
    assert_eq!(groups.len(), 2,
        "listeners with different stats masks must fall into distinct filter groups");

    // The listener that has stats=false must have a stats_mask containing "laps".
    let g_no_stats = groups.iter()
        .find(|g| g.listeners.iter().any(|l| !l.query.stats))
        .expect("must have a group for the stats=false listener");
    let mask = g_no_stats.stats_mask.as_deref()
        .expect("stats=false listener in a stats=true batch must have a stats_mask");
    assert!(mask.contains(&"laps".to_string()),
        "stats_mask must include 'laps' since the listener has laps but not stats");

    // The listener that has stats=true must have no stats_mask.
    let g_stats = groups.iter()
        .find(|g| g.listeners.iter().any(|l| l.query.stats))
        .expect("must have a group for the stats=true listener");
    assert!(g_stats.stats_mask.is_none(),
        "stats=true listener must have no stats_mask");
}

/// Listeners with identical mask signatures share one filter group.
///
/// Both l0 and l1 request "stats" but not "laps"; the batch includes both.
/// Their mask = ["laps"], stats_mask = None (batch.stats=false).
/// Same signature → one group with two listeners.
#[test]
fn create_filter_groups_merges_identical_mask_signatures() {
    let batch = QueryBatch {
        query: query(false, &["stats", "laps"]),
        listeners: vec![
            listener(false, &["stats"]),   // mask = ["laps"]
            listener(false, &["stats"]),   // mask = ["laps"] — identical
        ],
    };
    let groups = create_filter_groups(&batch);
    assert_eq!(groups.len(), 1,
        "listeners with identical masks must share one filter group");
    assert_eq!(groups[0].listeners.len(), 2,
        "both listeners must be in the shared group");
    assert!(groups[0].mask.contains(&"laps".to_string()),
        "mask must contain 'laps' since the listeners do not request it");
}

/// A listener whose query is a strict subset of the batch query carries the
/// difference resources in its mask so the filter step can strip them.
#[test]
fn create_filter_groups_mask_is_batch_minus_listener_resources() {
    let batch = QueryBatch {
        query: query(false, &["state", "stats", "lap"]),
        listeners: vec![
            listener(false, &["stats"]),   // mask = ["state", "lap"]
        ],
    };
    let groups = create_filter_groups(&batch);
    assert_eq!(groups.len(), 1);
    let mask: HashSet<_> = groups[0].mask.iter().cloned().collect();
    assert!(mask.contains("state"), "mask must include 'state'");
    assert!(mask.contains("lap"),   "mask must include 'lap'");
    assert!(!mask.contains("stats"), "mask must not include 'stats' (listener requests it)");
}
