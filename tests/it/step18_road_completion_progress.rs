// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 18 — road completion and progress populate.
//
// Sauce4zwift (`zwift.mjs:313, 321`):
//
// ```js
// o.progress       = (msg._progress >> 8 & 0xff) / 0xff;
// o.roadCompletion = o.reverse ? 1005000 - msg.roadTime : msg.roadTime - 5000;
// ```
//
// - `roadCompletion` is the direction-adjusted `road_time` already produced
//   by `ProtoView::road_time` (Step 4). Today the formatter emits it under
//   the JSON key `roadTime`; Step 18 surfaces it under the parity key
//   `roadCompletion` so downstream widgets see the sauce-shaped field.
// - `progress` is `(proto.progress >> 8) & 0xff) / 0xff` — the workout
//   progress fraction. Today neither `MostRecentState` nor the formatter
//   exposes it; this test fails until both add the field.
//
// Red state: `MostRecentState` has no `progress` or `road_completion`
// fields, and `format_state` emits neither `roadCompletion` nor `progress`.

use std::sync::Arc;

use ranchero::web::{format, proto_to_stats, WebState};
use zwift_proto::PlayerState;

#[test]
fn road_completion_populates_in_formatted_state() {
    // road_time = 35000, forward direction (f19 bit 2 set) ->
    // roadCompletion = 35000 - 5000 = 30000.
    let proto = PlayerState {
        id:         Some(1001),
        world_time: Some(10_000),
        world:      Some(6),
        road_time:  Some(35_000),
        f19:        Some(4),
        ..Default::default()
    };

    let state = Arc::new(WebState::new());
    proto_to_stats::route_player_state(&proto, &state, 1.0, 0);

    let registry = state.registry.read().unwrap();
    let mrs = registry
        .get(1001)
        .expect("athlete must be in registry")
        .most_recent_state
        .expect("most_recent_state must be set");

    let formatted = format::format_state(&mrs);
    assert_eq!(
        formatted.get("roadCompletion").and_then(|v| v.as_f64()),
        Some(30_000.0),
        "roadCompletion must be exposed under sauce's parity key: full payload = {formatted}",
    );
}

#[test]
fn progress_populates_in_formatted_state() {
    // proto.progress = 0xab00 -> (0xab00 >> 8) & 0xff = 0xab = 171
    // progress fraction = 171 / 255 ≈ 0.6706
    let proto = PlayerState {
        id:         Some(1001),
        world_time: Some(10_000),
        world:      Some(6),
        progress:   Some(0xab00),
        ..Default::default()
    };

    let state = Arc::new(WebState::new());
    proto_to_stats::route_player_state(&proto, &state, 1.0, 0);

    let registry = state.registry.read().unwrap();
    let mrs = registry
        .get(1001)
        .expect("athlete must be in registry")
        .most_recent_state
        .expect("most_recent_state must be set");

    let formatted = format::format_state(&mrs);
    let progress = formatted
        .get("progress")
        .and_then(|v| v.as_f64())
        .expect("progress field must be present in the formatted state");
    let expected = (171_f64) / 255.0;
    assert!(
        (progress - expected).abs() < 1e-6,
        "progress must equal (proto.progress >> 8) / 0xff: expected {expected}, got {progress}",
    );
}
