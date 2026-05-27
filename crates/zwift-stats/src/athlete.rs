// SPDX-License-Identifier: AGPL-3.0-only

//! [`AthleteData`] and [`AthleteRegistry`] — per-athlete state and garbage collection.

use std::collections::{HashMap, HashSet};
use crate::{
    DataBucket, DataSlice, ExpWeightedAvg, Sample,
    wbal::WBalAccumulator, zones::ZonesAccumulator,
    streams::{Streams, LatLng}, road_history::RoadHistory,
    periods::{ATHLETE_GC_TTL_SECS, GROUP_GC_TTL_SECS, SMOOTH_GRADE_WINDOW},
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventSubgroup {
    pub id: u32,
    pub course_id: u32,
    pub all_tags: Vec<String>,
    pub end_ts: u64,
    pub end_distance: f64,
    /// Athlete ids (from EventSubgroupProtobuf.invited_leaders) who lead the event.
    pub invited_leaders: Vec<u64>,
    /// Athlete ids (from EventSubgroupProtobuf.invited_sweepers) who sweep the event.
    pub invited_sweepers: Vec<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EventPrivacy {
    pub hide_w_bal: bool,
    pub hide_ftp: bool,
}

pub trait PlayerStateView {
    fn lat(&self) -> f64;
    fn lng(&self) -> f64;
    fn latlng(&self) -> LatLng;
    fn course_id(&self) -> u32;
    fn road_id(&self) -> u32;
    fn road_time(&self) -> f64;
    fn reverse(&self) -> bool;
    fn event_subgroup_id(&self) -> u32;
    fn group_id(&self) -> u32;
    fn world_time(&self) -> f64;
    fn time(&self) -> f64;
    fn event_distance(&self) -> f64;
    fn speed(&self) -> f64;
    fn power(&self) -> f64;
    fn heartrate(&self) -> u16;
    fn cadence(&self) -> u16;
    fn draft(&self) -> f64;
    fn distance(&self) -> f64;
    fn altitude(&self) -> f64;
    fn is_empty(&self) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MostRecentState {
    pub world_time: f64,
    pub speed: f64,
    pub power: f64,
    pub heartrate: u16,
    pub cadence: u16,
    pub draft: f64,
    pub distance: f64,
    pub altitude: f64,
    pub lat: f64,
    pub lng: f64,
    /// Web-Mercator-projected tile x-coordinate in `[0, 1]`.
    pub x: f64,
    /// Web-Mercator-projected tile y-coordinate in `[0, 1]`.
    pub y: f64,
    pub course_id: u32,
    pub road_id: u32,
    pub road_time: f64,
    pub reverse: bool,
    pub event_subgroup_id: u32,
    pub group_id: u32,
    pub time: f64,
    pub event_distance: f64,
    pub grade: f64,
    /// Workout progress fraction in `[0, 1]`, decoded from `(proto.progress >> 8) & 0xff) / 0xff`.
    pub progress: f64,
}

impl PlayerStateView for MostRecentState {
    fn lat(&self) -> f64 {
        self.lat
    }

    fn lng(&self) -> f64 {
        self.lng
    }

    fn latlng(&self) -> LatLng {
        LatLng { lat: self.lat, lng: self.lng }
    }

    fn course_id(&self) -> u32 {
        self.course_id
    }

    fn road_id(&self) -> u32 {
        self.road_id
    }

    fn road_time(&self) -> f64 {
        self.road_time
    }

    fn reverse(&self) -> bool {
        self.reverse
    }

    fn event_subgroup_id(&self) -> u32 {
        self.event_subgroup_id
    }

    fn group_id(&self) -> u32 {
        self.group_id
    }

    fn world_time(&self) -> f64 {
        self.world_time
    }

    fn time(&self) -> f64 {
        self.time
    }

    fn event_distance(&self) -> f64 {
        self.event_distance
    }

    fn speed(&self) -> f64 {
        self.speed
    }

    fn power(&self) -> f64 {
        self.power
    }

    fn heartrate(&self) -> u16 {
        self.heartrate
    }

    fn cadence(&self) -> u16 {
        self.cadence
    }

    fn draft(&self) -> f64 {
        self.draft
    }

    fn distance(&self) -> f64 {
        self.distance
    }

    fn altitude(&self) -> f64 {
        self.altitude
    }

    fn is_empty(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub struct AthleteData {
    pub athlete_id: u32,
    pub course_id: u32,
    pub sport: u8,
    pub created: f64,
    pub updated: f64,
    pub wt_offset: f64,
    pub distance: f64,
    pub distance_offset: f64,
    pub altitude: f64,
    pub internal_created: f64,
    pub internal_updated: f64,
    pub internal_accessed: f64,
    pub most_recent_state: Option<MostRecentState>,
    pub bucket: DataBucket,
    pub slice_counter: u32,
    pub w_bal: WBalAccumulator,
    pub time_in_power_zones: ZonesAccumulator,
    pub smooth_grade: ExpWeightedAvg,
    pub streams: Streams,
    pub road_history: RoadHistory,
    pub lap_slices: Vec<DataSlice>,
    pub event_slices: Vec<DataSlice>,
    pub segment_slices: Vec<DataSlice>,
    pub active_segments: HashMap<u32, DataSlice>,
    pub gap: Option<f64>,
    pub gap_distance: Option<f64>,
    pub is_gap_est: bool,
    pub gap_speed_ema: Option<ExpWeightedAvg>,
    pub group_id: Option<u32>,
    pub event_subgroup: Option<EventSubgroup>,
    pub event_privacy: EventPrivacy,
    pub disabled_by_event: bool,
    pub event_start_pending: bool,
    pub auto_lap_mark: Option<f64>,
    pub event_position: Option<u32>,
    pub event_participants: Option<u32>,
}

impl AthleteData {
    pub fn new(athlete_id: u32, course_id: u32, sport: u8, world_time: f64, now: f64) -> Self {
        let mut ad = AthleteData {
            athlete_id,
            course_id,
            sport,
            created: now,
            updated: now,
            wt_offset: world_time,
            distance: 0.0,
            distance_offset: 0.0,
            altitude: 0.0,
            internal_created: now,
            internal_updated: now,
            internal_accessed: now,
            most_recent_state: None,
            bucket: DataBucket::new(now),
            slice_counter: 0,
            w_bal: WBalAccumulator::new(),
            time_in_power_zones: ZonesAccumulator::new(),
            smooth_grade: ExpWeightedAvg::new(SMOOTH_GRADE_WINDOW, 0.0),
            streams: Streams::new(),
            road_history: RoadHistory::new(),
            lap_slices: Vec::new(),
            event_slices: Vec::new(),
            segment_slices: Vec::new(),
            active_segments: HashMap::new(),
            gap: None,
            gap_distance: None,
            is_gap_est: false,
            gap_speed_ema: None,
            group_id: None,
            event_subgroup: None,
            event_privacy: EventPrivacy::default(),
            disabled_by_event: false,
            event_start_pending: false,
            auto_lap_mark: None,
            event_position: None,
            event_participants: None,
        };

        // Push initial lap slice
        let initial_lap = DataSlice::new_from(&mut ad, now);
        ad.lap_slices.push(initial_lap);

        ad
    }

    pub fn touch(&mut self, now: f64) {
        self.internal_accessed = now;
    }

    pub fn record_streams(&mut self, state: &dyn PlayerStateView, _now: f64) {
        self.streams.distance.push(state.distance());
        self.streams.altitude.push(state.altitude());
        self.streams.latlng.push(state.latlng());
        let wbal = self.w_bal.accumulate(state.time(), Sample::Value(state.power()));
        self.streams.wbal.push(wbal);
        self.time_in_power_zones.accumulate(state.time(), state.power());
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

#[derive(Debug, Clone)]
pub struct GroupMeta {
    pub id: u32,
    pub accessed: f64,
    pub identity_set: HashSet<u32>,
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
            .or_insert(GroupMeta { id, accessed: now, identity_set: HashSet::new() });
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
