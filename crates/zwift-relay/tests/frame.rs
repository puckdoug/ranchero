// SPDX-License-Identifier: AGPL-3.0-only
//
// Plaintext envelopes (TCP/UDP) and TCP frame wrapping. The
// `*_round_trip_with_real_proto` tests stitch the entire codec
// pipeline together against a real `ClientToServer` proto so the
// envelope shape and AES-GCM AAD wiring are exercised end-to-end.

use prost::Message;
use zwift_relay::{
    CodecError, ChannelType, DeviceType, Header, HeaderFlags, RelayIv, decode_header, decrypt,
    encrypt, frame_tcp, next_tcp_frame, parse_tcp_plaintext, parse_udp_plaintext, tcp_plaintext,
    udp_plaintext,
};
use zwift_proto::ClientToServer;

const KEY: [u8; 16] = [
    0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7,
    0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF,
];

// --- TCP plaintext envelope ----------------------------------------

#[test]
fn tcp_plaintext_hello_byte_is_zero_for_hello_one_for_steady() {
    // Per zwift.mjs:1295-1297: `[u8 version=2][u8 hello?0:1][proto bytes]`.
    let hello = tcp_plaintext(b"abc", true);
    assert_eq!(&hello[0..2], &[2, 0], "hello byte must be 0 for hello=true");
    assert_eq!(&hello[2..], b"abc");

    let steady = tcp_plaintext(b"abc", false);
    assert_eq!(&steady[0..2], &[2, 1], "hello byte must be 1 for hello=false");
    assert_eq!(&steady[2..], b"abc");
}

#[test]
fn tcp_plaintext_round_trip() {
    let bytes = tcp_plaintext(b"\x10\x20\x30", true);
    let parsed = parse_tcp_plaintext(&bytes).expect("parse");
    assert_eq!(parsed.version, 2);
    assert!(parsed.hello, "hello byte 0x00 should parse as hello=true");
    assert_eq!(parsed.proto_bytes, b"\x10\x20\x30");
}

#[test]
fn tcp_plaintext_rejects_bad_version() {
    let bad = [0xFF, 0x00, 0xDE, 0xAD];
    let err = parse_tcp_plaintext(&bad).expect_err("version byte must match TCP_VERSION");
    assert!(matches!(err, CodecError::BadVersion { got: 0xFF }));
}

// --- UDP plaintext envelope ----------------------------------------

#[test]
fn udp_plaintext_version_byte() {
    // Per zwift.mjs:1437-1440: `[u8 version=1][proto bytes]`.
    // See STEP-08 "Open verification points" §1 — sauce sends a
    // version byte even though the spec says UDP plaintext is just
    // the proto bytes. We follow the code.
    let bytes = udp_plaintext(b"\x10\x20\x30");
    assert_eq!(bytes[0], 1, "UDP plaintext must start with version byte 1");
    assert_eq!(&bytes[1..], b"\x10\x20\x30");
}

#[test]
fn udp_plaintext_round_trip() {
    let bytes = udp_plaintext(b"\x10\x20\x30");
    let parsed = parse_udp_plaintext(&bytes).expect("parse");
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.proto_bytes, b"\x10\x20\x30");
}

#[test]
fn udp_plaintext_rejects_bad_version() {
    let bad = [0xFF, 0xDE, 0xAD];
    let err = parse_udp_plaintext(&bad).expect_err("version byte must match UDP_VERSION");
    assert!(matches!(err, CodecError::BadVersion { got: 0xFF }));
}

// --- TCP framing (BE u16 size prefix) ------------------------------

#[test]
fn tcp_frame_size_prefix_is_be_u16() {
    let header = [0u8; 5];
    let cipher = [0u8; 100];
    let frame = frame_tcp(&header, &cipher);
    // 5 + 100 = 105 = 0x69.
    assert_eq!(&frame[0..2], &[0x00, 0x69]);
    assert_eq!(&frame[2..2 + 5], &header[..]);
    assert_eq!(&frame[2 + 5..], &cipher[..]);
    assert_eq!(frame.len(), 2 + 5 + 100);
}

#[test]
fn tcp_next_frame_returns_none_on_short_buffer() {
    // Single byte of input — not even the 2-byte size prefix is
    // complete. Demuxer must report "need more bytes," not error.
    let buf = [0x42u8];
    let result = next_tcp_frame(&buf).expect("not an error");
    assert!(result.is_none(), "should report incomplete prefix as None");
}

#[test]
fn tcp_next_frame_returns_none_on_partial_payload() {
    // Size says 5, but we only have 3 payload bytes. Still incomplete.
    let buf = [0x00, 0x05, 0xAA, 0xBB, 0xCC];
    let result = next_tcp_frame(&buf).expect("not an error");
    assert!(result.is_none(), "partial payload should report None");
}

