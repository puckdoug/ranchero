// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use crate::{AthleteData, PlayerStateView};
use crate::athlete::EventSubgroup;

#[derive(Debug, Clone, Copy)]
pub struct EventBehavior {
    pub auto_reset: bool,
    pub auto_lap: bool,
}

pub fn apply_event_state(
    _ad: &mut AthleteData,
    _state: &dyn PlayerStateView,
    _self_athlete_id: u32,
    _sg_lookup: &HashMap<u32, EventSubgroup>,
    _behavior: EventBehavior,
    _now: f64,
    _wall_clock_ms: u64,
) {
    // Placeholder: will implement event detection and privacy in STEP 15.14-I
}
