// SPDX-License-Identifier: AGPL-3.0-only

use crate::{DataBucket, AthleteData};

#[derive(Debug)]
pub struct DataSlice {
    pub id: u64,
    pub start: f64,
    pub end: Option<f64>,
    pub course_id: u32,
    pub sport: u8,
    pub bucket: DataBucket,
}

impl DataSlice {
    pub fn new_from(ad: &mut AthleteData, start: f64) -> Self {
        // Get and increment the per-athlete counter
        let counter = ad.slice_counter;
        ad.slice_counter += 1;

        // Pack the ID: upper 32 bits = athlete_id, lower 32 bits = counter
        let id = ((ad.athlete_id as u64) << 32) | (counter as u64);

        DataSlice {
            id,
            start,
            end: None,
            course_id: ad.course_id,
            sport: ad.sport,
            bucket: ad.bucket.clone_reset(),
        }
    }

    pub fn close(&mut self, now: f64) {
        if self.end.is_none() {
            self.end = Some(now);
        }
    }
}
