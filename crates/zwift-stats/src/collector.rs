// SPDX-License-Identifier: AGPL-3.0-only

//! [`DataCollector`] and [`PowerDataCollector`] — orchestration of rolling windows
//! with per-period peak tracking.

use crate::{RollingAverage, RollingAverageOptions, RollingPower, Sample};

pub trait RollingWindow: Clone {
    fn new_with_period(period: Option<f64>, opts: RollingAverageOptions) -> Self;
    fn add(&mut self, ts: f64, value: Sample, active: Option<bool>);
    fn avg(&self, active: Option<bool>) -> Option<f64>;
    fn active(&self) -> f64;
    fn elapsed(&self) -> f64;
    fn full(&self, offt: usize) -> bool;
    fn last_time(&self) -> Option<f64>;
    fn reset(&mut self);
    fn size(&self) -> usize;
    fn value_at(&self, index: i32) -> Option<Sample>;
}

impl RollingWindow for RollingAverage {
    fn new_with_period(period: Option<f64>, opts: RollingAverageOptions) -> Self {
        RollingAverage::new(period, opts)
    }

    fn add(&mut self, ts: f64, value: Sample, active: Option<bool>) {
        RollingAverage::add(self, ts, value, active)
    }

    fn avg(&self, active: Option<bool>) -> Option<f64> {
        RollingAverage::avg(self, active)
    }

    fn active(&self) -> f64 {
        RollingAverage::active(self)
    }

    fn elapsed(&self) -> f64 {
        RollingAverage::elapsed(self)
    }

    fn full(&self, offt: usize) -> bool {
        RollingAverage::full(self, offt)
    }

    fn last_time(&self) -> Option<f64> {
        RollingAverage::last_time(self)
    }

    fn reset(&mut self) {
        RollingAverage::reset(self)
    }

    fn size(&self) -> usize {
        RollingAverage::size(self)
    }

    fn value_at(&self, index: i32) -> Option<Sample> {
        RollingAverage::value_at(self, index)
    }
}

// Self-describing snapshot of a peak window. `period` and `roll` make
// the snapshot independent of its enclosing periodized entry: when the
// snapshot is extracted (carried into a slice, surfaced to a UI), it
// still carries the window's identity and the rolling buffer that
// produced the peak. The `roll` is a deep clone (STEP 13's
// copy-on-clone), so the snapshot does not move when the source
// rolling is updated further.
#[derive(Debug, Clone)]
pub struct PeakSnapshot<R> {
    pub period: f64,
    pub snap_value: f64,
    pub snap_time: f64,
    pub roll: R,
}

#[derive(Debug, Clone)]
pub struct NpPeakSnapshot {
    pub period: f64,
    pub snap_value: f64,
    pub snap_time: f64,
    pub roll: RollingPower,
}

#[derive(Debug)]
pub struct PeriodizedEntry<R> {
    pub period: f64,
    pub roll: R,
    pub peak: Option<PeakSnapshot<R>>,
}

#[derive(Clone, Copy, Debug)]
pub struct DataCollectorOptions {
    pub ideal_gap: f64,
    pub max_gap: f64,
    pub active: bool,
    pub ignore_zeros: bool,
    pub round: bool,
}

// Default values match the `defOptions` constant in
// `sauce4zwift/src/stats.mjs:99` — `idealGap = 1`, `maxGap = 15`,
// `active = true`. The rolling primitives in STEP 13 use the same
// constants when no overrides are supplied.
impl Default for DataCollectorOptions {
    fn default() -> Self {
        DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: false,
            round: false,
        }
    }
}

#[derive(Debug)]
pub struct DataCollector<R: RollingWindow> {
    primary: R,
    periodized: Vec<PeriodizedEntry<R>>,
    max_value: f64,
    round: bool,
    buf_start: f64,
    buf_end: f64,
    buf_sum: f64,
    buf_len: u32,
    opts: RollingAverageOptions,
}

