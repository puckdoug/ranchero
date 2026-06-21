// SPDX-License-Identifier: AGPL-3.0-only
//! 17.28-Q — `route_player_state` updates `AthleteData.distance`,
//! `AthleteData.altitude`, and drives `smooth_grade` with the correct
//! raw-grade input.
//!
//! Two consecutive calls for the same athlete:
//!
//! 1. First call: `distance = 0`, `z = 0.0` — no grade update because
//!    the distance delta is zero.
//! 2. Second call: `distance = 1000` m, `z = 500.0` —
//!    altitude = 500.0 / 100.0 = 5.0 m,
//!    dist_delta = 1000.0 m,
//!    raw_grade = 5.0 / 1000.0 = 0.005.
//!
//! After the second call, `smooth_grade.get()` must equal one
//! `ExpWeightedAvg::new(SMOOTH_GRADE_WINDOW, 0.0).update(0.005)`.
//!
//! World-meta altitude adjustment (`(z - seaLevel + eleOffset) / 100 *
//! physicsSlopeScale`) is deferred; the router stores `z / 100.0` for now.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.28-Q.

use std::sync::Arc;

use ranchero::web::{proto_to_stats, WebState};
use zwift_proto::PlayerState;
use zwift_stats::{ExpWeightedAvg, periods::SMOOTH_GRADE_WINDOW};

fn athlete_proto(id: i64, world_time_ms: i64, distance_m: i32, z: f32) -> PlayerState {
    PlayerState {
        id:         Some(id),
        world_time: Some(world_time_ms),
        distance:   Some(distance_m),
        z:          Some(z),
        ..Default::default()
    }
}

#[test]
fn distance_altitude_grade_are_updated_by_router() {
    let state = Arc::new(WebState::new());
    let now   = 1.0_f64;

    // First call: zero distance and altitude — no grade update.
    let proto1 = athlete_proto(2001, 10_000_i64, 0, 0.0);
    proto_to_stats::route_player_state(&proto1, &state, now, 0);

    {
        let registry = state.registry.read().unwrap();
        let ad = registry.get(2001).expect("athlete must exist after first call");
        assert_eq!(ad.distance, 0.0, "distance must be 0 after first call");
        assert_eq!(ad.altitude, 0.0, "altitude must be 0 after first call");
        // smooth_grade seeded at 0, no update yet
        assert_eq!(ad.smooth_grade.get(), 0.0, "grade must be 0 after first call");
    }

    // Second call: 1000 m, z = 500 (altitude = 5.0 m).
    let proto2 = athlete_proto(2001, 20_000_i64, 1000, 500.0);
    proto_to_stats::route_player_state(&proto2, &state, now + 1.0, 0);

    {
        let registry = state.registry.read().unwrap();
        let ad = registry.get(2001).expect("athlete must exist after second call");

        assert_eq!(ad.distance, 1000.0, "distance must be 1000 m after second call");

        // altitude = z / 100.0; world-meta adjustment deferred (TODO in router)
        assert!(
            (ad.altitude - 5.0).abs() < 1e-9,
            "altitude must be 5.0 m (z/100); got {}",
            ad.altitude
        );

        // grade = 5.0 / 1000.0 = 0.005; EWA from seed 0 after one update
        let raw_grade = 5.0_f64 / 1000.0;
        let mut expected_ewa = ExpWeightedAvg::new(SMOOTH_GRADE_WINDOW, 0.0);
        let expected_grade = expected_ewa.update(raw_grade);
        assert!(
            (ad.smooth_grade.get() - expected_grade).abs() < 1e-9,
            "smooth_grade must be {:?}; got {:?}",
            expected_grade,
            ad.smooth_grade.get()
        );
    }
}
