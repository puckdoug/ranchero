// SPDX-License-Identifier: AGPL-3.0-only

//! [`RollingPower`] with inline Normalized Power (NP) and optional XP accumulators,
//! and the [`calc_tss`] Training Stress Score function.

use crate::{RollingAverage, RollingAverageOptions, Sample};

pub struct RollingPower {
    rolling: RollingAverage,
    qnpa_total: f64,
    qnpa_count: usize,
    qnpa_values: Vec<f64>,
    xpa_total: f64,
    xpa_count: usize,
    xpa_values: Vec<f64>,
    ideal_gap: Option<f64>,
}

impl RollingPower {
    pub fn new(period: Option<f64>, opts: RollingAverageOptions) -> Self {
        RollingPower {
            rolling: RollingAverage::new(period, opts),
            qnpa_total: 0.0,
            qnpa_count: 0,
            qnpa_values: Vec::new(),
            xpa_total: 0.0,
            xpa_count: 0,
            xpa_values: Vec::new(),
            ideal_gap: opts.ideal_gap,
        }
    }

    pub fn add(&mut self, ts: f64, value: Sample, active: Option<bool>) {
        self.rolling.add(ts, value, active);
        self.recompute_qnpa();
        self.recompute_xpa();
    }

    fn recompute_qnpa(&mut self) {
        let np_window = 30.0;
        let size = self.rolling.size();

        self.qnpa_total = 0.0;
        self.qnpa_count = 0;
        self.qnpa_values.clear();

        let times = self.rolling.times();
        let values = self.rolling.values();

        for i in 0..size {
            let current_time = times[i];
            let window_start = current_time - np_window;

            let mut sum = 0.0;
            let mut count = 0;
            for j in 0..=i {
                if times[j] >= window_start {
                    let val = values[j];
                    if crate::is_active_value(val, false) {
                        sum += val.as_f64();
                        count += 1;
                    }
                }
            }

            let contribution = if count > 0 {
                let avg = sum / count as f64;
                avg * avg * avg * avg
            } else {
                0.0
            };

            self.qnpa_values.push(contribution);
            self.qnpa_total += contribution;
            self.qnpa_count += 1;
        }
    }


    fn recompute_xpa(&mut self) {
        let size = self.rolling.size();
        let ideal_gap = self.ideal_gap.unwrap_or(1.0);
        let samples_per_window = (25.0 / ideal_gap).max(1.0) as usize;
        let decay_rate = 0.1;

        self.xpa_total = 0.0;
        self.xpa_count = 0;
        self.xpa_values.clear();

        let values = self.rolling.values();

        for i in 0..size {
            let window_start = if i >= samples_per_window { i - samples_per_window } else { 0 };

            let mut weighted_sum = 0.0;
            let mut weight_total = 0.0;

            for j in window_start..=i {
                let distance = (i - j) as f64;
                let weight = (-decay_rate * distance).exp();
                let val = values[j];
                if crate::is_active_value(val, false) {
                    weighted_sum += val.as_f64() * weight;
                    weight_total += weight;
                }
            }

            let contribution = if weight_total > 0.0 {
                weighted_sum / weight_total
            } else {
                0.0
            };

            self.xpa_values.push(contribution);
            self.xpa_total += contribution;
            self.xpa_count += 1;
        }
    }

    pub fn np(&self, force: bool) -> Option<f64> {
        let active_time = self.rolling.active();
        if !force && active_time < 300.0 {
            return None;
        }

        if self.qnpa_count == 0 {
            return None;
        }

        let mean = self.qnpa_total / self.qnpa_count as f64;
        Some(mean.powf(0.25))
    }

    pub fn xp(&self, force: bool) -> Option<f64> {
        let active_time = self.rolling.active();
        if !force && active_time < 300.0 {
            return None;
        }

        if self.xpa_count == 0 {
            return None;
        }

        let mean = self.xpa_total / self.xpa_count as f64;
        Some(mean)
    }

    pub fn size(&self) -> usize {
        self.rolling.size()
    }

    pub fn times(&self) -> &[f64] {
        self.rolling.times()
    }

    pub fn values(&self) -> &[crate::Sample] {
        self.rolling.values()
    }
}

pub fn calc_tss(np: f64, seconds: f64, ftp: f64) -> Option<f64> {
    if ftp <= 0.0 {
        return None;
    }

    let tss = (seconds * np * (np / ftp)) / (ftp * 3600.0) * 100.0;
    Some(tss)
}