impl<R: RollingWindow> DataCollector<R> {
    pub fn new(periods: &[f64], opts: DataCollectorOptions) -> Self {
        let rolling_opts = RollingAverageOptions {
            ideal_gap: Some(opts.ideal_gap),
            max_gap: Some(opts.max_gap),
            active: opts.active,
            ignore_zeros: opts.ignore_zeros,
        };
        let primary = R::new_with_period(None, rolling_opts);
        let periodized = periods
            .iter()
            .map(|&period| PeriodizedEntry {
                period,
                roll: R::new_with_period(Some(period), rolling_opts),
                peak: None,
            })
            .collect();

        DataCollector {
            primary,
            periodized,
            max_value: 0.0,
            round: opts.round,
            buf_start: 0.0,
            buf_end: 0.0,
            buf_sum: 0.0,
            buf_len: 0,
            opts: rolling_opts,
        }
    }

    pub fn add(&mut self, ts: f64, value: f64) -> usize {
        let ideal_gap = self.opts.ideal_gap.unwrap_or(1.0);
        let mut flushed_count = 0;

        if self.buf_len > 0 && ts - self.buf_start >= ideal_gap {
            flushed_count = self.flush();
        }

        if self.buf_len == 0 {
            self.buf_start = ts;
        }

        self.buf_sum += value;
        self.buf_len += 1;
        self.buf_end = ts;

        flushed_count
    }

    pub fn flush(&mut self) -> usize {
        if self.buf_len == 0 {
            return 0;
        }

        let mean = self.buf_sum / self.buf_len as f64;
        let push_value = if self.round { mean.round() } else { mean };
        let ts_for_roll = self.buf_end;

        self.primary
            .add(ts_for_roll, Sample::Value(push_value), None);

        for entry in &mut self.periodized {
            entry.roll.add(ts_for_roll, Sample::Value(push_value), None);

            if entry.roll.full(0) && let Some(avg) = entry.roll.avg(None) {
                let should_update = match &entry.peak {
                    None => true,
                    Some(peak) => avg >= peak.snap_value,
                };

                if should_update && let Some(snap_time) = entry.roll.last_time() {
                    entry.peak = Some(PeakSnapshot {
                        period: entry.period,
                        snap_value: avg,
                        snap_time,
                        roll: entry.roll.clone(),
                    });
                }
            }
        }

        if push_value > self.max_value {
            self.max_value = push_value;
        }

        self.buf_sum = 0.0;
        self.buf_len = 0;
        self.buf_start = 0.0;
        self.buf_end = 0.0;
        1
    }

    pub fn primary(&self) -> &R {
        &self.primary
    }

    pub fn peaks(&self) -> Vec<Option<PeakSnapshot<R>>> {
        self.periodized.iter().map(|e| e.peak.clone()).collect()
    }

    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    pub fn clone_reset(&self) -> Self {
        let opts = DataCollectorOptions {
            ideal_gap: self.opts.ideal_gap.unwrap_or(1.0),
            max_gap: self.opts.max_gap.unwrap_or(15.0),
            active: self.opts.active,
            ignore_zeros: self.opts.ignore_zeros,
            round: self.round,
        };
        let periods: Vec<f64> = self.periodized.iter().map(|p| p.period).collect();
        DataCollector::new(&periods, opts)
    }

    pub fn clone_continue(&self) -> Self {
        let primary = self.primary.clone();
        let periodized = self
            .periodized
            .iter()
            .map(|entry| PeriodizedEntry {
                period: entry.period,
                roll: entry.roll.clone(),
                peak: entry.peak.clone(),
            })
            .collect();

        DataCollector {
            primary,
            periodized,
            max_value: self.max_value,
            round: self.round,
            buf_start: self.buf_start,
            buf_end: self.buf_end,
            buf_sum: self.buf_sum,
            buf_len: self.buf_len,
            opts: self.opts,
        }
    }

    pub fn periodized(&self) -> &[PeriodizedEntry<R>] {
        &self.periodized
    }

