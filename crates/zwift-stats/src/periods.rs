// SPDX-License-Identifier: AGPL-3.0-only

//! Constants for rolling-window peak periods, GC intervals, and domain thresholds.

pub const DEFAULT_POWER_PERIODS: &[f64] = &[5.0, 15.0, 60.0, 300.0, 1200.0, 3600.0];
pub const DEFAULT_LONG_PERIODS: &[f64] = &[60.0, 300.0, 1200.0, 3600.0];
pub const MIN_WEIGHTED_POWER_PERIOD: f64 = 300.0;

pub const ATHLETE_GC_TTL_SECS: f64 = 3600.0;
pub const GROUP_GC_TTL_SECS: f64 = 90.0;
/// GC cadence matching sauce4zwift's `_gcAthleteData`, driven by
/// `setInterval(..., 62768)` (62 768 ms = 62.768 s).
pub const GC_TICK_INTERVAL_SECS: f64 = 62.768;

// Group classification
pub const GROUP_GAP_THRESHOLD_S: f64 = 2.0;
pub const GROUP_GAP_THRESHOLD_NO_DRAFT_S: f64 = 0.8;
pub const JACCARD_MATCH_THRESHOLD: f64 = 0.5;
pub const NEARBY_MAX_GAP_S: f64 = 900.0; // 15 minutes, matches sauce4zwift stats.mjs

// Road position encoding
pub const ROAD_TIME_OFFSET: f64 = 5000.0;
pub const ROAD_TIME_SCALE: f64 = 1_000_000.0;
pub const BOUNDARY_ERROR_TERM: f64 = 0.01;

// Segment detection
pub const SEGMENT_START_WINDOW_PCT: f64 = 0.05;
pub const SEGMENT_START_WINDOW_METRES: f64 = 150.0;
pub const SEGMENT_LONG_MIN_METRES: f64 = 1000.0;
pub const SEGMENT_MID_MIN_METRES: f64 = 400.0;
pub const SEGMENT_COMPLETION_LONG: f64 = 0.90;
pub const SEGMENT_COMPLETION_MID: f64 = 0.60;
pub const SEGMENT_COMPLETION_SHORT: f64 = 0.25;

// W' balance default
pub const W_PRIME_DEFAULT: f64 = 20000.0;

/// Window size for the grade exponential weighted average.
/// Matches sauce4zwift's `Sauce.data.expWeightedAvg(8)` in `_preprocessState`.
pub const SMOOTH_GRADE_WINDOW: f64 = 8.0;
