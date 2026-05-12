// SPDX-License-Identifier: AGPL-3.0-only

use crate::{AthleteData, PlayerStateView, DataSlice};

#[derive(Debug, Clone, Copy)]
pub struct AutoLapConfig {
    pub distance_interval: Option<f64>,
    pub time_interval: Option<f64>,
}

pub fn start_athlete_lap(ad: &mut AthleteData, now: f64) -> u64 {
    if let Some(last) = ad.lap_slices.last_mut() {
        last.close(now);
    }
    let slice = DataSlice::new_from(ad, now);
    let id = slice.id;
    ad.lap_slices.push(slice);
    id
}

pub fn auto_lap_check(
    ad: &mut AthleteData,
    state: &dyn PlayerStateView,
    cfg: &AutoLapConfig,
    now: f64,
) -> bool {
    let mark_value = if cfg.distance_interval.is_some() {
        state.event_distance()
    } else {
        state.time()
    };

    match ad.auto_lap_mark {
        None => {
            ad.auto_lap_mark = Some(mark_value);
            false
        }
        Some(mark) => {
            let interval = cfg.distance_interval.or(cfg.time_interval).unwrap_or(f64::MAX);
            if mark_value - mark >= interval {
                ad.auto_lap_mark = Some(mark_value);
                start_athlete_lap(ad, now);
                true
            } else {
                false
            }
        }
    }
}