    /// Snapshot of the collector in the `getStatsV2` shape.
    ///
    /// `ts_offset_ms` is added to each peak's `snap_time` to produce `ts`
    /// (converting a world-time or session-relative timestamp to a server
    /// Unix timestamp). Pass `0.0` when no conversion is needed.
    pub fn stats(&self, ts_offset_ms: f64) -> SignalStats {
        let avg = self.primary.avg(None);
        let max = self.max_value;

        let peaks = self.periodized.iter().map(|entry| {
            entry.peak.as_ref().map(|p| PeakStat {
                period: p.period,
                avg:    p.snap_value,
                time:   p.snap_time,
                ts:     ts_offset_ms + p.snap_time * 1000.0,
            })
        }).collect();

        let smooth = self.periodized.iter()
            .filter(|e| e.period <= MAX_SMOOTH_PERIOD)
            .filter_map(|e| e.roll.avg(None).map(|avg| SmoothStat {
                period: e.period,
                avg,
            }))
            .collect();

        SignalStats { avg, max, peaks, smooth }
    }
}

/// Maximum period (seconds) included in the `smooth` array. Mirrors
/// `maxSmoothPeriod = 1200` from `stats.mjs:128`.
const MAX_SMOOTH_PERIOD: f64 = 1200.0;

/// One entry in the `peaks` array of a [`SignalStats`].  Mirrors the
/// peak objects produced by `getStatsSlow` / `getStatsV2` (`stats.mjs:196`).
#[derive(Debug, Clone)]
pub struct PeakStat {
    pub period: f64,
    pub avg:    f64,
    /// World-time (seconds) at which the peak was captured (`snap_time`).
    pub time:   f64,
    /// `ts_offset_ms + time * 1000`: local-time ms of the peak.
    pub ts:     f64,
}

/// One entry in the `smooth` array of a [`SignalStats`].
#[derive(Debug, Clone)]
pub struct SmoothStat {
    pub period: f64,
    pub avg:    f64,
}

/// Output of [`DataCollector::stats`].  Mirrors the object returned by
/// `getStatsV2` (`stats.mjs:196`).
#[derive(Debug)]
pub struct SignalStats {
    /// Overall session average from the unbounded primary roller.
    /// `None` if no data has been ingested.
    pub avg:    Option<f64>,
    pub max:    f64,
    pub peaks:  Vec<Option<PeakStat>>,
    pub smooth: Vec<SmoothStat>,
}

#[derive(Debug)]
pub struct NpPeriodizedEntry {
    pub period: f64,
    pub peak: Option<NpPeakSnapshot>,
}

#[derive(Debug)]
pub struct PowerDataCollector {
    inner: DataCollector<crate::RollingPower>,
    np_periodized: Vec<NpPeriodizedEntry>,
}

impl PowerDataCollector {
    pub fn new(periods: &[f64], opts: DataCollectorOptions) -> Self {
        let inner = DataCollector::new(periods, opts);
        let np_periodized = periods
            .iter()
            .map(|&period| NpPeriodizedEntry {
                period,
                peak: None,
            })
            .collect();

        PowerDataCollector {
            inner,
            np_periodized,
        }
    }

    pub fn add(&mut self, ts: f64, value: f64) -> usize {
        let flushed = self.inner.add(ts, value);
        if flushed > 0 {
            self.update_np_peaks();
        }
        flushed
    }

    fn update_np_peaks(&mut self) {
        let inner_periodized = self.inner.periodized();
        for (i, entry) in inner_periodized.iter().enumerate() {
            if entry.period >= 300.0 && entry.roll.full(0)
                && let Some(np) = entry.roll.np(false)
            {
                let should_update = match &self.np_periodized[i].peak {
                    None => true,
                    Some(peak) => np >= peak.snap_value,
                };

                if should_update && let Some(snap_time) = entry.roll.last_time() {
                    self.np_periodized[i].peak = Some(NpPeakSnapshot {
                        period: entry.period,
                        snap_value: np,
                        snap_time,
                        roll: entry.roll.clone(),
                    });
                }
            }
        }
    }

    pub fn np_peaks(&self) -> Vec<Option<NpPeakSnapshot>> {
        self.np_periodized.iter().map(|e| e.peak.clone()).collect()
    }

