// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{MostRecentState, PlayerStateView};

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
        road_id: 42,
        road_time: 50.0,
        reverse: false,
        event_subgroup_id: 5,
        group_id: 10,
        time: 123.45,
        event_distance: 500.0,
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
        road_id: 42,
        road_time: 50.0,
        reverse: false,
        event_subgroup_id: 5,
        group_id: 10,
        time: 123.45,
        event_distance: 500.0,
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
