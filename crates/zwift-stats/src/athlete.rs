// SPDX-License-Identifier: AGPL-3.0-only

//! [`AthleteData`] and [`AthleteRegistry`] — per-athlete state and garbage collection.

use std::collections::HashMap;
use crate::{DataBucket, periods::{ATHLETE_GC_TTL_SECS, GROUP_GC_TTL_SECS}};

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
    pub slice_counter: u32,

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
            slice_counter: 0,
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

#[derive(Debug, Clone, Copy)]
pub struct GroupMeta {
    pub id: u32,
    pub accessed: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GcReport {
    pub athletes_dropped: usize,
    pub groups_dropped: usize,
}

#[derive(Debug)]
pub struct AthleteRegistry {
    athletes: HashMap<u32, AthleteData>,
    groups: HashMap<u32, GroupMeta>,
}

impl AthleteRegistry {
    pub fn new() -> Self {
        AthleteRegistry {
            athletes: HashMap::new(),
            groups: HashMap::new(),
        }
    }

    pub fn upsert(
        &mut self,
        athlete_id: u32,
        course_id: u32,
        sport: u8,
        world_time: f64,
        now: f64,
    ) -> &mut AthleteData {
        self.athletes
            .entry(athlete_id)
            .and_modify(|ad| ad.record_update(world_time, now))
            .or_insert_with(|| AthleteData::new(athlete_id, course_id, sport, world_time, now))
    }

    pub fn get(&self, id: u32) -> Option<&AthleteData> {
        self.athletes.get(&id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut AthleteData> {
        self.athletes.get_mut(&id)
    }

    pub fn len(&self) -> usize {
        self.athletes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.athletes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &AthleteData)> {
        self.athletes.iter()
    }

    pub fn touch_group(&mut self, id: u32, now: f64) {
        self.groups
            .entry(id)
            .and_modify(|gm| gm.accessed = now)
            .or_insert(GroupMeta { id, accessed: now });
    }

    pub fn group(&self, id: u32) -> Option<&GroupMeta> {
        self.groups.get(&id)
    }

    pub fn groups_len(&self) -> usize {
        self.groups.len()
    }

    pub fn gc(&mut self, now: f64) -> GcReport {
        let athletes_before = self.athletes.len();
        self.athletes
            .retain(|_, ad| ad.internal_accessed >= now - ATHLETE_GC_TTL_SECS);
        let athletes_dropped = athletes_before - self.athletes.len();

        let groups_before = self.groups.len();
        self.groups
            .retain(|_, gm| gm.accessed >= now - GROUP_GC_TTL_SECS);
        let groups_dropped = groups_before - self.groups.len();

        GcReport {
            athletes_dropped,
            groups_dropped,
        }
    }
}

impl Default for AthleteRegistry {
    fn default() -> Self {
        Self::new()
    }
}
