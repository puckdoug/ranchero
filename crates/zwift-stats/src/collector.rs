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

#[derive(Debug, Clone)]
pub struct NpPeakSnapshot {
    pub snap_value: f64,
    pub snap_time: f64,
}

#[derive(Debug)]
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
                        snap_value: np,
                        snap_time,
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

    pub fn max_value(&self) -> f64 {
        self.inner.max_value()
    }
}

#[derive(Debug)]
pub struct DataBucket {
    start: f64,
    coffee_time: f64,
    work_time: f64,
    follow_time: f64,
    solo_time: f64,
    work_kj: f64,
    follow_kj: f64,
    solo_kj: f64,
    power: PowerDataCollector,
    hr: DataCollector<RollingAverage>,
    speed: DataCollector<RollingAverage>,
    cadence: DataCollector<RollingAverage>,
    draft: DataCollector<crate::RollingPower>,
}

impl DataBucket {
    pub fn new(start: f64) -> Self {
        let power_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: false,
            round: true,
        };

        let hr_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: true,
            round: true,
        };

        let speed_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: true,
            round: false,
        };

        let cadence_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: true,
            round: true,
        };

        let draft_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: false,
            round: true,
        };

        DataBucket {
            start,
            coffee_time: 0.0,
            work_time: 0.0,
            follow_time: 0.0,
            solo_time: 0.0,
            work_kj: 0.0,
            follow_kj: 0.0,
            solo_kj: 0.0,
            power: PowerDataCollector::new(&[5.0, 15.0, 60.0, 300.0, 1200.0, 3600.0], power_opts),
            hr: DataCollector::new(&[60.0, 300.0, 1200.0, 3600.0], hr_opts),
            speed: DataCollector::new(&[60.0, 300.0, 1200.0, 3600.0], speed_opts),
            cadence: DataCollector::new(&[], cadence_opts),
            draft: DataCollector::new(&[60.0, 300.0, 1200.0, 3600.0], draft_opts),
        }
    }

    pub fn start(&self) -> f64 {
        self.start
    }

    pub fn coffee_time(&self) -> f64 {
        self.coffee_time
    }

    pub fn work_time(&self) -> f64 {
        self.work_time
    }

    pub fn follow_time(&self) -> f64 {
        self.follow_time
    }

    pub fn solo_time(&self) -> f64 {
        self.solo_time
    }

    pub fn work_kj(&self) -> f64 {
        self.work_kj
    }

    pub fn follow_kj(&self) -> f64 {
        self.follow_kj
    }

    pub fn solo_kj(&self) -> f64 {
        self.solo_kj
    }

    pub fn set_coffee_time(&mut self, value: f64) {
        self.coffee_time = value;
    }

    pub fn set_work_time(&mut self, value: f64) {
        self.work_time = value;
    }

    pub fn set_follow_time(&mut self, value: f64) {
        self.follow_time = value;
    }

    pub fn set_solo_time(&mut self, value: f64) {
        self.solo_time = value;
    }

    pub fn set_work_kj(&mut self, value: f64) {
        self.work_kj = value;
    }

    pub fn set_follow_kj(&mut self, value: f64) {
        self.follow_kj = value;
    }

    pub fn set_solo_kj(&mut self, value: f64) {
        self.solo_kj = value;
    }

    pub fn power(&self) -> &PowerDataCollector {
        &self.power
    }

    pub fn power_mut(&mut self) -> &mut PowerDataCollector {
        &mut self.power
    }

    pub fn hr(&self) -> &DataCollector<RollingAverage> {
        &self.hr
    }

    pub fn hr_mut(&mut self) -> &mut DataCollector<RollingAverage> {
        &mut self.hr
    }

    pub fn speed(&self) -> &DataCollector<RollingAverage> {
        &self.speed
    }

    pub fn speed_mut(&mut self) -> &mut DataCollector<RollingAverage> {
        &mut self.speed
    }

    pub fn cadence(&self) -> &DataCollector<RollingAverage> {
        &self.cadence
    }

    pub fn cadence_mut(&mut self) -> &mut DataCollector<RollingAverage> {
        &mut self.cadence
    }

    pub fn draft(&self) -> &DataCollector<crate::RollingPower> {
        &self.draft
    }

    pub fn draft_mut(&mut self) -> &mut DataCollector<crate::RollingPower> {
        &mut self.draft
    }

    pub fn ingest_power(&mut self, ts: f64, watts: f64) {
        self.power_mut().add(ts, watts);
    }

    pub fn ingest_hr(&mut self, ts: f64, bpm: f64) {
        self.hr_mut().add(ts, bpm);
    }

    pub fn ingest_speed(&mut self, ts: f64, kph: f64) {
        self.speed_mut().add(ts, kph);
    }

    pub fn ingest_cadence(&mut self, ts: f64, rpm: f64) {
        self.cadence_mut().add(ts, rpm);
    }

    pub fn ingest_draft(&mut self, ts: f64, value: f64) {
        self.draft_mut().add(ts, value);
    }

    pub fn clone_reset(&self) -> Self {
        DataBucket {
            start: self.start,
            coffee_time: 0.0,
            work_time: 0.0,
            follow_time: 0.0,
            solo_time: 0.0,
            work_kj: 0.0,
            follow_kj: 0.0,
            solo_kj: 0.0,
            power: self.power.clone_reset(),
            hr: self.hr.clone_reset(),
            speed: self.speed.clone_reset(),
            cadence: self.cadence.clone_reset(),
            draft: self.draft.clone_reset(),
        }
    }

    pub fn clone_continue(&self) -> Self {
        DataBucket {
            start: self.start,
            coffee_time: self.coffee_time,
            work_time: self.work_time,
            follow_time: self.follow_time,
            solo_time: self.solo_time,
            work_kj: self.work_kj,
            follow_kj: self.follow_kj,
            solo_kj: self.solo_kj,
            power: self.power.clone_continue(),
            hr: self.hr.clone_continue(),
            speed: self.speed.clone_continue(),
            cadence: self.cadence.clone_continue(),
            draft: self.draft.clone_continue(),
        }
    }
}
