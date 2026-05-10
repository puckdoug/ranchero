// SPDX-License-Identifier: AGPL-3.0-only

//! [`OneSecondBucket`] — buffers sub-second samples and emits one mean per second.

pub struct OneSecondBucket {
    #[allow(dead_code)]
    ideal_gap: f64,
    round_to_int: bool,
    current_second: i32,
    samples: Vec<f64>,
}

impl OneSecondBucket {
    pub fn new(ideal_gap: f64, round_to_int: bool) -> Self {
        OneSecondBucket {
            ideal_gap,
            round_to_int,
            current_second: -1,
            samples: Vec::new(),
        }
    }

    pub fn add(&mut self, time: f64, value: f64) -> Option<(f64, f64)> {
        let second = time.floor() as i32;

        if self.current_second == -1 {
            self.current_second = second;
            self.samples.push(value);
            None
        } else if second == self.current_second {
            self.samples.push(value);
            None
        } else {
            let mean = self.samples.iter().sum::<f64>() / self.samples.len() as f64;
            let count = self.samples.len() as f64;

            self.current_second = second;
            self.samples.clear();
            self.samples.push(value);

            Some((
                if self.round_to_int { mean.round() } else { mean },
                count,
            ))
        }
    }

    pub fn flush(&mut self) -> Option<(f64, f64)> {
        if self.samples.is_empty() {
            None
        } else {
            let mean = self.samples.iter().sum::<f64>() / self.samples.len() as f64;
            let count = self.samples.len() as f64;
            self.samples.clear();
            self.current_second = -1;
            Some((
                if self.round_to_int { mean.round() } else { mean },
                count,
            ))
        }
    }
}
