// SPDX-License-Identifier: AGPL-3.0-only
//! 19.0 (verify clean) — Asserts that the committed compat fixture produced
//! by `cargo run --bin sanitize_capture` contains no HTTP records, a zeroed
//! AES manifest key, and no athlete IDs below the synthetic-ID floor (10001).
//!
//! Returns early with an explanatory message when the fixture has not yet been
//! generated; once it exists this test must always pass.

use prost::Message as _;
use zwift_proto::ServerToClient;
use zwift_relay::capture::{CaptureItem, CaptureReader, TransportKind};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/compat/fixtures/server_to_client/recorded_ride.bin",
);

const SYNTHETIC_ID_FLOOR: i64 = 10001;

#[test]
fn recorded_ride_fixture_is_clean() {
    if !std::path::Path::new(FIXTURE).exists() {
        eprintln!(
            "SKIP: {} not yet generated.\n\
             Run: cargo run --bin sanitize_capture -- tmp/output.cap {}",
            FIXTURE, FIXTURE,
        );
        return;
    }

    let mut reader = CaptureReader::open(FIXTURE).expect("open fixture");
    let mut found_manifest = false;

    while let Some(item) = reader.next_item() {
        match item.expect("read fixture record") {
            CaptureItem::Manifest(m) => {
                found_manifest = true;
                assert_eq!(
                    m.aes_key,
                    [0u8; 16],
                    "manifest AES key must be zeroed in the committed fixture",
                );
            }
            CaptureItem::Frame(record) => {
                assert_ne!(
                    record.transport,
                    TransportKind::Http,
                    "HTTP records must be stripped (found one in committed fixture)",
                );

                // Decode each relay frame and check athlete IDs.
                if let Ok(stc) = ServerToClient::decode(record.payload.as_slice()) {
                    if let Some(pid) = stc.player_id {
                        assert!(
                            pid >= SYNTHETIC_ID_FLOOR,
                            "ServerToClient.player_id {pid} is below synthetic floor {SYNTHETIC_ID_FLOOR}",
                        );
                    }
                    for ps in stc.states.iter().chain(stc.player_states.iter()) {
                        if let Some(id) = ps.id {
                            assert!(
                                id >= SYNTHETIC_ID_FLOOR,
                                "PlayerState.id {id} is below synthetic floor {SYNTHETIC_ID_FLOOR}",
                            );
                        }
                    }
                }
            }
        }
    }

    assert!(found_manifest, "fixture must contain at least one manifest record");
}
