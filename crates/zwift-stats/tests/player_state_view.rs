// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{MostRecentState, PlayerStateView, LatLng};

#[test]
fn most_recent_state_implements_view_trait() {
    let state = MostRecentState {
        world_time: 100.0,
        speed: 10.5,
        power: 250.0,
        heartrate: 150,
        cadence: 90,
        draft: 0.5,
        distance: 1000.0,
        altitude: 100.0,
        lat: 40.7128,
        lng: -74.0060,
        course_id: 0,
        road_id: 42,
        road_time: 50.0,
        reverse: false,
        event_subgroup_id: 5,
        group_id: 10,
        time: 123.45,
        event_distance: 500.0,
        grade: 0.0,
    };

    // Should be able to treat it as a trait object
    let view: &dyn PlayerStateView = &state;
    assert!(!view.is_empty(), "state should not be empty");
}

#[test]
fn view_trait_exposes_road_event_and_group_fields() {
    let state = MostRecentState {
        world_time: 100.0,
        speed: 10.5,
        power: 250.0,
        heartrate: 150,
        cadence: 90,
        draft: 0.5,
        distance: 1000.0,
        altitude: 100.0,
        lat: 40.7128,
        lng: -74.0060,
        course_id: 0,
        road_id: 42,
        road_time: 50.0,
        reverse: false,
        event_subgroup_id: 5,
        group_id: 10,
        time: 123.45,
        event_distance: 500.0,
        grade: 0.0,
    };

    // Access through trait
    let view: &dyn PlayerStateView = &state;

    assert_eq!(view.lat(), 40.7128, "lat should be accessible");
    assert_eq!(view.lng(), -74.0060, "lng should be accessible");
    assert_eq!(view.road_id(), 42, "road_id should be accessible");
    assert_eq!(view.road_time(), 50.0, "road_time should be accessible");
    assert_eq!(view.reverse(), false, "reverse should be accessible");
    assert_eq!(view.event_subgroup_id(), 5, "event_subgroup_id should be accessible");
    assert_eq!(view.group_id(), 10, "group_id should be accessible");
    assert_eq!(view.time(), 123.45, "time should be accessible");
    assert_eq!(view.event_distance(), 500.0, "event_distance should be accessible");
}

// 15.12-T: motion and energy accessors added in C-3.1
#[test]
fn view_trait_exposes_motion_and_energy_fields() {
    let state = MostRecentState {
        world_time: 100.0,
        speed: 12.5,
        power: 280.0,
        heartrate: 155,
        cadence: 92,
        draft: 0.3,
        distance: 2000.0,
        altitude: 50.0,
        lat: 51.5074,
        lng: -0.1278,
        course_id: 2,
        road_id: 7,
        road_time: 30.0,
        reverse: true,
        event_subgroup_id: 3,
        group_id: 1,
        time: 60.0,
        event_distance: 800.0,
        grade: 0.0,
    };

    let view: &dyn PlayerStateView = &state;
    assert_eq!(view.speed(), 12.5, "speed");
    assert_eq!(view.power(), 280.0, "power");
    assert_eq!(view.heartrate(), 155u16, "heartrate");
    assert_eq!(view.cadence(), 92u16, "cadence");
    assert!((view.draft() - 0.3).abs() < 1e-9, "draft");
    assert_eq!(view.distance(), 2000.0, "distance");
    assert_eq!(view.altitude(), 50.0, "altitude");
    let ll: LatLng = view.latlng();
    assert!((ll.lat - 51.5074).abs() < 1e-9, "latlng.lat");
    assert!((ll.lng - (-0.1278)).abs() < 1e-9, "latlng.lng");
}
