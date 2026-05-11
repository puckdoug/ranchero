// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::Value;
use std::fs;
use zwift_stats::DataBucket;

// 14.21: Recorded-stream parity
#[test]
fn stream_parity_constant_power() {
    // Load fixture
    let fixture_path = format!(
        "{}/tests/fixtures/athlete_stream.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture_content = fs::read_to_string(&fixture_path)
        .expect(&format!("Failed to read athlete_stream.json fixture at {}", fixture_path));
    let fixture: Value = serde_json::from_str(&fixture_content)
        .expect("Failed to parse athlete_stream.json");

    let inputs = fixture["inputs"].as_array().expect("inputs should be an array");
    let expected = &fixture["outputs"];

    // Create bucket and replay stream
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

    // Verify power stats
    let power_max = expected["power"]["max"].as_f64().expect("power max expected");
    assert!(
        (bucket.power().max_value() - power_max).abs() < 1e-6,
        "power max_value mismatch: got {}, expected {}",
        bucket.power().max_value(),
        power_max
    );

    // Verify HR stats
    let hr_max = expected["hr"]["max"].as_f64().expect("hr max expected");
    assert!(
        (bucket.hr().max_value() - hr_max).abs() < 1e-6,
        "hr max_value mismatch: got {}, expected {}",
        bucket.hr().max_value(),
        hr_max
    );

    // Verify speed stats
    let speed_max = expected["speed"]["max"].as_f64().expect("speed max expected");
    assert!(
        (bucket.speed().max_value() - speed_max).abs() < 1e-6,
        "speed max_value mismatch: got {}, expected {}",
        bucket.speed().max_value(),
        speed_max
    );

    // Verify cadence stats
    let cadence_max = expected["cadence"]["max"].as_f64().expect("cadence max expected");
    assert!(
        (bucket.cadence().max_value() - cadence_max).abs() < 1e-6,
        "cadence max_value mismatch: got {}, expected {}",
        bucket.cadence().max_value(),
        cadence_max
    );

    // Verify draft stats
    let draft_max = expected["draft"]["max"].as_f64().expect("draft max expected");
    assert!(
        (bucket.draft().max_value() - draft_max).abs() < 1e-6,
        "draft max_value mismatch: got {}, expected {}",
        bucket.draft().max_value(),
        draft_max
    );

    // Verify primary rolling windows are populated
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
