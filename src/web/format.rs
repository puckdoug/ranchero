// SPDX-License-Identifier: AGPL-3.0-only

//! Athlete payload formatters for the HTTP and WebSocket layers.
//!
//! These are pure functions: they take an `AthleteData` reference and context
//! IDs and return a `serde_json::Value`. They carry no connection to the HTTP
//! server or subscription engine.

use serde::Serialize;
use serde_json::{json, Value};
use zwift_relay::ZWIFT_EPOCH_MS;
use zwift_stats::{AthleteData, DataBucket, DataSlice, Sample, SignalStats, NpStats, calc_tss};
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

// ---------------------------------------------------------------------------
// v2 stat-shape helpers (array-based, mirrors getStatsV2 / getNPStatsV2)
// ---------------------------------------------------------------------------

fn signal_stats_v2(stats: &SignalStats, periods: &[f64]) -> Value {
    let peaks: Vec<Value> = periods.iter().enumerate()
        .map(|(i, &period)| {
            match stats.peaks.get(i).and_then(|opt| opt.as_ref()) {
                Some(p) => json!({
                    "period": p.period,
                    "avg":    p.avg,
                    "time":   p.time,
                    "ts":     p.ts
                }),
                None => json!({"period": period, "avg": null, "time": null, "ts": null}),
            }
        })
        .collect();

    let smooth: Vec<Value> = periods.iter()
        .filter(|&&period| period <= MAX_SMOOTH_PERIOD)
        .map(|&period| {
            let avg = stats.smooth.iter()
                .find(|s| s.period == period)
                .map(|s| json!(s.avg))
                .unwrap_or(Value::Null);
            json!({"period": period, "avg": avg})
        })
        .collect();

    json!({
        "avg":    stats.avg,
        "max":    stats.max,
        "peaks":  peaks,
        "smooth": smooth
    })
}

