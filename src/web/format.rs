// SPDX-License-Identifier: AGPL-3.0-only

//! Athlete payload formatters for the HTTP and WebSocket layers.
//!
//! These are pure functions: they take an `AthleteData` reference and context
//! IDs and return a `serde_json::Value`. They carry no connection to the HTTP
//! server or subscription engine.

use serde::Serialize;
use serde_json::{json, Value};
use zwift_relay::ZWIFT_EPOCH_MS;
use zwift_stats::{AthleteData, DataBucket, SignalStats, NpStats, calc_tss};
use zwift_stats::athlete::MostRecentState;

const MAX_SMOOTH_PERIOD: f64 = 1200.0;
const MIN_NP_PERIOD: f64 = 300.0;

fn local_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn period_key(p: f64) -> String {
    if p == p.trunc() {
        format!("{}", p as i64)
    } else {
        format!("{p}")
    }
}

fn signal_stats_v1(stats: &SignalStats, periods: &[f64]) -> Value {
    let mut peaks_map = serde_json::Map::new();
    let mut smooth_map = serde_json::Map::new();

    for (i, &period) in periods.iter().enumerate() {
        let key = period_key(period);

        let peak_json = match stats.peaks.get(i).and_then(|opt| opt.as_ref()) {
            Some(p) => json!({
                "period": p.period,
                "avg":    p.avg,
                "time":   p.time,
                "ts":     p.ts
            }),
            None => json!({"period": period, "avg": null, "time": null, "ts": null}),
        };
        peaks_map.insert(key.clone(), peak_json);

        if period <= MAX_SMOOTH_PERIOD {
            let smooth_val = stats.smooth.iter()
                .find(|s| s.period == period)
                .map(|s| json!(s.avg))
                .unwrap_or(Value::Null);
            smooth_map.insert(key, smooth_val);
        }
    }

    json!({
        "avg":    stats.avg,
        "max":    stats.max,
        "peaks":  Value::Object(peaks_map),
        "smooth": Value::Object(smooth_map)
    })
}

fn np_stats_v1(avg: Option<f64>, stats: &NpStats, np_periods: &[f64]) -> Value {
    let mut peaks_map = serde_json::Map::new();
    let mut smooth_map = serde_json::Map::new();

    for (i, &period) in np_periods.iter().enumerate() {
        let key = period_key(period);

        let peak_json = match stats.peaks.get(i).and_then(|opt| opt.as_ref()) {
            Some(p) => json!({
                "period": p.period,
                "avg":    p.avg,
                "time":   p.time,
                "ts":     p.ts
            }),
            None => json!({"period": period, "avg": null, "time": null, "ts": null}),
        };
        peaks_map.insert(key.clone(), peak_json);

        if period <= MAX_SMOOTH_PERIOD {
            let smooth_val = stats.smooth.iter()
                .find(|s| s.period == period)
                .map(|s| json!(s.avg))
                .unwrap_or(Value::Null);
            smooth_map.insert(key, smooth_val);
        }
    }

    json!({
        "avg":    avg,
        "peaks":  Value::Object(peaks_map),
        "smooth": Value::Object(smooth_map)
    })
}

