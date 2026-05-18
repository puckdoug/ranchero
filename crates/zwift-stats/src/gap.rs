// SPDX-License-Identifier: AGPL-3.0-only

use crate::{AthleteData, road_history::{compare_road_positions, RoadGeometry}, ExpWeightedAvg};

pub fn apply_gap(ad: &mut AthleteData, watching: &AthleteData, env: &dyn RoadGeometry) {
    match compare_road_positions(&watching.road_history, &ad.road_history, env) {
        Some(cmp) => {
            let raw_gap = cmp.world_time / 1000.0;
            ad.gap = Some(if cmp.reversed { -raw_gap } else { raw_gap });
            ad.gap_distance = Some(if cmp.reversed { -cmp.distance } else { cmp.distance });
            ad.is_gap_est = false;
        }
        None => {
            let watching_speed = watching.most_recent_state.as_ref().map(|s| s.speed).unwrap_or(0.0);
            let speed_sample = watching_speed.max(10.0);
            if ad.gap_speed_ema.is_none() {
                ad.gap_speed_ema = Some(ExpWeightedAvg::new(10.0, speed_sample));
            }
            let smoothed = ad.gap_speed_ema.as_mut().unwrap().update(speed_sample);
            ad.gap = ad.gap_distance.map(|d| d / smoothed);
            ad.is_gap_est = true;
        }
    }
}