/// NP stats in v2 array shape.
///
/// `np_max` comes from `power.stats().max` — the JS `getNPStatsV2` exposes
/// `this._maxValue` which is the max raw-power value on the same
/// `PowerDataCollector` instance (`stats.mjs:304`).
fn np_stats_v2(
    np_avg:     Option<f64>,
    np_max:     f64,
    stats:      &NpStats,
    np_periods: &[f64],
) -> Value {
    let peaks: Vec<Value> = np_periods.iter().enumerate()
        .map(|(i, &period)| {
            match stats.peaks.get(i).and_then(|opt| opt.as_ref()) {
                Some(p) => json!({
                    "period": p.period,
                    "avg":    p.avg,
                    "time":   p.time,
                    "ts":     p.ts
                }),
                None => json!({"period": period, "avg": null, "time": null, "ts": null}),
            }
        })
        .collect();

    let smooth: Vec<Value> = np_periods.iter()
        .filter(|&&period| period <= MAX_SMOOTH_PERIOD)
        .map(|&period| {
            let avg = stats.smooth.iter()
                .find(|s| s.period == period)
                .map(|s| json!(s.avg))
                .unwrap_or(Value::Null);
            json!({"period": period, "avg": avg})
        })
        .collect();

    json!({
        "avg":    np_avg,
        "max":    np_max,
        "peaks":  peaks,
        "smooth": smooth
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

/// Format a `DataBucket` in the v2 stats shape (`_getBucketStatsV2`,
/// `stats.mjs:2714`).
///
/// Differences from the v1 shape:
/// - `peaks` and `smooth` are arrays (not period-keyed objects).
/// - Each `smooth` element is `{period, avg}`.
/// - The `np` sub-block includes `max` (the max raw-power value).
/// - Deprecated fields (`wBal`, `timeInPowerZones`, `power.wBal`,
///   `power.timeInZones`) are absent.
pub fn format_bucket_stats_v2(
    bucket:        &DataBucket,
    _athlete:      &AthleteData,
    ftp:           Option<f64>,
    ts_offset_ms:  f64,
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

    let power_periods: Vec<f64> = power.periodized().iter().map(|e| e.period).collect();
    let power_stats   = power.stats(ts_offset_ms);
    let np_max        = power_stats.max;
    let mut power_json = signal_stats_v2(&power_stats, &power_periods);
    let power_map = power_json.as_object_mut().unwrap();
    power_map.insert("np".to_string(),  json!(np));
    power_map.insert("tss".to_string(), json!(tss));
    power_map.insert("kj".to_string(),  json!(power_kj));

    let np_periods: Vec<f64> = power.periodized().iter()
        .filter(|e| e.period >= MIN_NP_PERIOD)
        .map(|e| e.period)
        .collect();
    let np_avg   = power_primary.np(false);
    let np_stats = power.np_stats(ts_offset_ms);
    let np_json  = np_stats_v2(np_avg, np_max, &np_stats, &np_periods);

    let speed_periods: Vec<f64> = bucket.speed().periodized().iter().map(|e| e.period).collect();
    let speed_json    = signal_stats_v2(&bucket.speed().stats(ts_offset_ms), &speed_periods);

    let hr_periods: Vec<f64> = bucket.hr().periodized().iter().map(|e| e.period).collect();
    let hr_json       = signal_stats_v2(&bucket.hr().stats(ts_offset_ms), &hr_periods);

    let cadence_periods: Vec<f64> = bucket.cadence().periodized().iter().map(|e| e.period).collect();
    let cadence_json  = signal_stats_v2(&bucket.cadence().stats(ts_offset_ms), &cadence_periods);

    let draft_periods: Vec<f64> = bucket.draft().periodized().iter().map(|e| e.period).collect();
    let mut draft_json = signal_stats_v2(&bucket.draft().stats(ts_offset_ms), &draft_periods);
    draft_json.as_object_mut().unwrap().insert("kj".to_string(), json!(draft_kj));

    json!({
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
    })
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

/// Base object always present in every v2 response, regardless of `resources`.
///
/// Mirrors the `data` object constructed at the top of `_formatAthleteDataV2`
/// (`stats.mjs:4334`).  Resource fields (`stats`, `lap`, `lastLap`, `state`,
/// `athlete`, `timeInPowerZones`, `laps`, `segments`, `events`) are NOT
/// included here; they are inserted into the serialised map below.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AthleteDataV2Base {
    version:             u32,
    created_server_time: i64,
    created:             f64,
    updated:             f64,
    age:                 f64,
    #[serde(rename = "self", skip_serializing_if = "Option::is_none")]
    self_:               Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    watching:            Option<bool>,
    course_id:           u32,
    athlete_id:          u32,
    lap_count:           usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_subgroup_id:   Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_position:      Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_participants:  Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    game_state:          Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap:                 Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap_distance:        Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_gap_est:          Option<bool>,
    // Privacy-gated: None = hidden (omit); Some(_) = present (may be null value)
    #[serde(skip_serializing_if = "Option::is_none")]
    w_bal:               Option<Value>,
    // Spread from _getEventOrRouteInfo; absent when no event/route metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    event_leader:        Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_sweeper:       Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining:           Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_metric:    Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_type:      Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_end:       Option<f64>,
}

/// Format an athlete record in the v2 shape (`_formatAthleteDataV2`,
/// `stats.mjs:4327`).
///
/// The base object is always returned.  When `resources` is non-empty the
/// named resource fields are added on top.
///
/// Decision D1: the JS bug at `stats.mjs:4376` writes the `lastLap` resource
/// value into `data.lap`, overwriting the lap key.  Ranchero deviates:
/// `lastLap` writes to `data.lastLap`.
pub(crate) fn format_athlete_v2(
    athlete:         &AthleteData,
    resources:       &[String],
    watching_id:     Option<u32>,
    self_athlete_id: Option<u32>,
    stats:           bool,
) -> Value {
    let now   = local_now();
    let id    = athlete.athlete_id;
    let ts_ms = athlete.wt_offset * 1000.0 + ZWIFT_EPOCH_MS as f64;

    let w_bal = if athlete.event_privacy.hide_w_bal {
        None
    } else {
        Some(json!(athlete.w_bal.value()))
    };

    let base = AthleteDataV2Base {
        version:            2,
        created_server_time: athlete.wt_offset as i64 + ZWIFT_EPOCH_MS,
        created:            athlete.created,
        updated:            athlete.updated,
        age:                now - athlete.internal_updated,
        self_:              (self_athlete_id == Some(id)).then_some(true),
        watching:           (watching_id     == Some(id)).then_some(true),
        course_id:          athlete.course_id,
        athlete_id:         id,
        lap_count:          athlete.lap_slices.len(),
        event_subgroup_id:  athlete.event_subgroup.as_ref().map(|sg| sg.id),
        event_position:     athlete.event_position,
        event_participants: athlete.event_participants,
        game_state:         None,
        gap:                athlete.gap,
        gap_distance:       athlete.gap_distance,
        is_gap_est:         athlete.is_gap_est.then_some(true),
        w_bal,
        event_leader:       None,
        event_sweeper:      None,
        remaining:          None,
        remaining_metric:   None,
        remaining_type:     None,
        remaining_end:      None,
    };

    let mut obj = match serde_json::to_value(base)
        .expect("AthleteDataV2Base is always serializable")
    {
        Value::Object(m) => m,
        _ => unreachable!(),
    };

    if !resources.is_empty() {
        let lap_count = athlete.lap_slices.len();
        for resource in resources {
            match resource.as_str() {
                "athlete" => {
                    obj.insert("athlete".into(), Value::Null);
                }
                "state" => {
                    let state_json = athlete.most_recent_state.as_ref().map(|s| format_state(s));
                    obj.insert("state".into(), state_json.unwrap_or(Value::Null));
                }
                "timeInPowerZones" => {
                    if !athlete.event_privacy.hide_ftp {
                        let zones: Vec<Value> = athlete.time_in_power_zones.value().iter()
                            .map(|z| json!({"label": z.label, "time": z.time}))
                            .collect();
                        obj.insert("timeInPowerZones".into(), json!(zones));
                    }
                }
                "stats" => {
                    obj.insert("stats".into(),
                        format_bucket_stats_v2(&athlete.bucket, athlete, None, ts_ms));
                }
                "lap" => {
                    let lap = athlete.lap_slices.last()
                        .map(|s| format_bucket_stats_v2(&s.bucket, athlete, None, ts_ms))
                        .unwrap_or_else(|| format_bucket_stats_v2(&athlete.bucket, athlete, None, ts_ms));
                    obj.insert("lap".into(), lap);
                }
                "lastLap" => {
                    // D1: JS writes to data.lap (stats.mjs:4376); ranchero writes to data.lastLap.
                    let last_lap = if lap_count > 1 {
                        athlete.lap_slices.get(lap_count - 2)
                            .map(|s| format_bucket_stats_v2(&s.bucket, athlete, None, ts_ms))
                            .unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    };
                    obj.insert("lastLap".into(), last_lap);
                }
                "laps" => {
                    let laps: Vec<Value> = athlete.lap_slices.iter()
                        .map(|s| format_data_slice_v2(s, athlete, stats, ts_ms))
                        .collect();
                    obj.insert("laps".into(), json!(laps));
                }
                "segments" => {
                    let segs: Vec<Value> = athlete.segment_slices.iter()
                        .map(|s| format_segment_slice_v2(s, athlete, stats, ts_ms))
                        .collect();
                    obj.insert("segments".into(), json!(segs));
                }
                "events" => {
                    let evts: Vec<Value> = athlete.event_slices.iter()
                        .map(|s| format_event_slice_v2(s, athlete, stats, ts_ms))
                        .collect();
                    obj.insert("events".into(), json!(evts));
                }
                _ => {}  // unknown resource names ignored
            }
        }
    }

    Value::Object(obj)
}

// ---------------------------------------------------------------------------
// v2 slice formatters — used only by `format_athlete_v2` for the
// `laps`/`segments`/`events` resources (`_formatDataSlice` with
// `{version: 2}`, `stats.mjs:1743-1765`).
// ---------------------------------------------------------------------------

/// Format a data slice in the v2 shape.
///
/// When `stats_flag` is true the `stats` field carries the full v2 bucket-stats
/// object; when false it is `null` — matching `_formatDataSlice` called with
/// `{version: 2, stats: false/true}` from `_formatAthleteDataV2`
/// (`stats.mjs:1750-1752`).
fn format_data_slice_v2(slice: &DataSlice, ad: &AthleteData, stats_flag: bool, ts_ms: f64) -> Value {
    let power       = slice.bucket.power().primary();
    let start_index: usize = 0;
    let end_index   = power.size().saturating_sub(1);

    let stats_json = if stats_flag {
        format_bucket_stats_v2(&slice.bucket, ad, None, ts_ms)
    } else {
        Value::Null
    };

    json!({
        "id":              slice.id,
        "stats":           stats_json,
        "active":          slice.end.is_none(),
        "startIndex":      start_index,
        "endIndex":        end_index,
        "startServerTime": ad.wt_offset as i64 + ZWIFT_EPOCH_MS
                           + (slice.start - ad.internal_created) as i64,
        "start":           power.time_at(0),
        "end":             power.time_at(end_index as i32),
        "sport":           slice.sport,
        "courseId":        slice.course_id,
    })
}

fn format_segment_slice_v2(slice: &DataSlice, ad: &AthleteData, stats_flag: bool, ts_ms: f64) -> Value {
    let mut obj = format_data_slice_v2(slice, ad, stats_flag, ts_ms);
    let map = obj.as_object_mut().expect("format_data_slice_v2 always returns an object");
    map.insert("segmentId".into(),          json!(slice.segment_id));
    map.insert("eventSubgroupId".into(),    json!(slice.event_subgroup_id));
    map.insert("startEventDistance".into(), json!(slice.start_event_distance));
    map.insert("endEventDistance".into(),   json!(slice.end_event_distance));
    map.insert("incomplete".into(),         json!(slice.incomplete));
    obj
}

fn format_event_slice_v2(slice: &DataSlice, ad: &AthleteData, stats_flag: bool, ts_ms: f64) -> Value {
    let mut obj = format_data_slice_v2(slice, ad, stats_flag, ts_ms);
    obj.as_object_mut()
        .expect("format_data_slice_v2 always returns an object")
        .insert("eventSubgroupId".into(), json!(slice.event_subgroup_id));
    obj
}

// ---------------------------------------------------------------------------
// Slice formatters (`_formatDataSlice` / `_formatSegmentDataSlice` /
// `_formatEventDataSlice`, `stats.mjs:1699-1762`)
// ---------------------------------------------------------------------------

/// Format a data slice in the v1 shape (`_formatDataSlice`, `stats.mjs:1741`).
///
/// `stats` uses the v1 bucket-stats shape without deprecated fields, matching
/// `_getBucketStats` called with no version option from the laps/segments/events
/// route handlers.
///
/// `startIndex` and `endIndex` are the first and last indices in the primary
/// power roll.  For the unbounded primary roll, the offset is always 0, so
/// `startIndex = 0` and `endIndex = size - 1` (both 0 when the slice is empty).
pub fn format_data_slice(slice: &DataSlice, ad: &AthleteData) -> Value {
    let power      = slice.bucket.power().primary();
    let start_index: usize = 0;
    let end_index = power.size().saturating_sub(1);
    let ts_ms = ad.wt_offset * 1000.0 + ZWIFT_EPOCH_MS as f64;

    json!({
        "id":              slice.id,
        "stats":           format_bucket_stats_v1(&slice.bucket, ad, None, ts_ms, false),
        "active":          slice.end.is_none(),
        "startIndex":      start_index,
        "endIndex":        end_index,
        "startServerTime": ad.wt_offset as i64 + ZWIFT_EPOCH_MS
                           + (slice.start - ad.internal_created) as i64,
        "start":           power.time_at(0),
        "end":             power.time_at(end_index as i32),
        "sport":           slice.sport,
        "courseId":        slice.course_id,
    })
}

/// Format a segment slice (`_formatSegmentDataSlice`, `stats.mjs:1699`).
///
/// Extends the base data-slice shape with `segmentId`, `eventSubgroupId`,
/// `startEventDistance`, `endEventDistance`, and `incomplete`.
pub fn format_segment_slice(slice: &DataSlice, ad: &AthleteData) -> Value {
    let mut obj = format_data_slice(slice, ad);
    let map = obj.as_object_mut().expect("format_data_slice always returns an object");
    map.insert("segmentId".into(),         json!(slice.segment_id));
    map.insert("eventSubgroupId".into(),   json!(slice.event_subgroup_id));
    map.insert("startEventDistance".into(), json!(slice.start_event_distance));
    map.insert("endEventDistance".into(),  json!(slice.end_event_distance));
    map.insert("incomplete".into(),        json!(slice.incomplete));
    obj
}

/// Format an event slice (`_formatEventDataSlice`, `stats.mjs:1719`).
///
/// Extends the base data-slice shape with `eventSubgroupId`.
pub fn format_event_slice(slice: &DataSlice, ad: &AthleteData) -> Value {
    let mut obj = format_data_slice(slice, ad);
    obj.as_object_mut()
        .expect("format_data_slice always returns an object")
        .insert("eventSubgroupId".into(), json!(slice.event_subgroup_id));
    obj
}

/// Filter a slice list (`_filterAthleteDataSlices`, `stats.mjs:1726`).
///
/// - `start_time`: keep only slices whose first power-roll time ≥ `start_time`.
/// - `end_time`: keep only slices that are open, or whose last power-roll time
///   > `end_time`.
/// - `active = true`: return all slices (open and closed).
/// - `active = false`: return only closed slices (those where `end` is `Some`).
pub fn filter_slices<'a>(
    slices:     &'a [DataSlice],
    start_time: Option<f64>,
    end_time:   Option<f64>,
    active:     bool,
) -> Vec<&'a DataSlice> {
    slices.iter()
        .filter(|s| {
            if let Some(st) = start_time {
                if s.bucket.power().primary().time_at(0).unwrap_or(0.0) < st {
                    return false;
                }
            }
            if let Some(et) = end_time {
                if s.end.is_some() {
                    let last = s.bucket.power().primary().last_time().unwrap_or(0.0);
                    if last <= et {
                        return false;
                    }
                }
            }
            if !active && s.end.is_none() {
                return false;
            }
            true
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Stream formatter (`_getAthleteStreams`, `stats.mjs:1782`)
// ---------------------------------------------------------------------------

/// Format the rolling time-series streams for an athlete.
///
/// Mirrors `_getAthleteStreams` (`stats.mjs:1782`): the primary power roll
/// supplies `time`, `power`, and the `active` boolean array; the other signal
/// rolls supply `speed`, `hr`, `cadence`, `draft`; and the per-state
/// `ad.streams` vectors supply `distance`, `altitude`, `latlng`, `wbal`.
///
/// The `active` predicate ports `!!+x || !(x instanceof Sauce.data.Pad)`:
/// `Value` → always true; `Pad(0)` → false; `Pad(v≠0)` → true; `Break` → false.
pub fn format_athlete_streams(ad: &AthleteData) -> Value {
    let power_roll   = ad.bucket.power().primary();
    let speed_roll   = ad.bucket.speed().primary();
    let hr_roll      = ad.bucket.hr().primary();
    let cadence_roll = ad.bucket.cadence().primary();
    let draft_roll   = ad.bucket.draft().primary();

    let times:   Vec<f64> = power_roll.times().to_vec();
    let power:   Vec<f64> = power_roll.values().iter().copied().map(|s| s.as_f64()).collect();
    let speed:   Vec<f64> = speed_roll.values().iter().copied().map(|s| s.as_f64()).collect();
    let hr:      Vec<f64> = hr_roll.values().iter().copied().map(|s| s.as_f64()).collect();
    let cadence: Vec<f64> = cadence_roll.values().iter().copied().map(|s| s.as_f64()).collect();
    let draft:   Vec<f64> = draft_roll.values().iter().copied().map(|s| s.as_f64()).collect();

    let active: Vec<bool> = power_roll.values().iter().copied().map(|s| match s {
        Sample::Value(_)     => true,
        Sample::Pad(v)       => v != 0.0,
        Sample::Break { .. } => false,
    }).collect();

    let latlng: Vec<[f64; 2]> = ad.streams.latlng.iter()
        .map(|ll| [ll.lat, ll.lng])
        .collect();

    json!({
        "time":     times,
        "power":    power,
        "speed":    speed,
        "hr":       hr,
        "cadence":  cadence,
        "draft":    draft,
        "active":   active,
        "distance": ad.streams.distance,
        "altitude": ad.streams.altitude,
        "latlng":   latlng,
        "wbal":     ad.streams.wbal,
    })
}
