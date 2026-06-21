// SPDX-License-Identifier: AGPL-3.0-only
//! 18.6-T — `format_athlete_data_v1` produces a v1 athlete block whose field
//! names, shapes, and values match the authored golden file.
//!
//! 18.7-T — Fields whose source data is absent serialise exactly as the JS
//! does: `watching`/`self`/`isGapEst` omitted when falsy, `lastLap` null with
//! one lap, `state` null with no state.
//!
//! Both fail to compile until 18.6-I adds `format_athlete_data_v1` to
//! `src/web/format.rs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, items 18.6-T and 18.7-T.

use crate::support;

use ranchero::web::format::format_athlete_data_v1;
use support::{assert_json_parity, build_athlete};
use zwift_stats::MostRecentState;

// ── 18.6-T ───────────────────────────────────────────────────────────────────

/// Full golden-file parity test for an empty-bucket athlete:
///   - athlete 1001, course 5, sport 0, wt_offset 0.0, created at now=0.0
///   - no samples → stats and lap are both empty-bucket shapes
///   - not watching, not self → those fields are absent
///   - lapCount = 1 → lastLap is null
///   - no most_recent_state → state is null
///   - w_bal and timeInPowerZones are included (privacy not set)
///   - stats includes the deprecated wBal/timeInPowerZones/power.wBal/power.timeInZones
///   - lap omits the deprecated fields (no includeDeprecated)
#[test]
fn v1_athlete_block_matches_golden() {
    let ad     = build_athlete(&[]);
    let actual = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0, None);

    let golden_str = std::fs::read_to_string("tests/fixtures/format/v1_athlete.json")
        .expect("golden file missing — run 18.6-I to create tests/fixtures/format/v1_athlete.json");
    let golden: serde_json::Value = serde_json::from_str(&golden_str)
        .expect("golden file must be valid JSON");

    assert_json_parity(&actual, &golden);
}

// ── 18.7-T ───────────────────────────────────────────────────────────────────

/// `watching` is absent when the athlete is not the currently watched athlete.
#[test]
fn watching_omitted_when_not_watching() {
    let ad     = build_athlete(&[]);
    let result = format_athlete_data_v1(&ad, Some(9999), None, None, 0.0, 0.0, None);
    assert!(
        result.get("watching").is_none(),
        "expected `watching` to be absent, got: {:?}",
        result.get("watching"),
    );
}

/// `self` is absent when the athlete is not the logged-in athlete.
#[test]
fn self_omitted_when_not_self() {
    let ad     = build_athlete(&[]);
    let result = format_athlete_data_v1(&ad, None, Some(9999), None, 0.0, 0.0, None);
    assert!(
        result.get("self").is_none(),
        "expected `self` to be absent, got: {:?}",
        result.get("self"),
    );
}

/// `isGapEst` is absent when `is_gap_est` is false.
#[test]
fn is_gap_est_omitted_when_false() {
    let mut ad = build_athlete(&[]);
    ad.is_gap_est = false;
    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0, None);
    assert!(
        result.get("isGapEst").is_none(),
        "expected `isGapEst` to be absent, got: {:?}",
        result.get("isGapEst"),
    );
}

/// `lastLap` is null when there is exactly one lap slice (no completed lap yet).
#[test]
fn last_lap_null_with_one_lap() {
    let ad     = build_athlete(&[]);
    assert_eq!(ad.lap_slices.len(), 1, "build_athlete must start with one lap");
    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0, None);
    assert_eq!(
        result["lastLap"],
        serde_json::Value::Null,
        "expected `lastLap` to be null with a single lap",
    );
}

/// `state` is null when `most_recent_state` is None.
#[test]
fn state_null_with_no_state() {
    let ad     = build_athlete(&[]);
    assert!(ad.most_recent_state.is_none(), "build_athlete must have no state");
    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0, None);
    assert_eq!(
        result["state"],
        serde_json::Value::Null,
        "expected `state` to be null when most_recent_state is None",
    );
}

/// `state.latlng` is a two-element `[lat, lng]` array matching sauce4zwift's
/// `_formatState` output.  Separate `lat`/`lng` scalar fields must not appear.
///
/// 19.7-T: fails until `format_state` is changed to emit `latlng` instead of
/// separate `lat`/`lng` fields.
#[test]
fn state_latlng_is_array_not_separate_scalars() {
    let mut ad = build_athlete(&[]);
    ad.most_recent_state = Some(MostRecentState {
        world_time: 0.0, speed: 0.0, power: 0.0, heartrate: 0, cadence: 0,
        draft: 0.0, distance: 0.0, altitude: 0.0,
        lat: 1.23, lng: 4.56,
        course_id: 0, road_id: 0, road_time: 0.0, reverse: false,
        event_subgroup_id: 0, group_id: 0, time: 0.0, event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    });
    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0, None);
    let state = &result["state"];

    let latlng = &state["latlng"];
    assert!(latlng.is_array(), "state.latlng must be an array; got {state}");
    let arr = latlng.as_array().unwrap();
    assert_eq!(arr.len(), 2, "state.latlng must have exactly 2 elements; got {latlng}");
    assert!((arr[0].as_f64().unwrap() - 1.23).abs() < 1e-9, "latlng[0] must be lat=1.23; got {latlng}");
    assert!((arr[1].as_f64().unwrap() - 4.56).abs() < 1e-9, "latlng[1] must be lng=4.56; got {latlng}");

    assert!(state.get("lat").is_none(), "state.lat must be absent; got {state}");
    assert!(state.get("lng").is_none(), "state.lng must be absent; got {state}");
}
