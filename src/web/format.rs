// SPDX-License-Identifier: AGPL-3.0-only

//! Athlete payload formatters for the HTTP and WebSocket layers.
//!
//! These are pure functions: they take an `AthleteData` reference and context
//! IDs and return a `serde_json::Value`. They carry no connection to the HTTP
//! server or subscription engine.

use serde_json::{json, Value};
use zwift_stats::AthleteData;

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
