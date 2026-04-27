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
    // 6 distinct latencies; the floor-indexed middle of the
    // sorted-by-latency list (sauce's `offsets[len / 2 | 0]`) is
    // index 3 of `[10, 11, 12, 13, 14, 15]` → latency 13.
    let samples = vec![
        s(13, 100),
        s(11, 100),
        s(14, 100),
        s(10, 100),
        s(15, 100),
        s(12, 100),
    ];
    match compute_offset(&samples) {
        SyncOutcome::Converged {
            median_latency_ms, ..
        } => {
            assert_eq!(median_latency_ms, 13, "median should be the floor-middle of sorted latencies");
        }
        SyncOutcome::NeedMore => panic!("expected Converged with all-tight samples"),
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
    //   samples = [(10, 5), (12, 7), (14, 9), (16, 11), (18, 13), (20, 15)]
    //   sorted by latency: same order
    //   mean_latency = (10+12+14+16+18+20)/6 = 15
    //   variance per sample: (5², 3², 1², 1², 3², 5²) = (25, 9, 1, 1, 9, 25)
    //   stddev = sqrt(70/6) ≈ 3.4156…
    //   median (floor 6/2 = 3): samples[3].latency = 16
    //   keep |latency - 16| < 3.4156 → keep latencies 14, 16, 18 (diffs 2, 0, 2)
    //                                  drop latencies 10, 12, 20 (diffs 6, 4, 4)
    //   valid offsets: 9, 11, 13 → mean = 11
    let samples = vec![s(10, 5), s(12, 7), s(14, 9), s(16, 11), s(18, 13), s(20, 15)];
    match compute_offset(&samples) {
        SyncOutcome::Converged {
            mean_offset_ms,
            median_latency_ms,
        } => {
            assert_eq!(median_latency_ms, 16, "floor-indexed median latency");
            assert_eq!(mean_offset_ms, 11, "mean of {{9, 11, 13}}");
        }
        SyncOutcome::NeedMore => panic!("expected Converged"),
    }
}
