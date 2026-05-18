// SPDX-License-Identifier: AGPL-3.0-only

use std::{collections::HashMap, path::Path};
use serde::{Deserialize, Serialize};
use zwift_stats::{
    AthleteData, GroupMeta, MostRecentState, NearbyEntry, PlayerStateView,
    Segment, SegmentLookup,
    active_segment_check, compute_groups,
};

// Input deserialization

#[derive(Deserialize)]
struct Session {
    meta: Meta,
    segments: Vec<SegmentSpec>,
    ticks: Vec<TickSpec>,
}

#[derive(Deserialize)]
struct Meta {
    cp: f64,
    w_prime: f64,
    road_length_m: f64,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct SegmentSpec {
    id: u32,
    course_id: u32,
    road_id: u32,
    reverse: bool,
    road_start: f64,
    road_finish: f64,
    distance: f64,
}

#[derive(Deserialize)]
struct TickSpec {
    power: f64,
    rpct: f64,
    a2_gap: f64,
}

// Output serialization

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Output {
    wbal_stream: Vec<Option<f64>>,
    distance_stream: Vec<f64>,
    group_counts: Vec<usize>,
    segment_slices: Vec<SliceOutput>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SliceOutput {
    segment_id: Option<u32>,
    incomplete: bool,
    duration_secs: f64,
}

// Stub SegmentLookup backed by the fixture's segment list.

struct StubSegments {
    by_road: HashMap<(u32, u32, bool), Vec<Segment>>,
    by_id: HashMap<u32, Segment>,
}

impl StubSegments {
    fn from_specs(specs: &[SegmentSpec]) -> Self {
        let mut by_road: HashMap<(u32, u32, bool), Vec<Segment>> = HashMap::new();
        let mut by_id: HashMap<u32, Segment> = HashMap::new();
        for s in specs {
            let seg = Segment {
                id: s.id,
                course_id: s.course_id,
                road_id: s.road_id,
                reverse: s.reverse,
                road_start: s.road_start,
                road_finish: s.road_finish,
                distance: s.distance,
            };
            by_road
                .entry((s.course_id, s.road_id, s.reverse))
                .or_default()
                .push(seg.clone());
            by_id.insert(s.id, seg);
        }
        StubSegments { by_road, by_id }
    }
}

impl SegmentLookup for StubSegments {
    fn road_segments(&self, course_id: u32, road_id: u32, reverse: bool) -> &[Segment] {
        self.by_road
            .get(&(course_id, road_id, reverse))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn segment(&self, id: u32) -> Option<&Segment> {
        self.by_id.get(&id)
    }
}

#[test]
fn step15_regression() {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/step15_session.json");
    let session: Session = serde_json::from_str(
        &std::fs::read_to_string(&fixture_path).expect("step15_session.json not found"),
    )
    .expect("step15_session.json parse failed");

    let segs = StubSegments::from_specs(&session.segments);

    let first_seg = session.segments.first().expect("fixture has no segments");
    let course_id = first_seg.course_id;
    let road_id = first_seg.road_id;

    let mut ad = AthleteData::new(1, course_id, 1, 0.0, 1.0);
    ad.w_bal.configure(session.meta.cp, session.meta.w_prime);

    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id: u32 = 1;
    let mut group_counts: Vec<usize> = Vec::new();
    let mut prev_state: Option<MostRecentState> = None;

    for (i, tick) in session.ticks.iter().enumerate() {
        let now = (i + 1) as f64;
        let state = MostRecentState {
            world_time: now * 1000.0,
            speed: 10.0,
            power: tick.power,
            heartrate: 0,
            cadence: 0,
            draft: 0.0,
            distance: now * session.meta.road_length_m / session.ticks.len() as f64,
            altitude: 0.0,
            lat: 0.0,
            lng: 0.0,
            course_id,
            road_id,
            road_time: tick.rpct * 1_000_000.0 + 5_000.0,
            reverse: false,
            event_subgroup_id: 0,
            group_id: 0,
            time: now,
            event_distance: 0.0,
        };

        let prev_ref: Option<&dyn PlayerStateView> =
            prev_state.as_ref().map(|s| s as &dyn PlayerStateView);
        ad.road_history.record(&state, prev_ref);
        ad.record_streams(&state, now);
        active_segment_check(&mut ad, &state, &segs, now);

        let nearby = [
            NearbyEntry {
                athlete_id: 1,
                gap: 0.0,
                draft: 50.0,
                weight: None,
                power: tick.power,
                heartrate: None,
                speed: 10.0,
                is_gap_est: false,
            },
            NearbyEntry {
                athlete_id: 3,
                gap: 0.5,
                draft: 50.0,
                weight: None,
                power: 200.0,
                heartrate: None,
                speed: 10.0,
                is_gap_est: false,
            },
            NearbyEntry {
                athlete_id: 2,
                gap: tick.a2_gap,
                draft: 50.0,
                weight: None,
                power: 200.0,
                heartrate: None,
                speed: 10.0,
                is_gap_est: false,
            },
        ];
        let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, now);
        group_counts.push(groups.len());

        prev_state = Some(state);
    }

    let output = Output {
        wbal_stream: ad.streams.wbal.clone(),
        distance_stream: ad.streams.distance.clone(),
        group_counts,
        segment_slices: ad
            .segment_slices
            .iter()
            .map(|s| SliceOutput {
                segment_id: s.segment_id,
                incomplete: s.incomplete,
                duration_secs: s.end.map(|e| e - s.start).unwrap_or(0.0),
            })
            .collect(),
    };

    let expected_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/step15_expected.json");

    if std::env::var("GENERATE_EXPECTED").is_ok() {
        std::fs::write(&expected_path, serde_json::to_string_pretty(&output).unwrap())
            .expect("could not write step15_expected.json");
        return;
    }

    if !expected_path.exists() {
        panic!(
            "Expected output not found: {}\n\
             Generate it with:\n  \
             GENERATE_EXPECTED=1 cargo test -p zwift-stats --test step15_regression",
            expected_path.display()
        );
    }

    let expected: Output = serde_json::from_str(
        &std::fs::read_to_string(&expected_path).unwrap(),
    )
    .unwrap();

    assert_eq!(output.group_counts, expected.group_counts, "group_counts mismatch");
    assert_eq!(
        output.segment_slices.len(),
        expected.segment_slices.len(),
        "segment_slices count mismatch"
    );
    for (i, (got, exp)) in output
        .segment_slices
        .iter()
        .zip(expected.segment_slices.iter())
        .enumerate()
    {
        assert_eq!(got.segment_id, exp.segment_id, "slice[{i}] segment_id");
        assert_eq!(got.incomplete, exp.incomplete, "slice[{i}] incomplete");
        assert!(
            (got.duration_secs - exp.duration_secs).abs() < 1e-6,
            "slice[{i}] duration_secs: {} vs {}",
            got.duration_secs,
            exp.duration_secs,
        );
    }

    assert_eq!(
        output.distance_stream.len(),
        expected.distance_stream.len(),
        "distance_stream length mismatch"
    );
    for (i, (got, exp)) in output
        .distance_stream
        .iter()
        .zip(expected.distance_stream.iter())
        .enumerate()
    {
        assert!((got - exp).abs() < 1e-6, "distance_stream[{i}]: {got} vs {exp}");
    }

    assert_eq!(
        output.wbal_stream.len(),
        expected.wbal_stream.len(),
        "wbal_stream length mismatch"
    );
    for (i, (got, exp)) in output
        .wbal_stream
        .iter()
        .zip(expected.wbal_stream.iter())
        .enumerate()
    {
        match (got, exp) {
            (Some(g), Some(e)) => assert!(
                (g - e).abs() < 1e-6,
                "wbal_stream[{i}]: {g} vs {e}",
            ),
            (None, None) => {}
            _ => panic!("wbal_stream[{i}]: {got:?} vs {exp:?}"),
        }
    }
}