/// Format a `DataBucket` in the v1 stats shape (`_getBucketStats` with
/// `includeDeprecated: true` from `stats.mjs:2666`).
///
/// When `include_deprecated` is true the output includes the deprecated top-level
/// `wBal`/`timeInPowerZones` keys and the matching `power.wBal`/`power.timeInZones`
/// keys, matching the `includeDeprecated: true` branch of `_getBucketStats`.
/// When false those keys are omitted, matching the lap/lastLap call sites.
pub fn format_bucket_stats_v1(
    bucket:           &DataBucket,
    athlete:          &AthleteData,
    ftp:              Option<f64>,
    ts_offset_ms:     f64,
    include_deprecated: bool,
) -> Value {
    let power         = bucket.power();
    let power_primary = power.primary();

    let last_ts      = power_primary.last_time().unwrap_or(bucket.start());
    let elapsed_time = last_ts - bucket.start();
    let active_time  = power_primary.active();
    let np           = power_primary.np(true);

    let tss      = np.zip(ftp).and_then(|(n, f)| calc_tss(n, active_time, f));
    let power_kj = power_primary.joules() / 1000.0;
    let draft_kj = bucket.draft().primary().joules() / 1000.0;

    let w_bal_json: Option<Value> = if include_deprecated {
        Some(if athlete.event_privacy.hide_w_bal {
            Value::Null
        } else {
            json!(athlete.w_bal.value())
        })
    } else {
        None
    };

    let time_in_zones_json: Option<Value> = if include_deprecated {
        Some(if athlete.event_privacy.hide_ftp {
            json!([])
        } else {
            let zones: Vec<Value> = athlete.time_in_power_zones.value().iter()
                .map(|z| json!({"label": z.label, "time": z.time}))
                .collect();
            json!(zones)
        })
    } else {
        None
    };

    let power_periods: Vec<f64> = power.periodized().iter().map(|e| e.period).collect();
    let power_stats   = power.stats(ts_offset_ms);
    let mut power_json = signal_stats_v1(&power_stats, &power_periods);
    let power_map = power_json.as_object_mut().unwrap();
    power_map.insert("np".to_string(),  json!(np));
    power_map.insert("tss".to_string(), json!(tss));
    power_map.insert("kj".to_string(),  json!(power_kj));
    if let Some(ref w) = w_bal_json {
        power_map.insert("wBal".to_string(),      w.clone());
        power_map.insert("timeInZones".to_string(),
            time_in_zones_json.as_ref().unwrap().clone());
    }

    let np_periods: Vec<f64> = power.periodized().iter()
        .filter(|e| e.period >= MIN_NP_PERIOD)
        .map(|e| e.period)
        .collect();
    let np_avg   = power_primary.np(false);
    let np_stats = power.np_stats(ts_offset_ms);
    let np_json  = np_stats_v1(np_avg, &np_stats, &np_periods);

    let speed_periods: Vec<f64> = bucket.speed().periodized().iter().map(|e| e.period).collect();
    let speed_stats   = bucket.speed().stats(ts_offset_ms);
    let speed_json    = signal_stats_v1(&speed_stats, &speed_periods);

    let hr_periods: Vec<f64> = bucket.hr().periodized().iter().map(|e| e.period).collect();
    let hr_stats      = bucket.hr().stats(ts_offset_ms);
    let hr_json       = signal_stats_v1(&hr_stats, &hr_periods);

    let cadence_periods: Vec<f64> = bucket.cadence().periodized().iter().map(|e| e.period).collect();
    let cadence_stats = bucket.cadence().stats(ts_offset_ms);
    let cadence_json  = signal_stats_v1(&cadence_stats, &cadence_periods);

    let draft_periods: Vec<f64> = bucket.draft().periodized().iter().map(|e| e.period).collect();
    let draft_stats   = bucket.draft().stats(ts_offset_ms);
    let mut draft_json = signal_stats_v1(&draft_stats, &draft_periods);
    draft_json.as_object_mut().unwrap().insert("kj".to_string(), json!(draft_kj));

    let mut result = json!({
        "elapsedTime":  elapsed_time,
        "activeTime":   active_time,
        "coffeeTime":   (bucket.coffee_time() / 1000.0).round() as i64,
        "workTime":     (bucket.work_time()   / 1000.0).round() as i64,
        "followTime":   (bucket.follow_time() / 1000.0).round() as i64,
        "soloTime":     (bucket.solo_time()   / 1000.0).round() as i64,
        "workKj":       bucket.work_kj(),
        "followKj":     bucket.follow_kj(),
        "soloKj":       bucket.solo_kj(),
        "power":        power_json,
        "np":           np_json,
        "speed":        speed_json,
        "hr":           hr_json,
        "cadence":      cadence_json,
        "draft":        draft_json
    });

    if let (Some(w), Some(z)) = (w_bal_json, time_in_zones_json) {
        result["wBal"]             = w;
        result["timeInPowerZones"] = z;
    }

    result
}

// ---------------------------------------------------------------------------
// v1 state formatter
// ---------------------------------------------------------------------------

fn format_state(state: &MostRecentState) -> Value {
    json!({
        "worldTime":        state.world_time,
        "speed":            state.speed,
        "power":            state.power,
        "heartrate":        state.heartrate,
        "cadence":          state.cadence,
        "draft":            state.draft,
        "distance":         state.distance,
        "altitude":         state.altitude,
        "lat":              state.lat,
        "lng":              state.lng,
        "courseId":         state.course_id,
        "roadId":           state.road_id,
        "roadTime":         state.road_time,
        "reverse":          state.reverse,
        "eventSubgroupId":  state.event_subgroup_id,
        "groupId":          state.group_id,
        "time":             state.time,
        "eventDistance":    state.event_distance,
    })
}

// ---------------------------------------------------------------------------
// v1 athlete record
// ---------------------------------------------------------------------------

/// Serialization shape for `_formatAthleteData` (`stats.mjs:4388`).
///
/// Fields with `skip_serializing_if = "Option::is_none"` are omitted from JSON
/// when absent, matching the JS `x: undefined` pattern.  Fields that use plain
/// `Option<T>` (no skip) serialize as `null` when `None`, matching `x: null`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AthleteDataV1 {
    created_server_time: i64,
    created: f64,
    updated: f64,
    age: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    watching: Option<bool>,
    #[serde(rename = "self", skip_serializing_if = "Option::is_none")]
    self_: Option<bool>,
    course_id: u32,
    athlete_id: u32,
    // Null when athlete is not in the cache; always present.
    athlete: Option<Value>,
    stats: Value,
    lap: Value,
    // Null when lapCount == 1; always present.
    last_lap: Option<Value>,
    lap_count: usize,
    // Null when most_recent_state is None; always present.
    state: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_subgroup_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_position: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_participants: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    game_state: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_gap_est: Option<bool>,
    // Privacy-gated: None = hidden (omit); Some(Null) = not hidden, no value; Some(v) = value.
    #[serde(skip_serializing_if = "Option::is_none")]
    w_bal: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_in_power_zones: Option<Value>,
    // Spread from _getEventOrRouteInfo; absent when no event/route data.
    #[serde(skip_serializing_if = "Option::is_none")]
    event_leader: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_sweeper: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_end: Option<f64>,
}

