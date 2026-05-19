// SPDX-License-Identifier: AGPL-3.0-only
//! 17.28-T — `route_player_state` upserts the athlete and ingests telemetry
//! with the correct unit conversions.
//!
//! Fails to compile until:
//! - `src/web/proto_to_stats.rs` is created with `route_player_state` (17.28-I)
//! - `DataBucket::flush_all()` is added to `zwift-stats` (17.28-I)
//!
//! See docs/plans/STEP-17-web-server.md, items 17.28-T / 17.28-I.

use std::sync::Arc;

use ranchero::web::{proto_to_stats, WebState};
use zwift_proto::PlayerState;

/// Build a `PlayerState` fixture with clean, round values that produce
/// integer-valued results after unit conversion.
///
/// - `cadence_u_hz = 1_000_000` µHz × 60 / 1_000_000  = 60 rpm
/// - `speed        = 36_000_000` mm/h ÷ 3_600_000      = 10.0 m/s
/// - `power        = 250` W                             → 250.0 (pass-through)
/// - `heartrate    = 140` bpm                           → 140.0 (pass-through)
/// - `draft        = 450`                               → 450.0 (pass-through)
fn fixture() -> PlayerState {
    PlayerState {
        id:            Some(1001),
        world_time:    Some(10_000),     // 10 000 ms = 10.0 s
        power:         Some(250),
        cadence_u_hz:  Some(1_000_000),
        speed:         Some(36_000_000),
        heartrate:     Some(140),
        draft:         Some(450),
        world:         Some(3),          // course_id 3
        sport:         Some(0),          // Sport::Cycling
        ..Default::default()
    }
}

#[test]
fn route_player_state_upserts_athlete_and_ingests_with_unit_conversions() {
    let state = Arc::new(WebState::new());
    let proto = fixture();
    let now: f64 = 1.0;
    let wall_clock_ms: u64 = 0;

    proto_to_stats::route_player_state(&proto, &state, now, wall_clock_ms);

    // --- athlete was upserted ---
    let mut registry = state.registry.write().unwrap();
    let ad = registry
        .get_mut(1001)
        .expect("athlete 1001 must be in registry after route_player_state");

    // Flush each DataCollector's internal buffer so max_value() is populated.
    ad.bucket.flush_all();

    // --- unit conversions ---
    // power: pass-through (watts)
    assert_eq!(
        ad.bucket.power_mut().max_value(),
        250.0,
        "power must be 250 W"
    );

    // heartrate: pass-through (bpm)
    assert_eq!(
        ad.bucket.hr_mut().max_value(),
        140.0,
        "heartrate must be 140 bpm"
    );

    // speed: mm/h → m/s   (36_000_000 / 3_600_000 = 10.0)
    assert!(
        (ad.bucket.speed_mut().max_value() - 10.0).abs() < 1e-9,
        "speed must be 10.0 m/s; got {}",
        ad.bucket.speed_mut().max_value()
    );

    // cadence: µHz → rpm   (1_000_000 * 60 / 1_000_000 = 60)
    assert_eq!(
        ad.bucket.cadence_mut().max_value(),
        60.0,
        "cadence must be 60 rpm"
    );

    // draft: pass-through
    assert_eq!(
        ad.bucket.draft_mut().max_value(),
        450.0,
        "draft must be 450"
    );
}
