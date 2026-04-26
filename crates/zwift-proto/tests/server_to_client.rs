// SPDX-License-Identifier: AGPL-3.0-only
//
// Vector tests for the inbound relay payload. Two flavors:
//
// 1. `synthetic_*` — build a `ServerToClient` in memory with selected
//    fields, encode, decode, and assert specific fields survive. This
//    catches obvious codec bugs without needing real Zwift wire
//    captures.
//
// 2. `fixture_*` — read a captured `ServerToClient` byte dump from
//    `tests/fixtures/` and assert known values. These require real
//    Zwift wire captures to be placed under the fixtures directory;
//    see the per-test comments for what each fixture should contain.

use prost::Message;
use std::path::PathBuf;
use zwift_proto::{PlayerState, ServerToClient};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn synthetic_server_to_client_preserves_seqno() {
    let original = ServerToClient {
        seqno: Some(42),
        ..Default::default()
    };
    let mut bytes = Vec::new();
    original.encode(&mut bytes).expect("encode");
    let decoded = ServerToClient::decode(&bytes[..]).expect("decode");
    assert_eq!(decoded.seqno, Some(42));
}

#[test]
fn synthetic_server_to_client_preserves_player_state_fields() {
    let player = PlayerState {
        athlete_id: Some(12_345),
        power: Some(250),
        heartrate: Some(155),
        distance: Some(10_000),
        ..Default::default()
    };
    let original = ServerToClient {
        seqno: Some(1),
        player_states: vec![player.clone()],
        ..Default::default()
    };
    let mut bytes = Vec::new();
    original.encode(&mut bytes).expect("encode");
    let decoded = ServerToClient::decode(&bytes[..]).expect("decode");
    assert_eq!(decoded.player_states.len(), 1);
    assert_eq!(decoded.player_states[0].athlete_id, Some(12_345));
    assert_eq!(decoded.player_states[0].power, Some(250));
    assert_eq!(decoded.player_states[0].heartrate, Some(155));
    assert_eq!(decoded.player_states[0].distance, Some(10_000));
}

// Fixture: any captured ServerToClient packet. Place a single decoded
// payload (i.e. plaintext after AES-GCM decrypt and after stripping any
// transport-layer framing) at tests/fixtures/server_to_client_basic.bin.
// The test asserts only that it decodes and contains at least one
// PlayerState, which any real packet should satisfy. Tighten the
// assertions once you know the fixture's contents.
#[test]
fn fixture_basic_packet_decodes() {
    let path = fixture_path("server_to_client_basic.bin");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing fixture {}: capture a real ServerToClient payload from \
             Zwift wire traffic and place it at this path. ({})",
            path.display(),
            e
        )
    });
    let msg = ServerToClient::decode(&bytes[..]).expect("decode captured packet");
    assert!(
        !msg.player_states.is_empty(),
        "expected at least one PlayerState in a real capture"
    );
}
