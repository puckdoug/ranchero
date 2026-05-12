// SPDX-License-Identifier: AGPL-3.0-only

use approx::assert_abs_diff_eq;
use serde_json::Value;
use std::fs;
use zwift_stats::DataBucket;

// 14.21: Recorded-stream parity
#[test]
fn stream_parity_constant_power() {
    let fixture_path = format!(
        "{}/tests/fixtures/athlete_stream.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture_content = fs::read_to_string(&fixture_path)
        .unwrap_or_else(|err| {
            panic!("failed to read athlete_stream.json fixture at {fixture_path}: {err}")
        });
    let fixture: Value = serde_json::from_str(&fixture_content)
        .expect("failed to parse athlete_stream.json");

    let inputs = fixture["inputs"].as_array().expect("inputs should be an array");
    let expected = &fixture["outputs"];

    let mut bucket = DataBucket::new(0.0);

    for input in inputs {
        let time = input["time"].as_f64().expect("time should be f64");
        let power = input["power"].as_f64().expect("power should be f64");
        let hr = input["hr"].as_f64().expect("hr should be f64");
        let speed = input["speed"].as_f64().expect("speed should be f64");
        let cadence = input["cadence"].as_f64().expect("cadence should be f64");
        let draft = input["draft"].as_f64().expect("draft should be f64");

        bucket.ingest_power(time, power);
        bucket.ingest_hr(time, hr);
        bucket.ingest_speed(time, speed);
        bucket.ingest_cadence(time, cadence);
        bucket.ingest_draft(time, draft);
    }

    // Tolerance per "Design decisions worth pre-committing" in the
    // STEP 14 plan: epsilon = 1e-6 for parity-style comparisons.
    let eps = 1e-6;

    let power_max = expected["power"]["max"].as_f64().expect("power max expected");
    assert_abs_diff_eq!(bucket.power().max_value(), power_max, epsilon = eps);

    let hr_max = expected["hr"]["max"].as_f64().expect("hr max expected");
    assert_abs_diff_eq!(bucket.hr().max_value(), hr_max, epsilon = eps);

    let speed_max = expected["speed"]["max"].as_f64().expect("speed max expected");
    assert_abs_diff_eq!(bucket.speed().max_value(), speed_max, epsilon = eps);

    let cadence_max = expected["cadence"]["max"].as_f64().expect("cadence max expected");
    assert_abs_diff_eq!(bucket.cadence().max_value(), cadence_max, epsilon = eps);

    let draft_max = expected["draft"]["max"].as_f64().expect("draft max expected");
    assert_abs_diff_eq!(bucket.draft().max_value(), draft_max, epsilon = eps);

    assert!(
        bucket.power().max_value() > 0.0,
        "power primary should have data"
    );
    assert!(bucket.hr().max_value() > 0.0, "hr primary should have data");
    assert!(
        bucket.speed().max_value() > 0.0,
        "speed primary should have data"
    );
}
