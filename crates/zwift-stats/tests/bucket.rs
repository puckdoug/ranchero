// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::OneSecondBucket;

#[test]
fn bucket_emits_mean_on_boundary() {
    // OneSecondBucket aggregates sub-second samples into per-second means.
    // Add samples at t=0.1 (100W), t=0.5 (150W), t=0.9 (200W) - all within second 0.
    // On flush, should emit mean of all samples in that second.
    // Mean = (100 + 150 + 200) / 3 = 150W

    let mut bucket = OneSecondBucket::new(1.0, false);
    bucket.add(0.1, 100.0);
    bucket.add(0.5, 150.0);
    bucket.add(0.9, 200.0);

    let result = bucket.flush();
    assert!(result.is_some(), "flush should emit mean at boundary");
    let (power, count) = result.unwrap();
    assert!((power - 150.0).abs() < 0.1, "mean power should be ~150W");
    assert!((count - 3.0).abs() < 0.01, "count should be 3 samples");
}

#[test]
fn bucket_round_to_int() {
    // Mean should be rounded to integer or kept precise, but bucket should track count.
    // Add samples that yield non-integer mean: t=0.25 (100W), t=0.75 (101W).
    // Mean = (100 + 101) / 2 = 100.5W
    // With round_to_int=true, should round to 100 or 101.

    let mut bucket = OneSecondBucket::new(1.0, true);
    bucket.add(0.25, 100.0);
    bucket.add(0.75, 101.0);

    let result = bucket.flush();
    assert!(result.is_some());
    let (power, count) = result.unwrap();
    assert!(power >= 100.0 && power <= 101.0, "mean should be rounded 100 or 101");
    assert!((count - 2.0).abs() < 0.01, "count should be 2");
}

#[test]
fn bucket_flush_drains_remainder() {
    // After flushing, bucket should be empty for the next second.
    // Add samples, flush, then add new samples to the next second.
    // Second flush should only contain new samples.

    let mut bucket = OneSecondBucket::new(1.0, false);
    bucket.add(0.5, 100.0);
    let _ = bucket.flush();

    bucket.add(1.25, 200.0);
    bucket.add(1.75, 300.0);
    let result = bucket.flush();

    assert!(result.is_some());
    let (power, count) = result.unwrap();
    assert!((power - 250.0).abs() < 0.1, "mean of new samples should be ~250W");
    assert!((count - 2.0).abs() < 0.01, "should have 2 new samples");
}
