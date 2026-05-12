// SPDX-License-Identifier: AGPL-3.0-only

use crate::PlayerStateView;

#[derive(Debug)]
pub struct RoadHistory {
    // Placeholder: will implement three-tier ladder in STEP 15.13
}

impl RoadHistory {
    pub fn new() -> Self {
        RoadHistory {}
    }

    pub fn is_empty(&self) -> bool {
        true
    }

    pub fn record(&mut self, _state: &dyn PlayerStateView, _prev: Option<&dyn PlayerStateView>) {
        // Placeholder: will implement in STEP 15.13-I
    }
}

impl Default for RoadHistory {
    fn default() -> Self {
        Self::new()
    }
}
