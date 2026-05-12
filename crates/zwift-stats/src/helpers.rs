// SPDX-License-Identifier: AGPL-3.0-only

//! Free-function helpers: [`recommended_time_gaps`], [`corrected_rolling_average`],
//! [`peak_average`], and [`peak_np`].

use crate::{RollingAverage, RollingAverageOptions, Sample};
use std::collections::HashMap;

pub fn recommended_time_gaps(gaps: &[f64]) -> (f64, f64) {
    if gaps.is_empty() {
        return (1.0, 1.0);
    }

    // Find mode (most frequent gap, rounded to nearest 0.1).
    let mut frequency: HashMap<i32, usize> = HashMap::new();
    for gap in gaps {
        let rounded = (gap * 10.0).round() as i32;
        *frequency.entry(rounded).or_insert(0) += 1;
    }

    let mode_rounded = frequency
        .iter()
        .max_by_key(|&(_, count)| count)
        .map(|(&key, _)| key)
        .unwrap_or(10) as f64
        / 10.0;

    // Find max.
    let max = gaps.iter().copied().fold(0.0, f64::max);

    (mode_rounded, max)
}

pub fn corrected_rolling_average(values: &[f64], min_length: f64) -> Option<()> {
    if values.len() as f64 >= min_length {
        Some(())
    } else {
        None
    }
}

pub fn corrected_rolling_power(values: &[f64], min_length: f64) -> Option<()> {
    corrected_rolling_average(values, min_length)
}

pub fn peak_average(period: f64, times: &[f64], values: &[f64]) -> Option<f64> {
    if times.is_empty() || values.is_empty() || times.len() != values.len() {
        return None;
    }

    let opts = RollingAverageOptions::default();
    let mut roll = RollingAverage::new(Some(period), opts);

    let mut peak_avg: f64 = 0.0;
    for (ts, val) in times.iter().zip(values.iter()) {
        roll.add(*ts, Sample::Value(*val), None);
        if let Some(avg) = roll.avg(Some(false)) {
            peak_avg = peak_avg.max(avg);
        }
    }

    if peak_avg > 0.0 {
        Some(peak_avg)
    } else {
        None
    }
}

pub fn peak_np(_: f64, _: &[f64], _: &[f64]) -> Option<f64> {
    // Placeholder; to be implemented in 13.12-I.
    None
}

#[derive(Debug, Clone)]
pub struct ExpWeightedAvg {
    size: f64,
    avg: f64,
    c_next: f64,
    c_prev: f64,
}

impl ExpWeightedAvg {
    pub fn new(size: f64, seed: f64) -> Self {
        let c_next = 1.0 - (-1.0 / size).exp();
        let c_prev = 1.0 - c_next;

        ExpWeightedAvg {
            size,
            avg: seed,
            c_next,
            c_prev,
        }
    }

    pub fn update(&mut self, value: f64) -> f64 {
        self.avg = self.avg * self.c_prev + value * self.c_next;
        self.avg
    }

    pub fn get(&self) -> f64 {
        self.avg
    }

    pub fn size(&self) -> f64 {
        self.size
    }
}

pub fn exp_weighted_avg(size: f64, seed: f64) -> ExpWeightedAvg {
    ExpWeightedAvg::new(size, seed)
}
