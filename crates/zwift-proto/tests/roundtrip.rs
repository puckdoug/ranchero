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
