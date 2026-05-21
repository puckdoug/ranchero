// SPDX-License-Identifier: AGPL-3.0-only

//! Athlete payload formatters for the HTTP and WebSocket layers.
//!
//! These are pure functions: they take an `AthleteData` reference and context
//! IDs and return a `serde_json::Value`. They carry no connection to the HTTP
//! server or subscription engine.

use serde_json::{json, Value};
use zwift_stats::{AthleteData, DataBucket, SignalStats, NpStats, calc_tss};

const MAX_SMOOTH_PERIOD: f64 = 1200.0;
const MIN_NP_PERIOD: f64 = 300.0;

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
pub fn format_bucket_stats_v1(
    bucket:       &DataBucket,
    athlete:      &AthleteData,
    ftp:          Option<f64>,
    ts_offset_ms: f64,
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

    let w_bal_json: Value = if athlete.event_privacy.hide_w_bal {
        Value::Null
    } else {
        json!(athlete.w_bal.value())
    };

    let time_in_zones_json: Value = if athlete.event_privacy.hide_ftp {
        json!([])
    } else {
        let zones: Vec<Value> = athlete.time_in_power_zones.value().iter()
            .map(|z| json!({"label": z.label, "time": z.time}))
            .collect();
        json!(zones)
    };

    let power_periods: Vec<f64> = power.periodized().iter().map(|e| e.period).collect();
    let power_stats   = power.stats(ts_offset_ms);
    let mut power_json = signal_stats_v1(&power_stats, &power_periods);
    let power_map = power_json.as_object_mut().unwrap();
    power_map.insert("np".to_string(),        json!(np));
    power_map.insert("tss".to_string(),       json!(tss));
    power_map.insert("kj".to_string(),        json!(power_kj));
    power_map.insert("wBal".to_string(),      w_bal_json.clone());
    power_map.insert("timeInZones".to_string(), time_in_zones_json.clone());

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

    json!({
        "elapsedTime":       elapsed_time,
        "activeTime":        active_time,
        "coffeeTime":        (bucket.coffee_time() / 1000.0).round() as i64,
        "workTime":          (bucket.work_time()   / 1000.0).round() as i64,
        "followTime":        (bucket.follow_time() / 1000.0).round() as i64,
        "soloTime":          (bucket.solo_time()   / 1000.0).round() as i64,
        "workKj":            bucket.work_kj(),
        "followKj":          bucket.follow_kj(),
        "soloKj":            bucket.solo_kj(),
        "wBal":              w_bal_json,
        "timeInPowerZones":  time_in_zones_json,
        "power":             power_json,
        "np":                np_json,
        "speed":             speed_json,
        "hr":                hr_json,
        "cadence":           cadence_json,
        "draft":             draft_json
    })
}

/// Format an athlete record in the v1 shape (`_formatAthleteData`).
///
/// The `watching` and `self` flags are included only when true (omitted
/// otherwise), matching sauce4zwift behaviour.
pub(crate) fn format_athlete(
    athlete:          &AthleteData,
    watching_id:      Option<u32>,
    self_athlete_id:  Option<u32>,
) -> Value {
    let mut obj = json!({
        "athleteId": athlete.athlete_id,
        "courseId":  athlete.course_id,
        "lapCount":  athlete.lap_slices.len() as u32,
        "stats":     {},
        "lap":       {},
    });
    if watching_id == Some(athlete.athlete_id) {
        obj["watching"] = json!(true);
    }
    if self_athlete_id == Some(athlete.athlete_id) {
        obj["self"] = json!(true);
    }
    obj
}

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
        let mut obj = format_athlete(athlete, watching_id, self_athlete_id);
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
