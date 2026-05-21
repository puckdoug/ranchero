// SPDX-License-Identifier: AGPL-3.0-only
//! Shared test-support utilities for golden-file parity tests (STEP 18).
//!
//! - `build_athlete` — constructs a deterministic `AthleteData` from a fixed
//!   sample script, calls `flush_all()` so single-point ingestion is visible.
//! - `assert_json_parity` — recursively asserts identical key sets at every
//!   object nesting level and value equality with an f64 tolerance on numbers.

use std::collections::BTreeSet;
use serde_json::Value;
use zwift_stats::AthleteData;

/// One row of the sample script fed to [`build_athlete`].
#[allow(dead_code)]
pub struct SampleRow {
    pub ts:      f64,
    pub power:   f64,
    pub hr:      f64,
    pub speed:   f64,
    pub cadence: f64,
    pub draft:   f64,
}

/// Build a deterministic `AthleteData` (athlete 1001, course 5) from a
/// sample script and flush internal buffers so values are immediately readable.
///
/// Both `now` (bucket start) and `world_time` offset are set to 0.0 so
/// elapsed-time arithmetic in formatters is straightforward: elapsed = last_ts.
#[allow(dead_code)]
pub fn build_athlete(script: &[SampleRow]) -> AthleteData {
    let now = 0.0_f64;
    let mut ad = AthleteData::new(1001, 5, 0, 0.0, now);
    for row in script {
        ad.bucket.ingest_power(row.ts, row.power);
        ad.bucket.ingest_hr(row.ts, row.hr);
        ad.bucket.ingest_speed(row.ts, row.speed);
        ad.bucket.ingest_cadence(row.ts, row.cadence);
        ad.bucket.ingest_draft(row.ts, row.draft);
    }
    ad.bucket.flush_all();
    ad
}

/// Assert that `actual` and `golden` are structurally identical JSON values.
///
/// Fails when:
/// - the key sets of any nested object differ (extra or missing keys), or
/// - any scalar value differs (numbers within f64 relative tolerance 1e-9 or
///   absolute tolerance 1e-12 are considered equal).
#[allow(dead_code)]
pub fn assert_json_parity(actual: &Value, golden: &Value) {
    assert_parity_at(actual, golden, "");
}

fn assert_parity_at(actual: &Value, golden: &Value, path: &str) {
    match (actual, golden) {
        (Value::Object(a_map), Value::Object(g_map)) => {
            let a_keys: BTreeSet<&String> = a_map.keys().collect();
            let g_keys: BTreeSet<&String> = g_map.keys().collect();
            assert_eq!(
                a_keys, g_keys,
                "key sets differ at '{path}': actual={a_keys:?}, golden={g_keys:?}",
            );
            for key in &a_keys {
                let child = if path.is_empty() {
                    (*key).clone()
                } else {
                    format!("{path}.{key}")
                };
                assert_parity_at(&a_map[*key], &g_map[*key], &child);
            }
        }
        (Value::Array(a_arr), Value::Array(g_arr)) => {
            assert_eq!(
                a_arr.len(), g_arr.len(),
                "array length differs at '{path}': actual={}, golden={}",
                a_arr.len(), g_arr.len(),
            );
            for (i, (a, g)) in a_arr.iter().zip(g_arr.iter()).enumerate() {
                assert_parity_at(a, g, &format!("{path}[{i}]"));
            }
        }
        (Value::Number(a_n), Value::Number(g_n)) => {
            let a_f = a_n.as_f64().unwrap_or(0.0);
            let g_f = g_n.as_f64().unwrap_or(0.0);
            let diff = (a_f - g_f).abs();
            let tol  = (a_f.abs().max(g_f.abs()) * 1e-9).max(1e-12);
            assert!(
                diff <= tol,
                "numeric mismatch at '{path}': actual={a_f}, golden={g_f}",
            );
        }
        (a, g) => {
            assert_eq!(a, g, "value mismatch at '{path}'");
        }
    }
}
