// SPDX-License-Identifier: AGPL-3.0-only

//! [`Sample`] enum and the [`is_active_value`] predicate that governs
//! whether a sample counts toward active time and average computations.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sample {
    Value(f64),
    Pad(f64),
    Break { pad: f64 },
}

impl Sample {
    pub fn as_f64(self) -> f64 {
        match self {
            Sample::Value(v) | Sample::Pad(v) => v,
            Sample::Break { .. } => 0.0,
        }
    }

    pub fn is_pad_or_break(self) -> bool {
        matches!(self, Sample::Pad(_) | Sample::Break { .. })
    }
}

pub fn is_active_value(_s: Sample, _ignore_zeros: bool) -> bool {
    // Placeholder; to be implemented in 13.2-I.
    false
}
