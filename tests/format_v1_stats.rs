// SPDX-License-Identifier: AGPL-3.0-only
//! 18.5-T — `format_bucket_stats_v1` produces a v1 bucket-stats block
//! whose field names, shapes, and values match the authored golden file.
//!
//! The golden file encodes the expected output of `_getBucketStats`
//! (`stats.mjs:2664`) for a deterministic 70-sample script at constant
//! values:
//!
//! - `power.peaks` is a period-keyed object; each value is
//!   `{period, avg, time, ts}` or `null` if the window is not filled.
//! - `power.smooth` is a period-keyed object of bare numbers.
//! - `power` includes `np`, `tss`, `kj`, `wBal`, `timeInZones`.
//! - The top-level `np` key holds the period-keyed NP-stats object from
//!   `getNPStatsSlow`; its `smooth` values are bare NP numbers.
//! - `hr`, `speed`, `cadence`, `draft` follow the same `getStatsSlow` shape.
//! - `draft` additionally carries a top-level `kj` field.
//! - `elapsedTime`, `activeTime`, `coffeeTime`, `workTime`, `followTime`,
//!   `soloTime`, `workKj`, `followKj`, `soloKj` are present at the top
//!   level.
//!
//! Fails to compile until 18.5-I adds `format_bucket_stats_v1` to
//! `src/web/format.rs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.5-T.

mod support;

use ranchero::web::format::format_bucket_stats_v1;
use support::{assert_json_parity, build_athlete, SampleRow};

/// Deterministic 70-sample script at constant values.
///
/// 70 seconds of 200 W / 140 bpm / 30 kph / 90 rpm / 0 W draft fills the
/// 5 s, 15 s, and 60 s power/hr/speed/draft peak windows but not the 300 s+
/// windows.  With `ts_offset_ms = 0` the golden file can be authored against
/// known arithmetic without a running Zwift session.
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
fn v1_stats_block_matches_golden() {
    let ad     = build_athlete(&make_script());
    let actual = format_bucket_stats_v1(&ad.bucket, &ad, None, 0.0, true);

    let golden_str = std::fs::read_to_string("tests/fixtures/format/v1_stats.json")
        .expect("golden file missing — run 18.5-I to create tests/fixtures/format/v1_stats.json");
    let golden: serde_json::Value = serde_json::from_str(&golden_str)
        .expect("golden file must be valid JSON");

    assert_json_parity(&actual, &golden);
}