    pub fn clone_reset(&self) -> Self {
        let inner = self.inner.clone_reset();
        let np_periodized = self
            .np_periodized
            .iter()
            .map(|e| NpPeriodizedEntry {
                period: e.period,
                peak: None,
            })
            .collect();

        PowerDataCollector {
            inner,
            np_periodized,
        }
    }

    pub fn clone_continue(&self) -> Self {
        let inner = self.inner.clone_continue();
        let np_periodized = self
            .np_periodized
            .iter()
            .map(|e| NpPeriodizedEntry {
                period: e.period,
                peak: e.peak.clone(),
            })
            .collect();

        PowerDataCollector {
            inner,
            np_periodized,
        }
    }

    pub fn periodized(&self) -> &[PeriodizedEntry<crate::RollingPower>] {
        self.inner.periodized()
    }

    pub fn flush(&mut self) -> usize {
        let flushed = self.inner.flush();
        if flushed > 0 {
            self.update_np_peaks();
        }
        flushed
    }

    pub fn max_value(&self) -> f64 {
        self.inner.max_value()
    }

    /// Direct access to the unbounded primary `RollingPower`.
    pub fn primary(&self) -> &crate::RollingPower {
        self.inner.primary()
    }

    /// Signal stats (avg, max, peaks, smooth) for the raw power values,
    /// in the same shape as `DataCollector::stats`.
    pub fn stats(&self, ts_offset_ms: f64) -> SignalStats {
        self.inner.stats(ts_offset_ms)
    }

    /// Snapshot of the NP collector in the `getNPStatsV2` shape.
    ///
    /// Only periods ≥ 300 s are included, mirroring `_npPeriodizedOfft`
    /// (`stats.mjs:265`). `ts_offset_ms` is added to each `snap_time`.
    pub fn np_stats(&self, ts_offset_ms: f64) -> NpStats {
        let inner_periods = self.inner.periodized();

        let qualified: Vec<(usize, &NpPeriodizedEntry)> = self.np_periodized
            .iter()
            .enumerate()
            .filter(|(_, e)| e.period >= MIN_NP_PERIOD)
            .collect();

        let peaks = qualified.iter().map(|(_, entry)| {
            entry.peak.as_ref().map(|p| NpPeakStat {
                period: p.period,
                avg:    p.snap_value,
                max:    p.snap_value,
                time:   p.snap_time,
                ts:     ts_offset_ms + p.snap_time * 1000.0,
            })
        }).collect();

        let smooth = qualified.iter()
            .filter(|(_, e)| e.period <= MAX_SMOOTH_PERIOD)
            .filter_map(|(i, e)| {
                inner_periods[*i].roll.np(false).map(|avg| NpSmoothStat {
                    period: e.period,
                    avg,
                })
            })
            .collect();

        NpStats { peaks, smooth }
    }
}

/// Minimum period (seconds) included in NP stats. Mirrors
/// `_npPeriodizedOfft` / `minWeightedPowerPeriod = 300`
/// from `stats.mjs:265`.
const MIN_NP_PERIOD: f64 = 300.0;

/// One entry in the `peaks` array of an [`NpStats`].
#[derive(Debug, Clone)]
pub struct NpPeakStat {
    pub period: f64,
    /// Normalized Power at peak time.
    pub avg:    f64,
    /// Maximum NP seen for this period across the session.
    pub max:    f64,
    /// World-time (seconds) at which the NP peak was captured.
    pub time:   f64,
    /// `ts_offset_ms + time * 1000`: local-time ms of the peak.
    pub ts:     f64,
}

/// One entry in the `smooth` array of an [`NpStats`].
#[derive(Debug, Clone)]
pub struct NpSmoothStat {
    pub period: f64,
    pub avg:    f64,
}

/// Output of [`PowerDataCollector::np_stats`].
#[derive(Debug)]
pub struct NpStats {
    pub peaks:  Vec<Option<NpPeakStat>>,
    pub smooth: Vec<NpSmoothStat>,
}

