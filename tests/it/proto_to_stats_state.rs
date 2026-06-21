// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 4 — failing tests for per-tick recording I: current state, streams,
// grade, road-time adjustment, and stale guard.
//
// Red state: `route_player_state` does not set `most_recent_state`, does not
// call `record_streams`, and has no elapsed-≤-0 guard. `ProtoView::road_time`
// returns the raw value without applying the direction adjustment.
//
// `publishes_grade` (S4-3) fails to compile in red state because `MostRecentState`
// has no `grade` field. This is intentional: the compilation failure drives the
// implementation of that field addition.
//
// All five tests pass after:
//   src/web/proto_to_stats.rs  — set `most_recent_state`, call `record_streams`,
//                                store the smoothed grade, add elapsed ≤ 0 guard
//   src/web/proto_view.rs      — apply reverse adjustment in `road_time()`
//   crates/zwift-stats          — add `grade: f64` to `MostRecentState`

use std::sync::Arc;

use ranchero::web::{proto_to_stats, WebState};
use ranchero::web::proto_view::ProtoView;
use zwift_proto::PlayerState;
use zwift_stats::athlete::PlayerStateView;

fn base_fixture() -> PlayerState {
    PlayerState {
        id:         Some(1001),
        world_time: Some(10_000), // 10 000 ms → 10.0 s
        power:      Some(250),
        speed:      Some(36_000_000), // mm/h → 10.0 m/s
        heartrate:  Some(140),
        world:      Some(3),
        ..Default::default()
    }
}

// S4-1: After one call to `route_player_state`, `AthleteData.most_recent_state`
// must be `Some` and carry the decoded values from the packet.
//
// Fails in red state: `most_recent_state` is never assigned inside
// `route_player_state`, so it remains `None`.
#[test]
fn records_most_recent_state() {
    let state = Arc::new(WebState::new());
    let proto = base_fixture();

    proto_to_stats::route_player_state(&proto, &state, 1.0, 0);

    let registry = state.registry.read().unwrap();
    let ad = registry
        .get(1001)
        .expect("athlete 1001 must be in registry after route_player_state");

    let mrs = ad
        .most_recent_state
        .expect("most_recent_state must be Some after route_player_state");

    assert_eq!(mrs.world_time, 10.0, "world_time must be 10.0 s");
    assert_eq!(mrs.power, 250.0, "power must be 250 W");
    assert!(
        (mrs.speed - 10.0).abs() < 1e-9,
        "speed must be 10.0 m/s; got {}",
        mrs.speed,
    );
    assert_eq!(mrs.heartrate, 140, "heartrate must be 140 bpm");
}

// S4-2: Each call to `route_player_state` must append one entry to every
// stream vector (distance, altitude, latlng, wbal).
//
// Fails in red state: `record_streams` is never called.
#[test]
fn records_streams() {
    let state = Arc::new(WebState::new());
    let proto = base_fixture();

    proto_to_stats::route_player_state(&proto, &state, 1.0, 0);

    let registry = state.registry.read().unwrap();
    let ad = registry
        .get(1001)
        .expect("athlete 1001 must be in registry after route_player_state");

    assert_eq!(ad.streams.distance.len(), 1, "distance stream must have one entry");
    assert_eq!(ad.streams.altitude.len(), 1, "altitude stream must have one entry");
    assert_eq!(ad.streams.latlng.len(), 1, "latlng stream must have one entry");
    assert_eq!(ad.streams.wbal.len(), 1, "wbal stream must have one entry");
}

