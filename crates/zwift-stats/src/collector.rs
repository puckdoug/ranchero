// SPDX-License-Identifier: AGPL-3.0-only

//! [`DataCollector`] and [`PowerDataCollector`] — orchestration of rolling windows
//! with per-period peak tracking.

use crate::{RollingAverage, RollingAverageOptions, Sample};

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

#[derive(Debug, Clone)]
pub struct PeakSnapshot {
    pub snap_value: f64,
    pub snap_time: f64,
}

pub struct PeriodizedEntry<R> {
    pub period: f64,
    pub roll: R,
    pub peak: Option<PeakSnapshot>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DataCollectorOptions {
    pub ideal_gap: f64,
    pub max_gap: f64,
    pub active: bool,
    pub ignore_zeros: bool,
    pub round: bool,
}

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
                        snap_value: avg,
                        snap_time,
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

    pub fn peaks(&self) -> Vec<Option<PeakSnapshot>> {
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
}
