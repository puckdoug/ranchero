// SPDX-License-Identifier: AGPL-3.0-only
//! Zwift route tables and route-progress computations.
//!
//! Port target: sauce4zwift's `shared/routes.mjs` + the route branch of
//! `_computeRouteDistance` (`stats.mjs:3197`) / `_getEventOrRouteInfo`
//! (`stats.mjs:4317`).
//!
//! Data source: route metadata vendored from
//! [github.com/andipaetzold/zwift-data](https://github.com/andipaetzold/zwift-data)
//! (`src/routes.ts`), exported as `data/routes.json` at build time and
//! embedded into the binary via `include_str!`.
//!
//! That source provides distance, lead-in lengths (event / free-ride /
//! meetup), elevation, named segments, and segment-on-route positions, but
//! not the road-section manifest sauce uses for its full `_computeRouteDistance`
//! walk. The implementation here mirrors sauce's `eventDistance`-minus-lead-in
//! fallback path; the road-section / curves walk is a follow-up that requires
//! a separate vendoring of `shared/curves.mjs`-equivalent data.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// Which lead-in section of the route a rider is using.
///
/// Mirrors sauce's branch in `_computeRouteDistance` that picks among
/// `event`, `freeride`, or `meetup` lead-in distances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadInType {
    Event,
    FreeRide,
    Meetup,
}

/// One named-segment occurrence on a route, in route-distance km.
#[derive(Debug, Clone, Deserialize)]
pub struct SegmentOnRoute {
    pub from:    f64,
    pub to:      f64,
    pub segment: String,
}

/// A single Zwift route's metadata. Field names are camelCase in the vendored
/// JSON; Rust mirrors them in snake_case via serde rename.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub id:                            Option<u64>,
    pub name:                          String,
    pub slug:                          String,
    pub world:                         String,
    pub event_only:                    bool,
    /// Lap distance in kilometres (excludes lead-in).
    pub distance:                      f64,
    pub elevation:                     f64,
    pub lead_in_distance:              Option<f64>,
    pub lead_in_elevation:             Option<f64>,
    pub lead_in_distance_free_ride:    Option<f64>,
    pub lead_in_elevation_free_ride:   Option<f64>,
    pub lead_in_distance_meetups:      Option<f64>,
    pub lead_in_elevation_in_meetups:  Option<f64>,
    pub experience:                    Option<u32>,
    pub segments:                      Vec<String>,
    pub segments_on_route:             Vec<SegmentOnRoute>,
    pub level_locked:                  bool,
    pub lap:                           bool,
    #[serde(rename = "supportsTT")]
    pub supports_tt:                   bool,
    pub supports_meetups:              bool,
    pub sports:                        Vec<String>,
    pub strava_segment_id:             Option<u64>,
    pub strava_segment_url:            Option<String>,
    pub zwift_insider_url:             Option<String>,
    pub whats_on_zwift_url:            Option<String>,
    pub zwifter_bikes_url:             Option<String>,
}

/// Remaining-distance info matching sauce's route branch of
/// `_getEventOrRouteInfo` (`stats.mjs:4317`).
#[derive(Debug, Clone, PartialEq)]
pub struct RemainingInfo {
    /// Metres still to ride.
    pub remaining:        f64,
    /// Always `"distance"` for routes.
    pub remaining_metric: &'static str,
    /// Always `"route"` for routes.
    pub remaining_type:   &'static str,
    /// Total metres from start of lead-in to end of lap.
    pub remaining_end:    f64,
}

const ROUTES_JSON: &str = include_str!("../data/routes.json");

fn table() -> &'static HashMap<u64, Route> {
    static TABLE: OnceLock<HashMap<u64, Route>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let routes: Vec<Route> = serde_json::from_str(ROUTES_JSON)
            .expect("embedded routes.json must be valid");
        routes
            .into_iter()
            .filter_map(|r| r.id.map(|id| (id, r)))
            .collect()
    })
}

impl Route {
    /// Look up a route by its Zwift internal `routeId`.
    pub fn by_id(route_id: u64) -> Option<&'static Route> {
        table().get(&route_id)
    }

    /// Lead-in distance in metres for the given `lead_in` type, or `0.0` if
    /// the route has no lead-in field for that type.
    pub fn lead_in_meters(&self, lead_in: LeadInType) -> f64 {
        let km = match lead_in {
            LeadInType::Event    => self.lead_in_distance,
            LeadInType::FreeRide => self.lead_in_distance_free_ride,
            LeadInType::Meetup   => self.lead_in_distance_meetups,
        };
        km.unwrap_or(0.0) * 1_000.0
    }

    /// Route distance in metres from the start of the lap to the rider's
    /// current `event_distance`. Negative when the rider is still in the
    /// lead-in.
    ///
    /// First-cut port of `_computeRouteDistance` (`stats.mjs:3197`): uses the
    /// `eventDistance` fallback path. The full road-section / curves
    /// implementation is follow-up work once curves data is vendored.
    pub fn route_distance_meters(
        &self,
        event_distance_m: f64,
        lead_in:          LeadInType,
    ) -> Option<f64> {
        Some(event_distance_m - self.lead_in_meters(lead_in))
    }

    /// Total metres from start of lead-in to end of lap, for the given
    /// lead-in type. This is sauce's `routeEnd` value.
    pub fn route_end_meters(&self, lead_in: LeadInType) -> f64 {
        self.lead_in_meters(lead_in) + self.distance * 1_000.0
    }

    /// Compute the route branch of `_getEventOrRouteInfo`: returns the four
    /// `remaining*` fields the formatter spreads when no event subgroup is
    /// active. `None` when the position is off-route.
    pub fn remaining_info(
        &self,
        event_distance_m: f64,
        lead_in:          LeadInType,
    ) -> Option<RemainingInfo> {
        let route_distance = self.route_distance_meters(event_distance_m, lead_in)?;
        let remaining_end  = self.route_end_meters(lead_in);
        Some(RemainingInfo {
            remaining:        self.distance * 1_000.0 - route_distance,
            remaining_metric: "distance",
            remaining_type:   "route",
            remaining_end,
        })
    }
}
