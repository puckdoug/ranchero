// SPDX-License-Identifier: AGPL-3.0-only
//! Step 17 — `compute_route_distance`.
//!
//! Sauce's `_computeRouteDistance` (`stats.mjs:3197`) walks the road-section
//! manifest, but its fallback / lead-in handling is straightforward and is
//! what these tests pin down for the first-cut Rust port:
//!
//! - When `eventDistance` is inside the lead-in, the route distance is
//!   `eventDistance - leadInMetres`, i.e. negative or zero (the rider has
//!   not yet entered the lap).
//! - Once past the lead-in, route distance equals
//!   `eventDistance - leadInMetres`.
//! - The chosen lead-in length depends on `LeadInType` (event / free-ride /
//!   meetup). Lady Liberty has distinct values for all three.
//! - `route_end_meters` returns `leadInMetres + lapDistanceMetres` — sauce's
//!   `routeEnd`.

use approx::assert_relative_eq;
use zwift_routes::{LeadInType, Route};

const LADY_LIBERTY_ID:                u64 = 5103974;
// From the vendored data (kilometres):
//   distance               = 12.361
//   leadInDistance         = 0.28
//   leadInDistanceFreeRide = 0.694
//   leadInDistanceMeetups  = 0.694
const LAP_DISTANCE_M:        f64 = 12_361.0;
const LEAD_IN_EVENT_M:       f64 =    280.0;
const LEAD_IN_FREE_RIDE_M:   f64 =    694.0;
const LEAD_IN_MEETUP_M:      f64 =    694.0;

fn lady_liberty() -> &'static Route {
    Route::by_id(LADY_LIBERTY_ID).expect("Lady Liberty must be in the route table")
}

#[test]
fn lead_in_meters_matches_source_per_type() {
    let r = lady_liberty();
    assert_relative_eq!(r.lead_in_meters(LeadInType::Event),    LEAD_IN_EVENT_M,    epsilon = 1e-9);
    assert_relative_eq!(r.lead_in_meters(LeadInType::FreeRide), LEAD_IN_FREE_RIDE_M, epsilon = 1e-9);
    assert_relative_eq!(r.lead_in_meters(LeadInType::Meetup),   LEAD_IN_MEETUP_M,   epsilon = 1e-9);
}

#[test]
fn route_end_meters_is_lead_in_plus_lap() {
    let r = lady_liberty();
    assert_relative_eq!(
        r.route_end_meters(LeadInType::Event),
        LEAD_IN_EVENT_M + LAP_DISTANCE_M,
        epsilon = 1e-9,
    );
    assert_relative_eq!(
        r.route_end_meters(LeadInType::FreeRide),
        LEAD_IN_FREE_RIDE_M + LAP_DISTANCE_M,
        epsilon = 1e-9,
    );
}

#[test]
fn route_distance_at_start_of_lap_is_zero() {
    let r = lady_liberty();
    let d = r
        .route_distance_meters(LEAD_IN_EVENT_M, LeadInType::Event)
        .expect("inside route");
    assert_relative_eq!(d, 0.0, epsilon = 1e-6);
}

#[test]
fn route_distance_inside_lap_subtracts_lead_in() {
    let r = lady_liberty();
    // 5 km past start of lap, event lead-in: route distance == 5000 m.
    let event_dist = LEAD_IN_EVENT_M + 5_000.0;
    let d = r
        .route_distance_meters(event_dist, LeadInType::Event)
        .expect("inside route");
    assert_relative_eq!(d, 5_000.0, epsilon = 1e-6);
}

#[test]
fn route_distance_inside_lead_in_is_zero_or_negative() {
    let r = lady_liberty();
    // 100 m into the event lead-in (route hasn't started yet): -180 m.
    let d = r
        .route_distance_meters(100.0, LeadInType::Event)
        .expect("inside lead-in");
    assert_relative_eq!(d, 100.0 - LEAD_IN_EVENT_M, epsilon = 1e-6);
}

#[test]
fn route_distance_uses_selected_lead_in_type() {
    let r = lady_liberty();
    // Same event_distance, different lead-in types → different route_distance.
    let ev = 5_000.0;
    let event_d     = r.route_distance_meters(ev, LeadInType::Event).expect("event");
    let free_ride_d = r.route_distance_meters(ev, LeadInType::FreeRide).expect("freeride");
    // Difference equals the lead-in difference (414 m).
    assert_relative_eq!(
        event_d - free_ride_d,
        LEAD_IN_FREE_RIDE_M - LEAD_IN_EVENT_M,
        epsilon = 1e-6,
    );
}
