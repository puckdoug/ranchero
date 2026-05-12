// SPDX-License-Identifier: AGPL-3.0-only

use crate::Sample;

pub struct WBalAccumulator {
    cp: Option<f64>,
    w_prime: Option<f64>,
    w_bal: f64,
    time_offset: Option<f64>,
}

impl WBalAccumulator {
    pub fn new() -> Self {
        WBalAccumulator {
            cp: None,
            w_prime: None,
            w_bal: 0.0,
            time_offset: None,
        }
    }

    pub fn configure(&mut self, cp: f64, w_prime: f64) {
        self.cp = Some(cp);
        self.w_prime = Some(w_prime);
        self.w_bal = w_prime;
        self.time_offset = None;
    }

    pub fn accumulate(&mut self, time: f64, sample: Sample) -> Option<f64> {
        let cp = self.cp?;
        let w_prime = self.w_prime?;

        if self.time_offset.is_none() {
            self.time_offset = Some(time);
        }

        let elapsed = time - self.time_offset.unwrap();
        self.time_offset = Some(time);

        let power = match sample {
            Sample::Value(v) => v,
            Sample::Pad(_) => return Some(self.w_bal),
            Sample::Break { pad } => {
                // Break refill: loop `pad` times, adding recovery per tick
                // early-exit when w_bal >= w_prime - 1e-6
                for _ in 0..pad {
                    let recovery_per_tick = cp * (w_prime - self.w_bal) / w_prime;
                    self.w_bal += recovery_per_tick;
                    if self.w_bal >= w_prime - 1e-6 {
                        self.w_bal = w_prime;
                        break;
                    }
                }
                return Some(self.w_bal);
            }
        };

        // Differential W' balance equation
        // Below CP: exponential recovery with scaling
        // Above CP: linear depletion
        if power < cp {
            // Recovery: scaled exponential term
            let delta = (cp - power) * elapsed * (w_prime - self.w_bal) / w_prime;
            self.w_bal += delta;
            if self.w_bal > w_prime {
                self.w_bal = w_prime;
            }
        } else {
            // Depletion: linear term, no scaling
            let delta = (cp - power) * elapsed;
            self.w_bal += delta;
        }

        Some(self.w_bal)
    }

    pub fn value(&self) -> Option<f64> {
        if self.cp.is_some() && self.w_prime.is_some() {
            Some(self.w_bal)
        } else {
            None
        }
    }

    pub fn reset(&mut self) {
        self.cp = None;
        self.w_prime = None;
        self.w_bal = 0.0;
        self.time_offset = None;
    }

    pub fn clone_reset(&self) -> Self {
        let w_prime = self.w_prime.unwrap_or(0.0);
        WBalAccumulator {
            cp: self.cp,
            w_prime: self.w_prime,
            w_bal: w_prime,
            time_offset: None,
        }
    }

    pub fn clone_continue(&self) -> Self {
        WBalAccumulator {
            cp: self.cp,
            w_prime: self.w_prime,
            w_bal: self.w_bal,
            time_offset: self.time_offset,
        }
    }
}

impl Default for WBalAccumulator {
    fn default() -> Self {
        Self::new()
    }
}
