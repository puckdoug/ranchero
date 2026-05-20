// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use crate::{AthleteData, DataSlice, PlayerStateView, start_athlete_lap};
use crate::athlete::EventSubgroup;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EventBehavior {
    pub auto_reset: bool,
    pub auto_lap: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventStateOutcome {
    Idle,
    Started { slice_id: u64 },
    StartPending,
    Ended { slice_id: u64 },
}

pub fn apply_event_state(
    ad: &mut AthleteData,
    state: &dyn PlayerStateView,
    self_athlete_id: u32,
    sg_lookup: &HashMap<u32, EventSubgroup>,
    behavior: EventBehavior,
    now: f64,
    wall_clock_ms: u64,
) -> EventStateOutcome {
    // Auto-end the active event if its distance or time limit is exceeded.
    if let Some(sg) = &ad.event_subgroup {
        let distance_exceeded = sg.end_distance > 0.0 && state.event_distance() >= sg.end_distance;
        let time_exceeded = sg.end_ts > 0 && wall_clock_ms >= sg.end_ts;
        if distance_exceeded || time_exceeded {
            return trigger_event_end(ad, state, now);
        }
    }

    let incoming_id = state.event_subgroup_id();
    let active_id = ad.event_subgroup.as_ref().map(|sg| sg.id);

    if incoming_id == 0 {
        if active_id.is_some() {
            return trigger_event_end(ad, state, now);
        }
        return EventStateOutcome::Idle;
    }

    // Same subgroup already active — nothing to do.
    if active_id == Some(incoming_id) {
        return EventStateOutcome::Idle;
    }

    // New subgroup: look it up.
    let sg = match sg_lookup.get(&incoming_id) {
        Some(s) => s.clone(),
        None => return EventStateOutcome::Idle,
    };

    apply_privacy_flags(ad, self_athlete_id, &sg);
    ad.event_subgroup = Some(sg);

    if state.time() == 0.0 {
        ad.event_start_pending = true;
        return EventStateOutcome::StartPending;
    }

    trigger_event_start(ad, state, behavior, now)
}

fn trigger_event_start(
    ad: &mut AthleteData,
    state: &dyn PlayerStateView,
    behavior: EventBehavior,
    now: f64,
) -> EventStateOutcome {
    if behavior.auto_reset {
        let fresh = ad.bucket.clone_reset();
        ad.bucket = fresh;
    } else if behavior.auto_lap {
        start_athlete_lap(ad, now);
    }

    let sg_id = ad.event_subgroup.as_ref().map(|sg| sg.id);
    let mut slice = DataSlice::new_from(ad, now);
    slice.event_subgroup_id = sg_id;
    slice.start_event_distance = state.event_distance();
    slice.start_world_time = state.world_time();
    let slice_id = slice.id;
    ad.event_slices.push(slice);
    ad.event_start_pending = false;

    EventStateOutcome::Started { slice_id }
}

fn trigger_event_end(
    ad: &mut AthleteData,
    state: &dyn PlayerStateView,
    now: f64,
) -> EventStateOutcome {
    let slice_id = if let Some(slice) = ad.event_slices.last_mut() {
        slice.end_event_distance = Some(state.event_distance());
        slice.close(now);
        slice.id
    } else {
        0
    };
    ad.event_subgroup = None;
    ad.event_start_pending = false;
    EventStateOutcome::Ended { slice_id }
}

fn apply_privacy_flags(ad: &mut AthleteData, self_athlete_id: u32, sg: &EventSubgroup) {
    if ad.athlete_id == self_athlete_id {
        return;
    }
    for tag in &sg.all_tags {
        match tag.as_str() {
            "hidewbal" => ad.event_privacy.hide_w_bal = true,
            "hideftp" => ad.event_privacy.hide_ftp = true,
            "hidethehud" | "nooverlays" => ad.disabled_by_event = true,
            _ => {}
        }
    }
}
