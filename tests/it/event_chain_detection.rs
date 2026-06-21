// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 13 — Event detection via ProtoView.
//
// `apply_event_state` looks up the incoming `event_subgroup_id()` in a cache.
// Today the production path goes through ProtoView, and
// `ProtoView::event_subgroup_id()` is hardcoded to return 0, so
// `apply_event_state` always returns `Idle` for live proto-sourced states.
//
// These tests confirm that once ProtoView reads tag 29 (eventSubgroupId), the
// detection logic fires correctly, matching behaviour already proven for
// MostRecentState in crates/zwift-stats/tests/event_detection.rs.

use std::collections::HashMap;

use ranchero::web::proto_view::ProtoView;
use zwift_proto::PlayerState;
use zwift_stats::{apply_event_state, AthleteData, EventBehavior, EventStateOutcome};
use zwift_stats::athlete::EventSubgroup;

fn no_behavior() -> EventBehavior {
    EventBehavior { auto_reset: false, auto_lap: false }
}

fn make_subgroup(id: u32) -> EventSubgroup {
    EventSubgroup {
        id,
        course_id: 1,
        ..Default::default()
    }
}

fn sg_map(sg: EventSubgroup) -> HashMap<u32, EventSubgroup> {
    let mut m = HashMap::new();
    m.insert(sg.id, sg);
    m
}

/// A PlayerState with group_id (tag 29) set to a known subgroup, paired with a
/// populated subgroup cache, must yield EventStateOutcome::Started.
///
/// Today this returns Idle because ProtoView::event_subgroup_id() is hardcoded
/// to 0; it passes once the implementation reads PlayerState.group_id.
#[test]
fn apply_event_state_detects_event_via_proto_view() {
    let state = PlayerState {
        group_id:   Some(7),     // tag 29 = eventSubgroupId per sauce
        time:       Some(5000),  // non-zero → event starts immediately
        world_time: Some(5000),
        ..Default::default()
    };

    let mut ad = AthleteData::new(1, 1, 0, 0.0, 0.0);
    let sgs = sg_map(make_subgroup(7));

    let outcome = apply_event_state(
        &mut ad,
        &ProtoView(&state),
        99,
        &sgs,
        no_behavior(),
        0.0,
        0,
    );

    assert!(
        matches!(outcome, EventStateOutcome::Started { .. }),
        "ProtoView with group_id=7 and a populated cache must yield Started, \
         not Idle (today Idle because event_subgroup_id() is hardcoded to 0): {outcome:?}",
    );
    assert!(ad.event_subgroup.is_some(), "event_subgroup must be set on the athlete");
}

/// A ProtoView with group_id = 0 (no event) leaves the athlete in Idle even
/// with a non-empty cache.  Regression guard: reading tag 29 must not
/// accidentally use another field.
#[test]
fn apply_event_state_idle_when_group_id_zero() {
    let state = PlayerState {
        group_id:   Some(0),
        time:       Some(5000),
        world_time: Some(5000),
        ..Default::default()
    };

    let mut ad = AthleteData::new(1, 1, 0, 0.0, 0.0);
    let sgs = sg_map(make_subgroup(7));  // cache has an entry, but rider is not in it

    let outcome = apply_event_state(
        &mut ad,
        &ProtoView(&state),
        99,
        &sgs,
        no_behavior(),
        0.0,
        0,
    );

    assert!(
        matches!(outcome, EventStateOutcome::Idle),
        "group_id=0 must remain Idle regardless of cache contents: {outcome:?}",
    );
}

/// A state with no group_id field (absent proto tag) must also be Idle.
#[test]
fn apply_event_state_idle_when_group_id_absent() {
    let state = PlayerState {
        time:       Some(5000),
        world_time: Some(5000),
        ..Default::default()
    };

    let mut ad = AthleteData::new(1, 1, 0, 0.0, 0.0);
    let sgs = sg_map(make_subgroup(7));

    let outcome = apply_event_state(
        &mut ad,
        &ProtoView(&state),
        99,
        &sgs,
        no_behavior(),
        0.0,
        0,
    );

    assert!(
        matches!(outcome, EventStateOutcome::Idle),
        "absent group_id must yield Idle: {outcome:?}",
    );
}
