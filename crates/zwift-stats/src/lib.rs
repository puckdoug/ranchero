// SPDX-License-Identifier: AGPL-3.0-only

//! Zwift statistics engine: rolling time-indexed windows with gap-fill semantics,
//! Normalized Power, TSS, W' balance, and zone tracking.
//!
//! Core primitives:
//! - [`RollingAverage`] — time-indexed ring with cumulative-active-time / value sums for O(1) averages.
//! - [`RollingPower`] — extends `RollingAverage` with inline Normalized Power (30 s window) and optional XP.
//! - [`Sample`] — telemetry value with `Pad` (filler) and `Break` (catastrophic-gap sentinel) variants.
//! - [`OneSecondBucket`] — aggregates sub-second samples into per-second means before rolling pushes.
//!
//! Consumers are expected to be per-athlete [`DataCollector`](https://docs.google.com/placeholder)
//! instances (STEP 14) that own a primary `RollingPower` plus clones per peak period.

mod sample;
mod rolling;
mod power;
mod helpers;
mod bucket;
pub mod periods;
pub mod collector;
pub mod data_bucket;
pub mod athlete;
pub mod zones;
pub mod wbal;
pub mod slice;
pub mod streams;
pub mod road_history;
pub mod groups;
pub mod laps;
pub mod segments;
pub mod events;

pub use sample::{Sample, is_active_value};
pub use rolling::{RollingAverage, RollingAverageOptions};
pub use power::{RollingPower, calc_tss};
pub use helpers::{recommended_time_gaps, corrected_rolling_average, corrected_rolling_power, peak_average, peak_np, ExpWeightedAvg, exp_weighted_avg};
pub use bucket::OneSecondBucket;
pub use collector::{DataCollector, PowerDataCollector, PeakSnapshot, NpPeakSnapshot, DataCollectorOptions, RollingWindow};
pub use data_bucket::DataBucket;
pub use athlete::{AthleteData, MostRecentState, AthleteRegistry, GroupMeta, GcReport};
pub use periods::{ATHLETE_GC_TTL_SECS, GROUP_GC_TTL_SECS, GC_TICK_INTERVAL_SECS};
pub use wbal::WBalAccumulator;
