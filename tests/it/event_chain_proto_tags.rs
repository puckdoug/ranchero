// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 13 QB1 — Proto tag 29 and 34 verification.
//
// sauce4zwift's merged proto3 schema names these fields:
//   tag 29: int32   eventSubgroupId
//   tag 34: float   _eventDistance  (value in cm; divide by 100 for metres)
//
// The zwift-offline proto2 schema — which ranchero vendors — uses different
// names for the same wire tags:
//   tag 29: int64   groupId
//   tag 34: float   dist_lat
//
// The tests below confirm that ProtoView reads these tags with the sauce
// interpretation (event context) rather than discarding them under the
// zwift-offline labels.  They fail today because:
//   ProtoView::event_subgroup_id() returns 0 (hardcoded)
//   ProtoView::event_distance()    returns 0.0 (hardcoded)
//
// They will pass once the implementation reads PlayerState.group_id (tag 29)
// and PlayerState.dist_lat (tag 34) as documented in docs/planning/
// STEP-20-additional-considerations.md Step 13.

use prost::Message as _;
use ranchero::web::proto_view::ProtoView;
use zwift_proto::PlayerState;
use zwift_stats::PlayerStateView as _;

/// Roundtrip a PlayerState through protobuf encoding and confirm that tag 29
/// (zwift-offline: groupId) is read by ProtoView as the event-subgroup id.
///
/// Sauce interprets tag 29 as `eventSubgroupId`; that is the correct reading
/// for identifying which event the athlete is riding.
#[test]
fn proto_tag_29_decoded_as_event_subgroup_id() {
    let state = PlayerState {
        group_id: Some(7777),
        ..Default::default()
    };

    // Roundtrip through protobuf bytes to confirm the wire-level tag.
    let decoded = PlayerState::decode(state.encode_to_vec().as_slice())
        .expect("roundtrip encode/decode must succeed");

    assert_eq!(
        ProtoView(&decoded).event_subgroup_id(),
        7777,
        "tag 29 (zwift-offline label: groupId) must be read as eventSubgroupId \
         (sauce label), not discarded",
    );
}

/// Roundtrip a PlayerState through protobuf encoding and confirm that tag 34
/// (zwift-offline: dist_lat) is read by ProtoView as event distance in metres
/// (i.e. the raw centimetre value divided by 100).
///
/// Sauce interprets tag 34 as `_eventDistance` (cm); divides by 100 to produce
/// metres in `processPlayerStateMessage`.
#[test]
fn proto_tag_34_decoded_as_event_distance_metres() {
    // 350000 cm = 3500 m
    let state = PlayerState {
        dist_lat: Some(350_000.0),
        ..Default::default()
    };

    let decoded = PlayerState::decode(state.encode_to_vec().as_slice())
        .expect("roundtrip encode/decode must succeed");

    let got = ProtoView(&decoded).event_distance();
    assert!(
        (got - 3500.0).abs() < 0.01,
        "tag 34 value 350000.0 (cm) must decode to 3500.0 m (÷ 100), got {got}",
    );
}

/// ProtoView exposes both event fields together with the expected values.
/// This is the direct unit test of the accessor pair before wiring into the
/// production path.
#[test]
fn proto_view_exposes_event_subgroup_id_and_distance() {
    let state = PlayerState {
        group_id: Some(42),     // tag 29 = eventSubgroupId per sauce
        dist_lat: Some(5000.0), // tag 34 = eventDistance in cm per sauce (= 50 m)
        ..Default::default()
    };

    let view = ProtoView(&state);

    assert_eq!(
        view.event_subgroup_id(),
        42,
        "event_subgroup_id() must return the group_id field value",
    );
    assert!(
        (view.event_distance() - 50.0).abs() < 0.001,
        "event_distance() must return dist_lat / 100 = 50.0 m, got {}",
        view.event_distance(),
    );
}
