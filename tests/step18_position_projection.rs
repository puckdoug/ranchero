// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 18 — integration tests for world-meta-driven position projection.
//
// These cover the three wired-up behaviours the plan calls out:
//
// - `altitude_adjusted_by_world_meta` — `MostRecentState.altitude` is the
//   sauce4zwift formula `(z - seaLevel + eleOffset) / 100 *
//   physicsSlopeScale` (`stats.mjs:3022`), not raw `z / 100`.
// - `latlng_projected` — `MostRecentState.lat` / `.lng` carry the projected
//   coordinates rather than `0.0`; emitted as separate scalars (QE1).
// - `web_mercator_x_y` — the formatter (or state) carries the WebMercator
//   `(x, y)` derived from the projected lat/lng, populated rather than absent.
//
// Red state: the wiring in `route_player_state` (`src/web/proto_to_stats.rs`)
// today divides raw `z` by 100, and `ProtoView::lat()` / `.lng()` return
// `0.0`. There is no `x` / `y` field on `MostRecentState`. Each assertion
// below fails until Step 18's green phase imports `zwift-worlds` and rewires
// the projection.

use std::sync::Arc;

use ranchero::web::{proto_to_stats, WebState};
use zwift_proto::PlayerState;
use zwift_worlds::WorldMeta;

const WATOPIA_COURSE_ID: i32 = 6;

fn watopia_or_skip() -> &'static WorldMeta {
    WorldMeta::by_course_id(WATOPIA_COURSE_ID).expect(
        "Watopia (course 6) must be present in the vendored worlds table — \
         Step 18 cannot wire production without at least one course of data",
    )
}

fn watopia_proto(x: f64, y: f64, z: f32) -> PlayerState {
    PlayerState {
        id:         Some(1001),
        world_time: Some(10_000),
        world:      Some(WATOPIA_COURSE_ID),
        x:          Some(x as f32),
        y_altitude: Some(y as f32),
        z:          Some(z),
        speed:      Some(36_000_000),
        power:      Some(200),
        ..Default::default()
    }
}

#[test]
fn altitude_adjusted_by_world_meta() {
    let meta = watopia_or_skip();
    let z_cm = 50_000.0_f32;
    let proto = watopia_proto(0.0, 0.0, z_cm);

    let state = Arc::new(WebState::new());
    proto_to_stats::route_player_state(&proto, &state, 1.0, 0);

    let registry = state.registry.read().unwrap();
    let mrs = registry
        .get(1001)
        .expect("athlete must be in registry")
        .most_recent_state
        .expect("most_recent_state must be set");

    let expected = meta.adjust_altitude(z_cm as f64);
    assert!(
        (mrs.altitude - expected).abs() < 1e-6,
        "altitude must equal worldMeta-adjusted value: expected {expected}, got {}",
        mrs.altitude,
    );
}

#[test]
fn latlng_projected_as_scalars() {
    let meta = watopia_or_skip();
    // Pick an off-origin point so `(0, 0)` projecting to `(latOffset, lonOffset)`
    // doesn't accidentally line up with a fallback `(0.0, 0.0)`.
    let (x, y) = (123_400.0_f64, -98_700.0_f64);
    let proto = watopia_proto(x, y, 0.0);

    let state = Arc::new(WebState::new());
    proto_to_stats::route_player_state(&proto, &state, 1.0, 0);

    let registry = state.registry.read().unwrap();
    let mrs = registry
        .get(1001)
        .expect("athlete must be in registry")
        .most_recent_state
        .expect("most_recent_state must be set");

    let (exp_lat, exp_lng) = meta.project_lat_lng(x, y);
    assert!(
        (mrs.lat - exp_lat).abs() < 1e-9,
        "lat must equal projected value: expected {exp_lat}, got {}",
        mrs.lat,
    );
    assert!(
        (mrs.lng - exp_lng).abs() < 1e-9,
        "lng must equal projected value: expected {exp_lng}, got {}",
        mrs.lng,
    );
    // Sanity: lat/lng are non-zero. Today the wiring returns 0.0, so this is
    // the proximate failure on red.
    assert!(mrs.lat != 0.0 || mrs.lng != 0.0, "lat/lng must not both be 0.0");
}

#[test]
fn web_mercator_x_y_populates_on_state() {
    // MostRecentState gains `x` and `y` fields holding the WebMercator
    // projection (tile coords in [0, 1]).  Today these fields do not exist;
    // this test fails to compile until Step 18 adds them.
    let meta = watopia_or_skip();
    let (x, y) = (200.0_f64, -100.0_f64);
    let proto = watopia_proto(x, y, 0.0);

    let state = Arc::new(WebState::new());
    proto_to_stats::route_player_state(&proto, &state, 1.0, 0);

    let registry = state.registry.read().unwrap();
    let mrs = registry
        .get(1001)
        .expect("athlete must be in registry")
        .most_recent_state
        .expect("most_recent_state must be set");

    let (lat, lng) = meta.project_lat_lng(x, y);
    let (exp_x, exp_y) = zwift_worlds::web_mercator_projection(lat, lng);
    assert!(
        (mrs.x - exp_x).abs() < 1e-9 && (mrs.y - exp_y).abs() < 1e-9,
        "WebMercator (x, y) must equal projected value: expected ({exp_x}, {exp_y}), got ({}, {})",
        mrs.x, mrs.y,
    );
}
