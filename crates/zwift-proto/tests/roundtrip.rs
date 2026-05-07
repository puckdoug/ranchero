// SPDX-License-Identifier: AGPL-3.0-only
//
// Round-trip tests for each message type the live-data core relies on.
// For every type T, the assertion is: encoding `T::default()` and
// decoding the resulting bytes yields a value `assert_eq!`-equal to the
// original. This exercises the prost-generated codec end-to-end and is
// the minimum bar STEP 06 must clear before downstream crates can rely
// on the schema.

use prost::Message;
use zwift_proto::{
    ClientToServer, Event, EventSubgroupProtobuf, LoginRequest, LoginResponse, PlayerLeftWorld,
    PlayerState, RelayAddress, RelayAddressesVod, RideOn, SegmentResult, ServerToClient,
    TcpAddress, TcpConfig, UdpConfig, UdpConfigVod, WorldAttribute,
};

fn assert_roundtrip<M: Message + Default + PartialEq + std::fmt::Debug>(original: M) {
    let mut bytes = Vec::with_capacity(original.encoded_len());
    original.encode(&mut bytes).expect("encode");
    let decoded = M::decode(&bytes[..]).expect("decode");
    assert_eq!(original, decoded, "round-trip mismatch");
}

#[test]
fn login_request_roundtrips() {
    assert_roundtrip(LoginRequest::default());
}

#[test]
fn login_response_roundtrips() {
    assert_roundtrip(LoginResponse::default());
}

#[test]
fn client_to_server_roundtrips() {
    assert_roundtrip(ClientToServer::default());
}

#[test]
fn server_to_client_roundtrips() {
    assert_roundtrip(ServerToClient::default());
}

#[test]
fn player_state_roundtrips() {
    assert_roundtrip(PlayerState::default());
}

#[test]
fn world_attribute_roundtrips() {
    assert_roundtrip(WorldAttribute::default());
}

#[test]
fn tcp_config_roundtrips() {
    assert_roundtrip(TcpConfig::default());
}

#[test]
fn tcp_address_roundtrips() {
    assert_roundtrip(TcpAddress::default());
}

#[test]
fn udp_config_roundtrips() {
    assert_roundtrip(UdpConfig::default());
}

#[test]
fn relay_address_roundtrips() {
    assert_roundtrip(RelayAddress::default());
}

#[test]
fn udp_config_vod_roundtrips() {
    assert_roundtrip(UdpConfigVod::default());
}

#[test]
fn relay_addresses_vod_roundtrips() {
    assert_roundtrip(RelayAddressesVod::default());
}

#[test]
fn segment_result_roundtrips() {
    assert_roundtrip(SegmentResult::default());
}

#[test]
fn ride_on_roundtrips() {
    assert_roundtrip(RideOn::default());
}

#[test]
fn player_left_world_roundtrips() {
    assert_roundtrip(PlayerLeftWorld::default());
}

#[test]
fn event_roundtrips() {
    assert_roundtrip(Event::default());
}

#[test]
fn event_subgroup_roundtrips() {
    assert_roundtrip(EventSubgroupProtobuf::default());
}

// ---------------------------------------------------------------------------
// Batch E — Ea red-state test (STEP-12.14 §C11)
// ---------------------------------------------------------------------------

/// `RelayAddress` must round-trip all nine proto fields, including the three
/// geographic / TLS fields: tag 7 (`x_bound_min`, f32), tag 8 (`y_bound_min`,
/// f32), tag 9 (`secure_port`, i32).
///
/// Approach: construct a `RelayAddress` with all 9 fields populated, encode
/// it, decode the bytes back, and assert the decoded values match the
/// originals.  If any field is absent from the generated struct it will be
/// `None` on decode and the equality check fails.
///
/// (STEP-12.14 §C11)
#[test]
fn relay_address_proto_carries_x_y_bound_min_and_secure_port() {
    let original = RelayAddress {
        lb_realm: Some(0),
        lb_course: Some(7),
        ip: Some("10.1.2.3".to_string()),
        port: Some(3024),
        ra_f5: Some(-50.0),
        ra_f6: Some(50.0),
        x_bound_min: Some(-100.0),
        y_bound_min: Some(-200.0),
        secure_port: Some(3025),
    };
    let bytes = original.encode_to_vec();
    let decoded = RelayAddress::decode(&bytes[..]).expect("decode RelayAddress");

    assert_eq!(decoded.x_bound_min, original.x_bound_min,
        "Batch E §C11: x_bound_min (tag 7) must survive encode→decode");
    assert_eq!(decoded.y_bound_min, original.y_bound_min,
        "Batch E §C11: y_bound_min (tag 8) must survive encode→decode");
    assert_eq!(decoded.secure_port, original.secure_port,
        "Batch E §C11: secure_port (tag 9) must survive encode→decode");
    assert_eq!(decoded, original,
        "Batch E §C11: RelayAddress must round-trip all 9 fields without loss");
}