#[test]
fn tcp_next_frame_handles_back_to_back_frames() {
    // Concat of two frames: [size1=3][p1=AA,BB,CC][size2=2][p2=DD,EE]
    // First call returns the first frame and consumed=5; the caller
    // would slice [consumed..] and call again to get the second.
    let buf = [0x00, 0x03, 0xAA, 0xBB, 0xCC, 0x00, 0x02, 0xDD, 0xEE];

    let (payload1, consumed1) = next_tcp_frame(&buf).expect("ok").expect("some");
    assert_eq!(payload1, &[0xAA, 0xBB, 0xCC]);
    assert_eq!(consumed1, 5, "2-byte prefix + 3-byte payload");

    let (payload2, consumed2) = next_tcp_frame(&buf[consumed1..]).expect("ok").expect("some");
    assert_eq!(payload2, &[0xDD, 0xEE]);
    assert_eq!(consumed2, 4);
}

// --- end-to-end round-trips with a real proto -----------------------

fn iv_for(channel: ChannelType, seqno: u32) -> [u8; 12] {
    RelayIv {
        device: DeviceType::Relay,
        channel,
        conn_id: 0x0042,
        seqno,
    }
    .to_bytes()
}

#[test]
fn tcp_round_trip_with_real_proto() {
    // Build a real ClientToServer with a non-default field so the
    // proto bytes aren't trivially empty. Encode → wrap → encrypt
    // → frame → unframe → decrypt → unwrap → decode, assert equality.
    let original = ClientToServer {
        seqno: Some(7),
        ..Default::default()
    };
    let proto_bytes = original.encode_to_vec();

    // ── send side ──
    let plaintext = tcp_plaintext(&proto_bytes, /* hello */ true);
    let header = Header {
        flags: HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
        relay_id: Some(0xDEAD_BEEF),
        conn_id: Some(0x0042),
        seqno: Some(0),
    };
    let header_bytes = header.encode();
    let send_iv = iv_for(ChannelType::TcpClient, 0);
    let ciphertext = encrypt(&KEY, &send_iv, &header_bytes, &plaintext);
    let wire = frame_tcp(&header_bytes, &ciphertext);

    // ── recv side ──
    let (frame_payload, consumed) = next_tcp_frame(&wire).expect("ok").expect("complete frame");
    assert_eq!(consumed, wire.len());
    let parsed_header = decode_header(frame_payload).expect("decode header");
    assert_eq!(parsed_header.header, header);
    let aad = &frame_payload[..parsed_header.consumed];
    let cipher_body = &frame_payload[parsed_header.consumed..];
    // Receive side rebuilds the IV from the relayed conn_id/seqno;
    // for the round-trip the values match the send IV by construction.
    let recv_iv = iv_for(ChannelType::TcpClient, 0);
    let recovered_plaintext = decrypt(&KEY, &recv_iv, aad, cipher_body).expect("decrypt");
    let recovered_envelope = parse_tcp_plaintext(&recovered_plaintext).expect("parse plaintext");
    assert!(recovered_envelope.hello);
    let recovered_proto =
        ClientToServer::decode(recovered_envelope.proto_bytes).expect("proto decode");
    assert_eq!(recovered_proto, original);
}

#[test]
fn udp_round_trip_with_real_proto() {
    // Same as the TCP case but no length prefix and the 1-byte UDP
    // plaintext envelope. UDP also always carries the seqno per
    // STEP-08 open verification points §2 (channel-layer policy);
    // here we just include it manually in the Header.
    let original = ClientToServer {
        seqno: Some(11),
        ..Default::default()
    };
    let proto_bytes = original.encode_to_vec();

    let plaintext = udp_plaintext(&proto_bytes);
    let header = Header {
        flags: HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
        relay_id: Some(0xDEAD_BEEF),
        conn_id: Some(0x0042),
        seqno: Some(0),
    };
    let header_bytes = header.encode();
    let send_iv = iv_for(ChannelType::UdpClient, 0);
    let ciphertext = encrypt(&KEY, &send_iv, &header_bytes, &plaintext);

    // UDP wire: just header || ciphertext, no length prefix.
    let mut wire = Vec::with_capacity(header_bytes.len() + ciphertext.len());
    wire.extend_from_slice(&header_bytes);
    wire.extend_from_slice(&ciphertext);

    let parsed_header = decode_header(&wire).expect("decode header");
    assert_eq!(parsed_header.header, header);
    let aad = &wire[..parsed_header.consumed];
    let cipher_body = &wire[parsed_header.consumed..];
    let recv_iv = iv_for(ChannelType::UdpClient, 0);
    let recovered_plaintext = decrypt(&KEY, &recv_iv, aad, cipher_body).expect("decrypt");
    let recovered_envelope = parse_udp_plaintext(&recovered_plaintext).expect("parse plaintext");
    assert_eq!(recovered_envelope.version, 1);
    let recovered_proto =
        ClientToServer::decode(recovered_envelope.proto_bytes).expect("proto decode");
    assert_eq!(recovered_proto, original);
}
