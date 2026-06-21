// SPDX-License-Identifier: AGPL-3.0-only
//! Step 17 — `route_remaining_fields`.
//!
//! Pins the route branch of sauce's `_getEventOrRouteInfo` (`stats.mjs:4317`):
//! given a route and the rider's current `eventDistance`, `remaining_info`
//! returns the four fields the formatter spreads when no event subgroup is
//! active:
//!
//! ```text
//! { remaining, remainingMetric: "distance", remainingType: "route", remainingEnd }
//! ```
//!
//! These fields are hardcoded as `None` in the v1/v2 formatters today; once
//! the formatter consumes `Route::remaining_info`, those slots populate.

use approx::assert_relative_eq;
use zwift_routes::{LeadInType, RemainingInfo, Route};

const LADY_LIBERTY_ID: u64 = 5103974;
const LAP_DISTANCE_M:  f64 = 12_361.0;
const LEAD_IN_EVENT_M: f64 =    280.0;

fn lady_liberty() -> &'static Route {
    Route::by_id(LADY_LIBERTY_ID).expect("Lady Liberty must be in the route table")
}

#[test]
fn remaining_info_at_start_of_lap_returns_full_lap_remaining() {
    let r = lady_liberty();
    let info = r
        .remaining_info(LEAD_IN_EVENT_M, LeadInType::Event)
        .expect("inside route");

    assert_eq!(info.remaining_metric, "distance");
    assert_eq!(info.remaining_type,   "route");

    let expected_end = LEAD_IN_EVENT_M + LAP_DISTANCE_M;
    assert_relative_eq!(info.remaining_end, expected_end, epsilon = 1e-9);
    assert_relative_eq!(info.remaining,     LAP_DISTANCE_M, epsilon = 1e-6);
}

#[test]
fn remaining_info_partway_through_lap() {
    let r = lady_liberty();
    // 5 km past start of lap.
    let event_dist = LEAD_IN_EVENT_M + 5_000.0;
    let info = r
        .remaining_info(event_dist, LeadInType::Event)
        .expect("inside route");

    let expected_end       = LEAD_IN_EVENT_M + LAP_DISTANCE_M;
    let expected_remaining = LAP_DISTANCE_M - 5_000.0;
    assert_relative_eq!(info.remaining_end, expected_end,       epsilon = 1e-9);
    assert_relative_eq!(info.remaining,     expected_remaining, epsilon = 1e-6);
}

#[test]
fn remaining_info_at_end_of_lap_is_zero() {
    let r = lady_liberty();
    let event_dist = LEAD_IN_EVENT_M + LAP_DISTANCE_M;
    let info = r
        .remaining_info(event_dist, LeadInType::Event)
        .expect("at end of route");
    assert_relative_eq!(info.remaining, 0.0, epsilon = 1e-6);
}

#[test]
fn remaining_info_static_string_fields_are_exact_match() {
    // Field values mirror the JS spread `{ remainingMetric: 'distance',
    // remainingType: 'route' }`; the formatter parity tests assert these
    // exact strings, so guard them here too.
    let r = lady_liberty();
    let info: RemainingInfo = r
        .remaining_info(LEAD_IN_EVENT_M + 100.0, LeadInType::Event)
        .expect("inside route");
    assert_eq!(info.remaining_metric, "distance");
    assert_eq!(info.remaining_type,   "route");
}
