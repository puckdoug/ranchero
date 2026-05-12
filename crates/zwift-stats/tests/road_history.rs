// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{RoadHistory, PlayerStateView};

struct TestState {
    road_id: u32,
    reverse: bool,
    road_time: f64,
    course_id: u32,
    world_time: f64,
    lat: f64,
    lng: f64,
    event_subgroup_id: u32,
    group_id: u32,
    time: f64,
    event_distance: f64,
}

impl PlayerStateView for TestState {
    fn lat(&self) -> f64 {
        self.lat
    }
    fn lng(&self) -> f64 {
        self.lng
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
    fn is_empty(&self) -> bool {
        false
    }
}

#[test]
fn same_road_no_shift_appends_to_a() {
    let mut history = RoadHistory::new();

    let state1 = TestState {
        road_id: 42,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    history.record(&state1, None);

    let state2 = TestState {
        road_id: 42,
        reverse: false,
        road_time: 5.0,
        course_id: 1,
        world_time: 5.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 5.0,
        event_distance: 100.0,
    };

    let prev = TestState {
        road_id: 42,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    history.record(&state2, Some(&prev));
}

#[test]
fn road_change_shifts_a_to_b_b_to_c() {
    let mut history = RoadHistory::new();

    let state1 = TestState {
        road_id: 42,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    history.record(&state1, None);

    let prev = TestState {
        road_id: 42,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    let state2 = TestState {
        road_id: 43,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 5.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 5.0,
        event_distance: 100.0,
    };

    history.record(&state2, Some(&prev));
}

#[test]
fn direction_change_shifts_when_delta_below_minus_001() {
    let mut history = RoadHistory::new();

    let state1 = TestState {
        road_id: 42,
        reverse: false,
        road_time: 10.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    history.record(&state1, None);

    let prev = TestState {
        road_id: 42,
        reverse: false,
        road_time: 10.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    let state2 = TestState {
        road_id: 42,
        reverse: true,
        road_time: 5.0,
        course_id: 1,
        world_time: 5.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 5.0,
        event_distance: 100.0,
    };

    history.record(&state2, Some(&prev));
}

#[test]
fn direction_change_wipes_a_when_delta_in_minus_001_to_0() {
    let mut history = RoadHistory::new();

    let state1 = TestState {
        road_id: 42,
        reverse: false,
        road_time: 10.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    history.record(&state1, None);

    let prev = TestState {
        road_id: 42,
        reverse: false,
        road_time: 10.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    let state2 = TestState {
        road_id: 42,
        reverse: true,
        road_time: 9.9995,
        course_id: 1,
        world_time: 5.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 5.0,
        event_distance: 100.0,
    };

    history.record(&state2, Some(&prev));
}

#[test]
fn course_change_resets_b_and_c() {
    let mut history = RoadHistory::new();

    let state1 = TestState {
        road_id: 42,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    history.record(&state1, None);

    let prev = TestState {
        road_id: 42,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    let state2 = TestState {
        road_id: 50,
        reverse: false,
        road_time: 0.0,
        course_id: 2,
        world_time: 5.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 5.0,
        event_distance: 100.0,
    };

    history.record(&state2, Some(&prev));
}

#[test]
fn first_state_seeds_aroad_without_shift() {
    let mut history = RoadHistory::new();

    let state = TestState {
        road_id: 42,
        reverse: false,
        road_time: 0.0,
        course_id: 1,
        world_time: 0.0,
        lat: 0.0,
        lng: 0.0,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
    };

    history.record(&state, None);
}