/// Format an athlete record in the v1 shape (`_formatAthleteData`,
/// `stats.mjs:4388`).
///
/// - `watching_id` / `self_id`: the currently-watched and logged-in athlete IDs
///   used to set the optional `watching` / `self` flags.
/// - `ftp`: the athlete's FTP (for TSS computation); pass `None` when not known.
/// - `now`: current local time in seconds (Unix epoch) used for `age`.
/// - `ts_offset_ms`: local-time offset in milliseconds added to peak timestamps.
pub fn format_athlete_data_v1(
    athlete:      &AthleteData,
    watching_id:  Option<u32>,
    self_id:      Option<u32>,
    ftp:          Option<f64>,
    now:          f64,
    ts_offset_ms: f64,
) -> Value {
    let id = athlete.athlete_id;
    let lap_count = athlete.lap_slices.len();

    let stats = format_bucket_stats_v1(&athlete.bucket, athlete, ftp, ts_offset_ms, true);

    let lap = athlete.lap_slices.last()
        .map(|s| format_bucket_stats_v1(&s.bucket, athlete, ftp, ts_offset_ms, false))
        .unwrap_or_else(|| format_bucket_stats_v1(&athlete.bucket, athlete, ftp, ts_offset_ms, false));

    let last_lap = if lap_count > 1 {
        athlete.lap_slices.get(lap_count - 2)
            .map(|s| format_bucket_stats_v1(&s.bucket, athlete, ftp, ts_offset_ms, false))
    } else {
        None  // serializes as null
    };

    let state_json = athlete.most_recent_state.as_ref().map(|s| format_state(s));

    let w_bal = if athlete.event_privacy.hide_w_bal {
        None  // omit (hidden)
    } else {
        Some(json!(athlete.w_bal.value()))
    };

    let time_in_power_zones = if athlete.event_privacy.hide_ftp {
        None  // omit (hidden)
    } else {
        let zones: Vec<Value> = athlete.time_in_power_zones.value().iter()
            .map(|z| json!({"label": z.label, "time": z.time}))
            .collect();
        Some(json!(zones))
    };

    let record = AthleteDataV1 {
        created_server_time: athlete.wt_offset as i64 + ZWIFT_EPOCH_MS,
        created:             athlete.created,
        updated:             athlete.updated,
        age:                 now - athlete.internal_updated,
        watching:            (watching_id == Some(id)).then_some(true),
        self_:               (self_id    == Some(id)).then_some(true),
        course_id:           athlete.course_id,
        athlete_id:          id,
        athlete:             None,  // cache not available at formatter level
        stats,
        lap,
        last_lap,
        lap_count,
        state:               state_json,
        event_subgroup_id:   athlete.event_subgroup.as_ref().map(|sg| sg.id),
        event_position:      athlete.event_position,
        event_participants:  athlete.event_participants,
        game_state:          None,  // only emitted for the self athlete with a live session
        gap:                 athlete.gap,
        gap_distance:        athlete.gap_distance,
        is_gap_est:          athlete.is_gap_est.then_some(true),
        w_bal,
        time_in_power_zones,
        // _getEventOrRouteInfo spread: not yet wired (requires route/event metadata)
        event_leader:     None,
        event_sweeper:    None,
        remaining:        None,
        remaining_metric: None,
        remaining_type:   None,
        remaining_end:    None,
    };

    serde_json::to_value(record).expect("AthleteDataV1 is always serializable")
}

// ---------------------------------------------------------------------------
// v2 athlete record
// ---------------------------------------------------------------------------

/// Format an athlete record in the v2 shape (`_formatAthleteDataV2`).
///
/// When `resources` is empty the full v1 shape is returned with `version: 2`
/// added. When resources are specified only the named fields are included.
pub(crate) fn format_athlete_v2(
    athlete:          &AthleteData,
    resources:        &[String],
    watching_id:      Option<u32>,
    self_athlete_id:  Option<u32>,
) -> Value {
    if resources.is_empty() {
        let now    = local_now();
        let ts_ms  = athlete.wt_offset * 1000.0 + ZWIFT_EPOCH_MS as f64;
        let mut obj = format_athlete_data_v1(athlete, watching_id, self_athlete_id, None, now, ts_ms);
        obj["version"] = json!(2);
        obj
    } else {
        let mut obj = serde_json::Map::new();
        for resource in resources {
            let value = match resource.as_str() {
                "stats"            => json!({}),
                "lap"              => json!({}),
                "lastLap"          => json!(null),
                "laps"             => json!([]),
                "segments"         => json!([]),
                "events"           => json!([]),
                "state"            => json!(null),
                "athlete"          => json!(null),
                "timeInPowerZones" => json!(null),
                _                  => continue,
            };
            obj.insert(resource.clone(), value);
        }
        Value::Object(obj)
    }
}
