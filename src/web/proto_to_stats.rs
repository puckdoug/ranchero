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

use zwift_stats::{
    apply_event_state, auto_lap_check, start_athlete_lap,
    ExpWeightedAvg, MostRecentState, PlayerStateView,
    periods::SMOOTH_GRADE_WINDOW,
};

use crate::web::WebState;
use crate::web::proto_view::ProtoView;

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
/// `now` is Unix-epoch seconds (used for GC accounting). It must come from
/// the same clock the GC reads — `gc_tick_loop` derives its `now` from
/// `SystemTime`, and `route_player_state` stamps last-seen times the GC later
/// compares against, so a monotonic clock here would make the GC mis-drop
/// athletes.
/// `wall_clock_ms` is Unix-epoch milliseconds (used for event-subgroup
/// time-limit comparisons in later items).
pub fn route_player_state(
    proto:         &zwift_proto::PlayerState,
    state:         &Arc<WebState>,
    now:           f64,
    wall_clock_ms: u64,
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

    // Drop packets that do not advance the athlete's world clock (sauce stats.mjs:3146).
    let prev_world_time = ad.most_recent_state.map(|s| s.world_time).unwrap_or(f64::NEG_INFINITY);
    if world_time <= prev_world_time {
        return;
    }

    // Detect a session-context change (world or sport changed since the last
    // frame for this athlete).  Matches sauce4zwift's `_preprocessState`
    // world/sport change handler.
    let context_changed = course_id != ad.course_id || sport != ad.sport;
    if context_changed {
        ad.course_id       = course_id;
        ad.sport           = sport;
        ad.distance_offset += ad.distance;
        ad.smooth_grade    = ExpWeightedAvg::new(SMOOTH_GRADE_WINDOW, 0.0);
        start_athlete_lap(ad, now);
    }

    ad.ingest_power(now, world_time, power_w);
    ad.ingest_hr(now, world_time, hr_bpm);
    ad.ingest_speed(now, world_time, speed_mps);
    ad.ingest_cadence(now, world_time, cadence_rpm);
    ad.ingest_draft(now, world_time, draft);

    // Skip the grade update on context-change frames: the distance and
    // altitude deltas would span two different courses and are meaningless.
    // Matches sauce4zwift's `state.grade = 0` in `_preprocessState`.
    if !context_changed {
        let dist_delta = distance - ad.distance;
        if dist_delta.abs() > f64::EPSILON {
            let alt_delta = altitude - ad.altitude;
            ad.smooth_grade.update(alt_delta / dist_delta);
        }
    }
    ad.distance = distance;
    ad.altitude = altitude;

    let view = ProtoView(proto);

    // Auto-lap check precedes road-history and stream recording, matching
    // sauce stats.mjs:3075 → _autoLapCheck → _recordAthleteRoadHistory →
    // _recordAthleteStats order.
    if let Some(cfg) = &state.auto_lap_config {
        auto_lap_check(ad, &view, cfg, now);
    }

    // Road history uses the previous tick's most_recent_state as the
    // `prev` argument (sauce _recordAthleteRoadHistory:3105).
    let prev_state_ref: Option<&dyn PlayerStateView> =
        ad.most_recent_state.as_ref().map(|s| s as &dyn PlayerStateView);
    ad.road_history.record(&view, prev_state_ref);

    ad.record_streams(&view, now);

    // Time splits (sauce stats.mjs:3397-3463).
    // `coffee_stop` = active power-up is COFFEE_STOP (proto enum value 9),
    // packed in the low 4 bits of aux3 (decodePlayerStateFlags2).
    let coffee_stop = (proto.aux3.unwrap_or(0) & 0xf) == 9;
    if let Some(prev) = &ad.most_recent_state {
        let elapsed_ms = (world_time - prev.world_time) * 1000.0;
        if elapsed_ms < 10_000.0 {
            if coffee_stop {
                ad.bucket.set_coffee_time(ad.bucket.coffee_time() + elapsed_ms);
            } else if speed_mps > 0.0 {
                let kj = power_w * elapsed_ms / 1_000_000.0;
                if view.group_id() != 0 {
                    if draft > 0.0 {
                        ad.bucket.set_follow_time(ad.bucket.follow_time() + elapsed_ms);
                        ad.bucket.set_follow_kj(ad.bucket.follow_kj() + kj);
                    } else {
                        ad.bucket.set_work_time(ad.bucket.work_time() + elapsed_ms);
                        ad.bucket.set_work_kj(ad.bucket.work_kj() + kj);
                    }
                } else {
                    ad.bucket.set_solo_time(ad.bucket.solo_time() + elapsed_ms);
                    ad.bucket.set_solo_kj(ad.bucket.solo_kj() + kj);
                }
            }
        }
    }

    let grade = ad.smooth_grade.get();
    ad.most_recent_state = Some(MostRecentState {
        world_time,
        speed:             speed_mps,
        power:             power_w,
        heartrate:         hr_bpm as u16,
        cadence:           cadence_rpm as u16,
        draft,
        distance,
        altitude,
        lat:               view.lat(),
        lng:               view.lng(),
        course_id,
        road_id:           view.road_id(),
        road_time:         view.road_time(),
        reverse:           view.reverse(),
        event_subgroup_id: view.event_subgroup_id(),
        group_id:          view.group_id(),
        time:              view.time(),
        event_distance:    view.event_distance(),
        grade,
    });

    let sg_lookup = state.event_subgroups.read().unwrap();
    apply_event_state(
        ad,
        &view,
        state.self_athlete_id.unwrap_or(0),
        &sg_lookup,
        state.event_behavior,
        now,
        wall_clock_ms,
    );
}

/// Populate the athlete registry from `proto`, then emit a
/// `GameEvent::PlayerState` on the web state's broadcast channel.
///
/// The registry update MUST happen before the event is sent so that the
/// stats fanout task reads an already-populated entry when it wakes up.
/// Reversing the order causes the fanout to find an empty registry and
/// discard the event.
pub fn bridge_player_state_event(
    proto:         &zwift_proto::PlayerState,
    state:         &Arc<WebState>,
    now:           f64,
    wall_clock_ms: u64,
) {
    route_player_state(proto, state, now, wall_clock_ms);

    if let Some(tx) = &state.game_events_tx {
        let _ = tx.send(crate::daemon::relay::GameEvent::PlayerState {
            athlete_id:    proto.id.unwrap_or(0),
            power_w:       proto.power.unwrap_or(0),
            cadence_u_hz:  proto.cadence_u_hz.unwrap_or(0),
            speed_mm_h:    proto.speed.unwrap_or(0),
            world_time_ms: proto.world_time.unwrap_or(0),
            world:         proto.world.unwrap_or(0),
            sport:         proto.sport.unwrap_or(0),
            distance:      proto.distance.unwrap_or(0),
            z:             proto.z.unwrap_or(0.0),
            draft:         proto.draft.unwrap_or(0),
            heartrate:     proto.heartrate.unwrap_or(0),
        });
    }
}
