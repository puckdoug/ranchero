// SPDX-License-Identifier: AGPL-3.0-only

use crate::{AthleteData, AthleteRegistry, road_history::{compare_road_positions, RoadGeometry}, ExpWeightedAvg};

/// Applies gap to every athlete in `registry` relative to the athlete with `watching_id`.
/// Athletes with no road history shared with the watcher keep whatever EMA-estimated gap
/// they already have (same fallback path as `apply_gap`).
pub fn apply_gaps_for_registry(
    registry: &mut AthleteRegistry,
    watching_id: u32,
    env: &dyn RoadGeometry,
) {
    let ids: Vec<u32> = registry.iter()
        .filter(|(id, _)| **id != watching_id)
        .map(|(id, _)| *id)
        .collect();
    // Capture as raw pointer so the immutable borrow ends here.
    // SAFETY: every id in `ids` differs from `watching_id`, so `get_mut` never targets the
    // same slot.  The registry is not resized (no inserts/removes) during this loop.
    let watching: *const AthleteData = match registry.get(watching_id) {
        Some(w) => w,
        None => return,
    };
    for id in ids {
        if let Some(ad) = registry.get_mut(id) {
            apply_gap(ad, unsafe { &*watching }, env);
        }
    }
}

/// Like `apply_gap` but falls back to chaining off `adjacent` when the direct road comparison
/// with `watching` fails.  The result is always marked estimated (`is_gap_est = true`).
pub fn apply_gap_chained(
    ad: &mut AthleteData,
    watching: &AthleteData,
    adjacent: Option<&AthleteData>,
    env: &dyn RoadGeometry,
) {
    apply_gap(ad, watching, env);
    if ad.gap.is_some() && !ad.is_gap_est {
        return;
    }
    let Some(adj) = adjacent else { return };
    let Some(adj_gap) = adj.gap else { return };
    // Try to measure the delta between adjacent and ad precisely.
    if let Some(cmp) = compare_road_positions(&adj.road_history, &ad.road_history, env) {
        let delta = cmp.world_time / 1000.0;
        let delta_signed = if cmp.reversed { -delta } else { delta };
        ad.gap = Some(adj_gap + delta_signed);
        ad.gap_distance = adj.gap_distance.map(|d| {
            let dist_signed = if cmp.reversed { -cmp.distance } else { cmp.distance };
            d + dist_signed
        });
    } else {
        // No road link to adjacent either; use adjacent's gap as a lower bound.
        ad.gap = Some(adj_gap);
        ad.gap_distance = adj.gap_distance;
    }
    ad.is_gap_est = true;
}

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
