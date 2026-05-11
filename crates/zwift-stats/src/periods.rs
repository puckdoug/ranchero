// SPDX-License-Identifier: AGPL-3.0-only

//! Constants for rolling-window peak periods and GC intervals.

pub const DEFAULT_POWER_PERIODS: &[f64] = &[5.0, 15.0, 60.0, 300.0, 1200.0, 3600.0];
pub const DEFAULT_LONG_PERIODS: &[f64] = &[60.0, 300.0, 1200.0, 3600.0];
pub const MIN_WEIGHTED_POWER_PERIOD: f64 = 300.0;

pub const ATHLETE_GC_TTL_SECS: f64 = 3600.0;
pub const GROUP_GC_TTL_SECS: f64 = 90.0;
pub const GC_TICK_INTERVAL_SECS: f64 = 62.768;
