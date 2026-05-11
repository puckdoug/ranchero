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

#[derive(Clone)]
pub struct RollingAverage {
    #[allow(dead_code)]
    period: Option<f64>,      // Used in 13.8 (period eviction).
    ideal_gap: Option<f64>,
    #[allow(dead_code)]
    max_gap: Option<f64>,     // Used in 13.6 (hard-gap zero-pad branch).
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

    pub fn add(&mut self, ts: f64, value: Sample, active: Option<bool>) {
        if self.length > 0 {
            let prev_ts = self.times[self.length - 1];
            let gap = ts - prev_ts;

            // Hard-gap zero-pad: triggered by max_gap exceeded OR explicit active=false.
            let trigger_hard_gap = (active.is_none() && self.max_gap.map_or(false, |mg| gap > mg))
                || active == Some(false);

            if trigger_hard_gap {
                // Determine ideal_gap for hard-gap padding: use stored ideal_gap, else min(1, gap/2).
                let hard_ideal_gap = self.ideal_gap.unwrap_or(1.0);
                let break_gap = 3600.0;

                if gap > break_gap {
                    // Catastrophic gap (Garmin glitch): split with Break sentinel.
                    let book_end_time = (break_gap / 2.0).floor() - hard_ideal_gap;

                    // Leading zero pads.
                    let mut i = hard_ideal_gap;
                    while i < book_end_time {
                        self.add_internal(prev_ts + i, crate::sample::zero_pad());
                        i += hard_ideal_gap;
                    }

                    // Break sentinel.
                    let break_pad = gap - (book_end_time * 2.0);
                    self.add_internal(prev_ts + book_end_time, Sample::Break { pad: break_pad });

                    // Trailing zero pads.
                    i = gap - book_end_time;
                    while i < gap {
                        self.add_internal(prev_ts + i, crate::sample::zero_pad());
                        i += hard_ideal_gap;
                    }
                } else {
                    // Normal hard-gap: just zero pads.
                    let mut i = hard_ideal_gap;
                    while i < gap {
                        self.add_internal(prev_ts + i, crate::sample::zero_pad());
                        i += hard_ideal_gap;
                    }
                }
            } else if let Some(ideal_gap) = self.ideal_gap {
                // Soft-pad gap fill: gap > ideal_gap * 1.61803 (golden ratio).
                let pad_threshold = ideal_gap * 1.61803;
                if gap > pad_threshold {
                    let pad_value = crate::sample::soft_pad(value.as_f64());
                    let mut i = ideal_gap;
                    while i < gap {
                        self.add_internal(prev_ts + i, pad_value);
                        i += ideal_gap;
                    }
                }
            }
        }

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
            if let Some(period) = self.period {
                let current_ts = self.times[i];
                while self.offt < self.length && current_ts - self.times[self.offt] > period {
                    self.shift();
                }
            }
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

    fn shift(&mut self) {
        if self.offt < self.length {
            self.process_shift(self.offt);
            self.offt += 1;
        }
    }

    fn process_shift(&mut self, i: usize) {
        let value = self.values[i];
        if crate::is_active_value(value, self.ignore_zeros) {
            let gap = if i + 1 < self.length {
                self.times[i + 1] - self.times[i]
            } else {
                0.0
            };
            self.active_acc -= gap;
            self.values_acc -= value.as_f64() * gap;
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

    pub fn full(&self, offt: usize) -> bool {
        if self.period.is_none() {
            return false;
        }
        let period = self.period.unwrap();
        if self.length - self.offt <= offt {
            return false;
        }
        let elapsed = self.times[self.length - 1] - self.times[self.offt + offt];
        elapsed >= period
    }

    pub fn last_time(&self) -> Option<f64> {
        self.time_at(-1)
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

    pub fn slice(&self, start: usize, end: usize) -> RollingAverage {
        let mut sliced = self.clone();
        let actual_end = std::cmp::min(end, sliced.size());
        let actual_start = std::cmp::min(start, actual_end);
        sliced.offt += actual_start;
        sliced.length = sliced.offt + (actual_end - actual_start);
        sliced
    }

    pub fn pop(&mut self) -> Option<(f64, Sample)> {
        if self.length > self.offt {
            self.length -= 1;
            let ts = self.times[self.length];
            let value = self.values[self.length];
            self.process_shift(self.length);
            Some((ts, value))
        } else {
            None
        }
    }

    pub fn time_at(&self, index: i32) -> Option<f64> {
        let size = self.size() as i32;
        let actual_index = if index < 0 {
            (size + index) as usize
        } else {
            index as usize
        };
        if actual_index < self.size() {
            Some(self.times[self.offt + actual_index])
        } else {
            None
        }
    }

    pub fn value_at(&self, index: i32) -> Option<Sample> {
        let size = self.size() as i32;
        let actual_index = if index < 0 {
            (size + index) as usize
        } else {
            index as usize
        };
        if actual_index < self.size() {
            Some(self.values[self.offt + actual_index])
        } else {
            None
        }
    }

    pub fn times(&self) -> &[f64] {
        &self.times[self.offt..self.length]
    }

    pub fn values(&self) -> &[Sample] {
        &self.values[self.offt..self.length]
    }

    pub fn entries(&self) -> impl Iterator<Item = (f64, Sample)> + '_ {
        (self.offt..self.length).map(move |i| (self.times[i], self.values[i]))
    }

    pub fn import_data(&mut self, data: &[(f64, Sample)]) {
        for (ts, val) in data {
            self.add(*ts, *val, None);
        }
    }

    pub fn import_reduce(&mut self, data: &[(f64, Sample)], _: Option<f64>) -> (f64, std::ops::Range<f64>) {
        let mut peak_avg = 0.0;
        let mut peak_start = 0.0;
        let mut peak_end = 0.0;

        for (ts, val) in data {
            self.add(*ts, *val, None);
            if let Some(avg) = self.avg(Some(false)) {
                if avg > peak_avg {
                    peak_avg = avg;
                    peak_start = self.times[self.offt];
                    peak_end = *ts;
                }
            }
        }

        (peak_avg, peak_start..peak_end)
    }

    pub fn reset(&mut self) {
        self.times.clear();
        self.values.clear();
        self.offt = 0;
        self.length = 0;
        self.active_acc = 0.0;
        self.values_acc = 0.0;
    }
}
