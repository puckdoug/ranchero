// SPDX-License-Identifier: AGPL-3.0-only
//! 18.19-T — `apply_filter_group(value: Value, group: &FilterGroup) -> Value`
//!
//! Three unit tests:
//!
//! 1. `mask_drops_named_keys` — a `FilterGroup` whose `mask` lists `"lap"`
//!    causes that key to be absent from the returned value.
//!
//! 2. `stats_mask_nulls_per_slice_stats` — a `FilterGroup` whose `stats_mask`
//!    lists `"laps"` causes the `stats` field inside every element of the
//!    `laps` array to become null.  The slice elements themselves remain in the
//!    array.
//!
//! 3. `identity_when_mask_empty` — a `FilterGroup` with an empty `mask` and no
//!    `stats_mask` returns the value unchanged.
//!
//! Fails to compile until 18.19-I adds `apply_filter_group` to
//! `ranchero::web::subs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.19-T.

use serde_json::json;
use ranchero::web::subs::{AthleteV2Query, FilterGroup, QueryListener, apply_filter_group};

fn listener(stats: bool, resources: &[&str]) -> QueryListener {
    QueryListener {
        query: AthleteV2Query {
            stats,
            resources: resources.iter().map(|s| s.to_string()).collect(),
        },
    }
}

/// A mask that lists a key removes that key from the returned payload.
#[test]
fn mask_drops_named_keys() {
    let payload = json!({
        "version": 2,
        "stats": { "avg": 200.0 },
        "lap":   { "avg": 195.0 },
    });
    let group = FilterGroup {
        listeners:  vec![listener(false, &["stats"])],
        mask:       vec!["lap".to_string()],
        stats_mask: None,
    };
    let result = apply_filter_group(payload, &group);

    assert!(result.get("stats").is_some() && !result["stats"].is_null(),
        "stats must remain after masking; got {result}");
    assert!(result.get("lap").is_none() || result["lap"].is_null(),
        "lap must be absent after masking; got {result}");
}

/// `stats_mask` nulls the `stats` field inside every element of each named
/// slice resource.
#[test]
fn stats_mask_nulls_per_slice_stats() {
    let payload = json!({
        "version": 2,
        "laps": [
            { "id": 1, "stats": { "avg": 200.0 } },
            { "id": 2, "stats": { "avg": 205.0 } },
        ],
    });
    let group = FilterGroup {
        listeners:  vec![listener(false, &["laps"])],
        mask:       vec![],
        stats_mask: Some(vec!["laps".to_string()]),
    };
    let result = apply_filter_group(payload, &group);

    let laps = result["laps"].as_array()
        .expect("laps must remain as an array after stats masking");
    assert_eq!(laps.len(), 2, "both lap elements must still be present");
    for lap in laps {
        assert!(lap["stats"].is_null(),
            "each lap's stats field must be null when laps is in stats_mask; got {lap}");
    }
}

/// An empty mask with no `stats_mask` passes the value through unchanged.
#[test]
fn identity_when_mask_empty() {
    let payload = json!({
        "version": 2,
        "stats": { "avg": 200.0 },
        "lap":   { "avg": 195.0 },
    });
    let group = FilterGroup {
        listeners:  vec![listener(false, &["stats", "lap"])],
        mask:       vec![],
        stats_mask: None,
    };
    let result = apply_filter_group(payload, &group);

    assert_eq!(result["version"], 2,
        "version must be preserved by the identity group; got {result}");
    assert!(result["stats"].is_object(),
        "stats must be preserved by the identity group; got {result}");
    assert!(result["lap"].is_object(),
        "lap must be preserved by the identity group; got {result}");
}
