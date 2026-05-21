// SPDX-License-Identifier: AGPL-3.0-only
//! 18.8-T — `format_bucket_stats_v2` produces a v2 bucket-stats block
//! whose field names, shapes, and values match the authored golden file.
//!
//! Key differences from the v1 shape tested in `format_v1_stats.rs`:
//! - `peaks` and `smooth` are **arrays**, not period-keyed objects.
//! - Each `smooth` element is `{period, avg}`, not a bare number.
//! - The `np` sub-block includes a `max` field (absent in v1).
//! - Deprecated fields (`wBal`, `timeInPowerZones`, `power.wBal`,
//!   `power.timeInZones`) are absent.
//!
//! Fails to compile until 18.8-I adds `format_bucket_stats_v2` to
//! `src/web/format.rs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.8-T.

mod support;

use ranchero::web::format::format_bucket_stats_v2;
use support::{assert_json_parity, build_athlete, SampleRow};

/// Same 70-sample deterministic script used by the v1 stats tests.
///
/// 70 seconds of 200 W / 140 bpm / 30 kph / 90 rpm / 0 W draft fills the
/// 5 s, 15 s, and 60 s peak windows but not the 300 s+ windows.  With
/// `ts_offset_ms = 0` every golden value is derivable from the constants above.
fn make_script() -> Vec<SampleRow> {
    (0..70)
        .map(|i| SampleRow {
            ts:      i as f64,
            power:   200.0,
            hr:      140.0,
            speed:   30.0,
            cadence: 90.0,
            draft:   0.0,
        })
        .collect()
}

#[test]
fn v2_stats_block_matches_golden() {
    let ad     = build_athlete(&make_script());
    let actual = format_bucket_stats_v2(&ad.bucket, &ad, None, 0.0);

    let golden_str = std::fs::read_to_string("tests/fixtures/format/v2_stats.json")
        .expect("golden file missing — run 18.8-I to create tests/fixtures/format/v2_stats.json");
    let golden: serde_json::Value = serde_json::from_str(&golden_str)
        .expect("golden file must be valid JSON");

    assert_json_parity(&actual, &golden);
}
