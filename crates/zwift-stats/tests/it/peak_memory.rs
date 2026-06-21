// SPDX-License-Identifier: AGPL-3.0-only
//! 19.9 — Peak-snapshot memory footprint measurement.
//!
//! Each periodized peak (`PeakSnapshot<R>`) stores a full clone of the rolling
//! window at the peak moment, matching the JS behaviour at `stats.mjs:185-189`.
//! This test measures the heap bytes attributable to those clones so the
//! decision on whether to keep the full clone or downgrade to
//! `(snap_value, snap_time)` is grounded in a measured number.
//!
//! **Method.** One `DataBucket` is fed 3 601 ticks at 1 Hz (filling every
//! period window up to 3 600 s).  The sizes of all peak-snapshot rolling
//! buffers are computed analytically from `roll.size()` and
//! `std::mem::size_of` — this is a lower bound on actual heap use because
//! `Vec` capacity may exceed `len` by up to 2× after growth.  The result is
//! extrapolated to 100 athletes (the published worst-case scenario).
//!
//! **Signals measured.**
//!
//! | Collector | Rolling type | Periods (s) | Peaks tracked |
//! |---|---|---|---|
//! | power peaks | `RollingPower` | 5, 15, 60, 300, 1200, 3600 | 6 |
//! | power NP peaks | `RollingPower` | 300, 1200, 3600 | 3 |
//! | hr peaks | `RollingAverage` | 60, 300, 1200, 3600 | 4 |
//! | speed peaks | `RollingAverage` | 60, 300, 1200, 3600 | 4 |
//! | cadence | — | none | 0 |
//! | draft peaks | `RollingPower` | 60, 300, 1200, 3600 | 4 |
//!
//! **Decision (recorded).** Decision (a) — keep the full clone.  The
//! measured per-athlete footprint is < 1 MB; even at 100 athletes the
//! extrapolated value is well under 100 MB (lower bound; actual heap use with
//! Vec overallocation is ≤ 2× that).  The full clone matches the JS reference
//! and is needed to reproduce the peak-power histogram on demand.  No change
//! required.
//!
//! See docs/planning/STEP-19-compatibility-tests.md, item 19.9.

use std::mem::size_of;
use zwift_stats::{DataBucket, RollingAverage, RollingPower, Sample};

// ---------------------------------------------------------------------------
// Analytical size helpers
// ---------------------------------------------------------------------------

/// Heap bytes used by the rolling buffers inside a `RollingAverage` snapshot.
///
/// Counts `times` (Vec<f64>) and `values` (Vec<Sample>) at their logical
/// length.  Actual heap use may be up to 2× due to Vec capacity growth.
fn rolling_average_snapshot_bytes(roll: &RollingAverage) -> usize {
    roll.size() * (size_of::<f64>() + size_of::<Sample>())
}

/// Heap bytes used by the rolling buffers inside a `RollingPower` snapshot.
///
/// Counts the inner `RollingAverage` (times + values) plus `qnpa_values` and
/// `xpa_values`, each of which has `roll.size()` entries after each push.
fn rolling_power_snapshot_bytes(roll: &RollingPower) -> usize {
    roll.size() * (size_of::<f64>() + size_of::<Sample>() + 2 * size_of::<f64>())
}

/// Sum of snapshot bytes for all peak snapshots in a fully-driven `DataBucket`.
fn bucket_snapshot_bytes(bucket: &DataBucket) -> usize {
    let mut total = 0usize;

    // Power peaks — DataCollector<RollingPower> inside PowerDataCollector.
    for entry in bucket.power().periodized() {
        if let Some(snap) = &entry.peak {
            total += rolling_power_snapshot_bytes(&snap.roll);
        }
    }

    // Power NP peaks — each NpPeakSnapshot carries a full RollingPower clone.
    for snap_opt in bucket.power().np_peaks() {
        if let Some(snap) = snap_opt {
            total += rolling_power_snapshot_bytes(&snap.roll);
        }
    }

    // HR peaks — RollingAverage.
    for entry in bucket.hr().periodized() {
        if let Some(snap) = &entry.peak {
            total += rolling_average_snapshot_bytes(&snap.roll);
        }
    }

    // Speed peaks — RollingAverage.
    for entry in bucket.speed().periodized() {
        if let Some(snap) = &entry.peak {
            total += rolling_average_snapshot_bytes(&snap.roll);
        }
    }

    // Cadence has no periodized windows — zero peaks.

    // Draft peaks — DataCollector<RollingPower>.
    for entry in bucket.draft().periodized() {
        if let Some(snap) = &entry.peak {
            total += rolling_power_snapshot_bytes(&snap.roll);
        }
    }

    total
}

// ---------------------------------------------------------------------------
// Measurement test
// ---------------------------------------------------------------------------

#[test]
#[ignore = "slow: memory measurement under multi-rider trace"]
fn peak_snapshot_memory_footprint() {
    const N_ATHLETES: usize = 100;
    const N_TICKS: usize = 3_601; // one tick past the 3600 s window to fill it

    let mut bucket = DataBucket::new(0.0);
    for i in 0..N_TICKS {
        let ts = i as f64;
        bucket.ingest_power(ts, 250.0);
        bucket.ingest_hr(ts, 120.0);
        bucket.ingest_speed(ts, 10.0);
        bucket.ingest_cadence(ts, 80.0);
        bucket.ingest_draft(ts, 5.0);
    }
    bucket.flush_all();

    let per_athlete = bucket_snapshot_bytes(&bucket);
    let extrapolated = per_athlete * N_ATHLETES;
    let extra_mb = extrapolated as f64 / (1024.0 * 1024.0);

    println!("size_of::<Sample>()   = {} bytes", size_of::<Sample>());
    println!("size_of::<f64>()      = {} bytes", size_of::<f64>());
    println!();
    println!("Per-athlete snapshot memory (lower bound): {} bytes ({:.1} KB)",
             per_athlete, per_athlete as f64 / 1024.0);
    println!("Extrapolated to {} athletes (lower bound): {} bytes ({:.1} MB)",
             N_ATHLETES, extrapolated, extra_mb);
    println!();
    println!("Actual heap use is <= 2× the lower bound due to Vec capacity growth.");
    println!("Decision: (a) keep full clone — footprint is acceptable.");

    // Guard against runaway growth if the signal set or period list is ever
    // expanded significantly.  500 MB extrapolated (before Vec overallocation)
    // would be the point at which a redesign is warranted.
    assert!(
        extrapolated < 500 * 1024 * 1024,
        "extrapolated snapshot memory {:.1} MB exceeds 500 MB ceiling",
        extra_mb,
    );
}
