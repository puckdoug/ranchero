// SPDX-License-Identifier: AGPL-3.0-only

//! [`AthleteData`] and [`AthleteRegistry`] — per-athlete state and garbage collection.

use crate::DataBucket;

#[derive(Debug, Clone, Copy)]
pub struct MostRecentState {
    pub world_time: f64,
    pub speed: f64,
    pub power: f64,
    pub heartrate: u16,
    pub cadence: u16,
    pub draft: f64,
    pub distance: f64,
    pub altitude: f64,
}

#[derive(Debug)]
pub struct AthleteData {
    pub athlete_id: u32,
    pub course_id: u32,
    pub sport: u8,
    pub created: f64,
    pub updated: f64,
    pub wt_offset: f64,
    pub distance_offset: f64,
    pub internal_created: f64,
    pub internal_updated: f64,
    pub internal_accessed: f64,
    pub most_recent_state: Option<MostRecentState>,
    pub bucket: DataBucket,

    // STEP 15: wBal, timeInPowerZones, smoothGrade, streams, roadHistory,
    // lapSlices, eventSlices, segmentSlices, activeSegments
}

impl AthleteData {
    pub fn new(athlete_id: u32, course_id: u32, sport: u8, world_time: f64, now: f64) -> Self {
        AthleteData {
            athlete_id,
            course_id,
            sport,
            created: now,
            updated: now,
            wt_offset: world_time,
            distance_offset: 0.0,
            internal_created: now,
            internal_updated: now,
            internal_accessed: now,
            most_recent_state: None,
            bucket: DataBucket::new(now),
        }
    }

    pub fn touch(&mut self, now: f64) {
        self.internal_accessed = now;
    }

    pub fn record_update(&mut self, world_time: f64, now: f64) {
        self.updated = world_time;
        self.internal_updated = now;
        self.internal_accessed = now;
    }

    pub fn ingest_power(&mut self, now: f64, time: f64, watts: f64) {
        self.bucket.ingest_power(time, watts);
        self.internal_updated = now;
        self.internal_accessed = now;
    }

    pub fn ingest_hr(&mut self, now: f64, time: f64, bpm: f64) {
        self.bucket.ingest_hr(time, bpm);
        self.internal_updated = now;
        self.internal_accessed = now;
    }

    pub fn ingest_speed(&mut self, now: f64, time: f64, mps: f64) {
        self.bucket.ingest_speed(time, mps);
        self.internal_updated = now;
        self.internal_accessed = now;
    }

    pub fn ingest_cadence(&mut self, now: f64, time: f64, rpm: f64) {
        self.bucket.ingest_cadence(time, rpm);
        self.internal_updated = now;
        self.internal_accessed = now;
    }

    pub fn ingest_draft(&mut self, now: f64, time: f64, draft: f64) {
        self.bucket.ingest_draft(time, draft);
        self.internal_updated = now;
        self.internal_accessed = now;
    }
}
