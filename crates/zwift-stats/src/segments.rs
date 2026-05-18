// SPDX-License-Identifier: AGPL-3.0-only

use crate::{AthleteData, DataSlice, PlayerStateView};
use crate::periods::{
    ROAD_TIME_OFFSET, ROAD_TIME_SCALE,
    SEGMENT_START_WINDOW_PCT, SEGMENT_START_WINDOW_METRES,
    SEGMENT_LONG_MIN_METRES, SEGMENT_MID_MIN_METRES,
    SEGMENT_COMPLETION_LONG, SEGMENT_COMPLETION_MID, SEGMENT_COMPLETION_SHORT,
};

#[derive(Debug, Clone)]
pub struct Segment {
    pub id: u32,
    pub course_id: u32,
    pub road_id: u32,
    pub reverse: bool,
    pub road_start: f64,
    pub road_finish: f64,
    pub distance: f64,
}

pub trait SegmentLookup {
    fn road_segments(&self, course_id: u32, road_id: u32, reverse: bool) -> &[Segment];
    fn segment(&self, id: u32) -> Option<&Segment>;
}

pub fn active_segment_check(
    ad: &mut AthleteData,
    state: &dyn PlayerStateView,
    env: &dyn SegmentLookup,
    now: f64,
) {
    let rpct = (state.road_time() - ROAD_TIME_OFFSET) / ROAD_TIME_SCALE;

    // Collect segments to close: those whose road no longer matches, or that the rider exited.
    let to_close: Vec<u32> = ad.active_segments.keys()
        .filter(|&&seg_id| {
            if let Some(seg) = env.segment(seg_id) {
                if seg.road_id != state.road_id()
                    || seg.course_id != state.course_id()
                    || seg.reverse != state.reverse()
                {
                    return true;
                }
                let seg_progress = (rpct - seg.road_start) / (seg.road_finish - seg.road_start);
                seg_progress > 1.0
            } else {
                false
            }
        })
        .copied()
        .collect();

    for seg_id in to_close {
        stop_segment(ad, state, seg_id, env, now);
    }

    // Open segments that start on the current road and are within the opening window.
    let segments = env.road_segments(state.course_id(), state.road_id(), state.reverse());
    for seg in segments {
        if ad.active_segments.contains_key(&seg.id) {
            continue;
        }
        let seg_progress = (rpct - seg.road_start) / (seg.road_finish - seg.road_start);
        if seg_progress < 0.0 {
            continue;
        }
        let open_threshold = f64::max(SEGMENT_START_WINDOW_PCT, SEGMENT_START_WINDOW_METRES / seg.distance);
        if seg_progress < open_threshold {
            let mut slice = DataSlice::new_from(ad, now);
            slice.segment_id = Some(seg.id);
            slice.start_world_time = state.world_time();
            ad.active_segments.insert(seg.id, slice);
        }
    }
}

pub fn stop_segment(
    ad: &mut AthleteData,
    _state: &dyn PlayerStateView,
    segment_id: u32,
    env: &dyn SegmentLookup,
    now: f64,
) {
    let seg = match env.segment(segment_id) {
        Some(s) => s.clone(),
        None => return,
    };

    let mut slice = match ad.active_segments.remove(&segment_id) {
        Some(s) => s,
        None => {
            let mut s = DataSlice::new_from(ad, now);
            s.segment_id = Some(segment_id);
            s
        }
    };

    slice.close(now);

    let max_rpct = ad.road_history.max_rpct_for(seg.road_id, seg.course_id, seg.reverse);
    let incomplete = match max_rpct {
        None => true,
        Some(r) => {
            let completion = (r - seg.road_start) / (seg.road_finish - seg.road_start);
            completion < completion_threshold(seg.distance)
        }
    };
    slice.incomplete = incomplete;

    ad.segment_slices.push(slice);
}

fn completion_threshold(distance: f64) -> f64 {
    if distance > SEGMENT_LONG_MIN_METRES {
        SEGMENT_COMPLETION_LONG
    } else if distance > SEGMENT_MID_MIN_METRES {
        SEGMENT_COMPLETION_MID
    } else {
        SEGMENT_COMPLETION_SHORT
    }
}
