// SPDX-License-Identifier: AGPL-3.0-only

use crate::PlayerStateView;

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
}

impl RoadHistory {
    pub fn new() -> Self {
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
        let rpct = (state.road_time() - 5000.0) / 1_000_000.0;
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
                            if delta < -0.01 {
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
    }
}

impl Default for RoadHistory {
    fn default() -> Self {
        Self::new()
    }
}
