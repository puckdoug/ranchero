// SPDX-License-Identifier: AGPL-3.0-only

//! `PlayerStateView` adapter for decoded protobuf player states.
//!
//! Rust's orphan rule prevents implementing a trait from one external crate
//! (`zwift_stats::PlayerStateView`) for a type from another external crate
//! (`zwift_proto::PlayerState`) inside `ranchero`.  The standard fix is a
//! newtype wrapper defined here: `ProtoView<'a>` holds a reference to the
//! decoded proto and implements `PlayerStateView`.  Call sites wrap the proto
//! before passing it to any `PlayerStateView`-accepting function:
//!
//! ```ignore
//! active_segment_check(&mut ad, &ProtoView(&proto), &env, now);
//! ```
//!
//! Absent proto fields (`None` after prost decode) return the Rust zero value
//! for the return type (`0`, `0.0`, `false`).  This matches sauce4zwift's
//! reliance on proto3 default semantics throughout `_preprocessState`.
//!
//! Fields with no direct proto equivalent (`lat`, `lng`, `event_subgroup_id`,
//! `event_distance`) return zero.  World-XYZ → lat/lng projection and
//! event-subgroup wiring are deferred to later steps.
//!
//! Unit conversions mirror `proto_to_stats.rs`:
//! speed mm/h → m/s, cadence µHz → rpm, world_time ms → s, altitude cm → m.
//!
//! As-built notes (17.30-I):
//! - `MostRecentState` remains the impl used in tests and as the in-memory
//!   snapshot stored on `AthleteData`.  `ProtoView` is the production call
//!   path inside `route_player_state` and sibling callers.

use zwift_stats::{PlayerStateView, streams::LatLng};

/// Newtype adapter that implements `PlayerStateView` for a decoded
/// `zwift_proto::PlayerState`.
pub struct ProtoView<'a>(pub &'a zwift_proto::PlayerState);

/// Extract the road identifier from the packed `aux3` proto field.
///
/// Matches sauce4zwift's `decodePlayerStateFlags2Into`: bits 8–23 carry the
/// road id.
fn decode_road_id(aux3: u32) -> u32 {
    (aux3 >> 8) & 0xffff
}

/// Determine direction of travel from the packed `f19` proto field.
///
/// Matches sauce4zwift's `decodePlayerStateFlags1Into`: bit 2 is the
/// *forward* flag; its absence means the rider is going in reverse.
fn decode_reverse(f19: u32) -> bool {
    (f19 & 0b100) == 0
}

impl PlayerStateView for ProtoView<'_> {
    fn lat(&self) -> f64 {
        0.0
    }

    fn lng(&self) -> f64 {
        0.0
    }

    fn latlng(&self) -> LatLng {
        LatLng { lat: 0.0, lng: 0.0 }
    }

    fn course_id(&self) -> u32 {
        self.0.world.unwrap_or(0) as u32
    }

    fn road_id(&self) -> u32 {
        decode_road_id(self.0.aux3.unwrap_or(0))
    }

    fn road_time(&self) -> f64 {
        let raw = self.0.road_time.unwrap_or(0) as f64;
        if decode_reverse(self.0.f19.unwrap_or(0)) {
            1_005_000.0 - raw
        } else {
            raw - 5_000.0
        }
    }

    fn reverse(&self) -> bool {
        decode_reverse(self.0.f19.unwrap_or(0))
    }

    fn event_subgroup_id(&self) -> u32 {
        // tag 29: zwift-offline labels this field `groupId`; sauce4zwift reads it
        // as `eventSubgroupId` (zwift.proto line 33).  The sauce reading is correct
        // for event detection — the zwift-offline label is misleading.
        self.0.group_id.unwrap_or(0).max(0) as u32
    }

    fn group_id(&self) -> u32 {
        self.0.group_id.unwrap_or(0) as u32
    }

    fn world_time(&self) -> f64 {
        self.0.world_time.unwrap_or(0) as f64 / 1000.0
    }

    fn time(&self) -> f64 {
        self.0.time.unwrap_or(0) as f64
    }

    fn event_distance(&self) -> f64 {
        // tag 34: zwift-offline labels this field `dist_lat`; sauce4zwift reads it
        // as `_eventDistance` in centimetres and divides by 100 to get metres
        // (zwift.proto line 38, processPlayerStateMessage line 320).
        self.0.dist_lat.unwrap_or(0.0) as f64 / 100.0
    }

    fn speed(&self) -> f64 {
        self.0.speed.unwrap_or(0) as f64 / 3_600_000.0
    }

    fn power(&self) -> f64 {
        self.0.power.unwrap_or(0) as f64
    }

    fn heartrate(&self) -> u16 {
        self.0.heartrate.unwrap_or(0) as u16
    }

    fn cadence(&self) -> u16 {
        (self.0.cadence_u_hz.unwrap_or(0) as f64 * 60.0 / 1_000_000.0) as u16
    }

    fn draft(&self) -> f64 {
        self.0.draft.unwrap_or(0) as f64
    }

    fn distance(&self) -> f64 {
        self.0.distance.unwrap_or(0) as f64
    }

    fn altitude(&self) -> f64 {
        self.0.z.unwrap_or(0.0) as f64 / 100.0
    }

    fn is_empty(&self) -> bool {
        false
    }
}
