// SPDX-License-Identifier: AGPL-3.0-only
//! 18.12-T — `format_athlete_streams` produces a correctly shaped stream
//! object matching the authored golden file.
//!
//! Key shape points verified by the golden:
//! - `time`/`power`/`speed`/`hr`/`cadence`/`draft` are arrays of f64, one
//!   entry per recorded sample.
//! - `active` is a boolean array: `true` for each `Sample::Value` entry in
//!   the power roll (all three samples are Value — see the `active` predicate
//!   port of `!!+x || !(x instanceof Sauce.data.Pad)`).
//! - `distance`/`altitude`/`latlng`/`wbal` are the athlete's stream arrays.
//!   `build_athlete` does not call `record_streams`, so all four are empty.
//!
//! Fails to compile until 18.12-I adds `format_athlete_streams` to
//! `src/web/format.rs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.12-T.

mod support;

use ranchero::web::format::format_athlete_streams;
use support::{assert_json_parity, build_athlete, SampleRow};

fn three_sample_script() -> Vec<SampleRow> {
    vec![
        SampleRow { ts: 0.0, power: 100.0, hr: 120.0, speed: 10.0, cadence: 80.0, draft: 5.0 },
        SampleRow { ts: 1.0, power: 200.0, hr: 125.0, speed: 11.0, cadence: 82.0, draft: 3.0 },
        SampleRow { ts: 2.0, power: 300.0, hr: 130.0, speed: 12.0, cadence: 84.0, draft: 1.0 },
    ]
}

/// The stream object has the right keys and values for a three-sample athlete.
#[test]
fn streams_matches_golden() {
    let ad = build_athlete(&three_sample_script());
    let actual = format_athlete_streams(&ad);

    let golden_str = std::fs::read_to_string("tests/fixtures/format/streams.json")
        .expect("golden file missing — create tests/fixtures/format/streams.json");
    let golden: serde_json::Value = serde_json::from_str(&golden_str)
        .expect("golden file must be valid JSON");

    assert_json_parity(&actual, &golden);
}
