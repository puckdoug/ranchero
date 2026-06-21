// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 5 — failing tests for per-tick recording II: road history, auto-lap,
// and work/follow/solo/coffee time splits.
//
// Red state:
//   - S5-3 fails to compile: `WebState` has no `auto_lap_config` field.
//   - S5-4 fails at runtime: `route_player_state` never calls
//     `road_history.record`, so `road_history.is_empty()` stays true.
//   - S5-5 fails at runtime: `route_player_state` never accumulates
//     work/follow/solo/coffee time, so the sum is always zero.
//
// All three tests pass after:
//   src/web/state.rs           — add `pub auto_lap_config: Option<AutoLapConfig>`
//   src/web/proto_to_stats.rs  — call `road_history.record`, `auto_lap_check`,
//                                and accumulate the four time splits

use std::sync::Arc;

use ranchero::web::{proto_to_stats, WebState};
use zwift_proto::PlayerState;
use zwift_stats::{AutoLapConfig, AutoLapMetric};

// S5-3: When `WebState.auto_lap_config` is set with a time threshold, repeated
// calls to `route_player_state` whose `time` field crosses the threshold must
// each create a new lap slice.
//
// Fails to compile in red state: `WebState` has no `auto_lap_config` field.
#[test]
fn auto_lap_detected_by_distance_and_time() {
    let mut ws = WebState::new();
    ws.auto_lap_config = Some(AutoLapConfig {
        metric:    AutoLapMetric::Time,
        threshold: 10.0,
    });
    let state = Arc::new(ws);

    // Route 25 ticks, each advancing `time` by 1 s.
    // With threshold = 10 s, a lap fires at ticks 11 and 21.
    for i in 0..25_i64 {
        let proto = PlayerState {
            id:         Some(500),
            world_time: Some((i + 1) * 1_000),
            time:       Some((i + 1) as i32),
            world:      Some(1),
            ..Default::default()
        };
        proto_to_stats::route_player_state(&proto, &state, (i + 1) as f64, 0);
    }

    let registry = state.registry.read().unwrap();
    let ad = registry.get(500).expect("athlete 500 must be in registry");
    assert!(
        ad.lap_slices.len() >= 2,
        "at least 2 auto-laps must be created (threshold=10s, 25 ticks); got {}",
        ad.lap_slices.len(),
    );
}

// S5-4: After two calls to `route_player_state` for the same athlete,
// `AthleteData.road_history` must be non-empty.
//
// Fails at runtime in red state: `route_player_state` never calls
// `road_history.record`.
#[test]
fn road_history_recorded() {
    let state = Arc::new(WebState::new());

    let proto1 = PlayerState {
        id:         Some(600),
        world_time: Some(1_000),
        world:      Some(1),
        road_time:  Some(100_000),
        f19:        Some(0b100), // forward direction
        ..Default::default()
    };
    proto_to_stats::route_player_state(&proto1, &state, 1.0, 0);

    let proto2 = PlayerState {
        id:         Some(600),
        world_time: Some(2_000),
        world:      Some(1),
        road_time:  Some(200_000),
        f19:        Some(0b100),
        ..Default::default()
    };
    proto_to_stats::route_player_state(&proto2, &state, 2.0, 0);

    let registry = state.registry.read().unwrap();
    let ad = registry.get(600).expect("athlete 600 must be in registry");
    assert!(
        !ad.road_history.is_empty(),
        "road_history must have entries after two route_player_state calls",
    );
}

// S5-5: After several ticks with non-zero power, the sum of
// `work_time + follow_time + solo_time` must be greater than zero.
//
// Fails at runtime in red state: `route_player_state` never accumulates
// any of the four time buckets.
#[test]
fn work_follow_solo_coffee_split() {
    let state = Arc::new(WebState::new());

    for i in 0..5_i64 {
        let proto = PlayerState {
            id:         Some(700),
            world_time: Some((i + 1) * 1_000),
            power:      Some(200),
            speed:      Some(36_000_000), // 10 m/s; sauce checks state.speed > 0
            world:      Some(1),
            ..Default::default()
        };
        proto_to_stats::route_player_state(&proto, &state, (i + 1) as f64, 0);
    }

    let registry = state.registry.read().unwrap();
    let ad = registry.get(700).expect("athlete 700 must be in registry");
    let active_time = ad.bucket.work_time() + ad.bucket.follow_time() + ad.bucket.solo_time();
    assert!(
        active_time > 0.0,
        "time splits must accumulate across ticks with non-zero power; \
         got work={}, follow={}, solo={}",
        ad.bucket.work_time(),
        ad.bucket.follow_time(),
        ad.bucket.solo_time(),
    );
}
