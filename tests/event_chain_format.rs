// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 13 — Formatter spreads event-context fields.
//
// `_getEventOrRouteInfo` in sauce4zwift (stats.mjs:4293) spreads:
//   eventLeader, eventSweeper, remaining, remainingMetric, remainingType,
//   remainingEnd
// into the formatted athlete record when the athlete is in an event subgroup.
//
// In ranchero these fields map to the corresponding camelCase output keys of
// `format_athlete_data_v1` and `format_athlete_v2`, using:
//   - EventSubgroup.invited_leaders / invited_sweepers to determine leader /
//     sweeper status for the athlete's own id.
//   - EventSubgroup.end_distance > 0 → distance-based remaining
//   - EventSubgroup.end_ts > 0       → time-based remaining (not exercised here)
//
// These tests fail today because:
//   1. EventSubgroup has no invited_leaders / invited_sweepers fields.
//   2. format_athlete_data_v1 / format_athlete_v2 hardcode all four fields
//      to None rather than reading from ad.event_subgroup.

use ranchero::web::format::{format_athlete_data_v1, format_athlete_v2};
use zwift_stats::AthleteData;
use zwift_stats::athlete::EventSubgroup;

fn ad_with_event(athlete_id: u32, sg: EventSubgroup) -> AthleteData {
    let mut ad = AthleteData::new(athlete_id, 1, 0, 0.0, 0.0);
    ad.event_subgroup = Some(sg);
    ad
}

// ---------------------------------------------------------------------------
// v1 formatter
// ---------------------------------------------------------------------------

/// Athlete is in a distance-based event and is in the invited-leaders list.
/// format_athlete_data_v1 must set eventLeader=true and spread the
/// remaining / remainingMetric / remainingType / remainingEnd fields.
#[test]
fn format_v1_spreads_event_leader_and_remaining_distance() {
    let sg = EventSubgroup {
        id:               42,
        course_id:        1,
        all_tags:         vec![],
        end_ts:           0,
        end_distance:     50_000.0, // 50 km race
        invited_leaders:  vec![1],  // athlete 1 is a leader
        invited_sweepers: vec![],
    };
    let ad = ad_with_event(1, sg);

    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0);

    assert_eq!(
        result["eventLeader"],
        serde_json::json!(true),
        "athlete in invited_leaders must produce eventLeader=true",
    );
    assert!(
        result.get("eventSweeper").is_none(),
        "eventSweeper must be absent when athlete is not a sweeper",
    );
    assert_eq!(
        result["remainingMetric"],
        serde_json::json!("distance"),
        "distance-based event must produce remainingMetric='distance'",
    );
    assert_eq!(
        result["remainingType"],
        serde_json::json!("event"),
        "event-based remaining must produce remainingType='event'",
    );
    assert_eq!(
        result["remainingEnd"],
        serde_json::json!(50_000.0),
        "remainingEnd must equal the subgroup end_distance",
    );
    assert!(
        result.get("remaining").is_some(),
        "remaining must be present for a distance-based event",
    );
}

/// Athlete is in a distance-based event and is in the invited-sweepers list.
/// format_athlete_data_v1 must set eventSweeper=true.
#[test]
fn format_v1_spreads_event_sweeper() {
    let sg = EventSubgroup {
        id:               42,
        course_id:        1,
        all_tags:         vec![],
        end_ts:           0,
        end_distance:     20_000.0,
        invited_leaders:  vec![],
        invited_sweepers: vec![1], // athlete 1 is a sweeper
    };
    let ad = ad_with_event(1, sg);

    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0);

    assert_eq!(
        result["eventSweeper"],
        serde_json::json!(true),
        "athlete in invited_sweepers must produce eventSweeper=true",
    );
    assert!(
        result.get("eventLeader").is_none(),
        "eventLeader must be absent when athlete is not a leader",
    );
}

/// Without an active event subgroup all event-spread fields must be absent
/// (omitted from JSON, not null).
#[test]
fn format_v1_event_fields_absent_without_active_event() {
    let ad = AthleteData::new(1, 1, 0, 0.0, 0.0);

    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0);

    for key in &["eventLeader", "eventSweeper", "remaining",
                  "remainingMetric", "remainingType", "remainingEnd"] {
        assert!(
            result.get(key).is_none(),
            "{key} must be absent when athlete has no active event (got {:?})",
            result.get(key),
        );
    }
}

/// Athlete is in a non-leader, non-sweeper position.
/// eventLeader and eventSweeper must be absent, but remaining fields must
/// still be present (parity with sauce's _getEventOrRouteInfo).
#[test]
fn format_v1_remaining_present_for_non_leader_participant() {
    let sg = EventSubgroup {
        id:               42,
        course_id:        1,
        all_tags:         vec![],
        end_ts:           0,
        end_distance:     10_000.0,
        invited_leaders:  vec![99],  // some other athlete
        invited_sweepers: vec![],
    };
    let ad = ad_with_event(1, sg); // athlete 1 is not in either list

    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0);

    assert!(
        result.get("eventLeader").is_none(),
        "eventLeader must be absent for non-leader",
    );
    assert!(
        result.get("eventSweeper").is_none(),
        "eventSweeper must be absent for non-sweeper",
    );
    assert!(
        result.get("remaining").is_some(),
        "remaining must be present even for a non-leader participant",
    );
}

// ---------------------------------------------------------------------------
// v2 formatter
// ---------------------------------------------------------------------------

/// The v2 formatter must also spread event-leader status when the athlete is
/// in the invited-leaders list.
#[test]
fn format_v2_spreads_event_leader() {
    let sg = EventSubgroup {
        id:               42,
        course_id:        1,
        all_tags:         vec![],
        end_ts:           0,
        end_distance:     25_000.0,
        invited_leaders:  vec![1],
        invited_sweepers: vec![],
    };
    let ad = ad_with_event(1, sg);

    let result = format_athlete_v2(&ad, &[], None, None, false);

    assert_eq!(
        result["eventLeader"],
        serde_json::json!(true),
        "v2 format must spread eventLeader=true when athlete is a leader",
    );
    assert_eq!(
        result["remainingMetric"],
        serde_json::json!("distance"),
        "v2 format must spread remainingMetric='distance' for a distance event",
    );
}

/// v2: all event-spread fields absent without an active event.
#[test]
fn format_v2_event_fields_absent_without_active_event() {
    let ad = AthleteData::new(1, 1, 0, 0.0, 0.0);

    let result = format_athlete_v2(&ad, &[], None, None, false);

    for key in &["eventLeader", "eventSweeper", "remaining",
                  "remainingMetric", "remainingType", "remainingEnd"] {
        assert!(
            result.get(key).is_none(),
            "v2: {key} must be absent without an active event (got {:?})",
            result.get(key),
        );
    }
}
