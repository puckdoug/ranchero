// SPDX-License-Identifier: AGPL-3.0-only

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
}

impl Default for RoadHistory {
    fn default() -> Self {
        Self::new()
    }
}
