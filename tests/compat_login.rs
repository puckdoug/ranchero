// SPDX-License-Identifier: AGPL-3.0-only
//! 19.4 — Login confirmation by replaying the sanitised capture.
//!
//! Spec §7.11 item 3, offline.  Replays `compat/fixtures/server_to_client/
//! recorded_ride.bin` through `CaptureReader` and asserts:
//!
//! 1. At least one inbound TCP frame decodes successfully as `ServerToClient`.
//! 2. At least one inbound UDP frame is present and also decodes as
//!    `ServerToClient`.
//!
//! These are the two artifacts that a real login produces: a `ServerToClient`
//! on the TCP channel followed by UDP data once the session is established.
//! The fixture was captured from a real login; the decode confirms the full
//! sanitise-decrypt-recode pipeline preserved a valid protobuf stream.
//!
//! The literal "within 5 s of establish()" timing from spec §7.11 item 3 is
//! a live-network property and is intentionally not asserted here.  The
//! recorded UDP data begins ≈121 s into the capture (the rider started later);
//! an offline replay cannot reproduce the 5 s window.  See also
//! `compat/README.md`.
//!
//! See docs/planning/STEP-19-compatibility-tests.md, item 19.4.

use prost::Message as _;
use zwift_proto::ServerToClient;
use zwift_relay::capture::{CaptureItem, CaptureReader, TransportKind};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/compat/fixtures/server_to_client/recorded_ride.bin",
);

#[test]
fn recorded_ride_has_decodable_tcp_and_udp_frames() {
    let mut reader = CaptureReader::open(FIXTURE).expect("open recorded_ride.bin");

    let mut tcp_ok = false;
    let mut udp_ok = false;

    while let Some(item) = reader.next_item() {
        match item.expect("read capture record") {
            CaptureItem::Manifest(_) => {}
            CaptureItem::Frame(record) => {
                match record.transport {
                    TransportKind::Tcp if !tcp_ok => {
                        ServerToClient::decode(record.payload.as_slice())
                            .expect("first TCP frame must decode as ServerToClient");
                        tcp_ok = true;
                    }
                    TransportKind::Udp if !udp_ok => {
                        ServerToClient::decode(record.payload.as_slice())
                            .expect("first UDP frame must decode as ServerToClient");
                        udp_ok = true;
                    }
                    _ => {}
                }
                if tcp_ok && udp_ok {
                    break;
                }
            }
        }
    }

    assert!(tcp_ok, "fixture must contain at least one inbound TCP ServerToClient frame");
    assert!(udp_ok, "fixture must contain at least one inbound UDP ServerToClient frame");
}
