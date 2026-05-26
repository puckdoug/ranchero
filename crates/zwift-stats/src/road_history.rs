// SPDX-License-Identifier: AGPL-3.0-only

use crate::PlayerStateView;
<<<<<<< Updated upstream
use crate::periods::{ROAD_TIME_OFFSET, ROAD_TIME_SCALE, BOUNDARY_ERROR_TERM};

pub type RoadKey = RoadDesc;

pub trait RoadGeometry {
    fn road_distance(&self, road: &RoadKey, start_pct: f64, end_pct: f64) -> f64;
}

pub struct RoadComparison {
    pub world_time: f64,
    pub distance: f64,
    pub reversed: bool,
}

#[derive(Debug, Clone)]
pub struct RoadPoint {
    pub rpct: f64,
    pub world_time: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoadDesc {
    pub road_id: u32,
    pub course_id: u32,
    pub reverse: bool,
}

#[derive(Debug)]
pub struct RoadHistory {
    a_road: Option<RoadDesc>,
    tier_a: Vec<RoadPoint>,
    b_road: Option<RoadDesc>,
    tier_b: Vec<RoadPoint>,
    c_road: Option<RoadDesc>,
    tier_c: Vec<RoadPoint>,
=======

#[derive(Debug)]
pub struct RoadHistory {
    // Placeholder: will implement three-tier ladder in STEP 15.13
>>>>>>> Stashed changes
}

impl RoadHistory {
    pub fn new() -> Self {
<<<<<<< Updated upstream
        RoadHistory {
            a_road: None,
            tier_a: Vec::new(),
            b_road: None,
            tier_b: Vec::new(),
            c_road: None,
            tier_c: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tier_a.is_empty() && self.tier_b.is_empty() && self.tier_c.is_empty()
    }

    pub fn record(&mut self, state: &dyn PlayerStateView, prev: Option<&dyn PlayerStateView>) {
        let rpct = (state.road_time() - ROAD_TIME_OFFSET) / ROAD_TIME_SCALE;
        let road_desc = RoadDesc {
            road_id: state.road_id(),
            course_id: state.course_id(),
            reverse: state.reverse(),
        };

        if let Some(prev) = prev {
            if prev.course_id() == state.course_id() {
                let mut shift = false;
                if let Some(a_road) = &self.a_road {
                    if *a_road != road_desc {
                        shift = true;
                    } else if let Some(last) = self.tier_a.last() {
                        let delta = rpct - last.rpct;
                        if delta < 0.0 {
                            if delta < -BOUNDARY_ERROR_TERM {
                                shift = true;
                            } else {
                                self.tier_a.clear();
                            }
                        }
                    }
                }
                if shift {
                    self.tier_c = std::mem::take(&mut self.tier_b);
                    self.c_road = self.b_road.take();
                    self.tier_b = std::mem::take(&mut self.tier_a);
                    self.b_road = self.a_road.take();
                }
            } else {
                self.tier_b.clear();
                self.b_road = None;
                self.tier_c.clear();
                self.c_road = None;
            }
        }

        if self.a_road.as_ref() != Some(&road_desc) {
            self.a_road = Some(road_desc);
        }
        self.tier_a.push(RoadPoint { rpct, world_time: state.world_time() });
    }

    /// Returns the maximum rpct recorded for a road across all tiers.
    pub fn max_rpct_for(&self, road_id: u32, course_id: u32, reverse: bool) -> Option<f64> {
        let tiers: [(&Option<RoadDesc>, &Vec<RoadPoint>); 3] = [
            (&self.a_road, &self.tier_a),
            (&self.b_road, &self.tier_b),
            (&self.c_road, &self.tier_c),
        ];
        for (road_opt, points) in tiers {
            if let Some(rd) = road_opt {
                if rd.road_id == road_id && rd.course_id == course_id && rd.reverse == reverse {
                    let max = points.iter().map(|p| p.rpct).fold(f64::NEG_INFINITY, f64::max);
                    if max > f64::NEG_INFINITY {
                        return Some(max);
                    }
                }
            }
        }
        None
=======
        RoadHistory {}
    }

    pub fn is_empty(&self) -> bool {
        true
    }

    pub fn record(&mut self, state: &dyn PlayerStateView, prev: Option<&dyn PlayerStateView>) {
        // Placeholder: will implement in STEP 15.13-I
>>>>>>> Stashed changes
    }
}

impl Default for RoadHistory {
    fn default() -> Self {
        Self::new()
    }
}
<<<<<<< Updated upstream

pub fn compare_road_positions(
    p1: &RoadHistory,
    p2: &RoadHistory,
    env: &dyn RoadGeometry,
) -> Option<RoadComparison> {
    let p2_a_road = p2.a_road.as_ref()?;
    let p2_a_last = p2.tier_a.last()?;

    // Same current road — direct comparison.
    if let Some(p1_a_road) = &p1.a_road {
        if p1_a_road == p2_a_road {
            if let Some(p1_a_last) = p1.tier_a.last() {
                let reversed = p1_a_last.rpct < p2_a_last.rpct;
                let distance = env.road_distance(p1_a_road, p2_a_last.rpct, p1_a_last.rpct);
                let world_time = p1_a_last.world_time - p2_a_last.world_time;
                return Some(RoadComparison { world_time, distance, reversed });
            }
        }
    }

    // p1 left p2's current road one road ago (tier B).
    if let Some(p1_b_road) = &p1.b_road {
        if p1_b_road == p2_a_road {
            if let Some(p1_b_last) = p1.tier_b.last() {
                if p1_b_last.rpct >= p2_a_last.rpct - BOUNDARY_ERROR_TERM {
                    let distance = env.road_distance(p1_b_road, p2_a_last.rpct, p1_b_last.rpct);
                    let world_time = p1_b_last.world_time - p2_a_last.world_time;
                    return Some(RoadComparison { world_time, distance, reversed: false });
                }
            }
        }
    }

    // p1 left p2's current road two roads ago (tier C).
    if let Some(p1_c_road) = &p1.c_road {
        if p1_c_road == p2_a_road {
            if let Some(p1_c_last) = p1.tier_c.last() {
                if p1_c_last.rpct >= p2_a_last.rpct - 0.01 {
                    let distance = env.road_distance(p1_c_road, p2_a_last.rpct, p1_c_last.rpct);
                    let world_time = p1_c_last.world_time - p2_a_last.world_time;
                    return Some(RoadComparison { world_time, distance, reversed: false });
                }
            }
        }
    }

    None
}
=======
>>>>>>> Stashed changes
