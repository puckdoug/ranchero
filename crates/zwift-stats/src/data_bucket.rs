// SPDX-License-Identifier: AGPL-3.0-only

//! [`DataBucket`] — per-session aggregation of five signal collectors.

use crate::collector::{DataCollector, DataCollectorOptions, PowerDataCollector};
use crate::{RollingAverage, RollingPower};

#[derive(Debug)]
pub struct DataBucket {
    start: f64,
    coffee_time: f64,
    work_time: f64,
    follow_time: f64,
    solo_time: f64,
    work_kj: f64,
    follow_kj: f64,
    solo_kj: f64,
    power: PowerDataCollector,
    hr: DataCollector<RollingAverage>,
    speed: DataCollector<RollingAverage>,
    cadence: DataCollector<RollingAverage>,
    draft: DataCollector<RollingPower>,
}

impl DataBucket {
    pub fn new(start: f64) -> Self {
        let power_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: false,
            round: true,
        };

        let hr_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: true,
            round: true,
        };

        let speed_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: true,
            round: false,
        };

        let cadence_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: true,
            round: true,
        };

        let draft_opts = DataCollectorOptions {
            ideal_gap: 1.0,
            max_gap: 15.0,
            active: true,
            ignore_zeros: false,
            round: true,
        };

        DataBucket {
            start,
            coffee_time: 0.0,
            work_time: 0.0,
            follow_time: 0.0,
            solo_time: 0.0,
            work_kj: 0.0,
            follow_kj: 0.0,
            solo_kj: 0.0,
            power: PowerDataCollector::new(&[5.0, 15.0, 60.0, 300.0, 1200.0, 3600.0], power_opts),
            hr: DataCollector::new(&[60.0, 300.0, 1200.0, 3600.0], hr_opts),
            speed: DataCollector::new(&[60.0, 300.0, 1200.0, 3600.0], speed_opts),
            cadence: DataCollector::new(&[], cadence_opts),
            draft: DataCollector::new(&[60.0, 300.0, 1200.0, 3600.0], draft_opts),
        }
    }

    pub fn start(&self) -> f64 {
        self.start
    }

    pub fn coffee_time(&self) -> f64 {
        self.coffee_time
    }

    pub fn work_time(&self) -> f64 {
        self.work_time
    }

    pub fn follow_time(&self) -> f64 {
        self.follow_time
    }

    pub fn solo_time(&self) -> f64 {
        self.solo_time
    }

    pub fn work_kj(&self) -> f64 {
        self.work_kj
    }

    pub fn follow_kj(&self) -> f64 {
        self.follow_kj
    }

    pub fn solo_kj(&self) -> f64 {
        self.solo_kj
    }

    pub fn set_coffee_time(&mut self, value: f64) {
        self.coffee_time = value;
    }

    pub fn set_work_time(&mut self, value: f64) {
        self.work_time = value;
    }

    pub fn set_follow_time(&mut self, value: f64) {
        self.follow_time = value;
    }

    pub fn set_solo_time(&mut self, value: f64) {
        self.solo_time = value;
    }

    pub fn set_work_kj(&mut self, value: f64) {
        self.work_kj = value;
    }

    pub fn set_follow_kj(&mut self, value: f64) {
        self.follow_kj = value;
    }

    pub fn set_solo_kj(&mut self, value: f64) {
        self.solo_kj = value;
    }

    pub fn power(&self) -> &PowerDataCollector {
        &self.power
    }

    pub fn power_mut(&mut self) -> &mut PowerDataCollector {
        &mut self.power
    }

    pub fn hr(&self) -> &DataCollector<RollingAverage> {
        &self.hr
    }

    pub fn hr_mut(&mut self) -> &mut DataCollector<RollingAverage> {
        &mut self.hr
    }

    pub fn speed(&self) -> &DataCollector<RollingAverage> {
        &self.speed
    }

    pub fn speed_mut(&mut self) -> &mut DataCollector<RollingAverage> {
        &mut self.speed
    }

    pub fn cadence(&self) -> &DataCollector<RollingAverage> {
        &self.cadence
    }

    pub fn cadence_mut(&mut self) -> &mut DataCollector<RollingAverage> {
        &mut self.cadence
    }

    pub fn draft(&self) -> &DataCollector<RollingPower> {
        &self.draft
    }

    pub fn draft_mut(&mut self) -> &mut DataCollector<RollingPower> {
        &mut self.draft
    }

    pub fn ingest_power(&mut self, ts: f64, watts: f64) {
        self.power_mut().add(ts, watts);
    }

    pub fn ingest_hr(&mut self, ts: f64, bpm: f64) {
        self.hr_mut().add(ts, bpm);
    }

    pub fn ingest_speed(&mut self, ts: f64, kph: f64) {
        self.speed_mut().add(ts, kph);
    }

    pub fn ingest_cadence(&mut self, ts: f64, rpm: f64) {
        self.cadence_mut().add(ts, rpm);
    }

    pub fn ingest_draft(&mut self, ts: f64, value: f64) {
        self.draft_mut().add(ts, value);
    }

    pub fn clone_reset(&self) -> Self {
        DataBucket {
            start: self.start,
            coffee_time: 0.0,
            work_time: 0.0,
            follow_time: 0.0,
            solo_time: 0.0,
            work_kj: 0.0,
            follow_kj: 0.0,
            solo_kj: 0.0,
            power: self.power.clone_reset(),
            hr: self.hr.clone_reset(),
            speed: self.speed.clone_reset(),
            cadence: self.cadence.clone_reset(),
            draft: self.draft.clone_reset(),
        }
    }

    pub fn clone_continue(&self) -> Self {
        DataBucket {
            start: self.start,
            coffee_time: self.coffee_time,
            work_time: self.work_time,
            follow_time: self.follow_time,
            solo_time: self.solo_time,
            work_kj: self.work_kj,
            follow_kj: self.follow_kj,
            solo_kj: self.solo_kj,
            power: self.power.clone_continue(),
            hr: self.hr.clone_continue(),
            speed: self.speed.clone_continue(),
            cadence: self.cadence.clone_continue(),
            draft: self.draft.clone_continue(),
        }
    }
}
