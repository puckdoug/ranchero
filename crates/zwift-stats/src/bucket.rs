// SPDX-License-Identifier: AGPL-3.0-only

//! [`OneSecondBucket`] — buffers sub-second samples and emits one mean per `ideal_gap` boundary.

pub struct OneSecondBucket {
    // Placeholder; to be implemented in 13.16-I.
}

impl OneSecondBucket {
    pub fn new(_ideal_gap: f64, _round_to_int: bool) -> Self {
        // Placeholder; to be implemented in 13.16-I.
        OneSecondBucket {}
    }

    pub fn add(&mut self, _time: f64, _value: f64) -> Option<(f64, f64)> {
        // Placeholder; to be implemented in 13.16-I.
        None
    }

    pub fn flush(&mut self) -> Option<(f64, f64)> {
        // Placeholder; to be implemented in 13.16-I.
        None
    }
}
