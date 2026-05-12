// SPDX-License-Identifier: AGPL-3.0-only

//! [`Sample`] enum and the [`is_active_value`] predicate that governs
//! whether a sample counts toward active time and average computations.
//! Also provides the soft-pad interner and the `ZERO` sentinel.

use std::sync::OnceLock;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sample {
    Value(f64),
    Pad(f64),
    Break { pad: u32 },
}

/// The cached ZERO sentinel (hard pad at 0.0), used for max-gap and explicit-active-false paths.
/// (Used in 13.6-I hard-gap branch.)
#[allow(dead_code)]
pub fn zero_pad() -> Sample {
    Sample::Pad(0.0)
}

/// Interner for soft pads. Returns a cached `Pad` instance keyed by `round(value * 10)`.
/// Multiple calls with close values (within 0.05) return the same signature.
#[allow(dead_code)]
fn pad_interner() -> &'static Mutex<HashMap<i32, Sample>> {
    static INTERNER: OnceLock<Mutex<HashMap<i32, Sample>>> = OnceLock::new();
    INTERNER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns a soft-pad instance keyed by the value (rounded to one decimal place).
/// (Used in 13.5-I soft-pad gap-fill and elsewhere.)
#[allow(dead_code)]
pub fn soft_pad(value: f64) -> Sample {
    let sig = (value * 10.0).round() as i32;
    let mut cache = pad_interner().lock().unwrap();
    *cache.entry(sig).or_insert(Sample::Pad(sig as f64 / 10.0))
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

pub fn is_active_value(s: Sample, ignore_zeros: bool) -> bool {
    match s {
        Sample::Value(v) => {
            // Non-zero values are always active; zero values are active only if ignore_zeros is false.
            (v != 0.0) || !ignore_zeros
        }
        Sample::Pad(_) | Sample::Break { .. } => false,
    }
}
