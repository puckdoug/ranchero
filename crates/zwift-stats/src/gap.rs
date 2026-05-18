// SPDX-License-Identifier: AGPL-3.0-only

use crate::{AthleteData, road_history::{compare_road_positions, RoadGeometry}};

pub fn apply_gap(ad: &mut AthleteData, watching: &AthleteData, env: &dyn RoadGeometry) {
    match compare_road_positions(&watching.road_history, &ad.road_history, env) {
        Some(cmp) => {
            let raw_gap = cmp.world_time / 1000.0;
            ad.gap = Some(if cmp.reversed { -raw_gap } else { raw_gap });
            ad.gap_distance = Some(if cmp.reversed { -cmp.distance } else { cmp.distance });
            ad.is_gap_est = false;
        }
        None => {
            ad.gap = None;
            ad.is_gap_est = true;
        }
    }
}
