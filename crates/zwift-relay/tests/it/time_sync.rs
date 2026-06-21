// SPDX-License-Identifier: AGPL-3.0-only
//
// SNTP-style time-sync filter (`zwift_relay::udp::sync::compute_offset`).
// Pure-math tests; no transport, no clock. Mirrors the body of
// `zwift.mjs:1359-1373`.

use zwift_relay::udp::sync::{Sample, SyncOutcome, compute_offset};

fn s(latency_ms: i64, offset_ms: i64) -> Sample {
    Sample {
        latency_ms,
        offset_ms,
    }
}

#[test]
fn sync_returns_need_more_below_threshold() {
    // Sauce attempts the filter only when the sample count is
    // **strictly greater than** `MIN_SYNC_SAMPLES` (default 5).
    // 5 tight samples are not enough; 6 are.
    let five = vec![s(10, 100), s(11, 100), s(12, 100), s(13, 100), s(14, 100)];
    assert_eq!(compute_offset(&five), SyncOutcome::NeedMore);
}

#[test]
fn sync_picks_median_by_latency() {
    // 7 samples, 5 of them at the median latency, 2 outliers
    // (`zwift.mjs:1361, 1365`). After sorting by latency the
    // floor-indexed middle (`offsets[7 / 2] = offsets[3]`) lands on
    // a `13`. The 5 valid samples after filtering carry the
    // assertion.
    let samples = vec![
        s(10, 100),
        s(13, 100),
        s(13, 100),
        s(13, 100),
        s(13, 100),
        s(13, 100),
        s(16, 100),
    ];
    match compute_offset(&samples) {
        SyncOutcome::Converged {
            median_latency_ms, ..
        } => {
            assert_eq!(
                median_latency_ms, 13,
                "median should be the floor-middle of sorted latencies",
            );
        }
        SyncOutcome::NeedMore => panic!("expected Converged with 5 valid samples after filter"),
    }
}

#[test]
fn sync_filters_outlier_outside_one_stddev() {
    // 5 tight samples (latency ~10 ms, offset +5) plus one absurd
    // (latency 500 ms, offset +500). The absurd sample falls well
    // outside one stddev of the mean latency, so it's filtered out
    // before averaging offsets. Expected: mean_offset ≈ +5, NOT
    // pulled toward +500.
    let samples = vec![
        s(10, 5),
        s(11, 5),
        s(10, 5),
        s(9, 5),
        s(10, 5),
        s(500, 500),
    ];
    match compute_offset(&samples) {
        SyncOutcome::Converged { mean_offset_ms, .. } => {
            assert_eq!(
                mean_offset_ms, 5,
                "outlier should be filtered; mean_offset stays at +5",
            );
        }
        SyncOutcome::NeedMore => {
            panic!("expected Converged with 5 tight + 1 outlier (≥ 5 valid after filter)");
        }
    }
}

#[test]
fn sync_returns_need_more_when_too_few_valid_after_filter() {
    // If outlier filtering leaves ≤ 4 samples, sauce's `>` 4
    // threshold fails and the loop keeps collecting.
    // Construct: 1 tight + 5 wild (varied latencies). The 5 wild
    // ones are widely spread; the 1 tight sample is the outlier
    // when computed against the wild mean. After filtering, 0 or
    // few survive — definitely ≤ 4.
    let samples = vec![s(10, 1), s(100, 100), s(200, 200), s(300, 300), s(400, 400), s(500, 500)];
    assert_eq!(compute_offset(&samples), SyncOutcome::NeedMore);
}

#[test]
fn sync_known_vector() {
    // Hand-computed against the sauce algorithm:
    //   samples (latency, offset):
    //     (10, 5), (12, 7), (14, 9), (14, 11), (14, 13), (16, 15), (18, 17)
    //   sorted by latency: same order
    //   n = 7
    //   mean_latency = (10+12+14+14+14+16+18) / 7 = 14
    //   variance per sample: (16, 4, 0, 0, 0, 4, 16)
    //   stddev = sqrt(40 / 7) ≈ 2.391
    //   median = sorted[7/2] = sorted[3] = latency 14
    //   keep |latency - 14| < 2.391 → drop {10, 18} (diff 4); keep {12,14,14,14,16}
    //   valid offsets: 7, 9, 11, 13, 15 → mean = 11
    let samples = vec![
        s(10, 5),
        s(12, 7),
        s(14, 9),
        s(14, 11),
        s(14, 13),
        s(16, 15),
        s(18, 17),
    ];
    match compute_offset(&samples) {
        SyncOutcome::Converged {
            mean_offset_ms,
            median_latency_ms,
        } => {
            assert_eq!(median_latency_ms, 14, "floor-indexed median latency");
            assert_eq!(mean_offset_ms, 11, "mean of {{7, 9, 11, 13, 15}}");
        }
        SyncOutcome::NeedMore => panic!("expected Converged"),
    }
}
