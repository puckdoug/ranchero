// SPDX-License-Identifier: AGPL-3.0-only
//! 18.10-T — `format_data_slice`, `format_segment_slice`, `format_event_slice`,
//! and `filter_slices` produce correctly shaped JSON matching the authored
//! golden files.
//!
//! Key shape points verified by the goldens:
//! - `stats` is the v1 bucket-stats shape (period-keyed objects) without
//!   deprecated fields — matching `_getBucketStats` called with no options.
//! - `active` is true when `slice.end` is `None` (open slice).
//! - `startIndex`/`endIndex` for an empty slice are both 0.
//! - `start`/`end` are null for an empty slice (no times recorded yet).
//! - `startServerTime` = `to_server_time(wt_offset) + (slice.start - internal_created)`.
//! - `format_segment_slice` adds `segmentId`, `eventSubgroupId`,
//!   `startEventDistance`, `endEventDistance`, `incomplete`.
//! - `format_event_slice` adds `eventSubgroupId`.
//! - `filter_slices(active=true)` returns all slices; `filter_slices(active=false)`
//!   returns only closed slices (end is Some).
//!
//! Fails to compile until 18.10-I adds these functions to `src/web/format.rs`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.10-T.

mod support;

use ranchero::web::format::{format_data_slice, format_segment_slice, format_event_slice, filter_slices};
use support::{assert_json_parity, build_athlete, SampleRow};

fn empty_script() -> Vec<SampleRow> {
    vec![]
}

#[test]
fn data_slice_matches_golden() {
    let ad = build_athlete(&empty_script());
    let actual = format_data_slice(&ad.lap_slices[0], &ad);

    let golden_str = std::fs::read_to_string("tests/fixtures/format/data_slice.json")
        .expect("golden file missing — run 18.10-I to create tests/fixtures/format/data_slice.json");
    let golden: serde_json::Value = serde_json::from_str(&golden_str)
        .expect("golden file must be valid JSON");

    assert_json_parity(&actual, &golden);
}

#[test]
fn segment_slice_matches_golden() {
    let ad = build_athlete(&empty_script());
    let actual = format_segment_slice(&ad.lap_slices[0], &ad);

    let golden_str = std::fs::read_to_string("tests/fixtures/format/segment_slice.json")
        .expect("golden file missing");
    let golden: serde_json::Value = serde_json::from_str(&golden_str)
        .expect("golden file must be valid JSON");

    assert_json_parity(&actual, &golden);
}

#[test]
fn event_slice_matches_golden() {
    let ad = build_athlete(&empty_script());
    let actual = format_event_slice(&ad.lap_slices[0], &ad);

    let golden_str = std::fs::read_to_string("tests/fixtures/format/event_slice.json")
        .expect("golden file missing");
    let golden: serde_json::Value = serde_json::from_str(&golden_str)
        .expect("golden file must be valid JSON");

    assert_json_parity(&actual, &golden);
}

/// `active=true` means do not filter out open slices — all slices are returned.
#[test]
fn filter_slices_active_true_returns_all() {
    let ad = build_athlete(&empty_script());
    // lap_slices[0] is open (end = None)
    let result = filter_slices(&ad.lap_slices, None, None, true);
    assert_eq!(result.len(), 1, "active=true must include open slices");
}

/// `active=false` means keep only closed slices (end is Some).
#[test]
fn filter_slices_active_false_excludes_open() {
    let ad = build_athlete(&empty_script());
    // lap_slices[0] is open; active=false should exclude it
    let result = filter_slices(&ad.lap_slices, None, None, false);
    assert_eq!(result.len(), 0, "active=false must exclude open slices");
}

/// A closed slice (end is Some) appears in active=false results.
#[test]
fn filter_slices_active_false_includes_closed() {
    let mut ad = build_athlete(&empty_script());
    ad.lap_slices[0].end = Some(1.0);
    let result = filter_slices(&ad.lap_slices, None, None, false);
    assert_eq!(result.len(), 1, "active=false must return closed slices");
}
