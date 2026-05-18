// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use zwift_stats::{
    AthleteData, MostRecentState,
    EventBehavior, EventStateOutcome,
    apply_event_state,
};
use zwift_stats::athlete::EventSubgroup;

fn make_state(event_subgroup_id: u32) -> MostRecentState {
    MostRecentState {
        world_time: 5000.0,
        speed: 0.0,
        power: 0.0,
        heartrate: 0,
        cadence: 0,
        draft: 0.0,
        distance: 0.0,
        altitude: 0.0,
        lat: 0.0,
        lng: 0.0,
        course_id: 1,
        road_id: 1,
        road_time: 5000.0,
        reverse: false,
        event_subgroup_id,
        group_id: 0,
        time: 5.0,
        event_distance: 0.0,
    }
}

fn no_behavior() -> EventBehavior {
    EventBehavior { auto_reset: false, auto_lap: false }
}

fn make_subgroup_with_tags(id: u32, tags: Vec<&str>) -> EventSubgroup {
    EventSubgroup {
        id,
        course_id: 1,
        all_tags: tags.into_iter().map(str::to_owned).collect(),
        end_ts: 0,
        end_distance: 0.0,
    }
}

fn sg_map(sg: EventSubgroup) -> HashMap<u32, EventSubgroup> {
    let mut m = HashMap::new();
    m.insert(sg.id, sg);
    m
}

// 15.15-T: when the target athlete IS the self athlete, no privacy flags are set
// regardless of the event's all_tags.
#[test]
fn self_athlete_skips_privacy_flags() {
    let athlete_id = 1u32;
    let mut ad = AthleteData::new(athlete_id, 42, 1, 0.0, 10.0);
    let sgs = sg_map(make_subgroup_with_tags(7, vec!["hidewbal", "hideftp", "hidethehud"]));

    apply_event_state(&mut ad, &make_state(7), athlete_id, &sgs, no_behavior(), 10.0, 0);

    assert!(!ad.event_privacy.hide_w_bal, "self athlete: hide_w_bal must not be set");
    assert!(!ad.event_privacy.hide_ftp, "self athlete: hide_ftp must not be set");
    assert!(!ad.disabled_by_event, "self athlete: disabled_by_event must not be set");
}

// 15.15-T: "hidewbal" tag sets event_privacy.hide_w_bal on a non-self athlete.
#[test]
fn non_self_hidewbal_sets_hide_w_bal() {
    let mut ad = AthleteData::new(2, 42, 1, 0.0, 10.0);
    let sgs = sg_map(make_subgroup_with_tags(7, vec!["hidewbal"]));

    apply_event_state(&mut ad, &make_state(7), 99, &sgs, no_behavior(), 10.0, 0);

    assert!(ad.event_privacy.hide_w_bal, "hidewbal tag must set hide_w_bal");
    assert!(!ad.event_privacy.hide_ftp, "only hidewbal tag: hide_ftp must stay false");
}

// 15.15-T: "hideftp" tag sets event_privacy.hide_ftp on a non-self athlete.
#[test]
fn non_self_hideftp_sets_hide_ftp() {
    let mut ad = AthleteData::new(2, 42, 1, 0.0, 10.0);
    let sgs = sg_map(make_subgroup_with_tags(7, vec!["hideftp"]));

    apply_event_state(&mut ad, &make_state(7), 99, &sgs, no_behavior(), 10.0, 0);

    assert!(ad.event_privacy.hide_ftp, "hideftp tag must set hide_ftp");
    assert!(!ad.event_privacy.hide_w_bal, "only hideftp tag: hide_w_bal must stay false");
}

// 15.15-T: "hidethehud" tag sets disabled_by_event on a non-self athlete.
#[test]
fn hidethehud_sets_disabled_by_event() {
    let mut ad = AthleteData::new(2, 42, 1, 0.0, 10.0);
    let sgs = sg_map(make_subgroup_with_tags(7, vec!["hidethehud"]));

    apply_event_state(&mut ad, &make_state(7), 99, &sgs, no_behavior(), 10.0, 0);

    assert!(ad.disabled_by_event, "hidethehud tag must set disabled_by_event");
}

// 15.15-T: "nooverlays" tag also sets disabled_by_event on a non-self athlete.
#[test]
fn nooverlays_sets_disabled_by_event() {
    let mut ad = AthleteData::new(2, 42, 1, 0.0, 10.0);
    let sgs = sg_map(make_subgroup_with_tags(7, vec!["nooverlays"]));

    apply_event_state(&mut ad, &make_state(7), 99, &sgs, no_behavior(), 10.0, 0);

    assert!(ad.disabled_by_event, "nooverlays tag must set disabled_by_event");
}
