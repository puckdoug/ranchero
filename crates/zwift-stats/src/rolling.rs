// SPDX-License-Identifier: AGPL-3.0-only

//! [`RollingAverage`] — time-indexed ring with gap-fill semantics.

use crate::Sample;

#[derive(Clone, Copy, Debug, Default)]
pub struct RollingAverageOptions {
    pub ideal_gap: Option<f64>,
    pub max_gap: Option<f64>,
    pub active: bool,
    pub ignore_zeros: bool,
}

pub struct RollingAverage {
    period: Option<f64>,
    ideal_gap: Option<f64>,
    max_gap: Option<f64>,
    active: bool,
    ignore_zeros: bool,

    times: Vec<f64>,
    values: Vec<Sample>,
    offt: usize,    // Offset (first live index after eviction).
    length: usize,  // Total length (inclusive of evicted prefix).

    active_acc: f64,      // Cumulative active time.
    values_acc: f64,      // Cumulative value * gap (weighted sum).
}

impl RollingAverage {
    pub fn new(period: Option<f64>, opts: RollingAverageOptions) -> Self {
        RollingAverage {
            period,
            ideal_gap: opts.ideal_gap,
            max_gap: opts.max_gap,
            active: opts.active,
            ignore_zeros: opts.ignore_zeros,
            times: Vec::new(),
            values: Vec::new(),
            offt: 0,
            length: 0,
            active_acc: 0.0,
            values_acc: 0.0,
        }
    }

    pub fn add(&mut self, ts: f64, value: Sample, _active: Option<bool>) {
        // Placeholder: no-gap path only.
        self.add_internal(ts, value);
    }

    fn add_internal(&mut self, ts: f64, value: Sample) {
        self.times.push(ts);
        self.values.push(value);
        self.resize(1);
    }

    fn resize(&mut self, size: usize) {
        let target_length = self.length + size;
        if target_length > self.values.len() {
            panic!("resize underflow");
        }
        for i in self.length..target_length {
            self.process_add(i);
            self.length += 1;
            // Period eviction would go here (13.8).
        }
    }

    fn process_add(&mut self, i: usize) {
        let value = self.values[i];
        if crate::is_active_value(value, self.ignore_zeros) {
            let gap = if i > 0 {
                self.times[i] - self.times[i - 1]
            } else {
                0.0
            };
            self.active_acc += gap;
            self.values_acc += value.as_f64() * gap;
        }
    }

    pub fn elapsed(&self) -> f64 {
        let len = self.length;
        let offt = self.offt;
        if len - offt == 0 {
            0.0
        } else {
            self.times[len - 1] - self.times[offt]
        }
    }

    pub fn active(&self) -> f64 {
        self.active_acc
    }

    pub fn avg(&self, active: Option<bool>) -> Option<f64> {
        let use_active = active.unwrap_or(self.active);
        let denominator = if use_active {
            self.active()
        } else {
            self.elapsed()
        };
        if denominator > 0.0 {
            Some(self.values_acc / denominator)
        } else {
            None
        }
    }

    pub fn size(&self) -> usize {
        self.length - self.offt
    }
}