// S4-3: The smoothed grade computed each tick must be stored in
// `most_recent_state.grade` so the formatter can expose it.
//
// Fails to compile in red state: `MostRecentState` has no `grade` field.
// After the fix: add `pub grade: f64` to `MostRecentState` and assign it from
// `ad.smooth_grade.get()` when building the snapshot in `route_player_state`.
#[test]
fn publishes_grade() {
    let state = Arc::new(WebState::new());

    // Packet 1: baseline position, establishes the reference distance/altitude.
    let proto1 = PlayerState {
        id:         Some(1001),
        world_time: Some(10_000),
        world:      Some(3),
        distance:   Some(0),
        z:          Some(0.0),
        ..Default::default()
    };
    proto_to_stats::route_player_state(&proto1, &state, 1.0, 0);

    // Packet 2: 1 000 m forward, 5 m up → positive grade (500 cm / 100 = 5 m).
    let proto2 = PlayerState {
        id:         Some(1001),
        world_time: Some(20_000),
        world:      Some(3),
        distance:   Some(1_000),
        z:          Some(500.0), // float z field; 500 cm → 5.0 m via /100
        ..Default::default()
    };
    proto_to_stats::route_player_state(&proto2, &state, 2.0, 0);

    let registry = state.registry.read().unwrap();
    let ad = registry
        .get(1001)
        .expect("athlete 1001 must be in registry");

    let mrs = ad
        .most_recent_state
        .expect("most_recent_state must be Some after second packet");

    assert!(
        mrs.grade > 0.0,
        "grade must be positive after climbing; got {}",
        mrs.grade,
    );
}

// S4-4: `ProtoView::road_time` must apply the direction adjustment before
// returning, matching sauce4zwift `zwift.mjs:321`:
//
//   forward  (bit 2 of f19 set):    road_time = raw − 5 000
//   reverse  (bit 2 of f19 absent): road_time = 1 005 000 − raw
//
// Fails in red state: `ProtoView::road_time` returns the raw value unchanged.
#[test]
fn road_time_reverse_adjustment() {
    let raw: i32 = 500_000;

    // Forward rider: bit 2 of f19 is set → road_time = raw − 5 000.
    let forward = PlayerState {
        road_time: Some(raw),
        f19:       Some(0b100), // bit 2 set → forward direction
        ..Default::default()
    };
    assert_eq!(
        ProtoView(&forward).road_time(),
        (raw - 5_000) as f64,
        "forward road_time must be raw − 5 000",
    );

    // Reverse rider: bit 2 of f19 absent → road_time = 1 005 000 − raw.
    let reverse = PlayerState {
        road_time: Some(raw),
        f19:       Some(0), // bit 2 absent → reverse direction
        ..Default::default()
    };
    assert_eq!(
        ProtoView(&reverse).road_time(),
        (1_005_000 - raw) as f64,
        "reverse road_time must be 1 005 000 − raw",
    );
}

// S4-5: A packet whose `world_time` does not advance past the previous packet's
// `world_time` must be silently dropped (sauce stats.mjs:3146: `elapsed <= 0`).
// Stream lengths must not change when a stale packet arrives.
//
// Fails in red state: there is no elapsed guard, so both packets are processed
// and the stream grows to length 2.
#[test]
fn rejects_stale_or_duplicate_state() {
    let state = Arc::new(WebState::new());

    // Packet 1: accepted, world_time advances from nothing to 10.0 s.
    let proto1 = PlayerState {
        id:         Some(1001),
        world_time: Some(10_000),
        world:      Some(3),
        distance:   Some(100),
        ..Default::default()
    };
    proto_to_stats::route_player_state(&proto1, &state, 1.0, 0);

    {
        let registry = state.registry.read().unwrap();
        let ad = registry.get(1001).expect("athlete 1001 must be registered");
        assert_eq!(ad.streams.distance.len(), 1, "first packet must populate streams");
    }

    // Packet 2: same world_time (elapsed = 0) — must be dropped by the stale guard.
    let proto2 = PlayerState {
        id:         Some(1001),
        world_time: Some(10_000), // identical → elapsed = 0 → stale
        world:      Some(3),
        distance:   Some(200),
        ..Default::default()
    };
    proto_to_stats::route_player_state(&proto2, &state, 2.0, 0);

    {
        let registry = state.registry.read().unwrap();
        let ad = registry.get(1001).expect("athlete 1001 must be registered");
        assert_eq!(
            ad.streams.distance.len(),
            1,
            "duplicate world_time must be dropped; stream must remain length 1",
        );
    }
}
