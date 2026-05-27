// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::{AthleteData, MostRecentState, LatLng};

fn make_state(distance: f64, altitude: f64, lat: f64, lng: f64) -> MostRecentState {
    MostRecentState {
        world_time: 0.0,
        speed: 0.0,
        power: 0.0,
        heartrate: 0,
        cadence: 0,
        draft: 0.0,
        distance,
        altitude,
        lat,
        lng,
        course_id: 1,
        road_id: 1,
        road_time: 0.0,
        reverse: false,
        event_subgroup_id: 0,
        group_id: 0,
        time: 0.0,
        event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    }
}

// 15.26-T: Streams

#[test]
fn record_streams_appends_distance_altitude_latlng_per_tick() {
    let mut ad = AthleteData::new(1, 1, 1, 0.0, 0.0);
    ad.record_streams(&make_state(10.0, 100.0, 1.0, 2.0), 0.0);
    ad.record_streams(&make_state(20.0, 110.0, 1.1, 2.1), 0.0);
    ad.record_streams(&make_state(30.0, 120.0, 1.2, 2.2), 0.0);

    assert_eq!(ad.streams.distance.len(), 3, "three distance entries appended");
    assert_eq!(ad.streams.altitude.len(), 3, "three altitude entries appended");
    assert_eq!(ad.streams.latlng.len(),   3, "three latlng entries appended");
    assert!((ad.streams.distance[2] - 30.0).abs() < 1e-9);
    assert!((ad.streams.altitude[1] - 110.0).abs() < 1e-9);
}

#[test]
fn latlng_uses_custom_type_with_named_fields() {
    let mut ad = AthleteData::new(1, 1, 1, 0.0, 0.0);
    let state = make_state(0.0, 0.0, 51.5074, -0.1278);
    ad.record_streams(&state, 0.0);

    let entry: &LatLng = &ad.streams.latlng[0];
    assert!((entry.lat - 51.5074).abs() < 1e-9, "lat field must match state.lat");
    assert!((entry.lng - (-0.1278)).abs() < 1e-9, "lng field must match state.lng");
}

#[test]
fn wbal_sample_appended_per_tick_when_accumulator_configured() {
    let mut ad = AthleteData::new(1, 1, 1, 0.0, 0.0);
    ad.w_bal.configure(200.0, 20_000.0);

    ad.record_streams(&make_state(0.0, 0.0, 0.0, 0.0), 1.0);
    ad.record_streams(&make_state(0.0, 0.0, 0.0, 0.0), 2.0);

    assert_eq!(ad.streams.wbal.len(), 2, "one wbal entry per tick");
    assert!(
        ad.streams.wbal.iter().all(|v| v.is_some()),
        "configured accumulator should produce Some values"
    );
}

#[test]
fn wbal_stream_carries_none_when_accumulator_unconfigured() {
    let mut ad = AthleteData::new(1, 1, 1, 0.0, 0.0);

    ad.record_streams(&make_state(0.0, 0.0, 0.0, 0.0), 1.0);
    ad.record_streams(&make_state(0.0, 0.0, 0.0, 0.0), 2.0);
    ad.record_streams(&make_state(0.0, 0.0, 0.0, 0.0), 3.0);

    assert_eq!(ad.streams.wbal.len(), 3, "one wbal entry per tick");
    assert!(
        ad.streams.wbal.iter().all(|v| v.is_none()),
        "unconfigured accumulator must produce None values"
    );
}
