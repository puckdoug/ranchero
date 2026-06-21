// SPDX-License-Identifier: AGPL-3.0-only
//! 18.0-T — Parity harness smoke tests.
//!
//! Verifies that:
//! 1. `build_athlete` produces an `AthleteData` with the expected seed values.
//! 2. `assert_json_parity` passes when both objects have identical structure
//!    and values.
//! 3. `assert_json_parity` panics when the actual object is missing a key that
//!    the golden object has.
//!
//! All three tests fail (red) until `18.0-I` implements the support module.
//!
//! See docs/plans/STEP-18-format-payloads.md, items 18.0-T and 18.0-I.

use crate::support;

use serde_json::json;

#[test]
fn build_athlete_returns_seeded_athlete_id() {
    let ad = support::build_athlete(&[]);
    assert_eq!(ad.athlete_id, 1001);
}

#[test]
fn assert_json_parity_passes_for_identical_objects() {
    support::assert_json_parity(
        &json!({ "a": 1, "b": "hello", "c": { "d": 3.14 } }),
        &json!({ "a": 1, "b": "hello", "c": { "d": 3.14 } }),
    );
}

#[test]
#[should_panic(expected = "key sets differ")]
fn assert_json_parity_detects_missing_key_in_actual() {
    support::assert_json_parity(
        &json!({ "a": 1 }),
        &json!({ "a": 1, "b": 2 }),
    );
}

#[test]
#[should_panic(expected = "numeric mismatch")]
fn assert_json_parity_detects_numeric_difference() {
    support::assert_json_parity(
        &json!({ "x": 1.0 }),
        &json!({ "x": 2.0 }),
    );
}
