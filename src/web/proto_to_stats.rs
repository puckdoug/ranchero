// SPDX-License-Identifier: AGPL-3.0-only

//! Proto-to-stats router: translates decoded `zwift_proto::PlayerState`
//! records into `AthleteData` updates.
//!
//! This module is proto-aware so that `zwift-stats` stays proto-free.
//!
//! As-built notes (17.28-I):
//! - The one-second-deferred flush in `DataCollector::add` is a potential
//!   logic bug inherited from sauce4zwift's `_preprocessState` buffering
//!   behaviour.  `DataBucket::flush_all()` is the explicit escape hatch
//!   for tests and callers that need immediate visibility; production
//!   callers can let the buffer flush naturally on the next data point.

use std::sync::Arc;

use crate::web::WebState;

/// Route a decoded `PlayerState` proto into the athlete registry.
///
/// Calls `registry.upsert(athlete_id, course_id, sport, world_time, now)`,
/// then ingests telemetry into the resulting `AthleteData` with the
/// following unit conversions:
///
/// | Proto field    | Unit      | Stats unit | Conversion                          |
/// |----------------|-----------|------------|-------------------------------------|
/// | `cadence_u_hz` | µHz       | rpm        | `µhz * 60 / 1_000_000`             |
/// | `speed`        | mm/h      | m/s        | `mm_h / 3_600_000.0`               |
/// | `power`        | W         | W          | pass-through                        |
/// | `heartrate`    | bpm       | bpm        | pass-through                        |
/// | `draft`        | 0-1000    | 0-1000     | pass-through                        |
///
/// `course_id` is read from `proto.world`; `sport` from `proto.sport`.
/// Absent proto fields (prost `None`) are treated as `0`.
///
/// `now` is monotonic seconds (used for GC accounting).
/// `wall_clock_ms` is Unix-epoch milliseconds (used for event-subgroup
/// time-limit comparisons in later items).
pub fn route_player_state(
    proto:         &zwift_proto::PlayerState,
    state:         &Arc<WebState>,
    now:           f64,
    _wall_clock_ms: u64,
) {
    let athlete_id  = proto.id.unwrap_or(0) as u32;
    let course_id   = proto.world.unwrap_or(0) as u32;
    let sport       = proto.sport.unwrap_or(0) as u8;
    let world_time  = proto.world_time.unwrap_or(0) as f64 / 1000.0;

    let speed_mps   = proto.speed.unwrap_or(0) as f64 / 3_600_000.0;
    let cadence_rpm = proto.cadence_u_hz.unwrap_or(0) as f64 * 60.0 / 1_000_000.0;
    let power_w     = proto.power.unwrap_or(0) as f64;
    let hr_bpm      = proto.heartrate.unwrap_or(0) as f64;
    let draft       = proto.draft.unwrap_or(0) as f64;

    // TODO: apply world-meta adjustment: (z - seaLevel + eleOffset) / 100 *
    // physicsSlopeScale.  That requires vendoring the world-meta tables,
    // deferred to a later step.
    let altitude = proto.z.unwrap_or(0.0) as f64 / 100.0;
    let distance = proto.distance.unwrap_or(0) as f64;

    let mut registry = state.registry.write().unwrap();
    let ad = registry.upsert(athlete_id, course_id, sport, world_time, now);

    ad.ingest_power(now, world_time, power_w);
    ad.ingest_hr(now, world_time, hr_bpm);
    ad.ingest_speed(now, world_time, speed_mps);
    ad.ingest_cadence(now, world_time, cadence_rpm);
    ad.ingest_draft(now, world_time, draft);

    let dist_delta = distance - ad.distance;
    if dist_delta.abs() > f64::EPSILON {
        let alt_delta = altitude - ad.altitude;
        ad.smooth_grade.update(alt_delta / dist_delta);
    }
    ad.distance = distance;
    ad.altitude = altitude;
}
